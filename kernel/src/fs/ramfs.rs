//! A small in-memory filesystem. Files are created from Multiboot modules at
//! boot time and can be created, read and removed at runtime.
//!
//! Every node carries POSIX-style metadata (mode/uid/gid). The syscall layer
//! enforces the permission bits; the filesystem itself only stores them.
//!
//! Symbolic links are resolved while walking a path (up to 16 hops), which is
//! what lets the Nix-style store be presented through the usual FHS paths.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const S_IFDIR: u16 = 0o040000;
pub const S_IFREG: u16 = 0o100000;
pub const S_IFLNK: u16 = 0o120000;

pub struct File {
    pub name: String,
    pub data: Vec<u8>,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    /// Pseudo-files (`/proc`, `/dev`) regenerate their contents on access
    /// instead of storing them.
    pub dynamic: Option<Dynamic>,
}

/// Content generators for the pseudo-filesystem. The files are ordinary
/// `File` nodes so path resolution, permissions and `ls` work unchanged; the
/// data is refreshed whenever the node is read or stat'ed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dynamic {
    Meminfo,
    Cpuinfo,
    Uptime,
    Version,
    Null,
    Zero,
}

pub struct Dir {
    pub name: String,
    pub entries: Vec<Entry>,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
}

pub struct Link {
    pub name: String,
    pub target: String,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
}

pub enum Entry {
    File(File),
    Dir(Dir),
    Link(Link),
}

/// A snapshot of one directory entry, used by `ls`, `stat` and `du`.
#[derive(Clone)]
pub struct Info {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: usize,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
}

impl Entry {
    pub fn name(&self) -> &str {
        match self {
            Entry::File(f) => &f.name,
            Entry::Dir(d) => &d.name,
            Entry::Link(l) => &l.name,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Entry::Dir(_))
    }

    pub fn is_link(&self) -> bool {
        matches!(self, Entry::Link(_))
    }

    pub fn mode(&self) -> u16 {
        match self {
            Entry::File(f) => f.mode,
            Entry::Dir(d) => d.mode,
            Entry::Link(l) => l.mode,
        }
    }

    pub fn uid(&self) -> u32 {
        match self {
            Entry::File(f) => f.uid,
            Entry::Dir(d) => d.uid,
            Entry::Link(l) => l.uid,
        }
    }

    pub fn gid(&self) -> u32 {
        match self {
            Entry::File(f) => f.gid,
            Entry::Dir(d) => d.gid,
            Entry::Link(l) => l.gid,
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Entry::File(f) => f.data.len(),
            Entry::Dir(d) => d.entries.len(),
            Entry::Link(l) => l.target.len(),
        }
    }

    pub fn info(&self) -> Info {
        Info {
            name: self.name().to_string(),
            is_dir: self.is_dir(),
            is_link: self.is_link(),
            size: self.size(),
            mode: self.mode(),
            uid: self.uid(),
            gid: self.gid(),
        }
    }
}

fn new_dir(name: &str) -> Dir {
    Dir {
        name: name.to_string(),
        entries: Vec::new(),
        mode: S_IFDIR | 0o755,
        uid: 0,
        gid: 0,
    }
}

fn new_file(name: &str, data: Vec<u8>, mode: u16) -> File {
    File {
        name: name.to_string(),
        data,
        mode: S_IFREG | mode,
        uid: 0,
        gid: 0,
        dynamic: None,
    }
}

fn new_dynamic_file(name: &str, kind: Dynamic, mode: u16) -> File {
    File {
        name: name.to_string(),
        data: Vec::new(),
        mode: S_IFREG | mode,
        uid: 0,
        gid: 0,
        dynamic: Some(kind),
    }
}

