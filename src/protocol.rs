//! Wire format: HID reports, PA request frames, PI reply frames, and the
//! receive assembler.
//!
//! Everything in this module is pure (no device access) so it can be exercised
//! by unit tests.

use crate::{Error, Result};

/// Report ID used by the vendor HID collection.
pub const REPORT_ID: u8 = 0x06;

/// Full output report length, including the report ID.
pub const OUTPUT_REPORT_LEN: usize = 515;

/// Full feature report length.
pub const FEATURE_REPORT_LEN: usize = 64;

/// Input report length, including the report ID.
pub const INPUT_REPORT_LEN: usize = 7;

/// Number of header bytes in an output report before the PA frame.
pub const OUTPUT_HEADER_LEN: usize = 5;

/// Maximum PA frame that fits after the output-report header.
pub const MAX_PA_FRAME_LEN: usize = OUTPUT_REPORT_LEN - OUTPUT_HEADER_LEN;

/// Magic of a PA request frame (`PA`).
pub const REQUEST_MAGIC: [u8; 2] = [0x50, 0x41];

/// Magic of a PI reply frame (`PI`).
pub const REPLY_MAGIC: [u8; 2] = [0x50, 0x49];

/// OTA command category.
pub const CATEGORY_OTA: u8 = 0x06;

/// Link/radio command category.
pub const CATEGORY_LINK: u8 = 0x0e;

/// Category used by the bridge for OTA reply frames.
pub const CATEGORY_BRIDGE: u8 = 0x01;

/// Offset of the first OTA data byte inside a PI reply frame.
pub const OTA_DATA_OFFSET: usize = 13;

/// Offset of the first link/event data byte inside a PI reply frame.
pub const LINK_DATA_OFFSET: usize = 11;

/// Maximum number of storage bytes requested per chunk.
pub const READ_CHUNK_SIZE: u32 = 48;

/// Maximum-length value requested with `READ_MAX_LEN`.
pub const READ_MAX_LEN_VALUE: u32 = 240;

/// Maximum storage range accepted by the driver (1 MiB external flash).
pub const MAX_STORAGE_READ: u32 = 1 << 20;

/// Bootloader ACK marker.
pub const BOOTLOADER_ACK: u8 = 0x5a;

/// Hard ceiling for a single PI frame accepted by the assembler.
///
/// The protocol's declared length is 16-bit; the bridge only ever emits small
/// frames. The ceiling prevents a hostile device from making the receive
/// buffer grow without bound.
pub const MAX_REPLY_FRAME_LEN: usize = 4096;

/// Hard ceiling for the pending receive buffer.
pub const MAX_RECEIVE_BUFFER: usize = 16 * 1024;

/// Build a 515-byte output report carrying one PA frame.
///
/// The frame is placed after the five-byte header and the rest of the report
/// is zero-padded. Overlong frames are rejected, never truncated.
pub fn encode_command_report(pa_frame: &[u8]) -> Result<Vec<u8>> {
    if pa_frame.is_empty() {
        return Err(Error::InvalidArgument(
            "PA frame must not be empty".to_string(),
        ));
    }
    if pa_frame.len() > MAX_PA_FRAME_LEN {
        return Err(Error::InvalidArgument(format!(
            "PA frame is {} bytes, maximum is {}",
            pa_frame.len(),
            MAX_PA_FRAME_LEN
        )));
    }

    let mut report = vec![0u8; OUTPUT_REPORT_LEN];
    report[0] = REPORT_ID;
    report[1] = 0x04;
    report[2] = 0x03;
    report[3..5].copy_from_slice(&(pa_frame.len() as u16).to_le_bytes());
    report[OUTPUT_HEADER_LEN..OUTPUT_HEADER_LEN + pa_frame.len()].copy_from_slice(pa_frame);
    Ok(report)
}

/// Build a bootloader bridge enter report.
pub fn bootloader_enter_report() -> Vec<u8> {
    bootloader_report(0x01)
}

/// Build a bootloader bridge exit report.
pub fn bootloader_exit_report() -> Vec<u8> {
    bootloader_report(0x08)
}

fn bootloader_report(operation: u8) -> Vec<u8> {
    let mut report = vec![0u8; OUTPUT_REPORT_LEN];
    report[0] = REPORT_ID;
    report[1] = operation;
    report[2] = 0x03;
    report
}

