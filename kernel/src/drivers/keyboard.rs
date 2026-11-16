use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::arch::port::inb;

pub const QUEUE_SIZE: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCode {
    None,
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    F(u8),
    CapsLock,
    NumLock,
    ScrollLock,
}

#[derive(Clone, Copy)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl KeyEvent {
    pub const NONE: Self = Self {
        code: KeyCode::None,
        shift: false,
        ctrl: false,
        alt: false,
    };
}

struct Queue(UnsafeCell<[KeyEvent; QUEUE_SIZE]>);

unsafe impl Sync for Queue {}

static QUEUE: Queue = Queue(UnsafeCell::new([KeyEvent::NONE; QUEUE_SIZE]));
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

/// PS/2 IRQs, UART IRQs, syscalls and console writes on other CPUs all enqueue
/// events, so the ring is guarded rather than relying on a single producer.
static QUEUE_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();

static mut SHIFT: bool = false;
static mut CTRL: bool = false;
static mut ALT: bool = false;
static mut CAPS: bool = false;
static mut NUMLOCK: bool = true;
static mut EXTENDED: bool = false;
static mut E0_1D: bool = false;
static mut E0_38: bool = false;

/// The i8042 status port reads as all ones when no controller is wired up.
pub fn probe() -> bool {
    unsafe { inb(0x64) != 0xFF }
}

pub fn init() {
    // Drain the controller output buffer.
    for _ in 0..32 {
        if unsafe { inb(0x64) } & 1 == 0 {
            break;
        }
        unsafe { inb(0x60) };
    }
}

pub(crate) fn push_event(event: KeyEvent) {
    let _g = QUEUE_LOCK.lock();
    let head = HEAD.load(Ordering::Relaxed);
    let tail = TAIL.load(Ordering::Acquire);
    let next = (head + 1) % QUEUE_SIZE;
    if next == tail {
        return;
    }
    unsafe {
        let queue = &mut *QUEUE.0.get();
        queue[head] = event;
    }
    HEAD.store(next, Ordering::Release);
}

pub fn try_pop() -> Option<KeyEvent> {
    let _g = QUEUE_LOCK.lock();
    let tail = TAIL.load(Ordering::Relaxed);
    let head = HEAD.load(Ordering::Acquire);
    if tail == head {
        return None;
    }
    let event = unsafe {
        let queue = &*QUEUE.0.get();
        queue[tail]
    };
    TAIL.store((tail + 1) % QUEUE_SIZE, Ordering::Release);
    Some(event)
}

pub fn is_empty() -> bool {
    let _g = QUEUE_LOCK.lock();
    unsafe { HEAD.load(Ordering::Acquire) == TAIL.load(Ordering::Acquire) && PENDING_HEAD == PENDING_TAIL }
}

pub fn pop_blocking() -> KeyEvent {
    loop {
        if let Some(event) = try_pop() {
            return event;
        }
        super::serial::poll();
        unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
    }
}

pub fn on_irq() {
    let status = unsafe { inb(0x64) };
    if status & 1 == 0 {
        return;
    }
    let scancode = unsafe { inb(0x60) };
    handle_scancode(scancode);
}

fn handle_scancode(scancode: u8) {
    unsafe {
        if scancode == 0xE0 {
            EXTENDED = true;
            return;
        }
    }

    let released = scancode & 0x80 != 0;
    let code = scancode & 0x7F;
    let extended = unsafe { EXTENDED };
    unsafe { EXTENDED = false };

    match code {
        0x2A | 0x36 => {
            if !extended {
                unsafe { SHIFT = !released };
            }
            return;
        }
        0x1D => {
            unsafe {
                if extended {
                    E0_1D = !released;
                } else {
                    CTRL = !released;
                }
            }
            return;
        }
        0x38 => {
            unsafe {
                if extended {
                    E0_38 = !released;
                } else {
                    ALT = !released;
                }
            }
            return;
        }
        0x3A if !extended => {
            if !released {
                unsafe { CAPS = !CAPS };
            }
            return;
        }
        0x45 if !extended => {
            if !released {
                unsafe { NUMLOCK = !NUMLOCK };
            }
            return;
        }
        0x46 if !extended => {
            return;
        }
        _ => {}
    }

    if released {
        return;
    }

    let key = if extended {
        extended_key(code)
    } else {
        normal_key(code)
    };

    if let Some(code) = key {
        push_event(KeyEvent {
            code,
            shift: unsafe { SHIFT },
            ctrl: unsafe { CTRL || E0_1D },
            alt: unsafe { ALT || E0_38 },
        });
    }
}

