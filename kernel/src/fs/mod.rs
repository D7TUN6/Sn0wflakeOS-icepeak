pub mod fd;
pub mod journal;
pub mod pipe;
pub mod ramfs;

use alloc::format;
use alloc::string::ToString;

/// The single store closure that the live image ships. FHS paths are symlinks
/// into it, the same shape NixOS presents.
const STORE_PATH: &str = "/nix/store/snf-coreutils-0.1";
const STORE_BIN: &str = "/nix/store/snf-coreutils-0.1/sw/bin";

const PASSWD: &str = "\
root:x:0:0:root:/root:/bin/peaksh
user::1000:1000:user:/home/user:/bin/peaksh
guest::1001:1001:guest:/home/guest:/bin/peaksh
nobody:x:65534:65534:nobody:/:/bin/false
";

const SHADOW: &str = "\
root:root:0:0:99999:7:::
user::0:0:99999:7:::
guest::0:0:99999:7:::
nobody:!:0:0:99999:7:::
";

const GROUP: &str = "\
root:x:0:root
user:x:1000:user
guest:x:1001:guest
nogroup:x:65534:
";

const OS_RELEASE: &str = "\
NAME=\"Sn0wflakeOS\"
ID=sn0wflake
PRETTY_NAME=\"Sn0wflakeOS 0.1.0 (icepeak)\"
VERSION_ID=\"0.1.0\"
HOME_URL=\"https://sn0wflake.os/\"
";

const HOSTNAME: &str = "sn0wflake\n";
const MOTD: &str = "Welcome to Sn0wflakeOS.\nType 'help' for shell commands.\n";

fn file(path: &str, text: &str) {
    ramfs::write(path, text.as_bytes());
}

/// Populate the initial filesystem from the Multiboot modules loaded by the
/// bootloader, then layer the Unix view (`/etc`, `/bin`, `/usr`) over the
/// immutable store.
pub fn init() {
    ramfs::init();
    crate::multiboot::for_each_module(|module| {
        let len = module.end.saturating_sub(module.start) as usize;
        if len == 0 {
            return;
        }
        let bytes = unsafe { core::slice::from_raw_parts(module.start as *const u8, len) };
        let name = module.name.trim_start_matches('/');
        // Executables live in the store; anything else keeps its boot path.
        let path = match name.strip_prefix("bin/") {
            Some(rest) => format!("{}/{}", STORE_BIN, rest),
            None => name.to_string(),
        };
        ramfs::add_file(&path, bytes.to_vec());
        crate::println!("ramfs: {} ({} bytes)", name, len);
    });

    // Standard directories that are not part of the store.
    for dir in ["/run", "/usr", "/etc", "/home", "/home/user", "/home/guest", "/root", "/tmp", "/var", "/var/log", "/proc", "/dev"] {
        ramfs::mkdir(dir);
    }

    // Pseudo-filesystems: contents are generated on access.
    ramfs::add_dynamic("/proc/meminfo", ramfs::Dynamic::Meminfo, 0o444);
    ramfs::add_dynamic("/proc/cpuinfo", ramfs::Dynamic::Cpuinfo, 0o444);
    ramfs::add_dynamic("/proc/uptime", ramfs::Dynamic::Uptime, 0o444);
    ramfs::add_dynamic("/proc/version", ramfs::Dynamic::Version, 0o444);
    ramfs::add_dynamic("/dev/null", ramfs::Dynamic::Null, 0o666);
    ramfs::add_dynamic("/dev/zero", ramfs::Dynamic::Zero, 0o666);

    // Nix-style closure plus the FHS compatibility view.
    ramfs::add_link("/run/current-system", STORE_PATH);
    ramfs::add_link("/bin", "/run/current-system/sw/bin");
    ramfs::add_link("/usr/bin", "/bin");
    ramfs::add_link("/usr/sbin", "/bin");
    ramfs::add_link("/sbin", "/bin");
    ramfs::add_link("/usr/lib", "/run/current-system/sw/lib");

    file("/etc/passwd", PASSWD);
    file("/etc/shadow", SHADOW);
    file("/etc/group", GROUP);
    file("/etc/os-release", OS_RELEASE);
    file("/etc/hostname", HOSTNAME);
    file("/etc/motd", MOTD);
    file("/etc/shells", "/bin/peaksh\n");
    ramfs::chmod("/etc/shadow", 0o640);
    // Home directories and the shared temporary directory.
    ramfs::chown("/home/user", 1000, 1000);
    ramfs::chown("/home/guest", 1001, 1001);
    ramfs::chmod("/tmp", 0o1777);
    ramfs::chmod("/home/user", 0o755);
    ramfs::chmod("/home/guest", 0o755);

    // Replay the on-disk log over the freshly seeded tree, then rewrite it in
    // compacted form. Without a disk the image simply stays memory-only.
    if !journal::mount() {
        crate::println!("persist: no disk, filesystem is memory-only");
    }
}
