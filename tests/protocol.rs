//! Pure protocol tests: encoding, parsing, malformed input, bounds, assembly.

use open_g435::protocol::{
    self, encode_command_report, encode_pa_request, feature_fragment, is_ota_reply, parse_pi_frame,
    storage_chunks, FrameAssembler, CATEGORY_BRIDGE, CATEGORY_LINK, CATEGORY_OTA, MAX_PA_FRAME_LEN,
    MAX_RECEIVE_BUFFER, MAX_REPLY_FRAME_LEN, MAX_STORAGE_READ, OUTPUT_REPORT_LEN,
};

fn reference_frame(category: u8, seq: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x50, 0x49, category, seq, 0xaa, 0xbb, 0xcc, 0xdd];
    frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn feature_report(frame: &[u8]) -> Vec<u8> {
    assert!(frame.len() <= 63);
    let mut report = vec![0u8; 64];
    report[0] = 0x06;
    report[1..1 + frame.len()].copy_from_slice(frame);
    report[63] = frame.len() as u8;
    report
}

#[test]
fn encodes_storage_read_request_exactly() {
    let payload = protocol::storage_read_payload(0x1234_5678, 0x10).expect("payload");
    assert_eq!(
        payload,
        vec![0x00, 0x23, 0x01, 0x78, 0x56, 0x34, 0x12, 0x10, 0x00, 0x00, 0x00]
    );

    let frame = encode_pa_request(CATEGORY_OTA, 0x00, &payload).expect("frame");
    assert_eq!(
        frame,
        vec![
            0x50, 0x41, 0x06, 0x00, 0x0a, 0x00, 0x23, 0x01, 0x78, 0x56, 0x34, 0x12, 0x10, 0x00,
            0x00, 0x00
        ]
    );

    let report = encode_command_report(&frame).expect("report");
    assert_eq!(report.len(), OUTPUT_REPORT_LEN);
    assert_eq!(&report[..5], &[0x06, 0x04, 0x03, 0x10, 0x00]);
    assert_eq!(&report[5..5 + frame.len()], frame.as_slice());
    assert!(report[5 + frame.len()..].iter().all(|byte| *byte == 0));
}

#[test]
fn encodes_link_request_with_full_payload_length() {
    let payload = protocol::link_payload(0xe3, &[]);
    let frame = encode_pa_request(CATEGORY_LINK, 7, &payload).expect("frame");
    assert_eq!(frame, vec![0x50, 0x41, 0x0e, 0x07, 0x01, 0xe3]);
}

#[test]
fn encodes_bootloader_reports() {
    let enter = protocol::bootloader_enter_report();
    let exit = protocol::bootloader_exit_report();
    assert_eq!(enter.len(), OUTPUT_REPORT_LEN);
    assert_eq!(exit.len(), OUTPUT_REPORT_LEN);
    assert_eq!(&enter[..4], &[0x06, 0x01, 0x03, 0x00]);
    assert_eq!(&exit[..4], &[0x06, 0x08, 0x03, 0x00]);
    assert!(enter[4..].iter().all(|byte| *byte == 0));
    assert!(exit[4..].iter().all(|byte| *byte == 0));
}

#[test]
fn requests_reject_invalid_lengths_and_magic() {
    // The one-byte length field cannot express a 257-byte OTA payload.
    let mut ota = vec![0u8; 257];
    ota[0] = 0x00;
    assert!(encode_pa_request(CATEGORY_OTA, 0, &ota).is_err());

    // Link payload of 256 bytes overflows the one-byte length field.
    let link = vec![0xe3; 256];
    assert!(encode_pa_request(CATEGORY_LINK, 0, &link).is_err());

    // OTA payload has to start with zero.
    assert!(encode_pa_request(CATEGORY_OTA, 0, &[0x01, 0x31]).is_err());

    // Unknown category and empty payload are rejected.
    assert!(encode_pa_request(0x42, 0, &[0x00]).is_err());
    assert!(encode_pa_request(CATEGORY_LINK, 0, &[]).is_err());
}

#[test]
fn command_report_rejects_overlong_frames() {
    let frame = vec![0u8; MAX_PA_FRAME_LEN + 1];
    assert!(encode_command_report(&frame).is_err());
    assert!(encode_command_report(&[]).is_err());

    let frame = vec![0u8; MAX_PA_FRAME_LEN];
    assert!(encode_command_report(&frame).is_ok());
}

#[test]
fn parses_link_reply_and_ignores_trailing_bytes() {
    let mut frame = reference_frame(CATEGORY_LINK, 9, &[0xe3, 0x55]);
    frame.extend_from_slice(&[0xde, 0xad]);

    let parsed = parse_pi_frame(&frame).expect("parse");
    assert_eq!(parsed.category, CATEGORY_LINK);
    assert_eq!(parsed.seq, 9);
    assert_eq!(parsed.reserved, [0xaa, 0xbb, 0xcc, 0xdd]);
    assert_eq!(parsed.payload, vec![0xe3, 0x55]);
    assert_eq!(parsed.total_len(), 12);
    assert_eq!(parsed.link_data().expect("link data"), &[0x55]);
    assert_eq!(parsed.link_byte().expect("link byte"), 0x55);
}

