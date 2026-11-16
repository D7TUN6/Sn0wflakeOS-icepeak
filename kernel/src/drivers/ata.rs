//! ATA PIO block driver.
//!
//! Only programmed I/O is used: it needs no DMA setup, no bus-master BAR and
//! no interrupt handler, which keeps the block layer independent of the PCI
//! and APIC code. Each sector is moved through the 16-bit data port with
//! `rep insw`/`rep outsw`.
//!
//! The driver probes the four legacy channel positions (primary/secondary,
//! master/slave) and keeps the first ATA disk it finds. ATAPI devices such as
//! the boot CD report a different signature in the LBA registers during
//! `IDENTIFY` and are skipped.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch::port::{inb, insw, io_wait, outb, outsw};

const SECTOR: usize = 512;

const PRIMARY: u16 = 0x1F0;
const PRIMARY_CTRL: u16 = 0x3F6;
const SECONDARY: u16 = 0x170;
const SECONDARY_CTRL: u16 = 0x376;

const REG_DATA: u16 = 0;
const REG_ERROR: u16 = 1;
const REG_COUNT: u16 = 2;
const REG_LBA0: u16 = 3;
const REG_LBA1: u16 = 4;
const REG_LBA2: u16 = 5;
const REG_DRIVE: u16 = 6;
const REG_STATUS: u16 = 7;
const REG_COMMAND: u16 = 7;

const ST_ERR: u8 = 0x01;
const ST_DRQ: u8 = 0x08;
const ST_DF: u8 = 0x20;
const ST_BSY: u8 = 0x80;

const CTRL_NIEN: u8 = 0x02;
const CTRL_HOB: u8 = 0x80;

const CMD_READ28: u8 = 0x20;
const CMD_WRITE28: u8 = 0x30;
const CMD_FLUSH: u8 = 0xE7;
const CMD_IDENTIFY: u8 = 0xEC;
const CMD_READ48: u8 = 0x24;
const CMD_WRITE48: u8 = 0x34;
const CMD_FLUSH48: u8 = 0xEA;

pub struct AtaDrive {
    base: u16,
    ctrl: u16,
    slave: bool,
    lba48: bool,
    sectors: u64,
}

static FOUND: AtomicBool = AtomicBool::new(false);
static mut DRIVE: Option<AtaDrive> = None;
static LOCK: AtomicBool = AtomicBool::new(false);

struct Guard;

impl Guard {
    fn acquire() -> Guard {
        while LOCK
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Guard
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        LOCK.store(false, Ordering::Release);
    }
}

/// The probed disk, if one is attached.
pub fn drive() -> Option<&'static AtaDrive> {
    if !FOUND.load(Ordering::Acquire) {
        return None;
    }
    unsafe { (*(&raw const DRIVE)).as_ref() }
}

pub fn present() -> bool {
    drive().is_some()
}

pub fn probe() -> bool {
    let _g = Guard::acquire();
    unsafe {
        let slot = &mut *(&raw mut DRIVE);
        for (base, ctrl) in [(PRIMARY, PRIMARY_CTRL), (SECONDARY, SECONDARY_CTRL)] {
            for slave in [false, true] {
                if let Some(d) = identify(base, ctrl, slave) {
                    *slot = Some(d);
                    FOUND.store(true, Ordering::Release);
                    return true;
                }
            }
        }
    }
    false
}

pub fn attach() {
    if let Some(d) = drive() {
        crate::println!(
            "ata: {} sectors ({} MiB) on {} {}",
            d.sectors,
            d.sectors * SECTOR as u64 / (1024 * 1024),
            if d.base == PRIMARY { "primary" } else { "secondary" },
            if d.slave { "slave" } else { "master" }
        );
    }
}

