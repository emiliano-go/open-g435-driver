//! Pure CLI formatting tests (no device access).

use std::path::Path;

use open_g435::cli::{
    format_battery, format_info, format_mode, format_mode_set, format_pair, format_read,
    format_read_written, format_rssi, format_scan, format_status, sha256_hex,
};
use open_g435::{GrsStatus, ImageVersion, Mode, Rssi};

fn json_value(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("valid JSON")
}

#[test]
fn formats_battery() {
    assert_eq!(format_battery(3831, false), "3831 mV");
    assert_eq!(
        json_value(&format_battery(3831, true)),
        serde_json::json!({ "millivolts": 3831 })
    );
}

#[test]
fn formats_status() {
    assert_eq!(
        format_status(0x01, false),
        "link status: 0x01 (bits 0b00000001)"
    );
    assert_eq!(
        json_value(&format_status(0xa5, true)),
        serde_json::json!({ "bits": 0xa5 })
    );
}

#[test]
fn formats_mode_and_mode_set() {
    assert_eq!(format_mode(Mode::Remote, false), "remote");
    assert_eq!(format_mode(Mode::Local, false), "local");
    assert_eq!(
        json_value(&format_mode(Mode::Remote, true)),
        serde_json::json!({ "mode": "remote" })
    );

    assert_eq!(format_mode_set(Mode::Remote, false), "mode set to remote");
    assert_eq!(
        json_value(&format_mode_set(Mode::Local, true)),
        serde_json::json!({ "set": "local" })
    );
}

#[test]
fn formats_rssi() {
    let rssi = Rssi {
        left_ap: -60,
        right_ap: -61,
        left_ptp: -127,
        right_ptp: -128,
    };
    assert_eq!(
        format_rssi(&rssi, false),
        "AP L/R: -60 / -61 dBm, PTP L/R: -127 / -128 dBm"
    );
    assert_eq!(
        json_value(&format_rssi(&rssi, true)),
        serde_json::json!({
            "left_ap": -60,
            "right_ap": -61,
            "left_ptp": -127,
            "right_ptp": -128,
        })
    );
}

#[test]
fn formats_scan() {
    assert_eq!(
        format_scan(true, false),
        "inquiry scan enabled (use --off to disable)"
    );
    assert_eq!(format_scan(false, false), "inquiry scan disabled");
    assert_eq!(
        json_value(&format_scan(true, true)),
        serde_json::json!({ "inquiry_scan": true })
    );
    assert_eq!(
        json_value(&format_scan(false, true)),
        serde_json::json!({ "inquiry_scan": false })
    );
}

#[test]
fn formats_pair() {
    let mac = [0xaa, 0x0b, 0x00, 0xdd, 0xee, 0xff];
    assert_eq!(
        format_pair(&mac, false),
        "connection requested for aa:0b:00:dd:ee:ff"
    );
    assert_eq!(
        json_value(&format_pair(&mac, true)),
        serde_json::json!({ "paired": "aa:0b:00:dd:ee:ff" })
    );
}

#[test]
fn formats_info() {
    let image = ImageVersion {
        raw: vec![0x01, 0x02, 0x03],
    };
    let grs = GrsStatus { bits: 0x05 };
    assert_eq!(
        format_info(&image, &grs, 3800, false),
        "image version: 010203\ngrs status: 0x05 (bits 0b00000101)\nbattery: 3800 mV"
    );
    assert_eq!(
        json_value(&format_info(&image, &grs, 3800, true)),
        serde_json::json!({
            "image_version": "010203",
            "grs_status": 5,
            "battery": 3800,
        })
    );
}

#[test]
fn formats_read_and_sha256() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        format_read(0x100, 3, b"abc", false),
        "616263\nsha256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        json_value(&format_read(0x100, 3, b"abc", true)),
        serde_json::json!({
            "addr": 0x100,
            "len": 3,
            "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "data": "616263",
        })
    );
    assert_eq!(
        format_read_written(Path::new("/tmp/out.bin"), b"abc"),
        "wrote 3 bytes to /tmp/out.bin sha256 ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
