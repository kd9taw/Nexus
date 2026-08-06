//! What a `/P` (and `/R`) station actually puts on the air — a ROUND TRIP, not a
//! string comparison.
//!
//! Reported on air by F4CYH/P: "the locator and the reports are INOP when the /P is
//! in the call." His TX line read `CQ  F4CYH/P` with no grid, and he never sent a
//! numeric report in either direction.
//!
//! `/P` and `/R` are the two suffixes the 77-bit protocol carries NATIVELY — a
//! dedicated `p1`/`r1` bit in the Type 1/2 layouts (`c28 p1 c28 p1 R1 g15`), which
//! keep BOTH a grid and a numeric report. WSJT-X's own `MainWindow::stdCall()`
//! (`widgets/mainwindow.cpp:6627`) accepts them, and `genCQMsg`/`genStdMsgs` then
//! take the plain grid/report branch. Nexus instead treated any slash as compound
//! and routed the whole QSO down i3=4, which carries NEITHER field — so the grid and
//! the report were dropped by us, before the modem ever saw them.
//!
//! Every assertion here goes through the real modem: build the sequencer's over,
//! pack it (`pack77`, the vendored WSJT-X Fortran), transmit it as a waveform,
//! decode the frame, and assert on the text that comes BACK. A string comparison
//! against `Msg::to_text()` cannot see this bug class at all — the packer silently
//! drops a field it has no slot for (and, for a genuinely nonstandard call, silently
//! drops the prefix and hands the partner a well-formed message naming a DIFFERENT
//! station). Only the recovered text proves what a partner's receiver produces.

