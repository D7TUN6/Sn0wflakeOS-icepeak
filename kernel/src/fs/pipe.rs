//! Anonymous pipes.
//!
//! A pipe is a bounded byte stream shared by every task that holds one of its
//! ends. Reads block while the buffer is empty and a writer is still attached;
//! writes block while the buffer is full. Closing the last writer wakes readers
//! with end-of-file, and closing the last reader makes writers fail with
//! `EPIPE`.
//!
//! The table is global because the two ends normally live in different
//! processes. Descriptor tables reference pipes by index and call
//! `add_ref`/`close` when a descriptor is duplicated or closed.

use alloc::vec::Vec;

use abi::{EIO, EPIPE};

use crate::sync::SpinLock;

pub const PIPE_CAP: usize = 64 * 1024;
pub const MAX_PIPES: usize = 32;

struct Pipe {
    buf: Vec<u8>,
    /// Bytes before `head` have already been consumed.
    head: usize,
    readers: u32,
    writers: u32,
    read_waiters: Vec<u64>,
    write_waiters: Vec<u64>,
}

static LOCK: SpinLock = SpinLock::new();
static mut PIPES: [Option<Pipe>; MAX_PIPES] = [const { None }; MAX_PIPES];

fn slot(idx: usize) -> Option<&'static mut Pipe> {
    if idx >= MAX_PIPES {
        return None;
    }
    unsafe { (*(&raw mut PIPES))[idx].as_mut() }
}

impl Pipe {
    fn len(&self) -> usize {
        self.buf.len() - self.head
    }

    fn compact(&mut self) {
        if self.head > 0 {
            self.buf.drain(..self.head);
            self.head = 0;
        }
    }
}

/// Create a pipe. Both ends start with one reference.
pub fn create() -> Option<usize> {
    let _g = LOCK.lock();
    let pipes = unsafe { &mut *(&raw mut PIPES) };
    let idx = pipes.iter().position(|p| p.is_none())?;
    pipes[idx] = Some(Pipe {
        buf: Vec::new(),
        head: 0,
        readers: 1,
        writers: 1,
        read_waiters: Vec::new(),
        write_waiters: Vec::new(),
    });
    Some(idx)
}

pub fn add_ref(idx: usize, write: bool) {
    let _g = LOCK.lock();
    if let Some(pipe) = slot(idx) {
        if write {
            pipe.writers += 1;
        } else {
            pipe.readers += 1;
        }
    }
}

/// Drop one end and return the task ids that should be woken.
pub fn close(idx: usize, write: bool) -> Vec<u64> {
    let _g = LOCK.lock();
    let Some(pipe) = slot(idx) else {
        return Vec::new();
    };
    if write {
        pipe.writers = pipe.writers.saturating_sub(1);
    } else {
        pipe.readers = pipe.readers.saturating_sub(1);
    }
    let mut wake = Vec::new();
    if pipe.writers == 0 {
        wake.append(&mut pipe.read_waiters);
    }
    if pipe.readers == 0 {
        wake.append(&mut pipe.write_waiters);
    }
    if pipe.readers == 0 && pipe.writers == 0 {
        unsafe { (*(&raw mut PIPES))[idx] = None };
    }
    wake
}

fn readable(idx: usize) -> bool {
    let _g = LOCK.lock();
    match slot(idx) {
        Some(pipe) => pipe.len() > 0 || pipe.writers == 0,
        None => true,
    }
}

fn writable(idx: usize) -> bool {
    let _g = LOCK.lock();
    match slot(idx) {
        Some(pipe) => pipe.readers == 0 || pipe.len() < PIPE_CAP,
        None => true,
    }
}

/// Read up to `buf.len()` bytes, blocking until data or end-of-file.
pub fn read(idx: usize, buf: &mut [u8]) -> isize {
    if buf.is_empty() {
        return 0;
    }
    let pid = crate::sched::current_pid();
    loop {
        let mut wake: Vec<u64> = Vec::new();
        let mut got = 0usize;
        let mut block = false;
        {
            let _g = LOCK.lock();
            let Some(pipe) = slot(idx) else {
                return EIO;
            };
            if pipe.len() > 0 {
                let n = buf.len().min(pipe.len());
                buf[..n].copy_from_slice(&pipe.buf[pipe.head..pipe.head + n]);
                pipe.head += n;
                if pipe.head == pipe.buf.len() {
                    pipe.buf.clear();
                    pipe.head = 0;
                } else if pipe.head >= PIPE_CAP / 2 {
                    pipe.compact();
                }
                got = n;
                wake.append(&mut pipe.write_waiters);
            } else if pipe.writers == 0 {
                return 0;
            } else {
                if pid != 0 && !pipe.read_waiters.contains(&pid) {
                    pipe.read_waiters.push(pid);
                }
                block = true;
            }
        }
        for waiter in wake {
            crate::sched::wake_pid(waiter);
        }
        if got > 0 {
            return got as isize;
        }
        if block {
            crate::sched::sleep_until(crate::sched::WaitKind::Generic, || readable(idx));
        }
    }
}

/// Write all of `data`, blocking while the pipe is full.
pub fn write(idx: usize, data: &[u8]) -> isize {
    let pid = crate::sched::current_pid();
    let mut done = 0usize;
    while done < data.len() {
        let mut wake: Vec<u64> = Vec::new();
        let mut block = false;
        {
            let _g = LOCK.lock();
            let Some(pipe) = slot(idx) else {
                return EIO;
            };
            if pipe.readers == 0 {
                return if done > 0 {
                    done as isize
                } else {
                    EPIPE
                };
            }
            let space = PIPE_CAP - pipe.len();
            if space > 0 {
                let n = space.min(data.len() - done);
                if pipe.head > 0 && pipe.buf.len() + n > PIPE_CAP {
                    pipe.compact();
                }
                pipe.buf.extend_from_slice(&data[done..done + n]);
                done += n;
                wake.append(&mut pipe.read_waiters);
            }
            if done < data.len() && PIPE_CAP - pipe.len() == 0 {
                if pid != 0 && !pipe.write_waiters.contains(&pid) {
                    pipe.write_waiters.push(pid);
                }
                block = true;
            }
        }
        for waiter in wake {
            crate::sched::wake_pid(waiter);
        }
        if done == data.len() {
            return done as isize;
        }
        if block {
            crate::sched::sleep_until(crate::sched::WaitKind::Generic, || writable(idx));
        }
    }
    done as isize
}
