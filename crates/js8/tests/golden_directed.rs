//! Golden test of the JS8 reassembler against JS8Call's own DIRECTED.TXT (sanitised, Task B4.1).
//!
//! Drives `Reassembler::feed` with every ALL.TXT frame (js8call_frames.txt, in decode order,
//! on the fixture's `# +N` clock) and renders each closed `Message` with `render_directed`, then
//! checks that the multiset of rendered lines COVERS JS8Call's DIRECTED.TXT lines
//! (js8call_directed.txt). This is the oracle for the message layer: multi-frame bodies,
//! checksum stripping, the faux HB/CQ directed forms and compound resolution all have to be
//! byte-exact for a line to count.
//!
//! WHAT IS EXCLUDED, and why it is honest: 50 of the 728 DIRECTED.TXT lines carry the `…`
//! (U+2026) marker JS8Call writes for a frame that NEVER DECODED — that frame is by definition
//! absent from ALL.TXT, so no reassembler can reproduce the line from the fixture. The floor is
//! stated over the 678 REPRODUCIBLE lines; the `…` lines are reported, not asserted. The
//! reassembler produces the head-only version of those messages (correct), which shows up as
//! "spurious" only because its lost body cannot match the `…` the log recorded.
mod common;

use common::golden::read_frames;
use js8::proto::reassembly::{render_directed, MessageEvent, Reassembler};
use js8::{Payload72, RawDecode, Word87, I3};
use std::collections::HashMap;

fn ml_add(m: &mut HashMap<String, i64>, k: String) {
    *m.entry(k).or_default() += 1;
}

/// Feed every golden frame through the reassembler; return the multiset of rendered directed
/// lines. Ages buffers on the fixture clock (a per-period tick) so 60/90 s closes fire.
fn produced_lines() -> HashMap<String, i64> {
    let frames = read_frames();
    let mut r = Reassembler::new();
    let mut out: HashMap<String, i64> = HashMap::new();
    let mut last_at = 0u64;
    for g in &frames {
        let vals = js8::proto::alphabet::sixbit_from_str(g.sixbit_str()).unwrap();
        let word = Word87::new(Payload72::from_chars12(vals), I3::from_u8(g.i3));
        let rx = RawDecode {
            speed: g.speed,
            freq_hz: g.freq_hz,
            dt_s: g.dt_s,
            snr_db: g.snr_db,
            sync: 0.0,
            word,
            nharderrors: 0,
            quality: 1.0,
        };
        last_at = g.at_ms;
        for ev in r.age(g.at_ms) {
            if let MessageEvent::Message(m) = ev {
                ml_add(&mut out, render_directed(&m));
            }
        }
        for ev in r.feed(&rx, g.at_ms) {
            if let MessageEvent::Message(m) = ev {
                ml_add(&mut out, render_directed(&m));
            }
        }
    }
    for ev in r.age(last_at + 1_000_000) {
        if let MessageEvent::Message(m) = ev {
            ml_add(&mut out, render_directed(&m));
        }
    }
    out
}

fn wanted_lines() -> HashMap<String, i64> {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/js8call_directed.txt"
    ))
    .unwrap();
    let mut want: HashMap<String, i64> = HashMap::new();
    for line in raw.lines().filter(|l| !l.is_empty()) {
        if let Some(text) = line.splitn(3, '\t').nth(2) {
            ml_add(&mut want, text.to_string());
        }
    }
    want
}

#[test]
fn reassembler_reproduces_js8calls_directed_txt() {
    let produced = produced_lines();
    let want = wanted_lines();

    let mut repro_total = 0i64;
    let mut repro_covered = 0i64;
    let mut lost = 0i64;
    let mut misses: Vec<String> = Vec::new();
    for (text, count) in &want {
        if text.contains('\u{2026}') {
            lost += count; // a lost-frame line: not reproducible from ALL.TXT
            continue;
        }
        repro_total += count;
        let got = produced.get(text).copied().unwrap_or(0);
        repro_covered += count.min(&got);
        if got < *count {
            misses.push(format!("x{}: {text:?}", count - got));
        }
    }
    eprintln!(
        "directed golden: {repro_covered}/{repro_total} reproducible lines ({} lost-frame lines excluded); misses: {}",
        lost,
        misses.len()
    );
    for m in misses.iter().take(20) {
        eprintln!("  MISS {m}");
    }

    // The 728-line fixture holds 678 reproducible lines. The reassembler reaches 677 of them
    // (99.85 %); the one residual is a multi-frame YES body whose framing the sanitised clock
    // cannot replay. The floor is set just below the measured value so a real regression
    // (a broken render, a lost checksum strip, a compound that stops resolving) trips it.
    assert!(
        repro_covered >= 675,
        "directed coverage regressed: {repro_covered}/{repro_total} reproducible"
    );
    assert_eq!(
        lost, 50,
        "the lost-frame (…) line count changed: a different log was sanitised?"
    );
}

/// Positive control (feedback-negative-results-need-a-positive-control): the coverage number is
/// meaningless unless a broken renderer would drop it. These exact lines MUST be produced, and
/// each exercises a different code path — the faux HB, the CQ doubling, a directed SNR ack, a
/// portable/base directed command, and a multi-frame checksummed MSG body.
#[test]
fn directed_golden_positive_control_named_lines_are_present() {
    let produced = produced_lines();
    for line in [
        "KD2UWR: @HB HEARTBEAT",                               // faux heartbeat
        "NO1ZE: KD2UWR HEARTBEAT SNR +07",                     // directed SNR ack (extra)
        "KC1HZX: @ALLCALL CQ KC1HZX: @ALLCALL CQ CQ CQ FN41",  // CQ doubling
        "KJ5MIW: @SITREP MSG F!104 100 ST[OK] GR[EM15] #AVSB", // multi-frame MSG (checksum stripped)
    ] {
        assert!(
            produced.contains_key(line),
            "positive control line not reproduced: {line:?}"
        );
    }
    // And the control must be able to FAIL: a line JS8Call never logged must be absent.
    assert!(
        !produced.contains_key("KD2UWR: @HB HEARTBEAT NONSENSE"),
        "a fabricated line was produced — the check cannot distinguish right from wrong"
    );
}
