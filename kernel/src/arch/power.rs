//! Processor idle states. C1 uses `hlt` or `mwait`; C2/C3 use the legacy ACPI
//! PM2_CNT / PM_TMR method when the FADT advertises usable exit latencies.
//! The next timer deadline gates how deep we are allowed to go.

use core::arch::asm;

use crate::acpi;
use crate::arch::apic::MAX_CPUS;
use crate::arch::port::{inb, outb};
use crate::arch::{cpu, cpuid, timer};

static mut MWAIT_FLAGS: [u64; MAX_CPUS] = [0; MAX_CPUS];
static mut MWAIT_OK: bool = false;
static mut LAST_STATE: u8 = 0;

pub fn init() {
    unsafe { MWAIT_OK = cpuid::has_mwait() };
    let c3 = acpi::c3_latency_us() != 0xFFFF && acpi::pm2_cnt().is_some();
    crate::println!(
        "power: mwait={} C2={} C3={}",
        unsafe { MWAIT_OK },
        acpi::pm2_cnt().is_some(),
        c3
    );
}

/// Publish a wake event for the monitor/mwait window. Called from the common
/// interrupt dispatcher so an interrupt that arrives between `sti` and `mwait`
/// cannot be lost.
pub fn notify() {
    let i = cpu::index().min(MAX_CPUS - 1);
    unsafe {
        let p = (&raw mut MWAIT_FLAGS) as *mut u64;
        let v = core::ptr::read_volatile(p.add(i));
        core::ptr::write_volatile(p.add(i), v.wrapping_add(1));
    }
}

fn arb_dis(enable: bool) {
    if let Some(port) = acpi::pm2_cnt() {
        unsafe {
            let v = inb(port);
            outb(port, if enable { v | 1 } else { v & !1 });
        }
    }
}

fn halt() {
    unsafe { asm!("sti", "hlt", options(nomem, nostack)) };
}

fn halt_or_mwait() {
    if unsafe { MWAIT_OK } {
        let i = cpu::index().min(MAX_CPUS - 1);
        let addr = unsafe { ((&raw mut MWAIT_FLAGS) as *mut u64).add(i) } as u64;
        unsafe {
            // MONITOR / MWAIT. A pending interrupt writes the monitored word
            // in `notify`, which makes the following MWAIT return immediately.
            asm!("monitor", in("rax") addr, in("ecx") 0u32, in("edx") 0u32, options(nostack));
            asm!("sti", options(nomem, nostack));
            asm!("mwait", in("eax") 0u32, in("ecx") 0u32, options(nostack));
        }
    } else {
        halt();
    }
}

/// Pick the deepest C-state whose exit latency fits in the time until the
/// next scheduled event.
fn choose(budget_ns: u64) -> u8 {
    let c3 = acpi::c3_latency_us();
    if c3 != 0xFFFF && acpi::pm2_cnt().is_some() && acpi::pm_tmr().is_some() {
        let need = (c3 as u64 + 100) * 1_000;
        if budget_ns == u64::MAX || budget_ns > need {
            return 3;
        }
    }
    let c2 = acpi::c2_latency_us();
    if c2 != 0xFFFF && acpi::pm2_cnt().is_some() {
        let need = (c2 as u64 + 100) * 1_000;
        if budget_ns == u64::MAX || budget_ns > need {
            return 2;
        }
    }
    1
}

/// Enter an idle state until the next interrupt. Returns after the wakeup.
pub fn idle() {
    timer::rearm();
    let now = timer::now_ns();
    let budget = crate::sched::next_event_ns(now)
        .map(|d| d.saturating_sub(now))
        .unwrap_or(u64::MAX);
    let state = choose(budget);
    unsafe { LAST_STATE = state };
    match state {
        3 => {
            arb_dis(true);
            if let Some(port) = acpi::pm_tmr() {
                unsafe { inb(port) };
            }
            halt();
            arb_dis(false);
        }
        2 => {
            if let Some(port) = acpi::pm2_cnt() {
                unsafe { inb(port) };
            }
            halt();
        }
        _ => halt_or_mwait(),
    }
}

pub fn last_state() -> u8 {
    unsafe { LAST_STATE }
}
