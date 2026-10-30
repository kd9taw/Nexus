//! The decode request's `activity` reaches WSJT-X's FT8 rover (`/R`) and `TU; ` filter through
//! every native entry: the plain decode, the a7 boundary pass, the streaming early and full passes
//! the engine runs, and the mode's own streaming decode. Without an activity a rover is dropped;
//! with one (Contest mode, or a Hound) it decodes.

use modes::{DecodeRequest, DecodeSink, ModeKind, NativeSource, SignalSource};

const NMAX: usize = 180_000;
const ROVER: &str = "KD9TAW W1AW/R R FN42";

/// `ROVER` at 1500 Hz, DT 0, well above the noise.
fn frame() -> Vec<i16> {
    let mut x = vec![0f32; NMAX];
    let mut s = 0x0A0B_0C0D_u64;
    for v in x.iter_mut() {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *v = ((s >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 4.0;
    }
    let w = ft8::gen_wave(&ft8::encode(ROVER), ft8::SAMPLE_RATE, 1500.0);
    let o = (0.5 * ft8::SAMPLE_RATE) as usize;
    for (i, v) in w.iter().enumerate() {
        if let Some(slot) = x.get_mut(o + i) {
            *slot += v;
        }
    }
    x.iter()
        .map(|v| (v * 1000.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn req(iwave: &[i16], activity: bool, partial: bool) -> DecodeRequest<'_> {
    DecodeRequest {
        iwave,
        nfa: 200,
        nfb: 2900,
        ndepth: 3,
        mycall: "KD9TAW",
        hiscall: "",
        nqso_progress: 0,
        nfqso: 1500,
        nftx: 1500,
        frame_time_ms: 15_000,
        ap: true,
        ap_cq_only: false,
        activity,
        partial,
    }
}

#[derive(Default)]
struct Found(Vec<String>);
impl DecodeSink for Found {
    fn found(&mut self, d: &modes::Decode) {
        self.0.push(d.message.trim().to_string());
    }
}

/// What each native entry returns for the rover frame, and (streaming) what it streamed.
fn entries(activity: bool) -> Vec<(&'static str, Vec<String>)> {
    let all = frame();
    let mut early = vec![0i16; NMAX];
    early[..41 * 3456].copy_from_slice(&all[..41 * 3456]);
    let text = |v: Vec<modes::Decode>| -> Vec<String> {
        v.into_iter()
            .map(|d| d.message.trim().to_string())
            .collect()
    };
    let mut out = Vec::new();
    modes::reset_ft8_a7();
    let mut src = NativeSource::from_kind(ModeKind::Ft8);
    out.push(("decode", text(src.decode(&req(&all, activity, false)))));
    modes::reset_ft8_a7();
    let mut src = NativeSource::from_kind(ModeKind::Ft8);
    out.push((
        "decode_a7",
        text(src.decode_a7(&req(&all, activity, false), true)),
    ));
    for (name, iwave, partial) in [
        ("streaming early pass", &early, true),
        ("streaming full pass", &all, false),
    ] {
        modes::reset_ft8_a7();
        let mut src = NativeSource::from_kind(ModeKind::Ft8);
        let mut sink = Found::default();
        let mut got =
            text(src.decode_streaming(&req(iwave, activity, partial), !partial, &mut sink));
        got.extend(sink.0);
        out.push((name, got));
    }
    modes::reset_ft8_a7();
    let src = NativeSource::from_kind(ModeKind::Ft8);
    let mut sink = Found::default();
    let mut got = text(src.mode().decode_frame_streaming(
        &all, 200, 2900, 3, "KD9TAW", "", 0, 1500, 1500, 15_000, true, false, true, false,
        activity, &mut sink,
    ));
    got.extend(sink.0);
    out.push(("mode streaming decode", got));
    modes::reset_ft8_a7();
    out
}

#[test]
fn every_native_entry_drops_a_rover_outside_an_activity_and_keeps_it_in_one() {
    for (entry, got) in entries(true) {
        assert!(
            got.iter().any(|m| m == ROVER),
            "{entry} in an activity must decode the rover, got {got:?}"
        );
    }
    for (entry, got) in entries(false) {
        assert!(
            got.iter().all(|m| !m.contains("/R")),
            "{entry} with no activity must drop the rover, got {got:?}"
        );
    }
}
