//! `g435ctl` entry point.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use clap::Parser;
use open_g435::cli::{
    format_battery, format_info, format_mode, format_mode_set, format_pair, format_read,
    format_read_written, format_rssi, format_scan, format_status, Cli, Command,
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
