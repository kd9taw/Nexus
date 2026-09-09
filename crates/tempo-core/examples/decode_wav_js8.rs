//! Nexus JS8 decoder CLI — the "parity lab" reference for measuring the
//! native JS8 decoder against stock JS8Call (`js8 -8 -b <A|B|C|E> -d 3`) on
//! identical audio. DEV-ONLY: an `example`, never shipped.
//!
//! Runs EXACTLY the path B5's boundary/multi-speed passes run:
//!   i16 period --> js8::phy::decode(&frame, speed, &DecodeParams{depth, ..stock()})
//! (stock = nfa 100, nfb 4000, subtract_passes 2). No a-priori context exists
//! in JS8 — the comparison is blind at both ends, matching how `js8` is run.
//!
//! Input: 16-bit PCM mono WAV at 12 kHz (JS8Call's media/tests and save/ files
//! are 12 kHz; anything else is refused — there is no 48 k front-end here,
//! the live capture path resamples before the ring). Audio is sliced into
//! `period_s` cycles (30/15/10/6 s) from the file start; a short tail is
//! zero-padded like the front-padded ring.
//!
//! Output (stdout), one line per decode:
//!     <cycle_utc_offset_s> <freq_hz> <snr_db> <dt_s> <sixbit12> <i3>
//! (`<sixbit12>` is the ALL.TXT 12-char frame column, `<i3>` the First|Last<<1|
//! Data<<2 value — the two columns ab_js8.py compares). B4 appends the
//! JS8Call-rendered text as a seventh column via `Frame::render`.
//! stderr: `# file:` markers and per-file counts — never mix the streams.
//!
//! Usage:
//!   cargo run -q -p tempo-core --example decode_wav_js8 -- --speed A [-d 3] FILE.wav [...]
//! Speed letters are JS8Call's submode letters: A Normal, B Fast, C Turbo,
//! E Slow (`Speed::letter`); the names slow|normal|fast|turbo are accepted too.

use js8::phy::{decode, DecodeParams, Speed};
use js8::proto::alphabet::sixbit_to_string;
use tempo_core::wavfile::read_wav_i16;

const MODEM_RATE: u32 = 12_000;

fn parse_speed(s: &str) -> Option<Speed> {
    match s.to_ascii_uppercase().as_str() {
        "A" | "NORMAL" => Some(Speed::Normal),
        "B" | "FAST" => Some(Speed::Fast),
        "C" | "TURBO" => Some(Speed::Turbo),
        "E" | "SLOW" => Some(Speed::Slow),
        _ => None,
    }
}

fn decode_file(path: &str, speed: Speed, depth: u8) -> std::io::Result<usize> {
    let (samples, sr) = read_wav_i16(path)?;
    if sr != MODEM_RATE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("JS8 expects {MODEM_RATE} Hz audio, got {sr} Hz"),
        ));
    }
    let cycle = speed.period_s() as usize * MODEM_RATE as usize;
    let n_cycles = samples.len().div_ceil(cycle).max(1);
    let params = DecodeParams {
        depth,
        ..DecodeParams::stock()
    };
    let mut total = 0usize;
    for c in 0..n_cycles {
        let start = c * cycle;
        let mut frame = vec![0i16; cycle];
        if start < samples.len() {
            let end = (start + cycle).min(samples.len());
            frame[..end - start].copy_from_slice(&samples[start..end]);
        }
        let mut decs = decode(&frame, speed, &params);
        decs.sort_by(|a, b| a.freq_hz.total_cmp(&b.freq_hz));
        for d in &decs {
            // cycle_utc_offset(s)  freq(Hz)  snr(dB)  dt(s)  sixbit12  i3
            println!(
                "{} {:.1} {} {:+.2} {} {}",
                c * speed.period_s() as usize,
                d.freq_hz,
                d.snr_db,
                d.dt_s,
                sixbit_to_string(&d.word.payload72().chars12()),
                d.word.i3().to_u8()
            );
            total += 1;
        }
    }
    Ok(total)
}

fn usage() -> ! {
    eprintln!("usage: decode_wav_js8 --speed A|B|C|E [-d 1..3] FILE.wav [FILE2.wav ...]");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut speed: Option<Speed> = None;
    let mut depth: u8 = 3;
    let mut files: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--speed" | "-b" => {
                speed = it.next().and_then(|s| parse_speed(s));
                if speed.is_none() {
                    eprintln!("--speed expects A|B|C|E or slow|normal|fast|turbo");
                    usage();
                }
            }
            "-d" | "--depth" => {
                depth = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                if !(1..=3).contains(&depth) {
                    eprintln!("-d expects 1..3");
                    usage();
                }
            }
            other if other.starts_with('-') => {
                eprintln!("unknown flag {other}");
                usage();
            }
            _ => files.push(a.clone()),
        }
    }
    let Some(speed) = speed else { usage() };
    if files.is_empty() {
        usage();
    }
    eprintln!("# speed: {} ({:?})  depth: {depth}", speed.letter(), speed);
    for path in &files {
        eprintln!("# file: {path}");
        match decode_file(path, speed, depth) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}
