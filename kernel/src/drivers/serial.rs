//! 16550 UART receive path.
//!
//! Transmit lives in `console.rs` because it shares the ANSI formatter with
//! VGA. This module owns the receive side: it turns raw bytes, including ANSI
//! escape sequences, into the same `KeyEvent` stream the PS/2 keyboard
//! produces so the console has a single input queue.

use crate::arch::port::{inb, outb};
use crate::drivers::keyboard::{self, KeyCode, KeyEvent};

const COM1: u16 = 0x3F8;
const SEQ_CAP: usize = 16;

static mut SEQ: [u8; SEQ_CAP] = [0; SEQ_CAP];
static mut SEQ_LEN: usize = 0;

/// Set by `init`. The console write path drains the FIFO too, including during
/// early boot before the driver registry has run.
static READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

static POLL_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();

/// A UART is only present if its scratch register retains a written value.
pub fn probe() -> bool {
    unsafe {
        outb(COM1 + 7, 0xAE);
        let ok = inb(COM1 + 7) == 0xAE;
        outb(COM1 + 7, 0x00);
        ok
    }
}

pub fn init() {
    // Receive-data-available interrupt. The line configuration was already
    // done by the console during early boot.
    unsafe { outb(COM1 + 1, 0x01) };
    READY.store(true, core::sync::atomic::Ordering::Release);
}

/// Drain the receive FIFO. Called from the IRQ 4 handler, from blocking input
/// waits and from the console write path so a missed interrupt cannot wedge
/// the console. Several CPUs can reach this at once, and the escape-sequence
/// assembler is stateful, so the whole drain is serialised.
pub fn poll() {
    if !READY.load(core::sync::atomic::Ordering::Acquire) {
        return;
    }
    let _g = POLL_LOCK.lock();
    let mut budget = 128;
    while budget > 0 {
        budget -= 1;
        unsafe {
            if inb(COM1 + 5) & 1 == 0 {
                break;
            }
            let b = inb(COM1);
            byte(b);
        }
    }
    // A lone ESC is the Escape key, not the start of a sequence.
    unsafe {
        if SEQ_LEN == 1 {
            SEQ_LEN = 0;
            keyboard::push_event(KeyEvent {
                code: KeyCode::Escape,
                shift: false,
                ctrl: false,
                alt: false,
            });
        }
    }
}

fn event(code: KeyCode, shift: bool, ctrl: bool, alt: bool) {
    keyboard::push_event(KeyEvent {
        code,
        shift,
        ctrl,
        alt,
    });
}

fn byte(b: u8) {
    unsafe {
        if SEQ_LEN > 0 {
            if SEQ_LEN < SEQ_CAP {
                SEQ[SEQ_LEN] = b;
                SEQ_LEN += 1;
            }
            let len = SEQ_LEN;
            let seq = core::slice::from_raw_parts((&raw const SEQ) as *const u8, len);
            if seq[0] != 0x1B {
                SEQ_LEN = 0;
                return;
            }
            if len == 2 && seq[1] != b'[' && seq[1] != b'O' {
                SEQ_LEN = 0;
                if b.is_ascii_graphic() {
                    event(KeyCode::Char(b as char), false, false, true);
                }
                return;
            }
            if len >= 2 && (seq[1] == b'[' || seq[1] == b'O') {
                let last = seq[len - 1];
                if len >= 3 && (0x40..=0x7E).contains(&last) {
                    SEQ_LEN = 0;
                    if let Some(ev) = decode_csi(&seq[1..]) {
                        keyboard::push_event(ev);
                    }
                } else if len >= SEQ_CAP {
                    SEQ_LEN = 0;
                }
                return;
            }
            return;
        }

        match b {
            0x1B => {
                SEQ[0] = 0x1B;
                SEQ_LEN = 1;
            }
            b'\r' | b'\n' => event(KeyCode::Enter, false, false, false),
            0x7F | 0x08 => event(KeyCode::Backspace, false, false, false),
            0x09 => event(KeyCode::Tab, false, false, false),
            0x01..=0x07 | 0x0B | 0x0C | 0x0E..=0x1A => {
                let c = (b - 1 + b'a') as char;
                event(KeyCode::Char(c), false, true, false);
            }
            b if b.is_ascii_graphic() || b == b' ' => {
                event(KeyCode::Char(b as char), false, false, false)
            }
            _ => {}
        }
    }
}

/// Decode the tail of a CSI/SS3 sequence (everything after ESC).
fn decode_csi(seq: &[u8]) -> Option<KeyEvent> {
    if seq.len() < 2 {
        return None;
    }
    let mut params = [0u16; 4];
    let mut nparams = 0usize;
    let mut cur = 0u16;
    let mut have = false;
    let mut i = 1usize;
    while i < seq.len() {
        let b = seq[i];
        match b {
            b'0'..=b'9' => {
                cur = cur.saturating_mul(10).saturating_add((b - b'0') as u16);
                have = true;
                i += 1;
            }
            b';' => {
                if nparams < params.len() {
                    params[nparams] = cur;
                    nparams += 1;
                }
                cur = 0;
                have = false;
                i += 1;
            }
            0x40..=0x7E => {
                if have && nparams < params.len() {
                    params[nparams] = cur;
                    nparams += 1;
                }
                let modifier = if nparams >= 2 { params[1] } else { 0 };
                let shift = modifier & 1 != 0;
                let alt = modifier & 2 != 0;
                let ctrl = modifier & 4 != 0;
                let p1 = params[0];
                let code = match b {
                    b'A' => KeyCode::Up,
                    b'B' => KeyCode::Down,
                    b'C' => KeyCode::Right,
                    b'D' => KeyCode::Left,
                    b'H' => KeyCode::Home,
                    b'F' => KeyCode::End,
                    b'Z' => KeyCode::Tab,
                    b'P' => KeyCode::F(1),
                    b'Q' => KeyCode::F(2),
                    b'R' => KeyCode::F(3),
                    b'S' => KeyCode::F(4),
                    b'~' => match p1 {
                        1 | 7 => KeyCode::Home,
                        2 => KeyCode::Insert,
                        3 => KeyCode::Delete,
                        4 | 8 => KeyCode::End,
                        5 => KeyCode::PageUp,
                        6 => KeyCode::PageDown,
                        11 => KeyCode::F(1),
                        12 => KeyCode::F(2),
                        13 => KeyCode::F(3),
                        14 => KeyCode::F(4),
                        15 => KeyCode::F(5),
                        17 => KeyCode::F(6),
                        18 => KeyCode::F(7),
                        19 => KeyCode::F(8),
                        20 => KeyCode::F(9),
                        21 => KeyCode::F(10),
                        23 => KeyCode::F(11),
                        24 => KeyCode::F(12),
                        _ => return None,
                    },
                    _ => return None,
                };
                return Some(KeyEvent {
                    code,
                    shift,
                    ctrl,
                    alt,
                });
            }
            _ => return None,
        }
    }
    None
}
