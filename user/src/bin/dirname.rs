#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    if args.len < 2 {
        eprint!("dirname: missing operand\n");
        exit(1);
    }
    let mut i = 1usize;
    let mut any = false;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: dirname NAME\nStrip the last component from NAME.\n");
            exit(0);
        }
        if a == "--" {
            i += 1;
            continue;
        }
        print!("{}\n", dir_name(a));
        any = true;
        i += 1;
    }
    if !any {
        eprint!("dirname: missing operand\n");
        exit(1);
    }
    exit(0)
}
