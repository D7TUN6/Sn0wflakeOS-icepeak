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
                print!("Usage: poweroff [OPTION]\n");
                print!("  -f, --force  force immediate power off\n");
                print!("  -p, --poweroff  power off (default)\n");
                print!("  -h, --halt  halt the machine\n");
                exit(0);
            }
            "-f" | "--force" | "-p" | "--poweroff" | "--halt" => {}
            _ if a.starts_with('-') && a.len() > 1 => {
                eprint!("poweroff: invalid option: {}\n", a);
                exit(2);
            }
            _ => {}
        }
        i += 1;
    }
    print!("Shutting down...\n");
    shutdown();
    eprint!("poweroff: ACPI shutdown failed\n");
    exit(1)
}
