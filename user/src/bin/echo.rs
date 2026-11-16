#![no_std]
#![no_main]

use icepeak_user::*;

fn emit_escaped(s: &str) {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 >= bytes.len() {
            let _ = write(1, &bytes[i..i + 1]);
            i += 1;
            continue;
        }
        i += 1;
        match bytes[i] {
            b'n' => print!("\n"),
            b't' => print!("\t"),
            b'r' => print!("\r"),
            b'e' => print!("\x1b"),
            b'a' => print!("\x07"),
            b'b' => print!("\x08"),
            b'f' => print!("\x0c"),
            b'v' => print!("\x0b"),
            b'\\' => print!("\\"),
            b'0' => print!("\x00"),
            other => {
                print!("\\");
                let _ = write(1, &[other]);
            }
        }
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut newline = true;
    let mut escapes = false;
    let mut first = 1usize;
    let mut end_opts = false;
    while let Some(a) = args.get(first) {
        if !end_opts && a == "--" {
            end_opts = true;
            first += 1;
            continue;
        }
        if !end_opts && a.starts_with("--") {
            match a {
                "--help" => {
                    print!("Usage: echo [OPTION]... [STRING]...\n");
                    print!("  -n  do not output the trailing newline\n");
                    print!("  -e  enable interpretation of backslash escapes\n");
                    print!("  -E  disable interpretation of backslash escapes (default)\n");
                    exit(0);
                }
                "--no-newline" => newline = false,
                "--escapes" => escapes = true,
                _ => break,
            }
            first += 1;
            continue;
        }
        if !end_opts && a.starts_with('-') && a.len() > 1 {
            let mut ok = true;
            for c in a[1..].chars() {
                match c {
                    'n' => newline = false,
                    'e' => escapes = true,
                    'E' => escapes = false,
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            first += 1;
            continue;
        }
        break;
    }
    let mut i = first;
    let mut any = false;
    while let Some(a) = args.get(i) {
        if any {
            print!(" ");
        }
        if escapes {
            emit_escaped(a);
        } else {
            print(a);
        }
        any = true;
        i += 1;
    }
    if newline {
        print!("\n");
    }
    exit(0)
}
