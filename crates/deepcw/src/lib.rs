//! DeepCW inference harness — runs the e04/deepcw-engine ONNX model (AGPL-3.0-only,
//! NOT vendored here; see Cargo.toml) through `tract` for low-SNR CW decoding.
//!
//! The preprocessing mirrors the engine's reference implementation exactly
//! (`examples/python/decode_morse.py` + `model.onnx.json`): mono f32 @ 3200 Hz →
//! reflect-pad by fft/2 → periodic-Hann STFT (fft 256, hop 48) → magnitude of the
//! 400–1200 Hz bins (65) → log1p → `[1, 1, time, freq]` float32 → the model's CTC
//! `log_probs [1, time, class]` → greedy CTC collapse (blank 41).
//!
//! Everything except the model itself is our code; the pure pieces (padding, window,
//! spectrogram shape, CTC) are unit-tested against the reference's semantics.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tract_onnx::prelude::*;

/// `PATH` with every WBEM directory removed, or `None` if there was none to remove.
///
/// `wmic.exe` lives in `System32\wbem`, and that directory is reachable from
/// `CreateProcess` ONLY through `PATH` — the image search covers the application
/// directory, the current directory, the system and Windows directories and then
/// `PATH`, and `wbem` is none of the first four. So dropping the `wbem` entries is
/// exactly "make `wmic` unfindable", nothing wider.
///
/// Pure and matched case-insensitively on the final component, so it is testable off
/// Windows — which is the whole reason it is split out from [`warm_cpu_cache_probe`].
#[cfg_attr(not(windows), allow(dead_code))] // only APPLIED on Windows; tested everywhere
fn path_without_wbem(path: &OsStr) -> Option<OsString> {
    let kept: Vec<PathBuf> = std::env::split_paths(path)
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("wbem"))
        })
        .collect();
    let rebuilt = std::env::join_paths(kept).ok()?;
    (rebuilt != path).then_some(rebuilt)
}

/// Settle `tract`'s CPU-cache probe now, while it cannot open a console window.
///
/// ⚠️ **This is the second Windows console flash, and it is not our spawn.**
/// `tract-linalg` sizes its matmul cache blocking from the real cache geometry, and
/// its Windows probe is `std::process::Command::new("wmic")` with no
/// `CREATE_NO_WINDOW` (`tract-linalg/src/cache.rs`, unchanged through 0.23.7 — the
/// latest published version as of 2026-09-16). Nexus is a GUI process, so that spawn
/// puts a command-prompt window on screen for as long as `wmic` runs, which is the
/// better part of a second.
///
/// It reaches an operator only through here: `tract` is pulled in by this crate alone,
/// this crate runs only on the AI CW decode thread, and that thread runs only in CW
/// with the AI decoder on (which is the default). **That is why both flash reports
/// named CW** — 2026-08-19 "moving to cw, I saw an ultra quick flash of what looked
/// like a terminal screen", and 1.13.0-test3 "I saw it on the CW section while
/// decoding". The probe is memoised in a `OnceLock`, so it is one flash per launch,
/// at whatever moment the first inference happens — which reads as intermittent.
///
/// We cannot pass that spawn a flag; it is inside a crates.io dependency. What we can
/// do is decide WHEN it runs and what it finds. Called once from the top of
/// `tempo_lib::run`, before any thread exists:
///
/// 1. drop the WBEM directory from this process's `PATH`, so `CreateProcess` cannot
///    find `wmic.exe` and fails outright — an error, never a window;
/// 2. call `cache_info()`, which runs the probe, gets that error, and memoises
///    "unknown" for the process lifetime;
/// 3. put `PATH` back, so every child Nexus DOES spawn (rigctld, rotctld, TQSL,
///    PowerShell, the clock-diagnosis tools) sees the environment it always saw.
///
/// The cost is that `tract` blocks against its documented fallback (256 KiB L2, no L3
/// tier) instead of the machine's real cache. That is not a new code path: it is what
/// `tract` uses on every platform without a probe, and what it already gets on Windows
/// 11 24H2 and later, where Microsoft has removed `wmic` from the OS.
///
/// `std::env::set_var` is why this must run before threads start — it is safe in
/// edition 2021 but not thread-safe, and `run()` is single-threaded at that point (it
/// already sets `WEBKIT_DISABLE_DMABUF_RENDERER` there for the same reason).
///
/// Off Windows the probe reads `/sys` or `sysctl` in-process and spawns nothing, so
/// this is just an early memoisation.
pub fn warm_cpu_cache_probe() {
    #[cfg(windows)]
    let restore = std::env::var_os("PATH").and_then(|original| {
        let stripped = path_without_wbem(&original)?;
        std::env::set_var("PATH", &stripped);
        Some(original)
    });
    // Reaching THROUGH tract-onnx rather than depending on tract-linalg directly: the
    // version must be the one this crate's inference actually uses, and a second
    // dependency line could drift from it.
    let _ = tract_onnx::tract_core::tract_linalg::cache::cache_info();
    #[cfg(windows)]
    if let Some(original) = restore {
        std::env::set_var("PATH", original);
    }
}

