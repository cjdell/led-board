#![no_std]
#![no_main]
#![feature(core_intrinsics)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[macro_use]
pub mod common;
pub mod animations;
pub mod draw;
pub mod fader;
pub mod playlist;
pub mod runner;
pub mod xp;

pub use crate::animations::*;
pub use crate::common::*;
pub use crate::fader::*;
pub use crate::playlist::*;
pub use crate::runner::*;
