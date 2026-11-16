#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    print("hello from ring 3!\n");
    for i in 0..args.len {
        print("argv[");
        print_num(i as u64);
        print("] = ");
        if let Some(s) = args.get(i) {
            print(s);
        }
        print("\n");
    }
    exit(0)
}

fn print_num(mut n: u64) {
    if n == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let _ = write(1, &buf[i..]);
}