#[test]
fn parses_ota_reply_data_and_status() {
    let frame = reference_frame(CATEGORY_OTA, 3, &[0x06, 0x03, 0x00, 0x2c, 0x01]);
    let parsed = parse_pi_frame(&frame).expect("parse");
    assert_eq!(parsed.ota_status().expect("status"), 0);
    assert_eq!(parsed.ota_data().expect("data"), &[0x2c, 0x01]);

    let failed = reference_frame(CATEGORY_OTA, 3, &[0x06, 0x03, 0x07]);
    let parsed = parse_pi_frame(&failed).expect("parse");
    assert_eq!(parsed.ota_status().expect("status"), 7);
    assert!(matches!(
        parsed.ota_data(),
        Err(open_g435::Error::DeviceStatus(7))
    ));
}

#[test]
fn parses_category_01_ota_reply_and_predicate() {
    let frame = reference_frame(
        CATEGORY_BRIDGE,
        0x81,
        &[0x06, 0x81, 0x00, 0xfd, 0x0e, 0x00, 0x00],
    );
    let parsed = parse_pi_frame(&frame).expect("category 01 OTA reply");
    assert_eq!(parsed.category, CATEGORY_BRIDGE);
    assert_eq!(parsed.seq, 0x81);
    assert_eq!(parsed.ota_status().expect("status"), 0);
    assert_eq!(parsed.ota_data().expect("data"), &[0xfd, 0x0e, 0x00, 0x00]);
    assert!(is_ota_reply(&parsed));

    let legacy = parse_pi_frame(&reference_frame(
        CATEGORY_OTA,
        2,
        &[0x06, 0x02, 0x00, 0x2c, 0x01],
    ))
    .expect("category 06 OTA reply");
    assert!(is_ota_reply(&legacy));

    // A link frame is not an OTA reply even if its payload starts with 0x06.
    let link =
        parse_pi_frame(&reference_frame(CATEGORY_LINK, 3, &[0x06, 0x01])).expect("link frame");
    assert!(!is_ota_reply(&link));

    // Category 01 with a non-OTA payload is not an OTA reply.
    let control =
        parse_pi_frame(&reference_frame(CATEGORY_BRIDGE, 4, &[0x0e, 0x00])).expect("control frame");
    assert!(!is_ota_reply(&control));
}

#[test]
fn rejects_malformed_reply_frames() {
    assert!(parse_pi_frame(&[]).is_err());
    assert!(parse_pi_frame(&[0x50, 0x49, 0x06, 0x00, 0, 0, 0, 0, 0, 0]).is_ok());

    let bad_magic = reference_frame(CATEGORY_OTA, 0, &[1, 2, 3]);
    let mut bad_magic = bad_magic;
    bad_magic[0] = 0x00;
    assert!(parse_pi_frame(&bad_magic).is_err());

    let frame = reference_frame(CATEGORY_OTA, 0, &[1, 2, 3]);
    assert!(parse_pi_frame(&frame[..frame.len() - 1]).is_err());

    // Declared payload above the assembler ceiling.
    let mut huge = vec![0x50, 0x49, 0x06, 0x00, 0, 0, 0, 0];
    huge.extend_from_slice(&((MAX_REPLY_FRAME_LEN + 1) as u16).to_le_bytes());
    assert!(parse_pi_frame(&huge).is_err());
}

#[test]
fn assembler_reassembles_fragmented_frame() {
    let frame = reference_frame(CATEGORY_OTA, 1, &[0x06, 0x01, 0x00, 9, 8, 7]);
    let mut assembler = FrameAssembler::new();

    assembler.push(&frame[..5]).expect("fragment");
    assert!(assembler.next_frame().is_none());

    assembler.push(&frame[5..]).expect("fragment");
    let parsed = assembler.next_frame().expect("frame");
    assert_eq!(parsed.payload, vec![0x06, 0x01, 0x00, 9, 8, 7]);
    assert!(assembler.next_frame().is_none());
    assert!(assembler.is_empty());
}

#[test]
fn assembler_resynchronizes_after_junk() {
    let frame = reference_frame(CATEGORY_LINK, 2, &[0xe0, 0x01]);
    let mut assembler = FrameAssembler::new();
    assembler.push(&[0x00, 0xff, 0x50, 0x00]).expect("junk");
    assert!(assembler.next_frame().is_none());
    assembler.push(&frame).expect("frame");
    let parsed = assembler.next_frame().expect("frame");
    assert_eq!(parsed.seq, 2);
}

