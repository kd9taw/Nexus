//! Encode a known signal through the REAL Remote receive-audio path and report what
//! came out — frame sizes, measured bit rate, and the mode/bandwidth libopus
//! actually chose, read off the packets rather than off our own settings.
//!
//! ```text
//! cargo run -p tempo-audio --example remote_audio_frames
//! cargo run -p tempo-audio --example remote_audio_frames -- offair.wav
//! cargo run -p tempo-audio --example remote_audio_frames -- offair.wav --out coded.wav
//! ```
//!
//! **Why it exists.** The CELT-vs-SILK choice this encoder rests on was measured on
//! *synthetic white noise* against a system libopus, not on real receiver audio and
//! not on the version we pin. Real receiver audio has QRN crashes, carriers, a
//! sloping spectrum and AGC action. So: record a minute off air across a range of CW
//! speeds including one near the noise floor, run it through here, and compare the
//! `--out` file against the original. A synthetic harness is a proxy for the band,
//! and the comparable C harness that produced the original numbers carries both an
//! uncoded control and a must-trip low-bit-rate control for exactly that reason —
//! keep them when you re-run it, because a reassuring result with no control proves
//! nothing.
//!
//! Everything here drives `receive_audio` + `receive_encode` as the station would:
//! publications at the 20 ms RX DSP tick, one reader, one poll per tick. It opens no
//! device, touches no radio, and transmits nothing.

use std::f32::consts::PI;

use tempo_audio::receive_encode::{encode_offline, EncodedFrame, ENCODE_RATE_HZ, FRAME_MS};

const DEFAULT_RATE: u32 = 48_000;
const DEFAULT_SECONDS: usize = 10;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input: Option<String> = None;
    let mut out: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "-h" | "--help" => {
                eprintln!(
                    "usage: remote_audio_frames [input.wav] [--out coded.wav]\n\
                     with no input, a keyed 700 Hz CW tone in noise is synthesized"
                );
                return;
            }
            other => input = Some(other.to_owned()),
        }
        i += 1;
    }

    let (rate, samples, what) = match &input {
        Some(path) => {
            let (rate, samples) = read_wav(path);
            (rate, samples, format!("{path} ({rate} Hz)"))
        }
        None => (
            DEFAULT_RATE,
            synth_cw(DEFAULT_RATE, DEFAULT_SECONDS),
            format!("synthetic keyed 700 Hz CW in noise ({DEFAULT_RATE} Hz)"),
        ),
    };
    println!("input   : {what}, {:.2} s", secs(samples.len(), rate));

    let frames = encode_offline(rate, &samples).unwrap_or_else(|err| {
        eprintln!("encoder ended: {err}");
        std::process::exit(1);
    });
    if frames.is_empty() {
        eprintln!("no frames — the input is shorter than one 20 ms tick");
        std::process::exit(1);
    }
    report(&frames);
    if let Some(path) = out {
        let pcm = decode(&frames);
        write_wav(&path, ENCODE_RATE_HZ, &pcm);
        println!(
            "wrote   : {path} — {} Hz mono, {:.2} s, for the listening comparison",
            ENCODE_RATE_HZ,
            secs(pcm.len(), ENCODE_RATE_HZ)
        );
    }
}