impl Dynamic {
    fn generate(self) -> Vec<u8> {
        use alloc::format;
        use core::fmt::Write;
        match self {
            Dynamic::Null => Vec::new(),
            Dynamic::Zero => alloc::vec![0u8; 4096],
            Dynamic::Version => {
                b"Sn0wflakeOS icepeak 0.1.0 #1-SMP x86_64\n".to_vec()
            }
            Dynamic::Uptime => {
                let ns = crate::arch::timer::now_ns();
                let secs = ns / 1_000_000_000;
                let centis = (ns % 1_000_000_000) / 10_000_000;
                format!("{}.{:02} {}.{:02}\n", secs, centis, secs, centis).into_bytes()
            }
            Dynamic::Meminfo => {
                let (heap_total, heap_used, _, frame_used, frame_total) =
                    crate::mm::mem_stats();
                let mut text = String::new();
                let _ = write!(
                    text,
                    "MemTotal:       {} kB\nMemFree:        {} kB\nMemUsed:        {} kB\n\
                     HeapTotal:      {} kB\nHeapUsed:       {} kB\n",
                    frame_total.saturating_mul(4),
                    frame_total.saturating_sub(frame_used).saturating_mul(4),
                    frame_used.saturating_mul(4),
                    heap_total / 1024,
                    heap_used / 1024
                );
                text.into_bytes()
            }
            Dynamic::Cpuinfo => {
                let vendor = crate::arch::cpuid::vendor();
                let vendor = core::str::from_utf8(&vendor).unwrap_or("unknown");
                let mhz = crate::arch::timer::tsc_hz() / 1_000_000;
                let mut text = String::new();
                let _ = write!(
                    text,
                    "vendor_id       : {}\ncpu MHz         : {}\ncpu cores       : {}\n",
                    vendor.trim(),
                    mhz,
                    crate::arch::cpu::online_count().max(1)
                );
                text.into_bytes()
            }
        }
    }
}

fn refresh(entry: &mut Entry) {
    if let Entry::File(f) = entry {
        if let Some(kind) = f.dynamic {
            f.data = kind.generate();
        }
    }
}

fn new_link(name: &str, target: &str) -> Link {
    Link {
        name: name.to_string(),
        target: target.to_string(),
        mode: S_IFLNK | 0o777,
        uid: 0,
        gid: 0,
    }
}

static mut ROOT: Option<Dir> = None;

fn root() -> &'static mut Dir {
    unsafe {
        let slot = &mut *(&raw mut ROOT);
        if slot.is_none() {
            *slot = Some(new_dir(""));
        }
        slot.as_mut().unwrap()
    }
}

fn root_ptr() -> *mut Dir {
    root() as *mut Dir
}

fn split(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .map(|p| p.to_string())
        .collect()
}

fn find_child_ptr(dir: *mut Dir, name: &str) -> Option<*mut Entry> {
    unsafe {
        (*dir)
            .entries
            .iter_mut()
            .find(|e| e.name() == name)
            .map(|e| e as *mut Entry)
    }
}

fn dir_of(entry: *mut Entry) -> Option<*mut Dir> {
    match unsafe { &mut *entry } {
        Entry::Dir(d) => Some(d as *mut Dir),
        _ => None,
    }
}

/// Walk `path`, resolving symbolic links. With `follow_final == false` the last
/// component is returned without being dereferenced (the `lstat` behaviour).
fn resolve(path: &str, follow_final: bool) -> Option<*mut Entry> {
    let mut parts = split(path);
    if parts.is_empty() {
        return None;
    }
    let mut cur = root_ptr();
    let mut parents: Vec<*mut Dir> = Vec::new();
    let mut hops = 0u32;
    let mut i = 0usize;
    while i < parts.len() {
        let part = parts[i].clone();
        if part == ".." {
            if let Some(p) = parents.pop() {
                cur = p;
            }
            i += 1;
            continue;
        }
        let child = find_child_ptr(cur, &part)?;
        let last = i + 1 == parts.len();
        let is_link = matches!(unsafe { &*child }, Entry::Link(_));
        if last && !(follow_final && is_link) {
            return Some(child);
        }
        match unsafe { &mut *child } {
            Entry::Dir(d) => {
                parents.push(cur);
                cur = d as *mut Dir;
                i += 1;
            }
            Entry::File(_) => return None,
            Entry::Link(l) => {
                hops += 1;
                if hops > 16 {
                    return None;
                }
                let target = l.target.clone();
                let mut next: Vec<String> = Vec::new();
                if target.starts_with('/') {
                    parents.clear();
                    cur = root_ptr();
                }
                next.extend(split(&target));
                next.extend_from_slice(&parts[i + 1..]);
                parts = next;
                i = 0;
            }
        }
    }
    None
}

fn ensure_dir_parts(mut dir: *mut Dir, parts: &[String]) -> Option<*mut Dir> {
    for name in parts {
        if name == ".." {
            return None;
        }
        let next = match find_child_ptr(dir, name) {
            Some(p) => dir_of(p)?,
            None => {
                unsafe { (*dir).entries.push(Entry::Dir(new_dir(name))) };
                dir_of(find_child_ptr(dir, name)?)?
            }
        };
        dir = next;
    }
    Some(dir)
}

/// Split a path into its parent directory and final component, resolving links
/// in the parent only. Used for create/remove operations.
fn parent_and_name(path: &str) -> Option<(*mut Dir, String)> {
    let parts = split(path);
    if parts.is_empty() {
        return None;
    }
    let name = parts[parts.len() - 1].clone();
    let parent = if parts.len() == 1 {
        root_ptr()
    } else {
        let parent_path = parts[..parts.len() - 1].join("/");
        dir_of(resolve(&parent_path, true)?)?
    };
    Some((parent, name))
}