#[test]
fn assembler_skips_invalid_complete_frame() {
    // Declared payload exceeds the ceiling; the assembler must drop one byte
    // and find the valid frame that follows.
    let mut bad = vec![0x50, 0x49, 0x06, 0x00, 0, 0, 0, 0];
    bad.extend_from_slice(&((MAX_REPLY_FRAME_LEN + 1) as u16).to_le_bytes());
    let good = reference_frame(CATEGORY_LINK, 4, &[0xe3, 0x01]);

    let mut assembler = FrameAssembler::new();
    assembler.push(&bad).expect("bad");
    assembler.push(&good).expect("good");
    let parsed = assembler.next_frame().expect("frame");
    assert_eq!(parsed.seq, 4);
}

#[test]
fn assembler_bounds_its_buffer() {
    let mut assembler = FrameAssembler::new();
    let chunk = vec![0u8; 1024];
    let mut errored = false;
    for _ in 0..(MAX_RECEIVE_BUFFER / chunk.len() + 2) {
        if assembler.push(&chunk).is_err() {
            errored = true;
            break;
        }
    }
    assert!(errored);
    assert!(assembler.buffered() <= 1);
}

#[test]
fn feature_fragment_validates_length_byte() {
    let frame = reference_frame(CATEGORY_LINK, 0, &[0xe3, 0x01]);
    let mut report = feature_report(&frame);
    assert_eq!(
        feature_fragment(&report).expect("fragment"),
        frame.as_slice()
    );

    report[63] = 0;
    assert!(feature_fragment(&report).is_none());
    report[63] = 64;
    assert!(feature_fragment(&report).is_none());

    assert!(feature_fragment(&[0u8; 32]).is_none());
    assert!(protocol::is_ack_feature(&[0x06, 0x5a]));
    assert!(protocol::is_ack_input(&[0x06, 0x5a, 0, 0, 0, 0, 0]));
    assert!(!protocol::is_ack_input(&[0x05, 0x5a, 0, 0, 0, 0, 0]));
}

#[test]
fn storage_plan_uses_four_aligned_chunks_of_at_most_48() {
    let plan = storage_chunks(0x100, 100).expect("plan");
    assert_eq!(plan, vec![(0x100, 48), (0x130, 48), (0x160, 4)]);

    for (addr, len) in [
        (0u32, 4u32),
        (0, 48),
        (0, 52),
        (0x1000, 96),
        (0x2000, 240),
        (0xfff0, 16),
    ] {
        let plan = storage_chunks(addr, len).expect("plan");
        assert_eq!(plan.iter().map(|(_, size)| *size).sum::<u32>(), len);
        for (chunk_addr, chunk_len) in plan {
            assert_eq!(chunk_addr % 4, 0);
            assert_eq!(chunk_len % 4, 0);
            assert!(chunk_len <= protocol::READ_CHUNK_SIZE);
            assert!(chunk_len > 0);
        }
    }

    assert!(storage_chunks(1, 4).is_err());
    assert!(storage_chunks(4, 6).is_err());
    assert!(storage_chunks(0xffff_fffc, 8).is_err());
    assert!(storage_chunks(4, 0).expect("empty").is_empty());
}

#[test]
fn storage_plan_enforces_one_mib_bound() {
    let plan = storage_chunks(0, MAX_STORAGE_READ).expect("full flash plan");
    assert_eq!(
        plan.iter().map(|(_, size)| *size).sum::<u32>(),
        MAX_STORAGE_READ
    );

    assert!(storage_chunks(0, MAX_STORAGE_READ + 4).is_err());
    assert!(storage_chunks(MAX_STORAGE_READ - 4, 8).is_err());
    assert!(storage_chunks(MAX_STORAGE_READ, 4).is_err());
    assert!(storage_chunks(0xffff_fffc, 4).is_err());
}

#[test]
fn storage_read_payload_rejects_unaligned_arguments() {
    assert!(protocol::storage_read_payload(2, 4).is_err());
    assert!(protocol::storage_read_payload(4, 2).is_err());
    assert!(protocol::storage_read_payload(0x10, 0x10).is_ok());
}

#[test]
fn constant_payloads_match_the_specification() {
    assert_eq!(
        protocol::read_max_len_payload(),
        vec![0x00, 0x25, 0x34, 0x12, 0x5a, 0x5a, 0x01, 0x00, 0x00, 0x00, 0xf0, 0x00, 0x00, 0x00]
    );
    assert_eq!(protocol::more_frame_payload(), vec![0x00, 0x24]);
    assert_eq!(protocol::get_battery_payload(), vec![0x00, 0x31]);
    assert_eq!(
        protocol::get_image_version_payload(),
        vec![0x00, 0x34, 0x00, 0x4d, 0x43]
    );

    let frame = encode_pa_request(CATEGORY_OTA, 0, &protocol::read_max_len_payload())
        .expect("read max frame");
    assert_eq!(frame[4], 0x0d);

    let frame = encode_pa_request(CATEGORY_OTA, 0, &protocol::get_image_version_payload())
        .expect("image version frame");
    assert_eq!(frame[4], 0x04);
}
