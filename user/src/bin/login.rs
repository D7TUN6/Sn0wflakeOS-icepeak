#![no_std]
#![no_main]

use icepeak_user::*;

struct Account {
    name: [u8; 32],
    name_len: usize,
    uid: u32,
    gid: u32,
    home: [u8; 64],
    home_len: usize,
    shell: [u8; 64],
    shell_len: usize,
}

impl Account {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
    fn home(&self) -> &str {
        core::str::from_utf8(&self.home[..self.home_len]).unwrap_or("/")
    }
    fn shell(&self) -> &str {
        core::str::from_utf8(&self.shell[..self.shell_len]).unwrap_or("/bin/peaksh")
    }
}

fn copy_into(dst: &mut [u8], src: &[u8]) -> usize {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    n
}

fn field<'a>(line: &'a [u8], index: usize) -> &'a [u8] {
    let mut start = 0usize;
    let mut current = 0usize;
    let mut i = 0usize;
    while i <= line.len() {
        if i == line.len() || line[i] == b':' {
            if current == index {
                return &line[start..i];
            }
            current += 1;
            start = i + 1;
        }
        i += 1;
    }
    &[]
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0u32;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    Some(value)
}

/// Look `name` up in `/etc/passwd`.
fn lookup(name: &str) -> Option<Account> {
    let mut buf = [0u8; 2048];
    let n = read_file("/etc/passwd", &mut buf);
    if n <= 0 {
        return None;
    }
    let text = &buf[..n as usize];
    for line in text.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if field(line, 0) != name.as_bytes() {
            continue;
        }
        let uid = parse_u32(field(line, 2))?;
        let gid = parse_u32(field(line, 3))?;
        let mut account = Account {
            name: [0; 32],
            name_len: 0,
            uid,
            gid,
            home: [0; 64],
            home_len: 0,
            shell: [0; 64],
            shell_len: 0,
        };
        account.name_len = copy_into(&mut account.name, name.as_bytes());
        account.home_len = copy_into(&mut account.home, field(line, 5));
        account.shell_len = copy_into(&mut account.shell, field(line, 6));
        if account.home_len == 0 {
            account.home_len = copy_into(&mut account.home, b"/");
        }
        return Some(account);
    }
    None
}

/// The stored password from `/etc/shadow`; empty means no password.
fn stored_password(name: &str, out: &mut [u8; 64]) -> usize {
    let mut buf = [0u8; 2048];
    let n = read_file("/etc/shadow", &mut buf);
    if n <= 0 {
        return 0;
    }
    let text = &buf[..n as usize];
    for line in text.split(|&b| b == b'\n') {
        if line.is_empty() || field(line, 0) != name.as_bytes() {
            continue;
        }
        return copy_into(out, field(line, 1));
    }
    0
}

/// Read one line from the console. With `echo == false` (passwords) the input
/// is not written back, matching a normal getty.
fn read_line(buf: &mut [u8], echo: bool) -> usize {
    let mut len = 0usize;
    let mut byte = [0u8; 1];
    loop {
        let n = read(0, &mut byte);
        if n <= 0 {
            continue;
        }
        match byte[0] {
            b'\r' | b'\n' => {
                if echo {
                    print!("\n");
                }
                return len;
            }
            0x7F | 0x08 => {
                if len > 0 {
                    len -= 1;
                    if echo {
                        print!("\x08 \x08");
                    }
                }
            }
            b if b >= 0x20 && b < 0x7F => {
                if len + 1 < buf.len() {
                    buf[len] = b;
                    len += 1;
                    if echo {
                        let _ = write(1, &[b]);
                    }
                }
            }
            _ => {}
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(_argc: usize, _argv: *const *const u8) -> ! {
    print!("\nSn0wflakeOS 0.1.0 (icepeak) console\n\n");
    loop {
        let mut name_buf = [0u8; 32];
        print!("sn0wflake login: ");
        let mut name_len = read_line(&mut name_buf, true);
        if name_len == 0 {
            name_len = copy_into(&mut name_buf, b"user");
        }
        let name = core::str::from_utf8(&name_buf[..name_len]).unwrap_or("");

        let Some(account) = lookup(name) else {
            print!("Login incorrect\n\n");
            continue;
        };

        let mut expected = [0u8; 64];
        let expected_len = stored_password(name, &mut expected);
        if expected_len > 0 {
            let mut password = [0u8; 64];
            print!("Password: ");
            let password_len = read_line(&mut password, false);
            print!("\n");
            if password_len != expected_len
                || &password[..password_len] != &expected[..expected_len]
            {
                print!("Login incorrect\n\n");
                continue;
            }
        }

        if setuid(account.uid, account.gid) < 0 {
            print!("login: cannot set user credentials\n");
            exit(1);
        }
        let _ = chdir(account.home());

        let mut motd = [0u8; 256];
        let motd_len = read_file("/etc/motd", &mut motd);
        if motd_len > 0 {
            let _ = write(1, &motd[..motd_len as usize]);
        }

        let shell = account.shell();
        let pid = exec(shell, &[shell, "-l"]);
        if pid < 0 {
            eprint!("login: cannot execute {}\n", shell);
            exit(1);
        }
        // Wait for the session to end, then let init start a fresh login.
        let _ = wait(pid as u64);
        exit(0)
    }
}