pub fn init() {
    unsafe {
        *(&raw mut ROOT) = Some(new_dir(""));
    }
}

pub fn add_file(path: &str, data: Vec<u8>) {
    let parts = split(path);
    if parts.is_empty() {
        return;
    }
    let Some(dir) = ensure_dir_parts(root_ptr(), &parts[..parts.len() - 1]) else {
        return;
    };
    // Boot modules under bin/ are executables; everything else is data.
    let mode = if parts.len() >= 2 && parts[parts.len() - 2] == "bin" {
        0o755
    } else {
        0o644
    };
    let name = &parts[parts.len() - 1];
    match find_child_ptr(dir, name) {
        Some(p) => match unsafe { &mut *p } {
            Entry::File(f) => f.data = data,
            Entry::Dir(_) => {}
            Entry::Link(l) => l.target = String::new(),
        },
        None => unsafe {
            (*dir).entries.push(Entry::File(new_file(name, data, mode)));
        },
    }
}

/// Create or replace a symbolic link (used while building the boot layout).
pub fn add_link(path: &str, target: &str) {
    let Some((parent, name)) = parent_and_name(path) else {
        return;
    };
    match find_child_ptr(parent, &name) {
        Some(p) => {
            unsafe { *p = Entry::Link(new_link(&name, target)) };
        }
        None => unsafe {
            (*parent).entries.push(Entry::Link(new_link(&name, target)));
        },
    }
}

/// Create a pseudo-file whose contents are generated on access.
pub fn add_dynamic(path: &str, kind: Dynamic, mode: u16) {
    let parts = split(path);
    if parts.is_empty() {
        return;
    }
    let Some(dir) = ensure_dir_parts(root_ptr(), &parts[..parts.len() - 1]) else {
        return;
    };
    let name = &parts[parts.len() - 1];
    match find_child_ptr(dir, name) {
        Some(p) => unsafe {
            *p = Entry::File(new_dynamic_file(name, kind, mode));
        },
        None => unsafe {
            (*dir)
                .entries
                .push(Entry::File(new_dynamic_file(name, kind, mode)));
        },
    }
}

pub fn read(path: &str) -> Option<&'static [u8]> {
    let p = resolve(path, true)?;
    unsafe {
        let entry = &mut *p;
        refresh(entry);
        match entry {
            Entry::File(f) => Some(core::slice::from_raw_parts(f.data.as_ptr(), f.data.len())),
            _ => None,
        }
    }
}

fn create_file(path: &str, data: &[u8], mode: u16) -> bool {
    let Some((parent, name)) = parent_and_name(path) else {
        return false;
    };
    if find_child_ptr(parent, &name).is_some() {
        return false;
    }
    unsafe {
        (*parent)
            .entries
            .push(Entry::File(new_file(&name, data.to_vec(), mode)));
    }
    true
}

pub fn write(path: &str, data: &[u8]) -> bool {
    if let Some(p) = resolve(path, true) {
        return match unsafe { &mut *p } {
            Entry::File(f) => {
                if f.dynamic.is_some() {
                    // Sinks accept and discard; the rest are read-only.
                    return matches!(f.dynamic, Some(Dynamic::Null) | Some(Dynamic::Zero));
                }
                f.data.clear();
                f.data.extend_from_slice(data);
                crate::fs::journal::note(
                    path,
                    crate::fs::journal::Op::Write {
                        offset: 0,
                        data,
                        truncate: true,
                    },
                );
                true
            }
            _ => false,
        };
    }
    let created = create_file(path, data, 0o644);
    if created {
        crate::fs::journal::note(
            path,
            crate::fs::journal::Op::Write {
                offset: 0,
                data,
                truncate: true,
            },
        );
    }
    created
}

pub fn remove(path: &str) -> bool {
    let Some((parent, name)) = parent_and_name(path) else {
        return false;
    };
    let Some(idx) = (unsafe { (*parent).entries.iter().position(|e| e.name() == name) }) else {
        return false;
    };
    let removable = {
        let dir = unsafe { &*parent };
        matches!(
            dir.entries.get(idx),
            Some(Entry::File(_)) | Some(Entry::Link(_))
        )
    };
    if !removable {
        return false;
    }
    unsafe { (*parent).entries.remove(idx) };
    crate::fs::journal::note(path, crate::fs::journal::Op::Remove);
    true
}

