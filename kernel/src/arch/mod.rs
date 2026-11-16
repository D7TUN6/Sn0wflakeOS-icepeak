pub mod apic;
pub mod context;
pub mod cpu;
pub mod cpufreq;
pub mod cpuid;
pub mod fpu;
pub mod gdt;
pub mod idt;
pub mod interrupts;
pub mod pic;
pub mod port;
pub mod power;
pub mod timer;

pub use interrupts::{disable, init};

pub fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

pub fn wait_for_interrupt() {
    unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
}
