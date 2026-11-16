#![no_std]
#![no_main]

use icepeak_user::*;

fn policy_name(p: u8) -> &'static str {
    match p {
        1 => "fifo",
        2 => "rr",
        _ => "other",
    }
}

fn parse_policy(s: &str) -> Option<u8> {
    match s {
        "other" | "0" => Some(0),
        "fifo" | "1" => Some(1),
        "rr" | "2" => Some(2),
        _ => None,
    }
}

fn command_path(buf: &mut [u8; 128], cmd: &str) -> usize {
    let mut n = 0usize;
    if !cmd.as_bytes().contains(&b'/') {
        for &b in b"/bin/" {
            buf[n] = b;
            n += 1;
        }
    }
    for &b in cmd.as_bytes() {
        if n + 1 < buf.len() {
            buf[n] = b;
            n += 1;
        }
    }
    n
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut policy: u8 = 0;
    let mut pid_mode = false;
    let mut verbose = false;
    let mut operands: [&str; 32] = [""; 32];
    let mut nops = 0usize;
    let mut i = 1usize;
    let mut end_opts = false;
    while let Some(a) = args.get(i) {
        if !end_opts && a == "--" {
            end_opts = true;
        } else if !end_opts && a.starts_with("--") {
            match a {
                "--help" => {
                    print!("Usage: chrt [OPTION]... PRIORITY COMMAND [ARG]...\n");
                    print!("       chrt [OPTION]... -p [PRIORITY] PID\n");
                    print!("  -o, --other   SCHED_OTHER (default)\n");
                    print!("  -f, --fifo    SCHED_FIFO\n");
                    print!("  -r, --rr      SCHED_RR\n");
                    print!("  -b, --batch   SCHED_BATCH (mapped to OTHER)\n");
                    print!("  -i, --idle    SCHED_IDLE (mapped to OTHER)\n");
                    print!("  -p, --pid     operate on an existing PID\n");
                    print!("  -v, --verbose print the resulting policy\n");
                    exit(0);
                }
                "--other" => policy = 0,
                "--fifo" => policy = 1,
                "--rr" => policy = 2,
                "--batch" | "--idle" | "--deadline" => policy = 0,
                "--pid" => pid_mode = true,
                "--verbose" => verbose = true,
                "--max" | "--all-tasks" | "--reset-on-fork" => {}
                _ => {
                    eprint!("chrt: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'o' => policy = 0,
                    'f' => policy = 1,
                    'r' => policy = 2,
                    'b' | 'i' | 'd' => policy = 0,
                    'p' => pid_mode = true,
                    'v' => verbose = true,
                    'm' | 'a' | 'R' => {}
                    _ => {
                        eprint!("chrt: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        } else if nops < operands.len() {
            operands[nops] = a;
            nops += 1;
        }
        i += 1;
    }
    if pid_mode {
        if nops == 0 {
            eprint!("chrt: missing PID\n");
            exit(1);
        }
        if nops == 1 {
            let pid = parse_u64(operands[0]).unwrap_or(0);
            let mut buf = [0u8; 64];
            let n = sched_get(pid, &mut buf);
            if n < 0 {
                eprint!("chrt: failed to get pid {}'s policy\n", pid);
                exit(1);
            }
            let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
            let mut parts = text.split_whitespace();
            let p = parts.next().and_then(parse_policy).unwrap_or(0);
            let pri = parts.next().and_then(parse_u64).unwrap_or(0);
            print!("pid {}'s current scheduling policy: {}\n", pid, policy_name(p));
            print!("pid {}'s current scheduling priority: {}\n", pid, pri);
            exit(0);
        }
        let priority = parse_u64(operands[0]).unwrap_or(0) as i32;
        let pid = parse_u64(operands[1]).unwrap_or(0);
        if sched_set(pid, policy, priority) < 0 {
            eprint!("chrt: failed to set pid {}'s policy\n", pid);
            exit(1);
        }
        if verbose {
            print!("pid {}'s new scheduling policy: {}\n", pid, policy_name(policy));
            print!("pid {}'s new scheduling priority: {}\n", pid, priority);
        }
        exit(0);
    }
    if nops < 2 {
        eprint!("chrt: missing command\n");
        exit(1);
    }
    let priority = parse_u64(operands[0]).unwrap_or(0) as i32;
    let pid = getpid();
    if sched_set(pid, policy, priority) < 0 {
        eprint!("chrt: failed to set scheduling policy\n");
        exit(1);
    }
    if verbose {
        print!("pid {}'s new scheduling policy: {}\n", pid, policy_name(policy));
        print!("pid {}'s new scheduling priority: {}\n", pid, priority);
    }
    let mut path_buf = [0u8; 128];
    let plen = command_path(&mut path_buf, operands[1]);
    let path = core::str::from_utf8(&path_buf[..plen]).unwrap_or(operands[1]);
    let pid = exec(path, &operands[1..nops]);
    if pid < 0 {
        eprint!("chrt: failed to execute '{}'\n", operands[1]);
        exit(127);
    }
    exit(0)
}