/// The engine's `model.onnx.json` — the model I/O contract.
#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    pub chars: Vec<String>,
    pub blank_index: usize,
    pub sample_rate: u32,
    pub fft_length: usize,
    pub hop_length: usize,
    pub spectrogram_min_freq_hz: f32,
    pub spectrogram_max_freq_hz: f32,
    pub spectrogram_frequency_bins: usize,
    pub normalization: String,
    pub onnx_input_name: String,
    pub onnx_output_name: String,
}

/// Reflect-pad without repeating the edge sample (numpy `mode="reflect"`):
/// `[a b c]` padded by 2 → `[c b a b c b a]`.
fn reflect_pad(audio: &[f32], pad: usize) -> Vec<f32> {
    let n = audio.len();
    assert!(n > pad, "audio shorter than the reflect pad");
    let mut out = Vec::with_capacity(n + 2 * pad);
    for i in (1..=pad).rev() {
        out.push(audio[i]);
    }
    out.extend_from_slice(audio);
    for i in 2..=(pad + 1) {
        out.push(audio[n - i]);
    }
    out
}

/// Periodic Hann window (numpy `hanning(N+1)[:-1]`): `0.5 − 0.5·cos(2πn/N)`.
fn hann_periodic(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect()
}

/// The model's input spectrogram: returns `(flattened [time × freq], time_frames)`.
pub fn spectrogram(audio: &[f32], m: &Metadata) -> (Vec<f32>, usize) {
    assert_eq!(m.fft_length, 256, "harness is specialized to fft 256");
    assert_eq!(m.normalization, "log1p");
    let bin_hz = m.sample_rate as f32 / m.fft_length as f32;
    let start_bin = (m.spectrogram_min_freq_hz / bin_hz).ceil() as usize;
    let stop_bin = (m.spectrogram_max_freq_hz / bin_hz).floor() as usize + 1;
    assert_eq!(
        stop_bin - start_bin,
        m.spectrogram_frequency_bins,
        "metadata bin range mismatch"
    );

    let padded = reflect_pad(audio, m.fft_length / 2);
    let window = hann_periodic(m.fft_length);
    let frames = 1 + (padded.len() - m.fft_length) / m.hop_length;
    let bins = m.spectrogram_frequency_bins;
    let mut out = Vec::with_capacity(frames * bins);
    let mut frame = [0f32; 256];
    for fi in 0..frames {
        let start = fi * m.hop_length;
        for (i, w) in window.iter().enumerate() {
            frame[i] = padded[start + i] * w;
        }
        // microfft packs Nyquist into bin 0's imaginary part; our 400–1200 Hz slice
        // (bins 32..97 at 12.5 Hz/bin) never touches bin 0, so plain magnitude is safe.
        let spectrum = microfft::real::rfft_256(&mut frame);
        for c in &spectrum[start_bin..stop_bin] {
            out.push((c.re * c.re + c.im * c.im).sqrt().ln_1p());
        }
    }
    (out, frames)
}

