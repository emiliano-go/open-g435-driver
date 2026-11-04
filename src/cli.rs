//! Command-line surface, kept in the library so parsing can be unit tested.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::{Parser, Subcommand};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{GrsStatus, ImageVersion, Mode, Rssi};

/// `g435ctl` command line.
#[derive(Debug, Parser)]
#[command(
    name = "g435ctl",
    version,
    about = "Control the Logitech G435 LIGHTSPEED receiver"
)]
pub struct Cli {
    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    /// Command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Supported commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Read the battery voltage.
    Battery,

    /// Read the link status bitmask.
    Status,

    /// Read or set the link mode.
    Mode {
        /// Set the mode instead of reading it.
        #[arg(long, value_name = "local|remote", value_parser = parse_mode)]
        set: Option<Mode>,
    },

    /// Read the four RSSI values.
    Rssi,

    /// Enable or disable Bluetooth inquiry scan.
    Scan {
        /// Disable inquiry scan.
        #[arg(long, conflicts_with = "duration")]
        off: bool,

        /// Enable scan, wait this many seconds, then disable it.
        #[arg(long, value_name = "SECONDS")]
        duration: Option<u64>,
    },

    /// Request a connection to a Bluetooth address.
    Pair {
        /// Six colon-separated octets.
        #[arg(value_parser = parse_mac)]
        mac: [u8; 6],
    },

    /// Show image version, GRS status, and battery.
    Info,

    /// Read storage and print the bytes and their SHA-256.
    Read {
        /// Start address, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_number)]
        addr: u32,

        /// Byte count, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_number)]
        len: u32,

        /// Write the raw bytes to this path instead of printing them.
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
}

/// Parse a decimal or `0x`-prefixed hexadecimal `u32`.
pub fn parse_number(value: &str) -> std::result::Result<u32, String> {
    let text = value.trim();
    let (digits, radix) = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(digits) => (digits, 16),
        None => (text, 10),
    };
    if digits.is_empty() {
        return Err(format!("invalid number {value:?}"));
    }
    u32::from_str_radix(digits, radix).map_err(|error| format!("invalid number {value:?}: {error}"))
}

/// Parse a six-octet, colon-separated MAC address.
pub fn parse_mac(value: &str) -> std::result::Result<[u8; 6], String> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 6 {
        return Err(format!(
            "invalid MAC address {value:?}: expected six colon-separated octets"
        ));
    }

    let mut mac = [0u8; 6];
    for (slot, part) in mac.iter_mut().zip(parts) {
        if part.is_empty() || part.len() > 2 {
            return Err(format!(
                "invalid MAC address {value:?}: octets must be one or two hex digits"
            ));
        }
        *slot = u8::from_str_radix(part, 16)
            .map_err(|error| format!("invalid MAC address {value:?}: {error}"))?;
    }
    Ok(mac)
}

/// Render a MAC address in lowercase colon-separated form.
pub fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|octet| format!("{octet:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn parse_mode(value: &str) -> std::result::Result<Mode, String> {
    Mode::from_str(value)
}

/// Lowercase SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Human or JSON output for `battery`.
pub fn format_battery(millivolts: u16, json_output: bool) -> String {
    if json_output {
        json!({ "millivolts": millivolts }).to_string()
    } else {
        format!("{millivolts} mV")
    }
}

/// Human or JSON output for `status`.
pub fn format_status(bits: u8, json_output: bool) -> String {
    if json_output {
        json!({ "bits": bits }).to_string()
    } else {
        format!("link status: 0x{bits:02X} (bits 0b{bits:08b})")
    }
}

/// Human or JSON output for `mode`.
pub fn format_mode(mode: Mode, json_output: bool) -> String {
    if json_output {
        json!({ "mode": mode.to_string() }).to_string()
    } else {
        mode.to_string()
    }
}

/// Human or JSON output for `mode --set`.
pub fn format_mode_set(mode: Mode, json_output: bool) -> String {
    if json_output {
        json!({ "set": mode.to_string() }).to_string()
    } else {
        format!("mode set to {mode}")
    }
}

/// Human or JSON output for `rssi`.
pub fn format_rssi(rssi: &Rssi, json_output: bool) -> String {
    if json_output {
        json!({
            "left_ap": rssi.left_ap,
            "right_ap": rssi.right_ap,
            "left_ptp": rssi.left_ptp,
            "right_ptp": rssi.right_ptp,
        })
        .to_string()
    } else {
        format!(
            "AP L/R: {} / {} dBm, PTP L/R: {} / {} dBm",
            rssi.left_ap, rssi.right_ap, rssi.left_ptp, rssi.right_ptp
        )
    }
}

/// Human or JSON output for `scan` and `scan --off`/`--duration`.
pub fn format_scan(enabled: bool, json_output: bool) -> String {
    if json_output {
        json!({ "inquiry_scan": enabled }).to_string()
    } else if enabled {
        "inquiry scan enabled (use --off to disable)".to_string()
    } else {
        "inquiry scan disabled".to_string()
    }
}

/// Human or JSON output for `pair`.
pub fn format_pair(mac: &[u8; 6], json_output: bool) -> String {
    let mac = format_mac(mac);
    if json_output {
        json!({ "paired": mac }).to_string()
    } else {
        format!("connection requested for {mac}")
    }
}

/// Human or JSON output for `info`.
pub fn format_info(
    image: &ImageVersion,
    grs: &GrsStatus,
    battery: u16,
    json_output: bool,
) -> String {
    if json_output {
        json!({
            "image_version": image.hex(),
            "grs_status": grs.bits,
            "battery": battery,
        })
        .to_string()
    } else {
        format!(
            "image version: {}\ngrs status: 0x{:02X} (bits 0b{:08b})\nbattery: {battery} mV",
            image.hex(),
            grs.bits,
            grs.bits
        )
    }
}

/// Human or JSON output for `read` without `--output`.
pub fn format_read(addr: u32, len: u32, data: &[u8], json_output: bool) -> String {
    let digest = sha256_hex(data);
    if json_output {
        json!({
            "addr": addr,
            "len": len,
            "sha256": digest,
            "data": hex::encode(data),
        })
        .to_string()
    } else {
        format!("{}\nsha256: {digest}", hex::encode(data))
    }
}

/// Human output for `read --output`.
pub fn format_read_written(path: &Path, data: &[u8]) -> String {
    format!(
        "wrote {} bytes to {} sha256 {}",
        data.len(),
        path.display(),
        sha256_hex(data)
    )
}