use modes::{make_mode, DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::message::Msg;
use tempo_core::qso::Station;

const FS: f32 = 12_000.0;
const F0: f32 = 1500.0;

/// Pack `text`, transmit it, decode the clean frame, and return every message the
/// receiver recovered. This — not what we intended to send — is the assertion surface.
fn on_air(kind: ModeKind, text: &str) -> Vec<String> {
    let mode = make_mode(kind);
    let tones = mode.encode(text);
    assert!(
        !tones.is_empty(),
        "{} could not pack '{text}' at all (nsym < 0)",
        kind.as_str()
    );
    let wave = mode.gen_wave(&tones, FS, F0);
    let mut iwave = vec![0i16; mode.frame_samples()];
    for (i, &s) in wave.iter().enumerate() {
        if i < iwave.len() {
            iwave[i] = (s * 1000.0).clamp(-32768.0, 32767.0) as i16;
        }
    }
    let mut src = NativeSource::from_kind(kind);
    src.decode(&DecodeRequest::full_band(&iwave))
        .into_iter()
        .map(|d| d.message)
        .collect()
}

/// Assert `text` survives the modem VERBATIM. A frame that decodes to something
/// else is the dangerous case: the partner copies a well-formed wrong message.
#[track_caller]
fn assert_on_air(kind: ModeKind, text: &str) {
    let got = on_air(kind, text);
    assert!(
        got.iter().any(|m| m == text),
        "{} sent [{text}] but the receiver recovered {got:?}",
        kind.as_str()
    );
}

/// One over: the sequencer's next transmission must be `expect`, AND `expect` must
/// come back off the air unchanged. Then mark it sent.
#[track_caller]
fn over(st: &mut Station, expect: &str) {
    let sent = st
        .pending_text()
        .unwrap_or_else(|| panic!("expected an over [{expect}], but nothing was pending"));
    assert_eq!(sent, expect, "wrong message queued");
    assert_on_air(ModeKind::Ft8, &sent);
    st.after_tx();
}

fn dec(text: &str, snr: i32) -> modes::Decode {
    modes::Decode {
        message: text.into(),
        sync: 5.0,
        snr,
        dt: 0.1,
        freq: F0,
        nap: 0,
        qual: 1.0,
        rv: None,
        mode: Some(ModeKind::Ft8),
    }
}

// ---------------------------------------------------------------------------
// /P and /R — protocol-standard, and every field survives
// ---------------------------------------------------------------------------

#[test]
fn a_portable_cq_puts_the_grid_on_the_air() {
    // THE REPORTED BUG, half one. `CQ F4CYH/P` (i3=4) carries no grid at all;
    // `CQ F4CYH/P JN18` is a Type 2 frame that carries it in the clear.
    let st = Station::calling_cq("F4CYH/P", "JN18");
    assert_eq!(st.pending_text().as_deref(), Some("CQ F4CYH/P JN18"));
    assert_on_air(ModeKind::Ft8, "CQ F4CYH/P JN18");

    // /R is the other natively-carried suffix (Type 1's r1 bit).
    let st = Station::calling_cq("KD9TAW/R", "EN52");
    assert_eq!(st.pending_text().as_deref(), Some("CQ KD9TAW/R EN52"));
    assert_on_air(ModeKind::Ft8, "CQ KD9TAW/R EN52");
}

#[test]
fn a_portable_station_running_cq_exchanges_a_numeric_report() {
    // THE REPORTED BUG, half two: F4CYH/P's Tx2 was `<W9XYZ> F4CYH/P` — rendering
    // IDENTICALLY to his Tx1, so the partner could not tell "answering you" from
    // "here is your report", and no number ever went out.
    let mut st = Station::calling_cq("F4CYH/P", "JN18");
    over(&mut st, "CQ F4CYH/P JN18");

    st.observe(&[dec("F4CYH/P W9XYZ EN52", -5)]);
    over(&mut st, "W9XYZ F4CYH/P -05");

    st.observe(&[dec("F4CYH/P W9XYZ R-08", -5)]);
    over(&mut st, "W9XYZ F4CYH/P RR73");

    // The partner's 73 closes it; the CQ runner sends nothing further (WSJT-X parity).
    st.observe(&[dec("F4CYH/P W9XYZ 73", -5)]);
    assert!(st.done(), "QSO complete: {:?}", st.transcript);
}

#[test]
fn a_portable_station_answering_a_cq_exchanges_a_numeric_report() {
    // The other role: his grid rides Tx1 and his rogered report rides Tx3, where
    // before both were replaced by the grid-less, number-less i3=4 call.
    let mut st = Station::answering("F4CYH/P", "JN18", "W9XYZ");
    over(&mut st, "W9XYZ F4CYH/P JN18");

    st.observe(&[dec("F4CYH/P W9XYZ -08", -8)]);
    over(&mut st, "W9XYZ F4CYH/P R-08");

    st.observe(&[dec("F4CYH/P W9XYZ RR73", -8)]);
    over(&mut st, "W9XYZ F4CYH/P 73");
    assert!(st.done(), "QSO complete: {:?}", st.transcript);
}

#[test]
fn a_standard_station_works_a_portable_dx_in_the_clear() {
    // The other side of the same QSO. Before, this station hashed its partner
    // (`<F4CYH/P> KD9TAW`) and lost its own grid — and put `<F4CYH/P>`, brackets
    // and all, into the log, the roster and PSKReporter.
    let mut st = Station::answering("KD9TAW", "EN52", "F4CYH/P");
    over(&mut st, "F4CYH/P KD9TAW EN52");

    st.observe(&[dec("KD9TAW F4CYH/P -08", -8)]);
    over(&mut st, "F4CYH/P KD9TAW R-08");

    st.observe(&[dec("KD9TAW F4CYH/P RR73", -8)]);
    over(&mut st, "F4CYH/P KD9TAW 73");
    assert_eq!(st.dxcall.as_deref(), Some("F4CYH/P"), "logs the full call");
}

#[test]
fn portable_messages_ride_ft4_on_the_same_packer() {
    // FT4 shares pack77, so the fix must hold there too (same `Ft8Mode::encode`
    // path into `genft8`/`genft4` → `pack77`).
    for text in [
        "CQ F4CYH/P JN18",
        "W9XYZ F4CYH/P JN18",
        "W9XYZ F4CYH/P +03",
        "W9XYZ F4CYH/P R-08",
    ] {
        assert_on_air(ModeKind::Ft4, text);
    }
}

// ---------------------------------------------------------------------------
// Genuinely nonstandard calls — the predicate must stay STRICT for these
// ---------------------------------------------------------------------------

#[test]
fn a_bare_nonstandard_call_is_never_transmitted() {
    // Why the predicate cannot simply be widened to "any slash". `chkcall` accepts
    // PJ4/K1ABC and returns the base K1ABC; `pack77_1` then packs K1ABC and drops
    // the prefix SILENTLY, with no error — the partner copies a well-formed message
    // naming a DIFFERENT station. This pins the hazard so nobody re-widens it.
    let got = on_air(ModeKind::Ft8, "CQ PJ4/K1ABC FK52");
    assert!(
        !got.iter().any(|m| m == "CQ PJ4/K1ABC FK52"),
        "the packer cannot carry a bare nonstandard call; got {got:?}"
    );
    assert!(
        got.iter().any(|m| m == "CQ K1ABC FK52"),
        "…it silently becomes a DIFFERENT station: {got:?}"
    );

    // So the sequencer must never build one: the hashed form goes out instead.
    let cq = Msg::parse("CQ PJ4/K1ABC");
    let st = Station::start(
        "KD9TAW",
        "EN52",
        "PJ4/K1ABC",
        Some((&cq, -10)),
        false,
        false,
    );
    let tx1 = st.pending_text().expect("Tx1");
    assert!(
        !tx1.contains("PJ4/K1ABC ") && tx1.contains("<PJ4/K1ABC>"),
        "the nonstandard DX must be hashed, not sent bare: [{tx1}]"
    );
}

#[test]
fn a_nonstandard_call_still_gets_its_report_through_the_hash() {
    // The 22-bit hash in Type 1 keeps the numeric report alive for a genuinely
    // nonstandard call — only the GRID is protocol-mandated away, and only where the
    // nonstandard station is the sender. Prime the packer's hash table by sending the
    // full call first, exactly as a real QSO does.
    let _ = on_air(ModeKind::Ft8, "CQ PJ4/K1ABC");

    let cq = Msg::parse("CQ PJ4/K1ABC");
    let mut st = Station::start(
        "KD9TAW",
        "EN52",
        "PJ4/K1ABC",
        Some((&cq, -10)),
        false,
        false,
    );
    // Tx1: DX hashed, my call the standard c28 — so MY grid still rides along.
    over(&mut st, "<PJ4/K1ABC> KD9TAW EN52");

    // The nonstandard DX answers grid-less (i3=4 has no grid field for it).
    st.observe(&[dec("<KD9TAW> PJ4/K1ABC", -7)]);
    // My report survives: I am the c28 sender, the DX is the hash.
    over(&mut st, "<PJ4/K1ABC> KD9TAW -07");

    st.observe(&[dec("<KD9TAW> PJ4/K1ABC RR73", -7)]);
    over(&mut st, "<PJ4/K1ABC> KD9TAW 73");
    assert_eq!(
        st.dxcall.as_deref(),
        Some("PJ4/K1ABC"),
        "logs the full call"
    );
}

#[test]
fn a_hashed_cq_is_never_generated() {
    // `CQ <F4CYH/P> JN18` PACKS but does not UNPACK — a hashed CQ is unusable, and
    // generating one would put a frame on the air that nobody can decode. A CQ
    // therefore always carries the calling station's call in full.
    let got = on_air(ModeKind::Ft8, "CQ <PJ4/K1ABC> FK52");
    assert!(
        got.is_empty(),
        "a hashed CQ does not survive the round trip; got {got:?}"
    );

    for mycall in ["F4CYH/P", "KD9TAW/R", "PJ4/K1ABC", "YW18FIFA", "KD9TAW/QRP"] {
        let cq = Station::calling_cq(mycall, "EN52")
            .pending_text()
            .expect("a CQ is always pending");
        assert!(
            !cq.contains('<') && !cq.contains('>'),
            "CQ for {mycall} must not be hashed: [{cq}]"
        );
    }
}
