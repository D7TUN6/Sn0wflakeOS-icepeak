#![no_std]
#![no_main]

use icepeak_user::*;

fn confirm(prefix: &str, path: &str) -> bool {
    print!("{}{}'? ", prefix, path);
    let mut buf = [0u8; 64];
    let mut total = 0usize;
    while total < buf.len() - 1 {
        let n = read(STDIN, &mut buf[total..total + 1]);
        if n <= 0 || buf[total] == b'\n' {
            break;
        }
        total += 1;
    }
    matches!(buf.first(), Some(b'y') | Some(b'Y'))
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut recursive = false;
    let mut force = false;
    let mut interactive = false;
    let mut verbose = false;
    let mut preserve = false;
    let mut no_clobber = false;
    let mut operands: [&str; 32] = [""; 32];
    let mut nops = 0usize;
    let mut i = 1usize;
    let mut end_opts = false;
    while let Some(a) = args.get(i) {
        if !end_opts && a == "--" {
            end_opts = true;
        } else if !end_opts && a.starts_with("--") {
            match a {
                "--help" => {
                    print!("Usage: cp [OPTION]... SOURCE... DEST\n");
                    print!("  -f, --force          overwrite without prompting\n");
                    print!("  -i, --interactive    prompt before overwrite\n");
                    print!("  -n, --no-clobber     do not overwrite existing files\n");
                    print!("  -r, -R, --recursive  copy directories recursively\n");
                    print!("  -v, --verbose        explain what is being done\n");
                    print!("  -p, --preserve       preserve mode\n");
                    print!("  -a, --archive        same as -rp\n");
                    exit(0);
                }
                "--recursive" => recursive = true,
                "--force" => force = true,
                "--interactive" => interactive = true,
                "--verbose" => verbose = true,
                "--preserve" => preserve = true,
                "--no-clobber" => no_clobber = true,
                "--archive" => {
                    recursive = true;
                    preserve = true;
                }
                _ => {
                    eprint!("cp: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    'f' => force = true,
                    'i' => interactive = true,
                    'v' => verbose = true,
                    'p' => preserve = true,
                    'n' => no_clobber = true,
                    'a' => {
                        recursive = true;
                        preserve = true;
                    }
                    'd' => preserve = true,
                    _ => {
                        eprint!("cp: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        } else if nops < operands.len() {
            operands[nops] = a;
            nops += 1;
        }
        i += 1;
    }
    if nops < 2 {
        eprint!("cp: missing file operand\n");
        exit(1);
    }
    if force {
        interactive = false;
    }
    let dst = operands[nops - 1];
    let sources = &operands[..nops - 1];
    let dst_is_dir = is_directory(dst);
    if sources.len() > 1 && !dst_is_dir {
        eprint!("cp: target '{}' is not a directory\n", dst);
        exit(1);
    }
    let mut status = 0;
    for src in sources.iter() {
        let mut target_buf = [0u8; 512];
        let target = if dst_is_dir {
            let tl = path_join(&mut target_buf, dst, base_name(src));
            core::str::from_utf8(&target_buf[..tl]).unwrap_or(dst)
        } else {
            dst
        };
        let src_is_dir = is_directory(src);
        if src_is_dir && !recursive {
            eprint!("cp: -r not specified; omitting directory '{}'\n", src);
            status = 1;
            continue;
        }
        if no_clobber && exists(target) >= 0 {
            continue;
        }
        if interactive && exists(target) >= 0 && !confirm("cp: overwrite '", target) {
            continue;
        }
        let ok = if src_is_dir {
            copy_tree(src, target, verbose, preserve)
        } else {
            let ok = copy_file(src, target, preserve);
            if ok && verbose {
                print!("'{}' -> '{}'\n", src, target);
            }
            ok
        };
        if !ok {
            eprint!("cp: cannot copy '{}' to '{}'\n", src, target);
            status = 1;
        }
    }
    exit(status)
}