/// Greedy CTC best-path collapse with per-character timing: each emitted character
/// carries the moment (seconds into the clip) of its first frame — the basis for
/// stitching overlapping windows into one transcript without re-emitting text.
pub fn greedy_ctc_timed(
    log_probs: &[f32],
    time: usize,
    classes: usize,
    clip_secs: f32,
    m: &Metadata,
) -> Vec<(f32, String)> {
    let mut decoded = Vec::new();
    let mut previous: Option<usize> = None;
    for t in 0..time {
        let row = &log_probs[t * classes..(t + 1) * classes];
        let best = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(m.blank_index);
        if best == m.blank_index {
            previous = None;
            continue;
        }
        if previous != Some(best) {
            let at = (t as f32 + 0.5) / time as f32 * clip_secs;
            decoded.push((at, m.chars[best].clone()));
        }
        previous = Some(best);
    }
    decoded
}

/// Greedy CTC best-path collapse: drop blanks (resetting the repeat tracker), merge repeats.
pub fn greedy_ctc(log_probs: &[f32], time: usize, classes: usize, m: &Metadata) -> String {
    let mut decoded = String::new();
    let mut previous: Option<usize> = None;
    for t in 0..time {
        let row = &log_probs[t * classes..(t + 1) * classes];
        let best = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(m.blank_index);
        if best == m.blank_index {
            previous = None;
            continue;
        }
        if previous != Some(best) {
            decoded.push_str(&m.chars[best]);
        }
        previous = Some(best);
    }
    decoded
}

/// Linear resampler matching the reference example (keeps the comparison honest).
pub fn resample_linear(audio: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate || audio.is_empty() {
        return audio.to_vec();
    }
    let target_len =
        ((audio.len() as f64) * target_rate as f64 / source_rate as f64).round() as usize;
    let mut out = Vec::with_capacity(target_len);
    for i in 0..target_len {
        let pos = i as f64 * source_rate as f64 / target_rate as f64;
        let left = pos.floor() as usize;
        let right = (left + 1).min(audio.len() - 1);
        let frac = (pos - left as f64) as f32;
        out.push(audio[left] * (1.0 - frac) + audio[right] * frac);
    }
    out
}

/// The DeepCW decoder: loads the model + metadata from a deepcw-engine checkout.
pub struct DeepCw {
    model_path: PathBuf,
    pub meta: Metadata,
    /// The compiled plan, cached per frame count (all windows share one in practice).
    // tract 0.23: `into_runnable()` returns the plan Arc'd, and `run` takes
    // `self: &Arc<Self>` — hold the Arc it hands us.
    plan: std::sync::Mutex<Option<(usize, std::sync::Arc<TypedSimplePlan>)>>,
}

impl DeepCw {
    pub fn load(engine_dir: &Path) -> Result<Self, String> {
        let meta_path = engine_dir.join("model.onnx.json");
        let model_path = engine_dir.join("model.onnx");
        // The engine is a PAIR — weights + metadata sidecar — and both are
        // gitignored, so a build that stages only one silently ships a decoder
        // that cannot load. Report WHICH half is missing.
        if !model_path.exists() {
            return Err(format!("model weights missing: {}", model_path.display()));
        }
        if !meta_path.exists() {
            return Err(format!("model metadata missing: {}", meta_path.display()));
        }
        let meta: Metadata = serde_json::from_str(
            &std::fs::read_to_string(&meta_path)
                .map_err(|e| format!("read {}: {e}", meta_path.display()))?,
        )
        .map_err(|e| format!("parse metadata: {e}"))?;
        Ok(DeepCw {
            model_path,
            meta,
            plan: std::sync::Mutex::new(None),
        })
    }

    /// Decode with per-character timestamps (seconds into the clip) — for stitching
    /// overlapping live windows into one transcript.
    pub fn decode_timed(&self, audio: &[f32]) -> Result<Vec<(f32, String)>, String> {
        let clip_secs = audio.len() as f32 / self.meta.sample_rate as f32;
        let (t, classes, flat) = self.run(audio)?;
        Ok(greedy_ctc_timed(&flat, t, classes, clip_secs, &self.meta))
    }