/// Append to a file, creating it if needed. Used by shell output redirection.
pub fn append(path: &str, data: &[u8]) -> bool {
    if let Some(p) = resolve(path, true) {
        return match unsafe { &mut *p } {
            Entry::File(f) => {
                if f.dynamic.is_some() {
                    return matches!(f.dynamic, Some(Dynamic::Null) | Some(Dynamic::Zero));
                }
                let offset = f.data.len();
                f.data.extend_from_slice(data);
                crate::fs::journal::note(
                    path,
                    crate::fs::journal::Op::Write {
                        offset: offset as u64,
                        data,
                        truncate: false,
                    },
                );
                true
            }
            _ => false,
        };
    }
    let created = create_file(path, data, 0o644);
    if created {
        crate::fs::journal::note(
            path,
            crate::fs::journal::Op::Write {
                offset: 0,
                data,
                truncate: true,
            },
        );
    }
    created
}

/// Write `data` at `offset`, zero-filling any gap. Used by streaming file
/// descriptors, which may seek past the end before writing.
pub fn write_at(path: &str, offset: usize, data: &[u8]) -> bool {
    let Some(p) = resolve(path, true) else {
        return false;
    };
    match unsafe { &mut *p } {
        Entry::File(f) => {
            if f.dynamic.is_some() {
                return matches!(f.dynamic, Some(Dynamic::Null) | Some(Dynamic::Zero));
            }
            let Some(end) = offset.checked_add(data.len()) else {
                return false;
            };
            if end > f.data.len() {
                f.data.resize(end, 0);
            }
            f.data[offset..end].copy_from_slice(data);
            crate::fs::journal::note(
                path,
                crate::fs::journal::Op::Write {
                    offset: offset as u64,
                    data,
                    truncate: false,
                },
            );
            true
        }
        _ => false,
    }
}

/// Truncate a regular file to zero bytes (`O_TRUNC`).
pub fn truncate(path: &str) -> bool {
    let Some(p) = resolve(path, true) else {
        return false;
    };
    match unsafe { &mut *p } {
        Entry::File(f) => {
            if f.dynamic.is_some() {
                // Sinks have no content to discard; the other pseudo-files are
                // read-only and reject `O_TRUNC`.
                return matches!(f.dynamic, Some(Dynamic::Null) | Some(Dynamic::Zero));
            }
            f.data.clear();
            crate::fs::journal::note(
                path,
                crate::fs::journal::Op::Write {
                    offset: 0,
                    data: &[],
                    truncate: true,
                },
            );
            true
        }
        _ => false,
    }
}

/// The pseudo-file generator behind `path`, if any.
pub fn dynamic(path: &str) -> Option<Dynamic> {
    let p = resolve(path, true)?;
    match unsafe { &*p } {
        Entry::File(f) => f.dynamic,
        _ => None,
    }
}

/// Remove an empty directory.
pub fn rmdir(path: &str) -> bool {
    let Some((parent, name)) = parent_and_name(path) else {
        return false;
    };
    let Some(idx) = (unsafe { (*parent).entries.iter().position(|e| e.name() == name) }) else {
        return false;
    };
    let empty = {
        let dir = unsafe { &*parent };
        matches!(
            dir.entries.get(idx),
            Some(Entry::Dir(d)) if d.entries.is_empty()
        )
    };
    if !empty {
        return false;
    }
    unsafe { (*parent).entries.remove(idx) };
    crate::fs::journal::note(path, crate::fs::journal::Op::Rmdir);
    true
}

pub fn mkdir(path: &str) -> bool {
    let parts = split(path);
    if parts.is_empty() {
        return false;
    }
    if resolve(path, true).is_some() {
        return is_dir(path);
    }
    let created = ensure_dir_parts(root_ptr(), &parts).is_some();
    if created {
        crate::fs::journal::note(path, crate::fs::journal::Op::Mkdir { mode: 0o755 });
    }
    created
}

pub fn list(path: &str, out: &mut Vec<Info>) -> bool {
    let parts = split(path);
    let dir = if parts.is_empty() {
        root_ptr()
    } else {
        match resolve(path, true) {
            Some(p) => match dir_of(p) {
                Some(d) => d,
                None => return false,
            },
            None => return false,
        }
    };
    for e in unsafe { &mut (*dir).entries } {
        refresh(e);
        out.push(e.info());
    }
    true
}

pub fn stat(path: &str) -> Option<Info> {
    let parts = split(path);
    if parts.is_empty() {
        let r = root();
        return Some(Info {
            name: String::new(),
            is_dir: true,
            is_link: false,
            size: r.entries.len(),
            mode: r.mode,
            uid: r.uid,
            gid: r.gid,
        });
    }
    let p = resolve(path, true)?;
    unsafe {
        let entry = &mut *p;
        refresh(entry);
        Some(entry.info())
    }
}

