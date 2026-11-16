//! Persistent storage: a log-structured store over the ATA disk.
//!
//! The in-memory filesystem stays `ramfs`; this module mirrors every mutation
//! of a persistent path to an append-only log on disk and replays that log at
//! boot. Records are self-describing and checksummed, so a torn write at the
//! tail is simply ignored. On mount the log is replayed and then rewritten in
//! compacted form, which keeps the on-disk size proportional to the live tree
//! instead of the number of operations since the disk was created.
//!
//! Volatile paths (`/proc`, `/dev`, `/tmp`, `/run`) and the immutable store
//! (`/nix/store`) are never logged: the store is shipped in the ISO and
//! re-created from the boot modules on every boot, and the rest has no meaning
//! across a reboot.
//!
//! Disk layout, in 512-byte sectors:
//!
//! ```text
//!   0..1   superblock copies (magic, log start, write head, generation)
//!   8..    records, each padded to a sector boundary
//! ```
//!
//! A record is a 128-byte header followed by the path and payload. The header
//! carries the operation, the file offset, the permission bits and a
//! checksum over header and payload.

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::drivers::ata;
use crate::sync::SpinLock;

const MAGIC_SUPER: u64 = 0x534E_4653_4C4F_4731; // "SNFSLOG1"
const MAGIC_REC: u32 = 0x534E_4652; // "SNFR"
const VERSION: u32 = 1;
const LOG_START: u64 = 8;
const SUPER_SIZE: usize = 512;
const HEADER_SIZE: usize = 128;
const FLUSH_BYTES: usize = 128 * 1024;

const KIND_WRITE: u16 = 1;
const KIND_MKDIR: u16 = 2;
const KIND_SYMLINK: u16 = 3;
const KIND_REMOVE: u16 = 4;
const KIND_RMDIR: u16 = 5;
const KIND_CHMOD: u16 = 6;
const KIND_CHOWN: u16 = 7;

const FLAG_TRUNCATE: u16 = 1;

/// One filesystem mutation, as handed over by `ramfs`.
pub enum Op<'a> {
    Write {
        offset: u64,
        data: &'a [u8],
        truncate: bool,
    },
    Mkdir {
        mode: u16,
    },
    Symlink(&'a str),
    Remove,
    Rmdir,
    Chmod(u16),
    Chown(u32, u32),
}

static LOCK: SpinLock = SpinLock::new();
static mut MOUNTED: bool = false;
static mut REPLAYING: bool = false;
static mut HEAD: u64 = LOG_START;
static mut END: u64 = 0;
static mut GENERATION: u64 = 0;
static mut STAGING: Vec<u8> = Vec::new();

pub fn is_mounted() -> bool {
    unsafe { *(&raw const MOUNTED) }
}

