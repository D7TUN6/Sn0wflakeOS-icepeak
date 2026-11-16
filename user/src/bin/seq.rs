#![no_std]
#![no_main]

use icepeak_user::*;

fn print_padded(n: u64, width: usize) -> bool {
    let mut digits = [0u8; 20];
    let mut d = digits.len();
    let mut v = n;
    if v == 0 {
        d -= 1;
        digits[d] = b'0';
    }
    while v > 0 {
        d -= 1;
        digits[d] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for _ in digits.len() - d..width {
        if write(1, b" ") != 1 {
            return false;
        }
    }
    write(1, &digits[d..]) == (digits.len() - d) as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut sep = "\n";
    let mut equal_width = false;
    let mut nums: [u64; 3] = [0; 3];
    let mut count = 0usize;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: seq [OPTION]... [FIRST [STEP]] LAST\n");
            print!("  -s, --separator=STRING  use STRING to separate numbers\n");
            print!("  -w, --equal-width       equalize width with leading zeros\n");
            exit(0);
        }
        if a == "-w" || a == "--equal-width" {
            equal_width = true;
        } else if a == "-s" || a == "--separator" {
            i += 1;
            sep = args.get(i).unwrap_or("\n");
        } else if let Some(rest) = a.strip_prefix("--separator=") {
            sep = rest;
        } else if a.starts_with('-') && a.len() > 1 && a.parse::<i64>().is_err() {
            eprint!("seq: invalid option: {}\n", a);
            exit(2);
        } else if let Some(n) = parse_u64(a) {
            if count < 3 {
                nums[count] = n;
                count += 1;
            }
        } else {
            eprint!("seq: invalid number: {}\n", a);
            exit(2);
        }
        i += 1;
    }
    let (first, step, last) = match count {
        0 => {
            eprint!("seq: missing operand\n");
            exit(2);
        }
        1 => (1, 1, nums[0]),
        2 => (nums[0], 1, nums[1]),
        _ => (nums[0], nums[1], nums[2]),
    };
    if step == 0 {
        eprint!("seq: step cannot be zero\n");
        exit(2);
    }
    let width = if equal_width {
        let mut w = 0usize;
        let mut d = [0u8; 20];
        let mut v = last;
        let mut n = 0usize;
        if v == 0 {
            n = 1;
        }
        while v > 0 {
            d[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
        w = n;
        w
    } else {
        0
    };
    let mut value = first;
    let mut first_out = true;
    let mut write_ok = true;
    while value <= last {
        if !first_out && write(1, sep.as_bytes()) != sep.len() as isize {
            write_ok = false;
            break;
        }
        if !print_padded(value, width) {
            write_ok = false;
            break;
        }
        first_out = false;
        match value.checked_add(step) {
            Some(v) if v > value => value = v,
            _ => break,
        }
    }
    if !first_out && write(1, b"\n") != 1 {
        write_ok = false;
    }
    if !write_ok {
        // A closed pipe is not a reason to keep formatting numbers.
        eprint!("seq: write error\n");
        exit(1);
    }
    exit(0)
}
