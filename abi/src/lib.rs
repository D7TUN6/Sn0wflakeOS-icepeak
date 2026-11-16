#![no_std]

// icepeak syscall ABI. Shared by the kernel and user programs.

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_READ: u64 = 2;
pub const SYS_OPEN: u64 = 3;
pub const SYS_CLOSE: u64 = 4;
pub const SYS_LSEEK: u64 = 5;
pub const SYS_EXEC: u64 = 6;
pub const SYS_WAIT: u64 = 7;
pub const SYS_GETPID: u64 = 8;
pub const SYS_SLEEP: u64 = 9;
pub const SYS_YIELD: u64 = 10;
pub const SYS_CLEAR: u64 = 11;
pub const SYS_MKDIR: u64 = 12;
pub const SYS_UNLINK: u64 = 13;
pub const SYS_RENAME: u64 = 14;
pub const SYS_LIST: u64 = 15;
pub const SYS_CHDIR: u64 = 16;
pub const SYS_GETCWD: u64 = 17;
pub const SYS_UNAME: u64 = 18;
pub const SYS_PS: u64 = 19;
pub const SYS_TIME: u64 = 20;
pub const SYS_SOUND: u64 = 21;
pub const SYS_BEEP: u64 = 22;
pub const SYS_READ_FILE: u64 = 23;
pub const SYS_WRITE_FILE: u64 = 24;
pub const SYS_EXISTS: u64 = 25;
pub const SYS_SHUTDOWN: u64 = 26;
pub const SYS_RMDIR: u64 = 27;
pub const SYS_STAT: u64 = 29;
pub const SYS_CHMOD: u64 = 30;
pub const SYS_CHOWN: u64 = 31;
pub const SYS_GETUID: u64 = 32;
pub const SYS_GETGID: u64 = 33;
pub const SYS_SETUID: u64 = 34;
pub const SYS_MEMINFO: u64 = 35;
pub const SYS_TASKSTATS: u64 = 36;
pub const SYS_SCHED_SET: u64 = 37;
pub const SYS_SCHED_GET: u64 = 38;
pub const SYS_FSSTAT: u64 = 39;
pub const SYS_CPUFREQ_GET: u64 = 40;
pub const SYS_CPUFREQ_SET: u64 = 41;
pub const SYS_SYMLINK: u64 = 42;
pub const SYS_READLINK: u64 = 43;
pub const SYS_LSTAT: u64 = 44;
pub const SYS_DUP2: u64 = 45;
pub const SYS_PIPE: u64 = 46;
pub const SYS_SYNC: u64 = 47;

pub const O_RDONLY: u64 = 0;
pub const O_WRONLY: u64 = 1;
pub const O_RDWR: u64 = 2;
pub const O_ACCMODE: u64 = 3;
pub const O_CREAT: u64 = 0x40;
pub const O_TRUNC: u64 = 0x200;
pub const O_APPEND: u64 = 0x400;
pub const O_CLOEXEC: u64 = 0x8_0000;

pub const SEEK_SET: u64 = 0;
pub const SEEK_CUR: u64 = 1;
pub const SEEK_END: u64 = 2;

pub const STDIN: u64 = 0;
pub const STDOUT: u64 = 1;
pub const STDERR: u64 = 2;

pub const ENOENT: isize = -2;
pub const EPERM: isize = -1;
pub const EIO: isize = -5;
pub const EBADF: isize = -9;
pub const EACCES: isize = -13;
pub const ENOMEM: isize = -12;
pub const EFAULT: isize = -14;
pub const EINVAL: isize = -22;
pub const EEXIST: isize = -17;
pub const ENOTDIR: isize = -20;
pub const EISDIR: isize = -21;
pub const EMFILE: isize = -24;
pub const ESPIPE: isize = -29;
pub const EPIPE: isize = -32;

pub const KEY_CTRL_BACKSPACE: u8 = 0x17;
pub const KEY_CTRL_LEFT: u8 = 0x1A;
pub const KEY_CTRL_RIGHT: u8 = 0x1B;
