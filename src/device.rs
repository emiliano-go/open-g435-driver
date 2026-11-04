//! High-level driver state machine.

use std::str::FromStr;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::protocol::{
    self, FrameAssembler, PaFrame, CATEGORY_BRIDGE, CATEGORY_LINK, CATEGORY_OTA,
    FEATURE_REPORT_LEN, MAX_STORAGE_READ,
};
use crate::transport::{HidTransport, Transport};
use crate::{Error, Result, DEFAULT_PID, DEFAULT_VID};

/// Default timeout for one reply exchange.
const REPLY_TIMEOUT: Duration = Duration::from_millis(2000);

/// Default timeout for the bootloader acknowledgement.
const ACK_TIMEOUT: Duration = Duration::from_millis(1000);

/// Maximum frames inspected while waiting for one command's reply.
const MAX_FRAMES_PER_EXCHANGE: usize = 64;

/// Maximum `MORE_FRAME` requests used by one exchange or chunk.
const MAX_MORE_FRAMES: usize = 64;

/// Poll interval while no report is available.
const POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Maximum input reports drained in one pass.
const DRAIN_LIMIT: usize = 64;

/// Number of mode retries described by the specification.
const MODE_RETRIES: usize = 5;

/// Settle time after a `SET_MODE` request.
const MODE_SETTLE: Duration = Duration::from_millis(100);

/// Maximum wait for the optional `READ_MAX_LEN` acknowledgement.
const READ_MAX_LEN_ACK_WAIT: Duration = Duration::from_millis(250);

/// Link mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Local (receiver-side) mode.
    Local,
    /// Remote (headset-side) mode.
    Remote,
}

impl Mode {
    fn bit(self) -> u8 {
        match self {
            Mode::Local => 0,
            Mode::Remote => 1,
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Local => f.write_str("local"),
            Mode::Remote => f.write_str("remote"),
        }
    }
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("local") {
            Ok(Mode::Local)
        } else if value.eq_ignore_ascii_case("remote") {
            Ok(Mode::Remote)
        } else {
            Err(format!("invalid mode {value:?}, expected local or remote"))
        }
    }
}

/// Signal-strength report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rssi {
    /// Left access-point RSSI in dBm.
    pub left_ap: i8,
    /// Right access-point RSSI in dBm.
    pub right_ap: i8,
    /// Left point-to-point RSSI in dBm.
    pub left_ptp: i8,
    /// Right point-to-point RSSI in dBm.
    pub right_ptp: i8,
}

/// Link/connection status bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkStatus {
    /// Raw status bits.
    pub bits: u8,
}

/// GRS status bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrsStatus {
    /// Raw status bits.
    pub bits: u8,
}

/// Raw image-version reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageVersion {
    /// Raw data bytes returned by the device.
    pub raw: Vec<u8>,
}

impl ImageVersion {
    /// Raw bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.raw
    }

    /// Lowercase hexadecimal representation.
    pub fn hex(&self) -> String {
        hex::encode(&self.raw)
    }
}

/// Open handle for the G435 receiver.
pub struct G435 {
    transport: Box<dyn Transport>,
    seq: u8,
    assembler: FrameAssembler,
    bridge_open: bool,
    bridge_maybe_open: bool,
    bridge_link_ack: Option<Vec<u8>>,
    reply_timeout: Duration,
    ack_timeout: Duration,
}

impl G435 {
    /// Open the first receiver with the default VID/PID.
    ///
    /// Opening does not change device state and does not enter the bootloader
    /// bridge.
    pub fn open() -> Result<Self> {
        Self::open_with(DEFAULT_VID, DEFAULT_PID)
    }

    /// Open the first receiver with an explicit VID/PID.
    pub fn open_with(vid: u16, pid: u16) -> Result<Self> {
        let transport = HidTransport::open_with(vid, pid)?;
        Ok(Self::with_transport(Box::new(transport)))
    }

    /// Build a driver over a custom transport.
    ///
    /// Intended for tests and alternate backends; opening does not touch the
    /// device.
    pub fn with_transport(transport: Box<dyn Transport>) -> Self {
        Self::with_transport_and_timeouts(transport, REPLY_TIMEOUT, ACK_TIMEOUT)
    }

