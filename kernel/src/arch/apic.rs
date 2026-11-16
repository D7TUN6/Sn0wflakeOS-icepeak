//! Local APIC and ACPI MADT handling. We parse the MADT ourselves for the
//! processor list and the local-APIC MMIO base, then use the APIC for IPIs.

use core::arch::asm;

use crate::acpi;

pub const MAX_CPUS: usize = 8;
pub const IPI_RESCHED: u8 = 0xF1;

const LAPIC_DEFAULT: u64 = 0xFEE0_0000;
const REG_ID: u32 = 0x20;
const REG_EOI: u32 = 0xB0;
const REG_SVR: u32 = 0xF0;
const REG_ICR_LOW: u32 = 0x300;
const REG_ICR_HIGH: u32 = 0x310;
const REG_TPR: u32 = 0x80;
const REG_LVT_TIMER: u32 = 0x320;
const REG_TIMER_INIT: u32 = 0x380;
const REG_TIMER_CUR: u32 = 0x390;
const REG_TIMER_DIV: u32 = 0x3E0;

static mut LAPIC_BASE: u64 = LAPIC_DEFAULT;
static mut BSP_ID: u32 = 0;
static mut IDS: [u32; MAX_CPUS] = [0; MAX_CPUS];
static mut COUNT: usize = 0;

pub fn read_msr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | lo as u64
}

pub fn write_msr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nomem, nostack)
        );
    }
}

pub fn base() -> u64 {
    unsafe { LAPIC_BASE }
}

pub fn read(offset: u32) -> u32 {
    unsafe { core::ptr::read_volatile((LAPIC_BASE + offset as u64) as *const u32) }
}

pub fn write(offset: u32, value: u32) {
    unsafe { core::ptr::write_volatile((LAPIC_BASE + offset as u64) as *mut u32, value) }
}

pub fn id() -> u32 {
    read(REG_ID) >> 24
}

pub fn bsp_id() -> u32 {
    unsafe { BSP_ID }
}

pub fn count() -> usize {
    unsafe { COUNT }
}

pub fn apic_id_of(index: usize) -> u32 {
    unsafe { IDS[index] }
}

/// Parse the MADT, remember the local APIC base and the enabled CPU list.
pub fn init() {
    let Some(rsdp) = acpi::rsdp() else {
        crate::println!("smp: no RSDP, staying uniprocessor");
        return;
    };
    let Some(madt) = acpi::find_table(rsdp, b"APIC") else {
        crate::println!("smp: no MADT, staying uniprocessor");
        return;
    };

    let length = acpi::phys_u32(madt + 4) as u64;
    if length < 44 || length > 0x10_0000 {
        crate::println!("smp: bad MADT length {}", length);
        return;
    }

    unsafe {
        LAPIC_BASE = acpi::phys_u32(madt + 36) as u64;
        COUNT = 0;
    }

    let mut p = madt + 44;
    let end = madt + length;
    while p + 2 <= end {
        let kind = acpi::phys_u8(p);
        let elen = acpi::phys_u8(p + 1) as u64;
        if elen < 2 || p + elen > end {
            break;
        }
        match kind {
            0 => {
                let apic_id = acpi::phys_u8(p + 3);
                let flags = acpi::phys_u32(p + 4);
                if flags & 1 != 0 {
                    unsafe {
                        if (COUNT as usize) < MAX_CPUS {
                            IDS[COUNT] = apic_id as u32;
                            COUNT += 1;
                        }
                    }
                }
            }
            5 => {
                unsafe { LAPIC_BASE = acpi::phys_u64(p + 4) };
            }
            9 => {
                let apic_id = acpi::phys_u32(p + 4);
                let flags = acpi::phys_u32(p + 8);
                if flags & 1 != 0 {
                    unsafe {
                        if (COUNT as usize) < MAX_CPUS {
                            IDS[COUNT] = apic_id;
                            COUNT += 1;
                        }
                    }
                }
            }
            _ => {}
        }
        p += elen;
    }

    // Enable the local APIC through the APIC_BASE MSR and the spurious
    // interrupt vector register.
    let apic_base_msr = read_msr(0x1B);
    write_msr(0x1B, (apic_base_msr & 0xFFFF_FFFF_FFFF_F000) | (1 << 11));
    write(REG_TPR, 0);
    write(REG_SVR, 0x1FF); // enable, spurious vector 0xFF
    write(REG_EOI, 0);

    unsafe { BSP_ID = id() };
    crate::println!(
        "smp: {} cpu(s) in MADT, bsp apic id {}, lapic base {:#x}",
        count(),
        bsp_id(),
        base()
    );
}

pub fn eoi() {
    write(REG_EOI, 0);
}

/// Configure the local-APIC timer as an unmasked one-shot with divide-by-16.
pub fn timer_setup() {
    write(REG_TIMER_DIV, 0x3);
    write(REG_LVT_TIMER, crate::arch::timer::TIMER_VECTOR as u32);
}

pub fn timer_oneshot(count: u32) {
    write(REG_TIMER_INIT, count);
}

pub fn timer_stop() {
    write(REG_TIMER_INIT, 0);
}

pub fn timer_current() -> u32 {
    read(REG_TIMER_CUR)
}

/// IA32_TSC_DEADLINE: absolute TSC value at which the timer fires.
pub fn set_tsc_deadline(value: u64) {
    write_msr(0x6E0, value);
}

/// Enable the local APIC on the CPU that is executing this call.
pub fn init_ap() {
    let base = read_msr(0x1B) & 0xFFFF_FFFF_FFFF_F000;
    write_msr(0x1B, base | (1 << 11));
    write(REG_TPR, 0);
    write(REG_SVR, 0x1FF);
    write(REG_EOI, 0);
    timer_setup();
}

fn wait_delivery() {
    for _ in 0..1_000_000 {
        if read(REG_ICR_LOW) & (1 << 12) == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

fn send_ipi_raw(dest: u32, low: u32) {
    write(REG_ICR_HIGH, dest << 24);
    write(REG_ICR_LOW, low);
    wait_delivery();
}

/// Fixed-delivery IPI to one CPU.
pub fn send_ipi(apic_id: u32, vector: u8) {
    send_ipi_raw(apic_id, vector as u32);
}

/// Fixed-delivery IPI to every other CPU.
pub fn broadcast_ipi(vector: u8) {
    send_ipi_raw(0, vector as u32 | (3 << 18));
}

/// INIT IPI followed by a startup IPI; `vector` is the trampoline page number.
pub fn send_init_sipi(apic_id: u32, vector: u8) {
    send_ipi_raw(apic_id, (5 << 8) | (1 << 14)); // INIT, level assert
    io_delay(10_000);
    send_ipi_raw(apic_id, (5 << 8) | (1 << 15)); // INIT, level deassert
    io_delay(10_000);
    send_ipi_raw(apic_id, (6 << 8) | vector as u32); // SIPI
    io_delay(500);
    send_ipi_raw(apic_id, (6 << 8) | vector as u32); // second SIPI
    io_delay(500);
}

pub fn io_delay(loops: u32) {
    for _ in 0..loops {
        unsafe { crate::arch::port::inb(0x80) };
    }
}
