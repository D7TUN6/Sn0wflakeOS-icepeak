#![no_std]
#![no_main]

use icepeak_user::*;

struct Counts {
    lines: u64,
    words: u64,
    bytes: u64,
    chars: u64,
    max_line: u64,
}

/// Incremental counter so a stream of any size can be measured with one
/// buffer. Word and line state carries across chunk boundaries.
struct Counter {
    counts: Counts,
    in_word: bool,
    line_len: u64,
}

impl Counter {
    fn new() -> Counter {
        Counter {
            counts: Counts {
                lines: 0,
                words: 0,
                bytes: 0,
                chars: 0,
                max_line: 0,
            },
            in_word: false,
            line_len: 0,
        }
    }

    fn feed(&mut self, data: &[u8]) {
        let c = &mut self.counts;
        for &b in data {
            c.bytes += 1;
            // `-m` counts characters: a UTF-8 continuation byte is not one.
            if b & 0xC0 != 0x80 {
                c.chars += 1;
            }
            if b == b'\n' {
                c.lines += 1;
                if self.line_len > c.max_line {
                    c.max_line = self.line_len;
                }
                self.line_len = 0;
            } else {
                self.line_len += 1;
            }
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.in_word = false;
            } else if !self.in_word {
                self.in_word = true;
                c.words += 1;
            }
        }
    }

    fn finish(mut self) -> Counts {
        if self.line_len > self.counts.max_line {
            self.counts.max_line = self.line_len;
        }
        self.counts
    }
}

/// Stream one input into `counter`. Returns false if it could not be opened.
fn count_input(path: &str, counter: &mut Counter) -> bool {
    let mut fd = STDIN;
    if path != "-" {
        let opened = open(path, O_RDONLY);
        if opened < 0 {
            return false;
        }
        fd = opened as u64;
    }
    let buf = aux_io();
    loop {
        let n = read(fd, buf);
        if n < 0 {
            if fd != STDIN {
                close(fd);
            }
            return false;
        }
        if n == 0 {
            break;
        }
        counter.feed(&buf[..n as usize]);
    }
    if fd != STDIN {
        close(fd);
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut show_lines = false;
    let mut show_words = false;
    let mut show_bytes = false;
    let mut show_chars = false;
    let mut show_max = false;
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
                    print!("Usage: wc [OPTION]... [FILE]...\n");
                    print!("  -l, --lines   print the newline counts\n");
                    print!("  -w, --words   print the word counts\n");
                    print!("  -c, --bytes   print the byte counts\n");
                    print!("  -m, --chars   print the character counts\n");
                    print!("  -L, --max-line-length  print the longest line length\n");
                    exit(0);
                }
                "--lines" => show_lines = true,
                "--words" => show_words = true,
                "--bytes" => show_bytes = true,
                "--chars" => show_chars = true,
                "--max-line-length" => show_max = true,
                _ => {
                    eprint!("wc: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'l' => show_lines = true,
                    'w' => show_words = true,
                    'c' => show_bytes = true,
                    'm' => show_chars = true,
                    'L' => show_max = true,
                    _ => {
                        eprint!("wc: invalid option: -{}\n", c);
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
    let any_selected = show_lines || show_words || show_bytes || show_chars || show_max;
    if !any_selected {
        show_lines = true;
        show_words = true;
        show_bytes = true;
    }
    if npaths == 0 {
        paths[0] = "-";
        npaths = 1;
    }
    let mut total = Counts {
        lines: 0,
        words: 0,
        bytes: 0,
        chars: 0,
        max_line: 0,
    };
    let mut status = 0;
    for path in paths[..npaths].iter() {
        let mut counter = Counter::new();
        if !count_input(path, &mut counter) {
            eprint!("wc: {}: no such file or directory\n", path);
            status = 1;
            continue;
        }
        let c = counter.finish();
        total.lines += c.lines;
        total.words += c.words;
        total.bytes += c.bytes;
        total.chars += c.chars;
        if c.max_line > total.max_line {
            total.max_line = c.max_line;
        }
        print_counts(
            &c,
            show_lines,
            show_words,
            show_bytes,
            show_chars,
            show_max,
            if *path == "-" { "" } else { path },
        );
    }
    if npaths > 1 {
        print_counts(
            &total,
            show_lines,
            show_words,
            show_bytes,
            show_chars,
            show_max,
            "total",
        );
    }
    exit(status)
}

fn print_counts(
    c: &Counts,
    lines: bool,
    words: bool,
    bytes: bool,
    chars: bool,
    max_line: bool,
    name: &str,
) {
    if lines {
        print!("{:>7}", c.lines);
    }
    if words {
        print!("{:>7}", c.words);
    }
    if bytes {
        print!("{:>7}", c.bytes);
    }
    if chars {
        print!("{:>7}", c.chars);
    }
    if max_line {
        print!("{:>7}", c.max_line);
    }
    if !name.is_empty() {
        print!(" {}", name);
    }
    print!("\n");
}
