//! Error type shared by the library and the CLI.

use thiserror::Error;

/// Errors produced by the driver.
#[derive(Debug, Error)]
pub enum Error {
    /// A HID transport operation failed.
    #[error("HID transport error: {0}")]
    Hid(#[from] hidapi::HidError),

    /// The receiver could not be opened.
    #[error("failed to open HID device {vid:04x}:{pid:04x}: {source}")]
    Open {
        vid: u16,
        pid: u16,
        #[source]
        source: hidapi::HidError,
    },

    /// An argument was rejected before any device access.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// The device sent data that violates the protocol.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// The device reported a non-zero OTA status byte.
    #[error("device reported status {0:#04x}")]
    DeviceStatus(u8),

    /// A bounded wait expired.
    #[error("timeout: {0}")]
    Timeout(String),

    /// A local I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, Error>;
