#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    if args.len < 2 {
        eprint!("basename: missing operand\n");
        exit(1);
    }
    let mut i = 1usize;
    let mut suffix: Option<&str> = None;
    let mut path: Option<&str> = None;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: basename NAME [SUFFIX]\nStrip directory and suffix from NAME.\n");
            exit(0);
        }
        if a == "--" {
            i += 1;
            continue;
        }
        if path.is_none() {
            path = Some(a);
        } else {
            suffix = Some(a);
        }
        i += 1;
    }
    let Some(path) = path else {
        eprint!("basename: missing operand\n");
        exit(1);
    };
    let mut name = base_name(path);
    if let Some(suf) = suffix {
        if !suf.is_empty() && name.len() > suf.len() && name.ends_with(suf) {
            name = &name[..name.len() - suf.len()];
        }
    }
    print!("{}\n", name);
    exit(0)
}
