#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;

use alloc::string::String;

mod acpi;
mod arch;
mod console;
mod drivers;
mod font;
mod fs;
mod mm;
mod multiboot;
mod sched;
mod smp;
mod string;
mod sync;
mod syscall;
mod video;

#[unsafe(no_mangle)]
pub extern "C" fn kmain(magic: u32, info: u32) -> ! {
    console::init();
    match console::video_info() {
        Some(fb) => println!(
            "video: {}x{}x{} fb={:#x} pitch={} masks r{}/g{}/b{} selftest={}",
            fb.width,
            fb.height,
            fb.bpp,
            fb.addr,
            fb.pitch,
            fb.red_size,
            fb.green_size,
            fb.blue_size,
            video::selftest(&fb)
        ),
        None => println!("video: VGA text 80x25"),
    }
    println!("icepeak: booted in 64-bit long mode");
    println!("multiboot magic={:#010x} info={:#010x}", magic, info);

    multiboot::init(magic, info);
    acpi::init();

    let mut entries = 0usize;
    let mut usable = 0u64;
    multiboot::for_each_mmap(|e| {
        entries += 1;
        if e.kind == 1 {
            usable += e.length;
        }
    });
    println!(
        "memory: {} mmap entries, {} MiB usable, {} modules",
        entries,
        usable / (1024 * 1024),
        multiboot::module_count()
    );

    mm::phys::init();
    mm::reserve_kernel();
    mm::init();
    // The APIC base and timer calibration both need the ACPI tables, and the
    // timer needs the local APIC, so both run before the generic arch setup.
    arch::apic::init();
    arch::init();
    arch::power::init();
    arch::cpufreq::init();
    let (used, total) = mm::stats();
    println!(
        "frames: {}/{} used ({} MiB), heap ready",
        used,
        total,
        used * 4096 / (1024 * 1024)
    );

    {
        use alloc::vec::Vec;
        let mut v: Vec<u32> = Vec::new();
        for i in 0..64 {
            v.push(i);
        }
        println!("alloc test: len={} sum={}", v.len(), v.iter().sum::<u32>());
    }

    fs::init();
    // Keep the scheduler's initial idle frame stable until we actually enter
    // it; otherwise a timer IRQ would save a kernel frame over it.
    arch::disable();
    sched::init();

    match sched::spawn_with_args("/bin/init", 0, &["/bin/init"], String::from("/")) {
        Some(pid) => println!("spawned /bin/init as pid {}", pid),
        None => println!("failed to spawn /bin/init"),
    }

    smp::bringup();

    println!("icepeak is alive");
    sched::start()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n*** KERNEL PANIC ***");
    println!("{}", info);
    arch::halt();
}

unsafe extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

pub fn kernel_start() -> usize {
    unsafe { &__kernel_start as *const u8 as usize }
}

pub fn kernel_end() -> usize {
    unsafe { &__kernel_end as *const u8 as usize }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::console::_print(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => { $crate::print!("{}\n", format_args!($($arg)*)) };
}
