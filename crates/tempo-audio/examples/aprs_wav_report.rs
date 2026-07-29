//! Run a real off-air recording through the ENTIRE APRS receive chain and report what happened,
//! burst by burst. The diagnostic for "I can hear packets and nothing decodes".
//!
//! Every other APRS test in the tree either starts from bytes or feeds the modem its own
//! modulator's output. This is the one path that takes what the radio actually produced —
//! `decimate to 12 kHz → mono fold → AFSK demod → HDLC deframe → AX.25 FCS → APRS parse` — using
//! the same code the live app runs, and prints the measurements that distinguish the plausible
//! causes from each other.
//!
//! ```text
//! cargo run -q -p tempo-audio --features device --example aprs_wav_report -- CAPTURE.wav
//! ```
//!
//! ## Reading the report
//! * **peak dBFS** — burst level. NOT a decodability threshold: the Bell-202 discriminator
//!   compares mark energy against space energy, so absolute level cancels (measured: decodes at
//!   -140 dBFS). Level costs headroom and margin against noise, nothing more.
//! * **clipped** — samples at/near full scale. FSK survives hard limiting (measured: decodes 30 dB
//!   into clipping), so this is a headroom warning, not a cause.
//! * **mark/space tilt** — how loud a 1200 Hz mark is when a mark is sent, versus a 2200 Hz space
//!   when a space is sent. Near 0 dB is a flat audio path. Strongly negative means the space tone
//!   is rolled off: the rig's RX filter, de-emphasis, or a narrow-FM setting — not anything in
//!   software. THE interesting number when level and clipping are both fine. (Pattern- and
//!   phase-independent by construction; see `tone_tilt_db` for the two wrong instruments first.)
//! * **frames / CRC** — candidates recovered, and for each failure how far it got: a frame whose
//!   addresses parse but whose FCS fails was nearly right (bit errors in the payload); a frame
//!   whose length is implausible never had sync at all.

use tempo_core::aprs::{fcs, Deframer, Demod, Frame};

const MODEM_RATE: u32 = 12_000;
/// Level above the clip line, for reporting.
const CLIP_PEAK: f32 = 0.99;
/// Burst segmentation opens this far below the file's loudest window, rather than at an absolute
/// level. A fixed threshold missed the field case entirely: a real burst at -62 dBFS sits far below
/// any sensible absolute gate, and reporting "no bursts" for a capture that plainly contains one is
/// exactly the kind of wrong answer this tool exists to avoid.
const BURST_OPEN_BELOW_PEAK_DB: f32 = -20.0;
/// Window used to segment bursts (~20 ms at 12 kHz).
const SEG: usize = 240;

fn db(a: f32) -> f32 {
    if a > 0.0 {
        20.0 * a.log10()
    } else {
        f32::NEG_INFINITY
    }
}

fn dbs(a: f32) -> String {
    if a > 0.0 {
        format!("{:>7.1}", db(a))
    } else {
        "   -inf".to_string()
    }
}

/// Samples per bit at the modem rate (12000/1200).
const SPB: usize = 10;

/// Quadrature magnitude at `hz` over exactly one bit window.
fn tone_mag(win: &[f32], hz: f32) -> f32 {
    let (mut i, mut q) = (0.0f32, 0.0f32);
    let w = std::f32::consts::TAU * hz / MODEM_RATE as f32;
    for (n, &s) in win.iter().enumerate() {
        let a = w * n as f32;
        i += s * a.cos();
        q += s * a.sin();
    }
    (i * i + q * q).sqrt()
}

/// How loud a MARK is when a mark is sent, versus a SPACE when a space is sent — in dB.
///
/// This is the audio path's tilt, and getting the instrument right took two tries. A single
/// coherent correlation across the burst measures the DATA PATTERN, not the channel: the same
/// signal read -5.2 dB on one frame and +14.6 dB on another. Restricting it to the preamble was
/// still unstable (-6.3 vs -19.3), because a tone that switches every 10 samples has no consistent
/// phase relationship with a continuous reference.
///
/// So: classify each bit by whichever tone wins, then compare the AVERAGE magnitude of mark bits
/// against that of space bits. Phase-independent, pattern-independent, and it answers the question
/// actually being asked — is the rig rolling off 2200 Hz?
fn tone_tilt_db(x: &[f32]) -> Option<f32> {
    let (mut m_sum, mut m_n, mut s_sum, mut s_n) = (0.0f32, 0usize, 0.0f32, 0usize);
    for win in x.chunks_exact(SPB) {
        let m = tone_mag(win, 1200.0);
        let sp = tone_mag(win, 2200.0);
        if m >= sp {
            m_sum += m;
            m_n += 1;
        } else {
            s_sum += sp;
            s_n += 1;
        }
    }
    if m_n < 8 || s_n < 8 {
        return None; // not enough of both tones to compare
    }
    let (m_avg, s_avg) = (m_sum / m_n as f32, s_sum / s_n as f32);
    if m_avg <= 0.0 || s_avg <= 0.0 {
        return None;
    }
    Some(20.0 * (m_avg / s_avg).log10())
}

