//! ESP32-C3 slave-radio firmware for the LED board.
//!
//! Architecture (see also `radio_link` for the wire protocol):
//!
//! ```text
//!                        ┌──────────────────────── ESP32-C3 ────────────────────────┐
//!  WiFi LAN ════════════► STA stack :80 ──┐                                        │
//!                        │                 ├─► link (UART0 @921600) ═══════════════►│ Pico 2 W
//!  Provisioning AP ═════► AP stack :80 ───┘        │        ▲                       │ (firmware
//!  (fallback + portal)   │      ▲                  │        │                       │  --features
//!                        │      └── captive portal │        └── RPC: scan/join/… ───►│   radio)
//!                        │         + DHCP + DNS    │                                 │
//!                        │      mDNS led-board.local                                  │
//!                        └────────────────────────────────────────────────────────────┘
//! ```
//!
//! * `wifi` — supervisor: owns the WiFi controller, joins the best-known
//!   network (fallback: open AP + captive portal + built-in provisioning UI).
//! * `link` — frame codec plumbing: RPC server, stream slots, events, hello.
//! * `httpd` — AP portal (WiFi connect UI) + stream relays for both stacks.
//! * `store` — persisted networks in the `cfg` flash partition.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub mod httpd;
pub mod link;
pub mod mdns;
pub mod net;
pub mod portal;
pub mod state;
pub mod store;
pub mod supervisor;
