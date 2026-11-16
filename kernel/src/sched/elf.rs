//! ELF64 loader for ring-3 programs.

use crate::mm::{paging, phys};

pub struct Loaded {
    pub entry: u64,
    pub end: u64,
}

fn u16_at(data: &[u8], off: usize) -> Option<u16> {
    let b = data.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(data: &[u8], off: usize) -> Option<u64> {
    let b = data.get(off..off + 8)?;
    Some(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

/// Load an ELF64 executable into `cr3`. All pages are allocated directly from
/// the physical allocator and copied through the kernel identity map.
pub fn load(cr3: u64, data: &[u8]) -> Option<Loaded> {
    if data.len() < 64 || &data[0..4] != b"\x7fELF" {
        return None;
    }
    if data[4] != 2 || data[5] != 1 {
        return None; // ELF64, little endian
    }
    if u16_at(data, 16)? != 2 || u16_at(data, 18)? != 62 {
        return None; // ET_EXEC, x86-64
    }

    let entry = u64_at(data, 24)?;
    let phoff = u64_at(data, 32)? as usize;
    let phentsize = u16_at(data, 54)? as usize;
    let phnum = u16_at(data, 56)? as usize;
    if phentsize < 56 {
        return None;
    }

    let mut image_end = entry;
    for i in 0..phnum {
        let ph = phoff.checked_add(i.checked_mul(phentsize)?)?;
        if ph + 56 > data.len() {
            return None;
        }
        let p_type = u32_at(data, ph)?;
        if p_type != 1 {
            continue; // PT_LOAD
        }
        let p_flags = u32_at(data, ph + 4)?;
        let p_offset = u64_at(data, ph + 8)? as usize;
        let p_vaddr = u64_at(data, ph + 16)?;
        let p_filesz = u64_at(data, ph + 32)?;
        let p_memsz = u64_at(data, ph + 40)?;

        if p_memsz == 0 {
            continue;
        }
        if p_vaddr < paging::USER_BASE {
            return None;
        }
        if p_offset.checked_add(p_filesz as usize)? > data.len() {
            return None;
        }

        let page_start = p_vaddr & !0xFFF;
        let page_end = (p_vaddr.checked_add(p_memsz)? + 0xFFF) & !0xFFF;
        let mut page = page_start;
        let writable = p_flags & 2 != 0;

        while page < page_end {
            let frame = phys::alloc_zeroed_page()?;

            let copy_begin = page.max(p_vaddr);
            let copy_end = (page + paging::PAGE_SIZE).min(p_vaddr + p_filesz);
            if copy_begin < copy_end {
                let src_off = p_offset + (copy_begin - p_vaddr) as usize;
                let len = (copy_end - copy_begin) as usize;
                if src_off + len > data.len() {
                    phys::free_frame(frame);
                    return None;
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr().add(src_off),
                        (frame + (copy_begin - page)) as *mut u8,
                        len,
                    );
                }
            }

            if !paging::map_user_page(cr3, page, frame, writable) {
                phys::free_frame(frame);
                return None;
            }
            page += paging::PAGE_SIZE;
        }

        image_end = image_end.max(p_vaddr + p_memsz);
    }

    Some(Loaded {
        entry,
        end: image_end,
    })
}
