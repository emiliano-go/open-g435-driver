//! Command-line surface, kept in the library so parsing can be unit tested.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::{Args, Parser, Subcommand};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{AfhMap, Devm, GrsStatus, ImageVersion, Lpc, Mode, Quality, Rssi};

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

    /// Read the filesystem table.
    FsTable,

    /// Read one filesystem index entry.
    FsIndex {
        /// Index, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        index: u8,
    },

    /// Read customer info by ID.
    CustInfo {
        /// ID, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        id: u8,
    },

    /// Read MP data by ID.
    MpRead {
        /// ID, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        id: u8,
    },

    /// Read NVDS data by ID.
    NvdsRead {
        /// ID, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        id: u8,
    },

    /// Read an ATB GPIO value.
    Atb {
        /// GPIO number, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        gpio: u8,
    },

    /// Echo bytes through the OTA echo command.
    Echo {
        /// Hex bytes, optional 0x prefix.
        #[arg(value_parser = parse_hex_bytes)]
        data: HexBytes,
    },

    /// Read LPC status.
    Lpc,

    /// Read link quality.
    Quality,

    /// Read DEVM status.
    Devm,

    /// Read the AFH map.
    AfhMap,

    /// Send a raw OTA command (unsafe).
    RawOta {
        /// OTA command ID, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        command_id: u8,

        /// Optional hex argument bytes.
        #[arg(value_parser = parse_hex_bytes)]
        args: Option<HexBytes>,

        #[command(flatten)]
        unsafe_opts: UnsafeOpts,
    },

    /// Send a raw link command (unsafe).
    RawLink {
        /// Link command ID, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        command_id: u8,

        /// Optional hex argument bytes.
        #[arg(value_parser = parse_hex_bytes)]
        args: Option<HexBytes>,

        #[command(flatten)]
        unsafe_opts: UnsafeOpts,
    },

    /// Send a raw PA request (unsafe).
    RawPa {
        /// PA category, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u8)]
        category: u8,

        /// PA payload bytes as hex.
        #[arg(value_parser = parse_hex_bytes)]
        payload: HexBytes,

        #[command(flatten)]
        unsafe_opts: UnsafeOpts,
    },

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

/// Explicit unsafe confirmation options.
#[derive(Debug, Args)]
pub struct UnsafeOpts {
    /// Acknowledge that raw commands can be destructive.
    #[arg(long = "unsafe", required = true)]
    pub unsafe_flag: bool,

    /// Exact confirmation token.
    #[arg(
        long,
        required = true,
        value_name = "I-UNDERSTAND",
        value_parser = parse_confirm
    )]
    pub confirm: String,
}

/// Parse a decimal or `0x`-prefixed hexadecimal `u8`.
pub fn parse_u8(value: &str) -> std::result::Result<u8, String> {
    let number = parse_number(value)?;
    u8::try_from(number).map_err(|_| format!("value {value:?} does not fit in a byte"))
}

/// Hex bytes parsed from one command-line argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexBytes(pub Vec<u8>);

/// Parse hex bytes, with an optional `0x` prefix.
pub fn parse_hex_bytes(value: &str) -> std::result::Result<HexBytes, String> {
    let text = value.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    hex::decode(digits)
        .map(HexBytes)
        .map_err(|error| format!("invalid hex {value:?}: {error}"))
}

