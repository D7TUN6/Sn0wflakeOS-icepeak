//! Verifies that x87/SSE state survives context switches.
//!
//! Timer preemption can happen at any instruction, so a task that keeps values
//! live in XMM registers across a `sleep` only observes them intact when the
//! scheduler saves and restores FXSAVE areas.

#![no_std]
#![no_main]

use core::arch::asm;

use icepeak_user::*;

/// 2.0 in IEEE-754 double precision.
const SENTINEL: u64 = 0x4000_0000_0000_0000;

#[unsafe(no_mangle)]
pub extern "C" fn _start(_argc: usize, _argv: *const *const u8) -> ! {
    unsafe {
        asm!(
            "movq xmm0, rax",
            "movq xmm1, rax",
            "movq xmm2, rax",
            "movq xmm3, rax",
            in("rax") SENTINEL,
        );
    }

    let mut i = 0u64;
    while i < 200 {
        sleep_ms(5);
        let got: u64;
        unsafe { asm!("movq rax, xmm0", out("rax") got) };
        if got != SENTINEL {
            print("fptest: FAIL, xmm0 was clobbered\n");
            exit(1);
        }
        i += 1;
    }

    print("fptest: ok\n");
    exit(0);
}
