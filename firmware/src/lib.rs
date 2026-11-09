#![no_std]
#![no_main]
#![recursion_limit = "256"]

extern crate alloc;

pub mod board;
pub mod tasks;
pub mod types;
pub mod ws2812;
pub mod ws2812p;

// The system logic (config, screen model, web app) lives in led-core and is
// re-exported here so the whole app reads as one crate from the bin.
pub use led_core::{config, led_screen};

/// Replacement for [`static_cell::make_static`](https://docs.rs/static_cell/latest/static_cell/macro.make_static.html) for use cases when the type is known.
#[macro_export]
macro_rules! make_static {
    ($t:ty, $val:expr) => ($crate::make_static!($t, $val,));
    ($t:ty, $val:expr, $(#[$m:meta])*) => {{
        $(#[$m])*
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}
