#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut human = false;
    let mut raw = false;
    let mut total_line = false;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" {
            print!("Usage: free [OPTION]...\n");
            print!("  -b, --bytes   show output in bytes\n");
            print!("  -k, --kibi    show output in KiB (default)\n");
            print!("  -m, --mebi    show output in MiB\n");
            print!("  -g, --gibi    show output in GiB\n");
            print!("  -h, --human   show human readable output\n");
            print!("  -t, --total   show a total line\n");
            exit(0);
        } else if a.starts_with("--") {
            match a {
                "--bytes" => raw = true,
                "--kibi" | "--mebi" | "--gibi" => {}
                "--human" => human = true,
                "--total" => total_line = true,
                _ => {
                    eprint!("free: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'b' => raw = true,
                    'k' | 'm' | 'g' => {}
                    'h' => human = true,
                    't' => total_line = true,
                    _ => {
                        eprint!("free: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        }
        i += 1;
    }
    let mut buf = [0u8; 256];
    let n = meminfo(&mut buf);
    if n <= 0 {
        eprint!("free: cannot read memory statistics\n");
        exit(1);
    }
    let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    let heap_total = stat_field(text, "total").unwrap_or(0);
    let heap_used = stat_field(text, "used").unwrap_or(0);
    let heap_free = stat_field(text, "free").unwrap_or(0);
    let mut frame_used = 0u64;
    let mut frame_total = 0u64;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("frames") {
            continue;
        }
        match parts.next() {
            Some("used") => frame_used = parts.next().and_then(parse_u64).unwrap_or(0),
            Some("total") => frame_total = parts.next().and_then(parse_u64).unwrap_or(0),
            _ => {}
        }
    }
    let phys_total = frame_total.saturating_mul(4096);
    let phys_used = frame_used.saturating_mul(4096);
    let phys_free = phys_total.saturating_sub(phys_used);
    print!("              total        used        free      shared  buff/cache\n");
    print!("Mem:      ");
    print_value(phys_total, human, raw);
    print_value(phys_used, human, raw);
    print_value(phys_free, human, raw);
    print_value(0, human, raw);
    print_value(heap_used, human, raw);
    print!("\n");
    print!("Heap:     ");
    print_value(heap_total, human, raw);
    print_value(heap_used, human, raw);
    print_value(heap_free, human, raw);
    print_value(0, human, raw);
    print_value(0, human, raw);
    print!("\n");
    if total_line {
        print!("Total:    ");
        print_value(phys_total, human, raw);
        print_value(phys_used, human, raw);
        print_value(phys_free, human, raw);
        print_value(0, human, raw);
        print_value(heap_used, human, raw);
        print!("\n");
    }
    exit(0)
}

fn print_value(n: u64, human: bool, raw: bool) {
    if raw {
        print!("{:>12}", n);
    } else if human {
        let mut buf = [0u8; 16];
        print!("{:>12}", human_size(n, &mut buf));
    } else {
        print!("{:>12}", (n + 1023) / 1024);
    }
}
