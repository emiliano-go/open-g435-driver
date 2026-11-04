//! Device state-machine tests over a mock transport.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use open_g435::protocol::{
    self, encode_command_report, encode_pa_request, CATEGORY_BRIDGE, CATEGORY_OTA, MAX_STORAGE_READ,
};
use open_g435::transport::Transport;
use open_g435::{Error, Mode, G435};

struct MockState {
    writes: Vec<Vec<u8>>,
    features: VecDeque<Vec<u8>>,
    delayed_features: VecDeque<(usize, Vec<u8>)>,
    inputs: VecDeque<Vec<u8>>,
    ack_enter: bool,
    ack_exit: bool,
    feature_reads: usize,
    write_feature_reads: Vec<usize>,
}

impl Default for MockState {
    fn default() -> Self {
        Self {
            writes: Vec::new(),
            features: VecDeque::new(),
            delayed_features: VecDeque::new(),
            inputs: VecDeque::new(),
            ack_enter: true,
            ack_exit: true,
            feature_reads: 0,
            write_feature_reads: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<MockState>>);

impl Mock {
    fn new() -> Self {
        Self::default()
    }

    fn push_feature(&self, report: Vec<u8>) {
        self.0
            .lock()
            .expect("mock state")
            .features
            .push_back(report);
    }

    fn push_feature_after(&self, report: Vec<u8>, polls: usize) {
        self.0
            .lock()
            .expect("mock state")
            .delayed_features
            .push_back((polls, report));
    }

    fn writes(&self) -> Vec<Vec<u8>> {
        self.0.lock().expect("mock state").writes.clone()
    }

    fn write_feature_reads(&self) -> Vec<usize> {
        self.0
            .lock()
            .expect("mock state")
            .write_feature_reads
            .clone()
    }

    fn suppress_enter_ack(&self) {
        self.0.lock().expect("mock state").ack_enter = false;
    }

    fn suppress_exit_ack(&self) {
        self.0.lock().expect("mock state").ack_exit = false;
    }
}

impl Transport for Mock {
    fn write_output_report(&self, data: &[u8]) -> Result<(), Error> {
        let mut state = self.0.lock().expect("mock state");
        let feature_reads = state.feature_reads;
        state.write_feature_reads.push(feature_reads);
        state.writes.push(data.to_vec());
        if is_enter(data) && state.ack_enter {
            state.inputs.push_back(vec![0x06, 0x5a, 0, 0, 0, 0, 0]);
        }
        if is_exit(data) && state.ack_exit {
            state.inputs.push_back(vec![0x06, 0x5a, 0, 0, 0, 0, 0]);
        }
        if is_read_max_len(data) {
            state.inputs.push_back(vec![0x06, 0x5a, 0, 0, 0, 0, 0]);
        }
        Ok(())
    }

    fn read_feature_report(&self) -> Result<Option<Vec<u8>>, Error> {
        let mut state = self.0.lock().expect("mock state");
        state.feature_reads += 1;
        let delay = state.delayed_features.front().map(|(delay, _)| *delay);
        if let Some(delay) = delay {
            if delay > 0 {
                if let Some((remaining, _)) = state.delayed_features.front_mut() {
                    *remaining -= 1;
                }
                return Ok(None);
            }
            let (_, report) = state.delayed_features.pop_front().expect("delayed feature");
            return Ok(Some(report));
        }
        Ok(state.features.pop_front())
    }

    fn read_input_report(&self, _timeout: Duration) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.0.lock().expect("mock state").inputs.pop_front())
    }
}

fn is_enter(report: &[u8]) -> bool {
    report.len() == 515 && report[0] == 0x06 && report[1] == 0x01 && report[2] == 0x03
}

fn is_exit(report: &[u8]) -> bool {
    report.len() == 515 && report[0] == 0x06 && report[1] == 0x08 && report[2] == 0x03
}

fn is_read_max_len(report: &[u8]) -> bool {
    report.len() > 11
        && report[5] == 0x50
        && report[6] == 0x41
        && report[7] == CATEGORY_OTA
        && report[10] == 0x00
        && report[11] == 0x25
}

