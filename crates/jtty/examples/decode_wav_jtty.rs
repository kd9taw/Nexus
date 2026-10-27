//! Nexus JTTY decoder CLI: the receive half of the JTTY parity lab. DEV-ONLY:
//! an `example`, never shipped.
//!
//! Decodes a 12 kHz mono 16-bit WAV the way WSJT-X decodes a file it opens
//! (File > Open, `MainWindow::decodeJttyDisk`): one reception on the
//! quarter-frame grid from sample 0, the whole file in one `process` call with
//! no step limit, then `end` as "reception ended" and every update taken. The
//! search band is WSJT-X's: `--center` ± `--ftol` always, plus the fixed
//! 1200-1500 and 1500-1800 Hz channels where they fall inside `--low..--high`.
//! The defaults are stock `rjtty`'s (1500 ± 50 Hz, 200-2800 Hz), so the two
//! decode the same audio over the same band. JTTY has no a-priori decoding, so
//! both are blind by construction.
//!
//! Output (stdout), one line per message, its last update, in order of start:
//!     <start_s> <freq_hz> <snr_db> <complete|expired|ended|growing> <text>
//! `--updates` prints every update instead, oldest first, each led by its
//! message id. `# file:` goes to stderr before each file, as in the other
//! decode CLIs. A file that cannot be decoded is an error line on stderr and
//! exit status 1, so a failed run never reads as "no decodes".
//!
//! `--block N` decodes the file the way the app decodes live audio instead: pushed
//! into a `jtty::Live` N samples at a time, with a pump after each (WSJT-X's
//! `MainWindow::pumpJttyReceive`), then the reception ended. Each message's last
//! update must come out the same either way; `--updates` shows more of a
//! message's life in this mode, because updates are taken more often.
//!
//! The decoder's DSP caches are process-wide, so the lab runs this once per
//! file; each file gets a fresh receiver either way.
//!
//! Usage:
//!   cargo run -q -p jtty --example decode_wav_jtty -- FILE.wav [FILE2.wav ...]
//!       [--center 1500] [--ftol 50] [--low 200] [--high 2800] [--block N] [--updates]

use std::collections::HashMap;

use jtty::{EndReason, Live, Receiver, SearchBand, Terminal, Update};

fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// 12 kHz mono 16-bit PCM, or why not. JTTY runs at 12 kHz only and WSJT-X
/// saves at that rate, so nothing is resampled.
fn read_pcm(path: &str) -> Result<Vec<i16>, String> {
    let reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate != 12_000
        || spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(format!(
            "want 12000 Hz mono 16-bit PCM, got {} Hz, {} channel(s), {}-bit {:?}",
            spec.sample_rate, spec.channels, spec.bits_per_sample, spec.sample_format
        ));
    }
    reader
        .into_samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())
}

fn decode(pcm: &[i16], band: SearchBand) -> Result<Vec<Update>, String> {
    let mut rx = Receiver::new().ok_or("no free receive context")?;
    rx.begin(1, 0, 0);
    rx.process(pcm, 0, pcm.len() as i64, u32::MAX, band)
        .map_err(|e| format!("{e:?}"))?;
    rx.end(EndReason::ReceptionEnded);
    Ok(rx.take_updates())
}

/// The live path: `block` samples pushed at a time, a pump after each.
fn decode_live(pcm: &[i16], band: SearchBand, block: usize) -> Result<Vec<Update>, String> {
    let mut live = Live::new().ok_or("no free receive context")?;
    live.begin(1);
    let mut updates = Vec::new();
    for piece in pcm.chunks(block) {
        live.push(piece);
        updates.extend(live.pump(band).map_err(|e| format!("{e:?}"))?);
    }
    updates.extend(live.end());
    Ok(updates)
}

fn terminal(t: Terminal) -> String {
    match t {
        Terminal::Growing => "growing".into(),
        Terminal::Complete => "complete".into(),
        Terminal::Expired => "expired".into(),
        Terminal::ReceptionEnded => "ended".into(),
        Terminal::Unknown(raw) => format!("terminal{raw}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags_with_values = ["--center", "--ftol", "--low", "--high", "--block"];
    let files: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            !a.starts_with("--") && (*i == 0 || !flags_with_values.contains(&args[i - 1].as_str()))
        })
        .map(|(_, a)| a)
        .collect();
    if files.is_empty() {
        eprintln!(
            "usage: decode_wav_jtty FILE.wav [...] [--center 1500] [--ftol 50] [--low 200] [--high 2800] [--block N] [--updates]"
        );
        std::process::exit(2);
    }
    let band = SearchBand {
        low_hz: arg(&args, "--low", 200),
        high_hz: arg(&args, "--high", 2800),
        center_hz: arg(&args, "--center", 1500.0),
        tolerance_hz: arg(&args, "--ftol", 50.0),
    };
    let every_update = args.iter().any(|a| a == "--updates");
    let block: usize = arg(&args, "--block", 0);
    eprintln!(
        "# mode: JTTY (blind, {:.0} ± {:.0} Hz, {}-{} Hz{})",
        band.center_hz,
        band.tolerance_hz,
        band.low_hz,
        band.high_hz,
        if block > 0 {
            format!(", live in {block}-sample blocks")
        } else {
            String::new()
        }
    );

    let mut failed = false;
    for path in files {
        eprintln!("# file: {path}");
        let updates = match read_pcm(path).and_then(|pcm| {
            if block > 0 {
                decode_live(&pcm, band, block)
            } else {
                decode(&pcm, band)
            }
        }) {
            Ok(u) => u,
            Err(e) => {
                eprintln!("# {path}: ERROR {e}");
                failed = true;
                continue;
            }
        };
        if every_update {
            for u in &updates {
                println!(
                    "{} {:.3} {:.3} {:.1} {} {} {}",
                    u.message_id,
                    u.start_s,
                    u.latest_s,
                    u.freq_hz,
                    u.snr_db,
                    terminal(u.terminal),
                    u.text
                );
            }
        }
        // Each message's last update is where it stands.
        let mut last: HashMap<i64, &Update> = HashMap::new();
        for u in &updates {
            last.insert(u.message_id, u);
        }
        let mut messages: Vec<&Update> = last.into_values().collect();
        messages.sort_by(|a, b| {
            a.start_s
                .total_cmp(&b.start_s)
                .then(a.freq_hz.total_cmp(&b.freq_hz))
        });
        if !every_update {
            for u in &messages {
                println!(
                    "{:.3} {:.1} {} {} {}",
                    u.start_s,
                    u.freq_hz,
                    u.snr_db,
                    terminal(u.terminal),
                    u.text
                );
            }
        }
        eprintln!("# {path}: {} message(s)", messages.len());
    }
    if failed {
        std::process::exit(1);
    }
}
