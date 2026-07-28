//! Nexus-as-configured WSPR decoder CLI — the WSPR arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! ⭐ THE REFERENCE HERE IS NOT jt9. WSJT-X has no library-shaped WSPR decoder —
//! it runs the `wsprd` EXECUTABLE as a subprocess — so stock `wsprd` itself is
//! the reference. That is also precisely what Nexus had to convert into a
//! callable core, which makes this the only check that the conversion preserved
//! the decoder rather than merely compiling.
//!
//! Usage:  decode_wav_wspr [--dial MHZ] [--quick] [--passes N] FILE.wav [...]
//!
//! Output (stdout), one line per decode, matching stock wsprd's own column order
//! so the two can be diffed directly:
//!     <snr> <dt> <freq_mhz> <drift> <message>

use modes::ModeKind;
use tempo_core::channel::capture_to_i16;
use tempo_core::wavfile::read_wav_i16;

const MODEM_RATE: u32 = 12000;

fn resample_linear(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let n_out = ((x.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let frac = (pos - i0 as f64) as f32;
        let a = x[i0.min(x.len() - 1)];
        let b = x[(i0 + 1).min(x.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

fn decode_file(path: &str, dial: f64, quick: bool, passes: i32) -> Result<usize, String> {
    let (samples, sr) = read_wav_i16(path).map_err(|e| e.to_string())?;
    let f: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let f12 = if sr == MODEM_RATE {
        f
    } else {
        resample_linear(&f, sr, MODEM_RATE)
    };
    let iwave = capture_to_i16(&f12);

    // WSPR takes the whole interval at once — no slot loop. A short buffer is
    // zero-padded by the decoder rather than refused.
    let decs = wspr::decode_frame(&iwave, dial, quick, passes, true, false, false);
    for d in &decs {
        println!(
            "{:3} {:5.1} {:12.6} {:3} {}",
            d.snr.round() as i32,
            d.dt,
            d.freq_mhz,
            d.drift.round() as i32,
            d.message.trim()
        );
    }
    Ok(decs.len())
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // 14.0956 MHz is the 20 m WSPR dial — the busiest band, and what wsprsim
    // assumes if you do not say otherwise.
    let mut dial: f64 = 14.0956;
    let mut quick = false;
    // 3 = upstream's default; the third pass is the weak-signal one.
    let mut passes: i32 = 3;
    let mut files: Vec<String> = Vec::new();

    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dial" => match it.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) => dial = v,
                None => {
                    eprintln!("--dial needs a frequency in MHz");
                    std::process::exit(2);
                }
            },
            "--passes" => match it.next().and_then(|v| v.parse::<i32>().ok()) {
                Some(v) => passes = v,
                None => {
                    eprintln!("--passes needs a count");
                    std::process::exit(2);
                }
            },
            "--quick" => quick = true,
            other => files.push(other.to_string()),
        }
    }

    if files.is_empty() {
        eprintln!("usage: decode_wav_wspr [--dial MHZ] [--quick] [--passes N] FILE.wav [...]");
        std::process::exit(2);
    }

    eprintln!(
        "# mode: {} (dial {dial} MHz, {} passes{}, {} samples/frame)",
        ModeKind::Wspr.as_str(),
        passes,
        if quick { ", quick" } else { "" },
        ModeKind::Wspr.frame_samples()
    );
    for path in &files {
        eprintln!("# file: {path}");
        match decode_file(path, dial, quick, passes) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}
