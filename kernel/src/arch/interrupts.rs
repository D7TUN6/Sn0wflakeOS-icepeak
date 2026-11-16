use core::arch::asm;

use super::idt::InterruptFrame;
use super::{apic, cpu, gdt, idt, pic, power, timer};

pub fn init() {
    super::fpu::init();
    // The boot GDT already has 64-bit kernel code/data descriptors, so the IDT
    // can be installed first. That way a fault inside GDT/TSS setup is visible.
    idt::init();
    gdt::init();
    // Publish this CPU's per-CPU area before interrupts are ever enabled.
    cpu::init(0, 0);
    pic::remap();
    timer::init();
    crate::drivers::init_all();

    // Timer, keyboard and the UART receive line.
    pic::mask_all();
    pic::unmask(0);
    pic::unmask(1);
    pic::unmask(4);

    unsafe { asm!("sti", options(nomem, nostack)) };
}

pub fn enable() {
    unsafe { asm!("sti", options(nomem, nostack)) };
}

pub fn disable() {
    unsafe { asm!("cli", options(nomem, nostack)) };
}

pub fn are_enabled() -> bool {
    let flags: u64;
    unsafe { asm!("pushfq", "pop {}", out(reg) flags, options(nomem, nostack)) };
    flags & (1 << 9) != 0
}

#[unsafe(no_mangle)]
pub extern "C" fn isr_dispatch(frame: *mut InterruptFrame) -> *mut InterruptFrame {
    // Publish the wakeup before any handler runs so an interrupt arriving in
    // the window between `sti` and `mwait` cannot be lost.
    power::notify();
    let frame_ref = unsafe { &mut *frame };
    let vector = frame_ref.vector as usize;

    if vector == idt::SYSCALL_VECTOR as usize {
        return crate::syscall::dispatch(frame_ref);
    }

    if vector == timer::TIMER_VECTOR as usize {
        timer::on_interrupt();
        if crate::sched::need_resched() && crate::sched::can_preempt(frame_ref) {
            return crate::sched::schedule(frame);
        }
        return frame;
    }

    // Local-APIC reschedule IPI and the spurious vector.
    if vector == apic::IPI_RESCHED as usize {
        apic::eoi();
        if crate::sched::can_preempt(frame_ref) {
            return crate::sched::schedule(frame);
        }
        return frame;
    }
    if vector == 0xFF {
        return frame;
    }

    if vector < 32 {
        // A fault raised by a user task kills only that task. The kernel used
        // to halt the whole machine, so one bad pointer in a coreutil took
        // everything down with it.
        if frame_ref.cs & 3 == 3 {
            return crate::sched::fault_current(frame, vector);
        }
        exception(frame_ref);
    }

    if pic::on_frame(frame_ref) {
        if crate::sched::need_resched() && crate::sched::can_preempt(frame_ref) {
            return crate::sched::schedule(frame);
        }
        return frame;
    }

    crate::println!("unhandled interrupt vector {}", vector);
    frame
}

pub fn exception_name(vector: usize) -> &'static str {
    const NAMES: [&str; 21] = [
        "divide error",
        "debug",
        "non-maskable interrupt",
        "breakpoint",
        "overflow",
        "bound range",
        "invalid opcode",
        "device not available",
        "double fault",
        "coprocessor segment",
        "invalid TSS",
        "segment not present",
        "stack fault",
        "general protection fault",
        "page fault",
        "reserved",
        "x87 floating point",
        "alignment check",
        "machine check",
        "SIMD floating point",
        "virtualization",
    ];
    NAMES.get(vector).copied().unwrap_or("unknown exception")
}

fn exception(frame: &InterruptFrame) -> ! {
    let name = exception_name(frame.vector as usize);

    crate::println!("\n*** CPU EXCEPTION {}: {} ***", frame.vector, name);
    crate::println!(
        "cpu={} rip={:#018x} cs={:#x} rflags={:#x} rsp={:#x} ss={:#x} err={:#x}",
        crate::arch::cpu::index(),
        frame.rip,
        frame.cs,
        frame.rflags,
        frame.rsp,
        frame.ss,
        frame.error
    );
    if frame.vector == 13 {
        crate::println!(
            "regs rax={:#x} rbx={:#x} rcx={:#x} rdx={:#x} rsi={:#x} rdi={:#x} rbp={:#x}",
            frame.rax,
            frame.rbx,
            frame.rcx,
            frame.rdx,
            frame.rsi,
            frame.rdi,
            frame.rbp
        );
        let words = unsafe { core::slice::from_raw_parts(frame.rsp as *const u64, 6) };
        crate::println!(
            "iretq stack: {:016x} {:016x} {:016x} {:016x} {:016x} {:016x}",
            words[0],
            words[1],
            words[2],
            words[3],
            words[4],
            words[5]
        );
    }
    if frame.vector == 14 {
        let cr2: u64;
        unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)) };
        crate::println!("cr2={:#018x}", cr2);
        if frame.cs & 3 == 3 {
            let cr3 = crate::mm::paging::read_cr3();
            let stack_page = frame.rsp & !0xFFF;
            let phys = crate::mm::paging::translate(cr3, stack_page);
            crate::println!("user cr3={:#x} rsp_page={:#x} phys={:?}", cr3, stack_page, phys);
            if let Some(p) = crate::mm::paging::translate(cr3, frame.rsp) {
                let vals = unsafe { core::slice::from_raw_parts(p as *const u64, 4) };
                crate::println!("stack: {:016x} {:016x} {:016x} {:016x}", vals[0], vals[1], vals[2], vals[3]);
            }
        }
    }
    crate::arch::halt()
}
