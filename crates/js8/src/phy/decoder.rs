//! The JS8 decode outer loop — `js8::phy::decode`. JS8Call's `DecodeMode::
//! operator()` (JS8.cpp:2255-2385 @ a7ff1be0) read as the SPEC: up to three
//! outer passes, each = fresh candidate search + fresh baseband spectrum over
//! the CURRENT audio (:2296-2329), candidates in ascending frequency
//! (JS8Call puts those within 10 Hz of `nfqso` first, :2308-2322;
//! `DecodeParams` carries no nfqso, so the order is plain ascending — the
//! same list JS8Call produces when nothing sits near its QSO frequency),
//! every accepted decode subtracted from the audio on passes ≤
//! `subtract_passes` (:2328 `ipass < 3`) so the NEXT pass finds weaker
//! stations; a pass with no candidates ends the decode (:2306), and so does
//! a pass that neither added a word nor raised an SNR (:2379). Dedupe
//! is by Word87 (payload + i3 + CRC — JS8Call's `Decode{type,data}` key,
//! :2353), never by text: phy never sees text (spec: the Word87 seam).
//!
//! Depth: the C++ decoder ignores it ("depth is now fixed", JS8.cpp:47-48);
//! the Fortran `js8` CLI the lab uses as oracle inherits ft8_decode's rule,
//! so `depth 1 → 2 passes, depth ≥ 2 → 3` (libtempo/ft8_cabi.f90:625-626)
//! is what `DecodeParams::depth` means here.
//!
//! CONTRACT (interfaces §1.6): `samples` is cycle-start aligned and normally
//! ≥ `speed.frames_needed()`; a longer window is truncated to one period, a
//! shorter one is zero-padded (JS8.cpp:2275-2294 zero-fills `dd` and copies
//! `sz ≤ NMAX` samples — the front-padded ring on the first cycle after a
//! tier switch decodes as silence, never panics). PURE — a fresh
//! `FftPlanner` and a fresh subtraction filter per call, no statics, so B5
//! may run four speeds under `std::thread::scope`. No decode path can key:
//! this returns bits.
//!
//! Bench (fill in at Task B3.9 from `cargo run --release -p js8 --example
//! decode_bench`): <date> <box>: Slow … ms, Normal … ms, Fast … ms, Turbo …
//! ms at depth 3, 3 signals + noise. B5's all-four-speeds-by-default ruling
//! (Turbo's 6 s cycle is the bound) cites this line.

use rustfft::FftPlanner;

use super::demod::demod_candidate;
use super::downsample::spectrum;
use super::params::geom;
use super::speed::Speed;
use super::subtract::{reference_wave, subtract, subtract_filter};
use super::sync::{find_candidates, SyncResult};
use super::{DecodeParams, RawDecode};

/// Outer passes for a depth (ft8_cabi.f90:625-626 convention).
pub(crate) fn npasses(depth: u8) -> usize {
    if depth <= 1 {
        2
    } else {
        3
    }
}

