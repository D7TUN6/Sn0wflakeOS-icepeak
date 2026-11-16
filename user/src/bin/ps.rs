#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut full = false;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        match a {
            "--help" | "-h" => {
                print!("Usage: ps [OPTION]\n");
                print!("  -e, -A, -a  select all processes (default)\n");
                print!("  -f          full listing with uid, ticks and scheduling\n");
                print!("  -l          long listing\n");
                exit(0);
            }
            "-f" | "-l" | "--full" => full = true,
            "-e" | "-A" | "-a" | "--all" | "--everyone" => {}
            _ if a.starts_with('-') && a.len() > 1 => {
                eprint!("ps: invalid option: {}\n", a);
                exit(2);
            }
            _ => {}
        }
        i += 1;
    }
    let buf = aux_list();
    if full {
        let n = taskstats(buf);
        if n < 0 {
            eprint!("ps: cannot read task statistics\n");
            exit(1);
        }
        print!("  PID NAME             STATE   UID  TICKS POLICY PRI      MEM\n");
        let _ = write(1, &buf[..n as usize]);
    } else {
        let n = ps(buf);
        if n < 0 {
            eprint!("ps: cannot read process table\n");
            exit(1);
        }
        print!("  PID NAME                 STATE\n");
        let _ = write(1, &buf[..n as usize]);
    }
    exit(0)
}
