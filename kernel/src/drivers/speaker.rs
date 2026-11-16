//! PC speaker driver using PIT channel 2.

use crate::arch::port::{inb, outb};

const PIT_HZ: u32 = 1_193_182;

/// The PIT is part of every PC-compatible chipset.
pub fn probe() -> bool {
    true
}

/// Silencing the speaker is the only setup the device needs.
pub fn init() {
    off();
}

pub fn on(freq: u32) {
    if freq < 20 {
        off();
        return;
    }
    let divisor = (PIT_HZ / freq).clamp(1, 0xFFFF) as u16;
    unsafe {
        // Channel 2, lobyte/hibyte, square wave.
        outb(0x43, 0xB6);
        outb(0x42, (divisor & 0xFF) as u8);
        outb(0x42, (divisor >> 8) as u8);
        // Gate the timer output to the speaker.
        outb(0x61, inb(0x61) | 0x03);
    }
}

pub fn off() {
    unsafe {
        outb(0x61, inb(0x61) & !0x03);
    }
}
