#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut freq = 880u64;
    let mut ms = 150u64;
    let mut positional = 0usize;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: beep [FREQ] [MS]\n");
            print!("  -f, --frequency=HZ  tone frequency (default 880)\n");
            print!("  -l, --length=MS     duration in milliseconds (default 150)\n");
            exit(0);
        }
        if a == "-f" || a == "--frequency" {
            i += 1;
            freq = args.get(i).and_then(parse_u64).unwrap_or(freq);
        } else if a.starts_with("--frequency=") {
            freq = parse_u64(&a[12..]).unwrap_or(freq);
        } else if a == "-l" || a == "--length" {
            i += 1;
            ms = args.get(i).and_then(parse_u64).unwrap_or(ms);
        } else if a.starts_with("--length=") {
            ms = parse_u64(&a[9..]).unwrap_or(ms);
        } else if a.starts_with('-') && a.len() > 1 {
            eprint!("beep: invalid option: {}\n", a);
            exit(2);
        } else if positional == 0 {
            freq = parse_u64(a).unwrap_or(freq);
            positional += 1;
        } else if positional == 1 {
            ms = parse_u64(a).unwrap_or(ms);
            positional += 1;
        }
        i += 1;
    }
    if freq == 0 || freq > 20_000 {
        eprint!("beep: frequency must be between 1 and 20000 Hz\n");
        exit(1);
    }
    beep(freq, ms);
    exit(0)
}
