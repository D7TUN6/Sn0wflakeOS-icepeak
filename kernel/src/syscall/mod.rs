//! System call dispatch. User pointers are only accepted from the user half
//! of the address space.

use alloc::string::String;
use alloc::vec::Vec;

use abi::*;

use crate::arch::idt::InterruptFrame;
use crate::arch::timer;
use crate::fs::ramfs;
use crate::mm::paging;
use crate::{console, drivers, sched};

const MAX_TRANSFER: u64 = 4 * 1024 * 1024;

fn user_bytes<'a>(ptr: u64, len: u64) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if len > MAX_TRANSFER || ptr < paging::USER_BASE {
        return None;
    }
    if ptr.checked_add(len)? >= paging::USER_BASE + 0x0000_0080_0000_0000 {
        return None;
    }
    if !paging::user_range_ok(paging::read_cr3(), ptr, len) {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) })
}

fn user_bytes_mut<'a>(ptr: u64, len: u64) -> Option<&'a mut [u8]> {
    if len == 0 {
        return Some(&mut []);
    }
    if len > MAX_TRANSFER || ptr < paging::USER_BASE {
        return None;
    }
    if ptr.checked_add(len)? >= paging::USER_BASE + 0x0000_0080_0000_0000 {
        return None;
    }
    if !paging::user_range_ok(paging::read_cr3(), ptr, len) {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len as usize) })
}

fn user_str<'a>(ptr: u64, len: u64) -> Option<&'a str> {
    if len > 1024 {
        return None;
    }
    core::str::from_utf8(user_bytes(ptr, len)?).ok()
}

/// Resolve a possibly-relative path against the current working directory and
/// collapse `.` and `..`.
fn resolve(path: &str) -> String {
    let mut components: Vec<String> = Vec::new();
    if !path.starts_with('/') {
        for part in sched::current_cwd().split('/') {
            if !part.is_empty() && part != "." {
                components.push(String::from(part));
            }
        }
    }
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            _ => components.push(String::from(part)),
        }
    }
    let mut out = String::new();
    for part in components {
        out.push('/');
        out.push_str(&part);
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

fn copy_out(ptr: u64, data: &[u8]) -> isize {
    let Some(dst) = user_bytes_mut(ptr, data.len() as u64) else {
        return EFAULT;
    };
    dst.copy_from_slice(data);
    data.len() as isize
}

fn parent_path(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(pos) => String::from(&path[..pos]),
    }
}

fn allowed(info: &ramfs::Info, need: u16) -> bool {
    let uid = sched::current_uid();
    if uid == 0 {
        return true;
    }
    let gid = sched::current_gid();
    let bits = if uid == info.uid {
        (info.mode >> 6) & 7
    } else if gid == info.gid {
        (info.mode >> 3) & 7
    } else {
        info.mode & 7
    };
    bits & need == need
}

/// Check permission on an existing node: 4 = read, 2 = write, 1 = execute.
fn check(path: &str, need: u16) -> Result<(), isize> {
    match ramfs::stat(path) {
        None => Err(ENOENT),
        Some(info) if allowed(&info, need) => Ok(()),
        Some(_) => Err(EACCES),
    }
}

/// Check permission to create/remove an entry inside `path`'s parent.
fn check_parent(path: &str, need: u16) -> Result<(), isize> {
    check(&parent_path(path), need)
}

fn is_root() -> bool {
    sched::current_uid() == 0
}

/// Render the compact `kind size mode uid gid` record consumed by userland.
fn stat_text(info: &ramfs::Info, dst: &mut [u8]) -> isize {
    use core::fmt::Write;
    let mut text = String::new();
    let kind = if info.is_dir {
        'd'
    } else if info.is_link {
        'l'
    } else {
        'f'
    };
    let _ = write!(
        text,
        "{} {} {:o} {} {}\n",
        kind, info.size, info.mode, info.uid, info.gid
    );
    let n = text.len().min(dst.len());
    dst[..n].copy_from_slice(&text.as_bytes()[..n]);
    n as isize
}

