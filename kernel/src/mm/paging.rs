//! Our own 4-level paging helpers. No external paging crate.

use crate::mm::phys;

pub const PAGE_SIZE: u64 = 4096;
pub const USER_BASE: u64 = 0x0000_4000_0000_0000;
pub const USER_STACK_TOP: u64 = USER_BASE + 0x0000_0000_8000_0000;
/// 256 KiB is plenty for the shell and coreutils; the old 2 MiB reservation
/// cost 512 frames per task, which dominated `free`'s physical figure.
pub const USER_STACK_SIZE: u64 = 0x4_0000;

pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;
pub const PTE_WRITE_THROUGH: u64 = 1 << 3;
pub const PTE_CACHE_DISABLE: u64 = 1 << 4;
pub const PTE_ACCESSED: u64 = 1 << 5;
pub const PTE_DIRTY: u64 = 1 << 6;
pub const PTE_HUGE: u64 = 1 << 7;
pub const PTE_NX: u64 = 1 << 63;

#[inline]
pub fn read_cr3() -> u64 {
    let value: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack)) };
    value
}

#[inline]
pub fn write_cr3(value: u64) {
    unsafe { core::arch::asm!("mov cr3, {}", "mfence", in(reg) value, options(nostack)) };
}

#[inline]
pub fn invlpg(addr: u64) {
    unsafe { core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack)) };
}

fn index(addr: u64, level: u32) -> usize {
    ((addr >> (12 + 9 * level)) & 0x1FF) as usize
}

unsafe fn entry(table: u64, idx: usize) -> *mut u64 {
    unsafe { (table as *mut u64).add(idx) }
}

/// Walk (and allocate) the intermediate tables for `virt` in `cr3`.
unsafe fn walk(cr3: u64, virt: u64, user: bool) -> Option<u64> {
    let flags = PTE_PRESENT | PTE_WRITABLE | if user { PTE_USER } else { 0 };
    let mut table = cr3 & 0x000F_FFFF_FFFF_F000;
    for level in [3u32, 2, 1] {
        let idx = index(virt, level);
        let e = unsafe { entry(table, idx) };
        if unsafe { *e } & PTE_PRESENT == 0 {
            let frame = phys::alloc_zeroed_page()?;
            unsafe { *e = frame | flags };
        }
        table = unsafe { *e } & 0x000F_FFFF_FFFF_F000;
    }
    Some(table)
}

pub fn map_user_page(cr3: u64, virt: u64, phys_addr: u64, writable: bool) -> bool {
    if virt < USER_BASE {
        return false;
    }
    let pt = unsafe { walk(cr3, virt, true) };
    let Some(pt) = pt else {
        return false;
    };
    let e = unsafe { entry(pt, index(virt, 0)) };
    let mut flags = PTE_PRESENT | PTE_USER;
    if writable {
        flags |= PTE_WRITABLE;
    }
    unsafe { *e = (phys_addr & 0x000F_FFFF_FFFF_F000) | flags };
    true
}

pub fn map_user_range(cr3: u64, virt: u64, size: u64, writable: bool) -> bool {
    let mut off = 0u64;
    while off < size {
        let Some(frame) = phys::alloc_zeroed_page() else {
            return false;
        };
        if !map_user_page(cr3, virt + off, frame, writable) {
            phys::free_frame(frame);
            return false;
        }
        off += PAGE_SIZE;
    }
    true
}

pub fn new_user_cr3() -> Option<u64> {
    let cr3 = phys::alloc_zeroed_page()?;
    let kernel = read_cr3();
    unsafe {
        // Copy the low kernel identity map (PML4 entry 0). It is supervisor
        // only, so ring 3 cannot touch kernel memory even though it is shared.
        *entry(cr3, 0) = *entry(kernel, 0);
        // Private user half at 64 TiB.
        let pdpt = phys::alloc_zeroed_page()?;
        *entry(cr3, 128) = pdpt | PTE_PRESENT | PTE_WRITABLE | PTE_USER;
    }
    Some(cr3)
}

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Tear down the private user half of an address space and free the PML4
/// frame. Supervisor mappings shared with the kernel (PML4 entry 0) are left
/// untouched.
pub fn destroy_user_cr3(cr3: u64) {
    let pml4 = cr3 & ADDR_MASK;
    unsafe {
        let e = *entry(pml4, 128);
        if e & PTE_PRESENT != 0 {
            let pdpt = e & ADDR_MASK;
            for i in 0..512 {
                let pe = *entry(pdpt, i);
                if pe & PTE_PRESENT == 0 {
                    continue;
                }
                let pd = pe & ADDR_MASK;
                for j in 0..512 {
                    let de = *entry(pd, j);
                    if de & PTE_PRESENT == 0 {
                        continue;
                    }
                    let pt = de & ADDR_MASK;
                    for k in 0..512 {
                        let te = *entry(pt, k);
                        if te & PTE_PRESENT != 0 && te & PTE_HUGE == 0 {
                            phys::free_frame(te & ADDR_MASK);
                        }
                    }
                    phys::free_frame(pt);
                }
                phys::free_frame(pd);
            }
            phys::free_frame(pdpt);
        }
        phys::free_frame(pml4);
    }
}

