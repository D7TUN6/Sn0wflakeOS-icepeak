#![no_std]
#![no_main]

use icepeak_user::*;

fn read_input(path: Option<&str>, buf: &mut [u8]) -> isize {
    match path {
        None | Some("-") => {
            let mut total = 0usize;
            while total < buf.len() {
                let n = read(0, &mut buf[total..]);
                if n <= 0 {
                    break;
                }
                total += n as usize;
            }
            total as isize
        }
        Some(p) => read_file(p, buf),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut lines = 10usize;
    let mut bytes = 0usize;
    let mut quiet = false;
    let mut verbose = false;
    let mut paths: [&str; 16] = [""; 16];
    let mut npaths = 0usize;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: head [OPTION]... [FILE]...\n");
            print!("  -n, --lines=N   print the first N lines (default 10)\n");
            print!("  -c, --bytes=N   print the first N bytes\n");
            print!("  -q, --quiet     never print file headers\n");
            exit(0);
        }
        if a == "-n" || a == "--lines" {
            i += 1;
            lines = args.get(i).and_then(parse_u64).unwrap_or(10) as usize;
        } else if let Some(v) = a.strip_prefix("--lines=") {
            lines = parse_u64(v).unwrap_or(10) as usize;
        } else if a == "-c" || a == "--bytes" {
            i += 1;
            bytes = args.get(i).and_then(parse_u64).unwrap_or(0) as usize;
        } else if let Some(v) = a.strip_prefix("--bytes=") {
            bytes = parse_u64(v).unwrap_or(0) as usize;
        } else if a == "-q" || a == "--quiet" || a == "--silent" {
            quiet = true;
        } else if a == "-v" || a == "--verbose" {
            verbose = true;
        } else if a.starts_with('-') && a.len() > 1 {
            eprint!("head: invalid option: {}\n", a);
            exit(2);
        } else if npaths < paths.len() {
            paths[npaths] = a;
            npaths += 1;
        }
        i += 1;
    }
    if npaths == 0 {
        paths[0] = "-";
        npaths = 1;
    }
    let headers = (npaths > 1 && !quiet) || verbose;
    let buf = aux_io();
    let mut failed = false;
    for (idx, path) in paths[..npaths].iter().enumerate() {
        let n = read_input(if *path == "-" { None } else { Some(path) }, buf);
        if n < 0 {
            eprint!("head: {}: no such file\n", path);
            failed = true;
            continue;
        }
        let data = &buf[..n as usize];
        if headers {
            if idx > 0 {
                print!("\n");
            }
            print!("==> {} <==\n", path);
        }
        if bytes > 0 {
            let take = bytes.min(data.len());
            let _ = write(1, &data[..take]);
        } else {
            let mut count = 0usize;
            let mut start = 0usize;
            for (pos, &b) in data.iter().enumerate() {
                if b == b'\n' {
                    count += 1;
                    if count == lines {
                        let _ = write(1, &data[start..=pos]);
                        start = pos + 1;
                        break;
                    }
                }
            }
            if start < data.len() && (count < lines || lines == 0) {
                let _ = write(1, &data[start..]);
            }
        }
    }
    exit(if failed { 1 } else { 0 })
}
