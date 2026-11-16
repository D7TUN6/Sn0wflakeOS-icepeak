use core::arch::asm;

use super::gdt;

pub const SYSCALL_VECTOR: u8 = 0x80;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    flags: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry {
    offset_low: 0,
    selector: 0,
    ist: 0,
    flags: 0,
    offset_mid: 0,
    offset_high: 0,
    zero: 0,
}; 256];

unsafe extern "C" {
    static isr_stub_table: [u64; 256];
}

fn entry(handler: u64, dpl: u8, ist: u8) -> IdtEntry {
    IdtEntry {
        offset_low: (handler & 0xFFFF) as u16,
        selector: gdt::KERNEL_CODE,
        ist,
        flags: 0x8E | (dpl << 5),
        offset_mid: ((handler >> 16) & 0xFFFF) as u16,
        offset_high: (handler >> 32) as u32,
        zero: 0,
    }
}

pub fn load() {
    unsafe {
        let table = &*(&raw const IDT);
        let ptr = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: table.as_ptr() as u64,
        };
        asm!("lidt [{}]", in(reg) &ptr, options(readonly, nostack, preserves_flags));
    }
}

pub fn init() {
    unsafe {
        let table = &mut *(&raw mut IDT);
        let stubs = &isr_stub_table;
        for i in 0..256 {
            let ist = if i == 8 { 1 } else { 0 };
            table[i] = entry(stubs[i], 0, ist);
        }
        table[SYSCALL_VECTOR as usize] = entry(stubs[SYSCALL_VECTOR as usize], 3, 0);
        table[3] = entry(stubs[3], 3, 0); // int3 from user

        let ptr = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: table.as_ptr() as u64,
        };
        asm!("lidt [{}]", in(reg) &ptr, options(readonly, nostack, preserves_flags));
    }
}

#[repr(C)]
pub struct InterruptFrame {
    // Layout must match boot/interrupts.asm: registers are pushed rax..r15,
    // so r15 ends up at the lowest address.
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}
