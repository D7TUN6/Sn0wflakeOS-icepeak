#![no_std]
#![no_main]

use icepeak_user::*;

fn confirm(prefix: &str, path: &str) -> bool {
    print!("{}{}'? ", prefix, path);
    let mut buf = [0u8; 64];
    let mut total = 0usize;
    while total < buf.len() - 1 {
        let n = read(STDIN, &mut buf[total..total + 1]);
        if n <= 0 {
            break;
        }
        if buf[total] == b'\n' {
            break;
        }
        total += 1;
    }
    matches!(buf.first(), Some(b'y') | Some(b'Y'))
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut force = false;
    let mut recursive = false;
    let mut verbose = false;
    let mut interactive = false;
    let mut dirs_ok = false;
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
                    print!("Usage: rm [OPTION]... [FILE]...\n");
                    print!("  -f, --force           ignore nonexistent files\n");
                    print!("  -r, -R, --recursive   remove directories recursively\n");
                    print!("  -v, --verbose         explain what is being done\n");
                    print!("  -i                    prompt before every removal\n");
                    print!("  -d, --dir             remove empty directories\n");
                    exit(0);
                }
                "--force" => force = true,
                "--recursive" => recursive = true,
                "--verbose" => verbose = true,
                "--dir" => dirs_ok = true,
                _ => {
                    eprint!("rm: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'f' => force = true,
                    'r' | 'R' => recursive = true,
                    'v' => verbose = true,
                    'i' => interactive = true,
                    'd' => dirs_ok = true,
                    _ => {
                        eprint!("rm: invalid option: -{}\n", c);
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
        if force {
            exit(0);
        }
        eprint!("rm: missing operand\n");
        exit(1);
    }
    if force {
        interactive = false;
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        let is_dir = is_directory(path);
        if is_dir && !recursive && !dirs_ok {
            eprint!("rm: cannot remove '{}': is a directory\n", path);
            status = 1;
            continue;
        }
        if interactive && !confirm("rm: remove '", path) {
            continue;
        }
        let ok = if is_dir && recursive {
            remove_tree(path, verbose)
        } else if is_dir {
            rmdir(path) >= 0
        } else {
            let ok = unlink(path) >= 0;
            if ok && verbose {
                print!("removed '{}'\n", path);
            }
            ok
        };
        if !ok {
            if !force {
                eprint!("rm: cannot remove '{}'\n", path);
                status = 1;
            }
        } else if verbose && is_dir {
            // remove_tree already printed, but it prints before removing.
        }
    }
    exit(status)
}
