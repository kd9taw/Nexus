//! MSK144 heard on the air reaches Band Activity (#579).
//!
//! The scene runs the real `RadioLoop::step` on the wall clock, as `run_radio` does
//! (`now_unix_ms`), and feeds it audio the way an Icom's USB codec delivers it: at 48 kHz,
//! through the capture route's resampler, 20 ms at a time. The loop numbers its periods from the
//! Unix epoch; every MSK144 test before this one started its clock at zero, which is how a label
//! the decoder cannot print (see `Msk144Mode::decode_frame`) dropped every live decode unseen.

use super::*;

/// 2025-11-27 12:00:00 UTC in Unix milliseconds, on a 15 s boundary.
const WALL_CLOCK_MS: f64 = 1_764_244_800_000.0;

/// Two stations heard off meteor trails in one period.
const PINGS: [(&str, f64); 2] = [("CQ K1ABC FN42", 3.1), ("CQ W4XYZ EM84", 9.7)];

/// One 15 s period of light noise holding each of `PINGS` at its time: three 72 ms frames of
/// the station's over at 1500 Hz, a meteor ping's length.
fn period_with_pings() -> Vec<f32> {
    let mode = tempo_core::modes::make_mode(tempo_core::modes::ModeKind::MSK144_15);
    let mut x = vec![0f32; 180_000];
    let mut s = 0x5EED_0579_u64;
    for v in x.iter_mut() {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *v = ((s >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.01;
    }
    for (msg, at_s) in PINGS {
        let over = mode.gen_wave(&mode.encode(msg), 12_000.0, 1_500.0);
        let o = (at_s * 12_000.0) as usize;
        for (v, p) in x[o..].iter_mut().zip(&over[..3 * 864]) {
            *v += 0.02 * p;
        }
    }
    x
}

/// `x` (12 kHz) at the Icom codec's 48 kHz, then back through the capture route's resampler in
/// 20 ms device callbacks, as cpal hands them over.
fn through_a_48k_codec(x: &[f32]) -> Vec<f32> {
    let mut up = crate::capture_resample::CaptureResampler::new(12_000, 48_000);
    let mut down = crate::capture_resample::CaptureResampler::new(48_000, 12_000);
    up.process(x)
        .chunks(960)
        .flat_map(|c| down.process(c))
        .collect()
}

/// ⭐ #579: pings in a receive period reach Band Activity. The period after a quiet one carries
/// them; the loop runs on through the next boundary and lets every decode it started land.
#[test]
fn msk144_pings_heard_through_a_48k_codec_reach_band_activity() {
    let engine = wsjt_engine(Tier::Msk144);
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    let mut station = StationSinks::new();

    let mut air = vec![0f32; 180_000];
    air.extend(through_a_48k_codec(&period_with_pings()));
    air.resize(540_000, 0.0);

    let mut now = WALL_CLOCK_MS;
    for chunk in air.chunks(240) {
        // A computer that keeps up: each decode lands before the next 20 ms of audio, so
        // every pass the schedule asks for runs (the pacing is not what is under test).
        let started = std::time::Instant::now();
        while state.decode_in_flight && started.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            tick(
                &mut state,
                &engine,
                &mut backend,
                &mut rig,
                &mut station,
                now,
            );
        }
        backend.queue_capture(chunk.to_vec());
        tick(
            &mut state,
            &engine,
            &mut backend,
            &mut rig,
            &mut station,
            now,
        );
        now += 20.0;
    }
    assert!(
        drain_decode(
            &mut state,
            &engine,
            &mut backend,
            &mut rig,
            &mut station,
            now,
            now + 10_000.0
        ),
        "every decode the loop started landed"
    );

    let e = engine.lock().unwrap();
    let heard: Vec<String> = e
        .band_activity()
        .map(|(_, d)| d.message.trim().to_string())
        .collect();
    for (ping, _) in PINGS {
        assert!(
            heard.iter().any(|m| m == ping),
            "{ping} reached Band Activity; heard {heard:?}"
        );
    }
}