/// Encode one PA request frame.
///
/// The length byte at offset 4 is one byte. For OTA it is
/// `payload.len() - 1` (the payload starts with `0x00`); for link it is
/// `payload.len()`.
pub fn encode_pa_request(category: u8, seq: u8, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.is_empty() {
        return Err(Error::InvalidArgument(
            "PA payload must not be empty".to_string(),
        ));
    }

    let len_byte = match category {
        CATEGORY_OTA => {
            if payload[0] != 0x00 {
                return Err(Error::InvalidArgument(
                    "OTA payload must start with 0x00".to_string(),
                ));
            }
            payload.len() - 1
        }
        CATEGORY_LINK => payload.len(),
        other => {
            return Err(Error::InvalidArgument(format!(
                "unknown PA category {other:#04x}"
            )))
        }
    };

    if len_byte > u8::MAX as usize {
        return Err(Error::InvalidArgument(format!(
            "PA payload length {len_byte} does not fit the one-byte length field"
        )));
    }

    let total = 5 + payload.len();
    if total > MAX_PA_FRAME_LEN {
        return Err(Error::InvalidArgument(format!(
            "PA request frame is {total} bytes, maximum is {MAX_PA_FRAME_LEN}"
        )));
    }

    let mut frame = Vec::with_capacity(total);
    frame.extend_from_slice(&REQUEST_MAGIC);
    frame.push(category);
    frame.push(seq);
    frame.push(len_byte as u8);
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Build an OTA payload: `00 <command> <args...>`.
pub fn ota_payload(command: u8, args: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + args.len());
    payload.push(0x00);
    payload.push(command);
    payload.extend_from_slice(args);
    payload
}

/// Build a link payload: `<command> <args...>`.
pub fn link_payload(command: u8, args: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + args.len());
    payload.push(command);
    payload.extend_from_slice(args);
    payload
}

/// `GET_BATTERY` OTA payload.
pub fn get_battery_payload() -> Vec<u8> {
    ota_payload(0x31, &[])
}

/// `GET_RSSI` OTA payload.
pub fn get_rssi_payload() -> Vec<u8> {
    ota_payload(0x32, &[])
}

/// `GET_GRS_STATUS` OTA payload.
pub fn get_grs_payload() -> Vec<u8> {
    ota_payload(0x33, &[])
}

/// `GET_IMAGE_VERSION` OTA payload.
///
/// The operation and signature bytes are not otherwise specified; zero is the
/// least intrusive read-only choice.
pub fn get_image_version_payload() -> Vec<u8> {
    ota_payload(0x34, &[0x00, 0x4d, 0x43])
}

/// `READ_MAX_LEN(240)` OTA payload.
pub fn read_max_len_payload() -> Vec<u8> {
    ota_payload(
        0x25,
        &[
            0x34, 0x12, 0x5a, 0x5a, 0x01, 0x00, 0x00, 0x00, 0xf0, 0x00, 0x00, 0x00,
        ],
    )
}

/// `MORE_FRAME` OTA payload.
pub fn more_frame_payload() -> Vec<u8> {
    ota_payload(0x24, &[])
}

/// `STORAGE_READ` OTA payload.
///
/// `addr` and `len` must be multiples of four.
pub fn storage_read_payload(addr: u32, len: u32) -> Result<Vec<u8>> {
    if addr % 4 != 0 {
        return Err(Error::InvalidArgument(format!(
            "storage address {addr:#x} is not a multiple of 4"
        )));
    }
    if len % 4 != 0 {
        return Err(Error::InvalidArgument(format!(
            "storage length {len:#x} is not a multiple of 4"
        )));
    }

    let mut args = Vec::with_capacity(9);
    args.push(0x01);
    args.extend_from_slice(&addr.to_le_bytes());
    args.extend_from_slice(&len.to_le_bytes());
    Ok(ota_payload(0x23, &args))
}

