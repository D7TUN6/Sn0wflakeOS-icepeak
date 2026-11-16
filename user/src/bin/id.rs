#![no_std]
#![no_main]

use icepeak_user::*;

fn user_name(uid: u32) -> &'static str {
    match uid {
        0 => "root",
        1000 => "user",
        1001 => "guest",
        65534 => "nobody",
        _ => "unknown",
    }
}

fn group_name(gid: u32) -> &'static str {
    match gid {
        0 => "root",
        1000 => "users",
        1001 => "guest",
        65534 => "nogroup",
        _ => "unknown",
    }
}

fn print_id(uid: u32, gid: u32, names: bool) {
    if names {
        print!("{}({}) gid={}({}) groups={}({})\n", uid, user_name(uid), gid, group_name(gid), gid, group_name(gid));
    } else {
        print!("uid={} gid={} groups={}\n", uid, gid, gid);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let args = unsafe { Args::new(argc, argv) };
    let mut only_u = false;
    let mut only_g = false;
    let mut names = false;
    let mut i = 1usize;
    while let Some(a) = args.get(i) {
        if a == "--help" || a == "-h" {
            print!("Usage: id [-u] [-g] [-G] [-n]\nPrint user and group identity.\n");
            exit(0);
        }
        if a == "-u" {
            only_u = true;
        } else if a == "-g" {
            only_g = true;
        } else if a == "-G" {
            only_g = true;
        } else if a == "-n" {
            names = true;
        } else {
            eprint!("id: invalid option: {}\n", a);
            exit(2);
        }
        i += 1;
    }
    let uid = getuid();
    let gid = getgid();
    if only_u {
        if names {
            print!("{}\n", user_name(uid));
        } else {
            print!("{}\n", uid);
        }
    } else if only_g {
        if names {
            print!("{}\n", group_name(gid));
        } else {
            print!("{}\n", gid);
        }
    } else {
        print_id(uid, gid, names);
    }
    exit(0)
}
