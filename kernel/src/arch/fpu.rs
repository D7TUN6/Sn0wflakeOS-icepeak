//! x87/SSE state, saved per task with FXSAVE.
//!
//! The kernel never touches floating point itself, but user programs are free
//! to use it. Each task owns a 512-byte, 64-byte-aligned FXSAVE area; the
//! scheduler swaps them on every context switch.
//!
//! The interrupt stubs need to find the area of the running task before any
//! Rust code executes, so the current area is published in a per-CPU array
//! indexed through `GS` (CPU index is always at `gs:0`).

use core::alloc::Layout;
use core::arch::asm;

use super::apic::MAX_CPUS;

pub const FXSAVE_SIZE: usize = 512;
const FXSAVE_ALIGN: usize = 64;

/// Physical address of the FXSAVE area for each CPU's current task. The
/// interrupt entry/exit stubs index this with the logical CPU number read from
/// `gs:0` before touching any vector register.
#[unsafe(no_mangle)]
pub static mut fpu_areas: [u64; MAX_CPUS] = [0; MAX_CPUS];

pub fn set_current(area: u64) {
    let idx = super::cpu::index();
    unsafe {
        let areas = (&raw mut fpu_areas) as *mut u64;
        *areas.add(idx) = area;
    }
}

pub fn current() -> u64 {
    let idx = super::cpu::index();
    unsafe {
        let areas = (&raw const fpu_areas) as *const u64;
        *areas.add(idx)
    }
}

/// Drop every reference to `area` so it can be freed after a task dies.
pub fn forget(area: u64) {
    unsafe {
        let areas = (&raw mut fpu_areas) as *mut u64;
        for i in 0..MAX_CPUS {
            if *areas.add(i) == area {
                *areas.add(i) = 0;
            }
        }
    }
}

/// Enable the hardware FPU and OS-managed context switching. Must run before
/// any task can execute `fxsave`/`fxrstor`.
pub fn init() {
    unsafe {
        let mut cr0: u64;
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        cr0 &= !(1 << 2); // EM: x87 is present, do not emulate
        cr0 &= !(1 << 3); // TS: no lazy switching, we save explicitly
        cr0 |= 1 << 1; // MP: monitor coprocessor
        cr0 |= 1 << 5; // NE: report x87 faults as #MF
        asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));

        let mut cr4: u64;
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        cr4 |= 1 << 9; // OSFXSR: fxsave/fxrstor are available
        cr4 |= 1 << 10; // OSXMMEXCPT: unmasked SSE exceptions
        asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));

        asm!("fninit", options(nostack, nomem));
    }
}

/// Allocate and initialise a fresh context in its power-on state.
pub fn alloc_area() -> Option<u64> {
    let layout = Layout::from_size_align(FXSAVE_SIZE, FXSAVE_ALIGN).ok()?;
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        return None;
    }
    let area = ptr as u64;
    reset_area(area);
    Some(area)
}

pub fn free_area(area: u64) {
    if let Ok(layout) = Layout::from_size_align(FXSAVE_SIZE, FXSAVE_ALIGN) {
        unsafe { alloc::alloc::dealloc(area as *mut u8, layout) };
    }
}

fn reset_area(area: u64) {
    unsafe {
        let p = area as *mut u8;
        core::ptr::write_bytes(p, 0, FXSAVE_SIZE);
        *(p as *mut u16) = 0x037F; // default x87 control word
        *(p.add(24) as *mut u32) = 0x1F80; // default MXCSR
    }
}

#[inline]
pub fn save(area: u64) {
    unsafe { asm!("fxsave [{}]", in(reg) area, options(nostack)) };
}

#[inline]
pub fn restore(area: u64) {
    unsafe { asm!("fxrstor [{}]", in(reg) area, options(nostack)) };
}
