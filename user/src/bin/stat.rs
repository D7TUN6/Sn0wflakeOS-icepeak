#![no_std]
#![no_main]

use icepeak_user::*;

fn user_name(uid: u32) -> &'static str {
    match uid {
        0 => "root",
        1000 => "user",
        1001 => "guest",
        65534 => "nobody",
        _ => "unknown",
    }
}

fn group_name(gid: u32) -> &'static str {
    match gid {
        0 => "root",
        1000 => "users",
        1001 => "guest",
        65534 => "nogroup",
        _ => "unknown",
    }
}

fn type_name(is_dir: bool, is_link: bool) -> &'static str {
    if is_dir {
        "directory"
    } else if is_link {
        "symbolic link"
    } else {
        "regular file"
    }
}

fn print_formatted(
    fmt: &str,
    path: &str,
    is_dir: bool,
    is_link: bool,
    size: u64,
    mode: u16,
    uid: u32,
    gid: u32,
) {
    let bytes = fmt.as_bytes();
    let mut i = 0usize;
    let mut mode_buf = [0u8; 10];
    let mode_str = mode_string(mode, &mut mode_buf);
    while i < bytes.len() {
        if bytes[i] != b'%' || i + 1 >= bytes.len() {
            let _ = write(1, &bytes[i..i + 1]);
            i += 1;
            continue;
        }
        i += 1;
        match bytes[i] {
            b'n' => print!("{}", path),
            b'N' => print!("'{}'", path),
            b's' => print!("{}", size),
            b'a' => print!("{:o}", mode & 0o7777),
            b'A' => print!("{}", mode_str),
            b'u' => print!("{}", uid),
            b'g' => print!("{}", gid),
            b'U' => print!("{}", user_name(uid)),
            b'G' => print!("{}", group_name(gid)),
            b'F' => print!("{}", type_name(is_dir, is_link)),
            b'i' => print!("0"),
            b'h' => print!("1"),
            b'x' | b'y' | b'z' => print!("-"),
            b'%' => print!("%"),
            other => {
                print!("%");
                let _ = write(1, &[other]);
            }
        }
        i += 1;
    }
    print!("\n");
}

fn stat_one(path: &str, format: Option<&str>, terse: bool, deref: bool) -> bool {
    let mut buf = [0u8; 128];
    let n = if deref {
        stat(path, &mut buf)
    } else {
        lstat(path, &mut buf)
    };
    if n <= 0 {
        eprint!("stat: cannot stat '{}': no such file or directory\n", path);
        return false;
    }
    let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    let mut parts = text.split_whitespace();
    let kind = parts.next().unwrap_or("f");
    let size = parts.next().and_then(parse_u64).unwrap_or(0);
    let mode = parts.next().and_then(parse_octal).unwrap_or(0);
    let uid = parts.next().and_then(parse_u64).unwrap_or(0) as u32;
    let gid = parts.next().and_then(parse_u64).unwrap_or(0) as u32;
    let is_dir = kind == "d";
    let is_link = kind == "l";
    if let Some(fmt) = format {
        print_formatted(fmt, path, is_dir, is_link, size, mode, uid, gid);
        return true;
    }
    let mut mode_buf = [0u8; 10];
    let mode_str = mode_string(mode, &mut mode_buf);
    if terse {
        print!(
            "{} {} {} {} {} {} {}\n",
            path,
            size,
            mode & 0o7777,
            mode_str,
            uid,
            gid,
            type_name(is_dir, is_link)
        );
        return true;
    }
    print!("  File: {}\n", path);
    print!(
        "  Size: {:<10} Blocks: 0          IO Block: 4096   {}\n",
        size,
        type_name(is_dir, is_link)
    );
    print!(
        "Access: (0{:o}/{})  Uid: ({:>5}/{:>8})   Gid: ({:>5}/{:>8})\n",
        mode & 0o7777,
        mode_str,
        uid,
        user_name(uid),
        gid,
        group_name(gid)
    );
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut format: Option<&str> = None;
    let mut terse = false;
    let mut deref = false;
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
                    print!("Usage: stat [OPTION]... FILE...\n");
                    print!("  -c, --format=FORMAT  use FORMAT instead of the default\n");
                    print!("  -t, --terse          print the information in terse form\n");
                    print!("  -L, --dereference    follow links\n");
                    exit(0);
                }
                "--format" => {
                    i += 1;
                    format = args.get(i);
                }
                "--terse" => terse = true,
                "--dereference" => deref = true,
                _ if a.starts_with("--format=") => {
                    format = Some(&a[9..]);
                }
                _ => {
                    eprint!("stat: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            let mut chars = a[1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    'c' => {
                        i += 1;
                        format = args.get(i);
                    }
                    't' => terse = true,
                    'L' => deref = true,
                    _ => {
                        eprint!("stat: invalid option: -{}\n", c);
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
        eprint!("stat: missing operand\n");
        exit(1);
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        if !stat_one(path, format, terse, deref) {
            status = 1;
        }
    }
    exit(status)
}
