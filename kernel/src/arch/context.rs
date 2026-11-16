//! Entering a saved interrupt frame from ordinary kernel code.

use super::idt::InterruptFrame;

/// Restore `frame` and `iretq` into it. Used once to leave `kmain` and enter
/// the scheduler's idle task.
#[unsafe(naked)]
pub unsafe extern "C" fn enter_frame(frame: *mut InterruptFrame) -> ! {
    core::arch::naked_asm!(
        "mov rsp, rdi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "add rsp, 16",
        "iretq",
    )
}
