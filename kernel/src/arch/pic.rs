use super::idt::InterruptFrame;
use crate::arch::port::{inb, outb};

pub const PIC1_OFFSET: u8 = 32;
pub const PIC2_OFFSET: u8 = 40;

pub fn remap() {
    unsafe {
        let a1 = inb(0x21);
        let a2 = inb(0xA1);

        outb(0x20, 0x11);
        outb(0xA0, 0x11);
        outb(0x21, PIC1_OFFSET);
        outb(0xA1, PIC2_OFFSET);
        outb(0x21, 0x04);
        outb(0xA1, 0x02);
        outb(0x21, 0x01);
        outb(0xA1, 0x01);
        outb(0x21, a1);
        outb(0xA1, a2);
    }
}

pub fn mask_all() {
    unsafe {
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
    }
}

pub fn unmask(irq: u8) {
    unsafe {
        let port = if irq < 8 { 0x21 } else { 0xA1 };
        let bit = irq % 8;
        let value = inb(port) & !(1 << bit);
        outb(port, value);
    }
}

pub fn mask(irq: u8) {
    unsafe {
        let port = if irq < 8 { 0x21 } else { 0xA1 };
        let bit = irq % 8;
        outb(port, inb(port) | (1 << bit));
    }
}

pub fn eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(0xA0, 0x20);
        }
        outb(0x20, 0x20);
    }
}

pub fn is_spurious(vector: u64) -> bool {
    matches!(vector, 39 | 47)
}

pub fn dispatch_spurious(vector: u64) {
    if vector == 39 {
        unsafe { outb(0x20, 0x20) };
    } else if vector == 47 {
        unsafe { outb(0xA0, 0x20); outb(0x20, 0x20) };
    }
}

pub fn on_frame(frame: &InterruptFrame) -> bool {
    let v = frame.vector as u8;
    if (PIC1_OFFSET..PIC1_OFFSET + 16).contains(&v) {
        let irq = v - PIC1_OFFSET;
        if is_spurious(frame.vector) {
            dispatch_spurious(frame.vector);
        } else {
            match irq {
                0 => crate::arch::timer::tick(),
                1 => {
                    crate::drivers::keyboard::on_irq();
                    crate::sched::wake_console();
                }
                4 => {
                    crate::drivers::serial::poll();
                    crate::sched::wake_console();
                }
                _ => {}
            }
            eoi(irq);
        }
        true
    } else {
        false
    }
}
