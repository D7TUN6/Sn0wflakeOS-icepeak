#![no_std]

use core::fmt::{self, Write};
use core::panic::PanicInfo;

pub use abi::*;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let _ = write_fmt(2, format_args!("user panic: {}\n", info));
    exit(70)
}

// Precompiled `core` still carries an unwind reference even when the binary is
// built with `panic=abort`; provide the personality symbol so the link closes.
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

pub unsafe fn syscall6(
    number: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
) -> isize {
    let result: isize;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as isize => result,
            in("rdi") a,
            in("rsi") b,
            in("rdx") c,
            in("r10") d,
            in("r8") e,
            in("r9") f,
            options(nostack),
        );
    }
    result
}

pub fn syscall(number: u64, a: u64, b: u64, c: u64) -> isize {
    unsafe { syscall6(number, a, b, c, 0, 0, 0) }
}

pub fn write(fd: u64, data: &[u8]) -> isize {
    syscall(SYS_WRITE, fd, data.as_ptr() as u64, data.len() as u64)
}

pub fn read(fd: u64, data: &mut [u8]) -> isize {
    syscall(SYS_READ, fd, data.as_mut_ptr() as u64, data.len() as u64)
}

pub fn open(path: &str, flags: u64) -> isize {
    syscall(SYS_OPEN, path.as_ptr() as u64, path.len() as u64, flags)
}

pub fn close(fd: u64) -> isize {
    syscall(SYS_CLOSE, fd, 0, 0)
}

pub fn lseek(fd: u64, offset: i64, whence: u64) -> isize {
    syscall(SYS_LSEEK, fd, offset as u64, whence)
}

pub fn dup2(old: u64, new: u64) -> isize {
    syscall(SYS_DUP2, old, new, 0)
}

/// Flush buffered filesystem writes to disk.
pub fn sync() -> isize {
    syscall(SYS_SYNC, 0, 0, 0)
}

/// Create a pipe, returning `(read_fd, write_fd)`.
pub fn pipe() -> Option<(u64, u64)> {
    pipe2(0)
}

/// Create a pipe; with `O_CLOEXEC` neither end survives into a child.
pub fn pipe2(flags: u64) -> Option<(u64, u64)> {
    let mut fds = [0u64; 2];
    if syscall(SYS_PIPE, fds.as_mut_ptr() as u64, flags, 0) < 0 {
        return None;
    }
    Some((fds[0], fds[1]))
}

pub fn exit(code: i32) -> ! {
    syscall(SYS_EXIT, code as u64, 0, 0);
    loop {}
}

pub fn getpid() -> u64 {
    syscall(SYS_GETPID, 0, 0, 0) as u64
}

pub fn yield_now() {
    syscall(SYS_YIELD, 0, 0, 0);
}

pub fn sleep_ms(ms: u64) {
    syscall(SYS_SLEEP, ms, 0, 0);
}

pub fn clear() {
    syscall(SYS_CLEAR, 0, 0, 0);
}

