#![no_std]
#![no_main]

use icepeak_user::*;

fn print_size(n: u64, human: bool) {
    if human {
        let mut buf = [0u8; 16];
        print!("{:>6}\t", human_size(n, &mut buf));
    } else {
        print!("{}\t", (n + 1023) / 1024);
    }
}

fn walk(path: &str, human: bool, all: bool, summarize: bool, top: bool) -> u64 {
    if !is_directory(path) {
        let size = exists(path);
        let size = if size > 0 { size as u64 } else { 0 };
        if !summarize && (all || top) {
            print_size(size, human);
            print!("{}\n", path);
        }
        return size;
    }
    let mut total = 0u64;
    let mut skip = 0usize;
    loop {
        let mut name = [0u8; 128];
        let mut name_len = 0usize;
        {
            let buf = aux_list();
            let n = list(path, buf);
            if n < 0 {
                break;
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
        total += walk(core::str::from_utf8(&child[..cl]).unwrap_or("/"), human, all, summarize, false);
        skip += 1;
    }
    if !summarize || top {
        print_size(total, human);
        print!("{}\n", path);
    }
    total
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut human = false;
    let mut all = false;
    let mut summarize = false;
    let mut grand_total = false;
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
                    print!("Usage: du [OPTION]... [FILE]...\n");
                    print!("  -a, --all        write counts for all files\n");
                    print!("  -s, --summarize  display only a total for each argument\n");
                    print!("  -h, --human-readable  print sizes in human readable format\n");
                    print!("  -c, --total      produce a grand total\n");
                    print!("  -b, --bytes      equivalent to --apparent-size --block-size=1\n");
                    exit(0);
                }
                "--all" => all = true,
                "--summarize" => summarize = true,
                "--human-readable" => human = true,
                "--total" => grand_total = true,
                "--bytes" | "--apparent-size" => {}
                _ => {
                    eprint!("du: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'a' => all = true,
                    's' => summarize = true,
                    'h' => human = true,
                    'c' => grand_total = true,
                    'b' => {}
                    _ => {
                        eprint!("du: invalid option: -{}\n", c);
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
    let mut total = 0u64;
    let mut status = 0;
    for path in paths[..npaths].iter() {
        if !is_directory(path) && exists(path) < 0 {
            eprint!("du: cannot access '{}': no such file or directory\n", path);
            status = 1;
            continue;
        }
        total += walk(path, human, all, summarize, true);
    }
    if grand_total {
        print_size(total, human);
        print!("total\n");
    }
    exit(status)
}
