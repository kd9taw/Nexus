//! The APRS RX chain against audio that has been through a RADIO, not just through our own
//! modulator.
//!
//! ## Why this file exists
//! Every other APRS test in the tree is self-referential about signal shape: the modem tests feed
//! [`modulate`]'s own output straight back into the demodulator, and the parser tests start from
//! bytes. A clean synthetic tone is the one signal we can be sure works. When an operator reported
//! hearing APRS traffic that never appeared on the map, there was no test that could say whether
//! the demodulator was the problem — so this pins its tolerance to the things a real 2 m FM
//! receiver does to a Bell-202 signal:
//!
//! - **level** anywhere from a barely-open squelch to a hot, nearly-clipping codec,
//! - **DC offset / AC coupling** from the discriminator and the sound card,
//! - **de-emphasis**, the 6 dB/octave tilt that puts the 2200 Hz space tone ~5 dB below the
//!   1200 Hz mark tone (the classic way a naive AFSK slicer gets biased),
//! - **clock error** between the sender's TNC and our sample rate,
//! - **noise**, down to the point where AX.25's checksum is doing the real work.
//!
//! **TWIST** (added after the 4th field round) is the classic real-Bell-202 killer: the two tones
//! arriving at unequal amplitudes, the net of TNC pre-emphasis x TX deviation x RX de-emphasis.
//! Real-world range is roughly ±9 dB, and it is the reason Dire Wolf ships multiple demodulator
//! profiles. Our suite generated from our own modulator — perfectly flat — so it had never been
//! modelled at all. The measured answer is in `twist_*` below, and it is not what was expected.
//!
//! ⚠️ STILL A GAP: this is impaired synthetic audio, not a recording off the air. A real capture
//! (multipath, squelch crashes, colliding packets, other stations' PTT clicks) would be strictly
//! better evidence, and there is no such fixture in the tree.

use tempo_core::aprs::{
    encode_frame, modulate, nrzi_encode, Address, AprsBody, AprsPacket, Deframer, Demod, Frame,
};

const SAMPLE_RATE: f32 = 12_000.0;

fn frame() -> Frame {
    Frame::ui(
        Address::new("APZNEX", 0),
        Address::new("KD9TAW", 9),
        vec![Address::new("WIDE1", 1), Address::new("WIDE2", 1)],
        b"!4903.50N/07201.75W-off air",
    )
}

fn clean() -> Vec<f32> {
    modulate(&nrzi_encode(&encode_frame(&frame().encode(), 32, 3)))
}

