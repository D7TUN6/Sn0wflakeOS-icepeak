//! Per-process file descriptor tables.
//!
//! Descriptors 0..=2 start out as console endpoints. A `File` descriptor is an
//! index into the task's open-file table, so `dup2` makes two descriptors share
//! one cursor, exactly like a POSIX open file description. A `Pipe` descriptor
//! names an end of a global pipe. A child inherits a copy of the table across
//! `exec`, which also takes a reference on each inherited pipe end.
//!
//! A descriptor can be marked close-on-exec. Such entries are left out of the
//! copy a child receives, which is what lets a shell hold both ends of a pipe
//! while handing one end to a stage without every later stage inheriting it.
//! `dup2` clears the mark on the new descriptor, as POSIX requires.
//!
//! The open-file table keeps closed entries around as reusable slots; `refs`
//! counts the descriptors that point at each one.

use alloc::string::String;
use alloc::vec::Vec;

use abi::O_CLOEXEC;

use super::pipe;

pub const MAX_FDS: usize = 16;

#[derive(Clone)]
pub struct OpenFile {
    pub path: String,
    pub offset: usize,
    pub flags: u64,
    refs: u32,
}

#[derive(Clone)]
pub enum Fd {
    Console {
        readable: bool,
        writable: bool,
    },
    File(usize),
    Pipe {
        idx: usize,
        write: bool,
    },
}

#[derive(Clone)]
struct Entry {
    kind: Fd,
    cloexec: bool,
}

/// A read-only view of one descriptor, used by the syscall layer.
#[derive(Clone)]
pub enum Desc {
    Console {
        readable: bool,
        writable: bool,
    },
    File {
        path: String,
        offset: usize,
        flags: u64,
    },
    Pipe {
        idx: usize,
        write: bool,
    },
}

pub struct Table {
    slots: [Option<Entry>; MAX_FDS],
    files: Vec<OpenFile>,
}

impl Clone for Table {
    fn clone(&self) -> Table {
        let mut slots = [const { None }; MAX_FDS];
        for (dst, slot) in slots.iter_mut().zip(self.slots.iter()) {
            if let Some(entry) = slot {
                if entry.cloexec {
                    continue;
                }
                if let Fd::Pipe { idx, write } = entry.kind {
                    pipe::add_ref(idx, write);
                }
                *dst = Some(entry.clone());
            }
        }
        Table {
            slots,
            files: self.files.clone(),
        }
    }
}

impl Table {
    pub fn new() -> Table {
        let mut slots = [const { None }; MAX_FDS];
        slots[0] = Some(Entry {
            kind: Fd::Console {
                readable: true,
                writable: false,
            },
            cloexec: false,
        });
        slots[1] = Some(Entry {
            kind: Fd::Console {
                readable: false,
                writable: true,
            },
            cloexec: false,
        });
        slots[2] = Some(Entry {
            kind: Fd::Console {
                readable: false,
                writable: true,
            },
            cloexec: false,
        });
        Table {
            slots,
            files: Vec::new(),
        }
    }

    pub fn get(&self, fd: usize) -> Option<&Fd> {
        self.slots.get(fd)?.as_ref().map(|e| &e.kind)
    }

    pub fn snapshot(&self, fd: usize) -> Option<Desc> {
        match self.get(fd)? {
            Fd::Console {
                readable,
                writable,
            } => Some(Desc::Console {
                readable: *readable,
                writable: *writable,
            }),
            Fd::File(idx) => {
                let file = self.files.get(*idx)?;
                Some(Desc::File {
                    path: file.path.clone(),
                    offset: file.offset,
                    flags: file.flags,
                })
            }
            Fd::Pipe { idx, write } => Some(Desc::Pipe {
                idx: *idx,
                write: *write,
            }),
        }
    }

    /// Install a new open file at the lowest free descriptor.
    pub fn install_file(&mut self, path: String, flags: u64) -> Option<usize> {
        let fd = self.slots.iter().position(|s| s.is_none())?;
        let idx = match self.files.iter().position(|f| f.refs == 0) {
            Some(idx) => {
                self.files[idx] = OpenFile {
                    path,
                    offset: 0,
                    flags,
                    refs: 1,
                };
                idx
            }
            None => {
                self.files.push(OpenFile {
                    path,
                    offset: 0,
                    flags,
                    refs: 1,
                });
                self.files.len() - 1
            }
        };
        self.slots[fd] = Some(Entry {
            kind: Fd::File(idx),
            cloexec: flags & O_CLOEXEC != 0,
        });
        Some(fd)
    }

    /// Install one end of a pipe, taking over the reference `pipe::create`
    /// handed out for it.
    pub fn install_pipe(&mut self, idx: usize, write: bool, cloexec: bool) -> Option<usize> {
        let fd = self.slots.iter().position(|s| s.is_none())?;
        self.slots[fd] = Some(Entry {
            kind: Fd::Pipe { idx, write },
            cloexec,
        });
        Some(fd)
    }

    /// Mark an existing descriptor close-on-exec.
    pub fn set_cloexec(&mut self, fd: usize, on: bool) -> bool {
        match self.slots.get_mut(fd) {
            Some(Some(entry)) => {
                entry.cloexec = on;
                true
            }
            _ => false,
        }
    }

    /// `dup2`: both descriptors then share one cursor or pipe end, and the
    /// target loses its close-on-exec mark.
    pub fn dup2(&mut self, old: usize, new: usize) -> Option<Vec<u64>> {
        if new >= MAX_FDS {
            return None;
        }
        let entry = self.slots[old].clone()?;
        if old == new {
            return Some(Vec::new());
        }
        let mut wake = Vec::new();
        if let Some(previous) = self.slots[new].take() {
            wake = self.release(previous);
        }
        if let Fd::Pipe { idx, write } = entry.kind {
            pipe::add_ref(idx, write);
        }
        self.slots[new] = Some(Entry {
            kind: entry.kind,
            cloexec: false,
        });
        Some(wake)
    }

    fn release(&mut self, entry: Entry) -> Vec<u64> {
        match entry.kind {
            Fd::File(idx) => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.refs = file.refs.saturating_sub(1);
                }
                Vec::new()
            }
            Fd::Pipe { idx, write } => pipe::close(idx, write),
            Fd::Console { .. } => Vec::new(),
        }
    }

    pub fn close(&mut self, fd: usize) -> Option<Vec<u64>> {
        let entry = match self.slots.get_mut(fd) {
            Some(slot) => slot.take()?,
            None => return None,
        };
        Some(self.release(entry))
    }

    /// Release every descriptor, used when a task exits.
    pub fn release_all(&mut self) -> Vec<u64> {
        let mut entries = Vec::new();
        for slot in self.slots.iter_mut() {
            if let Some(entry) = slot.take() {
                entries.push(entry);
            }
        }
        let mut wake = Vec::new();
        for entry in entries {
            wake.append(&mut self.release(entry));
        }
        wake
    }

    pub fn set_offset(&mut self, fd: usize, offset: usize) -> bool {
        let idx = match self.get(fd) {
            Some(Fd::File(idx)) => *idx,
            _ => return false,
        };
        match self.files.get_mut(idx) {
            Some(file) => {
                file.offset = offset;
                true
            }
            None => false,
        }
    }
}