    /// Build a driver over a custom transport with explicit timeouts.
    pub fn with_transport_and_timeouts(
        transport: Box<dyn Transport>,
        reply_timeout: Duration,
        ack_timeout: Duration,
    ) -> Self {
        Self {
            transport,
            seq: 0,
            assembler: FrameAssembler::new(),
            bridge_open: false,
            bridge_maybe_open: false,
            bridge_link_ack: None,
            reply_timeout,
            ack_timeout,
        }
    }

    /// Read battery voltage in millivolts.
    pub fn battery(&mut self) -> Result<u16> {
        let data = self.with_ota_bridge(&protocol::get_battery_payload())?;
        if data.len() < 2 {
            return Err(Error::Protocol(format!(
                "battery reply is {} bytes, need at least 2",
                data.len()
            )));
        }
        Ok(u16::from_le_bytes([data[0], data[1]]))
    }

    /// Read the link/connection status bitmask.
    pub fn link_status(&mut self) -> Result<LinkStatus> {
        let frame = self.link_query(&protocol::link_payload(0xe3, &[]))?;
        Ok(LinkStatus {
            bits: frame.link_byte()?,
        })
    }

    /// Read the current link mode.
    pub fn mode(&mut self) -> Result<Mode> {
        let frame = self.link_query(&protocol::link_payload(0xe0, &[]))?;
        Ok(if frame.link_byte()? == 1 {
            Mode::Remote
        } else {
            Mode::Local
        })
    }

    /// Set the link mode.
    ///
    /// The request is written and acknowledged only by draining stale input.
    pub fn set_mode(&mut self, mode: Mode) -> Result<()> {
        self.send_link(&protocol::link_payload(0xe1, &[mode.bit()]))
    }

    /// Enable or disable Bluetooth inquiry scan.
    pub fn set_inquiry_scan(&mut self, enabled: bool) -> Result<()> {
        self.send_link(&protocol::link_payload(0xf0, &[u8::from(enabled)]))
    }

    /// Request a connection to `address`.
    ///
    /// The address is the six bytes in display order (most significant octet
    /// first); it is sent least-significant byte first as the protocol
    /// requires. The profile byte is not specified, so `0x00` is used.
    pub fn create_connect(&mut self, address: [u8; 6]) -> Result<()> {
        let mut args = Vec::with_capacity(8);
        args.push(0x00);
        args.push(0x00);
        args.extend(address.iter().rev().copied());
        self.send_link(&protocol::link_payload(0xe5, &args))
    }

    /// Read the four RSSI values.
    pub fn rssi(&mut self) -> Result<Rssi> {
        let data = self.with_ota_bridge(&protocol::get_rssi_payload())?;
        if data.len() < 4 {
            return Err(Error::Protocol(format!(
                "RSSI reply is {} bytes, need at least 4",
                data.len()
            )));
        }
        Ok(Rssi {
            left_ap: data[0] as i8,
            right_ap: data[1] as i8,
            left_ptp: data[2] as i8,
            right_ptp: data[3] as i8,
        })
    }

    /// Read the GRS status bitmask.
    pub fn grs_status(&mut self) -> Result<GrsStatus> {
        let data = self.with_ota_bridge(&protocol::get_grs_payload())?;
        if data.is_empty() {
            return Err(Error::Protocol(
                "GRS reply contains no status byte".to_string(),
            ));
        }
        Ok(GrsStatus { bits: data[0] })
    }

    /// Read the raw image version.
    pub fn image_version(&mut self) -> Result<ImageVersion> {
        let raw = self.with_ota_bridge(&protocol::get_image_version_payload())?;
        Ok(ImageVersion { raw })
    }

