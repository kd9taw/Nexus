//! A decode pane's refill (#452, [`Engine::decode_refill`]): the received decodes of the periods a
//! pane missed, from the history the engine already keeps, drawn as the live feed drew them.
//!
//! The snapshot's feed is the current period only, and our own overs ride a 30-deep ring re-sent
//! on every poll, so a pane that stopped taking the feed in (closed, or a window the OS stopped
//! painting) came back showing our report and RR73 to a station whose calls it never drew. The
//! refill is a READ: nothing that keys the transmitter may see the station any differently for it.

use super::*;

fn decode(message: &str, freq: f32, snr: i32) -> modes::Decode {
    modes::Decode {
        message: message.into(),
        sync: 10.0,
        snr,
        dt: 0.1,
        freq,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: Some(modes::ModeKind::Ft8),
    }
}

/// The CQ run of the report: our CQ, a caller far off our RX offset, its R-report, a bystander.
fn period(slot: u64) -> Vec<modes::Decode> {
    match slot {
        3 => vec![
            decode("KN0BLE KF9UG EN71", 1228.0, -16),
            decode("CQ W9WHF EM65", 900.0, -8),
        ],
        5 => vec![decode("KN0BLE KF9UG R-11", 1228.0, -14)],
        _ => vec![decode(&format!("CQ K{slot}AB EN52"), 600.0, -20)],
    }
}

fn cq_run() -> Engine {
    let mut e = Engine::new("KN0BLE", "EL98", 0);
    e.set_tier(Tier::Ft8);
    e.start_cq(None).expect("premise: the CQ run starts");
    e.set_tx_enabled(true);
    e.set_rx_offset(2601.0);
    e
}

/// The snapshot as text, but for `log_tick` (a per-logbook counter two separate stations never
/// share), the overs' wall-clock stamps and the roster's order. Text, not `==`: a reading with no value yet is a NaN, which equals nothing, itself
/// included.
fn snap(e: &Engine) -> String {
    let mut s = e.snapshot();
    s.log_tick = 0;
    // An over's row carries the wall-clock second it was planned in, and the two stations plan
    // theirs a moment apart.
    for r in s.recent_decodes.iter_mut() {
        r.tx_at = None;
    }
    // The roster is a hash map, whose order differs between two stations.
    s.stations.sort_by(|a, b| a.call.cmp(&b.call));
    format!("{s:?}")
}

fn over_text(e: &mut Engine, slot: u64) -> Option<String> {
    e.plan_tx(slot).map(|p| match p.waveform {
        TxWaveform::Deep { text, .. }
        | TxWaveform::Fast { text, .. }
        | TxWaveform::Native { text, .. } => text,
        TxWaveform::Js8 { .. } => "<js8>".into(),
    })
}

/// A pane that stopped taking the feed in after slot 1 and came back at slot 6 gets every row the
/// engine heard in between, the caller's to-us rows among them, each exactly the row the live feed
/// drew for it at the time.
#[test]
fn a_pane_back_after_three_periods_gets_the_rows_it_missed() {
    let mut e = cq_run();
    let mut live: Vec<(u64, crate::dto::DecodeRow)> = Vec::new();
    for slot in 1..=6u64 {
        e.fold_decodes(None, period(slot), slot, false);
        let snap = e.snapshot();
        live.extend(
            snap.recent_decodes
                .into_iter()
                .filter(|r| !r.mine)
                .map(|r| (slot, r)),
        );
    }
    let refill = e.decode_refill(2, 6);
    let got: Vec<(u64, crate::dto::DecodeRow)> =
        refill.rows.into_iter().map(|r| (r.slot, r.row)).collect();
    let want: Vec<_> = live.into_iter().filter(|(s, _)| *s >= 2).collect();
    assert_eq!(got, want, "the missed rows, as the live feed drew them");
    let to_us: Vec<&str> = got
        .iter()
        .filter(|(_, r)| r.directed_to_me)
        .map(|(_, r)| r.message.as_str())
        .collect();
    assert_eq!(
        to_us,
        ["KN0BLE KF9UG EN71", "KN0BLE KF9UG R-11"],
        "the caller's calls to us, off our RX offset, are among them"
    );
    assert_eq!(
        refill.covered_from, 0,
        "nothing dropped yet: every slot is whole"
    );
}

/// Past the history's depth the oldest slot may be cut: the refill says where the whole slots
/// begin and returns none from before it, so the pane can mark what it can no longer fill.
#[test]
fn a_full_history_says_where_its_whole_slots_begin() {
    let mut e = Engine::new("KN0BLE", "EL98", 0);
    e.set_tier(Tier::Ft8);
    // 7 decodes a period: 241 periods hold 1687, of which the newest 240 are kept, so slot 207
    // (the oldest kept) has lost some of its decodes.
    for slot in 1..=241u64 {
        let p = (0..7)
            .map(|i| {
                decode(
                    &format!("CQ K{slot}X{i} EN52"),
                    300.0 + 100.0 * i as f32,
                    -10,
                )
            })
            .collect();
        e.fold_decodes(None, p, slot, false);
    }
    assert_eq!(
        e.decode_history.front().map(|(s, _)| *s),
        Some(207),
        "premise"
    );
    let refill = e.decode_refill(1, 241);
    assert_eq!(refill.covered_from, 208);
    assert_eq!(refill.rows.first().map(|r| r.slot), Some(208));
    assert_eq!(refill.rows.len(), 34 * 7, "slots 208..=241, whole");
}

/// ⛔ READ ONLY. Two stations run the same CQ run on the same decodes; one is asked for a refill
/// before every fold and every over. Every over each plans, its snapshot and the history the
/// sequencer reads stay identical, byte for byte.
#[test]
fn asking_for_a_refill_changes_nothing_the_sequencer_or_the_alerts_read() {
    let mut plain = cq_run();
    let mut asked = cq_run();
    let mut overs = Vec::new();
    for slot in 1..=8u64 {
        let _ = asked.decode_refill(0, slot);
        plain.fold_decodes(None, period(slot), slot, false);
        asked.fold_decodes(None, period(slot), slot, false);
        let _ = asked.decode_refill(slot.saturating_sub(3), slot);
        let (a, b) = (over_text(&mut plain, slot), over_text(&mut asked, slot));
        assert_eq!(a, b, "slot {slot}: the over planned");
        assert_eq!(
            plain.decode_history, asked.decode_history,
            "slot {slot}: the history"
        );
        assert_eq!(snap(&plain), snap(&asked), "slot {slot}: the snapshot");
        overs.extend(a);
    }
    assert!(
        overs.iter().any(|o| o.starts_with("KF9UG KN0BLE")),
        "premise: the run answered the caller: {overs:?}"
    );
}
