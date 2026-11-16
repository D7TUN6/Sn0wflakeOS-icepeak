#![no_std]
#![no_main]

use icepeak_user::*;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut human = false;
    let mut inodes = false;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" {
            print!("Usage: df [OPTION]...\n");
            print!("  -h, --human-readable  print sizes in powers of 1024\n");
            print!("  -k                    use 1024-byte blocks (default)\n");
            print!("  -i, --inodes          list inode information\n");
            print!("  -a, --all             include pseudo filesystems\n");
            print!("  -T, --print-type      print filesystem type\n");
            exit(0);
        } else if a.starts_with('-') && a.len() > 1 {
            for c in a[1..].chars() {
                match c {
                    'h' => human = true,
                    'k' | 'P' => {}
                    'i' => inodes = true,
                    'a' | 'T' => {}
                    _ => {
                        eprint!("df: invalid option: -{}\n", c);
                        exit(2);
                    }
                }
            }
        } else if a.starts_with("--") {
            match a {
                "--human-readable" => human = true,
                "--inodes" => inodes = true,
                "--all" | "--print-type" | "--portability" => {}
                _ => {
                    eprint!("df: unrecognized option: {}\n", a);
                    exit(2);
                }
            }
        }
        i += 1;
    }
    let mut fs_buf = [0u8; 256];
    let n = fsstat(&mut fs_buf);
    let fs_text = if n > 0 {
        core::str::from_utf8(&fs_buf[..n as usize]).unwrap_or("")
    } else {
        ""
    };
    let files = stat_field(fs_text, "files").unwrap_or(0);
    let dirs = stat_field(fs_text, "dirs").unwrap_or(0);
    let used = stat_field(fs_text, "bytes").unwrap_or(0);
    let mut mem_buf = [0u8; 256];
    let m = meminfo(&mut mem_buf);
    let mem_text = if m > 0 {
        core::str::from_utf8(&mem_buf[..m as usize]).unwrap_or("")
    } else {
        ""
    };
    let capacity = stat_field(mem_text, "total").unwrap_or(used).max(used);
    let avail = capacity.saturating_sub(used);
    if inodes {
        print!("Filesystem        Inodes    IUsed     IFree IUse% Mounted on\n");
        print!("ramfs        {:>10} {:>8} {:>9} {:>4}% /\n", files + dirs + 1, files + dirs, 0, 100);
        exit(0)
    }
    print!("Filesystem      Size  Used Avail Use% Mounted on\n");
    if human {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        let mut c = [0u8; 16];
        let size_s = human_size(capacity, &mut a);
        let used_s = human_size(used, &mut b);
        let avail_s = human_size(avail, &mut c);
        let pct = if capacity > 0 { used * 100 / capacity } else { 0 };
        print!("ramfs        {:>7} {:>5} {:>5} {:>3}% /\n", size_s, used_s, avail_s, pct);
    } else {
        let pct = if capacity > 0 { used * 100 / capacity } else { 0 };
        print!(
            "ramfs        {:>7} {:>5} {:>5} {:>3}% /\n",
            (capacity + 1023) / 1024,
            (used + 1023) / 1024,
            (avail + 1023) / 1024,
            pct
        );
    }
    exit(0)
}