/// Paths whose contents are re-created at every boot and therefore never
/// belong in the log.
fn persistent(path: &str) -> bool {
    !(path.starts_with("/nix/store")
        || path == "/nix"
        || path.starts_with("/nix/")
        || path.starts_with("/proc")
        || path.starts_with("/dev")
        || path.starts_with("/tmp")
        || path.starts_with("/run")
        || path == "/bin"
        || path == "/sbin"
        || path.starts_with("/usr"))
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = 0x811C_9DC5u32;
    for &b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn get_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn get_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn get_u64(bytes: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(v)
}

/// Build one padded record.
fn encode(kind: u16, path: &str, offset: u64, data: &[u8], flags: u16, mode: u16, uid: u32, gid: u32) -> Vec<u8> {
    let path = path.as_bytes();
    let total = HEADER_SIZE + path.len() + data.len();
    let sectors = total.div_ceil(ata::BLOCK_BYTES);
    let mut rec = Vec::with_capacity(sectors * ata::BLOCK_BYTES);
    put_u32(&mut rec, MAGIC_REC);
    put_u16(&mut rec, kind);
    put_u16(&mut rec, path.len() as u16);
    put_u16(&mut rec, flags);
    put_u16(&mut rec, 0);
    put_u64(&mut rec, offset);
    put_u32(&mut rec, data.len() as u32);
    put_u16(&mut rec, mode);
    put_u16(&mut rec, 0);
    put_u32(&mut rec, uid);
    put_u32(&mut rec, gid);
    put_u64(&mut rec, 0); // sequence, reserved
    put_u32(&mut rec, 0); // checksum, filled in below
    rec.resize(HEADER_SIZE, 0);
    rec.extend_from_slice(path);
    rec.extend_from_slice(data);
    debug_assert_eq!(rec.len(), total);
    // The checksum covers exactly the logical record, not the padding: the
    // decoder only reads `total` bytes back.
    let crc = fnv1a(&rec);
    rec[44..48].copy_from_slice(&crc.to_le_bytes());
    rec.resize(sectors * ata::BLOCK_BYTES, 0);
    rec
}

/// Record metadata decoded from a header.
struct Record {
    kind: u16,
    flags: u16,
    offset: u64,
    mode: u16,
    uid: u32,
    gid: u32,
    path: String,
    payload: Vec<u8>,
}

fn decode(bytes: &[u8]) -> Option<Record> {
    if bytes.len() < HEADER_SIZE || get_u32(bytes, 0) != MAGIC_REC {
        return None;
    }
    let kind = get_u16(bytes, 4);
    let path_len = get_u16(bytes, 6) as usize;
    let flags = get_u16(bytes, 8);
    let offset = get_u64(bytes, 12);
    let data_len = get_u32(bytes, 20) as usize;
    let mode = get_u16(bytes, 24);
    let uid = get_u32(bytes, 28);
    let gid = get_u32(bytes, 32);
    let total = HEADER_SIZE + path_len + data_len;
    if total > bytes.len() {
        return None;
    }
    let stored = get_u32(bytes, 44);
    let mut copy = bytes[..total].to_vec();
    copy[44..48].copy_from_slice(&[0, 0, 0, 0]);
    if fnv1a(&copy) != stored {
        return None;
    }
    let path = core::str::from_utf8(&bytes[HEADER_SIZE..HEADER_SIZE + path_len])
        .ok()?
        .to_string();
    let payload = bytes[HEADER_SIZE + path_len..total].to_vec();
    Some(Record {
        kind,
        flags,
        offset,
        mode,
        uid,
        gid,
        path,
        payload,
    })
}

fn apply(rec: &Record) {
    match rec.kind {
        KIND_WRITE => {
            if rec.flags & FLAG_TRUNCATE != 0 {
                crate::fs::ramfs::write(&rec.path, &rec.payload);
            } else {
                crate::fs::ramfs::write_at(&rec.path, rec.offset as usize, &rec.payload);
            }
        }
        KIND_MKDIR => {
            crate::fs::ramfs::mkdir(&rec.path);
            if rec.mode != 0 {
                crate::fs::ramfs::chmod(&rec.path, rec.mode);
            }
        }
        KIND_SYMLINK => {
            if let Ok(target) = core::str::from_utf8(&rec.payload) {
                crate::fs::ramfs::symlink(target, &rec.path);
            }
        }
        KIND_REMOVE => {
            crate::fs::ramfs::remove(&rec.path);
        }
        KIND_RMDIR => {
            crate::fs::ramfs::rmdir(&rec.path);
        }
        KIND_CHMOD => {
            crate::fs::ramfs::chmod(&rec.path, rec.mode);
        }
        KIND_CHOWN => {
            crate::fs::ramfs::chown(&rec.path, rec.uid, rec.gid);
        }
        _ => {}
    }
}

fn read_super() -> Option<(u64, u64)> {
    let drive = ata::drive()?;
    let mut best: Option<(u64, u64)> = None;
    for sector in 0..2u64 {
        let mut buf = [0u8; SUPER_SIZE];
        if !drive.read_sector(sector, &mut buf) {
            continue;
        }
        if get_u64(&buf, 0) != MAGIC_SUPER || get_u32(&buf, 8) != VERSION {
            continue;
        }
        let stored = get_u32(&buf, 40);
        let mut copy = buf;
        copy[40..44].copy_from_slice(&[0, 0, 0, 0]);
        if fnv1a(&copy[..44]) != stored {
            continue;
        }
        let head = get_u64(&buf, 16);
        let generation = get_u64(&buf, 24);
        if head < LOG_START || head > drive.sector_count() {
            continue;
        }
        if best.map_or(true, |(_, g)| generation > g) {
            best = Some((head, generation));
        }
    }
    best
}

fn write_super(head: u64, generation: u64) -> bool {
    let Some(drive) = ata::drive() else {
        return false;
    };
    let mut buf = [0u8; SUPER_SIZE];
    buf[0..8].copy_from_slice(&MAGIC_SUPER.to_le_bytes());
    buf[8..12].copy_from_slice(&VERSION.to_le_bytes());
    buf[12..16].copy_from_slice(&(SUPER_SIZE as u32).to_le_bytes());
    buf[16..24].copy_from_slice(&head.to_le_bytes());
    buf[24..32].copy_from_slice(&generation.to_le_bytes());
    buf[32..40].copy_from_slice(&LOG_START.to_le_bytes());
    buf[40..44].copy_from_slice(&[0, 0, 0, 0]);
    let crc = fnv1a(&buf[..44]);
    buf[40..44].copy_from_slice(&crc.to_le_bytes());
    drive.write_sector(0, &buf) && drive.write_sector(1, &buf)
}

/// Replay the log into the in-memory tree. Returns the number of records
/// applied, or `None` if the device is not usable.
fn replay(drive: &ata::AtaDrive, head: u64) -> Option<usize> {
    let mut sector = LOG_START;
    let mut applied = 0usize;
    let mut raw = [0u8; ata::BLOCK_BYTES];
    while sector < head {
        if !drive.read_sector(sector, &mut raw) {
            return Some(applied);
        }
        if get_u32(&raw, 0) != MAGIC_REC {
            break;
        }
        let path_len = get_u16(&raw, 6) as usize;
        let data_len = get_u32(&raw, 20) as usize;
        let total = HEADER_SIZE + path_len + data_len;
        let sectors = total.div_ceil(ata::BLOCK_BYTES).max(1);
        if total > sectors * ata::BLOCK_BYTES {
            break;
        }
        let mut record = vec![0u8; sectors * ata::BLOCK_BYTES];
        record[..ata::BLOCK_BYTES].copy_from_slice(&raw);
        let mut ok = true;
        for i in 1..sectors {
            if !drive.read_sector(sector + i as u64, &mut raw) {
                ok = false;
                break;
            }
            record[i * ata::BLOCK_BYTES..(i + 1) * ata::BLOCK_BYTES].copy_from_slice(&raw);
        }
        if !ok {
            break;
        }
        let Some(rec) = decode(&record) else {
            break;
        };
        apply(&rec);
        applied += 1;
        sector += sectors as u64;
    }
    Some(applied)
}

/// Collect every persistent path in the live tree, parents before children.
fn collect(path: &str, out: &mut Vec<String>) {
    let mut entries = Vec::new();
    if !crate::fs::ramfs::list(path, &mut entries) {
        return;
    }
    for entry in entries {
        let child = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };
        if !persistent(&child) {
            continue;
        }
        out.push(child.clone());
        if entry.is_dir {
            collect(&child, out);
        }
    }
}