/// `stat` without dereferencing the final symbolic link.
pub fn lstat(path: &str) -> Option<Info> {
    let parts = split(path);
    if parts.is_empty() {
        return stat(path);
    }
    let p = resolve(path, false)?;
    unsafe {
        let entry = &mut *p;
        refresh(entry);
        Some(entry.info())
    }
}

pub fn readlink(path: &str) -> Option<String> {
    match resolve(path, false) {
        Some(p) => match unsafe { &*p } {
            Entry::Link(l) => Some(l.target.clone()),
            _ => None,
        },
        None => None,
    }
}

pub fn symlink(target: &str, path: &str) -> bool {
    let Some((parent, name)) = parent_and_name(path) else {
        return false;
    };
    if find_child_ptr(parent, &name).is_some() {
        return false;
    }
    unsafe {
        (*parent)
            .entries
            .push(Entry::Link(new_link(&name, target)));
    }
    crate::fs::journal::note(path, crate::fs::journal::Op::Symlink(target));
    true
}

pub fn chmod(path: &str, mode: u16) -> bool {
    let parts = split(path);
    if parts.is_empty() {
        root().mode = S_IFDIR | (mode & 0o7777);
        return true;
    }
    match resolve(path, true) {
        Some(p) => match unsafe { &mut *p } {
            Entry::File(f) => {
                f.mode = S_IFREG | (mode & 0o7777);
                crate::fs::journal::note(path, crate::fs::journal::Op::Chmod(mode));
                true
            }
            Entry::Dir(d) => {
                d.mode = S_IFDIR | (mode & 0o7777);
                crate::fs::journal::note(path, crate::fs::journal::Op::Chmod(mode));
                true
            }
            Entry::Link(l) => {
                l.mode = S_IFLNK | (mode & 0o7777);
                crate::fs::journal::note(path, crate::fs::journal::Op::Chmod(mode));
                true
            }
        },
        None => false,
    }
}

pub fn chown(path: &str, uid: u32, gid: u32) -> bool {
    let parts = split(path);
    if parts.is_empty() {
        let r = root();
        r.uid = uid;
        r.gid = gid;
        return true;
    }
    match resolve(path, true) {
        Some(p) => match unsafe { &mut *p } {
            Entry::File(f) => {
                f.uid = uid;
                f.gid = gid;
                crate::fs::journal::note(path, crate::fs::journal::Op::Chown(uid, gid));
                true
            }
            Entry::Dir(d) => {
                d.uid = uid;
                d.gid = gid;
                crate::fs::journal::note(path, crate::fs::journal::Op::Chown(uid, gid));
                true
            }
            Entry::Link(l) => {
                l.uid = uid;
                l.gid = gid;
                crate::fs::journal::note(path, crate::fs::journal::Op::Chown(uid, gid));
                true
            }
        },
        None => false,
    }
}

/// Recursively sum file bytes below `path`. Returns `(files, dirs, bytes)`.
pub fn usage(path: &str) -> Option<(usize, usize, usize)> {
    fn walk(e: &Entry, files: &mut usize, dirs: &mut usize, bytes: &mut usize) {
        match e {
            Entry::File(f) => {
                *files += 1;
                *bytes += f.data.len();
            }
            Entry::Dir(d) => {
                *dirs += 1;
                for child in &d.entries {
                    walk(child, files, dirs, bytes);
                }
            }
            Entry::Link(_) => {}
        }
    }
    let parts = split(path);
    let mut files = 0;
    let mut dirs = 0;
    let mut bytes = 0;
    if parts.is_empty() {
        let r = root();
        dirs = 1;
        for child in &r.entries {
            walk(child, &mut files, &mut dirs, &mut bytes);
        }
        return Some((files, dirs, bytes));
    }
    let node = resolve(path, true)?;
    walk(unsafe { &*node }, &mut files, &mut dirs, &mut bytes);
    Some((files, dirs, bytes))
}

pub fn exists(path: &str) -> bool {
    split(path).is_empty() || resolve(path, true).is_some()
}

pub fn is_dir(path: &str) -> bool {
    split(path).is_empty()
        || matches!(
            resolve(path, true),
            Some(p) if matches!(unsafe { &*p }, Entry::Dir(_))
        )
}

pub fn size(path: &str) -> Option<usize> {
    let p = resolve(path, true)?;
    unsafe {
        let entry = &mut *p;
        refresh(entry);
        match entry {
            Entry::File(f) => Some(f.data.len()),
            _ => None,
        }
    }
}
