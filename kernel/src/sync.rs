//! Interrupt-safe spinlocks for small shared structures.
//!
//! The keyboard event queue and the UART receive state machine are touched from
//! IRQ context, from syscalls and from whichever CPU happens to be painting the
//! console. Both need mutual exclusion that also keeps the local CPU from
//! re-entering the same structure through an interrupt while it holds the lock.

use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock {
    held: AtomicBool,
}

pub struct SpinGuard<'a> {
    lock: &'a SpinLock,
    flags: u64,
}

impl SpinLock {
    pub const fn new() -> Self {
        Self {
            held: AtomicBool::new(false),
        }
    }

    /// Acquire with interrupts disabled on the local CPU. Holders therefore
    /// cannot be preempted, which keeps a same-CPU waiter from spinning on a
    /// lock it will never see released.
    pub fn lock(&self) -> SpinGuard<'_> {
        let flags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                "cli",
                out(reg) flags,
                options(nomem, nostack)
            );
        }
        while self
            .held
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        SpinGuard { lock: self, flags }
    }
}

impl Drop for SpinGuard<'_> {
    fn drop(&mut self) {
        self.lock.held.store(false, Ordering::Release);
        if self.flags & (1 << 9) != 0 {
            unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
        }
    }
}
