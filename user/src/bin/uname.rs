#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut show_sysname = false;
    let mut show_nodename = false;
    let mut show_release = false;
    let mut show_version = false;
    let mut show_machine = false;
    let mut show_os = false;
    let mut all = false;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: uname [OPTION]...\n");
            print!("  -a, --all           print all information\n");
            print!("  -s, --kernel-name   print the kernel name\n");
            print!("  -n, --nodename      print the network node hostname\n");
            print!("  -r, --kernel-release print the kernel release\n");
            print!("  -v, --kernel-version print the kernel version\n");
            print!("  -m, --machine       print the machine hardware name\n");
            print!("  -o, --operating-system print the operating system\n");
            exit(0);
        }
        if a.starts_with("--") {
            match a {
                "--all" => all = true,
                "--kernel-name" => show_sysname = true,
                "--nodename" => show_nodename = true,
                "--kernel-release" => show_release = true,
                "--kernel-version" => show_version = true,
                "--machine" => show_machine = true,
                "--operating-system" => show_os = true,
                _ => {
                    eprint!("uname: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        } else if a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'a' => all = true,
                    's' => show_sysname = true,
                    'n' => show_nodename = true,
                    'r' => show_release = true,
                    'v' => show_version = true,
                    'm' => show_machine = true,
                    'o' => show_os = true,
                    _ => {
                        eprint!("uname: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        }
        i += 1;
    }
    if !all
        && !show_sysname
        && !show_nodename
        && !show_release
        && !show_version
        && !show_machine
        && !show_os
    {
        show_sysname = true;
    }
    if all {
        show_sysname = true;
        show_nodename = true;
        show_release = true;
        show_version = true;
        show_machine = true;
        show_os = true;
    }
    let mut buf = [0u8; 128];
    let n = uname(&mut buf);
    if n <= 0 {
        eprint!("uname: cannot read system information\n");
        exit(1);
    }
    let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    let mut fields = text.split_whitespace();
    let sysname = fields.next().unwrap_or("Sn0wflakeOS");
    let nodename = fields.next().unwrap_or("icepeak");
    let release = fields.next().unwrap_or("0.1.0");
    let version = fields.next().unwrap_or("#1");
    let machine = fields.next().unwrap_or("x86_64");
    let mut printed = false;
    let mut out = |value: &str| {
        if printed {
            print!(" ");
        }
        print!("{}", value);
        printed = true;
    };
    if show_sysname {
        out(sysname);
    }
    if show_nodename {
        out(nodename);
    }
    if show_release {
        out(release);
    }
    if show_version {
        out(version);
    }
    if show_machine {
        out(machine);
    }
    if show_os {
        out("Sn0wflakeOS");
    }
    print!("\n");
    exit(0)
}