fn normal_key(code: u8) -> Option<KeyCode> {
    const QWERTY: [char; 10] = ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'];
    const HOME_ROW: [char; 9] = ['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'];
    const BOTTOM: [char; 7] = ['z', 'x', 'c', 'v', 'b', 'n', 'm'];
    const NUMBER: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
    const SHIFT_NUMBER: [char; 10] = [')', '!', '@', '#', '$', '%', '^', '&', '*', '('];
    const PUNCT: [(char, char); 11] = [
        ('-', '_'),
        ('=', '+'),
        ('[', '{'),
        (']', '}'),
        (';', ':'),
        ('\'', '"'),
        ('`', '~'),
        ('\\', '|'),
        (',', '<'),
        ('.', '>'),
        ('/', '?'),
    ];

    let shift = unsafe { SHIFT };
    let caps = unsafe { CAPS };
    let num = unsafe { NUMLOCK };

    let ch = match code {
        0x01 => return Some(KeyCode::Escape),
        0x0E => return Some(KeyCode::Backspace),
        0x0F => return Some(KeyCode::Tab),
        0x1C => return Some(KeyCode::Enter),
        0x39 => ' ',
        0x02..=0x0B => {
            let idx = (code - 0x02) as usize;
            if shift {
                SHIFT_NUMBER[idx]
            } else {
                NUMBER[idx]
            }
        }
        0x0C => {
            if shift {
                PUNCT[0].1
            } else {
                PUNCT[0].0
            }
        }
        0x0D => {
            if shift {
                PUNCT[1].1
            } else {
                PUNCT[1].0
            }
        }
        0x1A => {
            if shift {
                PUNCT[2].1
            } else {
                PUNCT[2].0
            }
        }
        0x1B => {
            if shift {
                PUNCT[3].1
            } else {
                PUNCT[3].0
            }
        }
        0x27 => {
            if shift {
                PUNCT[4].1
            } else {
                PUNCT[4].0
            }
        }
        0x28 => {
            if shift {
                PUNCT[5].1
            } else {
                PUNCT[5].0
            }
        }
        0x29 => {
            if shift {
                PUNCT[6].1
            } else {
                PUNCT[6].0
            }
        }
        0x2B => {
            if shift {
                PUNCT[7].1
            } else {
                PUNCT[7].0
            }
        }
        0x33 => {
            if shift {
                PUNCT[8].1
            } else {
                PUNCT[8].0
            }
        }
        0x34 => {
            if shift {
                PUNCT[9].1
            } else {
                PUNCT[9].0
            }
        }
        0x35 => {
            if shift {
                PUNCT[10].1
            } else {
                PUNCT[10].0
            }
        }
        0x10..=0x19 => {
            let mut c = QWERTY[(code - 0x10) as usize];
            if shift ^ caps {
                c = c.to_ascii_uppercase();
            }
            c
        }
        0x1E..=0x26 => {
            let mut c = HOME_ROW[(code - 0x1E) as usize];
            if shift ^ caps {
                c = c.to_ascii_uppercase();
            }
            c
        }
        0x2C..=0x32 => {
            let mut c = BOTTOM[(code - 0x2C) as usize];
            if shift ^ caps {
                c = c.to_ascii_uppercase();
            }
            c
        }
        0x47 => {
            if num {
                '7'
            } else {
                return Some(KeyCode::Home);
            }
        }
        0x48 => {
            if num {
                '8'
            } else {
                return Some(KeyCode::Up);
            }
        }
        0x49 => {
            if num {
                '9'
            } else {
                return Some(KeyCode::PageUp);
            }
        }
        0x4A => '-',
        0x4B => {
            if num {
                '4'
            } else {
                return Some(KeyCode::Left);
            }
        }
        0x4C => {
            if num {
                '5'
            } else {
                return Some(KeyCode::None);
            }
        }
        0x4D => {
            if num {
                '6'
            } else {
                return Some(KeyCode::Right);
            }
        }
        0x4E => '+',
        0x4F => {
            if num {
                '1'
            } else {
                return Some(KeyCode::End);
            }
        }
        0x50 => {
            if num {
                '2'
            } else {
                return Some(KeyCode::Down);
            }
        }
        0x51 => {
            if num {
                '3'
            } else {
                return Some(KeyCode::PageDown);
            }
        }
        0x52 => {
            if num {
                '0'
            } else {
                return Some(KeyCode::Insert);
            }
        }
        0x53 => {
            if num {
                '.'
            } else {
                return Some(KeyCode::Delete);
            }
        }
        0x3B..=0x44 => return Some(KeyCode::F(code - 0x3A)),
        0x57 => return Some(KeyCode::F(11)),
        0x58 => return Some(KeyCode::F(12)),
        _ => return None,
    };
    Some(KeyCode::Char(ch))
}

fn extended_key(code: u8) -> Option<KeyCode> {
    match code {
        0x1C => Some(KeyCode::Enter),
        0x35 => Some(KeyCode::Char('/')),
        0x47 => Some(KeyCode::Home),
        0x48 => Some(KeyCode::Up),
        0x49 => Some(KeyCode::PageUp),
        0x4B => Some(KeyCode::Left),
        0x4D => Some(KeyCode::Right),
        0x4F => Some(KeyCode::End),
        0x50 => Some(KeyCode::Down),
        0x51 => Some(KeyCode::PageDown),
        0x52 => Some(KeyCode::Insert),
        0x53 => Some(KeyCode::Delete),
        _ => None,
    }
}