/// Split a storage range into bounded chunks whose sizes are multiples of four.
///
/// `len == 0` yields an empty plan, but arguments are still validated.
pub fn storage_chunks(addr: u32, len: u32) -> Result<Vec<(u32, u32)>> {
    if addr % 4 != 0 {
        return Err(Error::InvalidArgument(format!(
            "storage address {addr:#x} is not a multiple of 4"
        )));
    }
    if len % 4 != 0 {
        return Err(Error::InvalidArgument(format!(
            "storage length {len:#x} is not a multiple of 4"
        )));
    }
    if len == 0 {
        return Ok(Vec::new());
    }

    if len > MAX_STORAGE_READ {
        return Err(Error::InvalidArgument(format!(
            "storage length {len:#x} exceeds the {MAX_STORAGE_READ:#x} byte limit"
        )));
    }
    let end = addr
        .checked_add(len)
        .ok_or_else(|| Error::InvalidArgument("storage range overflows u32".to_string()))?;
    if end > MAX_STORAGE_READ {
        return Err(Error::InvalidArgument(format!(
            "storage range {addr:#x}+{len:#x} exceeds the {MAX_STORAGE_READ:#x} byte limit"
        )));
    }

    let mut chunks = Vec::new();
    let mut current = addr;
    while current < end {
        let remaining = end - current;
        let size = remaining.min(READ_CHUNK_SIZE);
        chunks.push((current, size));
        current += size;
    }
    Ok(chunks)
}

/// A parsed PI reply frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaFrame {
    /// Category byte at frame offset 2.
    pub category: u8,
    /// Sequence byte at frame offset 3.
    pub seq: u8,
    /// The four uninterpreted bytes at frame offsets 4 through 7.
    pub reserved: [u8; 4],
    /// Declared payload, with trailing report padding removed.
    pub payload: Vec<u8>,
}

impl PaFrame {
    /// Total length of the frame on the wire, including header and payload.
    pub fn total_len(&self) -> usize {
        10 + self.payload.len()
    }

    /// OTA status byte.
    ///
    /// The reply category may be `0x01` or `0x06`; only the payload view is
    /// meaningful here.
    pub fn ota_status(&self) -> Result<u8> {
        if self.payload.len() < 3 {
            return Err(Error::Protocol(format!(
                "OTA reply payload is {} bytes, need at least 3",
                self.payload.len()
            )));
        }
        if self.payload[0] != CATEGORY_OTA {
            return Err(Error::Protocol(format!(
                "OTA reply payload starts with {:#04x}",
                self.payload[0]
            )));
        }
        Ok(self.payload[2])
    }

    /// OTA data bytes, returning a device-status error when the status is
    /// non-zero.
    pub fn ota_data(&self) -> Result<&[u8]> {
        let status = self.ota_status()?;
        if status != 0 {
            return Err(Error::DeviceStatus(status));
        }
        Ok(&self.payload[3..])
    }

    /// OTA data bytes without interpreting the status byte.
    pub fn ota_data_raw(&self) -> Result<&[u8]> {
        if self.payload.len() < 3 || self.payload[0] != CATEGORY_OTA {
            return Err(Error::Protocol(
                "OTA reply payload is too short or not an OTA payload".to_string(),
            ));
        }
        Ok(&self.payload[3..])
    }

    /// Link/radio data bytes, starting after the command-ish first byte.
    pub fn link_data(&self) -> Result<&[u8]> {
        if self.payload.len() < 2 {
            return Err(Error::Protocol(format!(
                "link reply payload is {} bytes, need at least 2",
                self.payload.len()
            )));
        }
        Ok(&self.payload[1..])
    }

    /// First link/radio data byte.
    pub fn link_byte(&self) -> Result<u8> {
        Ok(self.link_data()?[0])
    }
}

/// Parse one PI reply frame from the start of `buf`.
///
/// Extra bytes after the declared frame length are ignored. Frames shorter
/// than ten bytes, with the wrong magic, or truncated relative to the declared
/// length are rejected.
pub fn parse_pi_frame(buf: &[u8]) -> Result<PaFrame> {
    if buf.len() < 10 {
        return Err(Error::Protocol(format!(
            "PI frame is {} bytes, need at least 10",
            buf.len()
        )));
    }
    if buf[0..2] != REPLY_MAGIC {
        return Err(Error::Protocol("bad PI reply magic".to_string()));
    }

    let payload_len = u16::from_le_bytes([buf[8], buf[9]]) as usize;
    if payload_len > MAX_REPLY_FRAME_LEN {
        return Err(Error::Protocol(format!(
            "declared PI payload {payload_len} exceeds {MAX_REPLY_FRAME_LEN}"
        )));
    }
    let total = 10 + payload_len;
    if buf.len() < total {
        return Err(Error::Protocol(format!(
            "truncated PI frame: declared {total} bytes, received {}",
            buf.len()
        )));
    }

    Ok(PaFrame {
        category: buf[2],
        seq: buf[3],
        reserved: [buf[4], buf[5], buf[6], buf[7]],
        payload: buf[10..total].to_vec(),
    })
}

