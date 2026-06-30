//! Single-signal CW decoder — reads Morse from the receive audio at the operator's CW
//! pitch. Pipeline: Goertzel envelope at the pitch → adaptive threshold → mark/space
//! segments → adaptive unit (WPM) → dit/dah + gap classification → Morse → text.
//!
//! Pure + deterministic. Tuned + tested against [`crate::cw::morse_samples`] (clean,
//! machine-timed CW); weak/hand-sent signals are the expected on-air tuning frontier.

use crate::cw::morse_code;
use crate::spectrum::tone_power;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Reverse of [`morse_code`]: a Morse string ("-.-.") → its character. Built once from
/// the forward table (so it stays in sync with whatever glyphs the table supports).
fn morse_to_char(code: &str) -> Option<char> {
    static REV: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    let map = REV.get_or_init(|| {
        let mut m = HashMap::new();
        for u in 0x20u8..0x7f {
            let c = (u as char).to_ascii_uppercase();
            if let Some(code) = morse_code(c) {
                m.entry(code).or_insert(c);
            }
        }
        m
    });
    map.get(code).copied()
}

/// A CW decode result: the recovered text and the estimated sending speed (WPM).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CwDecode {
    pub text: String,
    pub wpm: u32,
}

/// Decode CW from `samples` (mono f32) heard at `pitch_hz`, sampled at `sr` Hz.
/// Returns empty text when there's no clear keyed signal in the buffer.
pub fn decode_cw(samples: &[f32], sr: f32, pitch_hz: f32) -> CwDecode {
    if sr <= 0.0 || samples.len() < (sr * 0.05) as usize {
        return CwDecode::default(); // < ~50 ms — nothing to decode
    }
    // 1. Envelope: non-overlapping ~4 ms hops of Goertzel power at the pitch.
    let hop = (sr * 0.004).max(1.0) as usize;
    let env: Vec<f32> = samples
        .chunks(hop)
        .filter(|c| c.len() == hop)
        .map(|c| tone_power(c, sr, pitch_hz))
        .collect();
    if env.len() < 8 {
        return CwDecode::default();
    }
    // 2. Threshold: midpoint between the noise floor (low percentile) and the signal
    //    (high percentile). Require a clear on/off ratio, else there's no keying.
    let mut sorted = env.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lo = sorted[sorted.len() / 10].max(1e-9);
    let hi = sorted[sorted.len() * 9 / 10];
    if hi < lo * 3.0 {
        return CwDecode::default(); // no clear keyed signal (steady noise/carrier/silence)
    }
    let thresh = (lo + hi) * 0.5;
    // 3. Segments: runs of mark (key-down) / space (key-up), in hops.
    let mut segs: Vec<(bool, usize)> = Vec::new();
    for &p in &env {
        let mark = p >= thresh;
        match segs.last_mut() {
            Some((m, n)) if *m == mark => *n += 1,
            _ => segs.push((mark, 1)),
        }
    }
    // Trim leading silence so a long pre-signal gap can't emit spurious spaces.
    let start = segs.iter().position(|(m, _)| *m).unwrap_or(segs.len());
    let segs = &segs[start..];
    if segs.is_empty() {
        return CwDecode::default();
    }
    // 4. Adaptive unit (1 dit, in hops): a low percentile of all element durations —
    //    dits and intra-character gaps are the shortest, most-common elements (1 unit).
    let mut durs: Vec<usize> = segs.iter().map(|(_, n)| *n).collect();
    durs.sort_unstable();
    let rough = (durs[durs.len() / 5].max(1)) as f32;
    // Refine the dit length by averaging the whole 1-unit cluster (elements < 2× rough):
    // threshold-crossing trims a hop off each dit but adds it to the adjacent intra-char
    // gap, so the dit-and-gap mean cancels that bias → an accurate WPM.
    let ones: Vec<usize> = durs.iter().copied().filter(|&d| (d as f32) < 2.0 * rough).collect();
    let unit = if ones.is_empty() {
        rough
    } else {
        ones.iter().sum::<usize>() as f32 / ones.len() as f32
    };
    // 5. Decode: marks → dit/dah (boundary 2 units); spaces → intra (<2u) / inter (2–5u,
    //    emit the character) / word (≥5u, also a space).
    let mut text = String::new();
    let mut sym = String::new();
    let flush = |sym: &mut String, text: &mut String| {
        if !sym.is_empty() {
            if let Some(c) = morse_to_char(sym) {
                text.push(c);
            }
            sym.clear();
        }
    };
    for (mark, n) in segs {
        let u = *n as f32 / unit;
        if *mark {
            sym.push(if u < 2.0 { '.' } else { '-' });
        } else if u >= 2.0 {
            flush(&mut sym, &mut text);
            if u >= 5.0 {
                text.push(' ');
            }
        }
    }
    flush(&mut sym, &mut text);
    // WPM from the dit unit: dit_secs = unit_hops × hop / sr; PARIS wpm = 1.2 / dit_secs.
    let dit_secs = unit * hop as f32 / sr;
    let wpm = if dit_secs > 0.0 {
        (1.2 / dit_secs).round().clamp(0.0, 99.0) as u32
    } else {
        0
    };
    CwDecode {
        text: text.trim().to_string(),
        wpm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cw::morse_samples;

    const SR: f32 = 48_000.0;
    const PITCH: f32 = 600.0;

    fn decode(text: &str, wpm: u32) -> CwDecode {
        // Pad with leading + trailing silence (a real capture is never perfectly trimmed).
        let mut audio = vec![0.0f32; (SR * 0.1) as usize];
        audio.extend(morse_samples(text, wpm, PITCH, SR as u32));
        audio.extend(vec![0.0f32; (SR * 0.1) as usize]);
        decode_cw(&audio, SR, PITCH)
    }

    #[test]
    fn decodes_a_clean_callsign_and_estimates_wpm() {
        let d = decode("CQ TEST DE W1ABC", 20);
        assert_eq!(d.text, "CQ TEST DE W1ABC");
        assert!((d.wpm as i32 - 20).abs() <= 2, "≈20 wpm, got {}", d.wpm);
    }

    #[test]
    fn decodes_across_speeds() {
        assert_eq!(decode("PARIS", 15).text, "PARIS");
        assert_eq!(decode("599 TU", 25).text, "599 TU");
        assert_eq!(decode("K", 30).text, "K");
    }

    #[test]
    fn empty_on_silence_and_steady_tone() {
        assert_eq!(decode_cw(&vec![0.0f32; 48_000], SR, PITCH), CwDecode::default());
        // A steady (un-keyed) carrier — no on/off ratio → nothing to decode.
        let steady: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * std::f32::consts::PI * PITCH * i as f32 / SR).sin())
            .collect();
        assert_eq!(decode_cw(&steady, SR, PITCH).text, "");
    }

    #[test]
    fn morse_to_char_reverses_the_table() {
        assert_eq!(morse_to_char("."), Some('E'));
        assert_eq!(morse_to_char("-.-."), Some('C'));
        assert_eq!(morse_to_char("....."), Some('5'));
        assert_eq!(morse_to_char(".-.-"), None); // not a glyph
    }
}
