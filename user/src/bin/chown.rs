#![no_std]
#![no_main]

use icepeak_user::*;

fn name_to_id(name: &str) -> Option<(u32, u32)> {
    match name {
        "root" => Some((0, 0)),
        "user" => Some((1000, 1000)),
        "guest" => Some((1001, 1001)),
        "nobody" => Some((65534, 65534)),
        _ => {
            if name.bytes().all(|b| b.is_ascii_digit()) {
                let id = parse_u64(name)? as u32;
                Some((id, id))
            } else {
                None
            }
        }
    }
}

fn parse_owner(spec: &str) -> Option<(Option<u32>, Option<u32>)> {
    let mut parts = spec.splitn(2, ':');
    let owner = parts.next().unwrap_or("");
    let group = parts.next();
    let owner_id = if owner.is_empty() {
        None
    } else {
        Some(name_to_id(owner)?.0)
    };
    let group_id = match group {
        None => None,
        Some("") => owner_id,
        Some(g) => Some(name_to_id(g)?.1),
    };
    Some((owner_id, group_id))
}

fn change_one(path: &str, spec: &str, verbose: bool) -> bool {
    let (owner, group) = match parse_owner(spec) {
        Some(v) => v,
        None => return false,
    };
    let mut buf = [0u8; 128];
    let n = stat(path, &mut buf);
    if n <= 0 {
        return false;
    }
    let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    let mut parts = text.split_whitespace();
    let _kind = parts.next();
    let _size = parts.next();
    let _mode = parts.next();
    let uid = parts.next().and_then(parse_u64).unwrap_or(0) as u32;
    let gid = parts.next().and_then(parse_u64).unwrap_or(0) as u32;
    let new_uid = owner.unwrap_or(uid);
    let new_gid = group.unwrap_or(gid);
    if chown(path, new_uid, new_gid) < 0 {
        return false;
    }
    if verbose {
        print!("changed ownership of '{}' to {}:{}\n", path, new_uid, new_gid);
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
                    print!("Usage: chown [OPTION]... OWNER[:GROUP] FILE...\n");
                    print!("  -R, --recursive  operate on files and directories recursively\n");
                    print!("  -v, --verbose    output a diagnostic for every file\n");
                    print!("  -f, --silent     suppress most error messages\n");
                    exit(0);
                }
                "--recursive" => recursive = true,
                "--verbose" => verbose = true,
                "--silent" | "--quiet" => silent = true,
                _ => {
                    eprint!("chown: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'R' => recursive = true,
                    'v' => verbose = true,
                    'f' => silent = true,
                    'h' => {}
                    _ => {
                        eprint!("chown: invalid option: -{}\n", c);
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
        eprint!("chown: missing operand\n");
        exit(1);
    };
    if npaths == 0 {
        eprint!("chown: missing operand after '{}'\n", spec);
        exit(1);
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        if !walk(path, spec, recursive, verbose) {
            if !silent {
                eprint!("chown: changing ownership of '{}' failed\n", path);
            }
            status = 1;
        }
    }
    exit(status)
}
