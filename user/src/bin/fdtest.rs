#![no_std]
#![no_main]

//! Exercises the kernel file-descriptor table through the Rust wrappers:
//! streaming reads and writes, seeking, append mode, `dup2`, and the
//! `/dev/null` and `/dev/zero` sinks.

use icepeak_user::*;

const PATH: &str = "/tmp/fdtest.txt";

fn fail(what: &str) -> ! {
    eprint!("fdtest: {} failed\n", what);
    exit(1)
}

fn expect(ok: bool, what: &str) {
    if !ok {
        fail(what);
    }
}

fn read_exact(fd: u64, want: &[u8], what: &str) {
    let mut buf = [0u8; 64];
    let n = read(fd, &mut buf[..want.len()]);
    expect(n == want.len() as isize, what);
    expect(&buf[..want.len()] == want, what);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(_argc: usize, _argv: *const *const u8) -> ! {
    let _ = unlink(PATH);

    let fd = open(PATH, O_WRONLY | O_CREAT | O_TRUNC);
    expect(fd >= 0, "create");
    expect(write(fd as u64, b"abc") == 3, "write");
    expect(close(fd as u64) == 0, "close");

    let fd = open(PATH, O_RDONLY);
    expect(fd >= 0, "open");
    read_exact(fd as u64, b"ab", "read/first");
    read_exact(fd as u64, b"c", "read/second");
    let mut byte = [0u8; 1];
    expect(read(fd as u64, &mut byte) == 0, "read/eof");
    expect(close(fd as u64) == 0, "close/read");

    let fd = open(PATH, O_WRONLY | O_APPEND);
    expect(fd >= 0, "open/append");
    expect(write(fd as u64, b"de") == 2, "write/append");
    expect(close(fd as u64) == 0, "close/append");

    let fd = open(PATH, O_RDONLY);
    expect(fd >= 0, "reopen");
    expect(lseek(fd as u64, 0, SEEK_END) == 5, "seek/end");
    expect(lseek(fd as u64, 1, SEEK_SET) == 1, "seek/set");
    read_exact(fd as u64, b"bcde", "read/after-seek");
    expect(close(fd as u64) == 0, "close/seek");

    // `dup2` shares the descriptor entry, so both numbers advance one cursor.
    let fd = open(PATH, O_WRONLY | O_APPEND);
    expect(fd >= 0, "open/dup");
    let copy = dup2(fd as u64, 9);
    expect(copy == 9, "dup2");
    expect(write(9, b"!") == 1, "write/dup2");
    expect(close(9) == 0, "close/dup2");
    expect(close(fd as u64) == 0, "close/dup");

    let fd = open(PATH, O_RDONLY);
    expect(fd >= 0, "final");
    let mut all = [0u8; 16];
    let n = read(fd as u64, &mut all);
    expect(n == 6 && &all[..6] == b"abcde!", "final/content");
    expect(close(fd as u64) == 0, "final/close");

    let zero = open("/dev/zero", O_RDONLY);
    expect(zero >= 0, "open/zero");
    let mut zeros = [0xffu8; 64];
    expect(read(zero as u64, &mut zeros) == 64, "read/zero");
    expect(zeros.iter().all(|&b| b == 0), "read/zero/bytes");
    expect(close(zero as u64) == 0, "close/zero");

    let sink = open("/dev/null", O_WRONLY);
    expect(sink >= 0, "open/null");
    expect(write(sink as u64, b"discard") == 7, "write/null");
    expect(close(sink as u64) == 0, "close/null");

    expect(open("/definitely/not/here", O_RDONLY) < 0, "open/missing");
    expect(close(100) < 0, "close/badfd");

    // Pipes: byte stream, EOF after the writer closes, EPIPE after the reader.
    let (r, w) = pipe().unwrap_or_else(|| fail("pipe"));
    expect(write(w, b"hello") == 5, "pipe/write");
    let mut buf = [0u8; 16];
    let n = read(r, &mut buf);
    expect(n == 5 && &buf[..5] == b"hello", "pipe/read");
    expect(close(w) == 0, "pipe/close-w");
    expect(read(r, &mut buf) == 0, "pipe/eof");
    expect(close(r) == 0, "pipe/close-r");

    let (r, w) = pipe().unwrap_or_else(|| fail("pipe2"));
    expect(close(r) == 0, "pipe2/close-r");
    expect(write(w, b"x") == EPIPE, "pipe/epipe");
    expect(close(w) == 0, "pipe2/close-w");

    // A larger message crosses the buffer in one write and several reads.
    let (r, w) = pipe().unwrap_or_else(|| fail("pipe3"));
    let mut chunk = [0u8; 1024];
    for i in 0..chunk.len() {
        chunk[i] = i as u8;
    }
    expect(write(w, &chunk) == 1024, "pipe/big-write");
    expect(close(w) == 0, "pipe3/close-w");
    let mut got = [0u8; 1024];
    let mut total = 0usize;
    loop {
        let n = read(r, &mut got[total..]);
        if n <= 0 {
            break;
        }
        total += n as usize;
    }
    expect(total == 1024 && got == chunk, "pipe/big-read");
    expect(close(r) == 0, "pipe3/close-r");

    let _ = unlink(PATH);
    print!("fdtest: ok\n");
    exit(0)
}