    /// Read `len` bytes starting at `addr`.
    ///
    /// Both `addr` and `len` must be multiples of four. A zero length returns
    /// an empty vector without touching the device.
    pub fn read_storage(&mut self, addr: u32, len: u32) -> Result<Vec<u8>> {
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
        if len == 0 {
            return Ok(Vec::new());
        }

        let chunks = protocol::storage_chunks(addr, len)?;
        let total = len as usize;

        self.with_bridge(|driver| {
            let seq = driver.next_seq();
            driver.send_request(CATEGORY_OTA, seq, &protocol::read_max_len_payload())?;
            driver.consume_optional_ack()?;

            let mut out = Vec::with_capacity(total);
            for (chunk_addr, chunk_len) in chunks {
                let chunk = driver.read_chunk(chunk_addr, chunk_len)?;
                out.extend_from_slice(&chunk);
                driver.drain_input()?;
            }

            if out.len() != total {
                return Err(Error::Protocol(format!(
                    "storage read returned {} of {total} requested bytes",
                    out.len()
                )));
            }
            Ok(out)
        })
    }

    /// Ensure the receiver is in remote mode; caller holds the bridge.
    fn ensure_remote_locked(&mut self) -> Result<()> {
        for attempt in 0..=MODE_RETRIES {
            let frame = self.link_query_raw(&protocol::link_payload(0xe0, &[]))?;
            if frame.link_byte()? == 1 {
                return Ok(());
            }
            if attempt == MODE_RETRIES {
                break;
            }
            self.send_link_raw(&protocol::link_payload(0xe1, &[Mode::Remote.bit()]))?;
            sleep(MODE_SETTLE);
        }
        Err(Error::Protocol(
            "receiver did not switch to remote mode".to_string(),
        ))
    }

    /// Run remote-mode enforcement and one OTA query in a single bridge scope.
    fn with_ota_bridge(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        self.with_bridge(|driver| {
            driver.ensure_remote_locked()?;
            driver.ota_query(payload)
        })
    }

