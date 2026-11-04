//! CLI parsing tests.

use clap::Parser;
use open_g435::cli::{format_mac, parse_mac, parse_number, Cli, Command};
use open_g435::Mode;

#[test]
fn parses_read_with_decimal_and_hex_numbers() {
    let cli = Cli::try_parse_from(["g435ctl", "read", "0x20", "48"]).expect("parse");
    assert!(!cli.json);
    match cli.command {
        Command::Read { addr, len, output } => {
            assert_eq!(addr, 0x20);
            assert_eq!(len, 48);
            assert!(output.is_none());
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_global_json_before_and_after_subcommand() {
    let before = Cli::try_parse_from(["g435ctl", "--json", "battery"]).expect("parse");
    assert!(before.json);

    let after = Cli::try_parse_from(["g435ctl", "battery", "--json"]).expect("parse");
    assert!(after.json);
}

#[test]
fn parses_read_output_path() {
    let cli = Cli::try_parse_from([
        "g435ctl",
        "read",
        "0x1000",
        "0x30",
        "--output",
        "/tmp/dump.bin",
    ])
    .expect("parse");

    match cli.command {
        Command::Read { addr, len, output } => {
            assert_eq!(addr, 0x1000);
            assert_eq!(len, 0x30);
            assert_eq!(output.expect("path").to_string_lossy(), "/tmp/dump.bin");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parses_mode_set() {
    let cli = Cli::try_parse_from(["g435ctl", "mode", "--set", "remote"]).expect("parse");
    match cli.command {
        Command::Mode { set } => assert_eq!(set, Some(Mode::Remote)),
        other => panic!("unexpected command: {other:?}"),
    }

    let cli = Cli::try_parse_from(["g435ctl", "mode"]).expect("parse");
    match cli.command {
        Command::Mode { set } => assert_eq!(set, None),
        other => panic!("unexpected command: {other:?}"),
    }

    assert!(Cli::try_parse_from(["g435ctl", "mode", "--set", "sideways"]).is_err());
}

#[test]
fn parses_scan_variants_and_conflicts() {
    let cli = Cli::try_parse_from(["g435ctl", "scan"]).expect("parse");
    match cli.command {
        Command::Scan { off, duration } => {
            assert!(!off);
            assert_eq!(duration, None);
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let cli = Cli::try_parse_from(["g435ctl", "scan", "--off"]).expect("parse");
    match cli.command {
        Command::Scan { off, .. } => assert!(off),
        other => panic!("unexpected command: {other:?}"),
    }

    let cli = Cli::try_parse_from(["g435ctl", "scan", "--duration", "3"]).expect("parse");
    match cli.command {
        Command::Scan { duration, .. } => assert_eq!(duration, Some(3)),
        other => panic!("unexpected command: {other:?}"),
    }

    assert!(Cli::try_parse_from(["g435ctl", "scan", "--off", "--duration", "3"]).is_err());
}

#[test]
fn parses_and_rejects_mac_addresses() {
    let cli = Cli::try_parse_from(["g435ctl", "pair", "aa:bb:cc:dd:ee:ff"]).expect("parse");
    match cli.command {
        Command::Pair { mac } => assert_eq!(mac, [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]),
        other => panic!("unexpected command: {other:?}"),
    }

    assert_eq!(
        parse_mac("AA:0B:cc:dd:ee:ff").expect("mac"),
        [0xaa, 0x0b, 0xcc, 0xdd, 0xee, 0xff]
    );
    assert!(parse_mac("aa:bb:cc:dd:ee").is_err());
    assert!(parse_mac("aa:bb:cc:dd:ee:ff:00").is_err());
    assert!(parse_mac("aa:bb:cc:dd:ee:gg").is_err());
    assert!(parse_mac("aa:bb:cc:dd:ee:fff").is_err());
    assert!(Cli::try_parse_from(["g435ctl", "pair", "not-a-mac"]).is_err());
}

#[test]
fn formats_mac_addresses_canonically() {
    assert_eq!(
        format_mac(&[0x0a, 0x0b, 0, 0x0f, 0xff, 0x10]),
        "0a:0b:00:0f:ff:10"
    );
}

#[test]
fn parses_numbers_and_rejects_bad_input() {
    assert_eq!(parse_number("0").expect("zero"), 0);
    assert_eq!(parse_number("65535").expect("decimal"), 65535);
    assert_eq!(parse_number("0xFF").expect("hex"), 255);
    assert_eq!(parse_number("0X10").expect("hex"), 16);
    assert_eq!(parse_number("  42 ").expect("trimmed"), 42);

    assert!(parse_number("").is_err());
    assert!(parse_number("0x").is_err());
    assert!(parse_number("-1").is_err());
    assert!(parse_number("0xgg").is_err());
    assert!(parse_number("4294967296").is_err());
    assert!(Cli::try_parse_from(["g435ctl", "read", "12x", "4"]).is_err());
}

#[test]
fn requires_a_subcommand() {
    assert!(Cli::try_parse_from(["g435ctl"]).is_err());
    assert!(Cli::try_parse_from(["g435ctl", "read", "16"]).is_err());
}

#[test]
fn parses_v3_typed_commands() {
    let cli = Cli::try_parse_from(["g435ctl", "fs-table"]).expect("fs-table");
    assert!(matches!(cli.command, Command::FsTable));

    let cli = Cli::try_parse_from(["g435ctl", "fs-index", "0x0f"]).expect("fs-index");
    match cli.command {
        Command::FsIndex { index } => assert_eq!(index, 15),
        other => panic!("unexpected command: {other:?}"),
    }

    for (name, id) in [("cust-info", 15u8), ("mp-read", 3), ("nvds-read", 4)] {
        let cli = Cli::try_parse_from(["g435ctl", name, &id.to_string()]).expect(name);
        match cli.command {
            Command::CustInfo { id: got } if name == "cust-info" => assert_eq!(got, id),
            Command::MpRead { id: got } if name == "mp-read" => assert_eq!(got, id),
            Command::NvdsRead { id: got } if name == "nvds-read" => assert_eq!(got, id),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    let cli = Cli::try_parse_from(["g435ctl", "atb", "0"]).expect("atb");
    match cli.command {
        Command::Atb { gpio } => assert_eq!(gpio, 0),
        other => panic!("unexpected command: {other:?}"),
    }

    let cli = Cli::try_parse_from(["g435ctl", "echo", "0x00010203"]).expect("echo");
    match cli.command {
        Command::Echo { data } => assert_eq!(data.0, vec![0, 1, 2, 3]),
        other => panic!("unexpected command: {other:?}"),
    }

    assert!(matches!(
        Cli::try_parse_from(["g435ctl", "lpc"]).map(|cli| cli.command),
        Ok(Command::Lpc)
    ));
    assert!(matches!(
        Cli::try_parse_from(["g435ctl", "quality"]).map(|cli| cli.command),
        Ok(Command::Quality)
    ));
    assert!(matches!(
        Cli::try_parse_from(["g435ctl", "devm"]).map(|cli| cli.command),
        Ok(Command::Devm)
    ));
    assert!(matches!(
        Cli::try_parse_from(["g435ctl", "afh-map"]).map(|cli| cli.command),
        Ok(Command::AfhMap)
    ));
}

#[test]
fn raw_commands_require_exact_unsafe_confirmation() {
    for args in [
        vec!["g435ctl", "raw-ota", "0x31"],
        vec!["g435ctl", "raw-ota", "0x31", "--unsafe"],
        vec!["g435ctl", "raw-ota", "0x31", "--confirm", "I-UNDERSTAND"],
        vec!["g435ctl", "raw-ota", "0x31", "--unsafe", "--confirm", "yes"],
        vec![
            "g435ctl",
            "raw-link",
            "0xe0",
            "--unsafe",
            "--confirm",
            "wrong",
        ],
        vec!["g435ctl", "raw-pa", "0x06", "0000", "--unsafe"],
    ] {
        assert!(
            Cli::try_parse_from(args.clone()).is_err(),
            "unexpected parse success for {args:?}"
        );
    }

    let cli = Cli::try_parse_from([
        "g435ctl",
        "raw-ota",
        "0x31",
        "00",
        "--unsafe",
        "--confirm",
        "I-UNDERSTAND",
    ])
    .expect("raw-ota parse");
    match cli.command {
        Command::RawOta {
            command_id,
            args,
            unsafe_opts,
        } => {
            assert_eq!(command_id, 0x31);
            assert_eq!(args, Some(open_g435::cli::HexBytes(vec![0])));
            assert!(unsafe_opts.unsafe_flag);
            assert_eq!(unsafe_opts.confirm, "I-UNDERSTAND");
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let cli = Cli::try_parse_from([
        "g435ctl",
        "raw-pa",
        "0x06",
        "0031",
        "--unsafe",
        "--confirm",
        "I-UNDERSTAND",
    ])
    .expect("raw-pa parse");
    match cli.command {
        Command::RawPa {
            category,
            payload,
            unsafe_opts,
        } => {
            assert_eq!(category, 6);
            assert_eq!(payload.0, vec![0x00, 0x31]);
            assert!(unsafe_opts.unsafe_flag);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn rejects_bad_v3_arguments() {
    assert!(Cli::try_parse_from(["g435ctl", "fs-index", "256"]).is_err());
    assert!(Cli::try_parse_from(["g435ctl", "echo", "abc"]).is_err());
    assert!(Cli::try_parse_from(["g435ctl", "echo", "zz"]).is_err());
    assert!(Cli::try_parse_from(["g435ctl", "atb", "0x100"]).is_err());
}