fn report(frames: &[EncodedFrame]) {
    let sizes: Vec<usize> = frames.iter().map(|f| f.packet.len()).collect();
    let total: usize = sizes.iter().sum();
    let secs = frames.len() as f64 * FRAME_MS as f64 / 1000.0;
    let mut sorted = sizes.clone();
    sorted.sort_unstable();

    println!("frames  : {} × {} ms = {secs:.2} s", frames.len(), FRAME_MS);
    println!(
        "packets : min {} B, median {} B, mean {:.1} B, max {} B",
        sorted[0],
        sorted[sorted.len() / 2],
        total as f64 / sizes.len() as f64,
        sorted[sorted.len() - 1]
    );
    println!(
        "bit rate: {:.0} bit/s payload ({total} B total)",
        total as f64 * 8.0 / secs
    );

    // Read the codec's real choice off the wire, not off our configuration. RFC 6716
    // §3.1: config 16..=31 is CELT-only; within it (config-16)/4 is the bandwidth and
    // (config-16)%4 the frame size.
    let mut modes = std::collections::BTreeMap::new();
    for frame in frames {
        *modes.entry(frame.packet[0] >> 3).or_insert(0usize) += 1;
    }
    for (config, count) in &modes {
        println!(
            "toc     : config {config:>2} ({}) × {count}",
            describe(*config)
        );
    }
    if modes.keys().any(|c| *c < 16) {
        println!("⚠️  a non-CELT packet appeared — the configuration has regressed");
    }

    // Capture marks must advance one frame at a time while the feed is contiguous.
    let holes = frames
        .windows(2)
        .filter(|p| p[1].capture_ms.saturating_sub(p[0].capture_ms) > FRAME_MS + 1)
        .count();
    println!(
        "marks   : seq 0..{}, {holes} gap(s) in the capture mark",
        frames.len() - 1
    );
}

fn describe(config: u8) -> String {
    if config < 12 {
        return format!(
            "SILK-only, {} ms",
            [10, 20, 40, 60][(config as usize % 4).min(3)]
        );
    }
    if config < 16 {
        return "hybrid".to_owned();
    }
    let band =
        ["narrowband", "wideband", "superwideband", "fullband"][((config - 16) / 4) as usize];
    let ms = ["2.5", "5", "10", "20"][((config - 16) % 4) as usize];
    format!("CELT-only, {band}, {ms} ms")
}

fn decode(frames: &[EncodedFrame]) -> Vec<f32> {
    let mut decoder =
        opus::Decoder::new(ENCODE_RATE_HZ, opus::Channels::Mono).expect("opus decoder");
    let frame_samples = (ENCODE_RATE_HZ as usize) * (FRAME_MS as usize) / 1000;
    let mut pcm = Vec::with_capacity(frames.len() * frame_samples);
    let mut buf = vec![0.0f32; frame_samples];
    for frame in frames {
        let n = decoder
            .decode_float(&frame.packet, &mut buf, false)
            .expect("decode");
        pcm.extend_from_slice(&buf[..n]);
    }
    pcm
}

/// A keyed 700 Hz tone at roughly 20 WPM on white noise — the shape the CW fidelity
/// argument is about. Deterministic, so two runs are comparable.
fn synth_cw(rate: u32, seconds: usize) -> Vec<f32> {
    let n = rate as usize * seconds;
    let dit = rate as usize / 12; // ~20 WPM
    let mut noise: u32 = 0x5EED_1234;
    (0..n)
        .map(|i| {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let hiss = ((noise >> 8) as f32 / 8_388_608.0) - 1.0;
            // ..-. — a repeating element pattern, key-down half the time.
            let keyed = matches!((i / dit) % 8, 0 | 2 | 3 | 4 | 6);
            let tone = (2.0 * PI * 700.0 * (i as f32) / (rate as f32)).sin();
            if keyed {
                0.25 * tone + 0.04 * hiss
            } else {
                0.04 * hiss
            }
        })
        .collect()
}

fn read_wav(path: &str) -> (u32, Vec<f32>) {
    let mut reader =
        hound::WavReader::open(path).unwrap_or_else(|err| panic!("cannot open {path}: {err}"));
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let mono: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / f32::from(i16::MAX);
            reader
                .samples::<i16>()
                .map(|s| f32::from(s.unwrap()) * scale)
                .collect()
        }
    };
    if channels <= 1 {
        return (spec.sample_rate, mono);
    }
    // Same phase-coherent average the RX path uses, not a loudest-lane pick.
    let folded = mono
        .chunks(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect();
    (spec.sample_rate, folded)
}

fn write_wav(path: &str, rate: u32, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for sample in samples {
        let clamped = (sample * f32::from(i16::MAX)).clamp(-32768.0, 32767.0);
        writer.write_sample(clamped as i16).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn secs(samples: usize, rate: u32) -> f64 {
    samples as f64 / f64::from(rate)
}
