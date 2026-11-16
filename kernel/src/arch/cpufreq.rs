//! CPU frequency scaling and Linux-style governors.
//!
//! `schedutil` is the default. It is driven by scheduler events (not a
//! periodic ondemand timer): every context switch feeds a short utilization
//! window, and the target P-state is updated at most once per 10 ms.
//!
//! Hardware back ends:
//! - Intel HWP (`IA32_HWP_REQUEST`) on modern parts,
//! - Intel EIST (`IA32_PERF_CTL`) on older parts,
//! - AMD CPPC (`MSR_AMD_CPPC_REQ`) when CPUID advertises it,
//! - AMD hardware P-states (`MSR_AMD_PSTATE_CTL`) otherwise.

use crate::arch::apic::{read_msr, write_msr};
use crate::arch::{cpuid, timer};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Governor {
    Performance,
    Powersave,
    Schedutil,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Driver {
    None,
    IntelHwp,
    IntelEist,
    AmdCppc,
    AmdPstate,
    Acpi,
}

static mut GOV: Governor = Governor::Schedutil;
static mut DRIVER: Driver = Driver::None;
static mut MIN_KHZ: u32 = 0;
static mut MAX_KHZ: u32 = 0;
static mut CUR_KHZ: u32 = 0;
static mut MIN_LEVEL: u32 = 0;
static mut MAX_LEVEL: u32 = 0;
static mut CUR_LEVEL: u32 = 0;
static mut AMD_STATES: u32 = 0;
static mut UTIL_AVG: u32 = 512;
static mut SAMPLE_BUSY_NS: u64 = 0;
static mut SAMPLE_TOTAL_NS: u64 = 0;
static mut LAST_APPLY_NS: u64 = 0;

pub const GOVERNORS: [&str; 3] = ["performance", "powersave", "schedutil"];

fn governor_name(g: Governor) -> &'static str {
    match g {
        Governor::Performance => "performance",
        Governor::Powersave => "powersave",
        Governor::Schedutil => "schedutil",
    }
}

fn driver_name(d: Driver) -> &'static str {
    match d {
        Driver::None => "none",
        Driver::IntelHwp => "intel_pstate",
        Driver::IntelEist => "acpi-cpufreq",
        Driver::AmdCppc => "amd-cppc",
        Driver::AmdPstate => "amd-pstate",
        Driver::Acpi => "acpi-cpufreq",
    }
}

pub fn governor() -> &'static str {
    governor_name(unsafe { GOV })
}

pub fn driver() -> &'static str {
    driver_name(unsafe { DRIVER })
}

pub fn min_khz() -> u32 {
    unsafe { MIN_KHZ }
}

pub fn max_khz() -> u32 {
    unsafe { MAX_KHZ }
}

pub fn cur_khz() -> u32 {
    unsafe { CUR_KHZ }
}

pub fn utilization() -> u32 {
    unsafe { UTIL_AVG }
}

fn scale_khz(util: u32) -> u32 {
    let min = unsafe { MIN_KHZ };
    let max = unsafe { MAX_KHZ };
    if max <= min {
        return max;
    }
    min + (((max - min) as u64 * util.min(1024) as u64) / 1024) as u32
}

fn scale_level(util: u32) -> u32 {
    let min = unsafe { MIN_LEVEL };
    let max = unsafe { MAX_LEVEL };
    if max <= min {
        return max;
    }
    min + (((max - min) as u64 * util.min(1024) as u64) / 1024) as u32
}

