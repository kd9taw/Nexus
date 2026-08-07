//! Every (mycall-class × dxcall-class) pair must reach 73 — measured END TO END, two
//! real [`Station`]s driving each other over the virtual air through the real modem.
//!
//! **Why this file exists rather than more unit tests on `observe`.** `c745a842` and
//! `b1651125` got the WIRE right: `/P` and `/R` ride Type 1/2 with their grid and their
//! numbers, and the two pairs the packer cannot express fall back to the hashed forms so
//! both callsigns still go on the air verbatim. Every guard for that work asserts on ONE
//! station's queued over — what we send, and what comes back off the air. All of them
//! passed while four quadrants could not complete a QSO at all, because what the packer
//! forces on a pair and what the SEQUENCER can ride are two different claims, and only an
//! exchange can test the second. A `/P` station and a `/R` station traded the same legal,
//! correctly-addressed over slot after slot forever: `observe` had no arm for it, so each
//! sat in `AwaitRoger` waiting for the other to roger first. Nothing was malformed;
//! nothing advanced.
//!
//! So the assertion here is COMPLETION: two stations, real encode, real decode, run to 73
//! and a contact the log will take. Sixteen quadrants — plain standard, `/P`, `/R` and
//! genuinely nonstandard on each side, because `/P` and `/R` must be told apart to see the
//! suffix conflict at all — with the CQ caller drawn from one set and the answering
//! station from the other, so both roles of every class pair are swept.
//!
//! Message-form correctness (the packer round trip, the never-rename-a-station rule) is
//! `portable_suffix_air.rs`. This file assumes those forms and asks only whether a QSO
//! built out of them finishes.

use tempo_core::qso::{run_loopback_qso, State, Station};

/// What the 77-bit packer can carry for a callsign, as a property of the CALL alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// A bare standard c28 call — the only shape that still carries a grid or a numeric
    /// report when it stands opposite a `<hash>`.
    Plain,
    /// Standard, and the suffix rides Type 2's `p1` bit.
    SlashP,
    /// Standard, and the suffix rides Type 1's `r1` bit.
    SlashR,
    /// 77-bit nonstandard: it can only travel as a full `c58` beside a hash (i3=4).
    Nonstandard,
}

/// The two facts the protocol fixes for a PAIR, restated here independently of
/// `message.rs` so this file cannot agree with a broken predicate:
///
/// * the frame falls to the hashed i3=4 forms when either call is nonstandard, or when
///   one `/P` sits opposite one `/R` (one `i3` cannot mean both suffixes at once);
/// * a numeric report then survives only from a sender that is a bare c28 call.
fn hashed(my: Class, dx: Class) -> bool {
    use Class::*;
    my == Nonstandard
        || dx == Nonstandard
        || matches!((my, dx), (SlashP, SlashR) | (SlashR, SlashP))
}

/// True when a station of class `me` can put a NUMBER on the air in this pair.
fn number_rides(me: Class, other: Class) -> bool {
    !hashed(me, other) || me == Class::Plain
}

/// The station that calls CQ (`dx`) and the station that answers it (`my`).
const THEIRS: [(&str, &str, Class); 4] = [
    ("W9XYZ", "JN18", Class::Plain),
    ("F4CYH/P", "JN18", Class::SlashP),
    ("F4CYH/R", "JN18", Class::SlashR),
    ("PJ4/K1ABC", "JN18", Class::Nonstandard),
];
const MINE: [(&str, &str, Class); 4] = [
    ("KD9TAW", "EN52", Class::Plain),
    ("KD9TAW/P", "EN52", Class::SlashP),
    ("KD9TAW/R", "EN52", Class::SlashR),
    ("YW18FIFA", "EN52", Class::Nonstandard),
];