fn pi_frame(category: u8, seq: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x50, 0x49, category, seq, 0, 0, 0, 0];
    frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn feature(frame: &[u8]) -> Vec<u8> {
    assert!(frame.len() <= 63);
    let mut report = vec![0u8; 64];
    report[0] = 0x06;
    report[1..1 + frame.len()].copy_from_slice(frame);
    report[63] = frame.len() as u8;
    report
}

fn push_frame(mock: &Mock, frame: &[u8]) {
    for chunk in frame.chunks(62) {
        mock.push_feature(feature(chunk));
    }
}

fn multi_feature(frames: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = Vec::new();
    for frame in frames {
        payload.extend_from_slice(frame);
    }
    feature(&payload)
}

fn bridge_control(opaque: &[u8]) -> Vec<u8> {
    let mut fragment = vec![0x01, 0xfb, 0xda, 0x1f, 0xfd];
    fragment.extend_from_slice(&(opaque.len() as u16).to_be_bytes());
    fragment.push(0x00);
    fragment.extend_from_slice(opaque);
    fragment
}

fn is_more_frame(report: &[u8]) -> bool {
    report.len() == 515
        && report[5] == 0x50
        && report[6] == 0x41
        && report[7] == CATEGORY_OTA
        && report[9] == 0x01
        && report[10] == 0x00
        && report[11] == 0x24
}

fn ota_reply_category(category: u8, seq: u8, data: &[u8]) -> Vec<u8> {
    let mut payload = vec![CATEGORY_OTA, seq, 0x00];
    payload.extend_from_slice(data);
    pi_frame(category, seq, &payload)
}

fn ota_reply(seq: u8, data: &[u8]) -> Vec<u8> {
    ota_reply_category(CATEGORY_OTA, seq, data)
}

fn read_max_ack(seq: u8) -> Vec<u8> {
    ota_reply_category(CATEGORY_BRIDGE, seq, &[0xf0, 0x00, 0x00, 0x00])
}

fn ota_status(seq: u8, status: u8) -> Vec<u8> {
    pi_frame(CATEGORY_OTA, seq, &[CATEGORY_OTA, seq, status])
}

fn link_reply(seq: u8, command: u8, data: &[u8]) -> Vec<u8> {
    let mut payload = vec![command];
    payload.extend_from_slice(data);
    pi_frame(0x0e, seq, &payload)
}

#[test]
fn open_does_not_touch_the_device() {
    let mock = Mock::new();
    let device = G435::with_transport(Box::new(mock.clone()));
    drop(device);
    assert!(mock.writes().is_empty());
}

