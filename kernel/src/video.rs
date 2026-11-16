//! Boot video hand-off.
//!
//! Both boot loaders write a `VideoInfo` block at `VIDEO_INFO_ADDR` once they
//! have selected a linear-framebuffer mode. The kernel only reads it; the
//! identity map installed during boot already covers the first 4 GiB, which is
//! where VBE and (on QEMU/OVMF) GOP place their framebuffers.

pub const VIDEO_INFO_ADDR: usize = 0x9000;
pub const MAGIC: u32 = 0x534E_4656; // "SNFV"

/// Must match the layout the BIOS and UEFI loaders write.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VideoInfo {
    pub magic: u32,
    pub width: u32,
    pub height: u32,
    /// Bytes per scanline, not pixels.
    pub pitch: u32,
    pub bpp: u32,
    /// 0 for direct-colour masks; other values are reserved.
    pub format: u32,
    pub red_pos: u8,
    pub red_size: u8,
    pub green_pos: u8,
    pub green_size: u8,
    pub blue_pos: u8,
    pub blue_size: u8,
    pub reserved: [u8; 2],
    pub addr: u64,
    /// Usable framebuffer size in bytes.
    pub size: u64,
}

impl VideoInfo {
    fn sane(&self) -> bool {
        if self.magic != MAGIC || self.addr == 0 {
            return false;
        }
        if self.width < 320 || self.height < 200 || self.width > 4096 || self.height > 2160 {
            return false;
        }
        if self.bpp != 24 && self.bpp != 32 {
            return false;
        }
        let row = self.pitch as u64;
        if row < self.width as u64 * (self.bpp as u64 / 8) {
            return false;
        }
        // Only the first 4 GiB is identity mapped.
        let end = self.addr.saturating_add(self.size.max(row * self.height as u64));
        end <= (1u64 << 32)
    }
}

/// Returns the framebuffer description if a loader selected a graphics mode.
pub fn info() -> Option<VideoInfo> {
    let info = unsafe { (VIDEO_INFO_ADDR as *const VideoInfo).read_volatile() };
    if info.sane() { Some(info) } else { None }
}

impl VideoInfo {
    /// Convert an 0xRRGGBB colour into the framebuffer's native pixel layout.
    /// Done once per cell rather than once per pixel.
    #[inline]
    pub fn pack(&self, rgb: u32) -> u32 {
        let scale = |value: u32, size: u8| -> u32 {
            if size == 0 {
                0
            } else if size >= 8 {
                value
            } else {
                value >> (8 - size)
            }
        };
        (scale((rgb >> 16) & 0xFF, self.red_size) << self.red_pos)
            | (scale((rgb >> 8) & 0xFF, self.green_size) << self.green_pos)
            | (scale(rgb & 0xFF, self.blue_size) << self.blue_pos)
    }

    #[inline]
    fn scanline(&self, y: u32) -> *mut u8 {
        (self.addr as usize + y as usize * self.pitch as usize) as *mut u8
    }
}

/// Write `count` pixels of one packed colour starting at `x` on row `y`.
pub fn fill_row(info: &VideoInfo, x: u32, y: u32, count: u32, pixel: u32) {
    if y >= info.height || x >= info.width {
        return;
    }
    let count = count.min(info.width - x);
    unsafe {
        let row = info.scanline(y);
        if info.bpp == 32 {
            let p = row.add(x as usize * 4) as *mut u32;
            // Two pixels per store; unaligned 64-bit stores are legal on x86
            // and halve the number of volatile accesses.
            let pair = (pixel as u64) | ((pixel as u64) << 32);
            let words = count as usize / 2;
            let q = p as *mut u64;
            for i in 0..words {
                q.add(i).write_volatile(pair);
            }
            if count % 2 != 0 {
                p.add(count as usize - 1).write_volatile(pixel);
            }
        } else {
            let p = row.add(x as usize * 3);
            for i in 0..count as usize {
                let q = p.add(i * 3);
                q.write_volatile((pixel & 0xFF) as u8);
                q.add(1).write_volatile(((pixel >> 8) & 0xFF) as u8);
                q.add(2).write_volatile(((pixel >> 16) & 0xFF) as u8);
            }
        }
    }
}

