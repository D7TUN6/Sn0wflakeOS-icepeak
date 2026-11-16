#![no_std]
#![no_main]

use icepeak_user::*;

fn two(n: u64) -> [u8; 2] {
    [b'0' + ((n / 10) % 10) as u8, b'0' + (n % 10) as u8]
}

fn print_uptime(ms: u64) {
    let secs = ms / 1000;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    if days > 0 {
        print!("{} day{}, ", days, if days == 1 { "" } else { "s" });
    }
    let hh = two(h);
    let mm = two(m);
    let ss = two(s);
    let _ = write(1, &hh);
    print!(":");
    let _ = write(1, &mm);
    print!(":");
    let _ = write(1, &ss);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut count = u64::MAX;
    let mut delay_ms = 2000u64;
    let mut batch = false;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" {
            print!("Usage: top [OPTION]...\n");
            print!("  -n, --iterations=N  stop after N iterations\n");
            print!("  -d, --delay=SEC     delay between updates (default 2)\n");
            print!("  -b, --batch         batch mode (no screen clearing)\n");
            exit(0);
        } else if a == "-n" || a == "--iterations" {
            i += 1;
            count = args.get(i).and_then(parse_u64).unwrap_or(1);
        } else if let Some(v) = a.strip_prefix("--iterations=") {
            count = parse_u64(v).unwrap_or(1);
        } else if a == "-d" || a == "--delay" {
            i += 1;
            delay_ms = args.get(i).and_then(parse_u64).unwrap_or(2).saturating_mul(1000);
        } else if let Some(v) = a.strip_prefix("--delay=") {
            delay_ms = parse_u64(v).unwrap_or(2).saturating_mul(1000);
        } else if a == "-b" || a == "--batch" {
            batch = true;
        } else if a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'n' => {
                        i += 1;
                        count = args.get(i).and_then(parse_u64).unwrap_or(1);
                    }
                    'd' => {
                        i += 1;
                        delay_ms = args.get(i).and_then(parse_u64).unwrap_or(2).saturating_mul(1000);
                    }
                    'b' => batch = true,
                    _ => {
                        eprint!("top: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        }
        i += 1;
    }
    let mut last_ns = 0u64;
    let mut last_time = 0u64;
    let mut iteration = 0u64;
    while iteration < count {
        let now = time_ms();
        let mut mem_buf = [0u8; 256];
        let m = meminfo(&mut mem_buf);
        let mem_text = if m > 0 {
            core::str::from_utf8(&mem_buf[..m as usize]).unwrap_or("")
        } else {
            ""
        };
        let heap_total = stat_field(mem_text, "total").unwrap_or(0);
        let heap_used = stat_field(mem_text, "used").unwrap_or(0);
        let heap_free = stat_field(mem_text, "free").unwrap_or(0);
        let mut frame_used = 0u64;
        let mut frame_total = 0u64;
        for line in mem_text.lines() {
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
        let mut ticks = 0u64;
        let mut cpu_ns = 0u64;
        let mut cpus = 1u64;
        let mut governor = "";
        let mut tasks = 0u64;
        let mut running = 0u64;
        for line in mem_text.lines() {
            let mut parts = line.split_whitespace();
            match parts.next() {
                Some("tasks") => match parts.next() {
                    Some("total") => tasks = parts.next().and_then(parse_u64).unwrap_or(0),
                    Some("runnable") => running = parts.next().and_then(parse_u64).unwrap_or(0),
                    _ => {}
                },
                Some("cpu") => match parts.next() {
                    Some("ns") => cpu_ns = parts.next().and_then(parse_u64).unwrap_or(0),
                    Some("ticks") => ticks = parts.next().and_then(parse_u64).unwrap_or(0),
                    _ => {}
                },
                Some("cpus") => {
                    let _ = parts.next();
                    cpus = parts.next().and_then(parse_u64).unwrap_or(1).max(1);
                }
                Some("governor") => governor = parts.next().unwrap_or(""),
                _ => {}
            }
        }
        let delta_ns = cpu_ns.saturating_sub(last_ns);
        let delta_time = now.saturating_sub(last_time);
        let capacity_ns = delta_time.saturating_mul(1_000_000).saturating_mul(cpus);
        let cpu_pct = if last_time > 0 && capacity_ns > 0 {
            ((delta_ns.saturating_mul(100)) / capacity_ns).min(100)
        } else {
            0
        };
        last_ns = cpu_ns;
        last_time = now;
        if !batch {
            // Home the cursor and repaint in place. The console only touches
            // cells whose contents changed, so this costs far less than a
            // full-screen clear and does not flicker.
            print!("\x1b[H");
        }
        // Every line ends with an erase-to-end-of-line: rows that were blank in
        // this frame but held text in the previous one must not keep it, and a
        // shorter value must not leave its tail behind. Batch output goes to a
        // pipe or file, so it stays free of escape sequences.
        let end = if batch { "\n" } else { "\x1b[K\n" };
        print!("top - up ");
        print_uptime(now);
        print!("  ({} iterations){}", iteration + 1, end);
        print!("Tasks: {} total, {} runnable{}", tasks, running, end);
        print!(
            "Mem:   {} total, {} used, {} free ({} frames used of {}){}",
            heap_total, heap_used, heap_free, frame_used, frame_total, end
        );
        print!(
            "Cpu:   {} cpus, {} ms busy, {}% busy, governor {}{}",
            cpus, ticks, cpu_pct, governor, end
        );
        print!(
            "{}  PID NAME             STATE   UID  CPUms POLICY PRI      MEM{}",
            end, end
        );
        let buf = aux_list();
        let n = taskstats(buf);
        if n > 0 {
            let _ = write(1, &buf[..n as usize]);
        }
        if !batch {
            print!("\x1b[J");
        }
        iteration += 1;
        if iteration < count && delay_ms > 0 {
            sleep_ms(delay_ms);
        }
    }
    exit(0)
}