pub fn uname(buf: &mut [u8]) -> isize {
    syscall(SYS_UNAME, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
}

pub fn time_ms() -> u64 {
    syscall(SYS_TIME, 0, 0, 0) as u64
}

pub fn read_file(path: &str, buf: &mut [u8]) -> isize {
    unsafe { syscall6(
        SYS_READ_FILE,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
        0,
        0,
    ) }
}

pub fn write_file(path: &str, data: &[u8]) -> isize {
    unsafe { syscall6(
        SYS_WRITE_FILE,
        path.as_ptr() as u64,
        path.len() as u64,
        data.as_ptr() as u64,
        data.len() as u64,
        0,
        0,
    ) }
}

pub fn exists(path: &str) -> isize {
    syscall(SYS_EXISTS, path.as_ptr() as u64, path.len() as u64, 0)
}

pub fn list(path: &str, buf: &mut [u8]) -> isize {
    unsafe { syscall6(
        SYS_LIST,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
        0,
        0,
    ) }
}

pub fn mkdir(path: &str) -> isize {
    syscall(SYS_MKDIR, path.as_ptr() as u64, path.len() as u64, 0)
}

pub fn unlink(path: &str) -> isize {
    syscall(SYS_UNLINK, path.as_ptr() as u64, path.len() as u64, 0)
}

pub fn rmdir(path: &str) -> isize {
    syscall(SYS_RMDIR, path.as_ptr() as u64, path.len() as u64, 0)
}

pub fn beep(freq: u64, ms: u64) -> isize {
    syscall(SYS_BEEP, freq, ms, 0)
}

pub fn sound(freq: u64) -> isize {
    syscall(SYS_SOUND, freq, 0, 0)
}

pub fn shutdown() -> isize {
    syscall(SYS_SHUTDOWN, 0, 0, 0)
}

pub fn rename(from: &str, to: &str) -> isize {
    unsafe { syscall6(
        SYS_RENAME,
        from.as_ptr() as u64,
        from.len() as u64,
        to.as_ptr() as u64,
        to.len() as u64,
        0,
        0,
    ) }
}

pub fn chdir(path: &str) -> isize {
    syscall(SYS_CHDIR, path.as_ptr() as u64, path.len() as u64, 0)
}

pub fn getcwd(buf: &mut [u8]) -> isize {
    syscall(SYS_GETCWD, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
}

pub fn ps(buf: &mut [u8]) -> isize {
    syscall(SYS_PS, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
}

pub fn stat(path: &str, buf: &mut [u8]) -> isize {
    unsafe { syscall6(
        SYS_STAT,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
        0,
        0,
    ) }
}

pub fn lstat(path: &str, buf: &mut [u8]) -> isize {
    unsafe { syscall6(
        SYS_LSTAT,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
        0,
        0,
    ) }
}

pub fn chmod(path: &str, mode: u16) -> isize {
    syscall(SYS_CHMOD, path.as_ptr() as u64, path.len() as u64, mode as u64)
}

pub fn chown(path: &str, uid: u32, gid: u32) -> isize {
    unsafe { syscall6(
        SYS_CHOWN,
        path.as_ptr() as u64,
        path.len() as u64,
        uid as u64,
        gid as u64,
        0,
        0,
    ) }
}

pub fn getuid() -> u32 {
    syscall(SYS_GETUID, 0, 0, 0) as u32
}

pub fn getgid() -> u32 {
    syscall(SYS_GETGID, 0, 0, 0) as u32
}

pub fn setuid(uid: u32, gid: u32) -> isize {
    syscall(SYS_SETUID, uid as u64, gid as u64, 0)
}

pub fn meminfo(buf: &mut [u8]) -> isize {
    syscall(SYS_MEMINFO, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
}

pub fn taskstats(buf: &mut [u8]) -> isize {
    syscall(SYS_TASKSTATS, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
}

pub fn fsstat(buf: &mut [u8]) -> isize {
    syscall(SYS_FSSTAT, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
}

pub fn sched_set(pid: u64, policy: u8, priority: i32) -> isize {
    syscall(SYS_SCHED_SET, pid, policy as u64, priority as u64)
}

pub fn sched_get(pid: u64, buf: &mut [u8]) -> isize {
    syscall(SYS_SCHED_GET, pid, buf.as_mut_ptr() as u64, buf.len() as u64)
}

pub fn cpufreq_get(buf: &mut [u8]) -> isize {
    syscall(SYS_CPUFREQ_GET, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
}

pub fn cpufreq_set(name: &str) -> isize {
    syscall(SYS_CPUFREQ_SET, name.as_ptr() as u64, name.len() as u64, 0)
}

pub fn symlink(target: &str, path: &str) -> isize {
    unsafe { syscall6(
        SYS_SYMLINK,
        target.as_ptr() as u64,
        target.len() as u64,
        path.as_ptr() as u64,
        path.len() as u64,
        0,
        0,
    ) }
}

pub fn readlink(path: &str, buf: &mut [u8]) -> isize {
    unsafe { syscall6(
        SYS_READLINK,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
        0,
        0,
    ) }
}

pub fn exec(path: &str, args: &[&str]) -> isize {
    let mut ptrs = [0u64; 32];
    let n = args.len().min(32);
    for (i, arg) in args[..n].iter().enumerate() {
        ptrs[i] = arg.as_ptr() as u64;
    }
    unsafe {
        syscall6(
            SYS_EXEC,
            path.as_ptr() as u64,
            path.len() as u64,
            n as u64,
            ptrs.as_ptr() as u64,
            0,
            0,
        )
    }
}

pub fn wait(pid: u64) -> i32 {
    syscall(SYS_WAIT, pid, 0, 0) as i32
}

pub fn print(text: &str) {
    let _ = write(1, text.as_bytes());
}

pub fn eprint(text: &str) {
    let _ = write(2, text.as_bytes());
}

pub fn write_fmt(fd: u64, args: fmt::Arguments) -> fmt::Result {
    struct W(u64);
    impl Write for W {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let _ = write(self.0, s.as_bytes());
            Ok(())
        }
    }
    let mut w = W(fd);
    w.write_fmt(args)
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let _ = $crate::write_fmt(1, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => { $crate::print("\n") };
    ($($arg:tt)*) => {
        {
            let _ = $crate::write_fmt(1, format_args!($($arg)*));
            $crate::print("\n");
        }
    };
}

#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {{
        let _ = $crate::write_fmt(2, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => {
        {
            let _ = $crate::write_fmt(2, format_args!($($arg)*));
            $crate::eprint("\n");
        }
    };
}

pub struct Args {
    ptr: *const *const u8,
    pub len: usize,
}

impl Args {
    pub unsafe fn new(argc: usize, argv: *const *const u8) -> Self {
        Self { ptr: argv, len: argc }
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        if index >= self.len {
            return None;
        }
        unsafe {
            let p = *self.ptr.add(index);
            if p.is_null() {
                return None;
            }
            let mut len = 0;
            while *p.add(len) != 0 && len < 4096 {
                len += 1;
            }
            core::str::from_utf8(core::slice::from_raw_parts(p, len)).ok()
        }
    }

    pub fn collect(&self) -> [&str; 32] {
        let mut out = [""; 32];
        let n = self.len.min(32);
        for i in 0..n {
            out[i] = self.get(i).unwrap_or("");
        }
        out
    }
}

/// Print a signed integer without allocating.
pub fn print_i64(mut n: i64) {
    let mut buf = [0u8; 24];
    let mut i = buf.len();
    let neg = n < 0;
    if n == 0 {
        print("0");
        return;
    }
    if neg {
        // Avoid overflow on i64::MIN by working with u64.
        let mut u = (n as i128).unsigned_abs() as u128;
        while u > 0 {
            i -= 1;
            buf[i] = b'0' + (u % 10) as u8;
            u /= 10;
        }
        print("-");
        let _ = write(1, &buf[i..]);
        return;
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let _ = write(1, &buf[i..]);
}

pub fn print_u64(mut n: u64) {
    if n == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 24];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let _ = write(1, &buf[i..]);
}

pub fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut n = 0u64;
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(n)
}

pub fn parse_octal(s: &str) -> Option<u16> {
    let s = s.strip_prefix("0o").unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    let mut n = 0u16;
    for &b in s.as_bytes() {
        if !(b'0'..=b'7').contains(&b) {
            return None;
        }
        n = n.checked_mul(8)?.checked_add((b - b'0') as u16)?;
    }
    Some(n)
}

/// Render `mode` as `drwxr-xr-x` (plus setuid/setgid/sticky markers).
pub fn mode_string(mode: u16, out: &mut [u8; 10]) -> &str {
    out[0] = if mode & 0o170000 == 0o120000 {
        b'l'
    } else if mode & 0o040000 != 0 {
        b'd'
    } else {
        b'-'
    };
    let bits = [(mode >> 6) & 7, (mode >> 3) & 7, mode & 7];
    let letters = [b'r', b'w', b'x'];
    for (i, b) in bits.iter().enumerate() {
        for (j, l) in letters.iter().enumerate() {
            out[1 + i * 3 + j] = if b & (4 >> j) != 0 { *l } else { b'-' };
        }
    }
    if mode & 0o4000 != 0 {
        out[3] = if out[3] == b'x' { b's' } else { b'S' };
    }
    if mode & 0o2000 != 0 {
        out[6] = if out[6] == b'x' { b's' } else { b'S' };
    }
    if mode & 0o1000 != 0 {
        out[9] = if out[9] == b'x' { b't' } else { b'T' };
    }
    core::str::from_utf8(&out[..10]).unwrap_or("----------")
}

/// Human readable size with one decimal place (1024-based).
pub fn human_size(n: u64, out: &mut [u8; 16]) -> &str {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    fn put(out: &mut [u8], mut len: usize, mut value: u64) -> usize {
        let mut digits = [0u8; 20];
        let mut d = digits.len();
        if value == 0 {
            d -= 1;
            digits[d] = b'0';
        }
        while value > 0 {
            d -= 1;
            digits[d] = b'0' + (value % 10) as u8;
            value /= 10;
        }
        for &b in &digits[d..] {
            if len < out.len() {
                out[len] = b;
                len += 1;
            }
        }
        len
    }

    let mut value = n;
    let mut unit = 0usize;
    let mut frac = 0u64;
    while value >= 1024 && unit + 1 < UNITS.len() {
        frac = value % 1024;
        value /= 1024;
        unit += 1;
    }
    let mut len = put(out, 0, value);
    if unit > 0 {
        out[len] = b'.';
        len += 1;
        out[len] = b'0' + (frac * 10 / 1024) as u8;
        len += 1;
    }
    for &b in UNITS[unit].as_bytes() {
        out[len] = b;
        len += 1;
    }
    core::str::from_utf8(&out[..len]).unwrap_or("?")
}

pub fn base_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        Some(pos) => &trimmed[pos + 1..],
        None => trimmed,
    }
}

pub fn dir_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        Some(0) => "/",
        Some(pos) => &trimmed[..pos],
        None => ".",
    }
}

/// Parse a `key value` statistics block into a u64 by key.
pub fn stat_field(text: &str, key: &str) -> Option<u64> {
    for line in text.lines() {
        let mut prev: Option<&str> = None;
        for word in line.split_whitespace() {
            if prev == Some(key) {
                if let Some(v) = parse_u64(word) {
                    return Some(v);
                }
            }
            prev = Some(word);
        }
    }
    None
}

/// Read a whole file into `buf` and return the number of bytes, or a negative
/// error code.
pub fn read_all(path: &str, buf: &mut [u8]) -> isize {
    read_file(path, buf)
}

static mut AUX_LIST: [u8; 8192] = [0; 8192];
static mut AUX_IO: [u8; 1048576] = [0; 1048576];

pub fn aux_list() -> &'static mut [u8] {
    unsafe { &mut *(&raw mut AUX_LIST) }
}

pub fn aux_io() -> &'static mut [u8] {
    unsafe { &mut *(&raw mut AUX_IO) }
}