    /// Decode a mono clip already at the model's sample rate (3200 Hz). Builds the tract
    /// plan for this clip's frame count (fine for offline/spike use).
    pub fn decode(&self, audio: &[f32]) -> Result<String, String> {
        let (t, classes, flat) = self.run(audio)?;
        Ok(greedy_ctc(&flat, t, classes, &self.meta))
    }

    /// Run inference: returns `(time_steps, classes, flattened log_probs)`.
    fn run(&self, audio: &[f32]) -> Result<(usize, usize, Vec<f32>), String> {
        let bins = self.meta.spectrogram_frequency_bins;
        let (spec, frames) = spectrogram(audio, &self.meta);
        // tract needs CONCRETE dims here (a Range node can't type-infer over a symbolic
        // time), but the graph's 718 value_info annotations pin symbolic batch/time and
        // reject concrete facts. The harness therefore runs against a copy of the model
        // constant-folded at the window's frame count (onnxsim; see the spike setup) —
        // with that, a concrete input fact analyses cleanly end to end. The compiled plan
        // is cached: production streams fixed windows, so it builds exactly once.
        let mut cache = self
            .plan
            .lock()
            .map_err(|_| "plan lock poisoned".to_string())?;
        if cache.as_ref().map(|(f, _)| *f) != Some(frames) {
            let model = tract_onnx::onnx()
                .model_for_path(&self.model_path)
                .map_err(|e| format!("load onnx: {e}"))?
                .with_input_fact(
                    0,
                    InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, frames, bins)),
                )
                .map_err(|e| format!("input fact: {e}"))?
                .into_optimized()
                .map_err(|e| format!("optimize: {e:?}"))?
                .into_runnable()
                .map_err(|e| format!("plan: {e}"))?;
            *cache = Some((frames, model));
        }
        let model = &cache.as_ref().unwrap().1;
        let input = tract_ndarray::Array4::from_shape_vec((1, 1, frames, bins), spec)
            .map_err(|e| format!("tensor shape: {e}"))?;
        let outputs = model
            .run(tvec!(Tensor::from(input).into()))
            .map_err(|e| format!("run: {e}"))?;
        // tract 0.23: TValue derefs to Tensor; the array-view accessor moved to
        // the TensorView layer, and the output is contiguous, so a flat slice is
        // the direct read.
        let out = &outputs[0];
        let shape = out.shape().to_vec(); // [1, time, classes]
        if shape.len() != 3 || shape[0] != 1 {
            return Err(format!("unexpected output shape {shape:?}"));
        }
        let flat: Vec<f32> = out
            .view()
            .as_slice::<f32>()
            .map_err(|e| format!("output view: {e}"))?
            .to_vec();
        Ok((shape[1], shape[2], flat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `PATH` string in this platform's separator, so the test says the same
    /// thing on Windows (`;`) as it does in CI on Linux (`:`).
    fn path_of(entries: &[&str]) -> OsString {
        std::env::join_paths(entries.iter().map(Path::new)).expect("join")
    }

    /// The WBEM directory goes, in any case and with or without a trailing separator,
    /// wherever it sits — and nothing else moves. That the REST is untouched is the half
    /// that matters: this edits the live process `PATH`, and a child that cannot find
    /// `rigctld` would be a worse bug than a flash.
    ///
    /// Forward slashes and no drive letters, because this test runs on Linux in CI:
    /// `join_paths` rejects a `:` there, and `/` is a directory separator on Windows too.
    /// What that leaves unproven on Linux is only `Path::file_name`'s handling of `\`,
    /// which is std's own documented Windows behaviour.
    #[test]
    fn strips_only_the_wbem_entry() {
        let winlike = ["/Windows/system32", "/Windows/System32/Wbem", "/ham"];
        assert_eq!(
            path_without_wbem(&path_of(&winlike)),
            Some(path_of(&["/Windows/system32", "/ham"]))
        );
        // Lower case, trailing separator, and first rather than middle.
        assert_eq!(
            path_without_wbem(&path_of(&["/windows/system32/wbem/", "/ham"])),
            Some(path_of(&["/ham"]))
        );
        // A directory that merely CONTAINS "wbem" is a different directory.
        assert_eq!(path_without_wbem(&path_of(&["/wbem-tools"])), None);
        // Nothing to strip → None, so the caller does not touch the environment at all.
        assert_eq!(path_without_wbem(&path_of(&["/Windows/system32"])), None);
        assert_eq!(path_without_wbem(OsStr::new("")), None);
    }

    /// The warm-up runs, does not panic, and leaves `PATH` exactly as it found it.
    #[test]
    fn warm_up_leaves_the_environment_alone() {
        let before = std::env::var_os("PATH");
        warm_cpu_cache_probe();
        assert_eq!(std::env::var_os("PATH"), before);
    }

    fn meta() -> Metadata {
        serde_json::from_str(
            r#"{
            "chars": ["A","B","C"," "],
            "blank_index": 3,
            "sample_rate": 3200,
            "fft_length": 256,
            "hop_length": 48,
            "spectrogram_min_freq_hz": 400.0,
            "spectrogram_max_freq_hz": 1200.0,
            "spectrogram_frequency_bins": 65,
            "normalization": "log1p",
            "onnx_input_name": "spectrogram",
            "onnx_output_name": "log_probs"
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn reflect_pad_matches_numpy_semantics() {
        // np.pad([1,2,3,4], 2, mode="reflect") == [3,2,1,2,3,4,3,2]
        assert_eq!(
            reflect_pad(&[1.0, 2.0, 3.0, 4.0], 2),
            vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]
        );
    }

    #[test]
    fn hann_is_periodic_form() {
        // Periodic Hann: w[0] = 0 and w[N/2] = 1 (np.hanning(N+1)[:-1]).
        let w = hann_periodic(256);
        assert!(w[0].abs() < 1e-6);
        assert!((w[128] - 1.0).abs() < 1e-6);
        assert_eq!(w.len(), 256);
    }

    #[test]
    fn spectrogram_shape_and_tone_bin() {
        let m = meta();
        // 1 s of 600 Hz tone at 3200 Hz → frames = 1 + (3200+256-256)/48 = 67…
        let sr = 3200.0f32;
        let audio: Vec<f32> = (0..3200)
            .map(|i| (2.0 * std::f32::consts::PI * 600.0 * i as f32 / sr).sin())
            .collect();
        let (spec, frames) = spectrogram(&audio, &m);
        assert_eq!(frames, 1 + 3200 / 48);
        assert_eq!(spec.len(), frames * 65);
        // The hottest bin in a mid frame must be 600 Hz: bin 600/12.5=48 → slice index 48-32=16.
        let mid = &spec[(frames / 2) * 65..(frames / 2 + 1) * 65];
        let hottest = mid
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(hottest, 16, "600 Hz lands in slice bin 16");
    }

    #[test]
    fn greedy_ctc_collapses_blanks_and_repeats() {
        let m = meta();
        // Frames argmax: A A blank A B B blank blank C → "AABC"… collapse: A, A(new after blank), B, C
        let mk = |idx: usize| {
            let mut row = vec![-10.0f32; 4];
            row[idx] = 0.0;
            row
        };
        let seq = [0, 0, 3, 0, 1, 1, 3, 3, 2];
        let flat: Vec<f32> = seq.iter().flat_map(|&i| mk(i)).collect();
        assert_eq!(greedy_ctc(&flat, seq.len(), 4, &m), "AABC");
    }

    #[test]
    fn resample_halves_length() {
        let audio: Vec<f32> = (0..1200).map(|i| i as f32).collect();
        let out = resample_linear(&audio, 12000, 3200);
        assert_eq!(out.len(), 320);
        // Linear interp of a ramp stays a ramp.
        assert!((out[100] - 100.0 * 12000.0 / 3200.0).abs() < 1.0);
    }
}
