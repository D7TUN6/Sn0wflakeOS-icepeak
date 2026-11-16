#![no_std]
#![no_main]

use icepeak_user::*;

/// Create every component of `path`, ignoring components that already exist.
fn make_parents(path: &str, verbose: bool) -> bool {
    let mut buf = [0u8; 512];
    let mut len = 0usize;
    let absolute = path.starts_with('/');
    let mut ok = true;
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if len == 0 && absolute {
            buf[len] = b'/';
            len += 1;
        }
        if len > 0 && buf[len - 1] != b'/' {
            buf[len] = b'/';
            len += 1;
        }
        for &b in part.as_bytes() {
            if len + 1 < buf.len() {
                buf[len] = b;
                len += 1;
            }
        }
        let prefix = core::str::from_utf8(&buf[..len]).unwrap_or("/");
        if !is_directory(prefix) {
            if mkdir(prefix) < 0 {
                ok = false;
            } else if verbose {
                print!("mkdir: created directory '{}'\n", prefix);
            }
        }
    }
    ok
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut parents = false;
    let mut verbose = false;
    let mut mode: Option<u16> = None;
    let mut paths: [&str; 32] = [""; 32];
    let mut npaths = 0usize;
    let mut i = 1usize;
    let mut end_opts = false;
    while let Some(a) = args.get(i) {
        if !end_opts && a == "--" {
            end_opts = true;
        } else if !end_opts && a.starts_with("--") {
            match a {
                "--help" => {
                    print!("Usage: mkdir [OPTION]... DIRECTORY...\n");
                    print!("  -p, --parents   make parent directories as needed\n");
                    print!("  -v, --verbose   print a message for each created directory\n");
                    print!("  -m, --mode=MODE set permission mode (octal)\n");
                    exit(0);
                }
                "--parents" => parents = true,
                "--verbose" => verbose = true,
                "--mode" => {
                    i += 1;
                    mode = args.get(i).and_then(parse_octal);
                }
                _ if a.starts_with("--mode=") => {
                    mode = parse_octal(&a[7..]);
                }
                _ => {
                    eprint!("mkdir: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            let mut chars = a[1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    'p' => parents = true,
                    'v' => verbose = true,
                    'm' => {
                        i += 1;
                        mode = args.get(i).and_then(parse_octal);
                    }
                    _ => {
                        eprint!("mkdir: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        } else if npaths < paths.len() {
            paths[npaths] = a;
            npaths += 1;
        }
        i += 1;
    }
    if npaths == 0 {
        eprint!("mkdir: missing operand\n");
        exit(1);
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        let ok = if parents {
            make_parents(path, verbose)
        } else if is_directory(path) {
            false
        } else if mkdir(path) < 0 {
            false
        } else {
            if verbose {
                print!("mkdir: created directory '{}'\n", path);
            }
            true
        };
        if !ok {
            if parents && is_directory(path) {
                continue;
            }
            eprint!("mkdir: cannot create directory '{}'\n", path);
            status = 1;
        } else if let Some(m) = mode {
            chmod(path, m);
        }
    }
    exit(status)
}
