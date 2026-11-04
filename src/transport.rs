//! HID transport boundary.
//!
//! The [`Transport`] trait keeps device I/O testable; [`HidTransport`] is the
//! real hidapi-backed implementation.

use std::time::Duration;

use crate::protocol::{FEATURE_REPORT_LEN, INPUT_REPORT_LEN, REPORT_ID};
use crate::{Error, Result};

/// Blocking HID operations used by the driver.
///
/// Implementations must not change device state when constructed or when
/// reports are read.
pub trait Transport: Send {
    /// Write one full output report.
    fn write_output_report(&self, data: &[u8]) -> Result<()>;

    /// Read one feature report, returning the full report bytes.
    ///
    /// `None` means no report was available.
    fn read_feature_report(&self) -> Result<Option<Vec<u8>>>;

    /// Read one input report, waiting at most `timeout`.
    ///
    /// `None` means no report was available.
    fn read_input_report(&self, timeout: Duration) -> Result<Option<Vec<u8>>>;
}

/// hidapi-backed transport.
pub struct HidTransport {
    device: hidapi::HidDevice,
}

impl HidTransport {
    /// Open the first HID interface matching `vid`/`pid`.
    ///
    /// Opening does not send anything to the device.
    pub fn open_with(vid: u16, pid: u16) -> Result<Self> {
        let api = hidapi::HidApi::new().map_err(Error::Hid)?;
        let device = api
            .open(vid, pid)
            .map_err(|source| Error::Open { vid, pid, source })?;
        Ok(Self { device })
    }
}

impl Transport for HidTransport {
    fn write_output_report(&self, data: &[u8]) -> Result<()> {
        let written = self.device.write(data).map_err(Error::Hid)?;
        if written != data.len() {
            return Err(Error::Protocol(format!(
                "HID write was truncated: {written} of {} bytes",
                data.len()
            )));
        }
        Ok(())
    }

    fn read_feature_report(&self) -> Result<Option<Vec<u8>>> {
        let mut buf = [0u8; FEATURE_REPORT_LEN];
        buf[0] = REPORT_ID;
        let read = self
            .device
            .get_feature_report(&mut buf)
            .map_err(Error::Hid)?;
        if read == 0 {
            return Ok(None);
        }

        let mut report = vec![0u8; FEATURE_REPORT_LEN];
        let copied = read.min(FEATURE_REPORT_LEN);
        report[..copied].copy_from_slice(&buf[..copied]);
        Ok(Some(report))
    }

    fn read_input_report(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        // One spare byte keeps backends that round up to the next report
        // boundary happy; the extra byte is discarded.
        let mut buf = [0u8; INPUT_REPORT_LEN + 1];
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        let read = self
            .device
            .read_timeout(&mut buf, millis)
            .map_err(Error::Hid)?;
        if read == 0 {
            return Ok(None);
        }
        let copied = read.min(buf.len());
        Ok(Some(buf[..copied].to_vec()))
    }
}
