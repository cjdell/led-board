pub mod common;

// The picoserve web app itself is shared system logic (led-core); the
// tunnel below is the Pico's radio-specific transport for it.
pub use led_core::tasks::http;

#[cfg(feature = "radio")]
pub mod tunnel;

pub mod ppp;
#[cfg(feature = "radio")]
pub mod radio;
pub mod usb_dbg;
pub mod usb_ethernet;
pub mod wifi;

pub use common::LedControl;
pub use common::{WifiControl, dhcp_task, mdns_runner, mdns_task};