fn apply() {
    let gov = unsafe { GOV };
    let util = match gov {
        Governor::Performance => 1024,
        Governor::Powersave => 0,
        Governor::Schedutil => unsafe { UTIL_AVG },
    };
    let khz = scale_khz(util);
    let level = scale_level(util);
    unsafe {
        CUR_KHZ = khz;
        CUR_LEVEL = level;
    }
    match unsafe { DRIVER } {
        Driver::IntelHwp => {
            // min 7:0, max 15:8, desired 23:16, EPP 31:24.
            let epp: u32 = match gov {
                Governor::Performance => 0x00,
                Governor::Powersave => 0xFF,
                Governor::Schedutil => 0x80,
            };
            let request = (unsafe { MIN_LEVEL } & 0xFF)
                | ((unsafe { MAX_LEVEL } & 0xFF) << 8)
                | ((level & 0xFF) << 16)
                | (epp << 24);
            write_msr(0x774, request as u64);
        }
        Driver::IntelEist => {
            // IA32_PERF_CTL bits 15:8 select the P-state ratio.
            write_msr(0x199, ((level & 0xFF) << 8) as u64);
        }
        Driver::AmdCppc => {
            let epp: u32 = match gov {
                Governor::Performance => 0x00,
                Governor::Powersave => 0xFF,
                Governor::Schedutil => 0x80,
            };
            let request = (unsafe { MIN_LEVEL } & 0xFF)
                | ((unsafe { MAX_LEVEL } & 0xFF) << 8)
                | ((level & 0xFF) << 16)
                | (epp << 24);
            write_msr(0xC001_02B3, request as u64);
        }
        Driver::AmdPstate => {
            let states = unsafe { AMD_STATES };
            let index = if states <= 1 {
                0
            } else {
                (((states - 1) as u64 * (1024 - util.min(1024)) as u64) / 1024) as u32
            };
            write_msr(0xC001_0062, index as u64);
        }
        Driver::None | Driver::Acpi => {}
    }
}

/// Base frequency in MHz. CPUID leaf 0x16 is not available everywhere (QEMU
/// leaves it at zero), so fall back to the measured TSC rate.
fn base_mhz() -> u32 {
    if let Some(m) = cpuid::base_freq_mhz() {
        return m;
    }
    let tsc = timer::tsc_hz();
    if tsc > 0 {
        return ((tsc / 1_000_000) as u32).max(1);
    }
    1000
}

fn setup_intel_hwp() {
    let cap = read_msr(0x771);
    let highest = (cap & 0xFF) as u32;
    let lowest = ((cap >> 24) & 0xFF) as u32;
    let base = base_mhz();
    unsafe {
        DRIVER = Driver::IntelHwp;
        MIN_LEVEL = lowest.min(highest);
        MAX_LEVEL = highest;
        CUR_LEVEL = highest;
        MIN_KHZ = (base * 1000).saturating_mul(lowest) / 255;
        MAX_KHZ = (base * 1000).saturating_mul(highest) / 255;
        if MAX_KHZ == 0 {
            MAX_KHZ = base * 1000;
        }
        if MIN_KHZ == 0 {
            MIN_KHZ = MAX_KHZ / 2;
        }
    }
    let enable = read_msr(0x770);
    write_msr(0x770, enable | 1);
    apply();
}

fn setup_intel_eist() {
    let platform = read_msr(0xCE);
    let mut max_ratio = ((platform >> 8) & 0xFF) as u32;
    let mut min_ratio = ((platform >> 40) & 0xFF) as u32;
    if max_ratio == 0 {
        max_ratio = 20;
    }
    if min_ratio == 0 || min_ratio > max_ratio {
        min_ratio = max_ratio / 2;
    }
    // Make sure EIST is not disabled by firmware.
    let misc = read_msr(0x1A0);
    if misc & (1 << 16) != 0 {
        write_msr(0x1A0, misc & !(1u64 << 16));
    }
    unsafe {
        DRIVER = Driver::IntelEist;
        MIN_LEVEL = min_ratio;
        MAX_LEVEL = max_ratio;
        CUR_LEVEL = max_ratio;
        MIN_KHZ = min_ratio * 100_000;
        MAX_KHZ = max_ratio * 100_000;
    }
    apply();
}

