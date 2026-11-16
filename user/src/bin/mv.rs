#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut force = false;
    let mut interactive = false;
    let mut verbose = false;
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
                    print!("Usage: mv [OPTION]... SOURCE... DEST\n");
                    print!("  -f, --force        do not prompt before overwriting\n");
                    print!("  -i, --interactive  prompt before overwrite\n");
                    print!("  -n, --no-clobber   do not overwrite an existing file\n");
                    print!("  -v, --verbose      explain what is being done\n");
                    exit(0);
                }
                "--force" => force = true,
                "--interactive" => interactive = true,
                "--verbose" => verbose = true,
                "--no-clobber" => no_clobber = true,
                _ => {
                    eprint!("mv: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'f' => force = true,
                    'i' => interactive = true,
                    'v' => verbose = true,
                    'n' => no_clobber = true,
                    _ => {
                        eprint!("mv: invalid option: -{}\n", c);
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
        eprint!("mv: missing file operand\n");
        exit(1);
    }
    if force {
        interactive = false;
    }
    let dst = operands[nops - 1];
    let sources = &operands[..nops - 1];
    let dst_is_dir = is_directory(dst);
    if sources.len() > 1 && !dst_is_dir {
        eprint!("mv: target '{}' is not a directory\n", dst);
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
        if no_clobber && exists(target) >= 0 {
            continue;
        }
        if interactive && exists(target) >= 0 {
            print!("mv: overwrite '{}'? ", target);
            let mut ans = [0u8; 8];
            let n = read(STDIN, &mut ans);
            if n <= 0 || !matches!(ans.first(), Some(b'y') | Some(b'Y')) {
                continue;
            }
        }
        let ok = if rename(src, target) >= 0 {
            true
        } else if is_directory(src) {
            let ok = copy_tree(src, target, false, true);
            if ok {
                remove_tree(src, false);
            }
            ok
        } else {
            let ok = copy_file(src, target, true);
            if ok {
                unlink(src);
            }
            ok
        };
        if ok {
            if verbose {
                print!("renamed '{}' -> '{}'\n", src, target);
            }
        } else {
            eprint!("mv: cannot move '{}' to '{}'\n", src, target);
            status = 1;
        }
    }
    exit(status)
}
