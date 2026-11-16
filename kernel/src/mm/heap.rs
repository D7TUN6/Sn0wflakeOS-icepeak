//! A small first-fit heap over a static arena, with block coalescing.

use core::alloc::{GlobalAlloc, Layout};

use crate::sync::SpinLock;

const HEAP_SIZE: usize = 6 * 1024 * 1024;
// Every block is 64-byte aligned so the allocator can honour the largest
// alignment the kernel asks for (FXSAVE areas need 16, caches prefer 64).
const ALIGN: usize = 64;

#[repr(C, align(64))]
struct Block {
    size: usize,
    free: bool,
    next: *mut Block,
    prev: *mut Block,
}

#[repr(C, align(64))]
struct Arena([u8; HEAP_SIZE]);

static mut ARENA: Arena = Arena([0; HEAP_SIZE]);
static LOCK: SpinLock = SpinLock::new();
static mut HEAD: *mut Block = core::ptr::null_mut();
static mut INITIALIZED: bool = false;

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

const HDR: usize = {
    let s = core::mem::size_of::<Block>();
    (s + ALIGN - 1) & !(ALIGN - 1)
};

pub fn init() {
    let _g = LOCK.lock();
    unsafe {
        let base = (&raw mut ARENA) as *mut u8 as usize;
        let block = base as *mut Block;
        (*block).size = HEAP_SIZE;
        (*block).free = true;
        (*block).next = core::ptr::null_mut();
        (*block).prev = core::ptr::null_mut();
        HEAD = block;
        INITIALIZED = true;
    }
}

unsafe fn alloc_locked(layout: Layout) -> *mut u8 {
    if !unsafe { INITIALIZED } {
        return core::ptr::null_mut();
    }
    let need = align_up(layout.size().max(1), ALIGN).max(layout.align());

    let mut cur = unsafe { HEAD };
    while !cur.is_null() {
        let block = unsafe { &mut *cur };
        if block.free && block.size >= HDR + need {
            let total_need = HDR + need;
            let remaining = block.size - total_need;
            if remaining >= HDR + ALIGN {
                let rest = unsafe { (cur as *mut u8).add(total_need) as *mut Block };
                unsafe {
                    (*rest).size = remaining;
                    (*rest).free = true;
                    (*rest).next = block.next;
                    (*rest).prev = cur;
                    if !block.next.is_null() {
                        (*block.next).prev = rest;
                    }
                    block.next = rest;
                }
                block.size = total_need;
            }
            block.free = false;
            return unsafe { (cur as *mut u8).add(HDR) };
        }
        cur = block.next;
    }
    core::ptr::null_mut()
}

unsafe fn dealloc_locked(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let block = unsafe { ptr.sub(HDR) as *mut Block };
    if block.is_null() {
        return;
    }
    unsafe {
        (*block).free = true;

        let next = (*block).next;
        if !next.is_null() && (*next).free {
            (*block).size += (*next).size;
            (*block).next = (*next).next;
            if !(*next).next.is_null() {
                (*(*next).next).prev = block;
            }
        }

        let prev = (*block).prev;
        if !prev.is_null() && (*prev).free {
            (*prev).size += (*block).size;
            (*prev).next = (*block).next;
            if !(*block).next.is_null() {
                (*(*block).next).prev = prev;
            }
        }
    }
}

pub struct IceHeap;

/// Walk the block list and report `(total, used, free)` arena bytes.
pub fn stats() -> (usize, usize, usize) {
    let _g = LOCK.lock();
    unsafe {
        if !INITIALIZED {
            return (HEAP_SIZE, 0, HEAP_SIZE);
        }
        let mut cur = HEAD;
        let mut used = 0usize;
        let mut free = 0usize;
        while !cur.is_null() {
            let block = &*cur;
            if block.free {
                free += block.size;
            } else {
                used += block.size;
            }
            cur = block.next;
        }
        (HEAP_SIZE, used, free)
    }
}

unsafe impl GlobalAlloc for IceHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _g = LOCK.lock();
        unsafe { alloc_locked(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let _g = LOCK.lock();
        unsafe { dealloc_locked(ptr) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _g = LOCK.lock();
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        let new_ptr = unsafe { alloc_locked(new_layout) };
        if !new_ptr.is_null() {
            let copy = layout.size().min(new_size);
            unsafe { core::ptr::copy_nonoverlapping(ptr, new_ptr, copy) };
            unsafe { dealloc_locked(ptr) };
        }
        new_ptr
    }
}
