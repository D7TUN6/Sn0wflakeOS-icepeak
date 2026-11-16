#![no_std]
#![no_main]

use icepeak_user::*;

fn user_ids(name: &str) -> Option<(u32, u32)> {
    match name {
        "root" => Some((0, 0)),
        "user" => Some((1000, 1000)),
        "guest" => Some((1001, 1001)),
        "nobody" => Some((65534, 65534)),
        _ => {
            if name.bytes().all(|b| b.is_ascii_digit()) {
                let id = parse_u64(name)? as u32;
                Some((id, id))
            } else {
                None
            }
        }
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
    let mut login = false;
    let mut command: Option<&str> = None;
    let mut shell: Option<&str> = None;
    let mut user: Option<&str> = None;
    let mut rest: [&str; 32] = [""; 32];
    let mut nrest = 0usize;
    let mut i = 1usize;
    let mut end_opts = false;
    while let Some(a) = args.get(i) {
        if !end_opts && a == "--" {
            end_opts = true;
        } else if !end_opts && a.starts_with("--") {
            match a {
                "--help" => {
                    print!("Usage: su [OPTION]... [-] [USER [ARG]...]\n");
                    print!("  -l, --login        start a login shell\n");
                    print!("  -c, --command=CMD  run CMD instead of an interactive shell\n");
                    print!("  -s, --shell=SHELL  use SHELL\n");
                    exit(0);
                }
                "--login" => login = true,
                "--command" => {
                    i += 1;
                    command = args.get(i);
                }
                "--shell" => {
                    i += 1;
                    shell = args.get(i);
                }
                _ if a.starts_with("--command=") => command = Some(&a[10..]),
                _ if a.starts_with("--shell=") => shell = Some(&a[8..]),
                _ => {
                    eprint!("su: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if !end_opts && a == "-" {
            login = true;
        } else if !end_opts && a.starts_with('-') && a.len() > 1 {
            let mut chars = a[1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    'l' => login = true,
                    'c' => {
                        i += 1;
                        command = args.get(i);
                    }
                    's' => {
                        i += 1;
                        shell = args.get(i);
                    }
                    _ => {
                        eprint!("su: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        } else if user.is_none() {
            user = Some(a);
        } else if nrest < rest.len() {
            rest[nrest] = a;
            nrest += 1;
        }
        i += 1;
    }
    let _ = login;
    let target = user.unwrap_or("root");
    let Some((uid, gid)) = user_ids(target) else {
        eprint!("su: user '{}' does not exist\n", target);
        exit(1);
    };
    let current = getuid();
    if current != 0 && current != uid {
        eprint!("su: permission denied\n");
        exit(1);
    }
    if setuid(uid, gid) < 0 {
        eprint!("su: cannot switch to user '{}'\n", target);
        exit(1);
    }
    if let Some(cmd) = command {
        let mut cmd_args: [&str; 32] = [""; 32];
        cmd_args[0] = cmd;
        let mut n = 1usize;
        for &arg in rest[..nrest].iter() {
            if n < cmd_args.len() {
                cmd_args[n] = arg;
                n += 1;
            }
        }
        let mut path_buf = [0u8; 128];
        let plen = command_path(&mut path_buf, cmd);
        let path = core::str::from_utf8(&path_buf[..plen]).unwrap_or(cmd);
        let pid = exec(path, &cmd_args[..n]);
        if pid < 0 {
            eprint!("su: failed to execute '{}'\n", cmd);
            exit(127);
        }
        exit(0);
    }
    let shell_path = shell.unwrap_or("/bin/peaksh");
    let pid = exec(shell_path, &[shell_path]);
    if pid < 0 {
        eprint!("su: failed to execute {}\n", shell_path);
        exit(127);
    }
    exit(0)
}
