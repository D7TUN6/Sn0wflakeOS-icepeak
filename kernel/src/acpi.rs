//! ACPI table discovery and the bits of the specification the kernel uses:
//! FADT (power management, reset, C-state latencies), HPET, MCFG, MADT and the
//! `_S5` sleep type from the DSDT. No external ACPI crate is involved.

use crate::arch::port::{inb, outb, outw};

#[derive(Clone, Copy)]
pub struct GenericAddress {
    pub space: u8,
    pub bit_width: u8,
    pub bit_offset: u8,
    pub access_width: u8,
    pub address: u64,
}

impl GenericAddress {
    const EMPTY: Self = Self {
        space: 0,
        bit_width: 0,
        bit_offset: 0,
        access_width: 0,
        address: 0,
    };

    fn read(addr: u64) -> Self {
        Self {
            space: phys_u8(addr),
            bit_width: phys_u8(addr + 1),
            bit_offset: phys_u8(addr + 2),
            access_width: phys_u8(addr + 3),
            address: phys_u64(addr + 4),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Hpet {
    pub base: u64,
    pub period_fs: u64,
    pub comparators: u8,
    pub counter_64: bool,
}

#[derive(Clone, Copy)]
pub struct McfgEntry {
    pub base: u64,
    pub segment: u16,
    pub start_bus: u8,
    pub end_bus: u8,
}

static mut RSDP: u64 = 0;
static mut FADT: u64 = 0;
static mut DSDT: u64 = 0;

static mut PM1A_EVT: u16 = 0;
static mut PM1B_EVT: u16 = 0;
static mut PM1A_CNT: u16 = 0;
static mut PM1B_CNT: u16 = 0;
static mut PM2_CNT: u16 = 0;
static mut PM_TMR: u16 = 0;
static mut PM1_CNT_LEN: u8 = 2;
static mut PM_TMR_LEN: u8 = 4;
static mut PM_TMR_32BIT: bool = false;
static mut C2_LATENCY: u16 = 0xFFFF;
static mut C3_LATENCY: u16 = 0xFFFF;
static mut CST_CNT: u8 = 0;
static mut SLP_TYPA: u16 = 5;
static mut RESET_REG: GenericAddress = GenericAddress::EMPTY;
static mut RESET_VALUE: u8 = 0;
static mut LAPIC_BASE: u64 = 0;
static mut FLAGS: u32 = 0;
static mut HPET: Option<Hpet> = None;
static mut MCFG: Option<McfgEntry> = None;
static mut MCFG_COUNT: usize = 0;

pub(crate) fn phys_u8(addr: u64) -> u8 {
    unsafe { core::ptr::read_volatile(addr as *const u8) }
}

pub(crate) fn phys_u16(addr: u64) -> u16 {
    unsafe { core::ptr::read_unaligned(addr as *const u16) }
}

pub(crate) fn phys_u32(addr: u64) -> u32 {
    unsafe { core::ptr::read_unaligned(addr as *const u32) }
}

pub(crate) fn phys_u64(addr: u64) -> u64 {
    unsafe { core::ptr::read_unaligned(addr as *const u64) }
}

fn checksum(addr: u64, len: usize) -> bool {
    let mut sum = 0u8;
    for i in 0..len {
        sum = sum.wrapping_add(phys_u8(addr + i as u64));
    }
    sum == 0
}

fn signature_is(addr: u64, sig: &[u8; 4]) -> bool {
    (0..4).all(|i| phys_u8(addr + i as u64) == sig[i])
}

fn signature_is_rsdp(addr: u64) -> bool {
    let sig = b"RSD PTR ";
    (0..8).all(|i| phys_u8(addr + i as u64) == sig[i])
}

fn find_rsdp() -> Option<u64> {
    // The UEFI loader hands us the firmware's RSDP through Multiboot's
    // config_table field; trust it only after validating the signature and
    // checksum.
    if let Some(addr) = crate::multiboot::config_table() {
        if signature_is_rsdp(addr) && checksum(addr, 20) {
            return Some(addr);
        }
    }
    // EBDA segment pointer lives at 0x40E. Search its first KiB.
    let ebda = (phys_u16(0x40E) as u64) << 4;
    if (0x80000..0xA0000).contains(&ebda) {
        let mut p = ebda;
        while p < ebda + 1024 {
            if checksum(p, 20) && signature_is_rsdp(p) {
                return Some(p);
            }
            p += 16;
        }
    }
    // Main BIOS area.
    let mut p = 0xE_0000u64;
    while p < 0x10_0000 {
        if checksum(p, 20) && signature_is_rsdp(p) {
            return Some(p);
        }
        p += 16;
    }
    None
}

/// Walk an RSDT (or XSDT when the RSDP revision allows it) looking for a
/// 4-byte signature. The returned table is validated for length and checksum.
pub fn find_table(rsdp: u64, sig: &[u8; 4]) -> Option<u64> {
    let revision = phys_u8(rsdp + 15);
    let use_xsdt = revision >= 2 && phys_u32(rsdp + 20) != 0;
    let (root, ptr_size) = if use_xsdt {
        (phys_u64(rsdp + 24), 8u64)
    } else {
        (phys_u32(rsdp + 16) as u64, 4u64)
    };
    if root == 0 {
        return None;
    }
    let length = phys_u32(root + 4) as u64;
    if length < 36 || length > 0x10_0000 {
        return None;
    }
    if !checksum(root, length as usize) {
        return None;
    }
    let entries = (length - 36) / ptr_size;
    for i in 0..entries {
        let entry = root + 36 + i * ptr_size;
        let table = if use_xsdt {
            phys_u64(entry)
        } else {
            phys_u32(entry) as u64
        };
        if table != 0 && signature_is(table, sig) {
            let len = phys_u32(table + 4) as usize;
            if len >= 36 && len <= 0x10_0000 && checksum(table, len) {
                return Some(table);
            }
        }
    }
    None
}

/// Extract the SLP_TYPa value from the DSDT's `_S5` package.
fn find_s5(dsdt: u64) -> Option<u8> {
    if dsdt == 0 {
        return None;
    }
    let length = (phys_u32(dsdt + 4) as u64).min(0x10_0000);
    let mut i = 36u64;
    while i + 4 < length {
        if phys_u8(dsdt + i) == b'_'
            && phys_u8(dsdt + i + 1) == b'S'
            && phys_u8(dsdt + i + 2) == b'5'
            && phys_u8(dsdt + i + 3) == b'_'
        {
            let mut j = i + 4;
            let end = (i + 64).min(length);
            while j < end && phys_u8(dsdt + j) != 0x12 {
                j += 1;
            }
            if j >= end {
                i += 4;
                continue;
            }
            j += 1;
            let lead = phys_u8(dsdt + j);
            let extra = (lead >> 6) as u64;
            j += 1 + extra;
            let count = phys_u8(dsdt + j);
            j += 1;
            if count == 0 {
                return None;
            }
            let first = phys_u8(dsdt + j);
            return Some(if first == 0x0A {
                phys_u8(dsdt + j + 1)
            } else {
                first & 0x0F
            });
        }
        i += 1;
    }
    None
}

fn parse_hpet(rsdp: u64) -> Option<Hpet> {
    let table = find_table(rsdp, b"HPET")?;
    let gaddr = GenericAddress::read(table + 44);
    if gaddr.address == 0 {
        return None;
    }
    let caps = unsafe { core::ptr::read_volatile(gaddr.address as *const u64) };
    Some(Hpet {
        base: gaddr.address,
        period_fs: caps >> 32,
        comparators: (((caps >> 8) & 0x1F) + 1) as u8,
        counter_64: caps & (1 << 13) != 0,
    })
}

fn parse_mcfg(rsdp: u64) -> (Option<McfgEntry>, usize) {
    let Some(table) = find_table(rsdp, b"MCFG") else {
        return (None, 0);
    };
    let length = phys_u32(table + 4) as u64;
    if length < 44 {
        return (None, 0);
    }
    let count = ((length - 44) / 16) as usize;
    if count == 0 {
        return (None, 0);
    }
    let first = McfgEntry {
        base: phys_u64(table + 44),
        segment: phys_u16(table + 52),
        start_bus: phys_u8(table + 54),
        end_bus: phys_u8(table + 55),
    };
    (Some(first), count)
}

pub fn rsdp() -> Option<u64> {
    let r = unsafe { RSDP };
    if r != 0 { Some(r) } else { find_rsdp() }
}

pub fn init() {
    let Some(rsdp) = find_rsdp() else {
        crate::println!("acpi: no RSDP found");
        return;
    };
    unsafe { RSDP = rsdp };

    let Some(fadt) = find_table(rsdp, b"FACP") else {
        crate::println!("acpi: no FADT found");
        return;
    };
    unsafe { FADT = fadt };

    let revision = phys_u8(fadt + 8);
    let length = phys_u32(fadt + 4) as u64;
    let dsdt_low = phys_u32(fadt + 40) as u64;
    let xdsdt = if revision >= 2 && length >= 148 {
        phys_u64(fadt + 140)
    } else {
        0
    };
    let dsdt = if xdsdt != 0 { xdsdt } else { dsdt_low };

    let pm1a_evt = phys_u32(fadt + 56) as u16;
    let pm1b_evt = phys_u32(fadt + 60) as u16;
    let pm1a_cnt = phys_u32(fadt + 64) as u16;
    let pm1b_cnt = phys_u32(fadt + 68) as u16;
    let pm2_cnt = phys_u32(fadt + 72) as u16;
    let pm_tmr = phys_u32(fadt + 76) as u16;
    let pm1_cnt_len = phys_u8(fadt + 89);
    let pm_tmr_len = phys_u8(fadt + 91);
    let cst_cnt = phys_u8(fadt + 95);
    let c2_lat = phys_u16(fadt + 96);
    let c3_lat = phys_u16(fadt + 98);
    let flags = phys_u32(fadt + 112);
    let reset = if length >= 128 {
        GenericAddress::read(fadt + 116)
    } else {
        GenericAddress::EMPTY
    };
    let reset_value = if length >= 129 { phys_u8(fadt + 128) } else { 0 };
    // The local-APIC base lives in the MADT, not the FADT; apic::init()
    // publishes it separately.
    let lapic = 0;

    let mut slp = find_s5(dsdt).unwrap_or(5);
    if slp == 0 || slp > 0x0F {
        slp = 5;
    }

    let hpet = parse_hpet(rsdp);
    let (mcfg, mcfg_count) = parse_mcfg(rsdp);

    unsafe {
        DSDT = dsdt;
        PM1A_EVT = pm1a_evt;
        PM1B_EVT = pm1b_evt;
        PM1A_CNT = pm1a_cnt;
        PM1B_CNT = pm1b_cnt;
        PM2_CNT = pm2_cnt;
        PM_TMR = pm_tmr;
        PM1_CNT_LEN = pm1_cnt_len;
        PM_TMR_LEN = pm_tmr_len;
        PM_TMR_32BIT = flags & (1 << 8) != 0 || pm_tmr_len >= 4;
        C2_LATENCY = c2_lat;
        C3_LATENCY = c3_lat;
        CST_CNT = cst_cnt;
        FLAGS = flags;
        SLP_TYPA = slp as u16;
        RESET_REG = reset;
        RESET_VALUE = reset_value;
        LAPIC_BASE = lapic;
        HPET = hpet;
        MCFG = mcfg;
        MCFG_COUNT = mcfg_count;
    }

    crate::println!(
        "acpi: FADT rev {} PM1a_CNT={:#06x} PM_TMR={:#06x} PM2={:#06x} SLP_TYPa={:#x}",
        revision,
        pm1a_cnt,
        pm_tmr,
        pm2_cnt,
        slp
    );
    crate::println!(
        "acpi: C2={}us C3={}us CST_CNT={} flags={:#010x}",
        c2_lat,
        c3_lat,
        cst_cnt,
        flags
    );
    if let Some(h) = hpet {
        crate::println!(
            "acpi: HPET base={:#x} period={}fs comparators={} 64bit={}",
            h.base,
            h.period_fs,
            h.comparators,
            h.counter_64
        );
    }
    if let Some(m) = mcfg {
        crate::println!(
            "acpi: MCFG {} entries, first base={:#x} segment={} bus {}-{}",
            mcfg_count,
            m.base,
            m.segment,
            m.start_bus,
            m.end_bus
        );
    }
    if reset.address != 0 {
        crate::println!(
            "acpi: reset register space={} addr={:#x} value={:#x}",
            reset.space,
            reset.address,
            reset_value
        );
    }
}

pub fn fadt() -> u64 {
    unsafe { FADT }
}

pub fn dsdt() -> u64 {
    unsafe { DSDT }
}

pub fn pm1a_cnt() -> u16 {
    unsafe { PM1A_CNT }
}

pub fn pm1b_cnt() -> u16 {
    unsafe { PM1B_CNT }
}

pub fn pm1_cnt_len() -> u8 {
    unsafe { PM1_CNT_LEN }
}

pub fn pm_tmr() -> Option<u16> {
    let p = unsafe { PM_TMR };
    if p == 0 { None } else { Some(p) }
}

pub fn pm_tmr_32bit() -> bool {
    unsafe { PM_TMR_32BIT }
}

pub fn pm2_cnt() -> Option<u16> {
    let p = unsafe { PM2_CNT };
    if p == 0 { None } else { Some(p) }
}

pub fn c2_latency_us() -> u16 {
    unsafe { C2_LATENCY }
}

pub fn c3_latency_us() -> u16 {
    unsafe { C3_LATENCY }
}

pub fn cst_count() -> u8 {
    unsafe { CST_CNT }
}

pub fn flags() -> u32 {
    unsafe { FLAGS }
}

pub fn hpet() -> Option<Hpet> {
    unsafe { HPET }
}

pub fn mcfg() -> Option<McfgEntry> {
    unsafe { MCFG }
}

pub fn lapic_base() -> u64 {
    unsafe { LAPIC_BASE }
}

pub fn slp_typa() -> u16 {
    unsafe { SLP_TYPA }
}

/// Power the machine off through PM1a/PM1b_CNT. Returns only if every method
/// failed.
pub fn poweroff() -> bool {
    let (pm1a, pm1b, slp) = unsafe { (PM1A_CNT, PM1B_CNT, SLP_TYPA) };
    let value = slp | (1 << 13); // SLP_EN

    if pm1a != 0 {
        unsafe { outw(pm1a, value) };
        for _ in 0..100_000 {
            unsafe { inb(0x80) };
        }
    }
    if pm1b != 0 {
        unsafe { outw(pm1b, value) };
    }

    // QEMU/Bochs ACPI poweroff ports, used when the firmware tables are
    // missing (for example under a bare Multiboot loader).
    unsafe {
        outw(0x604, 0x2000 | slp); // QEMU i440fx/q35
        outw(0xB004, 0x2000 | slp); // Bochs
        outw(0x4004, 0x3400 | slp); // VirtualBox
    }
    false
}

/// Reset the machine using the FADT reset register, then the usual fallbacks.
pub fn reboot() -> ! {
    let reset = unsafe { RESET_REG };
    let value = unsafe { RESET_VALUE };
    if reset.address != 0 {
        match reset.space {
            1 => unsafe {
                // I/O space.
                match reset.access_width {
                    1 => outb(reset.address as u16, value),
                    2 => outw(reset.address as u16, value as u16),
                    _ => outb(reset.address as u16, value),
                }
            },
            0 => unsafe {
                // Memory space.
                match reset.access_width {
                    1 => core::ptr::write_volatile(reset.address as *mut u8, value),
                    2 => core::ptr::write_volatile(reset.address as *mut u16, value as u16),
                    _ => core::ptr::write_volatile(reset.address as *mut u32, value as u32),
                }
            },
            _ => {}
        }
        for _ in 0..1_000_000 {
            unsafe { inb(0x80) };
        }
    }
    // Keyboard controller reset.
    unsafe {
        let mut status = inb(0x64);
        for _ in 0..100_000 {
            if status & 2 == 0 {
                break;
            }
            status = inb(0x64);
        }
        outb(0x64, 0xFE);
    }
    // PCI reset control.
    unsafe { outb(0xCF9, 0x06) };
    crate::arch::halt()
}