pub fn decode(samples: &[i16], speed: Speed, params: &DecodeParams) -> Vec<RawDecode> {
    let g = geom(speed);
    let mut dd = vec![0f32; g.nmax];
    for (d, &s) in dd.iter_mut().zip(samples.iter()) {
        *d = s as f32;
    }
    let mut planner = FftPlanner::<f32>::new();
    let filter = subtract_filter(g.nmax, &mut planner);
    let mut out: Vec<RawDecode> = Vec::new();

    for ipass in 1..=npasses(params.depth) {
        let SyncResult {
            candidates: mut cands,
            baseline_db,
        } = find_candidates(&dd, speed, params.nfa, params.nfb, &mut planner);
        if cands.is_empty() {
            break; // :2306 — more passes cannot yield more
        }
        cands.sort_by(|a, b| a.freq_hz.total_cmp(&b.freq_hz)); // :2308-2322 with no nfqso match
                                                               // The baseband spectrum is taken ONCE per pass over the audio as it
                                                               // stands (:2326 computeBasebandFFT); subtractions within the pass feed
                                                               // the next pass only.
        let sp = spectrum(&dd, speed, &mut planner);
        let lsubtract = ipass <= params.subtract_passes as usize;
        let mut improved = false;

        for c in &cands {
            let Some(d) = demod_candidate(&sp, &baseline_db, speed, c, &mut planner) else {
                continue;
            };
            // JS8Call subtracts inside js8dec on EVERY accepted decode, duplicates
            // included (:1412) — a second subtraction of an already-removed frame
            // only chases the residual.
            if lsubtract {
                let cref = reference_wave(&d.tones, speed, d.freq_hz);
                subtract(&mut dd, &cref, d.start_s, &filter, &mut planner);
            }
            match out.iter_mut().find(|r| r.word == d.word) {
                Some(prev) => {
                    if d.snr_db > prev.snr_db {
                        prev.snr_db = d.snr_db; // :2353-2360 — louder duplicate raises the SNR only
                        improved = true;
                    }
                }
                None => {
                    improved = true;
                    out.push(RawDecode {
                        speed,
                        freq_hz: d.freq_hz,
                        dt_s: d.dt_s,
                        snr_db: d.snr_db,
                        sync: c.sync,
                        word: d.word,
                        nharderrors: d.nharderrors,
                        quality: (1.0 - d.nharderrors as f32 / 60.0).clamp(0.0, 1.0), // :2370
                    });
                }
            }
        }
        if !improved {
            break; // :2379
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::params::geom;
    use crate::phy::testutil::{synth_window, word};

    /// The whole chain, every speed: one −5 dB frame in, exactly that word
    /// out, freq within 1 Hz, dt within two coarse steps, sync ≥ ASYNCMIN,
    /// quality in [0,1], speed stamped, word verified.
    #[test]
    fn round_trip_at_every_speed() {
        for speed in Speed::ALL {
            let g = geom(speed);
            let w = word("KD9TAWEN52ab");
            let iw = synth_window(&[(w, 1500.0, -5.0)], speed, 42);
            let out = decode(&iw, speed, &DecodeParams::stock());
            assert_eq!(out.len(), 1, "{speed:?}: {} decodes", out.len());
            let d = &out[0];
            assert_eq!(d.word, w);
            assert_eq!(d.speed, speed);
            assert!(
                (d.freq_hz - 1500.0).abs() <= 1.0,
                "{speed:?}: freq {}",
                d.freq_hz
            );
            assert!(
                d.dt_s.abs() <= 2.0 * g.tstep + 1e-6,
                "{speed:?}: dt {}",
                d.dt_s
            );
            assert!(d.sync >= 1.5);
            assert!((0.0..=1.0).contains(&d.quality));
            assert!(d.word.verify());
        }
    }

    /// The contract tolerates a window longer than frames_needed (the 36 s
    /// ring hands B5 more than one period) and one that is SHORT and
    /// front-padded (the first cycle after a tier switch): no panic, and the
    /// long window still decodes.
    #[test]
    fn tolerates_long_and_short_windows() {
        let speed = Speed::Fast;
        let w = word("KD9TAWEN52ab");
        let mut iw = synth_window(&[(w, 1000.0, -5.0)], speed, 9);
        iw.extend(std::iter::repeat_n(0i16, 36_000)); // 3 s of extra tail
        let out = decode(&iw, speed, &DecodeParams::stock());
        assert_eq!(out.len(), 1);
        let short = vec![0i16; speed.frames_needed() / 2];
        assert!(decode(&short, speed, &DecodeParams::stock()).is_empty());
        assert!(decode(&[], speed, &DecodeParams::stock()).is_empty());
    }

    /// depth 1 = two outer passes; depth ≥ 2 = three (the ft8_decode
    /// convention JS8Call's Fortran CLI inherits; the C++ GUI decoder always
    /// runs three). Pinned so decode_bench numbers mean what they say.
    #[test]
    fn passes_follow_depth() {
        assert_eq!(npasses(1), 2);
        assert_eq!(npasses(2), 3);
        assert_eq!(npasses(3), 3);
        assert_eq!(npasses(9), 3);
    }

    /// Two frames at the same offset decode as two words, never merged, and
    /// the same word twice in one window is reported once.
    #[test]
    fn dedupes_by_word_not_by_frequency() {
        let speed = Speed::Normal;
        let a = word("KD9TAWEN52ab");
        let b = word("W1AWFN31cdef");
        let iw = synth_window(&[(a, 900.0, -8.0), (b, 2100.0, -8.0)], speed, 77);
        let out = decode(&iw, speed, &DecodeParams::stock());
        assert_eq!(
            out.len(),
            2,
            "{:?}",
            out.iter().map(|d| d.freq_hz).collect::<Vec<_>>()
        );
        assert!(out.iter().any(|d| d.word == a) && out.iter().any(|d| d.word == b));
    }
}