// Byte-level input for the read(2) syscall. Escape sequences use the usual
// ANSI forms so a user-space line editor can decode arrows, home and end.
const ESC: u8 = 0x1B;
const PENDING_SIZE: usize = 32;
static mut PENDING: [u8; PENDING_SIZE] = [0; PENDING_SIZE];
static mut PENDING_HEAD: usize = 0;
static mut PENDING_TAIL: usize = 0;

fn pending_push(bytes: &[u8]) {
    unsafe {
        for &b in bytes {
            let head = PENDING_HEAD;
            let next = (head + 1) % PENDING_SIZE;
            if next == PENDING_TAIL {
                break;
            }
            PENDING[head] = b;
            PENDING_HEAD = next;
        }
    }
}

fn pending_pop(buf: &mut [u8]) -> usize {
    let mut n = 0;
    unsafe {
        while n < buf.len() && PENDING_TAIL != PENDING_HEAD {
            buf[n] = PENDING[PENDING_TAIL];
            PENDING_TAIL = (PENDING_TAIL + 1) % PENDING_SIZE;
            n += 1;
        }
    }
    n
}

fn encode(event: KeyEvent) -> [u8; 8] {
    let mut out = [0u8; 8];
    use KeyCode::*;

    if event.ctrl {
        match event.code {
            Char(c) => {
                let b = c.to_ascii_lowercase() as u8;
                if b.is_ascii_lowercase() {
                    out[0] = b - b'a' + 1;
                    return out;
                }
            }
            Backspace => {
                out[0] = abi::KEY_CTRL_BACKSPACE;
                return out;
            }
            Left => {
                out[0] = abi::KEY_CTRL_LEFT;
                return out;
            }
            Right => {
                out[0] = abi::KEY_CTRL_RIGHT;
                return out;
            }
            _ => {}
        }
    }

    if event.alt {
        out[0] = ESC;
        out[1] = match event.code {
            Char(c) => c as u8,
            _ => 0,
        };
        return out;
    }

    match event.code {
        Char(c) => out[0] = c as u8,
        Enter => out[0] = b'\n',
        Backspace => out[0] = 0x7F,
        Tab => out[0] = b'\t',
        Escape => out[0] = ESC,
        Delete => {
            out[..4].copy_from_slice(b"\x1b[3~");
        }
        Insert => {
            out[..4].copy_from_slice(b"\x1b[2~");
        }
        Home => {
            out[..3].copy_from_slice(b"\x1b[H");
        }
        End => {
            out[..3].copy_from_slice(b"\x1b[F");
        }
        PageUp => {
            out[..4].copy_from_slice(b"\x1b[5~");
        }
        PageDown => {
            out[..4].copy_from_slice(b"\x1b[6~");
        }
        Up => {
            out[..3].copy_from_slice(b"\x1b[A");
        }
        Down => {
            out[..3].copy_from_slice(b"\x1b[B");
        }
        Left => {
            out[..3].copy_from_slice(b"\x1b[D");
        }
        Right => {
            out[..3].copy_from_slice(b"\x1b[C");
        }
        F(n) => {
            let seq = match n {
                1 => b"\x1bOP".as_slice(),
                2 => b"\x1bOQ",
                3 => b"\x1bOR",
                4 => b"\x1bOS",
                5 => b"\x1b[15~",
                6 => b"\x1b[17~",
                7 => b"\x1b[18~",
                8 => b"\x1b[19~",
                9 => b"\x1b[20~",
                10 => b"\x1b[21~",
                11 => b"\x1b[23~",
                _ => b"\x1b[24~",
            };
            out[..seq.len()].copy_from_slice(seq);
        }
        CapsLock | NumLock | ScrollLock | None => {}
    }
    out
}

fn encoded_len(bytes: &[u8; 8]) -> usize {
    if bytes[0] == ESC {
        if bytes[1] == 0 {
            return 1;
        }
        if bytes[1] == b'[' || bytes[1] == b'O' {
            if bytes[2] == b'3' || bytes[2] == b'2' || bytes[2] == b'5' || bytes[2] == b'6' {
                if bytes[3] == b'~' {
                    return 4;
                }
            }
            if bytes[1] == b'[' && bytes[2].is_ascii_digit() {
                for i in 2..8 {
                    if bytes[i] == b'~' {
                        return i + 1;
                    }
                }
            }
            return 3;
        }
        return 2;
    }
    1
}

/// Non-blocking read of encoded key bytes. Returns 0 when no input is queued.
pub fn read_bytes(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    let n = pending_pop(buf);
    if n > 0 {
        return n;
    }
    super::serial::poll();
    let Some(event) = try_pop() else {
        return 0;
    };
    let encoded = encode(event);
    let len = encoded_len(&encoded);
    pending_push(&encoded[..len]);
    pending_pop(buf)
}