/// The live RX chain exactly as `aprsrx.rs` runs it: streaming demod + deframer, fed in ~100 ms
/// drains, decoded through the AX.25 FCS. Returns the positions recovered.
fn positions(audio: &[f32]) -> Vec<(f64, f64)> {
    let mut demod = Demod::new();
    let mut deframer = Deframer::new();
    let mut out = Vec::new();
    for chunk in audio.chunks(1200) {
        for bytes in deframer.push(&demod.feed(chunk)) {
            if let Some(pkt) = AprsPacket::from_bytes(&bytes) {
                if let Some(p) = pkt.position() {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Asserts the packet came through with its position intact — decoding a frame whose coordinates
/// are wrong would put a station in the wrong place, which is worse than not plotting it.
fn assert_recovered(audio: &[f32], what: &str) {
    let got = positions(audio);
    assert_eq!(got.len(), 1, "{what}: expected exactly one position");
    assert!(
        (got[0].0 - 49.058_333).abs() < 1e-4 && (got[0].1 - (-72.029_166)).abs() < 1e-4,
        "{what}: position came through wrong: {:?}",
        got[0]
    );
}

/// Deterministic Box–Muller AWGN at a target SNR over the signal's own power.
fn awgn(sig: &mut [f32], snr_db: f32, seed: u64) {
    let p: f32 = sig.iter().map(|x| x * x).sum::<f32>() / sig.len().max(1) as f32;
    let sigma = (p / 10f32.powf(snr_db / 10.0)).sqrt();
    let mut s = seed | 1;
    let mut next = || {
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        ((s.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f32 + 1.0) / (1u64 << 53) as f32
    };
    for x in sig.iter_mut() {
        let (u1, u2) = (next(), next());
        *x += sigma * (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos();
    }
}

/// One-pole high-pass — the DC block every discriminator and sound card puts in the path.
fn highpass(audio: &[f32], fc_hz: f32) -> Vec<f32> {
    let rc = 1.0 / (std::f32::consts::TAU * fc_hz);
    let dt = 1.0 / SAMPLE_RATE;
    let a = rc / (rc + dt);
    let mut out = Vec::with_capacity(audio.len());
    let (mut py, mut px) = (0.0f32, 0.0f32);
    for &x in audio {
        let y = a * (py + x - px);
        out.push(y);
        py = y;
        px = x;
    }
    out
}

/// De-emphasis: 6 dB/octave, so 2200 Hz lands ~5 dB below 1200 Hz.
fn deemphasis(audio: &[f32]) -> Vec<f32> {
    let rc = 1.0 / (std::f32::consts::TAU * 500.0);
    let dt = 1.0 / SAMPLE_RATE;
    let a = dt / (rc + dt);
    let mut out = Vec::with_capacity(audio.len());
    let mut y = 0.0f32;
    for &x in audio {
        y += a * (x - y);
        out.push(y);
    }
    out
}

/// Resample by `ratio` — the sender's TNC clock running fast (>1) or slow (<1) against ours.
fn clock_skew(audio: &[f32], ratio: f64) -> Vec<f32> {
    let n = (audio.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let k = p as usize;
            let f = (p - k as f64) as f32;
            let a = audio.get(k).copied().unwrap_or(0.0);
            let b = audio.get(k + 1).copied().unwrap_or(0.0);
            a + (b - a) * f
        })
        .collect()
}

#[test]
fn decodes_at_any_level_a_receiver_might_deliver() {
    // A barely-open squelch through to a hot codec. The discriminator compares mark ENERGY
    // against space energy, so this should be level-independent — pin that it is.
    for gain in [0.02f32, 0.1, 0.5, 1.0, 4.0] {
        let audio: Vec<f32> = clean().iter().map(|x| x * gain).collect();
        assert_recovered(&audio, &format!("gain {gain}"));
    }
}

#[test]
fn survives_dc_offset_and_ac_coupling() {
    for dc in [0.05f32, 0.2, 0.5] {
        let audio: Vec<f32> = clean().iter().map(|x| x + dc).collect();
        assert_recovered(&audio, &format!("dc offset {dc}"));
    }
    assert_recovered(&highpass(&clean(), 300.0), "300 Hz high-pass");
}

#[test]
fn survives_receiver_de_emphasis() {
    // The tilt that biases a naive slicer toward mark. It does not bias THIS one, because only
    // one tone is present per bit, so the tilt scales both correlator outputs together.
    assert_recovered(&deemphasis(&clean()), "de-emphasis");
    assert_recovered(
        &highpass(&deemphasis(&clean()), 300.0),
        "de-emphasis + DC block",
    );
}

#[test]
fn survives_a_sender_whose_clock_disagrees_with_ours() {
    // ±0.5% is far beyond any real TNC; the timing PLL re-centres on every tone transition and
    // bit-stuffing guarantees one at least every six bits.
    for ppm in [200i32, 1000, 5000] {
        let r = 1.0 + f64::from(ppm) / 1e6;
        assert_recovered(&clock_skew(&clean(), r), &format!("+{ppm} ppm"));
        assert_recovered(&clock_skew(&clean(), 2.0 - r), &format!("-{ppm} ppm"));
    }
}

#[test]
fn survives_a_noisy_channel_and_everything_at_once() {
    let mut noisy = clean();
    awgn(&mut noisy, 10.0, 0xC0FFEE);
    assert_recovered(&noisy, "10 dB SNR");

    // The realistic composite: de-emphasised, DC-blocked, off-clock, quiet, and noisy.
    let mut worst = clock_skew(&highpass(&deemphasis(&clean()), 300.0), 1.0005);
    for x in worst.iter_mut() {
        *x *= 0.25;
    }
    awgn(&mut worst, 15.0, 0xBEEF);
    assert_recovered(&worst, "composite off-air impairments");
}

#[test]
fn a_packet_arriving_inside_a_live_channel_still_decodes() {
    // What the tap actually sees: an open squelch hissing, then a burst, then hiss again. The
    // deframer must find the frame in the middle of a continuous bit stream, not a tidy buffer.
    let mut audio = vec![0.0f32; 6000];
    audio.extend(clean().iter().map(|x| x * 0.4));
    audio.extend(std::iter::repeat_n(0.0f32, 6000));
    awgn(&mut audio, 25.0, 0x1234);
    assert_recovered(&audio, "burst inside a live channel");
}

#[test]
fn back_to_back_packets_are_both_recovered() {
    // A busy channel digipeats bursts nose to tail. Losing the second one to deframer state left
    // over from the first would quietly halve what reaches the map.
    let second = Frame::ui(
        Address::new("APZNEX", 0),
        Address::new("W1AW", 0),
        vec![],
        b"!4903.50N/07201.75W-second",
    );
    let mut audio = clean();
    audio.extend(modulate(&nrzi_encode(&encode_frame(
        &second.encode(),
        32,
        3,
    ))));
    assert_eq!(positions(&audio).len(), 2, "both packets recovered");
}

#[test]
fn a_corrupted_packet_is_rejected_rather_than_plotted_wrong() {
    // The FCS is the only thing standing between a noisy channel and a station drawn in the wrong
    // place. A bad frame must produce NOTHING, never a plausible-looking position.
    let mut bytes = frame().encode();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x20;
    let audio = modulate(&nrzi_encode(&encode_frame(&bytes, 32, 3)));
    assert!(
        positions(&audio).is_empty(),
        "a bad checksum must plot nothing"
    );
}

#[test]
fn a_digipeated_packet_keeps_the_originating_station() {
    // Nearly everything on a real 2 m channel has been through a digipeater, which sets the
    // has-been-repeated bit on the path entries. The map must still credit the ORIGINATOR.
    let mut f = frame();
    for d in f.path.iter_mut() {
        d.cbit = true; // H bit — "this digi has repeated it"
    }
    let audio = modulate(&nrzi_encode(&encode_frame(&f.encode(), 32, 3)));
    let mut demod = Demod::new();
    let mut deframer = Deframer::new();
    let mut got = None;
    for chunk in audio.chunks(1200) {
        for bytes in deframer.push(&demod.feed(chunk)) {
            if let Some(p) = AprsPacket::from_bytes(&bytes) {
                got = Some(p);
            }
        }
    }
    let pkt = got.expect("a digipeated frame must still decode");
    assert_eq!(pkt.source.call, "KD9TAW");
    assert_eq!(pkt.source.ssid, 9);
    assert!(
        pkt.path.iter().all(|d| d.cbit),
        "the used-digi marks survive"
    );
    assert!(matches!(pkt.body, AprsBody::Info(_)));
}

// ─── TWIST ───────────────────────────────────────────────────────────────────────────────────
//
// MEASURED RESULT, and it refutes the hypothesis it was written to test: this demodulator is
// essentially immune to twist on its own. Pure net tilt decodes cleanly to ±24 dB, far outside the
// ±9 dB real-world range.
//
// WHY, mechanically: the discriminator compares mark ENERGY against space energy **within one bit
// window**, and only one tone is present in a bit. A static amplitude imbalance scales both terms
// of that comparison by the same factor, so it cannot move the decision. (Worst-case inter-tone
// leakage measures -11.8 dB, and twist does not eat that margin in the steady state.) There is
// nothing here that an envelope-tracking design would do better — this architecture never had an
// envelope comparison to lose.
//
// What twist DOES cost is SNR on the weaker tone, measured:
//
//     twist    minimum reliable SNR    cost
//      0 dB          5 dB              +0
//     +3 dB          5 dB              +0
//     +6 dB          7 dB              +2
//     +9 dB          9 dB              +4
//    +12 dB         13 dB              +8
//
// So at the worst realistic twist we still decode down to 9 dB SNR — a margin an audible local
// burst clears easily. Twist is therefore NOT an explanation for 0-of-N CRC failures on a strong
// signal, and no twist-compensation stage is warranted on this evidence.

const SPB: usize = 10;

/// Modulate with TWIST — mark and space at different amplitudes, continuous phase like the real
/// modulator. The louder tone is normalised to 1.0 so peak level is constant across a sweep, which
/// keeps this a test of IMBALANCE rather than of level.
fn modulate_twisted(twist_db: f32) -> Vec<f32> {
    let bits = nrzi_encode(&encode_frame(&frame().encode(), 32, 3));
    let r = 10f32.powf(twist_db / 20.0);
    let (a_mark, a_space) = if r >= 1.0 { (1.0, 1.0 / r) } else { (r, 1.0) };
    let (d_mark, d_space) = (
        std::f32::consts::TAU * 1200.0 / SAMPLE_RATE,
        std::f32::consts::TAU * 2200.0 / SAMPLE_RATE,
    );
    let mut out = Vec::with_capacity(bits.len() * SPB);
    let mut phase = 0.0f32;
    for &lvl in &bits {
        let (step, amp) = if lvl {
            (d_mark, a_mark)
        } else {
            (d_space, a_space)
        };
        for _ in 0..SPB {
            out.push(phase.sin() * amp);
            phase += step;
            if phase >= std::f32::consts::TAU {
                phase -= std::f32::consts::TAU;
            }
        }
    }
    out
}

#[test]
fn twist_across_the_whole_real_world_range_decodes() {
    // ±9 dB is the range the TNC-test literature describes; go well past it in both directions.
    for twist in [-12.0f32, -9.0, -6.0, -3.0, 0.0, 3.0, 6.0, 9.0, 12.0] {
        assert_recovered(&modulate_twisted(twist), &format!("twist {twist:+} dB"));
    }
}

#[test]
fn twist_far_beyond_anything_physical_still_decodes() {
    // Pins the mechanism: a per-bit energy COMPARISON cannot be moved by scaling both of its
    // terms. If someone later replaces the discriminator with an envelope/threshold design, this
    // is the test that will catch the twist sensitivity it introduces.
    for twist in [-24.0f32, -18.0, 18.0, 24.0] {
        assert_recovered(
            &modulate_twisted(twist),
            &format!("extreme twist {twist:+} dB"),
        );
    }
}

#[test]
fn twist_costs_snr_on_the_weaker_tone_but_stays_usable() {
    // The real cost, and the shape of it: at the worst realistic twist we still want a decode at
    // 12 dB SNR, which any audible local burst clears.
    let mut audio = modulate_twisted(9.0);
    awgn(&mut audio, 12.0, 0xC0FFEE);
    assert_recovered(&audio, "twist +9 dB at 12 dB SNR");

    let mut audio = modulate_twisted(-9.0);
    awgn(&mut audio, 12.0, 0xBEEF);
    assert_recovered(&audio, "twist -9 dB at 12 dB SNR");
}

#[test]
fn twist_composes_with_the_rest_of_the_real_world_stack() {
    // De-emphasis already tilts the path ~5 dB; twist in the SAME direction stacks with it, and
    // net tilt is what matters. This is the one place the composition genuinely bites, so pin that
    // a realistic combination still works.
    let mut audio = highpass(&deemphasis(&modulate_twisted(6.0)), 300.0);
    for x in audio.iter_mut() {
        *x *= 0.25;
    }
    awgn(&mut audio, 20.0, 0x1234);
    assert_recovered(
        &audio,
        "twist +6 dB + de-emphasis + DC block + quiet + noise",
    );
}