pub fn is_user_address(addr: u64, len: u64) -> bool {
    addr >= USER_BASE && addr.saturating_add(len) < USER_BASE + 0x0000_0080_0000_0000
}

/// Translate a virtual address in `cr3` to a physical address.
pub fn translate(cr3: u64, virt: u64) -> Option<u64> {
    let mut table = cr3 & 0x000F_FFFF_FFFF_F000;
    for level in [3u32, 2, 1] {
        let e = unsafe { *entry(table, index(virt, level)) };
        if e & PTE_PRESENT == 0 {
            return None;
        }
        if e & PTE_HUGE != 0 {
            let mask = match level {
                2 => 0x000F_FFFF_C000_0000, // 1 GiB
                _ => 0x000F_FFFF_FFE0_0000, // 2 MiB
            };
            return Some((e & mask & 0x000F_FFFF_FFFF_F000) | (virt & !mask));
        }
        table = e & 0x000F_FFFF_FFFF_F000;
    }
    let e = unsafe { *entry(table, index(virt, 0)) };
    if e & PTE_PRESENT == 0 {
        return None;
    }
    Some((e & 0x000F_FFFF_FFFF_F000) | (virt & 0xFFF))
}

/// Check that every page touched by `[ptr, ptr+len)` is mapped *and* carries
/// the user bit. The kernel dereferences user pointers directly (the user
/// address space is active while a syscall runs), so a bad pointer would
/// otherwise fault in ring 0 and take the whole machine down.
pub fn user_range_ok(cr3: u64, ptr: u64, len: u64) -> bool {
    if len == 0 {
        return true;
    }
    let Some(end) = ptr.checked_add(len - 1) else {
        return false;
    };
    let last = end & !(PAGE_SIZE - 1);
    let mut page = ptr & !(PAGE_SIZE - 1);
    loop {
        if !user_page_ok(cr3, page) {
            return false;
        }
        if page >= last {
            return true;
        }
        page += PAGE_SIZE;
    }
}

fn user_page_ok(cr3: u64, page: u64) -> bool {
    let mask = 0x000F_FFFF_FFFF_F000;
    let mut table = cr3 & mask;
    for level in [3u32, 2, 1] {
        let e = unsafe { *entry(table, index(page, level)) };
        if e & PTE_PRESENT == 0 || e & PTE_USER == 0 {
            return false;
        }
        if e & PTE_HUGE != 0 {
            return true;
        }
        table = e & mask;
    }
    let e = unsafe { *entry(table, index(page, 0)) };
    e & PTE_PRESENT != 0 && e & PTE_USER != 0
}

/// Write bytes into a user address space from kernel context.
pub fn write_user(cr3: u64, mut virt: u64, mut data: &[u8]) -> bool {
    while !data.is_empty() {
        let Some(phys_addr) = translate(cr3, virt) else {
            return false;
        };
        let page_left = PAGE_SIZE - (virt & (PAGE_SIZE - 1));
        let n = (data.len() as u64).min(page_left) as usize;
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), phys_addr as *mut u8, n);
        }
        virt += n as u64;
        data = &data[n..];
    }
    true
}

pub fn zero_user(cr3: u64, mut virt: u64, mut len: u64) -> bool {
    while len > 0 {
        let Some(phys_addr) = translate(cr3, virt) else {
            return false;
        };
        let page_left = PAGE_SIZE - (virt & (PAGE_SIZE - 1));
        let n = len.min(page_left);
        unsafe {
            core::ptr::write_bytes(phys_addr as *mut u8, 0, n as usize);
        }
        virt += n;
        len -= n;
    }
    true
}
