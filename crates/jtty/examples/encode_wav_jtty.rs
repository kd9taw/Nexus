//! Nexus JTTY encode CLI: the transmit half of the JTTY parity lab. DEV-ONLY:
//! an `example`, never shipped, and it keys nothing: it writes a WAV file.
//!
//! # The claim this proves
//! > **Encode here, decode with stock WSJT-X** (`rjtty`, and the WSJT-X GUI's
//! > File > Open).
//!
//! If the stock decoder prints back the text Nexus meant to send, the packing,
//! the CRC and code, the sync and the 4-GFSK waveform are on-air correct with no
//! Nexus decoder in the loop.
//!
//! Each message argument becomes one transmission's frames: text through
//! `jtty::encode` under `--profile`, or, with a leading `@`, structured atoms
//! through `jtty::encode_atoms` (WSJT-X's macro path), as
//! `@kind,subtype,role,value,text;kind,...` with libtempo.h's descriptor fields.
//! With a leading `+`, text is sent as an OPEN segment, without end-of-message
//! (`jtty::encode_segment`): what WSJT-X sends for every segment of a message
//! but its last, and for what its live entry sends while the typing goes on.
//! Each message is rendered by `jtty::gen_wave` on its own, as WSJT-X renders
//! each queued message (`MainWindow::execute_jtty_tones`), and the next one
//! starts where it ends: queued messages carry no spacing frame. `--dt` seconds
//! lead the first frame and one frame (1.888 s) follows the last, the layout
//! of a stock `sjtty` file.
//!
//! Noise (`--snr-db`): sjtty's arithmetic, so a Nexus file and a stock file at
//! the same nominal SNR are the same strength: SNR in 2500 Hz, unit-variance
//! Gaussian noise, signal amplitude `sqrt(2*2500/6000)*10^(snr/20)`, both
//! scaled by 100. Without it the file is scaled to full range, as sjtty does
//! for a clean file.
//!
//! `--tones` also prints each message's channel tones on stdout, one line per
//! message, for a tone-for-tone comparison with stock `sjtty`, which prints
//! the tones it sends: equal tones mean equal packing, CRC, code and sync.
//!
//! stderr gets one `# message N:` line per message (frames and the normalised
//! text) and `# wrote`; a message the modem refuses is an error, exit status 1.
//!
//! Usage:
//!   cargo run -q -p jtty --example encode_wav_jtty -- OUT.wav "CQ KD9TAW EN52" ["+OPEN " "NEXT" ...]
//!       [--profile unknown|field-day|rtty-roundup] [--f0 1500] [--dt 1.0]
//!       [--snr-db N] [--seed N] [--tones]

use jtty::{Profile, FRAME_SAMPLES, SAMPLE_RATE};

fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// `kind,subtype,role,value,text` separated by `;`.
fn parse_atoms(spec: &str) -> Option<Vec<jtty::AtomDescriptor>> {
    spec.split(';')
        .map(|atom| {
            let f: Vec<&str> = atom.splitn(5, ',').collect();
            if f.len() != 5 {
                return None;
            }
            jtty::atom(
                f[0].trim().parse().ok()?,
                f[1].trim().parse().ok()?,
                f[2].trim().parse().ok()?,
                f[3].trim().parse().ok()?,
                f[4].trim(),
            )
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags_with_values = ["--profile", "--f0", "--dt", "--snr-db", "--seed"];
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            !a.starts_with("--") && (*i == 0 || !flags_with_values.contains(&args[i - 1].as_str()))
        })
        .map(|(_, a)| a)
        .collect();
    if positional.len() < 2 {
        eprintln!(
            "usage: encode_wav_jtty OUT.wav \"TEXT\"|@k,s,r,v,text;... [...] [--profile unknown|field-day|rtty-roundup] [--f0 1500] [--dt 1.0] [--snr-db N] [--seed N] [--tones]"
        );
        std::process::exit(2);
    }
    let profile = match arg(&args, "--profile", "unknown".to_string()).as_str() {
        "unknown" => Profile::Unknown,
        "field-day" => Profile::FieldDay,
        "rtty-roundup" => Profile::RttyRoundup,
        other => {
            eprintln!("unknown --profile {other:?}: want unknown|field-day|rtty-roundup");
            std::process::exit(2);
        }
    };
    let f0: f32 = arg(&args, "--f0", 1500.0);
    let dt: f32 = arg(&args, "--dt", 1.0);
    let print_tones = args.iter().any(|a| a == "--tones");
    let snr_db: Option<f32> = args
        .iter()
        .position(|a| a == "--snr-db")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    let lead = (dt.max(0.0) * SAMPLE_RATE).round() as usize;
    let mut buf = vec![0f32; lead];
    for (n, message) in positional[1..].iter().enumerate() {
        let (tones, what) = if let Some(spec) = message.strip_prefix('@') {
            let Some(atoms) = parse_atoms(spec) else {
                eprintln!("bad atoms {spec:?}: want kind,subtype,role,value,text;...");
                std::process::exit(2);
            };
            match jtty::encode_atoms(&atoms) {
                Ok(t) => (t, format!("atoms {spec}")),
                Err(e) => {
                    eprintln!("encode failed: {message:?}: {e:?}");
                    std::process::exit(1);
                }
            }
        } else if let Some(text) = message.strip_prefix('+') {
            match jtty::encode_segment(text, profile, false) {
                Ok(e) => (e.tones, format!("{:?} (open)", e.sent)),
                Err(e) => {
                    eprintln!("encode failed: {message:?}: {e:?}");
                    std::process::exit(1);
                }
            }
        } else {
            match jtty::encode(message, profile) {
                Ok(e) => (e.tones, format!("{:?}", e.sent)),
                Err(e) => {
                    eprintln!("encode failed: {message:?}: {e:?}");
                    std::process::exit(1);
                }
            }
        };
        let wave = jtty::gen_wave(&tones, f0).expect("the encoder made whole frames");
        if print_tones {
            let line: Vec<String> = tones.iter().map(|t| t.to_string()).collect();
            println!("{}", line.join(" "));
        }
        eprintln!(
            "# message {}: {} frame(s) {what}",
            n + 1,
            tones.len() / jtty::FRAME_SYMBOLS
        );
        buf.extend_from_slice(&wave);
    }
    buf.resize(buf.len() + FRAME_SAMPLES, 0.0);

    let pcm: Vec<i16> = match snr_db {
        Some(snr) => {
            let sig = (2.0f32 * 2500.0 / (SAMPLE_RATE / 2.0)).sqrt() * 10f32.powf(0.05 * snr);
            let mut rng = Rng(u64::from(arg(&args, "--seed", 0xC0FFEEu32)));
            buf.iter()
                .map(|&s| {
                    ((sig * s + rng.gauss()) * 100.0)
                        .round()
                        .clamp(-32768.0, 32767.0) as i16
                })
                .collect()
        }
        None => {
            let peak = buf.iter().fold(0f32, |m, &x| m.max(x.abs())).max(1e-9);
            buf.iter()
                .map(|&x| (x * 32766.9 / peak).round() as i16)
                .collect()
        }
    };

    let out = positional[0];
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let write = || -> Result<(), hound::Error> {
        let mut w = hound::WavWriter::create(out, spec)?;
        for &s in &pcm {
            w.write_sample(s)?;
        }
        w.finalize()
    };
    if let Err(e) = write() {
        eprintln!("write {out}: {e}");
        std::process::exit(1);
    }
    eprintln!(
        "# wrote {out} ({} samples, f0 {f0} Hz, dt {dt} s{})",
        pcm.len(),
        snr_db.map_or(String::new(), |s| format!(", SNR {s} dB"))
    );
}

/// Deterministic LCG + Box-Muller, the FT8 harness's RNG
/// (crates/ft8/tests/decode_parity.rs), so a corpus is repeatable while
/// `--seed` varies its realisations.
struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn gauss(&mut self) -> f32 {
        let u1 = (self.next_f64() + 1e-12).min(1.0);
        let u2 = self.next_f64();
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
}