fn wait_not_busy(base: u16) -> bool {
    for _ in 0..1_000_000u32 {
        let status = unsafe { inb(base + REG_STATUS) };
        if status == 0xFF {
            return false;
        }
        if status & ST_BSY == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn wait_drq(base: u16) -> bool {
    for _ in 0..1_000_000u32 {
        let status = unsafe { inb(base + REG_STATUS) };
        if status == 0xFF || status & ST_ERR != 0 || status & ST_DF != 0 {
            return false;
        }
        if status & ST_BSY == 0 && status & ST_DRQ != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn identify(base: u16, ctrl: u16, slave: bool) -> Option<AtaDrive> {
    unsafe {
        // Soft-reset the channel so both devices start from a known state.
        outb(ctrl, 0x04);
        for _ in 0..4 {
            io_wait();
        }
        outb(ctrl, 0x00);
        for _ in 0..4 {
            io_wait();
        }
        outb(base + REG_DRIVE, if slave { 0xB0 } else { 0xA0 });
        for _ in 0..4 {
            io_wait();
        }
        outb(base + REG_COUNT, 0);
        outb(base + REG_LBA0, 0);
        outb(base + REG_LBA1, 0);
        outb(base + REG_LBA2, 0);
        outb(base + REG_COMMAND, CMD_IDENTIFY);
        io_wait();

        if !wait_not_busy(base) {
            return None;
        }
        // A non-zero signature means ATAPI (or SATA), not a plain ATA disk.
        let lba1 = inb(base + REG_LBA1);
        let lba2 = inb(base + REG_LBA2);
        if lba1 != 0 || lba2 != 0 {
            return None;
        }
        if !wait_drq(base) {
            return None;
        }
        let mut words = [0u16; 256];
        for w in words.iter_mut() {
            *w = crate::arch::port::inw(base + REG_DATA);
        }
        let lba48 = words[83] & (1 << 10) != 0;
        let sectors28 = (words[60] as u64) | ((words[61] as u64) << 16);
        let sectors48 = (words[100] as u64)
            | ((words[101] as u64) << 16)
            | ((words[102] as u64) << 32)
            | ((words[103] as u64) << 48);
        let sectors = if lba48 && sectors48 != 0 {
            sectors48
        } else {
            sectors28
        };
        if sectors == 0 {
            return None;
        }
        Some(AtaDrive {
            base,
            ctrl,
            slave,
            lba48,
            sectors,
        })
    }
}

impl AtaDrive {
    pub fn sector_size(&self) -> usize {
        SECTOR
    }

    pub fn sector_count(&self) -> u64 {
        self.sectors
    }

    /// Program the drive/head register and wait out the 400 ns select delay.
    fn select(&self, head_bits: u8) {
        unsafe {
            outb(
                self.base + REG_DRIVE,
                head_bits | (if self.slave { 0x10 } else { 0 }),
            );
            for _ in 0..4 {
                io_wait();
            }
        }
    }

    /// Program LBA and sector count for the next command.
    ///
    /// Addresses below 2^28 use the plain 28-bit registers. Larger ones need
    /// the 48-bit command set, whose high-order bytes live in a second register
    /// file selected by the HOB bit of the device-control port; without that
    /// bit the high bytes overwrite the low ones and every access collapses
    /// onto LBA 0.
    fn program(&self, lba: u64, count: u8) -> bool {
        let use48 = self.lba48 && (lba >> 28) != 0;
        unsafe {
            if use48 {
                self.select(0x40);
                outb(self.ctrl, CTRL_HOB | CTRL_NIEN);
                outb(self.base + REG_COUNT, 0);
                outb(self.base + REG_LBA0, (lba >> 24) as u8);
                outb(self.base + REG_LBA1, (lba >> 32) as u8);
                outb(self.base + REG_LBA2, (lba >> 40) as u8);
                outb(self.ctrl, CTRL_NIEN);
            } else {
                self.select(0xE0 | (((lba >> 24) & 0x0F) as u8));
            }
            outb(self.base + REG_COUNT, count);
            outb(self.base + REG_LBA0, lba as u8);
            outb(self.base + REG_LBA1, (lba >> 8) as u8);
            outb(self.base + REG_LBA2, (lba >> 16) as u8);
        }
        use48
    }

    /// Read one sector into `buf` (must be exactly 512 bytes).
    pub fn read_sector(&self, lba: u64, buf: &mut [u8]) -> bool {
        if buf.len() != SECTOR || lba >= self.sectors {
            return false;
        }
        let _g = Guard::acquire();
        unsafe {
            let use48 = self.program(lba, 1);
            outb(
                self.base + REG_COMMAND,
                if use48 { CMD_READ48 } else { CMD_READ28 },
            );
            io_wait();
            if !wait_drq(self.base) {
                return false;
            }
            insw(self.base + REG_DATA, buf.as_mut_ptr() as *mut u16, SECTOR / 2);
            true
        }
    }

    /// Write one sector from `buf` (must be exactly 512 bytes).
    pub fn write_sector(&self, lba: u64, buf: &[u8]) -> bool {
        if buf.len() != SECTOR || lba >= self.sectors {
            return false;
        }
        let _g = Guard::acquire();
        unsafe {
            let use48 = self.program(lba, 1);
            outb(
                self.base + REG_COMMAND,
                if use48 { CMD_WRITE48 } else { CMD_WRITE28 },
            );
            io_wait();
            if !wait_drq(self.base) {
                return false;
            }
            outsw(self.base + REG_DATA, buf.as_ptr() as *const u16, SECTOR / 2);
            // Wait for the device to finish the transfer and report a clean
            // status, so the next command does not race the write.
            if !wait_not_busy(self.base) {
                return false;
            }
            let status = inb(self.base + REG_STATUS);
            status & (ST_ERR | ST_DF) == 0
        }
    }

    /// Flush the device write cache.
    pub fn flush(&self) -> bool {
        let _g = Guard::acquire();
        unsafe {
            self.select(0xE0);
            outb(
                self.base + REG_COMMAND,
                if self.lba48 { CMD_FLUSH48 } else { CMD_FLUSH },
            );
            io_wait();
            wait_not_busy(self.base)
        }
    }

    /// Read `count` sectors starting at `lba` into `buf`.
    pub fn read(&self, lba: u64, buf: &mut [u8]) -> bool {
        let sectors = buf.len() / SECTOR;
        if buf.len() % SECTOR != 0 {
            return false;
        }
        for i in 0..sectors {
            let start = i * SECTOR;
            if !self.read_sector(lba + i as u64, &mut buf[start..start + SECTOR]) {
                return false;
            }
        }
        true
    }

    /// Write `data` (a whole number of sectors) starting at `lba`.
    pub fn write(&self, lba: u64, data: &[u8]) -> bool {
        let sectors = data.len() / SECTOR;
        if data.len() % SECTOR != 0 {
            return false;
        }
        for i in 0..sectors {
            let start = i * SECTOR;
            if !self.write_sector(lba + i as u64, &data[start..start + SECTOR]) {
                return false;
            }
        }
        self.flush()
    }
}

/// Read a byte range that does not start or end on a sector boundary. The
/// block layer works in whole sectors; this helper covers the tail case.
pub fn read_bytes(lba: u64, offset: usize, dst: &mut [u8]) -> bool {
    let Some(d) = drive() else {
        return false;
    };
    let mut sector = [0u8; SECTOR];
    let mut done = 0usize;
    while done < dst.len() {
        let pos = offset + done;
        let idx = lba + (pos / SECTOR) as u64;
        if !d.read_sector(idx, &mut sector) {
            return false;
        }
        let within = pos % SECTOR;
        let n = (SECTOR - within).min(dst.len() - done);
        dst[done..done + n].copy_from_slice(&sector[within..within + n]);
        done += n;
    }
    true
}

/// Read-modify-write a byte range. Used by the superblock and inode updates.
pub fn write_bytes(lba: u64, offset: usize, src: &[u8]) -> bool {
    let Some(d) = drive() else {
        return false;
    };
    let mut sector = [0u8; SECTOR];
    let mut done = 0usize;
    while done < src.len() {
        let pos = offset + done;
        let idx = lba + (pos / SECTOR) as u64;
        let within = pos % SECTOR;
        if !d.read_sector(idx, &mut sector) {
            return false;
        }
        let n = (SECTOR - within).min(src.len() - done);
        sector[within..within + n].copy_from_slice(&src[done..done + n]);
        if !d.write_sector(idx, &sector) {
            return false;
        }
        done += n;
    }
    d.flush()
}

/// Bytes per sector, for the filesystem layer.
pub const BLOCK_BYTES: usize = SECTOR;

/// Convenience wrapper used by the filesystem to allocate a zeroed sector.
pub fn zero_sector() -> Vec<u8> {
    alloc::vec![0u8; SECTOR]
}
