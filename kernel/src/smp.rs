//! Application-processor bring-up. We copy a position-dependent trampoline to
//! physical 0x8000, fill a small mailbox, and wake each AP with INIT/SIPI.
//! No external SMP crate is involved.

use core::sync::atomic::Ordering;

use crate::arch::{apic, cpu, fpu, gdt, idt};
use crate::mm::paging;
use crate::sched;

const TRAMPOLINE_ADDR: u64 = 0x8000;
const MAILBOX_ADDR: u64 = 0x8F00;

/// Assembled by build.sh from boot2/ap_trampoline.asm (`org 0x8000`).
const TRAMPOLINE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../build/ap_trampoline.bin"
));

pub fn bringup() {
    let bsp = apic::bsp_id();
    cpu::mark_online(0, bsp);

    let total = apic::count();
    let mut index = 1usize;
    for i in 0..total {
        if index >= apic::MAX_CPUS {
            break;
        }
        let apic_id = apic::apic_id_of(i);
        if apic_id == bsp {
            continue;
        }
        start_ap(index, apic_id);
        index += 1;
    }

    crate::println!(
        "smp: {} cpu(s) online, {} in MADT",
        cpu::online_count(),
        total
    );
}

fn start_ap(index: usize, apic_id: u32) -> bool {
    let Some(stack_top) = sched::alloc_idle_stack() else {
        crate::println!("smp: no stack for cpu {}", index);
        return false;
    };
    let Some(idle_fpu) = fpu::alloc_area() else {
        crate::println!("smp: no FPU area for cpu {}", index);
        return false;
    };

    cpu::prepare(index, apic_id);
    {
        let c = cpu::get(index);
        c.idle_stack_top = stack_top;
        c.idle_fpu = idle_fpu;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(
            TRAMPOLINE.as_ptr(),
            TRAMPOLINE_ADDR as *mut u8,
            TRAMPOLINE.len(),
        );
        let mb = MAILBOX_ADDR as *mut u8;
        core::ptr::write_unaligned(mb as *mut u32, paging::read_cr3() as u32);
        core::ptr::write_unaligned(mb.add(8) as *mut u64, stack_top);
        core::ptr::write_unaligned(mb.add(16) as *mut u64, index as u64);
        core::ptr::write_unaligned(mb.add(24) as *mut u64, ap_entry as usize as u64);
    }
    core::sync::atomic::fence(Ordering::SeqCst);

    apic::send_init_sipi(apic_id, (TRAMPOLINE_ADDR >> 12) as u8);

    for _ in 0..500 {
        if cpu::is_online(index) {
            return true;
        }
        apic::io_delay(2_000);
    }

    crate::println!("smp: cpu {} (apic id {}) did not start", index, apic_id);
    false
}

/// First Rust code executed by an application processor.
#[unsafe(no_mangle)]
pub extern "C" fn ap_entry(index: u64) -> ! {
    let idx = index as usize;
    apic::init_ap();
    let apic_id = apic::id();

    // Loading the kernel GDT reloads the GS selector, which resets the GS base
    // in 64-bit mode, so attach the per-CPU block afterwards.
    gdt::init_cpu(idx);
    gdt::set_kernel_stack(idx, cpu::get(idx).idle_stack_top);
    idt::load();
    fpu::init();
    cpu::attach(idx);
    fpu::set_current(cpu::get(idx).idle_fpu);

    cpu::get(idx).apic_id = apic_id as u64;
    cpu::get(idx).online = 1;
    crate::println!("smp: cpu {} online (apic id {})", idx, apic_id);

    loop {
        crate::arch::power::idle();
    }
}
