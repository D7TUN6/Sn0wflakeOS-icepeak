#![no_std]
#![no_main]

use icepeak_user::*;

/// PID 1. It only starts the console login and reaps children; everything
/// else, including the shell, runs as a child so the system can recover from
/// a crashed session instead of dropping to the kernel.
#[unsafe(no_mangle)]
pub extern "C" fn _start(_argc: usize, _argv: *const *const u8) -> ! {
    print!("\nSn0wflakeOS init 0.1.0 (icepeak)\n");
    loop {
        let pid = exec("/bin/login", &["login"]);
        if pid < 0 {
            eprint!("init: cannot start /bin/login\n");
            let shell = exec("/bin/peaksh", &["/bin/peaksh"]);
            if shell < 0 {
                sleep_ms(1000);
                continue;
            }
            let _ = wait(shell as u64);
            continue;
        }
        let status = wait(pid as u64);
        if status != 0 {
            print!("init: login exited with status {}\n", status);
        }
    }
}