/// Require the exact unsafe confirmation token.
pub fn parse_confirm(value: &str) -> std::result::Result<String, String> {
    if value == "I-UNDERSTAND" {
        Ok(value.to_string())
    } else {
        Err(format!(
            "confirmation token must be exactly I-UNDERSTAND, got {value:?}"
        ))
    }
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

/// Human or JSON output for `fs-table`.
pub fn format_fs_table(data: &[u8], json_output: bool) -> String {
    if json_output {
        json!({ "len": data.len(), "data": hex::encode(data) }).to_string()
    } else {
        format!(
            "filesystem table ({} bytes)\n{}",
            data.len(),
            hex::encode(data)
        )
    }
}

/// Human or JSON output for `fs-index`.
pub fn format_fs_index(offset: u32, size: u32, data: &[u8], json_output: bool) -> String {
    if json_output {
        json!({
            "offset": offset,
            "size": size,
            "data": hex::encode(data),
        })
        .to_string()
    } else {
        format!(
            "offset: 0x{offset:08X} size: 0x{size:08X}\ndata: {}",
            hex::encode(data)
        )
    }
}

/// Human or JSON output for a raw data blob (`cust-info`, `mp-read`, `nvds-read`).
pub fn format_blob(label: &str, id: u8, data: &[u8], json_output: bool) -> String {
    if json_output {
        json!({ "id": id, "len": data.len(), "data": hex::encode(data) }).to_string()
    } else {
        format!("{label}[{id}]: {} bytes\n{}", data.len(), hex::encode(data))
    }
}

/// Human or JSON output for `atb`.
pub fn format_atb(gpio: u8, value: u32, json_output: bool) -> String {
    let data = hex::encode(value.to_le_bytes());
    if json_output {
        json!({ "gpio": gpio, "value": value, "data": data }).to_string()
    } else {
        format!("atb[{gpio}]: 0x{value:08X} ({value})\ndata: {data}")
    }
}

/// Human or JSON output for `echo`.
pub fn format_echo(data: &[u8], json_output: bool) -> String {
    if json_output {
        json!({ "len": data.len(), "data": hex::encode(data) }).to_string()
    } else {
        hex::encode(data)
    }
}

/// Human or JSON output for a raw tunnel reply.
pub fn format_raw(
    status: Option<u8>,
    data: &[u8],
    frame: &[u8],
    category_seq: Option<(u8, u8)>,
    json_output: bool,
) -> String {
    if json_output {
        let mut value = json!({
            "status": status,
            "data": hex::encode(data),
            "frame": hex::encode(frame),
        });
        if let Some((category, seq)) = category_seq {
            value["category"] = json!(category);
            value["seq"] = json!(seq);
        }
        value.to_string()
    } else {
        let status_text = match status {
            Some(status) => format!("0x{status:02X}"),
            None => "n/a".to_string(),
        };
        let mut text = format!(
            "status: {status_text}\ndata: {}\nframe: {}",
            hex::encode(data),
            hex::encode(frame)
        );
        if let Some((category, seq)) = category_seq {
            text = format!("category: 0x{category:02X}\nseq: {seq}\n{text}");
        }
        text
    }
}

/// Human or JSON output for `lpc`.
pub fn format_lpc(lpc: &Lpc, json_output: bool) -> String {
    let plc = hex::encode(lpc.plc);
    if json_output {
        json!({ "count": lpc.count, "plc": plc }).to_string()
    } else {
        format!("count: {}\nPLC: {plc}", lpc.count)
    }
}

/// Human or JSON output for `quality`.
pub fn format_quality(quality: &Quality, json_output: bool) -> String {
    if json_output {
        json!({
            "conn_handle": quality.conn_handle,
            "count": quality.count,
            "rssi": quality.rssi,
            "quality": quality.quality,
        })
        .to_string()
    } else {
        format!(
            "conn handle: 0x{:04X}\ncount: {}\nrssi: {} dBm\nquality: {}",
            quality.conn_handle, quality.count, quality.rssi, quality.quality
        )
    }
}

/// Human or JSON output for `devm`.
pub fn format_devm(devm: &Devm, json_output: bool) -> String {
    let data = hex::encode(devm.devm);
    if json_output {
        json!({
            "conn_handle": devm.conn_handle,
            "count": devm.count,
            "devm": data,
        })
        .to_string()
    } else {
        format!(
            "conn handle: 0x{:04X}\ncount: {}\nDEVM: {data}",
            devm.conn_handle, devm.count
        )
    }
}

/// Human or JSON output for `afh-map`.
pub fn format_afh_map(afh: &AfhMap, json_output: bool) -> String {
    let channels = hex::encode(afh.channels);
    if json_output {
        json!({ "conn_handle": afh.conn_handle, "channels": channels }).to_string()
    } else {
        format!(
            "conn handle: 0x{:04X}\nchannels: {channels}",
            afh.conn_handle
        )
    }
}