fn sync_sleep(ms: u64) {
    let deadline = timer::now_ns().saturating_add(ms.max(1).saturating_mul(1_000_000));
    sched::sleep_until(sched::WaitKind::Until(deadline), || timer::now_ns() >= deadline);
}

fn blocking_read(ptr: u64, len: u64) -> isize {
    let Some(buf) = user_bytes_mut(ptr, len) else {
        return EFAULT;
    };
    loop {
        let n = drivers::keyboard::read_bytes(buf);
        if n > 0 {
            return n as isize;
        }
        sched::sleep_until(sched::WaitKind::Console, || {
            // Poll serial input as well as the keyboard IRQ queue.
            drivers::serial::poll();
            !drivers::keyboard::is_empty()
        });
    }
}

fn console_write(fd: u64, data: &[u8]) -> isize {
    if fd == STDERR || fd == STDOUT {
        console::write_bytes(data);
        return data.len() as isize;
    }
    EBADF
}

/// Stream `data` to a descriptor.
fn fd_write(fd: u64, data: &[u8]) -> isize {
    match sched::fd_desc(fd) {
        Some(crate::fs::fd::Desc::File {
            path,
            offset,
            flags,
        }) => {
            // `O_APPEND` re-reads the size so several writers can share a file.
            let pos = if flags & O_APPEND != 0 {
                ramfs::size(&path).unwrap_or(0)
            } else {
                offset
            };
            if !ramfs::write_at(&path, pos, data) {
                return EIO;
            }
            sched::fd_set_offset(fd, pos + data.len());
            data.len() as isize
        }
        Some(crate::fs::fd::Desc::Console { writable: true, .. }) => console_write(fd, data),
        Some(crate::fs::fd::Desc::Pipe { idx, write: true }) => {
            crate::fs::pipe::write(idx, data)
        }
        _ => EBADF,
    }
}

/// Fill `buf` from a descriptor, advancing its cursor.
fn fd_read(fd: u64, buf: &mut [u8]) -> isize {
    match sched::fd_desc(fd) {
        Some(crate::fs::fd::Desc::File { path, offset, .. }) => {
            // `/dev/zero` is an endless source, not a 4 KiB file.
            if matches!(ramfs::dynamic(&path), Some(ramfs::Dynamic::Zero)) {
                for byte in buf.iter_mut() {
                    *byte = 0;
                }
                return buf.len() as isize;
            }
            let Some(data) = ramfs::read(&path) else {
                return ENOENT;
            };
            if offset >= data.len() {
                return 0;
            }
            let n = (data.len() - offset).min(buf.len());
            buf[..n].copy_from_slice(&data[offset..offset + n]);
            sched::fd_set_offset(fd, offset + n);
            n as isize
        }
        Some(crate::fs::fd::Desc::Console { readable: true, .. }) => {
            blocking_read(buf.as_mut_ptr() as u64, buf.len() as u64)
        }
        Some(crate::fs::fd::Desc::Pipe { idx, write: false }) => crate::fs::pipe::read(idx, buf),
        _ => EBADF,
    }
}

fn fd_seek(fd: u64, offset: i64, whence: u64) -> isize {
    let Some(crate::fs::fd::Desc::File {
        path, offset: pos, ..
    }) = sched::fd_desc(fd)
    else {
        return ESPIPE;
    };
    let base = match whence {
        0 => 0i64,
        1 => pos as i64,
        2 => ramfs::size(&path).unwrap_or(0) as i64,
        _ => return EINVAL,
    };
    let target = base.saturating_add(offset);
    if target < 0 {
        return EINVAL;
    }
    if !sched::fd_set_offset(fd, target as usize) {
        return EBADF;
    }
    target as isize
}

