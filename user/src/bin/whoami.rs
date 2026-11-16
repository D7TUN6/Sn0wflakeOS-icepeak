#![no_std]
#![no_main]

use icepeak_user::*;

pub fn user_name(uid: u32) -> &'static str {
    match uid {
        0 => "root",
        1000 => "user",
        1001 => "guest",
        65534 => "nobody",
        _ => "unknown",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    if let Some(a) = args.get(1) {
        if a == "--help" || a == "-h" {
            print!("Usage: whoami\nPrint the effective user name.\n");
            exit(0);
        }
    }
    let uid = getuid();
    if uid == 0 || uid == 1000 || uid == 1001 || uid == 65534 {
        print!("{}\n", user_name(uid));
    } else {
        print!("{}\n", uid);
    }
    exit(0)
}
