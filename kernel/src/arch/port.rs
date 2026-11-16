#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    unsafe { core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack)) };
}

#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe { core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack)) };
    value
}

#[inline]
pub unsafe fn outw(port: u16, value: u16) {
    unsafe { core::arch::asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack)) };
}

#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    unsafe { core::arch::asm!("in eax, dx", out("eax") value, in("dx") port, options(nomem, nostack)) };
    value
}

#[inline]
pub unsafe fn outl(port: u16, value: u32) {
    unsafe { core::arch::asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack)) };
}

#[inline]
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    unsafe { core::arch::asm!("in ax, dx", out("ax") value, in("dx") port, options(nomem, nostack)) };
    value
}

#[inline]
pub unsafe fn io_wait() {
    unsafe { outb(0x80, 0) };
}

/// Read `count` 16-bit words from `port` into `dst` (`rep insw`).
#[inline]
pub unsafe fn insw(port: u16, dst: *mut u16, count: usize) {
    unsafe {
        core::arch::asm!(
            "rep insw",
            in("dx") port,
            inout("di") dst => _,
            inout("cx") count => _,
            options(nostack)
        );
    }
}

/// Write `count` 16-bit words from `src` to `port` (`rep outsw`).
#[inline]
pub unsafe fn outsw(port: u16, src: *const u16, count: usize) {
    unsafe {
        core::arch::asm!(
            "rep outsw",
            in("dx") port,
            inout("si") src => _,
            inout("cx") count => _,
            options(nostack)
        );
    }
}