/// Whether a PI frame is an OTA reply.
///
/// OTA replies use category `0x01` or `0x06` and their payload starts with
/// `0x06`. The request category is not echoed.
pub fn is_ota_reply(frame: &PaFrame) -> bool {
    matches!(frame.category, CATEGORY_BRIDGE | CATEGORY_OTA)
        && frame.payload.first() == Some(&CATEGORY_OTA)
}

/// Persistent receive buffer that turns feature-report fragments into frames.
#[derive(Debug, Default)]
pub struct FrameAssembler {
    buf: Vec<u8>,
}

impl FrameAssembler {
    /// Create an empty assembler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Discard everything buffered.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Number of buffered bytes.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Append a fragment.
    ///
    /// Returns an error if the pending buffer exceeds its hard ceiling; the
    /// buffer is then reduced to a possible magic prefix so the next call can
    /// resynchronize.
    pub fn push(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.buf.extend_from_slice(data);
        if self.buf.len() > MAX_RECEIVE_BUFFER {
            let keep_magic = self.buf.last() == Some(&REPLY_MAGIC[0]);
            let start = self.buf.len() - usize::from(keep_magic);
            self.buf.drain(..start);
            return Err(Error::Protocol(format!(
                "receive buffer exceeded {MAX_RECEIVE_BUFFER} bytes"
            )));
        }
        Ok(())
    }

    /// Pop the next complete, valid frame.
    ///
    /// Bytes before the next `PI` magic are discarded. A complete frame that
    /// fails validation consumes one byte and resynchronization continues.
    pub fn next_frame(&mut self) -> Option<PaFrame> {
        loop {
            if self.buf.is_empty() {
                return None;
            }
            if self.buf.len() < 2 {
                if self.buf[0] != REPLY_MAGIC[0] {
                    self.buf.clear();
                }
                return None;
            }

            if self.buf[0] != REPLY_MAGIC[0] || self.buf[1] != REPLY_MAGIC[1] {
                match find_reply_magic(&self.buf[1..]) {
                    Some(pos) => {
                        self.buf.drain(..1 + pos);
                    }
                    None => {
                        let keep_magic = self.buf.last() == Some(&REPLY_MAGIC[0]);
                        let start = self.buf.len() - usize::from(keep_magic);
                        self.buf.drain(..start);
                        return None;
                    }
                }
                continue;
            }

            if self.buf.len() < 10 {
                return None;
            }

            let payload_len = u16::from_le_bytes([self.buf[8], self.buf[9]]) as usize;
            let total = 10 + payload_len;
            if payload_len > MAX_REPLY_FRAME_LEN {
                self.buf.remove(0);
                continue;
            }
            if self.buf.len() < total {
                return None;
            }

            match parse_pi_frame(&self.buf[..total]) {
                Ok(frame) => {
                    self.buf.drain(..total);
                    return Some(frame);
                }
                Err(_) => {
                    self.buf.remove(0);
                }
            }
        }
    }
}

fn find_reply_magic(data: &[u8]) -> Option<usize> {
    data.windows(2).position(|pair| pair == REPLY_MAGIC)
}

/// Extract the fragment bytes from a 64-byte feature report.
///
/// Returns `None` when `n == 0` or `n > 63`.
pub fn feature_fragment(report: &[u8]) -> Option<&[u8]> {
    if report.len() < FEATURE_REPORT_LEN {
        return None;
    }
    let n = report[FEATURE_REPORT_LEN - 1] as usize;
    if n == 0 || n > FEATURE_REPORT_LEN - 1 {
        return None;
    }
    Some(&report[1..1 + n])
}

/// Interpret a feature report as a bootloader acknowledgement.
pub fn is_ack_feature(report: &[u8]) -> bool {
    report.len() >= 2 && report[1] == BOOTLOADER_ACK
}

/// Interpret an input report as a bootloader acknowledgement.
pub fn is_ack_input(report: &[u8]) -> bool {
    report.len() >= 2 && report[0] == REPORT_ID && report[1] == BOOTLOADER_ACK
}
