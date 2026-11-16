//! Timekeeping and the dynamic (tickless) timer.
//!
//! The kernel keeps a monotonic nanosecond clock from the best source the
//! platform offers: invariant TSC, HPET, the ACPI PM timer or the PIT. Instead
//! of a fixed periodic interrupt, the local APIC timer is programmed as a
//! one-shot for the next event (a sleeping task's deadline or the current
//! scheduling slice). When nothing needs to happen the timer is disarmed and
//! the CPU can stay in a deep C-state.

use core::arch::asm;

use crate::arch::port::{inb, inl, outb};
use crate::arch::{apic, cpuid};

pub const TIMER_VECTOR: u8 = 0x40;
/// Scheduling slice used only when more than one task is runnable.
pub const SLICE_NS: u64 = 10_000_000;
pub const PIT_HZ: u64 = 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Clock {
    Tsc,
    Hpet,
    Pm,
    Pit,
}

static mut CLOCK: Clock = Clock::Pit;
static mut TSC_HZ: u64 = 0;
static mut HPET_BASE: u64 = 0;
static mut HPET_PERIOD_FS: u64 = 0;
static mut PM_PORT: u16 = 0;
static mut PM_MASK: u32 = 0;
static mut PM_LAST: u32 = 0;
static mut PM_ACC: u64 = 0;
static mut LAPIC_HZ: u64 = 0;
static mut USE_LAPIC: bool = false;
static mut TSC_DEADLINE: bool = false;
static mut READY: bool = false;
static mut PIT_TICKS: u64 = 0;

#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | lo as u64
}

#[inline]
fn tsc_ns(tsc: u64) -> u64 {
    let hz = unsafe { TSC_HZ };
    if hz == 0 {
        return 0;
    }
    ((tsc as u128 * 1_000_000_000u128) / hz as u128) as u64
}

#[inline]
fn tsc_delta_ns(delta: u64) -> u64 {
    let hz = unsafe { TSC_HZ };
    if hz == 0 {
        return 0;
    }
    ((delta as u128 * 1_000_000_000u128) / hz as u128) as u64
}

fn pm_read() -> u32 {
    let port = unsafe { PM_PORT };
    if port == 0 {
        return 0;
    }
    unsafe { inl(port) & PM_MASK }
}

fn pm_delta(start: u32) -> u32 {
    let mask = unsafe { PM_MASK };
    pm_read().wrapping_sub(start) & mask
}

fn pm_ns(ticks: u64) -> u64 {
    ((ticks as u128 * 1_000_000_000u128) / 3_579_545u128) as u64
}

fn hpet_counter() -> u64 {
    let base = unsafe { HPET_BASE };
    if base == 0 {
        return 0;
    }
    unsafe { core::ptr::read_volatile((base + 0xF0) as *const u64) }
}

fn hpet_ns(counter: u64) -> u64 {
    let period = unsafe { HPET_PERIOD_FS };
    if period == 0 {
        return 0;
    }
    ((counter as u128 * period as u128) / 1_000_000u128) as u64
}

fn hpet_wait_ns(ns: u64) {
    let start = hpet_counter();
    loop {
        let elapsed = hpet_counter().wrapping_sub(start);
        if hpet_ns(elapsed) >= ns {
            break;
        }
        core::hint::spin_loop();
    }
}

fn pm_wait_ns(ns: u64) {
    let start = pm_read();
    let want = ((ns as u128 * 3_579_545u128) / 1_000_000_000u128) as u32;
    while pm_delta(start) < want {
        core::hint::spin_loop();
    }
}

/// Channel 2 one-shot used only as a last-resort calibration reference.
fn pit_wait_10ms() {
    unsafe {
        // Gate channel 2 on, speaker off.
        let p61 = inb(0x61);
        outb(0x61, (p61 & !0x02) | 0x01);
        outb(0x43, 0xB0); // channel 2, mode 0, lo/hi
        let divisor: u16 = 11_932; // ~10 ms at 1.193182 MHz
        outb(0x42, divisor as u8);
        outb(0x42, (divisor >> 8) as u8);
        loop {
            outb(0x43, 0x80); // latch channel 2
            let lo = inb(0x42) as u16;
            let hi = inb(0x42) as u16;
            let value = (hi << 8) | lo;
            if value == 0 || value == 0xFFFF {
                break;
            }
            core::hint::spin_loop();
        }
        outb(0x61, p61);
    }
}