/// Join `dir` and `name` into `out`, returning the used length.
pub fn path_join(out: &mut [u8; 512], dir: &str, name: &str) -> usize {
    let mut n = 0usize;
    for &b in dir.as_bytes() {
        if n + 2 >= out.len() {
            break;
        }
        out[n] = b;
        n += 1;
    }
    if n == 0 || out[n - 1] != b'/' {
        if n + 2 < out.len() {
            out[n] = b'/';
            n += 1;
        }
    }
    for &b in name.as_bytes() {
        if n + 1 >= out.len() {
            break;
        }
        out[n] = b;
        n += 1;
    }
    n
}

/// True when `path` can be listed as a directory.
pub fn is_directory(path: &str) -> bool {
    let buf = aux_list();
    list(path, buf) >= 0
}

pub fn copy_file(src: &str, dst: &str, preserve: bool) -> bool {
    let buf = aux_io();
    let n = read_file(src, buf);
    if n < 0 {
        return false;
    }
    let n = n as usize;
    if write_file(dst, &buf[..n]) < 0 {
        return false;
    }
    if preserve {
        let mut info = [0u8; 128];
        let m = stat(src, &mut info);
        if m > 0 {
            let text = core::str::from_utf8(&info[..m as usize]).unwrap_or("");
            let mut parts = text.split_whitespace();
            let _kind = parts.next();
            let _size = parts.next();
            if let Some(mode) = parts.next().and_then(parse_octal) {
                chmod(dst, mode & 0o7777);
            }
        }
    }
    true
}

