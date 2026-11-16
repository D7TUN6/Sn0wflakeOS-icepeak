#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut logical = false;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        match a {
            "--help" | "-h" => {
                print!("Usage: pwd [-L|-P]\nPrint the full name of the current directory.\n");
                exit(0);
            }
            "-L" | "--logical" => logical = true,
            "-P" | "--physical" => logical = false,
            "--" => {}
            _ if a.starts_with('-') && a.len() > 1 => {
                eprint!("pwd: invalid option: {}\n", a);
                exit(2);
            }
            _ => {}
        }
        i += 1;
    }
    let _ = logical;
    let mut buf = [0u8; 512];
    let n = getcwd(&mut buf);
    if n < 0 {
        eprint!("pwd: cannot determine current directory\n");
        exit(1);
    }
    let _ = write(1, &buf[..n as usize]);
    print!("\n");
    exit(0)
}