fn calibrate_tsc() -> Option<u64> {
    if unsafe { HPET_BASE } != 0 {
        let t0 = rdtsc();
        hpet_wait_ns(10_000_000);
        let t1 = rdtsc();
        let hz = ((t1 - t0) as u128 * 1_000_000_000u128 / 10_000_000u128) as u64;
        if hz > 1_000_000 {
            return Some(hz);
        }
    }
    if unsafe { PM_PORT } != 0 {
        let start = pm_read();
        let t0 = rdtsc();
        let want = 35_795u32; // 10 ms at 3.579545 MHz
        while pm_delta(start) < want {
            core::hint::spin_loop();
        }
        let t1 = rdtsc();
        let elapsed = pm_ns(pm_delta(start) as u64);
        if elapsed == 0 {
            return None;
        }
        let hz = ((t1 - t0) as u128 * 1_000_000_000u128 / elapsed as u128) as u64;
        if hz > 1_000_000 {
            return Some(hz);
        }
    }
    let t0 = rdtsc();
    pit_wait_10ms();
    let t1 = rdtsc();
    let hz = ((t1 - t0) as u128 * 1_000_000_000u128 / 10_000_000u128) as u64;
    if hz > 1_000_000 { Some(hz) } else { None }
}

fn calibrate_lapic() {
    apic::timer_setup();
    const COUNT: u32 = 100_000;
    apic::timer_oneshot(COUNT);
    let start = rdtsc();
    while apic::timer_current() != 0 {
        core::hint::spin_loop();
    }
    let elapsed = tsc_delta_ns(rdtsc().wrapping_sub(start));
    apic::timer_stop();
    if elapsed > 0 {
        unsafe {
            LAPIC_HZ = ((COUNT as u128 * 1_000_000_000u128) / elapsed as u128) as u64;
        }
    }
}

fn pit_init_periodic() {
    let divisor = (1_193_182 / PIT_HZ) as u16;
    unsafe {
        outb(0x43, 0x36);
        outb(0x40, (divisor & 0xFF) as u8);
        outb(0x40, (divisor >> 8) as u8);
    }
}

pub fn init() {
    if let Some(h) = crate::acpi::hpet() {
        unsafe {
            HPET_BASE = h.base;
            HPET_PERIOD_FS = h.period_fs;
        }
    }
    if let Some(p) = crate::acpi::pm_tmr() {
        unsafe {
            PM_PORT = p;
            PM_MASK = if crate::acpi::pm_tmr_32bit() {
                0xFFFF_FFFF
            } else {
                0x00FF_FFFF
            };
            PM_LAST = pm_read();
        }
    }

    if cpuid::has_tsc() {
        if let Some(hz) = calibrate_tsc() {
            unsafe {
                TSC_HZ = hz;
                CLOCK = Clock::Tsc;
            }
        }
    }
    if unsafe { CLOCK } == Clock::Pit && unsafe { HPET_BASE } != 0 {
        unsafe { CLOCK = Clock::Hpet };
    } else if unsafe { CLOCK } == Clock::Pit && unsafe { PM_PORT } != 0 {
        unsafe {
            CLOCK = Clock::Pm;
            PM_LAST = pm_read();
        }
    }

    // The local APIC timer is the only tick source that can be programmed
    // one-shot. Fall back to the PIT when the APIC is missing.
    if cpuid::has_apic() && apic::base() != 0 {
        unsafe { TSC_DEADLINE = cpuid::has_tsc_deadline() };
        apic::timer_setup();
        if unsafe { TSC_DEADLINE } {
            apic::timer_stop();
            unsafe { USE_LAPIC = true };
        } else if unsafe { CLOCK } == Clock::Tsc {
            calibrate_lapic();
            unsafe { USE_LAPIC = LAPIC_HZ != 0 };
        }
    }

    if unsafe { USE_LAPIC } {
        crate::arch::pic::mask(0);
    } else {
        pit_init_periodic();
        crate::arch::pic::unmask(0);
    }

    unsafe { READY = true };

    let clock = match unsafe { CLOCK } {
        Clock::Tsc => "tsc",
        Clock::Hpet => "hpet",
        Clock::Pm => "pm-timer",
        Clock::Pit => "pit",
    };
    crate::println!(
        "timer: clock={} tsc={}Hz lapic={}Hz tsc-deadline={} tickless={}",
        clock,
        unsafe { TSC_HZ },
        unsafe { LAPIC_HZ },
        unsafe { TSC_DEADLINE },
        unsafe { USE_LAPIC }
    );
}

