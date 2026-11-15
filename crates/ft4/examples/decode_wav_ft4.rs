//! FT4 decoder bench CLI: decodes 12 kHz 16-bit mono WAV files with one of Nexus's FT4 decoders and prints
//! every decode, so Nexus can be compared with stock WSJT-X (`jt9 -5 -d 3 file.wav`) on the same
//! audio. DEV-ONLY: an `example`, never shipped.
//!
//! One frame per file: the first [`NMAX`] samples (zero-padded when shorter), as the engine hands
//! the decoder a slot. Files are decoded in the order given, in one process, so any state the
//! decoder carries from one period to the next (none for the standard decoder) carries across
//! files as it would across the periods of a live session (the JTDX-style decoder's running
//! average DT does).
//!
//! Output (stdout), one line per decode:
//!     <file> <freq_hz> <snr_db> <dt_s> <nap>[u] <message>
//! where a `u` after the AP type marks a decode the JTDX-style decoder reports `unchecked`.
//! and one line per file on stderr: `#time <file> <ms> <ndecodes>`.
//!
//! Usage:
//!     cargo run -q -p ft4 --example decode_wav_ft4 -- [--decoder standard|jtdx] [--depth N] [--nfa HZ] [--nfb HZ]
//!         [--mycall C] [--hiscall C] [--progress N] [--nfqso HZ] [--cq-only] [--swl] [--db DIR] FILE.wav [...]
//!
//! `--db DIR` loads the callsign database (`DIR/ALLCALL7.TXT`) the JTDX-style decoder's checks read,
//! as the engine does at start-up; without it those checks pass every call.

use ft4::{decode_frame, decode_frame_jtdx, load_callsign_db, NMAX};

fn read_wav_i16(path: &str) -> Vec<i16> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let sz = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = i + 8;
        if &b[i..i + 4] == b"data" {
            let end = (body + sz).min(b.len());
            return b[body..end]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
        }
        i = body + sz + (sz & 1);
    }
    panic!("no data chunk in {path}");
}

fn main() {
    let mut depth = 3;
    let (mut nfa, mut nfb) = (200, 2900);
    let (mut mycall, mut hiscall) = (String::new(), String::new());
    let (mut progress, mut nfqso) = (0, 0);
    let mut cq_only = false;
    let mut jtdx = false;
    let mut swl = false;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--depth" => depth = val().parse().expect("--depth N"),
            "--nfa" => nfa = val().parse().expect("--nfa HZ"),
            "--nfb" => nfb = val().parse().expect("--nfb HZ"),
            "--mycall" => mycall = val(),
            "--hiscall" => hiscall = val(),
            "--progress" => progress = val().parse().expect("--progress N"),
            "--nfqso" => nfqso = val().parse().expect("--nfqso HZ"),
            "--cq-only" => cq_only = true,
            "--swl" => swl = true,
            "--db" => {
                let dir = val();
                let ok = load_callsign_db(std::path::Path::new(&dir));
                eprintln!("#db {dir} valid={ok}");
            }
            "--decoder" => {
                jtdx = match val().as_str() {
                    "jtdx" => true,
                    "standard" => false,
                    d => panic!("--decoder standard|jtdx, not {d}"),
                }
            }
            _ => files.push(a),
        }
    }
    for f in &files {
        let mut iwave = read_wav_i16(f);
        iwave.resize(NMAX, 0);
        let t0 = std::time::Instant::now();
        let decs = if jtdx {
            decode_frame_jtdx(
                &iwave, nfa, nfb, depth, &mycall, &hiscall, progress, nfqso, cq_only, swl,
            )
        } else {
            decode_frame(
                &iwave, nfa, nfb, depth, &mycall, &hiscall, progress, nfqso, cq_only,
            )
        };
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        let name = std::path::Path::new(f)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| f.clone());
        for d in &decs {
            println!(
                "{name} {:.0} {} {:.2} {}{} {}",
                d.freq,
                d.snr,
                d.dt,
                d.nap,
                if d.unchecked { "u" } else { "" },
                d.message
            );
        }
        eprintln!("#time {name} {ms:.1} {}", decs.len());
    }
}
