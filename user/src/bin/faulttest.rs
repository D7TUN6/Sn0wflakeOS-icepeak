#![no_std]
#![no_main]

//! Fault-path self-test: deliberately faults in ring 3 so the kernel's
//! kill-the-task path can be exercised without taking the machine down.
//! The shell should survive and report exit status 128 + vector.

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mode = args.get(1).unwrap_or("badptr");
    match mode {
        "divzero" => {
            print!("faulttest: divide error\n");
            // Force a real #DE; a Rust `1 / 0` would hit the panic path
            // instead of the hardware exception.
            let mut rax: u64 = 1;
            unsafe {
                core::arch::asm!(
                    "xor rdx, rdx",
                    "div {d}",
                    d = in(reg) 0u64,
                    inout("rax") rax,
                    out("rdx") _,
                );
            }
            print!("faulttest: survived? {}\n", rax);
        }
        "badptr" => {
            print!("faulttest: null dereference\n");
            let p = core::ptr::null::<u64>();
            let v = unsafe { core::ptr::read_volatile(p) };
            print!("faulttest: survived? {}\n", v);
        }
        "kernel" => {
            print!("faulttest: user pointer into kernel space\n");
            // Not reachable through the syscall boundary: the kernel must
            // reject it with EFAULT instead of faulting in ring 0.
            let n = write(1, unsafe {
                core::slice::from_raw_parts(0x10 as *const u8, 4)
            });
            print!("faulttest: write returned {}\n", n);
        }
        _ => {
            print!("Usage: faulttest [badptr|divzero|kernel]\n");
            exit(2);
        }
    }
    exit(0)
}