/// Recursively copy `src` to `dst`. Directory listings are re-read after each
/// child so a deep recursion cannot clobber the shared listing buffer.
pub fn copy_tree(src: &str, dst: &str, verbose: bool, preserve: bool) -> bool {
    if !is_directory(src) {
        if !copy_file(src, dst, preserve) {
            return false;
        }
        if verbose {
            print!("'{}' -> '{}'\n", src, dst);
        }
        return true;
    }
    if mkdir(dst) < 0 && !is_directory(dst) {
        return false;
    }
    if verbose {
        print!("'{}' -> '{}'\n", src, dst);
    }
    let mut skip = 0usize;
    loop {
        let mut name = [0u8; 128];
        let mut name_len = 0usize;
        let mut child_dir = false;
        {
            let buf = aux_list();
            let n = list(src, buf);
            if n < 0 {
                return false;
            }
            let text = &buf[..n as usize];
            let mut index = 0usize;
            for line in text.split(|&b| b == b'\n') {
                if line.len() < 5 {
                    continue;
                }
                if index == skip {
                    let mut i = 2;
                    while i < line.len() && line[i] != b' ' {
                        i += 1;
                    }
                    name_len = (i - 2).min(name.len());
                    name[..name_len].copy_from_slice(&line[2..2 + name_len]);
                    child_dir = line[0] == b'd';
                    break;
                }
                index += 1;
            }
            if name_len == 0 {
                break;
            }
        }
        let mut from = [0u8; 512];
        let mut to = [0u8; 512];
        let fl = path_join(&mut from, src, core::str::from_utf8(&name[..name_len]).unwrap_or(""));
        let tl = path_join(&mut to, dst, core::str::from_utf8(&name[..name_len]).unwrap_or(""));
        let from_s = core::str::from_utf8(&from[..fl]).unwrap_or("/");
        let to_s = core::str::from_utf8(&to[..tl]).unwrap_or("/");
        if child_dir {
            if !copy_tree(from_s, to_s, verbose, preserve) {
                return false;
            }
        } else {
            if !copy_file(from_s, to_s, preserve) {
                return false;
            }
            if verbose {
                print!("'{}' -> '{}'\n", from_s, to_s);
            }
        }
        skip += 1;
    }
    if preserve {
        let mut info = [0u8; 128];
        let m = stat(src, &mut info);
        if m > 0 {
            let text = core::str::from_utf8(&info[..m as usize]).unwrap_or("");
            let mut parts = text.split_whitespace();
            let _kind = parts.next();
            let _size = parts.next();
            if let Some(mode) = parts.next().and_then(parse_octal) {
                chmod(dst, mode & 0o7777);
            }
        }
    }
    true
}

