//! Replay a real ALL.TXT through the QSO state machine.
//!
//! ⭐ WHY THIS EXISTS. On 2026-08-25 the operator was working RI1FJL on 10.131, cleared his
//! outgoing message back to his grid (Tx1), and watched Nexus go back to sending `R-21` on its
//! own — twice, with nothing addressed to him on the air in between. Reading the code could not
//! settle it: every arm of [`Station::observe`] that writes `pending` is gated on
//! `same_call(to, mycall)`, and a line-by-line check of his ALL.TXT confirmed **no message was
//! addressed to KD9TAW anywhere in either window**. So either the state machine flips without
//! input, or something OUTSIDE it is rewriting the message.
//!
//! This decides that, and it is a measurement rather than an argument: feed the machine exactly
//! the decodes the radio heard, in period order, apply the operator's manual picks where the log
//! shows him making one, and compare what the machine would send against what Nexus actually
//! transmitted. Every disagreement is a place the app departed from its own sequencer.
//!
//! Point it at a different file with `NEXUS_ALLTXT=/path/to/ALL.TXT`; with nothing set it runs
//! the checked-in RI1FJL window so the harness itself stays honest in CI.
//!
//! ⚠️ SCOPE: ONE `Station` IS ONE QSO. Give it a window containing a single contact. Pointed at
//! a whole ALL.TXT it replays every over in the file through one machine targeted at one DX, so
//! other contacts show up as `sequencer=<nothing>` divergences — an artefact of the harness, not
//! a finding. Measured on the operator's full 4.7 MB file: 90 divergences, of which the ones
//! outside the RI1FJL window are exactly that. Cut the window first.

use modes::Decode;
use std::collections::BTreeMap;
use tempo_core::message::Msg;
use tempo_core::qso::Station;

const MYCALL: &str = "KD9TAW";
const MYGRID: &str = "EN52";
const DXCALL: &str = "RI1FJL";

#[derive(Debug, Clone)]
struct Line {
    stamp: String,
    is_tx: bool,
    snr: i32,
    dt: f32,
    hz: f32,
    msg: String,
}

/// Parse one WSJT-X ALL.TXT line. Whitespace-split is safe here: every field before the
/// message is non-empty, and the message is whatever remains.
fn parse_line(s: &str) -> Option<Line> {
    let mut it = s.split_whitespace();
    let stamp = it.next()?.to_string();
    if stamp.len() != 13 || !stamp.contains('_') {
        return None;
    }
    let _dial = it.next()?;
    let dir = it.next()?;
    let is_tx = match dir {
        "Tx" => true,
        "Rx" => false,
        _ => return None,
    };
    let _mode = it.next()?;
    let snr = it.next()?.parse().ok()?;
    let dt = it.next()?.parse().ok()?;
    let hz: f32 = it.next()?.parse().ok()?;
    let msg = it.collect::<Vec<_>>().join(" ");
    if msg.is_empty() {
        return None;
    }
    Some(Line {
        stamp,
        is_tx,
        snr,
        dt,
        hz,
        msg,
    })
}

fn decode_of(l: &Line) -> Decode {
    Decode {
        message: l.msg.clone(),
        sync: 0.0,
        snr: l.snr,
        dt: l.dt,
        freq: l.hz,
        nap: 0,
        qual: 1.0,
        rv: None,
        mode: None,
    }
}

/// Is `msg` addressed to us? The whole question the code review turned on.
fn addressed_to_me(msg: &str) -> bool {
    matches!(msg.split_whitespace().next(), Some(to)
        if tempo_core::message::same_call(to.trim_matches(['<', '>']), MYCALL))
}

struct Divergence {
    stamp: String,
    machine: String,
    actual: String,
    rx_to_me_since: usize,
}

fn replay(lines: &[Line]) -> (Vec<Divergence>, Station) {
    // Start where the log does: we are calling RI1FJL and awaiting his report.
    let mut st = Station::answering(MYCALL, MYGRID, DXCALL);
    // The engine's own default — a station that answered and then went quiet stops being
    // called. Left as shipped so the replay matches the app.
    st.call_cap = Some(8);

    // Group RX by period so `observe` sees a slot's worth at once, as the engine does.
    let mut periods: BTreeMap<String, Vec<Decode>> = BTreeMap::new();
    for l in lines.iter().filter(|l| !l.is_tx) {
        periods
            .entry(l.stamp.clone())
            .or_default()
            .push(decode_of(l));
    }

    let mut out = Vec::new();
    let mut rx_to_me_since = 0usize;
    let mut fed: Vec<&String> = Vec::new();

    for l in lines {
        if !l.is_tx {
            continue;
        }
        // Feed every RX period that precedes this over and has not been fed yet.
        for (stamp, decs) in periods.iter() {
            if stamp.as_str() >= l.stamp.as_str() || fed.contains(&stamp) {
                continue;
            }
            fed.push(stamp);
            rx_to_me_since += decs.iter().filter(|d| addressed_to_me(&d.message)).count();
            st.observe(decs);
        }

        let machine = st.pending_text().unwrap_or_else(|| "<nothing>".into());
        if machine.trim() != l.msg.trim() {
            out.push(Divergence {
                stamp: l.stamp.clone(),
                machine: machine.clone(),
                actual: l.msg.clone(),
                rx_to_me_since,
            });
            // Re-sync to reality: whatever the app actually sent is now the state of the
            // world, so the rest of the replay stays meaningful.
            st.override_next(Msg::parse(&l.msg));
        }
        rx_to_me_since = 0;
        st.after_tx();
    }
    (out, st)
}

