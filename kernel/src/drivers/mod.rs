//! Device driver registry.
//!
//! Each driver exposes a `probe` that reports whether the hardware is present
//! and an `attach` that initialises it. `init_all` walks the table in order, so
//! device setup is one list instead of a sequence of calls in `main`.

pub mod ata;
pub mod keyboard;
pub mod serial;
pub mod speaker;

pub struct Driver {
    pub name: &'static str,
    pub probe: fn() -> bool,
    pub attach: fn(),
}

/// Order matters: the input devices are attached before anything that might
/// consume their events.
pub static DRIVERS: &[Driver] = &[
    Driver {
        name: "ata",
        probe: ata::probe,
        attach: ata::attach,
    },
    Driver {
        name: "serial",
        probe: serial::probe,
        attach: serial::init,
    },
    Driver {
        name: "keyboard",
        probe: keyboard::probe,
        attach: keyboard::init,
    },
    Driver {
        name: "speaker",
        probe: speaker::probe,
        attach: speaker::init,
    },
];

pub fn init_all() {
    for driver in DRIVERS {
        if (driver.probe)() {
            (driver.attach)();
            crate::println!("driver: {} attached", driver.name);
        }
    }
}
