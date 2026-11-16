use core::arch::asm;
use core::mem::size_of;

use super::apic::MAX_CPUS;

pub const KERNEL_CODE: u16 = 0x08;
pub const KERNEL_DATA: u16 = 0x10;
pub const USER_DATA: u16 = 0x18;
pub const USER_CODE: u16 = 0x20;
pub const TSS_SEL: u16 = 0x28;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Tss {
    reserved0: u32,
    rsp0: u64,
    rsp1: u64,
    rsp2: u64,
    reserved1: u64,
    ist1: u64,
    ist2: u64,
    ist3: u64,
    ist4: u64,
    ist5: u64,
    ist6: u64,
    ist7: u64,
    reserved2: u64,
    reserved3: u16,
    iomap_base: u16,
}

const TSS_INIT: Tss = Tss {
    reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    reserved1: 0,
    ist1: 0,
    ist2: 0,
    ist3: 0,
    ist4: 0,
    ist5: 0,
    ist6: 0,
    ist7: 0,
    reserved2: 0,
    reserved3: 0,
    iomap_base: size_of::<Tss>() as u16,
};

static mut GDTS: [[u64; 7]; MAX_CPUS] = [[0; 7]; MAX_CPUS];
static mut TSSES: [Tss; MAX_CPUS] = [TSS_INIT; MAX_CPUS];

#[repr(align(16))]
struct IstStack([u8; 8192]);
static mut ISTS: [IstStack; MAX_CPUS] = [const { IstStack([0; 8192]) }; MAX_CPUS];

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

fn code_desc(dpl: u16, long: bool) -> u64 {
    let access = 0x9A | ((dpl & 3) << 5);
    let flags = if long { 0xA } else { 0xC };
    0xFFFF | ((access as u64) << 40) | (0xF_u64 << 48) | ((flags as u64) << 52)
}

fn data_desc(dpl: u16) -> u64 {
    let access = 0x92 | ((dpl & 3) << 5);
    0xFFFF | ((access as u64) << 40) | (0xF_u64 << 48) | (0xC_u64 << 52)
}

unsafe fn tss_desc(base: u64) -> (u64, u64) {
    let limit = (size_of::<Tss>() - 1) as u64;
    let low = (limit & 0xFFFF)
        | ((base & 0xFFFF) << 16)
        | (((base >> 16) & 0xFF) << 32)
        | (0x89_u64 << 40)
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    let high = base >> 32;
    (low, high)
}

/// Build and load the GDT/TSS belonging to `cpu`. Every CPU has its own TSS so
/// `rsp0` can point at the kernel stack of the task it is currently running.
pub fn init_cpu(cpu: usize) {
    let cpu = cpu.min(MAX_CPUS - 1);
    unsafe {
        let gdt = &mut (*(&raw mut GDTS))[cpu];
        let tss = &mut (*(&raw mut TSSES))[cpu];
        let ist = &mut (*(&raw mut ISTS))[cpu];
        tss.rsp0 = 0;
        tss.ist1 = ist as *mut IstStack as u64 + 8192;
        tss.iomap_base = size_of::<Tss>() as u16;

        gdt[0] = 0;
        gdt[1] = code_desc(0, true);
        gdt[2] = data_desc(0);
        gdt[3] = data_desc(3);
        gdt[4] = code_desc(3, true);
        let (lo, hi) = tss_desc(tss as *mut Tss as u64);
        gdt[5] = lo;
        gdt[6] = hi;

        let ptr = GdtPointer {
            limit: (size_of::<[u64; 7]>() - 1) as u16,
            base: gdt.as_ptr() as u64,
        };
        asm!("lgdt [{}]", in(reg) &ptr, options(readonly, nostack, preserves_flags));

        // The stale CS cache still carries the selector we entered this GDT
        // with. On an AP that is 0x18 from the trampoline's own GDT, which in
        // the kernel GDT means USER_DATA: harmless until the next iretq tries
        // to re-validate it and takes a #GP. Reload CS with a far return.
        asm!(
            "push {sel}",
            "lea {tmp}, [rip + 3f]",
            "push {tmp}",
            "retfq",
            "3:",
            sel = in(reg) KERNEL_CODE as u64,
            tmp = out(reg) _,
            options(preserves_flags)
        );

        asm!(
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            out("ax") _,
            options(nostack, preserves_flags)
        );

        asm!("ltr ax", in("ax") TSS_SEL, options(nostack, preserves_flags));
    }
}

/// Backwards-compatible entry point for the bootstrap CPU.
pub fn init() {
    init_cpu(0);
}

pub fn set_kernel_stack(cpu: usize, stack_top: u64) {
    let cpu = cpu.min(MAX_CPUS - 1);
    unsafe {
        (*((&raw mut TSSES) as *mut Tss).add(cpu)).rsp0 = stack_top;
    }
}

pub fn set_ist1(cpu: usize, stack_top: u64) {
    let cpu = cpu.min(MAX_CPUS - 1);
    unsafe {
        (*((&raw mut TSSES) as *mut Tss).add(cpu)).ist1 = stack_top;
    }
}

pub fn reload() {
    init();
}

#[allow(dead_code)]
pub fn enable_iomap() {}
