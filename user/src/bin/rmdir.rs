#![no_std]
#![no_main]

use icepeak_user::*;

fn remove_one(path: &str) -> bool {
    rmdir(path) >= 0
}

/// Remove `path` and then its empty parents when `parents` is set.
fn remove_parents(path: &str, verbose: bool) -> bool {
    if !remove_one(path) {
        return false;
    }
    if verbose {
        print!("rmdir: removing directory '{}'\n", path);
    }
    let mut cur = dir_name(path);
    loop {
        if cur == "/" || cur == "." || cur.is_empty() {
            break;
        }
        if !remove_one(cur) {
            break;
        }
        if verbose {
            print!("rmdir: removing directory '{}'\n", cur);
        }
        cur = dir_name(cur);
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut parents = false;
    let mut verbose = false;
    let mut ignore_non_empty = false;
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
                    print!("Usage: rmdir [OPTION]... DIRECTORY...\n");
                    print!("  -p, --parents                 remove DIRECTORY and its ancestors\n");
                    print!("  -v, --verbose                 print a message for each directory\n");
                    print!("      --ignore-fail-on-non-empty ignore non-empty failures\n");
                    exit(0);
                }
                "--parents" => parents = true,
                "--verbose" => verbose = true,
                "--ignore-fail-on-non-empty" => ignore_non_empty = true,
                _ => {
                    eprint!("rmdir: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'p' => parents = true,
                    'v' => verbose = true,
                    _ => {
                        eprint!("rmdir: invalid option: -{}\n", c);
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
        eprint!("rmdir: missing operand\n");
        exit(1);
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        let ok = if parents {
            remove_parents(path, verbose)
        } else {
            let ok = remove_one(path);
            if ok && verbose {
                print!("rmdir: removing directory '{}'\n", path);
            }
            ok
        };
        if !ok && !ignore_non_empty {
            eprint!("rmdir: failed to remove '{}'\n", path);
            status = 1;
        }
    }
    exit(status)
}
