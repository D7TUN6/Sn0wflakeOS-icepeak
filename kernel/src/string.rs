//! Freestanding memory/string primitives needed by core and alloc.

use core::ffi::c_void;

#[inline]
unsafe fn copy_forward(d: *mut u8, s: *const u8, n: usize) {
    let mut i = 0;
    // Word-sized copies dominate: the framebuffer scroll moves megabytes.
    while i + 8 <= n {
        unsafe {
            core::ptr::write_unaligned(
                d.add(i) as *mut u64,
                core::ptr::read_unaligned(s.add(i) as *const u64),
            );
        }
        i += 8;
    }
    while i < n {
        unsafe { *d.add(i) = *s.add(i) };
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    unsafe { copy_forward(dest as *mut u8, src as *const u8, n) };
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dest as *mut u8;
    let s = src as *const u8;
    if (d as usize) < (s as usize) {
        unsafe { copy_forward(d, s, n) };
    } else {
        let mut i = n;
        while i >= 8 {
            i -= 8;
            unsafe {
                core::ptr::write_unaligned(
                    d.add(i) as *mut u64,
                    core::ptr::read_unaligned(s.add(i) as *const u64),
                );
            }
        }
        while i > 0 {
            i -= 1;
            unsafe { *d.add(i) = *s.add(i) };
        }
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut c_void, value: i32, n: usize) -> *mut c_void {
    let d = dest as *mut u8;
    let v = value as u8;
    let word = (v as u64) * 0x0101_0101_0101_0101;
    let mut i = 0;
    while i + 8 <= n {
        unsafe { core::ptr::write_unaligned(d.add(i) as *mut u64, word) };
        i += 8;
    }
    while i < n {
        unsafe { *d.add(i) = v };
        i += 1;
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let a = a as *const u8;
    let b = b as *const u8;
    let mut i = 0;
    while i < n {
        let x = unsafe { *a.add(i) };
        let y = unsafe { *b.add(i) };
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const u8) -> usize {
    let mut n = 0;
    while unsafe { *s.add(n) } != 0 {
        n += 1;
    }
    n
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

// `alloc::fmt::format` is still compiled with unwind landing pads even under
// `panic=abort`, so the link needs these two symbols.
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Resume() -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Backtrace() -> u32 {
    0
}