/// Read a 16-bit PCM WAV and fold to mono by AVERAGING the channels.
///
/// ⚠️ Deliberately NOT `tempo_core::wavfile::read_wav_i16`, which takes channel 0 only. The live
/// capture callback averages (`device.rs`, and the invariant is load-bearing enough to have its own
/// memory: averaging keeps a mono signal phase-coherent however the rig's codec lays it across a
/// stereo stream). A diagnostic that folds differently from the app is measuring a different
/// signal, and its verdict would not transfer.
fn read_wav_mono_avg(path: &str) -> std::io::Result<(Vec<f32>, u32)> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let buf = std::fs::read(path)?;
    if buf.len() < 12 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"WAVE" {
        return Err(bad("not a RIFF/WAVE file"));
    }
    let u16le = |b: &[u8]| u16::from(b[0]) | (u16::from(b[1]) << 8);
    let u32le = |b: &[u8]| {
        u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16) | (u32::from(b[3]) << 24)
    };
    let (mut channels, mut rate, mut bits) = (1u16, 0u32, 16u16);
    let mut out: Option<Vec<f32>> = None;
    let mut pos = 12usize;
    while pos + 8 <= buf.len() {
        let id = &buf[pos..pos + 4];
        let len = u32le(&buf[pos + 4..pos + 8]) as usize;
        let body = pos + 8;
        if body + len > buf.len() {
            break;
        }
        if id == b"fmt " && len >= 16 {
            channels = u16le(&buf[body + 2..body + 4]).max(1);
            rate = u32le(&buf[body + 4..body + 8]);
            bits = u16le(&buf[body + 14..body + 16]);
        } else if id == b"data" {
            if bits != 16 {
                return Err(bad("only 16-bit PCM is handled"));
            }
            let ch = channels as usize;
            let frame_bytes = 2 * ch;
            let n = len / frame_bytes;
            let mut v = Vec::with_capacity(n);
            for i in 0..n {
                let o = body + i * frame_bytes;
                let sum: i32 = (0..ch)
                    .map(|c| i32::from(i16::from_le_bytes([buf[o + 2 * c], buf[o + 2 * c + 1]])))
                    .sum();
                v.push(sum as f32 / ch as f32 / 32768.0);
            }
            out = Some(v);
        }
        pos = body + len + (len & 1);
    }
    match (out, rate) {
        (Some(v), r) if r > 0 => Ok((v, r)),
        _ => Err(bad("no fmt/data chunk found")),
    }
}

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: aprs_wav_report CAPTURE.wav");
            std::process::exit(2);
        }
    };

    let (raw, sr) = match read_wav_mono_avg(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not read {path}: {e}");
            std::process::exit(1);
        }
    };
    let secs = raw.len() as f32 / sr as f32;
    let raw_peak = raw.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let raw_clipped = raw.iter().filter(|s| s.abs() >= CLIP_PEAK).count();

    println!("FILE   {path}");
    println!(
        "INPUT  {sr} Hz, {:.1} s, peak {} dBFS, {raw_clipped} clipped sample(s)",
        secs,
        dbs(raw_peak).trim()
    );

    // The real capture path: anti-aliased decimation to the modem rate, exactly as device.rs does.
    let mut rs = tempo_audio::capture_resample::CaptureResampler::new(sr, MODEM_RATE);
    let audio = rs.process(&raw);
    println!(
        "MODEM  {} samples at {MODEM_RATE} Hz after the capture resampler\n",
        audio.len()
    );

    // ---- Burst segmentation, so every measurement is reported AT BURST TIME ----
    let file_peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let open_at = file_peak * 10f32.powf(BURST_OPEN_BELOW_PEAK_DB / 20.0);
    let mut bursts: Vec<(usize, usize)> = Vec::new();
    let mut open: Option<usize> = None;
    for (w, chunk) in audio.chunks(SEG).enumerate() {
        let p = chunk.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        match (p >= open_at, open) {
            (true, None) => open = Some(w * SEG),
            (false, Some(start)) => {
                bursts.push((start, w * SEG));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        bursts.push((start, audio.len()));
    }

    // ---- The full chain, in 100 ms drains like the live thread, recording where each frame fell ----
    let mut demod = Demod::new();
    let mut deframer = Deframer::new();
    let mut frames: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut pos = 0usize;
    for chunk in audio.chunks(1200) {
        for f in deframer.push(&demod.feed(chunk)) {
            frames.push((pos, f));
        }
        pos += chunk.len();
    }

    println!(
        "BURSTS {} detected (opening {BURST_OPEN_BELOW_PEAK_DB:.0} dB below the file peak, i.e. {} dBFS)",
        bursts.len(),
        dbs(open_at).trim()
    );
    if bursts.is_empty() {
        println!("  (nothing above the burst threshold — the capture may be silence, or the");
        println!("   level is so low that segmentation missed it; check INPUT peak above)");
    }
    for (n, &(a, b)) in bursts.iter().enumerate() {
        let seg = &audio[a..b.min(audio.len())];
        let peak = seg.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let clipped = seg.iter().filter(|s| s.abs() >= CLIP_PEAK).count();
        let balance = match tone_tilt_db(seg) {
            Some(d) => format!("{d:>+6.1} dB"),
            None => "   n/a".to_string(),
        };
        let here = frames
            .iter()
            .filter(|(p, _)| *p >= a && *p < b + 2400)
            .count();
        println!(
            "  #{:<2} {:>6.2}-{:>6.2} s  peak {} dBFS  clipped {clipped:<5}  mark/space tilt {balance}  frames {here}",
            n + 1,
            a as f32 / MODEM_RATE as f32,
            b as f32 / MODEM_RATE as f32,
            dbs(peak)
        );
    }

    println!(
        "\nFRAMES {} candidate(s) recovered by the deframer",
        frames.len()
    );
    let mut passed = 0;
    for (n, (at, bytes)) in frames.iter().enumerate() {
        let t = *at as f32 / MODEM_RATE as f32;
        if bytes.len() < 18 {
            println!(
                "  #{:<2} {t:>6.2} s  {:>4} bytes  REJECT: too short to be an AX.25 UI frame — no real sync",
                n + 1,
                bytes.len()
            );
            continue;
        }
        let (content, fcs_bytes) = bytes.split_at(bytes.len() - 2);
        let got = u16::from(fcs_bytes[0]) | (u16::from(fcs_bytes[1]) << 8);
        let want = fcs(content);
        match Frame::decode(bytes) {
            Some(f) => {
                passed += 1;
                println!(
                    "  #{:<2} {t:>6.2} s  {:>4} bytes  CRC OK   {} > {}  info {:?}",
                    n + 1,
                    bytes.len(),
                    f.source.call,
                    f.dest.call,
                    String::from_utf8_lossy(&f.info)
                        .chars()
                        .take(48)
                        .collect::<String>()
                );
            }
            None => {
                // How far did it get? Addresses parsing means near-miss; otherwise no sync.
                let addr_ok = content.len() >= 14
                    && content[..14].iter().all(|b| {
                        let c = (b >> 1) & 0x7F;
                        c == b' ' || c.is_ascii_alphanumeric()
                    });
                println!(
                    "  #{:<2} {t:>6.2} s  {:>4} bytes  CRC FAIL fcs got {got:#06x} want {want:#06x} — {}",
                    n + 1,
                    bytes.len(),
                    if addr_ok {
                        "addresses look sane: bit errors in the payload (near miss)"
                    } else {
                        "address field is garbage: never really had sync"
                    }
                );
            }
        }
    }

    println!("\nVERDICT");
    println!("  {passed} of {} candidate(s) passed the FCS", frames.len());
    if frames.is_empty() {
        println!("  Nothing recovered. If BURSTS found audio, the demodulator never reached bit");
        println!("  sync — look at tone balance (RX filter / narrow-FM rolloff) before level.");
    } else if passed == 0 {
        println!("  Candidates found, none verified. Check tone balance first: level and clipping");
        println!("  are measured NOT to prevent decode (energy-difference discriminator, survives");
        println!("  30 dB of limiting). A near-miss on every frame points at bit errors — narrow");
        println!("  FM deviation clipping, audio rolloff, or a station transmitting badly.");
    }
}
