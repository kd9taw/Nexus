//! `flexrecord`: record a session from a FlexRadio to attach to a Nexus tester report.
//!
//! ```text
//! cargo run -p tempo-flexsim --bin flexrecord -- <radio-ip> [--seconds N] [--out FILE]
//! ```
//!
//! Observe-only. It connects to the radio's API port as an ordinary client (not a GUI client),
//! subscribes to status, asks for the radio's info, slice list and meter list, registers a UDP
//! port for the meter stream and pings once a second. It can send nothing else: the command list
//! is printed when it starts, and [`tempo_flexsim::record`] explains how that is enforced. It
//! never transmits and never changes a setting.
//!
//! IP and MAC addresses, serial numbers, client ids, host and station names and GPS position are
//! replaced before anything is written. The file stays on this computer and is never sent
//! anywhere: read it, then attach it to your report if you choose. It refuses to overwrite an
//! existing file.
//!
//! Exit 0 = the session was recorded. Exit 1 = it was not, with the reason.

use std::fs::OpenOptions;
use std::io::BufWriter;
use std::net::{IpAddr, SocketAddr};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempo_flexsim::record::{self, Options, ALLOWED_VERBS, API_PORT};

const USAGE: &str = "usage: flexrecord <radio-ip> [--seconds N] [--out FILE]

Records a FlexRadio session for a Nexus tester report. Observe-only: it never transmits and
never changes a setting. Addresses and serial numbers are replaced before anything is written.
  --seconds N   how long to observe after connecting (default 60, at most 3600)
  --out FILE    where to write the session (default flex-session-<time>.flexsession);
                an existing file is never overwritten";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("flexrecord: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let mut ip: Option<IpAddr> = None;
    let mut seconds: u64 = 60;
    let mut out: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--seconds" => {
                seconds = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .filter(|s| (1..=3600).contains(s))
                    .ok_or("--seconds needs a number from 1 to 3600")?;
            }
            "--out" => out = Some(it.next().ok_or("--out needs a file name")?.clone()),
            other if ip.is_none() && !other.starts_with('-') => {
                ip = Some(
                    other
                        .parse()
                        .map_err(|_| format!("{other:?} is not an IP address\n\n{USAGE}"))?,
                );
            }
            other => return Err(format!("unexpected argument {other:?}\n\n{USAGE}")),
        }
    }
    let ip = ip.ok_or(USAGE)?;
    let radio = SocketAddr::new(ip, API_PORT);
    let path = out.unwrap_or_else(|| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        format!("flex-session-{now}.flexsession")
    });
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("cannot create {path}: {e}"))?;

    println!("Recording {radio} for {seconds} s into {path}.");
    println!("Observe-only: the only commands it can send are {ALLOWED_VERBS:?}.");
    let options = Options {
        duration: Duration::from_secs(seconds),
        ..Options::default()
    };
    let summary = record::record(radio, radio, &options, BufWriter::new(file))
        .map_err(|e| format!("recording failed: {e} (anything recorded so far is in {path})"))?;

    println!(
        "Recorded {} lines and {} meter or display packets; sent {} observe-only commands.",
        summary.lines, summary.packets, summary.commands
    );
    if summary.packets_skipped > 0 {
        println!(
            "Left out {} UDP packets that were not meter or display data.",
            summary.packets_skipped
        );
    }
    if summary.radio_closed {
        println!("The radio closed the connection before the time was up.");
    }
    if summary.scrubbed.is_empty() {
        println!("Nothing needed replacing.");
    } else {
        let replaced: Vec<String> = summary
            .scrubbed
            .iter()
            .map(|(kind, n)| format!("{n} {}", kind.label()))
            .collect();
        println!("Replaced: {}.", replaced.join(", "));
    }
    println!("Kept as recorded: the callsign and the radio's nickname.");
    println!(
        "Nothing was sent anywhere. Read {path}, then attach it to your report if you choose."
    );
    Ok(())
}
