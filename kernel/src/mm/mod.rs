pub mod heap;
pub mod paging;
pub mod phys;

use crate::mm::heap::IceHeap;

#[global_allocator]
pub static ALLOCATOR: IceHeap = IceHeap;

pub fn init() {
    heap::init();
}

pub fn reserve_kernel() {
    let start = crate::kernel_start() as u64;
    let end = crate::kernel_end() as u64;
    phys::reserve_range(start, end);

    crate::multiboot::for_each_module(|m| {
        phys::reserve_range(m.start, m.end);
    });

    // Multiboot info and the memory-map list live in low memory, already
    // covered by the 1 MiB reserve, but be explicit for loaders that place
    // them high.
    if crate::multiboot::valid() {
        let info = crate::multiboot::info_address() as u64;
        phys::reserve_range(info, info + 0x1000);
        let mm = crate::multiboot::mmap_addr() as u64;
        let len = crate::multiboot::mmap_length() as u64;
        if mm != 0 {
            phys::reserve_range(mm, mm + len + 0x1000);
        }
    }
}

pub fn stats() -> (usize, usize) {
    (phys::used_frames(), phys::ram_frames())
}

/// `(heap_total, heap_used, heap_free, frame_used, frame_total)`.
pub fn mem_stats() -> (usize, usize, usize, usize, usize) {
    let (total, used, free) = heap::stats();
    (total, used, free, phys::used_frames(), phys::ram_frames())
}
