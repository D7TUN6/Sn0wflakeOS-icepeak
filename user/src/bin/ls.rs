#![no_std]
#![no_main]

use icepeak_user::*;

#[derive(Clone, Copy)]
struct Entry {
    name: [u8; 128],
    name_len: usize,
    is_dir: bool,
    is_link: bool,
    size: u64,
    mode: u16,
    uid: u32,
    gid: u32,
}

impl Entry {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

fn parse_line(line: &[u8]) -> Option<Entry> {
    if line.len() < 5 {
        return None;
    }
    let is_dir = line[0] == b'd';
    let is_link = line[0] == b'l';
    let mut i = 2usize;
    while i < line.len() && line[i] != b' ' {
        i += 1;
    }
    let mut e = Entry {
        name: [0; 128],
        name_len: 0,
        is_dir,
        is_link,
        size: 0,
        mode: 0,
        uid: 0,
        gid: 0,
    };
    e.name_len = (i - 2).min(e.name.len());
    e.name[..e.name_len].copy_from_slice(&line[2..2 + e.name_len]);
    let mut fields = line[i..].split(|&b| b == b' ');
    fields.next();
    let size = fields.next().unwrap_or(b"0");
    let mode = fields.next().unwrap_or(b"0");
    let uid = fields.next().unwrap_or(b"0");
    let gid = fields.next().unwrap_or(b"0");
    e.size = core::str::from_utf8(size)
        .ok()
        .and_then(parse_u64)
        .unwrap_or(0);
    e.mode = core::str::from_utf8(mode)
        .ok()
        .and_then(parse_octal)
        .unwrap_or(0);
    e.uid = core::str::from_utf8(uid)
        .ok()
        .and_then(parse_u64)
        .unwrap_or(0) as u32;
    e.gid = core::str::from_utf8(gid)
        .ok()
        .and_then(parse_u64)
        .unwrap_or(0) as u32;
    Some(e)
}

fn print_long(e: &Entry, human: bool, base: &str) {
    let mut mode_buf = [0u8; 10];
    let mode = mode_string(e.mode, &mut mode_buf);
    if human {
        let mut size_buf = [0u8; 16];
        let size = human_size(e.size, &mut size_buf);
        print!("{} {:>5} {:>5} {:>8} {}", mode, e.uid, e.gid, size, e.name());
    } else {
        print!("{} {:>5} {:>5} {:>8} {}", mode, e.uid, e.gid, e.size, e.name());
    }
    if e.is_dir {
        print!("/");
    } else if e.is_link {
        let mut child = [0u8; 512];
        let cl = if base.is_empty() {
            let n = e.name().len().min(child.len());
            child[..n].copy_from_slice(&e.name().as_bytes()[..n]);
            n
        } else {
            path_join(&mut child, base, e.name())
        };
        let mut target = [0u8; 256];
        let tn = readlink(core::str::from_utf8(&child[..cl]).unwrap_or(""), &mut target);
        if tn > 0 {
            print!(" -> ");
            let _ = write(1, &target[..tn as usize]);
        }
    }
    print!("\n");
}

fn print_short(e: &Entry, classify: bool) {
    print!("{}", e.name());
    if classify {
        if e.is_dir {
            print!("/");
        } else if e.is_link {
            print!("@");
        }
    }
    print!("\n");
}

fn list_dir(path: &str, long: bool, human: bool, classify: bool, show_all: bool) -> bool {
    let buf = aux_list();
    let n = list(path, buf);
    if n < 0 {
        return false;
    }
    if show_all {
        let dot = Entry {
            name: [0; 128],
            name_len: 1,
            is_dir: true,
            is_link: false,
            size: 0,
            mode: 0o040755,
            uid: 0,
            gid: 0,
        };
        let mut dot = dot;
        dot.name[0] = b'.';
        let mut dotdot = dot;
        dotdot.name_len = 2;
        dotdot.name[0] = b'.';
        dotdot.name[1] = b'.';
        if long {
            print_long(&dot, human, path);
            print_long(&dotdot, human, path);
        } else {
            print!("./\n../\n");
        }
    }
    let text = &buf[..n as usize];
    for line in text.split(|&b| b == b'\n') {
        if let Some(e) = parse_line(line) {
            if long {
                print_long(&e, human, path);
            } else {
                print_short(&e, classify);
            }
        }
    }
    true
}

fn list_recursive(path: &str, long: bool, human: bool, classify: bool, show_all: bool) {
    print!("{}:\n", path);
    list_dir(path, long, human, classify, show_all);
    let buf = aux_list();
    let n = list(path, buf);
    if n < 0 {
        return;
    }
    let text = &buf[..n as usize];
    let mut names: [([u8; 128], usize); 64] = [([0; 128], 0); 64];
    let mut count = 0usize;
    for line in text.split(|&b| b == b'\n') {
        if let Some(e) = parse_line(line) {
            if e.is_dir && e.name() != "." && e.name() != ".." && count < names.len() {
                names[count].0[..e.name_len].copy_from_slice(&e.name[..e.name_len]);
                names[count].1 = e.name_len;
                count += 1;
            }
        }
    }
    for (name, len) in names[..count].iter() {
        let mut child = [0u8; 512];
        let cl = path_join(&mut child, path, core::str::from_utf8(&name[..*len]).unwrap_or(""));
        print!("\n");
        list_recursive(core::str::from_utf8(&child[..cl]).unwrap_or("/"), long, human, classify, show_all);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut long = false;
    let mut human = false;
    let mut classify = false;
    let mut show_all = false;
    let mut recursive = false;
    let mut dir_itself = false;
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
                    print!("Usage: ls [OPTION]... [FILE]...\n");
                    print!("  -l  long format\n  -a  include . and ..\n  -h  human readable sizes\n");
                    print!("  -F  append / to directories\n  -R  recurse into directories\n");
                    print!("  -d  list directories themselves\n");
                    exit(0);
                }
                "--all" => show_all = true,
                "--human-readable" => human = true,
                "--recursive" => recursive = true,
                "--classify" => classify = true,
                "--directory" => dir_itself = true,
                _ => {
                    eprint!("ls: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'l' => long = true,
                    'a' => show_all = true,
                    'h' => human = true,
                    'F' => classify = true,
                    'R' => recursive = true,
                    'd' => dir_itself = true,
                    '1' => {}
                    _ => {
                        eprint!("ls: invalid option: -{}\n", c);
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
        paths[0] = ".";
        npaths = 1;
    }
    let mut status = 0;
    for path in paths[..npaths].iter() {
        if is_directory(path) && !dir_itself {
            if npaths > 1 {
                print!("{}:\n", path);
            }
            if recursive {
                list_recursive(path, long, human, classify, show_all);
            } else if !list_dir(path, long, human, classify, show_all) {
                eprint!("ls: cannot open directory: {}\n", path);
                status = 1;
            }
        } else {
            let mut sbuf = [0u8; 128];
            let n = lstat(path, &mut sbuf);
            if n <= 0 {
                eprint!("ls: cannot access '{}': no such file or directory\n", path);
                status = 1;
                continue;
            }
            let text = core::str::from_utf8(&sbuf[..n as usize]).unwrap_or("");
            let mut parts = text.split_whitespace();
            let kind = parts.next().unwrap_or("f");
            let size = parts.next().and_then(parse_u64).unwrap_or(0);
            let mode = parts.next().and_then(parse_octal).unwrap_or(0);
            let uid = parts.next().and_then(parse_u64).unwrap_or(0) as u32;
            let gid = parts.next().and_then(parse_u64).unwrap_or(0) as u32;
            let e = Entry {
                name: [0; 128],
                name_len: 0,
                is_dir: kind == "d",
                is_link: kind == "l",
                size,
                mode,
                uid,
                gid,
            };
            let mut e = e;
            e.name_len = path.len().min(127);
            e.name[..e.name_len].copy_from_slice(&path.as_bytes()[..e.name_len]);
            if long {
                print_long(&e, human, "");
            } else {
                print_short(&e, classify);
            }
        }
    }
    exit(status)
}