#[test]
fn battery_enters_and_exits_the_bridge() {
    let mock = Mock::new();
    mock.push_feature(feature(&link_reply(0, 0xe0, &[1])));
    mock.push_feature(feature(&ota_reply(0x7f, &[0x2c, 0x01])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.battery().expect("battery"), 300);

    let writes = mock.writes();
    // GET_MODE enforcement and the OTA query share one bridge session.
    assert_eq!(writes.len(), 4);
    assert!(is_enter(&writes[0]));
    assert!(is_exit(writes.last().expect("last write")));
    assert_eq!(writes.iter().filter(|write| is_enter(write)).count(), 1);
    assert_eq!(writes.iter().filter(|write| is_exit(write)).count(), 1);
}

#[test]
fn battery_mode_enforcement_and_ota_share_one_bridge() {
    let mock = Mock::new();
    mock.push_feature(feature(&link_reply(0, 0xe0, &[0])));
    mock.push_feature(feature(&link_reply(1, 0xe0, &[1])));
    mock.push_feature(feature(&ota_reply(0x30, &[0x2c, 0x01])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.battery().expect("battery"), 300);

    let writes = mock.writes();
    // enter, GET_MODE(local), SET_MODE(remote), GET_MODE(remote), OTA, exit
    assert_eq!(writes.len(), 6);
    assert!(is_enter(&writes[0]));
    assert!(is_exit(writes.last().expect("last write")));
    assert_eq!(writes.iter().filter(|write| is_enter(write)).count(), 1);
    assert_eq!(writes.iter().filter(|write| is_exit(write)).count(), 1);
    assert_eq!(
        writes[2],
        encode_command_report(
            &encode_pa_request(0x0e, 1, &protocol::link_payload(0xe1, &[1]))
                .expect("set mode frame")
        )
        .expect("report")
    );
}

#[test]
fn storage_waits_for_read_max_len_ack_before_first_read() {
    let mock = Mock::new();
    let data: Vec<u8> = (0..48u8).map(|value| value.wrapping_mul(5)).collect();
    // The optional READ_MAX_LEN acknowledgement arrives only after five empty
    // polls.
    mock.push_feature_after(feature(&read_max_ack(0x40)), 5);
    mock.push_feature(feature(&ota_reply(1, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.read_storage(0, 48).expect("read"), data);

    let writes = mock.writes();
    assert_eq!(writes.len(), 4);
    let feature_reads = mock.write_feature_reads();
    assert_eq!(feature_reads.len(), writes.len());
    // writes[2] is the first STORAGE_READ; the 250 ms wait must have consumed
    // the delayed acknowledgement first.
    assert!(
        feature_reads[2] >= 6,
        "first STORAGE_READ was sent before the READ_MAX_LEN acknowledgement"
    );
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn read_max_len_feature_ack_is_not_chunk_data() {
    let mock = Mock::new();
    // The READ_MAX_LEN feature ACK carries f0 00 00 00; it must be discarded.
    mock.push_feature(feature(&read_max_ack(0x40)));
    let data: Vec<u8> = (0..48u8).map(|value| value.wrapping_mul(9)).collect();
    mock.push_feature(feature(&ota_reply(1, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let out = device.read_storage(0, 48).expect("read");
    assert_eq!(out, data);
    assert_eq!(&out[..4], &[0x00, 0x09, 0x12, 0x1b]);
    assert_ne!(&out[..4], &[0xf0, 0x00, 0x00, 0x00]);
}

#[test]
fn two_chunk_storage_read_sends_chunks_in_order() {
    let mock = Mock::new();
    let data: Vec<u8> = (0..96u8).map(|value| value.wrapping_mul(7)).collect();
    mock.push_feature(feature(&read_max_ack(0x40)));
    mock.push_feature(feature(&ota_reply(1, &data[..48])));
    mock.push_feature(feature(&ota_reply(2, &data[48..])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.read_storage(0, 96).expect("read"), data);

    let writes = mock.writes();
    assert_eq!(writes.len(), 5);
    for (index, (addr, len)) in [(0u32, 48u32), (48, 48)].iter().enumerate() {
        let expected = encode_command_report(
            &encode_pa_request(
                CATEGORY_OTA,
                (index + 1) as u8,
                &protocol::storage_read_payload(*addr, *len).expect("payload"),
            )
            .expect("frame"),
        )
        .expect("report");
        assert_eq!(writes[index + 2], expected);
    }
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn read_storage_assembles_chunks_and_returns_exactly_len() {
    let mock = Mock::new();
    mock.push_feature(feature(&read_max_ack(0x40)));
    let data: Vec<u8> = (0..100u32).map(|value| (value * 3) as u8).collect();
    mock.push_feature(feature(&ota_reply(1, &data[..48])));
    mock.push_feature(feature(&ota_reply(2, &data[48..96])));
    mock.push_feature(feature(&ota_reply(3, &data[96..])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let out = device.read_storage(0x100, 100).expect("read storage");
    assert_eq!(out, data);

    let writes = mock.writes();
    assert_eq!(writes.len(), 6);
    assert!(is_enter(&writes[0]));
    assert!(is_exit(writes.last().expect("last write")));

    let expected_read_max = encode_command_report(
        &encode_pa_request(CATEGORY_OTA, 0, &protocol::read_max_len_payload()).expect("frame"),
    )
    .expect("report");
    assert_eq!(writes[1], expected_read_max);
    assert_eq!(
        writes
            .iter()
            .filter(|write| **write == expected_read_max)
            .count(),
        1
    );

    let expected_chunks = [(0x100, 48), (0x130, 48), (0x160, 4)];
    for (index, (addr, len)) in expected_chunks.iter().enumerate() {
        let expected = encode_command_report(
            &encode_pa_request(
                CATEGORY_OTA,
                (index + 1) as u8,
                &protocol::storage_read_payload(*addr, *len).expect("payload"),
            )
            .expect("frame"),
        )
        .expect("report");
        assert_eq!(writes[index + 2], expected);
    }
}

#[test]
fn read_storage_exits_the_bridge_on_error() {
    let mock = Mock::new();
    mock.push_feature(feature(&read_max_ack(0x40)));
    mock.push_feature(feature(&ota_status(1, 0x03)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let error = device.read_storage(0x100, 48).expect_err("must fail");
    assert!(matches!(error, Error::DeviceStatus(3)));
    assert!(is_exit(mock.writes().last().expect("last write")));
}

#[test]
fn battery_accepts_category_01_ota_reply() {
    let mock = Mock::new();
    mock.push_feature(feature(&link_reply(0, 0xe0, &[1])));
    mock.push_feature(feature(&ota_reply_category(
        CATEGORY_BRIDGE,
        0x81,
        &[0xfd, 0x0e, 0x00, 0x00],
    )));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.battery().expect("battery"), 0x0efd);

    let writes = mock.writes();
    assert_eq!(writes.len(), 4);
    assert!(!writes.iter().any(|write| is_more_frame(write)));
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn bridge_control_fragment_is_ignored_until_the_pi_frame() {
    let mock = Mock::new();
    mock.push_feature(feature(&link_reply(0, 0xe0, &[1])));
    // A 01-prefixed bridge control whose opaque bytes look like an OTA reply
    // with a bogus voltage.
    mock.push_feature(feature(&bridge_control(&[
        0x06, 0x81, 0x00, 0x11, 0x11, 0x00, 0x00,
    ])));
    mock.push_feature(feature(&ota_reply_category(
        CATEGORY_BRIDGE,
        0x82,
        &[0xfd, 0x0e, 0x00, 0x00],
    )));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.battery().expect("battery"), 0x0efd);
}

#[test]
fn bridge_control_and_pi_frame_in_one_fragment() {
    let mock = Mock::new();
    mock.push_feature(feature(&read_max_ack(0x40)));
    let data: Vec<u8> = (0..8u8).map(|value| value.wrapping_add(1)).collect();
    mock.push_feature(multi_feature(&[
        bridge_control(&[]),
        ota_reply_category(CATEGORY_BRIDGE, 1, &data),
    ]));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.read_storage(0x300, 8).expect("read"), data);
}

#[test]
fn read_storage_accepts_category_01_ota_reply() {
    let mock = Mock::new();
    mock.push_feature(feature(&read_max_ack(0x40)));
    let data: Vec<u8> = (0..8u8).map(|value| value * 3).collect();
    mock.push_feature(feature(&ota_reply_category(CATEGORY_BRIDGE, 1, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.read_storage(0x300, 8).expect("read"), data);

    let writes = mock.writes();
    assert_eq!(writes.len(), 4);
    assert!(!writes.iter().any(|write| is_more_frame(write)));
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn ota_query_skips_empty_ack_without_sending_more_frame() {
    let mock = Mock::new();
    mock.push_feature(feature(&link_reply(0, 0xe0, &[1])));
    mock.push_feature(feature(&ota_reply(0x11, &[])));
    mock.push_feature(feature(&ota_reply(0x22, &[0x2c, 0x01])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.battery().expect("battery"), 300);

    let writes = mock.writes();
    assert_eq!(writes.len(), 4);
    assert!(
        !writes.iter().any(|write| is_more_frame(write)),
        "battery/RSSI/GRS/image queries must never send MORE_FRAME"
    );
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn buffered_data_frame_survives_the_next_request() {
    let mock = Mock::new();
    mock.push_feature(feature(&read_max_ack(0x40)));
    let data: Vec<u8> = (0..8u8).collect();
    // Both frames arrive in one feature report: an empty storage frame and
    // the data frame. The second frame stays buffered while MORE_FRAME is
    // written.
    mock.push_feature(multi_feature(&[ota_reply(1, &[]), ota_reply(2, &data)]));

    let mut device = G435::with_transport_and_timeouts(
        Box::new(mock.clone()),
        Duration::from_millis(50),
        Duration::from_millis(50),
    );
    assert_eq!(device.read_storage(0x200, 8).expect("read"), data);

    let writes = mock.writes();
    assert_eq!(writes.len(), 5);
    assert!(is_more_frame(&writes[3]));
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn read_storage_requests_more_frames_when_data_is_empty() {
    let mock = Mock::new();
    mock.push_feature(feature(&read_max_ack(0x40)));
    let data: Vec<u8> = (0..8u8).collect();
    mock.push_feature(feature(&ota_reply(1, &[])));
    mock.push_feature(feature(&ota_reply(2, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.read_storage(0x200, 8).expect("read"), data);

    let writes = mock.writes();
    assert_eq!(writes.len(), 5);
    let more_frame = encode_command_report(
        &encode_pa_request(CATEGORY_OTA, 2, &protocol::more_frame_payload()).expect("frame"),
    )
    .expect("report");
    assert_eq!(writes[3], more_frame);
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn invalid_storage_arguments_do_not_touch_the_device() {
    let mock = Mock::new();
    let mut device = G435::with_transport(Box::new(mock.clone()));

    assert!(matches!(
        device.read_storage(2, 4),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        device.read_storage(4, 6),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        device.read_storage(0, MAX_STORAGE_READ + 4),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        device.read_storage(MAX_STORAGE_READ - 4, 8),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        device.read_storage(0xffff_fffc, 4),
        Err(Error::InvalidArgument(_))
    ));
    assert!(device.read_storage(4, 0).expect("empty").is_empty());
    assert!(mock.writes().is_empty());
}

#[test]
fn stale_events_and_acks_are_ignored() {
    let mock = Mock::new();
    mock.push_feature(feature(&link_reply(0, 0xe0, &[1])));
    // A stale event, a short bootloader ACK feature, then the real reply.
    mock.push_feature(feature(&link_reply(0, 0xe3, &[0xff])));
    mock.push_feature(vec![0x06, 0x5a]);
    mock.push_feature(feature(&ota_reply(0x7f, &[0x2c, 0x01])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.battery().expect("battery"), 300);
}

#[test]
fn link_mode_and_status_parse() {
    let mock = Mock::new();
    // Deliberately non-matching reply sequence bytes: replies are filtered by
    // category only.
    mock.push_feature(feature(&link_reply(0x42, 0xe0, &[1])));
    mock.push_feature(feature(&link_reply(0x99, 0xe3, &[0b1010_0101])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.mode().expect("mode"), Mode::Remote);
    assert_eq!(device.link_status().expect("status").bits, 0b1010_0101);

    let writes = mock.writes();
    assert_eq!(writes.len(), 6);
    for link_index in [1usize, 4] {
        assert!(is_enter(&writes[link_index - 1]));
        assert!(is_exit(&writes[link_index + 1]));
    }
}

#[test]
fn set_mode_is_confirmed_by_get_mode() {
    let mock = Mock::new();
    mock.push_feature(feature(&link_reply(0, 0xe0, &[1])));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    device.set_mode(Mode::Remote).expect("set mode");

    let writes = mock.writes();
    assert_eq!(writes.len(), 4);
    assert!(is_enter(&writes[0]));
    assert_eq!(
        writes[1],
        encode_command_report(
            &encode_pa_request(0x0e, 0, &protocol::link_payload(0xe1, &[1])).expect("set frame")
        )
        .expect("report")
    );
    assert_eq!(
        writes[2],
        encode_command_report(
            &encode_pa_request(0x0e, 1, &protocol::link_payload(0xe0, &[])).expect("get frame")
        )
        .expect("report")
    );
    assert!(is_exit(&writes[3]));
}

#[test]
fn inquiry_scan_and_connect_are_fire_and_forget() {
    let mock = Mock::new();
    let mut device = G435::with_transport(Box::new(mock.clone()));
    device.set_inquiry_scan(true).expect("scan on");
    device.set_inquiry_scan(false).expect("scan off");
    device
        .create_connect([1, 2, 3, 4, 5, 6])
        .expect("create connect");

    assert_eq!(mock.writes().len(), 9);
}

#[test]
fn set_mode_and_scan_are_written_as_link_commands() {
    let mock = Mock::new();
    // SET_MODE confirmation reply; scans are fire-and-forget.
    mock.push_feature(feature(&link_reply(0, 0xe0, &[1])));
    let mut device = G435::with_transport(Box::new(mock.clone()));
    device.set_mode(Mode::Remote).expect("set mode");
    device.set_inquiry_scan(true).expect("scan on");
    device.set_inquiry_scan(false).expect("scan off");

    let writes = mock.writes();
    assert_eq!(writes.len(), 10);
    // SET_MODE and its GET_MODE confirmation share one bridge session.
    assert!(is_enter(&writes[0]));
    assert!(is_exit(&writes[3]));
    // Each fire-and-forget scan has its own bridge session.
    for (enter_index, exit_index) in [(4usize, 6usize), (7, 9)] {
        assert!(is_enter(&writes[enter_index]));
        assert!(is_exit(&writes[exit_index]));
    }
    assert_eq!(
        writes[1],
        encode_command_report(
            &encode_pa_request(0x0e, 0, &protocol::link_payload(0xe1, &[1])).expect("frame")
        )
        .expect("report")
    );
    assert_eq!(
        writes[2],
        encode_command_report(
            &encode_pa_request(0x0e, 1, &protocol::link_payload(0xe0, &[])).expect("frame")
        )
        .expect("report")
    );
    assert_eq!(
        writes[5],
        encode_command_report(
            &encode_pa_request(0x0e, 2, &protocol::link_payload(0xf0, &[1])).expect("frame")
        )
        .expect("report")
    );
}

#[test]
fn create_connect_sends_address_lsb_first() {
    let mock = Mock::new();
    let mut device = G435::with_transport(Box::new(mock.clone()));
    device
        .create_connect([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])
        .expect("pair");

    let expected_payload = vec![0xe5, 0x00, 0x00, 0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa];
    let expected =
        encode_command_report(&encode_pa_request(0x0e, 0, &expected_payload).expect("frame"))
            .expect("report");
    let writes = mock.writes();
    assert_eq!(writes.len(), 3);
    assert!(is_enter(&writes[0]));
    assert_eq!(writes[1], expected);
    assert!(is_exit(&writes[2]));
}

#[test]
fn timeouts_are_bounded() {
    let mock = Mock::new();
    let mut device = G435::with_transport_and_timeouts(
        Box::new(mock.clone()),
        Duration::from_millis(10),
        Duration::from_millis(10),
    );
    let error = device.mode().expect_err("must time out");
    assert!(matches!(error, Error::Timeout(_)));
    let writes = mock.writes();
    assert_eq!(writes.len(), 3);
    assert!(is_enter(&writes[0]));
    assert!(is_exit(writes.last().expect("last write")));
}

#[test]
fn exit_ack_timeout_keeps_maybe_open_for_drop_retry() {
    let mock = Mock::new();
    mock.suppress_exit_ack();
    let mut device = G435::with_transport_and_timeouts(
        Box::new(mock.clone()),
        Duration::from_millis(10),
        Duration::from_millis(10),
    );
    let error = device
        .mode()
        .expect_err("exit acknowledgement must time out");
    assert!(matches!(error, Error::Timeout(_)));

    let exits_before_drop = mock.writes().iter().filter(|write| is_exit(write)).count();
    assert_eq!(exits_before_drop, 1);
    drop(device);
    let exits_after_drop = mock.writes().iter().filter(|write| is_exit(write)).count();
    assert_eq!(exits_after_drop, 2);
}

#[test]
fn bridge_enter_timeout_attempts_an_exit() {
    // Override the mock's enter acknowledgement by consuming it before it is
    // produced: the write is recorded, but no input report is queued.
    let mock = Mock::new();
    mock.suppress_enter_ack();
    let mut device = G435::with_transport_and_timeouts(
        Box::new(mock.clone()),
        Duration::from_millis(10),
        Duration::from_millis(10),
    );
    let result = device.read_storage(0x100, 4);
    // read_storage cannot succeed without replies; bridge enter fails first.
    assert!(matches!(result, Err(Error::Timeout(_))));

    let writes = mock.writes();
    assert!(is_enter(&writes[0]));
    assert!(
        writes.iter().any(|write| is_exit(write)),
        "an exit must be attempted after an enter timeout"
    );
}

#[test]
fn fs_table_reads_140_bytes() {
    let mock = Mock::new();
    let data: Vec<u8> = (0..140u32).map(|value| (value * 3) as u8).collect();
    push_frame(&mock, &ota_reply_category(CATEGORY_BRIDGE, 1, &data));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.fs_table().expect("fs-table"), data);

    let writes = mock.writes();
    assert_eq!(writes.len(), 3);
    assert!(is_enter(&writes[0]));
    assert!(is_exit(writes.last().expect("last write")));
    assert_eq!(
        writes[1],
        encode_command_report(
            &encode_pa_request(CATEGORY_OTA, 0, &protocol::fs_table_payload()).expect("frame")
        )
        .expect("report")
    );
}

#[test]
fn fs_index_decodes_offset_and_size() {
    let mock = Mock::new();
    let mut data = 0x1234u32.to_le_bytes().to_vec();
    data.extend_from_slice(&0x5678u32.to_le_bytes());
    mock.push_feature(feature(&ota_reply_category(CATEGORY_BRIDGE, 1, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let entry = device.fs_index(3).expect("fs-index");
    assert_eq!(entry.offset, 0x1234);
    assert_eq!(entry.size, 0x5678);
}

#[test]
fn blob_reads_cust_mp_and_nvds() {
    for (index, expected_id) in [("cust", 0x0fu8), ("mp", 0x01), ("nvds", 0x02)] {
        let mock = Mock::new();
        let data = vec![index.len() as u8, expected_id, 0xaa, 0xbb];
        mock.push_feature(feature(&ota_reply_category(CATEGORY_BRIDGE, 1, &data)));

        let mut device = G435::with_transport(Box::new(mock.clone()));
        let reply = match index {
            "cust" => device.cust_info(expected_id),
            "mp" => device.mp_read(expected_id),
            _ => device.nvds_read(expected_id),
        }
        .expect("blob read");
        assert_eq!(reply, data);
    }
}

#[test]
fn atb_decodes_4_byte_value() {
    let mock = Mock::new();
    mock.push_feature(feature(&ota_reply_category(
        CATEGORY_BRIDGE,
        1,
        &0x1234_5678u32.to_le_bytes(),
    )));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.atb(0).expect("atb"), 0x1234_5678);
}

#[test]
fn echo_returns_the_echoed_bytes() {
    let mock = Mock::new();
    let data = vec![0x00, 0x01, 0x02, 0x03];
    mock.push_feature(feature(&ota_reply_category(CATEGORY_BRIDGE, 1, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    assert_eq!(device.echo(&data).expect("echo"), data);
}

#[test]
fn lpc_parses_count_and_plc() {
    let mock = Mock::new();
    let mut data = vec![3u8];
    data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    mock.push_feature(feature(&link_reply(0, 0x04, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let lpc = device.lpc().expect("lpc");
    assert_eq!(lpc.count, 3);
    assert_eq!(lpc.plc, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(mock.writes().len(), 3);
}

#[test]
fn quality_parses_conn_handle_count_rssi_and_quality() {
    let mock = Mock::new();
    let data = vec![0x34, 0x12, 2, 0xc4, 77];
    mock.push_feature(feature(&link_reply(0, 0x03, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let quality = device.quality().expect("quality");
    assert_eq!(quality.conn_handle, 0x1234);
    assert_eq!(quality.count, 2);
    assert_eq!(quality.rssi, -60);
    assert_eq!(quality.quality, 77);
}

#[test]
fn devm_parses_conn_handle_count_and_80_bytes() {
    let mock = Mock::new();
    let mut data = vec![0x78, 0x56, 4];
    data.extend((0..80u8).map(|value| value.wrapping_add(10)));
    push_frame(&mock, &link_reply(0, 0x05, &data));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let devm = device.devm().expect("devm");
    assert_eq!(devm.conn_handle, 0x5678);
    assert_eq!(devm.count, 4);
    assert_eq!(devm.devm[0], 10);
    assert_eq!(devm.devm[79], 89);
}

#[test]
fn afh_map_parses_conn_handle_and_10_channels() {
    let mock = Mock::new();
    let mut data = vec![0xbc, 0x9a];
    data.extend(0..10u8);
    mock.push_feature(feature(&link_reply(0, 0x02, &data)));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let afh = device.afh_map().expect("afh-map");
    assert_eq!(afh.conn_handle, 0x9abc);
    assert_eq!(afh.channels, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

#[test]
fn typed_link_queries_reject_short_replies() {
    for (command, data) in [
        (0x04u8, vec![0u8; 8]),
        (0x03, vec![0u8; 4]),
        (0x05, vec![0u8; 82]),
        (0x02, vec![0u8; 11]),
    ] {
        let mock = Mock::new();
        push_frame(&mock, &link_reply(0, command, &data));
        let mut device = G435::with_transport(Box::new(mock.clone()));
        let result = match command {
            0x04 => device.lpc().map(|_| ()),
            0x03 => device.quality().map(|_| ()),
            0x05 => device.devm().map(|_| ()),
            _ => device.afh_map().map(|_| ()),
        };
        assert!(
            matches!(result, Err(Error::Protocol(_))),
            "command {command:#04x} accepted a short reply"
        );
    }
}

#[test]
fn raw_ota_returns_status_data_and_frame() {
    let mock = Mock::new();
    let reply = ota_reply_category(CATEGORY_BRIDGE, 0x55, &[0xaa, 0xbb]);
    mock.push_feature(feature(&reply));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let result = device.raw_ota(0x38, &[]).expect("raw ota");
    assert_eq!(result.status, 0);
    assert_eq!(result.data, vec![0xaa, 0xbb]);
    assert_eq!(result.frame, reply);
    assert_eq!(mock.writes().len(), 3);
}

#[test]
fn raw_ota_reports_nonzero_status() {
    let mock = Mock::new();
    let frame = pi_frame(CATEGORY_BRIDGE, 0x56, &[CATEGORY_OTA, 0x56, 0x07]);
    mock.push_feature(feature(&frame));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let result = device.raw_ota(0x31, &[]).expect("raw ota");
    assert_eq!(result.status, 7);
    assert!(result.data.is_empty());
}

#[test]
fn raw_link_returns_event_data_and_frame() {
    let mock = Mock::new();
    let reply = link_reply(0x55, 0xe0, &[1]);
    mock.push_feature(feature(&reply));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let result = device.raw_link(0xe0, &[]).expect("raw link");
    assert_eq!(result.data, vec![1]);
    assert_eq!(result.frame, reply);
}

#[test]
fn raw_pa_returns_first_pi_frame() {
    let mock = Mock::new();
    let reply = ota_reply_category(CATEGORY_BRIDGE, 0x55, &[0x2c, 0x01]);
    mock.push_feature(feature(&reply));

    let mut device = G435::with_transport(Box::new(mock.clone()));
    let result = device.raw_pa(CATEGORY_OTA, &[0x00, 0x31]).expect("raw pa");
    assert_eq!(result.category, CATEGORY_BRIDGE);
    assert_eq!(result.seq, 0x55);
    assert_eq!(result.status, Some(0));
    assert_eq!(result.data, vec![0x2c, 0x01]);
    assert_eq!(result.frame, reply);
}
