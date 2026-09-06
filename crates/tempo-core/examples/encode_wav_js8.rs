//! Nexus JS8 TRANSMIT CLI — the encode arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! Writes a period-length 12 kHz WAV containing ONE JS8 frame, produced by exactly the
//! path the radio will use in B7 (`TxWaveform::Js8` → `js8::phy::encode_word` →
//! `js8::phy::modulate`): the start-delay silence, then 79 symbols of continuous-phase
//! rectangular 8-FSK, then silence to the period boundary.
//!
//! # The claim this proves
//! > **Encode here → decode with the stock JS8Call `js8` CLI.**
//!
//! If JS8Call's own decoder prints back the 12-char field and the i3 digit we put in, our
//! LDPC(174,87) generator/colorder, CRC-12, i3 placement, tone map (no Gray) and Costas
//! kernels are on-air correct — with no Nexus decoder in the loop, so a shared bug in our
//! encode/decode pair cannot hide. `crates/js8/tests/encode_parity.rs` automates this.
//!
//! ⭐ The input is the LAB TEXT FORM, not operator text: `"<12 sixbit chars> <i3>"` — the two
//! columns JS8Call's ALL.TXT records per decode (alphabet `0-9A-Za-z-+`, i3 = First(1) |
//! Last(2) | Data(4)). Text → frames (callsigns, commands, JSC) is proto's job (B4) and is
//! deliberately not in the loop here: this CLI tests the PHYSICAL layer only. The word form is
//! parsed by `Word87::from_lab` (phy::frame, plan interface addendum) — B2/B3/B5 share it, so
//! there is no private copy of the lab-text parser here.
//!
//! ⭐ WHY the wave goes through `js8::phy` directly and not `Mode::encode`/`gen_wave`:
//! `Js8Mode` lands in B5. When it does, `Js8Mode::encode` parses this same lab form
//! (interfaces.md §2) and this example may switch to the `Mode` path like its siblings.
//! `encode_word` takes the `Speed` because the Costas kernel is Normal-only ORIGINAL vs
//! MODIFIED elsewhere (modulate.rs) — the speed is not optional.
//!
//! SNR convention (when `--snr-db` is given): WSJT-X's 2500 Hz reference bandwidth, in the
//! FT8 harness's arithmetic (crates/ft8/tests/decode_parity.rs:50-66) — unit-variance
//! Gaussian noise scaled ×100 and a signal amplitude `sqrt(2·2500/6000)·10^(snr/20)`, so a
//! −28 dB Slow ladder point never clips the way "scale the noise up to the signal" would.
//!
//! Usage:
//!   cargo run -q -p tempo-core --example encode_wav_js8 -- OUT.wav "KD9TAWabcxyz 3" \
//!       --speed normal [--f0 1500] [--snr-db N] [--seed N]
//!   --speed accepts slow|normal|fast|turbo or the ALL.TXT letter E|A|B|C.
//!
//! Then, from the parity lab (cwd = a scratch dir; it drops jt9_wisdom.dat + timer.out):
//!   ~/work/twowayfd/paritylab/js8/stock/js8 -8 -b A -d 3 OUT.wav

use js8::phy::{encode_word, modulate, Speed, Word87};
use tempo_core::wavfile::write_wav_i16;

const SAMPLE_RATE: f32 = 12_000.0;

fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// `slow|normal|fast|turbo` or the ALL.TXT letter `E|A|B|C` (either case).
fn parse_speed(s: &str) -> Option<Speed> {
    match s.to_ascii_lowercase().as_str() {
        "slow" => Some(Speed::Slow),
        "normal" => Some(Speed::Normal),
        "fast" => Some(Speed::Fast),
        "turbo" => Some(Speed::Turbo),
        other if other.len() == 1 => Speed::from_letter(other.to_ascii_uppercase().chars().next()?),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--")))
        .map(|(_, a)| a)
        .collect();
    if positional.len() < 2 {
        eprintln!(
            "usage: encode_wav_js8 OUT.wav \"<12 sixbit> <i3>\" --speed slow|normal|fast|turbo [--f0 1500] [--snr-db N] [--seed N]"
        );
        std::process::exit(2);
    }
    let (out_path, lab) = (positional[0], positional[1].as_str());
    let speed_arg: String = arg(&args, "--speed", "normal".to_string());
    let Some(speed) = parse_speed(&speed_arg) else {
        eprintln!("unknown --speed {speed_arg:?}: want slow|normal|fast|turbo or E|A|B|C");
        std::process::exit(2);
    };
    let f0: f32 = arg(&args, "--f0", 1500.0);
    let snr_db: Option<f32> = args
        .iter()
        .position(|a| a == "--snr-db")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    let Some(word) = Word87::from_lab(lab) else {
        eprintln!("encode failed: {lab:?} is not \"<12 chars of 0-9A-Za-z-+> <i3 0..7>\"");
        std::process::exit(1);
    };
    let tones = encode_word(&word, speed);
    let wave = modulate(&tones, speed, f0, SAMPLE_RATE);

    // Period-length buffer; the wave is already slot-positioned (leading delay silence).
    let slot_len = speed.period_s() as usize * SAMPLE_RATE as usize;
    let mut buf = vec![0f32; slot_len];
    let n = wave.len().min(slot_len);
    buf[..n].copy_from_slice(&wave[..n]);

    let pcm: Vec<i16> = match snr_db {
        Some(snr) => {
            let sig = (2.0f32 * 2500.0 / (SAMPLE_RATE / 2.0)).sqrt() * 10f32.powf(0.05 * snr);
            let mut rng = Rng(u64::from(arg(&args, "--seed", 0xC0FFEEu32)));
            buf.iter()
                .map(|&s| (((sig * s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0)) as i16)
                .collect()
        }
        None => {
            let peak = buf.iter().fold(0f32, |m, &x| m.max(x.abs())).max(1e-9);
            buf.iter().map(|&x| (x * 8000.0 / peak) as i16).collect()
        }
    };
    write_wav_i16(out_path, &pcm, SAMPLE_RATE as u32).expect("write wav");

    eprintln!(
        "# JS8 {speed:?} letter={} f0={f0} Hz spacing={:.4} Hz over={:.2}s of {}s slot",
        speed.letter(),
        speed.tone_spacing_hz(),
        speed.slot_fit_s(),
        speed.period_s(),
    );
    eprintln!("# wrote {out_path} ({} samples)", pcm.len());
}

/// Deterministic LCG + Box–Muller (the FT8 harness's RNG, crates/ft8/tests/decode_parity.rs:31-47)
/// so a ladder is repeatable while `--seed` varies its realisations.
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
