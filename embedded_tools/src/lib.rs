#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

pub mod config;
pub mod log;
pub mod local_fs;

#[cfg(not(feature = "std"))]
pub use log::{Debug2Format, debug, error, info, trace, warn};
#[cfg(feature = "std")]
pub use log::Debug2Format;

use embassy_futures::select::Either;
use embassy_time::{Duration, Timer};

pub async fn sleep(ms: u64) {
    Timer::after(Duration::from_millis(ms)).await;
}

pub fn either_into_result<T, E>(either: Either<Result<T, E>, Result<T, E>>) -> Result<T, E> {
    match either {
        Either::First(r) => r,
        Either::Second(r) => r,
    }
}
