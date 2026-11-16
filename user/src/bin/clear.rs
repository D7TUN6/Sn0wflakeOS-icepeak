#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        match a {
            "--help" | "-h" => {
                print!("Usage: clear\nClear the terminal screen.\n");
                exit(0);
            }
            "-x" | "--no-clear" => {}
            _ if a.starts_with('-') && a.len() > 1 => {
                eprint!("clear: invalid option: {}\n", a);
                exit(2);
            }
            _ => {}
        }
        i += 1;
    }
    clear();
    exit(0)
}