/// Rewrite the log so it contains exactly the live persistent tree.
fn compact(drive: &ata::AtaDrive) -> bool {
    let mut paths = Vec::new();
    collect("/", &mut paths);
    let mut bytes: Vec<u8> = Vec::new();
    for path in &paths {
        let Some(info) = crate::fs::ramfs::lstat(path) else {
            continue;
        };
        if info.is_link {
            if let Some(target) = crate::fs::ramfs::readlink(path) {
                bytes.extend_from_slice(&encode(
                    KIND_SYMLINK,
                    path,
                    0,
                    target.as_bytes(),
                    0,
                    info.mode,
                    info.uid,
                    info.gid,
                ));
            }
        } else if info.is_dir {
            bytes.extend_from_slice(&encode(
                KIND_MKDIR,
                path,
                0,
                &[],
                0,
                info.mode,
                info.uid,
                info.gid,
            ));
        } else if let Some(data) = crate::fs::ramfs::read(path) {
            bytes.extend_from_slice(&encode(
                KIND_WRITE,
                path,
                0,
                data,
                FLAG_TRUNCATE,
                info.mode,
                info.uid,
                info.gid,
            ));
        }
    }
    if bytes.is_empty() {
        return write_super(LOG_START, unsafe { *(&raw const GENERATION) } + 1);
    }
    let sectors = bytes.len().div_ceil(ata::BLOCK_BYTES);
    if LOG_START + sectors as u64 > drive.sector_count() {
        return false;
    }
    bytes.resize(sectors * ata::BLOCK_BYTES, 0);
    if !drive.write(LOG_START, &bytes) {
        return false;
    }
    let head = LOG_START + sectors as u64;
    unsafe {
        *(&raw mut HEAD) = head;
        *(&raw mut GENERATION) += 1;
        *(&raw mut END) = drive.sector_count();
        *(&raw mut MOUNTED) = true;
        *(&raw mut STAGING) = Vec::new();
    }
    write_super(head, unsafe { *(&raw const GENERATION) })
}

