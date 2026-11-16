#![no_std]
#![no_main]

use icepeak_user::*;

fn parse_duration(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    let mut split = bytes.len();
    while split > 0 && (bytes[split - 1] == b's' || bytes[split - 1] == b'm' || bytes[split - 1] == b'h' || bytes[split - 1] == b'd') {
        split -= 1;
    }
    let number = core::str::from_utf8(&bytes[..split]).ok()?;
    let value = parse_u64(number)?;
    let unit = core::str::from_utf8(&bytes[split..]).ok()?;
    Some(match unit {
        "" | "s" => value.saturating_mul(1000),
        "m" => value.saturating_mul(60_000),
        "h" => value.saturating_mul(3_600_000),
        "d" => value.saturating_mul(86_400_000),
        _ => return None,
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut total = 0u64;
    let mut any = false;
    let mut i = 1usize;
    let mut end_opts = false;
    while let Some(a) = args.get(i) {
        if !end_opts && a == "--" {
            end_opts = true;
        } else if !end_opts && a == "--help" {
            print!("Usage: sleep NUMBER[SUFFIX]...\n");
            print!("Pause for NUMBER seconds. SUFFIX may be s, m, h or d.\n");
            exit(0);
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            eprint!("sleep: invalid option: {}\n", a);
            exit(2);
        } else if let Some(ms) = parse_duration(a) {
            total = total.saturating_add(ms);
            any = true;
        } else {
            eprint!("sleep: invalid time interval '{}'\n", a);
            exit(1);
        }
        i += 1;
    }
    if !any {
        eprint!("sleep: missing operand\n");
        exit(1);
    }
    sleep_ms(total);
    exit(0)
}
