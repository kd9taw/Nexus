use modes::{make_mode, DecodeRequest, ModeKind, NativeSource, SignalSource};

const FS: f32 = 12000.0;

fn frame_at(f0: f32) -> Vec<i16> {
    let mode = make_mode(ModeKind::TempoFast);
    let tones = mode.encode("CQ KD9TAW EN52");
    let wave = mode.gen_wave(&tones, FS, f0);
    let n = tempo_fast::NMAX;
    let mut f = vec![0i16; n];
    for (i, s) in wave.iter().take(n).enumerate() {
        f[i] = (s * 16000.0).clamp(-32768.0, 32767.0) as i16;
    }
    f
}

fn decodes_at(f0: f32, nfa: i32, nfb: i32) -> bool {
    let frame = frame_at(f0);
    let mut src = NativeSource::from_kind(ModeKind::TempoFast);
    let mut req = DecodeRequest::full_band(&frame);
    req.nfa = nfa;
    req.nfb = nfb;
    src.decode(&req).iter().any(|d| d.message == "CQ KD9TAW EN52")
}

#[test]
fn does_the_window_follow_nfqso() {
    // Each row uses a different nfa/nfb, so FT1's C ABI forces a DIFFERENT
    // nfqso = (nfa+nfb)/2. If the decodable window tracks that midpoint, nfqso
    // is steering the decode. If it stays put, something else is.
    for (nfa, nfb) in [(200, 2900), (200, 1400), (1600, 2900), (2000, 2900)] {
        let mid = (nfa + nfb) / 2;
        let mut ok = Vec::new();
        let mut f0 = 400.0f32;
        while f0 <= 2800.0 {
            if decodes_at(f0, nfa, nfb) { ok.push(f0 as i32); }
            f0 += 50.0;
        }
        println!("nfa={nfa:4} nfb={nfb:4} -> forced nfqso={mid:4} | decodes at f0 = {ok:?}");
    }
}