fn setup_amd_cppc() {
    let base = base_mhz();
    unsafe {
        DRIVER = Driver::AmdCppc;
        MIN_LEVEL = 0;
        MAX_LEVEL = 255;
        CUR_LEVEL = 255;
        MIN_KHZ = base * 1000 / 2;
        MAX_KHZ = base * 1000;
    }
    let enable = read_msr(0xC001_02B1);
    write_msr(0xC001_02B1, enable | 1);
    apply();
}

fn setup_amd_pstate() {
    let mut count = 0u32;
    for i in 0..8u32 {
        let def = read_msr(0xC001_0064 + i);
        if def & (1u64 << 63) == 0 {
            break;
        }
        count += 1;
    }
    if count == 0 {
        return;
    }
    let base = base_mhz();
    unsafe {
        DRIVER = Driver::AmdPstate;
        AMD_STATES = count;
        MIN_LEVEL = count - 1;
        MAX_LEVEL = 0;
        CUR_LEVEL = 0;
        MIN_KHZ = base * 1000 / 2;
        MAX_KHZ = base * 1000;
    }
    apply();
}

pub fn init() {
    let vendor = cpuid::vendor();
    if vendor == *b"GenuineIntel" {
        if cpuid::has_hwp() {
            setup_intel_hwp();
        } else if cpuid::has_eist() {
            setup_intel_eist();
        }
    } else if vendor == *b"AuthenticAMD" || vendor == *b"HygonGenuine" {
        if cpuid::has_cppc() {
            setup_amd_cppc();
        } else if cpuid::has_hw_pstate() {
            setup_amd_pstate();
        }
    }
    if unsafe { DRIVER } == Driver::None {
        let base = base_mhz();
        unsafe {
            DRIVER = Driver::Acpi;
            MIN_KHZ = base * 1000 / 2;
            MAX_KHZ = base * 1000;
            CUR_KHZ = MAX_KHZ;
        }
    }
    unsafe { LAST_APPLY_NS = timer::now_ns() };
    crate::println!(
        "cpufreq: driver={} governor={} {}..{} kHz",
        driver(),
        governor(),
        min_khz(),
        max_khz()
    );
}

/// Feed one scheduling observation. `busy_ns` is the time the CPU spent
/// running a task; `total_ns` is the elapsed wall time. Updates are rate
/// limited to 10 ms, matching schedutil's default rate limit.
pub fn account(busy_ns: u64, total_ns: u64, now_ns: u64) {
    unsafe {
        SAMPLE_BUSY_NS = SAMPLE_BUSY_NS.saturating_add(busy_ns);
        SAMPLE_TOTAL_NS = SAMPLE_TOTAL_NS.saturating_add(total_ns);
        if now_ns.saturating_sub(LAST_APPLY_NS) < 10_000_000 {
            return;
        }
        if SAMPLE_TOTAL_NS > 0 {
            let util = ((SAMPLE_BUSY_NS as u128 * 1024) / SAMPLE_TOTAL_NS as u128)
                .min(1024) as u32;
            UTIL_AVG = (UTIL_AVG * 3 + util) / 4;
        }
        SAMPLE_BUSY_NS = 0;
        SAMPLE_TOTAL_NS = 0;
        LAST_APPLY_NS = now_ns;
    }
    if unsafe { GOV } == Governor::Schedutil {
        apply();
    }
}

pub fn set_governor(name: &str) -> bool {
    let gov = match name {
        "performance" => Governor::Performance,
        "powersave" => Governor::Powersave,
        "schedutil" => Governor::Schedutil,
        _ => return false,
    };
    unsafe { GOV = gov };
    apply();
    true
}

pub fn info_text() -> alloc::string::String {
    use core::fmt::Write;
    let mut out = alloc::string::String::new();
    let _ = write!(
        out,
        "driver {}\ngovernor {}\nmin {}\nmax {}\ncur {}\nutil {}\nlevels {} {}\navailable performance powersave schedutil\n",
        driver(),
        governor(),
        min_khz(),
        max_khz(),
        cur_khz(),
        utilization(),
        unsafe { MIN_LEVEL },
        unsafe { MAX_LEVEL }
    );
    out
}