    /// Run `operation` with the bootloader bridge open, exiting on every path.
    fn with_bridge<T>(&mut self, operation: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.enter_bridge()?;
        let result = operation(self);
        let exit_result = self.exit_bridge();

        match (result, exit_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(exit_error)) => Err(exit_error),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(exit_error)) => {
                tracing::warn!(%exit_error, "bootloader exit failed after operation error");
                Err(error)
            }
        }
    }

    fn enter_bridge(&mut self) -> Result<()> {
        if self.bridge_open {
            return Ok(());
        }

        self.assembler.clear();
        self.drain_input()?;
        self.transport
            .write_output_report(&protocol::bootloader_enter_report())?;

        let deadline = Instant::now() + self.ack_timeout;
        loop {
            if self.acknowledged()? {
                self.bridge_open = true;
                self.bridge_maybe_open = false;
                self.drain_input()?;
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            sleep(POLL_INTERVAL);
        }

        // The write succeeded but the acknowledgement did not: the device may
        // still have entered the bridge. Attempt an exit before failing.
        self.bridge_open = false;
        self.bridge_maybe_open = true;
        let _ = self.exit_bridge();
        Err(Error::Timeout(
            "bootloader enter acknowledgement".to_string(),
        ))
    }

    fn exit_bridge(&mut self) -> Result<()> {
        if !self.bridge_open && !self.bridge_maybe_open {
            return Ok(());
        }

        self.bridge_open = false;
        self.bridge_maybe_open = true;
        self.write_bridge_exit()?;

        let deadline = Instant::now() + self.ack_timeout;
        loop {
            if self.acknowledged()? {
                self.bridge_maybe_open = false;
                self.drain_input()?;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout(
                    "bootloader exit acknowledgement".to_string(),
                ));
            }
            sleep(POLL_INTERVAL);
        }
    }

    fn write_bridge_exit(&self) -> Result<()> {
        self.transport
            .write_output_report(&protocol::bootloader_exit_report())
    }

    fn acknowledged(&mut self) -> Result<bool> {
        if let Some(report) = self.transport.read_input_report(Duration::ZERO)? {
            if protocol::is_ack_input(&report) {
                return Ok(true);
            }
        }
        if let Some(report) = self.transport.read_feature_report()? {
            if protocol::is_ack_feature(&report) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn send_link(&mut self, payload: &[u8]) -> Result<()> {
        self.with_bridge(|driver| driver.send_link_raw(payload))
    }

    fn send_link_raw(&mut self, payload: &[u8]) -> Result<()> {
        let seq = self.next_seq();
        self.bridge_link_ack = None;
        self.send_request(CATEGORY_LINK, seq, payload)?;

        let deadline = Instant::now() + self.reply_timeout;
        let mut frames = 0usize;
        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout(
                    "waiting for link event after setter request".to_string(),
                ));
            }

            self.drain_input()?;
            if let Some(frame) = self.poll_frame()? {
                frames += 1;
                if frames > MAX_FRAMES_PER_EXCHANGE {
                    return Err(Error::Protocol(format!(
                        "received more than {MAX_FRAMES_PER_EXCHANGE} frames waiting for setter event"
                    )));
                }
                if frame.category == CATEGORY_LINK {
                    frame.link_data()?;
                    self.bridge_link_ack = None;
                    return Ok(());
                }
            } else if self.bridge_link_ack.take().is_some() {
                // Some setters answer with a bridge control ACK carrying an
                // 0x0e payload but no PI event.
                return Ok(());
            }
        }
    }

    fn link_query(&mut self, payload: &[u8]) -> Result<PaFrame> {
        self.with_bridge(|driver| driver.link_query_raw(payload))
    }

    fn link_query_raw(&mut self, payload: &[u8]) -> Result<PaFrame> {
        let seq = self.next_seq();
        self.send_request(CATEGORY_LINK, seq, payload)?;
        let frame = self.wait_for_frame(CATEGORY_LINK)?;
        frame.link_data()?;
        Ok(frame)
    }

    fn ota_query(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let seq = self.next_seq();
        self.send_request(CATEGORY_OTA, seq, payload)?;

        for _ in 0..MAX_FRAMES_PER_EXCHANGE {
            let frame = self.wait_for_frame(CATEGORY_OTA)?;
            let status = frame.ota_status()?;
            if status != 0 {
                return Err(Error::DeviceStatus(status));
            }
            let data = frame.ota_data_raw()?;
            if !data.is_empty() {
                return Ok(data.to_vec());
            }
        }

        Err(Error::Protocol(
            "device returned no non-empty OTA data frame within frame limit".to_string(),
        ))
    }

    fn read_chunk(&mut self, addr: u32, len: u32) -> Result<Vec<u8>> {
        let want = len as usize;
        let mut seq = self.next_seq();
        self.send_request(
            CATEGORY_OTA,
            seq,
            &protocol::storage_read_payload(addr, len)?,
        )?;

        let mut out = Vec::with_capacity(want);
        for _ in 0..=MAX_MORE_FRAMES {
            let frame = self.wait_for_frame(CATEGORY_OTA)?;
            let status = frame.ota_status()?;
            if status != 0 {
                return Err(Error::DeviceStatus(status));
            }

            let data = frame.ota_data_raw()?;
            let take = (want - out.len()).min(data.len());
            out.extend_from_slice(&data[..take]);
            if out.len() >= want {
                return Ok(out);
            }

            seq = self.next_seq();
            self.send_request(CATEGORY_OTA, seq, &protocol::more_frame_payload())?;
        }

        Err(Error::Protocol(
            "storage chunk did not complete within MORE_FRAME limit".to_string(),
        ))
    }

    fn consume_optional_ack(&mut self) -> Result<()> {
        let deadline = Instant::now() + READ_MAX_LEN_ACK_WAIT;
        loop {
            // Input ACKs (06 5a) must not end this wait: the READ_MAX_LEN
            // acknowledgement arrives as a feature-report PI or bridge-control
            // frame and carries data that must be discarded.
            if let Some(report) = self.transport.read_feature_report()? {
                // A short 06 5a feature ACK is not the READ_MAX_LEN frame;
                // keep waiting for the feature PI/control frame.
                if !protocol::is_ack_feature(&report) && report.len() >= FEATURE_REPORT_LEN {
                    if let Some(fragment) = protocol::feature_fragment(&report) {
                        if fragment.first() == Some(&CATEGORY_BRIDGE) {
                            self.drain_input()?;
                            return Ok(());
                        }
                        self.assembler.push(fragment)?;
                        if self.assembler.next_frame().is_some() {
                            self.drain_input()?;
                            return Ok(());
                        }
                    }
                }
            }

            if Instant::now() >= deadline {
                return Ok(());
            }
            sleep(POLL_INTERVAL);
        }
    }

    fn next_seq(&mut self) -> u8 {
        let current = self.seq;
        self.seq = self.seq.wrapping_add(1);
        current
    }

    fn send_request(&mut self, category: u8, seq: u8, payload: &[u8]) -> Result<()> {
        // Keep the persistent receive buffer: a data frame may already be
        // buffered after an empty ACK, including across MORE_FRAME requests.
        self.drain_input()?;

        let frame = protocol::encode_pa_request(category, seq, payload)?;
        tracing::debug!(
            category = format_args!("{category:#04x}"),
            seq,
            payload_len = payload.len(),
            "sending PA request"
        );
        let report = protocol::encode_command_report(&frame)?;
        self.transport.write_output_report(&report)?;
        Ok(())
    }

    fn wait_for_frame(&mut self, category: u8) -> Result<PaFrame> {
        let deadline = Instant::now() + self.reply_timeout;
        let mut frames = 0usize;

        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout(format!(
                    "waiting for category {category:#04x} reply"
                )));
            }

            self.drain_input()?;
            if let Some(frame) = self.poll_frame()? {
                frames += 1;
                if frames > MAX_FRAMES_PER_EXCHANGE {
                    return Err(Error::Protocol(format!(
                        "received more than {MAX_FRAMES_PER_EXCHANGE} frames waiting for reply"
                    )));
                }
                let matches = if category == CATEGORY_OTA {
                    protocol::is_ota_reply(&frame)
                } else {
                    frame.category == category
                };
                if matches {
                    // Drain stale ACKs/events before the next command.
                    self.drain_input()?;
                    return Ok(frame);
                }
            }
        }
    }

    fn poll_frame(&mut self) -> Result<Option<PaFrame>> {
        if let Some(frame) = self.assembler.next_frame() {
            return Ok(Some(frame));
        }

        let report = match self.transport.read_feature_report()? {
            Some(report) => report,
            None => {
                sleep(POLL_INTERVAL);
                return Ok(None);
            }
        };

        if protocol::is_ack_feature(&report) {
            return Ok(None);
        }
        if report.len() < FEATURE_REPORT_LEN {
            return Ok(None);
        }

        let Some(fragment) = protocol::feature_fragment(&report) else {
            tracing::debug!(
                report = %hex::encode(&report[..16.min(report.len())]),
                last = report.last().copied().unwrap_or(0),
                "feature report without a usable fragment"
            );
            return Ok(None);
        };

        tracing::debug!(
            fragment_len = fragment.len(),
            fragment = %hex::encode(&fragment[..fragment.len().min(32)]),
            "feature fragment"
        );

        if fragment.first() == Some(&CATEGORY_BRIDGE) && self.assembler.is_empty() {
            // Bridge control/ACK frame, not a PI frame. It may be followed by
            // a complete PI frame in the same fragment; keep the PI suffix and
            // discard only the control prefix.
            if fragment.get(8) == Some(&CATEGORY_LINK) {
                self.bridge_link_ack = Some(fragment.to_vec());
            }
            if let Some(offset) = fragment
                .windows(2)
                .position(|pair| pair == protocol::REPLY_MAGIC)
            {
                tracing::debug!(offset, "recovering PI frame after bridge control");
                self.assembler.push(&fragment[offset..])?;
                return Ok(self.assembler.next_frame());
            }
            tracing::debug!("ignoring bridge control fragment");
            return Ok(None);
        }

        self.assembler.push(fragment)?;
        let frame = self.assembler.next_frame();
        if let Some(frame) = &frame {
            tracing::debug!(
                category = format_args!("{:#04x}", frame.category),
                seq = frame.seq,
                payload_len = frame.payload.len(),
                "parsed PI frame"
            );
        }
        Ok(frame)
    }

    fn drain_input(&mut self) -> Result<()> {
        for _ in 0..DRAIN_LIMIT {
            if self.transport.read_input_report(Duration::ZERO)?.is_none() {
                break;
            }
        }
        Ok(())
    }
}

impl Drop for G435 {
    fn drop(&mut self) {
        if self.bridge_open || self.bridge_maybe_open {
            let _ = self.write_bridge_exit();
        }
    }
}