/// Remove a file or directory tree. The first entry is re-listed after each
/// removal for the same buffer-safety reason as `copy_tree`.
pub fn remove_tree(path: &str, verbose: bool) -> bool {
    if !is_directory(path) {
        if unlink(path) < 0 {
            return false;
        }
        if verbose {
            print!("removed '{}'\n", path);
        }
        return true;
    }
    loop {
        let mut name = [0u8; 128];
        let mut name_len = 0usize;
        {
            let buf = aux_list();
            let n = list(path, buf);
            if n < 0 {
                return false;
            }
            let text = &buf[..n as usize];
            for line in text.split(|&b| b == b'\n') {
                if line.len() < 5 {
                    continue;
                }
                let mut i = 2;
                while i < line.len() && line[i] != b' ' {
                    i += 1;
                }
                name_len = (i - 2).min(name.len());
                name[..name_len].copy_from_slice(&line[2..2 + name_len]);
                break;
            }
            if name_len == 0 {
                break;
            }
        }
        let mut child = [0u8; 512];
        let cl = path_join(&mut child, path, core::str::from_utf8(&name[..name_len]).unwrap_or(""));
        if !remove_tree(core::str::from_utf8(&child[..cl]).unwrap_or("/"), verbose) {
            return false;
        }
    }
    if rmdir(path) < 0 {
        return false;
    }
    if verbose {
        print!("removed directory '{}'\n", path);
    }
    true
}

/// Recursively measure `path`. Returns `(files, dirs, bytes)`.
pub fn tree_usage(path: &str) -> (usize, usize, usize) {
    if !is_directory(path) {
        let size = exists(path);
        return (1, 0, if size > 0 { size as usize } else { 0 });
    }
    let mut files = 0usize;
    let mut dirs = 1usize;
    let mut bytes = 0usize;
    let mut skip = 0usize;
    loop {
        let mut name = [0u8; 128];
        let mut name_len = 0usize;
        {
            let buf = aux_list();
            let n = list(path, buf);
            if n < 0 {
                break;
            }
            let text = &buf[..n as usize];
            let mut index = 0usize;
            for line in text.split(|&b| b == b'\n') {
                if line.len() < 5 {
                    continue;
                }
                if index == skip {
                    let mut i = 2;
                    while i < line.len() && line[i] != b' ' {
                        i += 1;
                    }
                    name_len = (i - 2).min(name.len());
                    name[..name_len].copy_from_slice(&line[2..2 + name_len]);
                    break;
                }
                index += 1;
            }
            if name_len == 0 {
                break;
            }
        }
        let mut child = [0u8; 512];
        let cl = path_join(&mut child, path, core::str::from_utf8(&name[..name_len]).unwrap_or(""));
        let (f, d, b) = tree_usage(core::str::from_utf8(&child[..cl]).unwrap_or("/"));
        files += f;
        dirs += d;
        bytes += b;
        skip += 1;
    }
    (files, dirs, bytes)
}

// Freestanding memory primitives; rustc may emit calls to these for slice
// copies in no_std binaries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *dest.add(i) = *src.add(i) };
        i += 1;
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dest as usize) < (src as usize) {
        let mut i = 0;
        while i < n {
            unsafe { *dest.add(i) = *src.add(i) };
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe { *dest.add(i) = *src.add(i) };
        }
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut u8, value: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *dest.add(i) = value as u8 };
        i += 1;
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let x = unsafe { *a.add(i) };
        let y = unsafe { *b.add(i) };
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const u8) -> usize {
    let mut n = 0;
    while unsafe { *s.add(n) } != 0 {
        n += 1;
    }
    n
}
