#![no_std]
#![no_main]

use icepeak_user::*;

fn class_mask(who: &[u8]) -> u16 {
    let mut mask = 0u16;
    let mut any = false;
    for &c in who {
        any = true;
        match c {
            b'u' => mask |= 0o4700,
            b'g' => mask |= 0o2070,
            b'o' => mask |= 0o1007,
            b'a' => mask |= 0o7777,
            _ => {}
        }
    }
    if !any {
        mask = 0o7777;
    }
    mask
}

fn perm_value(perms: &[u8], who: &[u8], current: u16, is_dir: bool) -> u16 {
    let mut value = 0u16;
    let classes: &[u8] = if who.is_empty() { b"ugo" } else { who };
    let exec_ok = is_dir || current & 0o111 != 0;
    for &cls in classes {
        let shift = match cls {
            b'u' => 6,
            b'g' => 3,
            b'o' => 0,
            _ => continue,
        };
        for &p in perms {
            match p {
                b'r' => value |= 4 << shift,
                b'w' => value |= 2 << shift,
                b'x' => value |= 1 << shift,
                b'X' if exec_ok => value |= 1 << shift,
                b's' if cls == b'u' => value |= 0o4000,
                b's' if cls == b'g' => value |= 0o2000,
                b't' if cls == b'o' => value |= 0o1000,
                _ => {}
            }
        }
    }
    value
}

fn apply_symbolic(current: u16, spec: &str, is_dir: bool) -> Option<u16> {
    let mut mode = current & 0o7777;
    for clause in spec.split(',') {
        let bytes = clause.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o' | b'a') {
            i += 1;
        }
        let who = &bytes[..i];
        if i >= bytes.len() {
            return None;
        }
        let op = bytes[i];
        if op != b'+' && op != b'-' && op != b'=' {
            return None;
        }
        i += 1;
        let perms = &bytes[i..];
        let mask = class_mask(who);
        let value = perm_value(perms, who, mode, is_dir);
        mode = match op {
            b'+' => mode | (value & mask),
            b'-' => mode & !(value & mask),
            b'=' => (mode & !mask) | (value & mask),
            _ => return None,
        };
    }
    Some(mode & 0o7777)
}

fn parse_mode_spec(spec: &str, current: u16, is_dir: bool) -> Option<u16> {
    if spec.bytes().all(|b| b.is_ascii_digit()) {
        return parse_octal(spec);
    }
    apply_symbolic(current, spec, is_dir)
}

fn change_one(path: &str, spec: &str, verbose: bool) -> bool {
    let mut buf = [0u8; 128];
    let n = stat(path, &mut buf);
    if n <= 0 {
        return false;
    }
    let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    let mut parts = text.split_whitespace();
    let kind = parts.next().unwrap_or("f");
    let _size = parts.next();
    let mode = parts.next().and_then(parse_octal).unwrap_or(0);
    let Some(new_mode) = parse_mode_spec(spec, mode, kind == "d") else {
        return false;
    };
    if chmod(path, new_mode) < 0 {
        return false;
    }
    if verbose {
        print!("mode of '{}' changed to {:o}\n", path, new_mode);
    }
    true
}

fn walk(path: &str, spec: &str, recursive: bool, verbose: bool) -> bool {
    let mut ok = change_one(path, spec, verbose);
    if !recursive || !is_directory(path) {
        return ok;
    }
    let mut skip = 0usize;
    loop {
        let mut name = [0u8; 128];
        let mut name_len = 0usize;
        {
            let buf = aux_list();
            let n = list(path, buf);
            if n < 0 {
                return ok;
            }
            let text = &buf[..n as usize];
            let mut index = 0usize;
            for line in text.split(|&b| b == b'\n') {
                if line.len() < 5 {
                    continue;
                }
                if index == skip {
                    let mut i = 2;
                    while i < line.len() && line[i] != b' ' {
                        i += 1;
                    }
                    name_len = (i - 2).min(name.len());
                    name[..name_len].copy_from_slice(&line[2..2 + name_len]);
                    break;
                }
                index += 1;
            }
            if name_len == 0 {
                break;
            }
        }
        let mut child = [0u8; 512];
        let cl = path_join(&mut child, path, core::str::from_utf8(&name[..name_len]).unwrap_or(""));
        if !walk(core::str::from_utf8(&child[..cl]).unwrap_or("/"), spec, recursive, verbose) {
            ok = false;
        }
        skip += 1;
    }
    ok
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut recursive = false;
    let mut verbose = false;
    let mut silent = false;
    let mut spec: Option<&str> = None;
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
                    print!("Usage: chmod [OPTION]... MODE FILE...\n");
                    print!("  -R, --recursive  change files and directories recursively\n");
                    print!("  -v, --verbose    output a diagnostic for every file\n");
                    print!("  -f, --silent     suppress most error messages\n");
                    print!("MODE may be octal (755) or symbolic (u=rwx,go=rx).\n");
                    exit(0);
                }
                "--recursive" => recursive = true,
                "--verbose" => verbose = true,
                "--silent" | "--quiet" => silent = true,
                _ => {
                    eprint!("chmod: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'R' => recursive = true,
                    'v' => verbose = true,
                    'f' => silent = true,
                    _ => {
                        eprint!("chmod: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        } else if spec.is_none() {
            spec = Some(a);
        } else if npaths < paths.len() {
            paths[npaths] = a;
            npaths += 1;
        }
        i += 1;
    }
    let Some(spec) = spec else {
        eprint!("chmod: missing operand\n");
        exit(1);
    };
    if npaths == 0 {
        eprint!("chmod: missing operand after '{}'\n", spec);
        exit(1);
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        if !walk(path, spec, recursive, verbose) {
            if !silent {
                eprint!("chmod: cannot access '{}': no such file or directory\n", path);
            }
            status = 1;
        }
    }
    exit(status)
}