/// Fill an axis-aligned rectangle with one packed colour. Used for clearing and
/// for the scroll background, where 32bpp output is written 64 bits at a time.
pub fn fill_rect(info: &VideoInfo, x: u32, y: u32, w: u32, h: u32, pixel: u32) {
    if info.bpp != 32 {
        for row in y..y.saturating_add(h).min(info.height) {
            fill_row(info, x, row, w, pixel);
        }
        return;
    }
    let pair = (pixel as u64) | ((pixel as u64) << 32);
    let w = w.min(info.width.saturating_sub(x));
    let h = h.min(info.height.saturating_sub(y));
    unsafe {
        for row in y..y + h {
            let start = info.scanline(row).add(x as usize * 4) as *mut u64;
            let words = w as usize / 2;
            for i in 0..words {
                start.add(i).write_volatile(pair);
            }
            if w % 2 != 0 {
                (start.add(words) as *mut u32).write_volatile(pixel);
            }
        }
    }
}

/// One output pixel as a 64-bit pair element: `fg` when `mask` is set in the
/// font scanline, `bg` otherwise.
fn pick(bits: u8, mask: u8, fg: u32, bg: u32) -> u64 {
    if bits & mask != 0 { fg as u64 } else { bg as u64 }
}

/// Expand one font scanline (8 pixels, MSB leftmost) into the framebuffer.
pub fn blit_bits(info: &VideoInfo, x: u32, y: u32, bits: u8, fg: u32, bg: u32) {
    if bits == 0 {
        fill_row(info, x, y, 8, bg);
        return;
    }
    if bits == 0xFF {
        fill_row(info, x, y, 8, fg);
        return;
    }
    if y >= info.height || x >= info.width {
        return;
    }
    unsafe {
        let row = info.scanline(y);
        if info.bpp == 32 {
            // Text columns start on 8-pixel boundaries, so the four 64-bit
            // stores below are aligned in practice; odd columns fall back to
            // one store per pixel.
            if x & 1 == 0 && x + 8 <= info.width {
                let q = row.add(x as usize * 4) as *mut u64;
                q.write_volatile(pick(bits, 0x80, fg, bg) | (pick(bits, 0x40, fg, bg) << 32));
                q.add(1).write_volatile(pick(bits, 0x20, fg, bg) | (pick(bits, 0x10, fg, bg) << 32));
                q.add(2).write_volatile(pick(bits, 0x08, fg, bg) | (pick(bits, 0x04, fg, bg) << 32));
                q.add(3).write_volatile(pick(bits, 0x02, fg, bg) | (pick(bits, 0x01, fg, bg) << 32));
                return;
            }
            let base = row.add(x as usize * 4);
            let n = 8u32.min(info.width - x);
            for i in 0..n {
                let pixel = if bits & (0x80 >> i) != 0 { fg } else { bg };
                (base.add(i as usize * 4) as *mut u32).write_volatile(pixel);
            }
        } else {
            for i in 0..8u32 {
                if x + i >= info.width {
                    break;
                }
                let pixel = if bits & (0x80 >> i) != 0 { fg } else { bg };
                let q = row.add((x + i) as usize * 3);
                q.write_volatile((pixel & 0xFF) as u8);
                q.add(1).write_volatile(((pixel >> 8) & 0xFF) as u8);
                q.add(2).write_volatile(((pixel >> 16) & 0xFF) as u8);
            }
        }
    }
}

/// Write one pixel in the format described by `info`. Used by the boot
/// self-test; the console uses the batched helpers above.
pub fn put_pixel(info: &VideoInfo, x: u32, y: u32, rgb: u32) {
    fill_row(info, x, y, 1, info.pack(rgb));
}

pub fn get_pixel(info: &VideoInfo, x: u32, y: u32) -> u32 {
    let base = info.addr as usize + y as usize * info.pitch as usize;
    unsafe {
        if info.bpp == 32 {
            ((base + x as usize * 4) as *const u32).read_volatile()
        } else {
            let p = (base + x as usize * 3) as *const u8;
            p.read_volatile() as u32
                | (p.add(1).read_volatile() as u32) << 8
                | (p.add(2).read_volatile() as u32) << 16
        }
    }
}

/// Confirm that the reported framebuffer is actually writable.
pub fn selftest(info: &VideoInfo) -> bool {
    let x = info.width - 1;
    let y = info.height - 1;
    let old = get_pixel(info, x, y);
    put_pixel(info, x, y, 0x00FF_FFFF);
    let new = get_pixel(info, x, y);
    put_pixel(info, x, y, old);
    new != old
}