/// Monotonic nanoseconds since boot.
pub fn now_ns() -> u64 {
    unsafe {
        match CLOCK {
            Clock::Tsc => tsc_ns(rdtsc()),
            Clock::Hpet => hpet_ns(hpet_counter()),
            Clock::Pm => {
                let v = pm_read();
                let delta = v.wrapping_sub(PM_LAST) & PM_MASK;
                if delta != 0 {
                    PM_ACC = PM_ACC.wrapping_add(delta as u64);
                    PM_LAST = v;
                }
                pm_ns(PM_ACC)
            }
            Clock::Pit => core::ptr::read_volatile(&raw const PIT_TICKS) * 1_000_000,
        }
    }
}

/// Milliseconds since boot. Kept as the public `ticks()` name because the
/// syscall ABI and the scheduler's grace periods use it.
pub fn ticks() -> u64 {
    now_ns() / 1_000_000
}

pub fn clock_name() -> &'static str {
    match unsafe { CLOCK } {
        Clock::Tsc => "tsc",
        Clock::Hpet => "hpet",
        Clock::Pm => "pm-timer",
        Clock::Pit => "pit",
    }
}

pub fn tsc_hz() -> u64 {
    unsafe { TSC_HZ }
}

pub fn lapic_hz() -> u64 {
    unsafe { LAPIC_HZ }
}

pub fn tickless() -> bool {
    unsafe { USE_LAPIC }
}

fn arm_oneshot(delta_ns: u64) {
    let delta = delta_ns.clamp(1_000, 1_000_000_000);
    if unsafe { TSC_DEADLINE } && unsafe { TSC_HZ } != 0 {
        let ticks = ((delta as u128 * unsafe { TSC_HZ } as u128) / 1_000_000_000u128) as u64;
        apic::set_tsc_deadline(rdtsc().wrapping_add(ticks.max(1)));
    } else if unsafe { LAPIC_HZ } != 0 {
        let count =
            ((delta as u128 * unsafe { LAPIC_HZ } as u128) / 1_000_000_000u128) as u32;
        apic::timer_oneshot(count.max(1));
    }
}

/// Force a timer interrupt soon. Used when a task blocks so the scheduler
/// does not have to wait for the next scheduling slice.
pub fn kick() {
    if unsafe { USE_LAPIC } {
        arm_oneshot(50_000);
    }
}

/// Program this CPU's one-shot timer for the next global event.
pub fn rearm() {
    if !unsafe { READY } || !unsafe { USE_LAPIC } {
        return;
    }
    let now = now_ns();
    match crate::sched::next_event_ns(now) {
        Some(deadline) => arm_oneshot(deadline.saturating_sub(now).max(1_000)),
        None => apic::timer_stop(),
    }
}

/// Called from the local-APIC timer vector.
pub fn on_interrupt() {
    apic::eoi();
    let now = now_ns();
    crate::sched::on_timer(now);
    crate::sched::rearm();
}

/// Called from the legacy PIT vector when the LAPIC timer is unavailable.
pub fn tick() {
    unsafe { PIT_TICKS = PIT_TICKS.wrapping_add(1) };
    let now = now_ns();
    crate::sched::on_timer(now);
}
