#![no_std]
#![no_main]

use icepeak_user::*;

fn put(b: u8) {
    let _ = write(1, &[b]);
}

fn put_str(s: &str) {
    let _ = write(1, s.as_bytes());
}

fn print_hex(n: u64, upper: bool) {
    let digits: &[u8] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    let mut v = n;
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        buf[i] = digits[(v & 0xF) as usize];
        v >>= 4;
    }
    let _ = write(1, &buf[i..]);
}

fn parse_int(s: &str) -> i64 {
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let mut n = 0i64;
    for &b in rest.as_bytes() {
        if !b.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    if neg { -n } else { n }
}

/// Handle one backslash escape. `i` points at the character after `\` and is
/// advanced past everything the escape consumed.
fn escape(bytes: &[u8], i: &mut usize) {
    if *i >= bytes.len() {
        put(b'\\');
        return;
    }
    let c = bytes[*i];
    match c {
        b'n' => put(b'\n'),
        b't' => put(b'\t'),
        b'r' => put(b'\r'),
        b'e' => put(0x1b),
        b'a' => put(0x07),
        b'b' => put(0x08),
        b'f' => put(0x0c),
        b'v' => put(0x0b),
        b'\\' => put(b'\\'),
        b'"' => put(b'"'),
        b'x' => {
            let mut v = 0u8;
            let mut used = 0;
            while used < 2 && *i + 1 < bytes.len() {
                let d = match bytes[*i + 1] {
                    b'0'..=b'9' => bytes[*i + 1] - b'0',
                    b'a'..=b'f' => bytes[*i + 1] - b'a' + 10,
                    b'A'..=b'F' => bytes[*i + 1] - b'A' + 10,
                    _ => break,
                };
                v = v.wrapping_mul(16).wrapping_add(d);
                *i += 1;
                used += 1;
            }
            if used == 0 {
                put(b'x');
            } else {
                put(v);
            }
        }
        b'0'..=b'7' => {
            let mut v = c - b'0';
            let mut used = 1;
            while used < 3 && *i + 1 < bytes.len() && (b'0'..=b'7').contains(&bytes[*i + 1]) {
                *i += 1;
                v = v.wrapping_mul(8).wrapping_add(bytes[*i] - b'0');
                used += 1;
            }
            put(v);
        }
        other => {
            put(b'\\');
            put(other);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let Some(fmt) = args.get(1) else {
        eprint!("printf: missing format\n");
        exit(2);
    };
    if fmt == "--help" || fmt == "-h" {
        print!("Usage: printf FORMAT [ARG]...\n");
        print!("Interpret backslash escapes and %s %d %x %c format specifiers.\n");
        exit(0);
    }
    let bytes = fmt.as_bytes();
    let mut i = 0usize;
    let mut arg_index = 2usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                escape(bytes, &mut i);
                i += 1;
            }
            b'%' if i + 1 < bytes.len() => {
                i += 1;
                // Skip flags, width and precision (we do not pad).
                while i < bytes.len()
                    && matches!(bytes[i], b'-' | b'+' | b'0' | b' ' | b'#' | b'.' | b'0'..=b'9')
                {
                    i += 1;
                }
                if i >= bytes.len() {
                    put(b'%');
                    break;
                }
                let spec = bytes[i];
                i += 1;
                let arg = args.get(arg_index).unwrap_or("");
                match spec {
                    b'%' => put(b'%'),
                    b's' => {
                        put_str(arg);
                        arg_index += 1;
                    }
                    b'c' => {
                        put(arg.as_bytes().first().copied().unwrap_or(0));
                        arg_index += 1;
                    }
                    b'd' | b'i' => {
                        print_i64(parse_int(arg));
                        arg_index += 1;
                    }
                    b'x' | b'X' => {
                        print_hex(parse_int(arg) as u64, spec == b'X');
                        arg_index += 1;
                    }
                    _ => {
                        put(b'%');
                        put(spec);
                    }
                }
            }
            b => {
                put(b);
                i += 1;
            }
        }
    }
    exit(0)
}
