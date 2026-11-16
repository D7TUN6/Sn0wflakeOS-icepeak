#![no_std]
#![no_main]

use icepeak_user::*;

fn show_info() -> ! {
    let mut buf = [0u8; 512];
    let n = cpufreq_get(&mut buf);
    if n <= 0 {
        eprint!("cpupower: cannot read cpufreq state\n");
        exit(1);
    }
    let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("driver") => print!("driver:    {}\n", parts.next().unwrap_or("")),
            Some("governor") => print!("governor:  {}\n", parts.next().unwrap_or("")),
            Some("min") => print!("min:       {} kHz\n", parts.next().unwrap_or("")),
            Some("max") => print!("max:       {} kHz\n", parts.next().unwrap_or("")),
            Some("cur") => print!("current:   {} kHz\n", parts.next().unwrap_or("")),
            Some("util") => print!("util:      {}/1024\n", parts.next().unwrap_or("")),
            Some("available") => {
                let rest = line.strip_prefix("available").unwrap_or("").trim();
                print!("available: {}\n", rest);
            }
            _ => {}
        }
    }
    exit(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    match args.get(1) {
        None | Some("info") | Some("frequency-info") => show_info(),
        Some("--help") | Some("-h") => {
            print!("Usage: cpupower [COMMAND]\n");
            print!("  info                 show driver, governor and limits\n");
            print!("  set GOVERNOR         select performance, powersave or schedutil\n");
            print!("  governor GOVERNOR    alias for set\n");
            exit(0);
        }
        Some("set") | Some("governor") => {
            let Some(name) = args.get(2) else {
                eprint!("cpupower: missing governor name\n");
                exit(2);
            };
            if cpufreq_set(name) < 0 {
                eprint!("cpupower: unknown governor: {}\n", name);
                exit(1);
            }
            exit(0)
        }
        Some(other) => {
            eprint!("cpupower: unknown command: {}\n", other);
            exit(2);
        }
    }
}
