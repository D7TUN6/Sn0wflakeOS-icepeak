#![no_std]
#![no_main]

use icepeak_user::*;

fn lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() { b + 32 } else { b }
}

fn contains(haystack: &[u8], needle: &[u8], ignore_case: bool) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    for start in 0..=haystack.len() - needle.len() {
        let mut ok = true;
        for j in 0..needle.len() {
            let a = haystack[start + j];
            let b = needle[j];
            if (if ignore_case { lower(a) } else { a }) != (if ignore_case { lower(b) } else { b }) {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

/// Lines longer than this are matched on their prefix only.
const LINE_MAX: usize = 16 * 1024;

struct Grep<'a> {
    pattern: &'a [u8],
    ignore_case: bool,
    invert: bool,
    number: bool,
    count_only: bool,
    files_only: bool,
    quiet: bool,
    single: bool,
    matched_any: bool,
}

impl Grep<'_> {
    fn report(&mut self, path: &str, line_no: usize, line: &[u8]) {
        self.matched_any = true;
        if self.quiet || self.count_only {
            return;
        }
        if self.files_only {
            if line_no != usize::MAX {
                print!("{}\n", path);
            }
            return;
        }
        if !self.single {
            print!("{}:", path);
        }
        if self.number {
            print!("{}:", line_no);
        }
        let _ = write(1, line);
        print!("\n");
    }

    /// Scan one stream line by line. Returns `(hits, read_error)`.
    fn scan(&mut self, path: &str, fd: u64) -> (usize, bool) {
        let buf = aux_io();
        let mut line = [0u8; LINE_MAX];
        let mut len = 0usize;
        let mut line_no = 0usize;
        let mut hits = 0usize;
        let mut listed = false;
        loop {
            let n = read(fd, buf);
            if n < 0 {
                return (hits, true);
            }
            if n == 0 {
                break;
            }
            for &b in &buf[..n as usize] {
                if b != b'\n' {
                    if len < line.len() {
                        line[len] = b;
                        len += 1;
                    }
                    continue;
                }
                line_no += 1;
                if contains(&line[..len], self.pattern, self.ignore_case) != self.invert {
                    hits += 1;
                    if self.files_only {
                        if !listed {
                            listed = true;
                            self.report(path, line_no, &[]);
                        }
                    } else {
                        self.report(path, line_no, &line[..len]);
                    }
                }
                len = 0;
                if self.quiet && self.matched_any {
                    return (hits, false);
                }
            }
        }
        if len > 0 {
            line_no += 1;
            if contains(&line[..len], self.pattern, self.ignore_case) != self.invert {
                hits += 1;
                if self.files_only {
                    if !listed {
                        self.report(path, line_no, &[]);
                    }
                } else {
                    self.report(path, line_no, &line[..len]);
                }
            }
        }
        (hits, false)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut ignore_case = false;
    let mut invert = false;
    let mut number = false;
    let mut count_only = false;
    let mut files_only = false;
    let mut quiet = false;
    let mut pattern: Option<&str> = None;
    let mut paths: [&str; 16] = [""; 16];
    let mut npaths = 0usize;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: grep [OPTION]... PATTERN [FILE]...\n");
            print!("  -i  ignore case\n  -v  invert match\n  -n  show line numbers\n");
            print!("  -c  count matching lines\n  -l  list matching files\n  -q  quiet\n");
            exit(0);
        }
        if a == "--" {
            i += 1;
            continue;
        }
        if a.starts_with('-') && a.len() > 1 && pattern.is_none() {
            for c in a[1..].chars() {
                match c {
                    'i' => ignore_case = true,
                    'v' => invert = true,
                    'n' => number = true,
                    'c' => count_only = true,
                    'l' => files_only = true,
                    'q' => quiet = true,
                    'E' | 'F' => {}
                    _ => {
                        eprint!("grep: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        } else if pattern.is_none() {
            pattern = Some(a);
        } else if npaths < paths.len() {
            paths[npaths] = a;
            npaths += 1;
        }
        i += 1;
    }
    let Some(pattern) = pattern else {
        eprint!("grep: missing pattern\n");
        exit(2);
    };
    let single = npaths <= 1;
    if npaths == 0 {
        paths[0] = "-";
        npaths = 1;
    }
    let mut grep = Grep {
        pattern: pattern.as_bytes(),
        ignore_case,
        invert,
        number,
        count_only,
        files_only,
        quiet,
        single,
        matched_any: false,
    };
    let mut failed = false;
    for path in paths[..npaths].iter() {
        let mut fd = STDIN;
        if *path != "-" {
            let opened = open(path, O_RDONLY);
            if opened < 0 {
                eprint!("grep: {}: no such file\n", path);
                failed = true;
                continue;
            }
            fd = opened as u64;
        }
        let (hits, read_error) = grep.scan(path, fd);
        if fd != STDIN {
            close(fd);
        }
        if read_error {
            eprint!("grep: {}: read error\n", path);
            failed = true;
        }
        if count_only && !quiet {
            if !single {
                print!("{}:", path);
            }
            print!("{}\n", hits);
        }
        if quiet && grep.matched_any {
            exit(0);
        }
    }
    if failed {
        exit(2);
    }
    exit(if grep.matched_any { 0 } else { 1 })
}