/// Open `path` and return the new descriptor.
fn open_file(path: &str, flags: u64) -> isize {
    let path = resolve(path);
    let access = flags & O_ACCMODE;
    let readable = access == O_RDONLY || access == O_RDWR;
    let writable = access == O_WRONLY || access == O_RDWR;
    let existed = ramfs::exists(&path);

    if existed {
        if ramfs::is_dir(&path) {
            return EISDIR;
        }
        let need = match (readable, writable) {
            (true, true) => 6,
            (false, true) => 2,
            _ => 4,
        };
        if let Err(e) = check(&path, need) {
            return e;
        }
    } else {
        if flags & O_CREAT == 0 {
            return ENOENT;
        }
        if let Err(e) = check_parent(&path, 2) {
            return e;
        }
        if !ramfs::write(&path, &[]) {
            return EIO;
        }
        if !is_root() {
            ramfs::chown(&path, sched::current_uid(), sched::current_gid());
        }
    }

    if writable && flags & O_TRUNC != 0 && !ramfs::truncate(&path) {
        return EIO;
    }

    match sched::fd_install_file(path, flags) {
        Some(fd) => fd as isize,
        None => EMFILE,
    }
}

fn blocking_wait(pid: u64) -> isize {
    loop {
        match sched::poll_wait(pid) {
            Some(code) => {
                sched::reap(pid);
                return code as isize;
            }
            None => {
                if !sched::task_exists(pid) {
                    return ENOENT;
                }
                sched::sleep_until(sched::WaitKind::Child(pid), || sched::poll_wait(pid).is_some());
            }
        }
    }
}

