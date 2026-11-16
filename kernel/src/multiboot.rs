//! Minimal Multiboot 1 information reader.
//!
//! We deliberately parse the GRUB-provided structures ourselves instead of
//! depending on a bootloader crate.

use core::ptr;

const MB_BOOTLOADER_MAGIC: u32 = 0x2BADB002;

#[derive(Clone, Copy)]
pub struct MmapEntry {
    pub base: u64,
    pub length: u64,
    pub kind: u32,
}

#[derive(Clone, Copy)]
pub struct Module {
    pub start: u64,
    pub end: u64,
    pub name: &'static str,
}

static mut INFO: usize = 0;
static mut VALID: bool = false;

pub fn init(magic: u32, info: u32) {
    unsafe {
        VALID = magic == MB_BOOTLOADER_MAGIC && info != 0;
        INFO = info as usize;
    }
}

pub fn info_address() -> usize {
    unsafe { INFO }
}

pub fn valid() -> bool {
    unsafe { VALID }
}

fn field(offset: usize) -> u32 {
    unsafe {
        let p = INFO as *const u8;
        if p.is_null() {
            return 0;
        }
        ptr::read_unaligned(p.add(offset) as *const u32)
    }
}

pub fn cmdline() -> Option<&'static str> {
    if field(0) & (1 << 2) == 0 {
        return None;
    }
    let addr = field(16) as usize;
    if addr == 0 {
        return None;
    }
    Some(unsafe { cstr(addr) })
}

pub fn mmap_length() -> usize {
    if field(0) & (1 << 6) == 0 {
        0
    } else {
        field(44) as usize
    }
}

pub fn mmap_addr() -> usize {
    field(48) as usize
}

/// Physical address of the ACPI RSDP passed by the bootloader through
/// Multiboot's `config_table` field (flag bit 8).
pub fn config_table() -> Option<u64> {
    if field(0) & (1 << 8) == 0 {
        return None;
    }
    let addr = field(60) as u64;
    if addr == 0 { None } else { Some(addr) }
}

pub fn for_each_mmap(mut f: impl FnMut(MmapEntry)) {
    let mut off = 0usize;
    let len = mmap_length();
    let base = mmap_addr();
    while off < len {
        unsafe {
            let entry = (base + off) as *const u8;
            let size = ptr::read_unaligned(entry as *const u32) as usize;
            if size < 20 || off + size + 4 > len {
                break;
            }
            let e = MmapEntry {
                base: ptr::read_unaligned(entry.add(4) as *const u64),
                length: ptr::read_unaligned(entry.add(12) as *const u64),
                kind: ptr::read_unaligned(entry.add(20) as *const u32),
            };
            f(e);
            off += size + 4;
        }
    }
}

pub fn module_count() -> usize {
    if field(0) & (1 << 3) == 0 {
        0
    } else {
        field(20) as usize
    }
}

pub fn for_each_module(mut f: impl FnMut(Module)) {
    let count = module_count();
    let base = field(24) as usize;
    for i in 0..count {
        unsafe {
            let entry = (base + i * 16) as *const u8;
            let start = ptr::read_unaligned(entry as *const u32) as u64;
            let end = ptr::read_unaligned(entry.add(4) as *const u32) as u64;
            let name_ptr = ptr::read_unaligned(entry.add(8) as *const u32) as usize;
            let name = if name_ptr == 0 {
                ""
            } else {
                cstr(name_ptr)
            };
            f(Module { start, end, name });
        }
    }
}

unsafe fn cstr(addr: usize) -> &'static str {
    let mut len = 0;
    let p = addr as *const u8;
    while unsafe { *p.add(len) } != 0 && len < 4096 {
        len += 1;
    }
    unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(p, len)) }
}
