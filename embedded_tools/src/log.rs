//! Logging facade: defmt on the microcontroller, `println!` on the host.
//!
//! `defmt`'s macros cannot link without a firmware logger implementation, so
//! shared system code (led-core, this crate) logs through these re-exports
//! instead of `defmt::` directly. The embedded build re-exports the real
//! defmt macros; the host build prints the same calls to stdout.
//!
//! Use as `use embedded_tools::{info, warn, error, debug, Debug2Format};`.

#[cfg(not(feature = "std"))]
pub use defmt::{Debug2Format, debug, error, info, trace, warn};

#[cfg(all(not(feature = "std"), not(feature = "defmt")))]
compile_error!("embedded-tools: enable exactly one of the `defmt` or `std` features");

#[cfg(feature = "std")]
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => { ::std::println!("[trace] {}", ::std::format_args!($($arg)*)) };
}

#[cfg(feature = "std")]
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => { ::std::println!("[debug] {}", ::std::format_args!($($arg)*)) };
}

#[cfg(feature = "std")]
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { ::std::println!("[info ] {}", ::std::format_args!($($arg)*)) };
}

#[cfg(feature = "std")]
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { ::std::println!("[warn ] {}", ::std::format_args!($($arg)*)) };
}

#[cfg(feature = "std")]
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { ::std::eprintln!("[error] {}", ::std::format_args!($($arg)*)) };
}

/// Host stand-in for [`defmt::Debug2Format`]: wraps a `Debug` value so it can
/// be logged with either `{}` or `{:?}` in the facade macros above.
#[cfg(feature = "std")]
pub struct Debug2Format<'a, T: core::fmt::Debug + ?Sized>(pub &'a T);

#[cfg(feature = "std")]
impl<T: core::fmt::Debug + ?Sized> core::fmt::Debug for Debug2Format<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "std")]
impl<T: core::fmt::Debug + ?Sized> core::fmt::Display for Debug2Format<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}