pub fn dispatch(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let number = frame.rax;
    let a = frame.rdi;
    let b = frame.rsi;
    let c = frame.rdx;
    let d = frame.r10;

    let result: isize = match number {
        SYS_WRITE => {
            let Some(bytes) = user_bytes(b, c) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            fd_write(a, bytes)
        }
        SYS_EXIT => {
            return sched::exit_current(frame, a as i32);
        }
        SYS_READ => {
            let Some(buf) = user_bytes_mut(b, c) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            fd_read(a, buf)
        }
        SYS_OPEN => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            open_file(path, c)
        }
        SYS_CLOSE => {
            if sched::fd_close(a) {
                0
            } else {
                EBADF
            }
        }
        SYS_LSEEK => fd_seek(a, b as i64, c),
        SYS_DUP2 => {
            if sched::fd_dup2(a, b) {
                b as isize
            } else {
                EBADF
            }
        }
        SYS_PIPE => {
            let cloexec = b & O_CLOEXEC != 0;
            let Some(dst) = user_bytes_mut(a, 16) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let Some(idx) = crate::fs::pipe::create() else {
                frame.rax = EMFILE as u64;
                return frame;
            };
            let Some(rfd) = sched::fd_install_pipe(idx, false, cloexec) else {
                for pid in crate::fs::pipe::close(idx, false) {
                    sched::wake_pid(pid);
                }
                for pid in crate::fs::pipe::close(idx, true) {
                    sched::wake_pid(pid);
                }
                frame.rax = EMFILE as u64;
                return frame;
            };
            let Some(wfd) = sched::fd_install_pipe(idx, true, cloexec) else {
                sched::fd_close(rfd);
                for pid in crate::fs::pipe::close(idx, true) {
                    sched::wake_pid(pid);
                }
                frame.rax = EMFILE as u64;
                return frame;
            };
            dst[..8].copy_from_slice(&rfd.to_le_bytes());
            dst[8..16].copy_from_slice(&wfd.to_le_bytes());
            0
        }
        SYS_GETPID => sched::current_pid() as isize,
        SYS_YIELD => {
            sched::yield_now();
            0
        }
        SYS_SLEEP => {
            sync_sleep(a);
            0
        }
        SYS_CLEAR => {
            console::clear();
            0
        }
        SYS_UNAME => {
            let text = b"Sn0wflakeOS icepeak 0.1.0 #1-SMP x86_64";
            copy_out(a, text)
        }
        SYS_TIME => timer::ticks() as isize,
        SYS_READ_FILE => {
            let (Some(path), Some(dst)) = (user_str(a, b), user_bytes_mut(c, d)) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            if let Err(e) = check(&path, 4) {
                frame.rax = e as u64;
                return frame;
            }
            match ramfs::read(&path) {
                Some(data) => {
                    let n = data.len().min(dst.len());
                    dst[..n].copy_from_slice(&data[..n]);
                    n as isize
                }
                None => ENOENT,
            }
        }
        SYS_WRITE_FILE => {
            let (Some(path), Some(data)) = (user_str(a, b), user_bytes(c, d)) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            let existed = ramfs::exists(&path);
            let permit = if existed {
                check(&path, 2)
            } else {
                check_parent(&path, 2)
            };
            if let Err(e) = permit {
                frame.rax = e as u64;
                return frame;
            }
            if ramfs::write(&path, data) {
                if !existed && !is_root() {
                    ramfs::chown(&path, sched::current_uid(), sched::current_gid());
                }
                data.len() as isize
            } else {
                EIO
            }
        }
        SYS_EXISTS => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            match ramfs::size(&resolve(path)) {
                Some(n) => n as isize,
                None => ENOENT,
            }
        }
        SYS_MKDIR => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            if let Err(e) = check_parent(&path, 2) {
                frame.rax = e as u64;
                return frame;
            }
            if ramfs::mkdir(&path) {
                if !is_root() {
                    ramfs::chown(&path, sched::current_uid(), sched::current_gid());
                }
                0
            } else {
                EIO
            }
        }
        SYS_UNLINK => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            if let Err(e) = check_parent(&path, 2) {
                frame.rax = e as u64;
                return frame;
            }
            if ramfs::remove(&path) {
                0
            } else {
                ENOENT
            }
        }
        SYS_RMDIR => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            if let Err(e) = check_parent(&path, 2) {
                frame.rax = e as u64;
                return frame;
            }
            if ramfs::rmdir(&path) {
                0
            } else {
                ENOENT
            }
        }
        SYS_BEEP => {
            let freq = a as u32;
            if freq == 0 {
                drivers::speaker::off();
            } else {
                drivers::speaker::on(freq);
                if b > 0 {
                    sync_sleep(b);
                    drivers::speaker::off();
                }
            }
            0
        }
        SYS_SOUND => {
            if a == 0 {
                drivers::speaker::off();
            } else {
                drivers::speaker::on(a as u32);
            }
            0
        }
        SYS_SHUTDOWN => {
            crate::fs::journal::flush();
            if crate::acpi::poweroff() {
                0
            } else {
                EIO
            }
        }
        SYS_SYNC => {
            crate::fs::journal::flush();
            0
        }
        SYS_RENAME => {
            let (Some(from), Some(to)) = (user_str(a, b), user_str(c, d)) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let from = resolve(from);
            let to = resolve(to);
            if let Err(e) = check(&from, 2) {
                frame.rax = e as u64;
                return frame;
            }
            let permit = if ramfs::exists(&to) {
                check(&to, 2)
            } else {
                check_parent(&to, 2)
            };
            if let Err(e) = permit {
                frame.rax = e as u64;
                return frame;
            }
            match ramfs::read(&from) {
                Some(data) => {
                    let owned = data.to_vec();
                    if ramfs::write(&to, &owned) && ramfs::remove(&from) {
                        0
                    } else {
                        EIO
                    }
                }
                None => ENOENT,
            }
        }
        SYS_LIST => {
            let (Some(path), Some(dst)) = (user_str(a, b), user_bytes_mut(c, d)) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let mut entries = Vec::new();
            let resolved = resolve(path);
            if let Err(e) = check(&resolved, 4) {
                frame.rax = e as u64;
                return frame;
            }
            if !ramfs::list(&resolved, &mut entries) {
                frame.rax = ENOENT as u64;
                return frame;
            }
            let mut text = String::new();
            for e in entries {
                use core::fmt::Write;
                let kind = if e.is_dir {
                    'd'
                } else if e.is_link {
                    'l'
                } else {
                    'f'
                };
                let _ = write!(
                    text,
                    "{} {} {} {:o} {} {}\n",
                    kind, e.name, e.size, e.mode, e.uid, e.gid
                );
            }
            let n = text.len().min(dst.len());
            dst[..n].copy_from_slice(&text.as_bytes()[..n]);
            n as isize
        }
        SYS_CHDIR => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            if let Err(e) = check(&path, 1) {
                frame.rax = e as u64;
                return frame;
            }
            if ramfs::is_dir(&path) {
                sched::set_cwd(path);
                0
            } else {
                ENOENT
            }
        }
        SYS_GETCWD => {
            let Some(dst) = user_bytes_mut(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let cwd = sched::current_cwd();
            let n = cwd.len().min(dst.len());
            dst[..n].copy_from_slice(&cwd.as_bytes()[..n]);
            n as isize
        }
        SYS_EXEC => {
            let (Some(path), Some(argv_ptr)) = (user_str(a, b), Some(d)) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            if let Err(e) = check(&path, 1) {
                frame.rax = e as u64;
                return frame;
            }
            let mut owned: Vec<String> = Vec::new();
            if c > 32 {
                frame.rax = EINVAL as u64;
                return frame;
            }
            for i in 0..c {
                let Some(slot) = argv_ptr.checked_add(i * 8) else {
                    frame.rax = EFAULT as u64;
                    return frame;
                };
                let Some(bytes) = user_bytes(slot, 8) else {
                    frame.rax = EFAULT as u64;
                    return frame;
                };
                let ptr = u64::from_le_bytes(bytes.try_into().unwrap());
                if ptr == 0 {
                    break;
                }
                let mut len = 0u64;
                loop {
                    let Some(at) = ptr.checked_add(len) else {
                        frame.rax = EFAULT as u64;
                        return frame;
                    };
                    let Some(b) = user_bytes(at, 1) else {
                        frame.rax = EFAULT as u64;
                        return frame;
                    };
                    if b[0] == 0 || len >= 4096 {
                        break;
                    }
                    len += 1;
                }
                let Some(s) = user_str(ptr, len) else {
                    frame.rax = EFAULT as u64;
                    return frame;
                };
                owned.push(String::from(s));
            }
            let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
            let cwd = sched::current_cwd();
            match sched::spawn_with_args(&path, sched::current_pid(), &refs, cwd) {
                Some(pid) => pid as isize,
                None => ENOENT,
            }
        }
        SYS_WAIT => blocking_wait(a),
        SYS_PS => {
            let Some(dst) = user_bytes_mut(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let text = sched::task_list_text();
            let n = text.len().min(dst.len());
            dst[..n].copy_from_slice(&text.as_bytes()[..n]);
            n as isize
        }
        SYS_STAT => {
            let (Some(path), Some(dst)) = (user_str(a, b), user_bytes_mut(c, d)) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let Some(info) = ramfs::stat(&resolve(path)) else {
                frame.rax = ENOENT as u64;
                return frame;
            };
            stat_text(&info, dst)
        }
        SYS_LSTAT => {
            let (Some(path), Some(dst)) = (user_str(a, b), user_bytes_mut(c, d)) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let Some(info) = ramfs::lstat(&resolve(path)) else {
                frame.rax = ENOENT as u64;
                return frame;
            };
            stat_text(&info, dst)
        }
        SYS_CHMOD => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            let Some(info) = ramfs::stat(&path) else {
                frame.rax = ENOENT as u64;
                return frame;
            };
            if !is_root() && sched::current_uid() != info.uid {
                frame.rax = EPERM as u64;
                return frame;
            }
            if ramfs::chmod(&path, c as u16) {
                0
            } else {
                ENOENT
            }
        }
        SYS_CHOWN => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            if !is_root() {
                frame.rax = EPERM as u64;
                return frame;
            }
            if ramfs::chown(&resolve(path), c as u32, d as u32) {
                0
            } else {
                ENOENT
            }
        }
        SYS_SYMLINK => {
            let Some(target) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let Some(path) = user_str(c, d) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let path = resolve(path);
            if let Err(e) = check_parent(&path, 2) {
                frame.rax = e as u64;
                return frame;
            }
            if ramfs::symlink(target, &path) {
                0
            } else {
                EEXIST
            }
        }
        SYS_READLINK => {
            let Some(path) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let Some(dst) = user_bytes_mut(c, d) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let Some(target) = ramfs::readlink(&resolve(path)) else {
                frame.rax = EINVAL as u64;
                return frame;
            };
            let n = target.len().min(dst.len());
            dst[..n].copy_from_slice(&target.as_bytes()[..n]);
            n as isize
        }
        SYS_GETUID => sched::current_uid() as isize,
        SYS_GETGID => sched::current_gid() as isize,
        SYS_SETUID => {
            if sched::set_current_uid(a as u32, b as u32) {
                0
            } else {
                EPERM
            }
        }
        SYS_MEMINFO => {
            let Some(dst) = user_bytes_mut(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let (total, used, free, frame_used, frame_total) = crate::mm::mem_stats();
            let (tasks, running, cpu_ns, cpus) = sched::task_counts();
            use core::fmt::Write;
            let mut text = String::new();
            let _ = write!(
                text,
                "mem total {}\nmem used {}\nmem free {}\nframes used {}\nframes total {}\n\
                 tasks total {}\ntasks runnable {}\ncpu ns {}\ncpu ticks {}\ncpus online {}\n\
                 governor {}\ncpufreq driver {}\nclock {}\n",
                total,
                used,
                free,
                frame_used,
                frame_total,
                tasks,
                running,
                cpu_ns,
                cpu_ns / 1_000_000,
                cpus,
                crate::arch::cpufreq::governor(),
                crate::arch::cpufreq::driver(),
                timer::clock_name()
            );
            let n = text.len().min(dst.len());
            dst[..n].copy_from_slice(&text.as_bytes()[..n]);
            n as isize
        }
        SYS_TASKSTATS => {
            let Some(dst) = user_bytes_mut(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let text = sched::task_stats_text();
            let n = text.len().min(dst.len());
            dst[..n].copy_from_slice(&text.as_bytes()[..n]);
            n as isize
        }
        SYS_FSSTAT => {
            let Some(dst) = user_bytes_mut(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            use core::fmt::Write;
            let mut text = String::new();
            if let Some((files, dirs, bytes)) = ramfs::usage("/") {
                let _ = write!(
                    text,
                    "fs files {}\nfs dirs {}\nfs bytes {}\n",
                    files, dirs, bytes
                );
            }
            let n = text.len().min(dst.len());
            dst[..n].copy_from_slice(&text.as_bytes()[..n]);
            n as isize
        }
        SYS_SCHED_SET => {
            if sched::set_policy(a, b as u8, c as i32) {
                0
            } else {
                EPERM
            }
        }
        SYS_SCHED_GET => {
            let Some(dst) = user_bytes_mut(b, c) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let Some((policy, priority)) = sched::get_policy(a) else {
                frame.rax = ENOENT as u64;
                return frame;
            };
            use core::fmt::Write;
            let mut text = String::new();
            let _ = write!(text, "{} {}\n", policy, priority);
            let n = text.len().min(dst.len());
            dst[..n].copy_from_slice(&text.as_bytes()[..n]);
            n as isize
        }
        SYS_CPUFREQ_GET => {
            let Some(dst) = user_bytes_mut(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            let text = crate::arch::cpufreq::info_text();
            let n = text.len().min(dst.len());
            dst[..n].copy_from_slice(&text.as_bytes()[..n]);
            n as isize
        }
        SYS_CPUFREQ_SET => {
            let Some(name) = user_str(a, b) else {
                frame.rax = EFAULT as u64;
                return frame;
            };
            if crate::arch::cpufreq::set_governor(name) {
                0
            } else {
                EINVAL
            }
        }
        _ => EINVAL,
    };

    frame.rax = result as u64;
    frame
}
