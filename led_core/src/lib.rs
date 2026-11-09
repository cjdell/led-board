//! Core system logic for the LED art board.
//!
//! This crate holds the whole application — the picoserve web app, the
//! WebSocket control pump, the animation display worker, config, and the
//! dot-matrix screen model — with every hardware touch-point behind a
//! trait:
//!
//! * [`board::Board`] — reboot / watchdog / status LED / WiFi provisioning.
//! * [`board::OtaBackend`] — where an `/api/ota` upload gets written.
//! * [`embedded_tools::local_fs::LocalFsTrait`] — the file store (littlefs on
//!   flash for the Pico, a plain directory for the simulator).
//! * [`led_screen::Ws2812ParallelDriver`] — how a frame reaches the LEDs
//!   (PIO + DMA on the Pico, a window on the Mac).
//!
//! The async plumbing (embassy-executor, embassy-time, embassy-sync) is part
//! of the core, not the platform: it runs identically on the RP2350 and on
//! the host, so task code is shared verbatim. `firmware` (Pico) and `sim`
//! (macOS) are the two implementations.

#![no_std]
#![feature(impl_trait_in_assoc_type)]
#![recursion_limit = "256"]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod tasks;

pub mod board;
pub mod config;
pub mod led_screen;
pub mod types;

pub use embedded_tools::{Debug2Format, debug, error, info, trace, warn};

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