fn load() -> Vec<Line> {
    let path = std::env::var("NEXUS_ALLTXT").unwrap_or_else(|_| {
        format!(
            "{}/tests/fixtures/alltxt/ri1fjl-30m.txt",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read ALL.TXT at {path}: {e}"));
    let lines: Vec<Line> = body.lines().filter_map(parse_line).collect();
    assert!(
        !lines.is_empty(),
        "control: {path} parsed to ZERO lines — the parser or the file is wrong"
    );
    lines
}

/// THE MEASUREMENT. Print every point where the app's transmission disagreed with what its own
/// sequencer would have sent, and how many messages addressed to us arrived in between.
///
/// A disagreement with `rx_to_me = 0` is the report: the message changed with NO input that
/// could legitimately change it.
#[test]
fn replay_names_every_departure_from_the_sequencer() {
    let lines = load();
    let (divs, st) = replay(&lines);

    eprintln!(
        "\n=== {} overs replayed ===",
        lines.iter().filter(|l| l.is_tx).count()
    );
    for d in &divs {
        eprintln!(
            "{}  sequencer={:<28} actually sent={:<28} rx_addressed_to_me_since_last_over={}",
            d.stamp, d.machine, d.actual, d.rx_to_me_since
        );
    }
    eprintln!("\n=== transcript ===");
    for t in &st.transcript {
        eprintln!("  {t}");
    }

    // The operator's two manual picks are EXPECTED divergences (he chose Tx1 by hand, which the
    // sequencer would not have). Everything else is the app departing from its own state machine.
    let unexplained: Vec<&Divergence> = divs
        .iter()
        .filter(|d| d.rx_to_me_since == 0 && !d.actual.contains(MYGRID))
        .collect();

    eprintln!(
        "\n{} divergence(s); {} of them with NOTHING addressed to us since the last over",
        divs.len(),
        unexplained.len()
    );
    for d in &unexplained {
        eprintln!(
            "  UNEXPLAINED {}  sequencer={}  sent={}",
            d.stamp, d.machine, d.actual
        );
    }
}

/// The harness's own positive control: a clean, synthetic QSO must replay with ZERO divergences.
/// Without this, "no divergences" from the real file would prove only that the replay is inert.
#[test]
fn a_clean_qso_replays_with_no_divergence() {
    let script = "\
260825_010000    14.074 Rx FT8    -10  0.2 1500 KD9TAW W1ABC -12
260825_010015    14.074 Tx FT8      0  0.0 1500 W1ABC KD9TAW R-10
260825_010030    14.074 Rx FT8    -10  0.2 1500 KD9TAW W1ABC RR73
260825_010045    14.074 Tx FT8      0  0.0 1500 W1ABC KD9TAW 73
";
    let lines: Vec<Line> = script.lines().filter_map(parse_line).collect();
    assert_eq!(lines.len(), 4, "control: the script itself must parse");

    let mut st = Station::answering(MYCALL, MYGRID, "W1ABC");
    st.call_cap = Some(8);
    let mut periods: BTreeMap<String, Vec<Decode>> = BTreeMap::new();
    for l in lines.iter().filter(|l| !l.is_tx) {
        periods
            .entry(l.stamp.clone())
            .or_default()
            .push(decode_of(l));
    }
    let mut fed: Vec<&String> = Vec::new();
    let mut divergences = 0;
    for l in &lines {
        if !l.is_tx {
            continue;
        }
        for (stamp, decs) in periods.iter() {
            if stamp.as_str() >= l.stamp.as_str() || fed.contains(&stamp) {
                continue;
            }
            fed.push(stamp);
            st.observe(decs);
        }
        if st.pending_text().unwrap_or_default().trim() != l.msg.trim() {
            eprintln!(
                "divergence at {}: sequencer={:?} sent={:?}",
                l.stamp,
                st.pending_text(),
                l.msg
            );
            divergences += 1;
        }
        st.after_tx();
    }
    assert_eq!(
        divergences, 0,
        "a textbook QSO must replay exactly — if this fails the harness is broken, \
         not the app"
    );
}

/// The addressed-to-me predicate is the load-bearing one (it is what decides whether a
/// divergence is explainable), so it gets its own both-directions check.
#[test]
fn addressed_to_me_is_exact_and_handles_the_hashed_form() {
    assert!(addressed_to_me("KD9TAW RI1FJL -07"));
    assert!(addressed_to_me("<KD9TAW> RI1FJL -07"));
    // The Fox's traffic to everyone else — the whole pileup — must NOT count.
    assert!(!addressed_to_me("KZ7Y RI1FJL -08"));
    assert!(!addressed_to_me("W3US RI1FJL RR73"));
    assert!(!addressed_to_me("RI1FJL W5MMW EM75"));
    assert!(!addressed_to_me("CQ KC9IVQ EN62"));
}
