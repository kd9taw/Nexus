//! The AI CW decode thread (beta) — feeds 15 s windows of the engine's AI-CW audio ring
//! through the DeepCW model (see the `deepcw` crate) and pushes each window's text into
//! the engine for the CW cockpit's side panel.
//!
//! Design constraints:
//! - The decode costs ~seconds of CPU, so it runs on its OWN thread; engine locks are
//!   held only for the brief window copy and the result push.
//! - The model is AGPL-3.0 (© e04) and ships as an app resource, NOT in this repo; if
//!   it's missing the panel says so and the thread naps — nothing else is affected.
//! - Gated on `Settings::ai_cw_active()` + the CW operating mode: off = zero work
//!   (the engine's ring stays empty too). `ai_cw_active` is the operator's
//!   `ai_cw_enabled` MINUS the Unassisted-mode override, so declaring an unassisted
//!   contest entry runs no inference at all — read it, never the raw setting.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_app::engine::{engine_lock, Engine};
use tempo_app::settings::OperatingMode;

use crate::aicw_stitch::{pass_gap, Stitcher};
use crate::service::SHUTDOWN;

/// How often the loop re-checks the enable/mode gates while idle. While decoding, passes
/// run back to back with [`pass_gap`]'s rest between them: the stitch emits only characters
/// that are new since the previous pass, so frequent passes mean small batches that trail
/// the key-up by about the tail guard plus one pass, without changing a decoded character.
const IDLE_POLL: Duration = Duration::from_millis(250);
/// A pass this slow is logged every time (the first pass's time is always logged).
const SLOW_PASS: Duration = Duration::from_secs(2);
/// Re-attempt a failed model load this often (the operator may install it mid-session).
const MODEL_RETRY: Duration = Duration::from_secs(30);

/// Spawn the decode thread. `model_dir` holds `model.onnx` (pre-folded for the 15 s
/// window) + `model.onnx.json`.
pub fn spawn_ai_cw(engine: Arc<Mutex<Engine>>, model_dir: std::path::PathBuf) {
    std::thread::Builder::new()
        .name("ai-cw".into())
        .spawn(move || run(engine, model_dir))
        .expect("spawn ai-cw");
}

/// Drop the calling thread below normal priority: the radio loop and the audio callbacks
/// run at normal or above, and an inference pass must never delay them.
#[cfg(windows)]
fn lower_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: the pseudo-handle of the calling thread; failure leaves the priority as it was.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

fn run(engine: Arc<Mutex<Engine>>, model_dir: std::path::PathBuf) {
    #[cfg(windows)]
    lower_priority();
    let mut model: Option<deepcw::DeepCw> = None;
    let mut last_model_try: Option<Instant> = None;
    // The rest before the next loop turn: the idle poll, or `pass_gap` after a pass.
    let mut pause = IDLE_POLL;
    let mut logged_pass_time = false;
    // The transcript cursor (see `aicw_stitch`).
    let mut stitch = Stitcher::default();
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(pause);
        pause = IDLE_POLL;
        // Gates: feature EFFECTIVELY on (`ai_cw_active` folds in Unassisted mode, so an
        // unassisted contest entry runs no inference at all) + CW cockpit active (brief lock).
        let on = {
            let e = engine_lock(&engine);
            e.settings().ai_cw_active() && e.settings().operating_mode == OperatingMode::Cw
        };
        if !on {
            stitch.reset(); // ring + stream clock reset when the feature is off
            continue;
        }
        // Lazy model load, with a retry backoff and an honest status.
        if model.is_none() {
            let due = last_model_try.is_none_or(|t| t.elapsed() >= MODEL_RETRY);
            if !due {
                continue;
            }
            last_model_try = Some(Instant::now());
            match deepcw::DeepCw::load(&model_dir) {
                Ok(m) => {
                    model = Some(m);
                    set_status(&engine, "listening…");
                }
                Err(e) => {
                    eprintln!("ai-cw: model unavailable: {e}");
                    set_status(&engine, "model not installed");
                    continue;
                }
            }
        }
        // A 15 s window (zero-padded while the ring is still filling) + the absolute stream
        // position of its end + the stream's generation, copied under a brief lock; decode
        // runs off-lock.
        let window = {
            let e = engine_lock(&engine);
            e.ai_cw_window()
        };
        let Some((window, fed, epoch)) = window else {
            set_status(&engine, "listening…");
            continue;
        };
        let win_secs = window.len() as f64 / 12_000.0;
        let end_abs = fed as f64 / 12_000.0;
        let ai = model.as_ref().unwrap();
        let audio_3200 = deepcw::resample_linear(&window, 12_000, ai.meta.sample_rate);
        let pass_t0 = Instant::now();
        let decoded = ai.decode_timed(&audio_3200);
        let last_pass = pass_t0.elapsed();
        pause = pass_gap(last_pass);
        // One log line for the record (the pass time was never measured before), plus
        // any very slow pass — silence otherwise.
        if !logged_pass_time || last_pass > SLOW_PASS {
            eprintln!("ai-cw: inference pass took {:.2}s", last_pass.as_secs_f64());
            logged_pass_time = true;
        }
        match decoded {
            Ok(chars) => {
                let pass = stitch.pass(epoch, end_abs, win_secs, &chars);
                let mut e = engine_lock(&engine);
                e.set_ai_cw_status("");
                // Every batch, a lone word space included: small batches carry the spaces
                // between words on their own. The provisional tail replaces the last one.
                // Dropped by the engine if a QSY started a new stream during the pass.
                e.push_ai_cw_pass(epoch, &pass.confirmed, &pass.pending);
                // …and the same letters with the time each was heard, for the CW turn cue.
                e.push_ai_cw_times(epoch, end_abs, &pass.confirmed_at, &pass.pending_at);
            }
            Err(e) => {
                eprintln!("ai-cw: decode failed: {e}");
                set_status(&engine, "decode error (see log)");
                // Drop the model so the next attempt reloads clean (a poisoned plan
                // cache or a swapped-out resource dir both heal this way).
                model = None;
            }
        }
    }
}

fn set_status(engine: &Arc<Mutex<Engine>>, s: &str) {
    engine_lock(engine).set_ai_cw_status(s);
}
