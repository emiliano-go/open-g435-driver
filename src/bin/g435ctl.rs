//! `g435ctl` entry point.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use clap::Parser;
use open_g435::cli::{
    format_afh_map, format_atb, format_battery, format_blob, format_devm, format_echo,
    format_fs_index, format_fs_table, format_info, format_lpc, format_mode, format_mode_set,
    format_pair, format_quality, format_raw, format_read, format_read_written, format_rssi,
    format_scan, format_status, Cli, Command,
};
use open_g435::{Error, G435};
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

fn run(cli: Cli) -> Result<(), Error> {
    let json = cli.json;
    let mut device = G435::open()?;

    match cli.command {
        Command::Battery => {
            let millivolts = device.battery()?;
            println!("{}", format_battery(millivolts, json));
        }

        Command::Status => {
            let status = device.link_status()?;
            println!("{}", format_status(status.bits, json));
        }

        Command::Mode { set } => match set {
            Some(mode) => {
                device.set_mode(mode)?;
                println!("{}", format_mode_set(mode, json));
            }
            None => {
                let mode = device.mode()?;
                println!("{}", format_mode(mode, json));
            }
        },

        Command::Rssi => {
            let rssi = device.rssi()?;
            println!("{}", format_rssi(&rssi, json));
        }

        Command::Scan { off, duration } => {
            if let Some(seconds) = duration {
                device.set_inquiry_scan(true)?;
                sleep(Duration::from_secs(seconds));
                device.set_inquiry_scan(false)?;
                println!("{}", format_scan(false, json));
            } else if off {
                device.set_inquiry_scan(false)?;
                println!("{}", format_scan(false, json));
            } else {
                device.set_inquiry_scan(true)?;
                println!("{}", format_scan(true, json));
            }
        }

        Command::Pair { mac } => {
            device.create_connect(mac)?;
            println!("{}", format_pair(&mac, json));
        }

        Command::Info => {
            let image = device.image_version()?;
            let grs = device.grs_status()?;
            let battery = device.battery()?;
            println!("{}", format_info(&image, &grs, battery, json));
        }

        Command::FsTable => {
            let data = device.fs_table()?;
            println!("{}", format_fs_table(&data, json));
        }

        Command::FsIndex { index } => {
            let entry = device.fs_index(index)?;
            let mut data = entry.offset.to_le_bytes().to_vec();
            data.extend_from_slice(&entry.size.to_le_bytes());
            println!("{}", format_fs_index(entry.offset, entry.size, &data, json));
        }

        Command::CustInfo { id } => {
            let data = device.cust_info(id)?;
            println!("{}", format_blob("cust-info", id, &data, json));
        }

        Command::MpRead { id } => {
            let data = device.mp_read(id)?;
            println!("{}", format_blob("mp-read", id, &data, json));
        }

        Command::NvdsRead { id } => {
            let data = device.nvds_read(id)?;
            println!("{}", format_blob("nvds-read", id, &data, json));
        }

        Command::Atb { gpio } => {
            let value = device.atb(gpio)?;
            println!("{}", format_atb(gpio, value, json));
        }

        Command::Echo { data } => {
            let reply = device.echo(&data.0)?;
            println!("{}", format_echo(&reply, json));
        }

        Command::Lpc => {
            let lpc = device.lpc()?;
            println!("{}", format_lpc(&lpc, json));
        }

        Command::Quality => {
            let quality = device.quality()?;
            println!("{}", format_quality(&quality, json));
        }

        Command::Devm => {
            let devm = device.devm()?;
            println!("{}", format_devm(&devm, json));
        }

        Command::AfhMap => {
            let afh = device.afh_map()?;
            println!("{}", format_afh_map(&afh, json));
        }

        Command::RawOta {
            command_id,
            args,
            unsafe_opts: _,
        } => {
            eprintln!("warning: unsafe raw OTA command; no safety checks applied");
            let reply = device.raw_ota(
                command_id,
                args.as_ref().map(|args| args.0.as_slice()).unwrap_or(&[]),
            )?;
            println!(
                "{}",
                format_raw(Some(reply.status), &reply.data, &reply.frame, None, json)
            );
        }

        Command::RawLink {
            command_id,
            args,
            unsafe_opts: _,
        } => {
            eprintln!("warning: unsafe raw link command; no safety checks applied");
            let reply = device.raw_link(
                command_id,
                args.as_ref().map(|args| args.0.as_slice()).unwrap_or(&[]),
            )?;
            println!(
                "{}",
                format_raw(None, &reply.data, &reply.frame, None, json)
            );
        }

        Command::RawPa {
            category,
            payload,
            unsafe_opts: _,
        } => {
            eprintln!("warning: unsafe raw PA command; no safety checks applied");
            let reply = device.raw_pa(category, &payload.0)?;
            println!(
                "{}",
                format_raw(
                    reply.status,
                    &reply.data,
                    &reply.frame,
                    Some((reply.category, reply.seq)),
                    json,
                )
            );
        }

        Command::Read { addr, len, output } => {
            let data = device.read_storage(addr, len)?;
            match output {
                Some(path) => {
                    std::fs::write(&path, &data)?;
                    println!("{}", format_read_written(&path, &data));
                }
                None => println!("{}", format_read(addr, len, &data, json)),
            }
        }
    }

    Ok(())
}