#[test]
fn every_callsign_class_pair_runs_to_73_and_logs() {
    for (my, mygrid, myc) in MINE {
        for (dx, dxgrid, dxc) in THEIRS {
            // The DX runs CQ; we answer it — the ordinary S&P contact, and between the two
            // sets every (CQ-caller class × answerer class) combination is covered.
            let mut theirs = Station::calling_cq(dx, dxgrid);
            let mut mine = Station::answering(my, mygrid, dx);
            let air = run_loopback_qso(&mut theirs, &mut mine, 15.0, 24);

            let show = || {
                let mut s = format!("\n--- {my} × {dx} ---\n");
                for e in &air {
                    s.push_str(&format!("  slot {:>2}  {}: {}\n", e.slot, e.from, e.text));
                }
                s
            };

            // THE ASSERTION. Not "no over was malformed" — every over in the livelock was
            // perfectly well-formed — but that the exchange ENDED.
            assert_eq!(
                (theirs.state, mine.state),
                (State::Done, State::Done),
                "{my} × {dx}: the QSO never reached 73{}",
                show()
            );
            assert!(
                theirs.done() && mine.done(),
                "{my} × {dx}: reached Done with an over still owed{}",
                show()
            );
            assert!(
                air.len() <= 8,
                "{my} × {dx}: took {} overs{}",
                air.len(),
                show()
            );

            // Each side logs the OTHER's call exactly as it is, brackets and suffix and
            // all — the `b1651125` guarantee, re-checked after a full exchange rather
            // than on a single queued over.
            assert_eq!(
                theirs.dxcall.as_deref(),
                Some(my),
                "{my} × {dx}: the DX logged the wrong call{}",
                show()
            );
            assert_eq!(
                mine.dxcall.as_deref(),
                Some(dx),
                "{my} × {dx}: we logged the wrong call{}",
                show()
            );
            // (A CQ has no addressee, so it names one station by construction.)
            for e in air.iter().filter(|e| !e.text.starts_with("CQ ")) {
                assert!(
                    e.text.contains(my) && e.text.contains(dx),
                    "{my} × {dx}: an over named a different station: [{}]{}",
                    e.text,
                    show()
                );
            }

            // The numbers, exactly where the protocol has room for them and nowhere else.
            // `theirs.rx_report` is the number WE got through to them, and vice versa.
            assert_eq!(
                theirs.rx_report.is_some(),
                number_rides(myc, dxc),
                "{my} × {dx}: our report reached the DX = {:?}, expected rideable = {}{}",
                theirs.rx_report,
                number_rides(myc, dxc),
                show()
            );
            assert_eq!(
                mine.rx_report.is_some(),
                number_rides(dxc, myc),
                "{my} × {dx}: the DX's report reached us = {:?}, expected rideable = {}{}",
                mine.rx_report,
                number_rides(dxc, myc),
                show()
            );

            // …and the contact is loggable on BOTH sides. This is the engine's auto-log
            // gate restated: a report in one direction, or an exchange the protocol gave
            // no room for a report in. Seven of the sixteen pairs exchange no number at
            // all, and demanding one of them dropped a completed QSO from the log (and,
            // on a CQ run, stopped the run — `resume_cq` waits on the contact being
            // claimed). Nothing here invents a number that never went out.
            for (st, sent, side) in [
                (&theirs, number_rides(dxc, myc), "the DX"),
                (&mine, number_rides(myc, dxc), "we"),
            ] {
                assert!(
                    st.rx_report.is_some() || sent || st.report_impossible_exchange(),
                    "{my} × {dx}: {side} completed a contact the log would refuse{}",
                    show()
                );
            }
        }
    }
}

#[test]
fn a_synthesized_done_is_still_not_a_contact() {
    // The guard `report_impossible_exchange` must not weaken: `call_station` can build a
    // `Done` straight out of one decoded RR73 the operator double-clicked, with no
    // exchange behind it (the "3 identical calls I never worked" report). That seed is
    // Done and report-less for a report-IMPOSSIBLE pair too — the case the relaxed log
    // gate would wave through if it asked only "could a number have ridden?". It never
    // advanced on the air, so it stays refused.
    let rr73 = tempo_core::message::Msg::parse("KD9TAW/P F4CYH/R RR73");
    let seed = Station::start(
        "KD9TAW/P",
        "EN52",
        "F4CYH/R",
        Some((&rr73, -7)),
        false,
        false,
    );
    assert_eq!(seed.state, State::Done, "the seed lands at Done");
    assert!(seed.rx_report.is_none(), "…carrying no report");
    assert!(
        !seed.advanced_on_air,
        "…and nothing on the air put it there"
    );
    assert!(
        !seed.report_impossible_exchange(),
        "a synthesized Done must never claim a report-less contact"
    );
}
