//! Clean-room host driver for the Logitech G435 LIGHTSPEED USB receiver.
//!
//! The crate implements the read-only vendor HID control plane described by
//! `g435-host-protocol-v2`: battery, link status, link mode, RSSI, GRS status,
//! image version, inquiry scan, connection requests, and read-only storage
//! access.
//!
//! Destructive OTA operations (write, erase, reboot, handover, update) are
//! deliberately not implemented.

#![forbid(unsafe_code)]

pub mod cli;
mod device;
pub mod error;
pub mod protocol;
pub mod transport;

pub use device::{GrsStatus, ImageVersion, LinkStatus, Mode, Rssi, G435};
pub use error::{Error, Result};

/// Default vendor ID of the receiver.
pub const DEFAULT_VID: u16 = 0x046d;
/// Default product ID of the receiver (`HDT647`).
pub const DEFAULT_PID: u16 = 0x0acb;
