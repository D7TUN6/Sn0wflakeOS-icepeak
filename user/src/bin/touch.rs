#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut no_create = false;
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
                    print!("Usage: touch [OPTION]... FILE...\n");
                    print!("  -a  change only the access time\n");
                    print!("  -m  change only the modification time\n");
                    print!("  -c, --no-create  do not create any files\n");
                    print!("  -t STAMP, -d STRING, -r FILE  accepted for compatibility\n");
                    exit(0);
                }
                "--no-create" => no_create = true,
                "--date" | "--reference" => i += 1,
                _ if a.starts_with("--date=") || a.starts_with("--reference=") => {}
                _ => {
                    eprint!("touch: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'a' | 'm' => {}
                    'c' => no_create = true,
                    't' | 'd' | 'r' => {
                        i += 1;
                    }
                    _ => {
                        eprint!("touch: invalid option: -{}\n", c);
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
        eprint!("touch: missing file operand\n");
        exit(1);
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        if exists(path) >= 0 {
            continue;
        }
        if no_create {
            continue;
        }
        if write_file(path, &[]) < 0 {
            eprint!("touch: cannot touch '{}'\n", path);
            status = 1;
        }
    }
    exit(status)
}
