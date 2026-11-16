//! Physical frame allocator: a bitmap covering the first 4 GiB.

use core::sync::atomic::{AtomicBool, Ordering};

pub const FRAME_SIZE: u64 = 4096;
pub const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_FRAMES: usize = (MAX_BYTES / FRAME_SIZE) as usize;

static mut BITMAP: [u64; MAX_FRAMES / 64] = [u64::MAX; MAX_FRAMES / 64];
const BITMAP_WORDS: usize = MAX_FRAMES / 64;
static mut USED_FRAMES: usize = 0;
static mut RAM_FRAMES: usize = 0;

static FRAME_LOCK: AtomicBool = AtomicBool::new(false);

struct FrameGuard;

impl FrameGuard {
    fn acquire() -> FrameGuard {
        while FRAME_LOCK
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        FrameGuard
    }
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        FRAME_LOCK.store(false, Ordering::Release);
    }
}

/// Set or clear a frame bit. The caller must hold `FRAME_LOCK`.
fn bit_set_locked(frame: usize, used: bool) {
    unsafe {
        let word = &mut (*(&raw mut BITMAP))[frame / 64];
        let mask = 1u64 << (frame % 64);
        let was = *word & mask != 0;
        if used && !was {
            *word |= mask;
            USED_FRAMES += 1;
        } else if !used && was {
            *word &= !mask;
            USED_FRAMES -= 1;
        }
    }
}

pub fn init() {
    {
        let _g = FrameGuard::acquire();
        unsafe {
            let bitmap = &mut (*(&raw mut BITMAP));
            for word in bitmap.iter_mut() {
                *word = u64::MAX;
            }
            USED_FRAMES = MAX_FRAMES;
            RAM_FRAMES = 0;
        }

        crate::multiboot::for_each_mmap(|entry| {
            if entry.kind != 1 {
                return;
            }
            let start = (entry.base + FRAME_SIZE - 1) & !(FRAME_SIZE - 1);
            let mut end = entry.base.saturating_add(entry.length) & !(FRAME_SIZE - 1);
            if end > MAX_BYTES {
                end = MAX_BYTES;
            }
            if end <= start {
                return;
            }
            unsafe { RAM_FRAMES += ((end - start) / FRAME_SIZE) as usize };
            let mut addr = start;
            while addr + FRAME_SIZE <= end {
                bit_set_locked((addr / FRAME_SIZE) as usize, false);
                addr += FRAME_SIZE;
            }
        });

        // Keep the traditional low 1 MiB, the kernel, GRUB modules and the
        // Multiboot structures exclusively for the kernel.
        reserve_range_locked(0, 0x10_0000);
    }
}

fn reserve_range_locked(start: u64, end: u64) {
    let first = start / FRAME_SIZE;
    let last = (end + FRAME_SIZE - 1) / FRAME_SIZE;
    let mut f = first as usize;
    while f < last as usize && f < MAX_FRAMES {
        bit_set_locked(f, true);
        f += 1;
    }
}

pub fn reserve_range(start: u64, end: u64) {
    let _g = FrameGuard::acquire();
    reserve_range_locked(start, end);
}

pub fn alloc_frame() -> Option<u64> {
    let _g = FrameGuard::acquire();
    unsafe {
        let bitmap = &mut (*(&raw mut BITMAP));
        for word_idx in 0..BITMAP_WORDS {
            let word = bitmap[word_idx];
            if word == u64::MAX {
                continue;
            }
            let bit = (!word).trailing_zeros() as usize;
            let frame = word_idx * 64 + bit;
            if frame < MAX_FRAMES {
                bitmap[word_idx] |= 1u64 << bit;
                USED_FRAMES += 1;
                return Some((frame as u64) * FRAME_SIZE);
            }
        }
    }
    None
}

pub fn free_frame(addr: u64) {
    if addr % FRAME_SIZE != 0 {
        return;
    }
    let frame = (addr / FRAME_SIZE) as usize;
    if frame < MAX_FRAMES {
        let _g = FrameGuard::acquire();
        bit_set_locked(frame, false);
    }
}

pub fn alloc_zeroed_page() -> Option<u64> {
    let frame = alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(frame as *mut u8, 0, FRAME_SIZE as usize);
    }
    Some(frame)
}

pub fn used_frames() -> usize {
    let _g = FrameGuard::acquire();
    let free = MAX_FRAMES - unsafe { USED_FRAMES };
    ram_frames_locked().saturating_sub(free)
}

pub fn ram_frames() -> usize {
    let _g = FrameGuard::acquire();
    ram_frames_locked()
}

fn ram_frames_locked() -> usize {
    unsafe { RAM_FRAMES }
}

pub fn total_frames() -> usize {
    MAX_FRAMES
}

pub fn used_bytes() -> usize {
    used_frames() * FRAME_SIZE as usize
}