/// Attach the disk, replay the log over the freshly seeded tree and compact it.
pub fn mount() -> bool {
    let Some(drive) = ata::drive() else {
        return false;
    };
    let _g = LOCK.lock();
    unsafe {
        *(&raw mut REPLAYING) = true;
        *(&raw mut END) = drive.sector_count();
    }
    let replayed = match read_super() {
        Some((head, generation)) => {
            unsafe {
                *(&raw mut HEAD) = head;
                *(&raw mut GENERATION) = generation;
            }
            replay(drive, head)
        }
        None => Some(0),
    };
    unsafe {
        *(&raw mut REPLAYING) = false;
    }
    let Some(count) = replayed else {
        return false;
    };
    if !compact(drive) {
        crate::println!("persist: cannot write superblock");
        return false;
    }
    crate::println!(
        "persist: {} records replayed, head={} sectors, {} MiB disk",
        count,
        unsafe { *(&raw const HEAD) },
        drive.sector_count() * ata::BLOCK_BYTES as u64 / (1024 * 1024)
    );
    true
}

/// Append one mutation to the log. Called from `ramfs` after the in-memory
/// change succeeded. Volatile paths and replay are ignored.
pub fn note(path: &str, op: Op) {
    if !is_mounted() || unsafe { *(&raw const REPLAYING) } || !persistent(path) {
        return;
    }
    let rec = match op {
        Op::Write {
            offset,
            data,
            truncate,
        } => encode(
            KIND_WRITE,
            path,
            offset,
            data,
            if truncate { FLAG_TRUNCATE } else { 0 },
            0,
            0,
            0,
        ),
        Op::Mkdir { mode } => encode(KIND_MKDIR, path, 0, &[], 0, mode, 0, 0),
        Op::Symlink(target) => encode(KIND_SYMLINK, path, 0, target.as_bytes(), 0, 0, 0, 0),
        Op::Remove => encode(KIND_REMOVE, path, 0, &[], 0, 0, 0, 0),
        Op::Rmdir => encode(KIND_RMDIR, path, 0, &[], 0, 0, 0, 0),
        Op::Chmod(mode) => encode(KIND_CHMOD, path, 0, &[], 0, mode, 0, 0),
        Op::Chown(uid, gid) => encode(KIND_CHOWN, path, 0, &[], 0, 0, uid, gid),
    };
    let _g = LOCK.lock();
    unsafe {
        let staging = &mut *(&raw mut STAGING);
        staging.extend_from_slice(&rec);
        if staging.len() >= FLUSH_BYTES {
            flush_locked();
        }
    }
}

/// Write out everything buffered so far. Safe to call from a syscall, from the
/// poweroff path, or from the shell's `sync`.
pub fn flush() {
    if !is_mounted() {
        return;
    }
    let _g = LOCK.lock();
    flush_locked();
}

fn flush_locked() {
    unsafe {
        let staging = &mut *(&raw mut STAGING);
        if staging.is_empty() {
            return;
        }
        let Some(drive) = ata::drive() else {
            return;
        };
        let head = *(&raw const HEAD);
        let end = *(&raw const END);
        let sectors = staging.len().div_ceil(ata::BLOCK_BYTES);
        if head + sectors as u64 > end {
            crate::println!("persist: log full");
            staging.clear();
            return;
        }
        staging.resize(sectors * ata::BLOCK_BYTES, 0);
        if drive.write(head, &staging[..]) {
            *(&raw mut HEAD) = head + sectors as u64;
            *(&raw mut GENERATION) += 1;
            write_super(*(&raw const HEAD), *(&raw const GENERATION));
        }
        staging.clear();
    }
}
