#![no_std]
#![no_main]

use icepeak_user::*;

/// Small write buffer so the formatted path emits bytes in blocks instead of
/// one syscall per character.
struct Out {
    buf: [u8; 4096],
    len: usize,
}

impl Out {
    fn new() -> Out {
        Out {
            buf: [0; 4096],
            len: 0,
        }
    }

    fn put(&mut self, b: u8) {
        if self.len == self.buf.len() {
            self.flush();
        }
        self.buf[self.len] = b;
        self.len += 1;
    }

    fn put_slice(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.put(b);
        }
    }

    fn number(&mut self, mut n: u64) {
        let mut digits = [b' '; 6];
        let mut i = digits.len();
        if n == 0 {
            i -= 1;
            digits[i] = b'0';
        }
        while n > 0 && i > 0 {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        self.put_slice(&digits);
        self.put(b'\t');
    }

    fn flush(&mut self) {
        if self.len > 0 {
            let _ = write(1, &self.buf[..self.len]);
            self.len = 0;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut number = false;
    let mut number_nonblank = false;
    let mut squeeze_blank = false;
    let mut show_ends = false;
    let mut show_tabs = false;
    let mut show_all = false;
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
                    print!("Usage: cat [OPTION]... [FILE]...\n");
                    print!("  -n  number all output lines\n  -b  number nonblank lines\n");
                    print!("  -s  squeeze repeated blank lines\n  -E  show $ at line ends\n");
                    print!("  -T  show tabs as ^I\n  -A  equivalent to -vET\n");
                    exit(0);
                }
                "--number" => number = true,
                "--number-nonblank" => number_nonblank = true,
                "--squeeze-blank" => squeeze_blank = true,
                "--show-ends" => show_ends = true,
                "--show-tabs" => show_tabs = true,
                "--show-all" => show_all = true,
                _ => {
                    eprint!("cat: unrecognized option: {}\n", a);
                    exit(1);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'n' => number = true,
                    'b' => number_nonblank = true,
                    's' => squeeze_blank = true,
                    'E' => show_ends = true,
                    'T' => show_tabs = true,
                    'A' => show_all = true,
                    'u' => {}
                    _ => {
                        eprint!("cat: invalid option: -{}\n", c);
                        exit(1);
                    }
                }
            }
        } else if npaths < paths.len() {
            paths[npaths] = a;
            npaths += 1;
        }
        i += 1;
    }
    if show_all {
        show_ends = true;
        show_tabs = true;
    }
    if npaths == 0 {
        paths[0] = "-";
        npaths = 1;
    }
    let plain = !number && !number_nonblank && !squeeze_blank && !show_ends && !show_tabs;
    let buf = aux_io();
    let mut out = Out::new();
    let mut status = 0;
    let mut line_no = 0u64;
    let mut line_start = true;
    let mut last_blank = false;
    for path in paths[..npaths].iter() {
        let mut fd = STDIN;
        if *path != "-" {
            let opened = open(path, O_RDONLY);
            if opened < 0 {
                eprint!("cat: {}: no such file or directory\n", path);
                status = 1;
                continue;
            }
            fd = opened as u64;
        }
        loop {
            let n = read(fd, buf);
            if n < 0 {
                status = 1;
                break;
            }
            if n == 0 {
                break;
            }
            let data = &buf[..n as usize];
            if plain {
                if write(1, data) != n {
                    status = 1;
                    break;
                }
                continue;
            }
            for &b in data {
                if line_start {
                    if b == b'\n' {
                        if squeeze_blank && last_blank {
                            continue;
                        }
                        last_blank = true;
                        if number {
                            line_no += 1;
                            out.number(line_no);
                        }
                    } else {
                        last_blank = false;
                        if number || number_nonblank {
                            line_no += 1;
                            out.number(line_no);
                        }
                    }
                    line_start = false;
                }
                if b == b'\t' && show_tabs {
                    out.put_slice(b"^I");
                } else if b == b'\n' {
                    if show_ends {
                        out.put(b'$');
                    }
                    out.put(b'\n');
                    line_start = true;
                } else {
                    out.put(b);
                }
            }
        }
        if fd != STDIN {
            close(fd);
        }
        if status != 0 {
            break;
        }
    }
    out.flush();
    exit(status)
}
