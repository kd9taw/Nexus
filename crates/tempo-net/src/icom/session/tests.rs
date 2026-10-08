//! The session against the simulated radio, on a fake clock and on real loopback sockets.
//!
//! Translated from upstream's `test/test_icom_network_session.c` (its handshake, capability,
//! sequence, CI-V, liveness, socket-error, retransmit, ping, teardown and free cases; the audio
//! and asynchronous-routing cases are not translated, there being no audio and no frame router
//! at this stage), `test/test_icom_network_reconnect.c` (all five cases, against Nexus's
//! reconnect policy, since a session never reconnects by itself) and
//! `test/test_icom_network_conf.c` (the credential, port, liveness and default cases, re-expressed
//! for the typed configuration). Each test names the upstream case it translates. The rest are
//! Nexus's own: the property that no input makes the session send a CI-V command, the teardown
//! order, a refused login, the keepalive cadence, and the password's path.
//!
//! PORTED from Hamlib (https://github.com/Hamlib/Hamlib, pull request #2178, open when taken),
//! `test/test_icom_network_session.c`, `test/test_icom_network_reconnect.c` and
//! `test/test_icom_network_conf.c` at commit
//! `2e3e4a6add3bd806e828d035200777608d2bdbd5` (2026-10-07), translated from C to Rust.
//! Deliberate differences: a fake clock in place of sleeps, so the liveness and retransmit
//! timings are checked exactly and the suite is fast, with real sockets kept for what only a
//! real socket shows; the expectations follow the session's own differences (a name mismatch
//! with one advertised radio connects with a warning, transmit-enable is 0 whatever the radio
//! offers, liveness cannot be turned off, a lost session never reconnects by itself, a frame is
//! refused rather than sent on a failing socket); the reconnect cases run the session against
//! Nexus's reconnect policy instead of a backend's background thread; the configuration cases
//! test typed values instead of string tokens; made-up credentials and addresses.
//! The protocol knowledge descends from kappanhang (https://github.com/nonoo/kappanhang, MIT), as the
//! upstream author states. Recorded in the repo-root NOTICE (Hamlib entry), which reproduces
//! kappanhang's notice.
//!
//! The upstream notice, converted to the GNU General Public License under section 3 of the GNU
//! Lesser General Public License, version 2.1, and otherwise unchanged:
//!
//! Copyright (c) 2026 by Mikael Nousiainen OH3BHX
//!
//! This library is free software; you can redistribute it and/or modify it under the terms of the
//! GNU General Public License as published by the Free Software Foundation; either version 3 of the
//! License, or (at your option) any later version.
//!
//! This library is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
//! without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
//! the GNU General Public License for more details.
//!
//! You should have received a copy of the GNU General Public License along with this library; if
//! not, write to the Free Software Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA
//! 02110-1301 USA
//!
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Modified by KD9TAW, 2026-10-08: translated from C to Rust and changed as listed
//! above. These changes are licensed under the GNU General Public License, version 3 only, as Nexus
//! is (see COPYING), so this file as a whole is distributed under GPL version 3.

use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::time::Duration;

use super::*;
use crate::icom::caps::{Model, Rates, RATE_24000, RATE_8000};
use crate::icom::reconnect::{End, Ladder, Next};
use crate::icom::sim::{
    self, Entry, SimRadio, SocketClient, SocketRadio, World, FREQ_REPLY, READ_FREQ, STEP_MS,
};
use crate::icom::wire::tests::Rng;
use crate::icom::wire::{le16, put_be16, put_le16, put_le32};

fn world(radio: SimRadio) -> World {
    World::start(radio, sim::config(Model::Ic7610))
}

fn connected(radio: SimRadio) -> World {
    let mut w = world(radio);
    assert!(w.connect(), "{:?}", w.events);
    w
}

/// The kinds of what the radio received on `role`, idles and pings left out.
fn requests(w: &World, role: Role) -> Vec<String> {
    w.radio_saw(role)
        .iter()
        .filter_map(|p| match p {
            Packet::Control {
                op: ControlOp::Idle,
                ..
            }
            | Packet::Ping(_)
            | Packet::Retransmit(_) => None,
            Packet::Control { op, .. } => Some(format!("{op:?}")),
            Packet::Token(t) => Some(format!("Token {:?}", t.request())),
            Packet::Login(_) => Some("Login".into()),
            Packet::ConnectionInfo(_) => Some("ConnectionInfo".into()),
            Packet::OpenClose(o) => Some(format!("{:?}", o.op)),
            Packet::Civ(_) => Some("Civ".into()),
            other => Some(format!("{other:?}")),
        })
        .collect()
}

fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.dedup();
    v
}

fn connected_radio(w: &World) -> Radio {
    w.events
        .iter()
        .find_map(|e| match e {
            Event::Connected(radio) => Some(radio.clone()),
            _ => None,
        })
        .expect("connected")
}

fn failure(w: &World) -> Option<ConnectError> {
    w.events.iter().find_map(|e| match e {
        Event::Failed(f) => Some(f.clone()),
        _ => None,
    })
}

// ── the handshake ───────────────────────────────────────────────────────────────────────────

// upstream: test_session_handshake_and_civ_roundtrip
#[test]
fn handshake_and_civ_roundtrip() {
    let mut w = connected(SimRadio::new());
    assert_eq!(w.state(), State::Connected);
    // the radio's order, on each socket
    assert_eq!(
        dedup(requests(&w, Role::Control)),
        [
            "Probe",
            "Ready",
            "Login",
            "Token Some(Create)",
            "ConnectionInfo"
        ]
    );
    assert_eq!(dedup(requests(&w, Role::Civ)), ["Probe", "Ready", "Open"]);
    // the data sockets are pointed at the ports the radio assigned
    assert_eq!(
        w.redirects,
        [(Role::Civ, wire::PORT_CIV), (Role::Audio, wire::PORT_AUDIO)]
    );
    // a CI-V command and its answer through the session
    assert_eq!(w.roundtrip(), Some(FREQ_REPLY.to_vec()));
    assert_eq!(w.radio.civ_commands, [READ_FREQ.to_vec()]);
    let radio = connected_radio(&w);
    assert_eq!((radio.name.as_str(), radio.civ_addr), ("IC-7610", 0x98));
    assert!(radio.name_matches());
}

#[test]
fn connected_is_reported_after_the_drain() {
    // The leftover frames are drained until 50 ms pass with none.
    let mut w = world(SimRadio::new());
    assert_eq!(w.state(), State::Draining);
    assert!(!w.connected());
    assert!(w.connect());
    assert!(
        w.now >= DRAIN_QUIET_MS && w.now <= DRAIN_TOTAL_MS,
        "{}",
        w.now
    );
    // no frame is handed over before then
    let mut early = world(SimRadio::new());
    assert_eq!(
        early.session.send_civ(&READ_FREQ, 0),
        Err(CivError::NotConnected)
    );
    early.step(STEP_MS);
    assert_eq!(
        early.session.send_civ(&READ_FREQ, 5),
        Err(CivError::NotConnected)
    );
}

// upstream: test_session_stale_frame_drain
#[test]
fn stale_frame_drain() {
    // The radio flushes a leftover NAK when the stream opens; it must never surface as the
    // answer to the first command.
    let mut radio = SimRadio::new();
    radio.stale_nak = true;
    let mut w = connected(radio);
    assert!(w.delivered.is_empty(), "{:02x?}", w.delivered);
    assert_eq!(w.roundtrip(), Some(FREQ_REPLY.to_vec()));
}

// upstream: test_session_failed_connect_releases_slot
#[test]
fn failed_connect_releases_slot() {
    // A connect that fails after the login gives the token back and disconnects, so the radio
    // does not hold the slot and refuse the next attempt.
    let mut radio = SimRadio::new();
    radio.status_error = 0xffff_ffff;
    let mut w = world(radio);
    assert!(w.run_until(1000, World::closed));
    assert_eq!(failure(&w), Some(ConnectError::Busy(0xffff_ffff)));
    assert_eq!(w.radio.token_removes, 1);
    assert_eq!(w.radio.ctrl_disconnects, 1);
    // the token goes back first, the disconnect after the grace
    let at = |w: &World, pick: &dyn Fn(&Packet) -> bool| {
        w.sent
            .iter()
            .zip(&w.sent_at)
            .find(|(d, _)| wire::decode(d.role, &d.bytes).is_ok_and(|p| pick(&p)))
            .map(|(_, t)| *t)
            .expect("sent")
    };
    let removed = at(
        &w,
        &|p| matches!(p, Packet::Token(t) if t.request() == Some(TokenRequest::Remove)),
    );
    let disconnected = at(&w, &|p| {
        matches!(
            p,
            Packet::Control {
                op: ControlOp::Disconnect,
                ..
            }
        )
    });
    assert!(
        disconnected >= removed + DISCONNECT_GRACE_MS,
        "{removed} {disconnected}"
    );
    // and a new attempt then succeeds
    let World { mut radio, .. } = w;
    radio.status_error = 0;
    let mut again = World::start(radio, sim::config(Model::Ic7610));
    assert!(again.connect());
}

// upstream: test_session_handshake_recovers_lost_reply
#[test]
fn handshake_recovers_lost_reply() {
    // A lost handshake reply is recovered the way the protocol intends: the gap the radio's next
    // tracked packet leaves is noticed and the reply asked for again. Resending the request does
    // not help, because the radio has it and does not answer it twice.
    let mut radio = SimRadio::new();
    radio.ctrl_tracked = true;
    radio.ctrl_ignore_resends = true;
    radio.drop_capabilities_replies = 1;
    let mut w = world(radio);
    assert!(w.run_until(3000, World::connected), "{:?}", w.events);
    assert!(w.radio.ctrl_retransmit_requests >= 1);
}

// upstream: test_session_handshake_recovers_lost_first_reply
#[test]
fn handshake_recovers_lost_first_reply() {
    // The radio numbers its tracked replies from 0, so the login reply is the very first; lost,
    // it leaves no gap unless tracking expects 0.
    let mut radio = SimRadio::new();
    radio.ctrl_tracked = true;
    radio.ctrl_ignore_resends = true;
    radio.drop_login_replies = 1;
    let mut w = world(radio);
    assert!(w.connect(), "{:?}", w.events);
    assert!(w.radio.ctrl_retransmit_requests >= 1);
}

#[test]
fn a_refused_login_gives_nothing_back_and_disconnects() {
    // No token was granted, so there is none to return: the control socket disconnects at once.
    let mut radio = SimRadio::new();
    radio.login_error = 0x0000_0001;
    let w = world(radio);
    assert!(w.closed());
    assert_eq!(failure(&w), Some(ConnectError::LoginRefused));
    assert_eq!((w.radio.token_removes, w.radio.ctrl_disconnects), (0, 1));
    assert!(w.radio.connection_info.is_none());
}

// ── choosing the radio ──────────────────────────────────────────────────────────────────────

// upstream: test_session_capability_name_is_echoed
#[test]
fn capability_name_is_echoed() {
    // The name sent on connect is the one the server reported, trimmed of its padding.
    let mut radio = SimRadio::new();
    radio.radios[0].name = "IC-7610  ".into();
    let w = connected(radio);
    let info = w.radio.connection_info.as_ref().expect("the request");
    assert_eq!(&info[0x40..0x48], b"IC-7610\0");
}

// upstream: test_session_capability_select_by_name
#[test]
fn capability_select_by_name() {
    // With several radios advertised, the model's name picks one and its identity is echoed.
    let mut radio = SimRadio::new();
    radio.radios.push(Entry::new("IC-9700", 0xa2));
    radio.radios.push(Entry::new("IC-705", 0xa4));
    let mut w = World::start(radio, sim::config(Model::Ic705));
    assert!(w.connect());
    let info = w.radio.connection_info.as_ref().expect("the request");
    assert_eq!(&info[0x40..0x47], b"IC-705\0");
    // the simulated radio stamps each entry's index into its identity's last byte
    assert_eq!(info[0x20 + 0x0f], 2);
    assert_eq!(connected_radio(&w).civ_addr, 0xa4);
}

// upstream: test_session_capability_select_by_index
#[test]
fn capability_select_by_index() {
    let mut radio = SimRadio::new();
    radio.radios.push(Entry::new("IC-9700", 0xa2));
    let mut config = sim::config(Model::Ic7610);
    config.choice = Some(Choice::Index(1));
    let mut w = World::start(radio, config);
    assert!(w.connect());
    let info = w.radio.connection_info.as_ref().expect("the request");
    assert_eq!(&info[0x40..0x48], b"IC-9700\0");
}

// upstream: test_session_capability_no_match
#[test]
fn capability_no_match() {
    // Upstream refuses a radio whose name is not the model's. A radio's own server advertises
    // exactly one, and the name each model reports is not confirmed for all six, so here that is
    // a warning: the session connects and says which name it saw.
    let mut w = World::start(SimRadio::new(), sim::config(Model::Ic9700));
    assert!(w.connect());
    let radio = connected_radio(&w);
    assert_eq!(
        (radio.name.as_str(), radio.expected),
        ("IC-7610", "IC-9700")
    );
    assert!(!radio.name_matches());
    // With several advertised and none matching, there is no telling which is meant: refused,
    // before any connection request, and the token goes back.
    let mut several = SimRadio::new();
    several.radios.push(Entry::new("IC-705", 0xa4));
    let mut w = World::start(several, sim::config(Model::Ic9700));
    assert!(w.run_until(1000, World::closed));
    assert_eq!(
        failure(&w),
        Some(ConnectError::NoRadio(caps::Unselected::NoSuchName))
    );
    assert!(w.radio.connection_info.is_none());
    assert_eq!(w.radio.token_removes, 1);
}

// upstream: test_session_capability_index_out_of_range
#[test]
fn capability_index_out_of_range() {
    let mut config = sim::config(Model::Ic7610);
    config.choice = Some(Choice::Index(3));
    let mut w = World::start(SimRadio::new(), config);
    assert!(w.run_until(1000, World::closed));
    assert_eq!(
        failure(&w),
        Some(ConnectError::NoRadio(caps::Unselected::IndexOutOfRange {
            index: 3,
            advertised: 1
        }))
    );
    assert!(w.radio.connection_info.is_none());
}

// upstream: test_session_capability_rate_rejected
#[test]
fn capability_rate_rejected() {
    // A radio that offers no receive rate at all fails at connect, where the message can say so.
    let mut radio = SimRadio::new();
    radio.radios[0].rx = 0;
    radio.radios[0].tx = 0;
    let mut w = world(radio);
    assert!(w.run_until(1000, World::closed));
    assert_eq!(failure(&w), Some(ConnectError::RateNotOffered(Rates(0))));
    assert!(w.radio.connection_info.is_none());
}

/// The rate the connection request names, as the radio received it.
fn requested_rates(w: &World) -> (u32, u32) {
    let info = w.radio.connection_info.as_ref().expect("the request");
    let be32 = |at: usize| u32::from_be_bytes([info[at], info[at + 1], info[at + 2], info[at + 3]]);
    (be32(0x74), be32(0x78))
}

/// ⭐ NO AUDIO STARTS AT THIS STAGE, so the rate only has to be one the radio takes: 48 kHz where
/// it offers it, as upstream asks, else the highest it offers. Upstream refuses a radio that lacks
/// the configured rate; here that would cost the operator CAT over a rate nothing uses.
#[test]
fn the_request_names_48_khz_or_the_highest_rate_the_radio_offers() {
    let w = connected(SimRadio::new());
    assert_eq!(
        requested_rates(&w),
        (48_000, 48_000),
        "every rate offered: 48 kHz"
    );
    let mut radio = SimRadio::new();
    radio.radios[0].rx = RATE_8000 | RATE_24000;
    radio.radios[0].tx = RATE_8000 | RATE_24000;
    let w = connected(radio);
    assert_eq!(
        requested_rates(&w),
        (24_000, 24_000),
        "no 48 kHz: the highest offered"
    );
    let mut radio = SimRadio::new();
    radio.radios[0].rx = RATE_8000;
    radio.radios[0].tx = RATE_8000;
    let w = connected(radio);
    assert_eq!(requested_rates(&w), (8_000, 8_000));
    assert!(
        connected_radio(&w).tx_audio_advertised,
        "its transmit audio is still reported"
    );
}

// upstream: test_session_capability_tx_suppressed
// upstream: test_defaults_after_init (net_tx_enable: here there is no such setting)
#[test]
fn capability_tx_suppressed() {
    // Upstream reserves a transmit path on a radio that offers one. Here the request carries
    // transmit-enable 0 whatever the radio advertises; the advertisement is only reported.
    for (tx, advertised) in [(0u16, false), (sim::ALL_RATES, true)] {
        let mut radio = SimRadio::new();
        radio.radios[0].tx = tx;
        let w = connected(radio);
        let info = w.radio.connection_info.as_ref().expect("the request");
        assert_eq!(info[0x70], 1, "receive enable");
        assert_eq!(info[0x71], 0, "transmit enable");
        assert_eq!(connected_radio(&w).tx_audio_advertised, advertised);
    }
}

// ── sequence numbers and CI-V frames ────────────────────────────────────────────────────────

// upstream: test_session_sequence_resync
#[test]
fn sequence_resync() {
    // A loss burst wider than the replay window cannot be recovered packet by packet: the session
    // starts tracking again instead of asking for retransmits for ever.
    let mut w = connected(SimRadio::new());
    assert!(w.roundtrip().is_some());
    assert_eq!(w.session.resyncs(Role::Civ), 0);
    w.radio.civ_sequence_jump = MISSING_FLUSH + 10;
    assert!(w.roundtrip().is_some());
    assert_eq!(w.session.resyncs(Role::Civ), 1);
    assert_eq!(w.session.resyncs(Role::Control), 0);
    // and the session keeps working afterwards
    assert_eq!(w.roundtrip(), Some(FREQ_REPLY.to_vec()));
}

// upstream: test_session_civ_reply_with_management_length
#[test]
fn civ_reply_with_management_length() {
    // A CI-V reply whose packet is as long as a management packet is still delivered.
    let mut w = connected(SimRadio::new());
    for total in [0x40usize, 0x50, 0x60, 0x80, 0x90, 0xa8] {
        w.radio.civ_reply_length = total - wire::CIV_HEADER_LEN;
        let frame = w.roundtrip().expect("a reply");
        assert_eq!(frame.len(), total - wire::CIV_HEADER_LEN, "{total:#x}");
    }
}

// upstream: test_session_civ_idles_not_requested
#[test]
fn civ_idles_not_requested() {
    // The radio numbers its CI-V-socket idles with its frames; tracking only the frames saw a gap
    // for every idle and kept asking the radio to resend idles.
    let mut w = connected(SimRadio::new());
    for i in 0..4 {
        assert!(w.roundtrip().is_some());
        w.step(if i == 2 {
            (u64::from(MISSING_FLUSH) + 20) * 50
        } else {
            300
        });
    }
    assert_eq!(w.radio.civ_retransmit_requests, 0);
    assert_eq!(w.session.resyncs(Role::Civ), 0);
}

// upstream: test_session_civ_duplicate_dropped
#[test]
fn civ_duplicate_dropped() {
    // A reply that arrives twice is delivered once: a repeated ACK or reply would be taken for
    // the answer to the next command.
    let mut radio = SimRadio::new();
    radio.civ_duplicate_reply = true;
    let mut w = connected(radio);
    assert_eq!(w.roundtrip(), Some(FREQ_REPLY.to_vec()));
    w.step(300);
    assert_eq!(w.delivered.len(), 1);
}

#[test]
fn echoes_are_dropped_and_a_missing_preamble_restored() {
    let mut w = connected(SimRadio::new());
    let ids = Ids {
        sender: sim::RADIO_ID,
        receiver: 0,
    };
    // the radio echoing our own command back, addressed to it (0x98)
    let echo = wire::civ(&READ_FREQ, 0xc0, 0, 900, ids).unwrap();
    // a reply missing its second FE
    let short = wire::civ(&FREQ_REPLY[1..], 0xc0, 0, 901, ids).unwrap();
    // an empty payload
    let empty = {
        let mut b = wire::civ(&[0xfd], 0xc0, 0, 902, ids).unwrap();
        b.truncate(wire::CIV_HEADER_LEN);
        put_le32(&mut b, 0, wire::CIV_HEADER_LEN as u32);
        put_le16(&mut b, 0x11, 0);
        b
    };
    for packet in [echo, short, empty] {
        w.inject(Role::Civ, &packet);
    }
    assert_eq!(w.delivered, [FREQ_REPLY.to_vec()]);
}

// upstream: test_session_retransmit_request
#[test]
fn retransmit_request() {
    // The radio asks for a tracked packet again: the first on the CI-V socket, the stream open.
    let mut w = connected(SimRadio::new());
    assert!(w.roundtrip().is_some());
    w.radio.ask_retransmit = Some(1);
    assert!(w.run_until(2000, |w| w.radio.retransmit_replies > 0));
    // and the caller's frame, sequence 2, comes back byte for byte
    let first = w
        .sent
        .iter()
        .find(|d| d.role == Role::Civ && le16(&d.bytes, 6) == 2 && d.bytes.len() > 0x16)
        .expect("the frame")
        .clone();
    w.radio.ask_retransmit = Some(2);
    let before = w.sent.len();
    w.step(20);
    assert!(w.sent[before..].contains(&first));
}

// upstream: test_session_ping_request
#[test]
fn ping_request() {
    // The radio pings the client; the client must answer, or the radio takes the session for gone.
    let mut w = connected(SimRadio::new());
    w.radio.ask_ping = true;
    assert!(w.run_until(2000, |w| w.radio.saw_ping_reply));
}

// ── liveness and socket errors ──────────────────────────────────────────────────────────────

/// The loss, and how long after the socket's last datagram it was reported.
fn loss_after_silence(w: &mut World, role: Role, max_ms: u64) -> (Loss, u64) {
    assert!(w.run_until(max_ms, |w| w.loss().is_some()), "not lost");
    let heard = w.last_heard[role as usize].expect("heard once");
    (w.loss().expect("lost"), w.now - heard)
}

// upstream: test_session_liveness_timeout
// upstream: test_liveness_timeout_token (the default, 5000 ms; there is no other setting)
#[test]
fn liveness_timeout() {
    // A radio that stops answering is noticed, not pinged at for ever.
    let mut w = connected(SimRadio::new());
    w.radio.go_silent = true;
    let (loss, after) = loss_after_silence(&mut w, Role::Control, 10_000);
    assert_eq!(loss.reason, LossReason::LinkTimeout);
    assert!(
        after > LIVENESS_MS && after <= LIVENESS_MS + 2 * STEP_MS,
        "{after}"
    );
}

// upstream: test_session_liveness_no_false_positive
#[test]
fn liveness_no_false_positive() {
    // A healthy session never trips the check, at several times the window.
    let mut w = connected(SimRadio::new());
    w.step(6 * LIVENESS_MS);
    assert_eq!(w.loss(), None);
    assert!(w.roundtrip().is_some());
}

// upstream: test_session_liveness_disabled (here liveness cannot be turned off)
#[test]
fn liveness_cannot_be_turned_off() {
    // Upstream offers 0 for "never". Here the window is fixed: silent just under it, the session
    // is still valid; past it, lost.
    let mut w = connected(SimRadio::new());
    w.radio.go_silent = true;
    let heard = w.now;
    w.step(LIVENESS_MS - 100);
    assert_eq!(w.loss(), None);
    w.step(200);
    assert!(w.loss().is_some());
    assert!(w.now - heard <= LIVENESS_MS + 100);
}

// upstream: test_session_civ_silence_is_link_timeout
#[test]
fn civ_silence_is_link_timeout() {
    // The radio stops serving CI-V while the control socket carries on with keepalives: each
    // socket has its own clock, so the session is lost even though control is busy.
    let mut w = connected(SimRadio::new());
    w.step(LIVENESS_MS + 1000);
    assert_eq!(w.loss(), None);
    w.radio.civ_silent = true;
    let (loss, after) = loss_after_silence(&mut w, Role::Civ, 10_000);
    assert_eq!(
        loss,
        Loss {
            reason: LossReason::LinkTimeout,
            socket: Role::Civ
        }
    );
    assert!(after <= LIVENESS_MS + 2 * STEP_MS, "{after}");
}

// upstream: test_session_civ_port_refusing_is_socket_error (on the fake clock; a real refused
// port is in `civ_port_refusing_is_socket_error_over_sockets`)
#[test]
fn civ_port_refusing_is_socket_error() {
    let mut w = connected(SimRadio::new());
    w.radio.refuse_civ = true;
    let mut refused = 0;
    while w.loss().is_none() && w.now < 20_000 {
        if w.civ(&READ_FREQ) == Err(CivError::SocketFailing) {
            refused += 1;
        }
        w.step(100);
    }
    assert!(refused > 0, "no frame was refused on the failing socket");
    assert_eq!(
        w.loss(),
        Some(Loss {
            reason: LossReason::SocketError,
            socket: Role::Civ
        })
    );
}

/// A connected session whose CI-V stream is provably quiet: no idles, no ping answers.
fn quiet_civ() -> World {
    let mut radio = SimRadio::new();
    radio.civ_idle_ms = 0;
    radio.civ_no_ping_reply = true;
    let mut w = connected(radio);
    w.radio.civ_silent = true;
    w
}

// upstream: test_session_injected_socket_error_is_socket_error
#[test]
fn injected_socket_error_is_socket_error() {
    // A socket that reported an error refuses frames at once, and silence after it is a socket
    // error rather than a timeout.
    let mut w = quiet_civ();
    w.session
        .on_socket_error(Role::Civ, SocketError::Hard, w.now);
    assert_eq!(w.civ(&READ_FREQ), Err(CivError::SocketFailing));
    let (loss, _) = loss_after_silence(&mut w, Role::Civ, 10_000);
    assert_eq!(loss.reason, LossReason::SocketError);
}

// upstream: test_session_socket_error_wins_over_a_silent_link
#[test]
fn socket_error_wins_over_a_silent_link() {
    // Sockets usually go quiet together; then the reason is the more telling one. Only CI-V
    // reports an error, and control, silent too and checked first, must not turn that into plain
    // silence. Both are put past the window in one move, so one check sees both.
    let mut w = quiet_civ();
    w.radio.go_silent = true;
    w.session
        .on_socket_error(Role::Civ, SocketError::Hard, w.now);
    w.jump(LIVENESS_MS + 500);
    assert_eq!(
        w.loss(),
        Some(Loss {
            reason: LossReason::SocketError,
            socket: Role::Civ
        })
    );
}

// upstream: test_session_transient_socket_error_is_not_a_failure
#[test]
fn transient_socket_error_is_not_a_failure() {
    // A momentary error is not a failing socket: the frame still goes out, and if the radio then
    // goes quiet the session is lost as plain silence.
    let mut w = quiet_civ();
    w.session
        .on_socket_error(Role::Civ, SocketError::Transient, w.now);
    assert_eq!(w.civ(&READ_FREQ), Ok(()));
    let (loss, _) = loss_after_silence(&mut w, Role::Civ, 10_000);
    assert_eq!(loss.reason, LossReason::LinkTimeout);
}

// upstream: test_session_socket_errors_respect_liveness_disabled (liveness cannot be turned off
// here; what carries over is that an error alone never ends the session before the window)
#[test]
fn socket_errors_respect_liveness_disabled() {
    let mut w = quiet_civ();
    let heard = w.last_heard[Role::Civ as usize].expect("heard");
    w.session
        .on_socket_error(Role::Civ, SocketError::Hard, w.now);
    let mut refused = 0;
    while w.now - heard < LIVENESS_MS - 100 {
        if w.civ(&READ_FREQ) == Err(CivError::SocketFailing) {
            refused += 1;
        }
        w.step(100);
        assert_eq!(
            w.loss(),
            None,
            "lost {} ms after the last datagram",
            w.now - heard
        );
    }
    assert!(refused > 0);
    w.step(500);
    assert_eq!(w.loss().map(|l| l.reason), Some(LossReason::SocketError));
}

#[test]
fn a_datagram_clears_the_socket_error() {
    let mut w = connected(SimRadio::new());
    w.session
        .on_socket_error(Role::Civ, SocketError::Hard, w.now);
    assert_eq!(w.civ(&READ_FREQ), Err(CivError::SocketFailing));
    w.step(60); // the radio's next CI-V idle arrives
    assert_eq!(w.civ(&READ_FREQ), Ok(()));
}

#[test]
fn socket_errors_are_sorted_like_upstream() {
    use std::io::{Error, ErrorKind};
    for kind in [ErrorKind::WouldBlock, ErrorKind::Interrupted] {
        assert_eq!(SocketError::of(&Error::from(kind)), SocketError::Transient);
    }
    for kind in [
        ErrorKind::ConnectionRefused,
        ErrorKind::ConnectionReset,
        ErrorKind::PermissionDenied,
    ] {
        assert_eq!(SocketError::of(&Error::from(kind)), SocketError::Hard);
    }
    #[cfg(target_os = "linux")]
    assert_eq!(
        SocketError::of(&Error::from_raw_os_error(105)), // ENOBUFS
        SocketError::Transient
    );
}

// upstream: test_session_peer_disconnect
#[test]
fn peer_disconnect() {
    // The radio announcing a disconnect is reported as such, not as a timeout: it tells the
    // operator someone else took the radio.
    let mut w = connected(SimRadio::new());
    w.radio.announce_disconnect = true;
    assert!(w.run_until(1000, |w| w.loss().is_some()));
    assert_eq!(
        w.loss(),
        Some(Loss {
            reason: LossReason::PeerDisconnect,
            socket: Role::Control
        })
    );
}

// ── keepalive and teardown ──────────────────────────────────────────────────────────────────

#[test]
fn the_keepalive_cadence() {
    // Once connected: an idle every 100 ms and a ping every 500 ms on control and on CI-V, and the
    // token renewed every 60 s with the radio's identity.
    let mut w = connected(SimRadio::new());
    let (from, start) = (w.sent.len(), w.now);
    w.step(TOKEN_RENEW_MS + 1000);
    let span = w.now - start;
    let count = |role: Role, pick: &dyn Fn(&Packet) -> bool| {
        w.sent[from..]
            .iter()
            .filter(|d| d.role == role)
            .filter(|d| wire::decode(d.role, &d.bytes).is_ok_and(|p| pick(&p)))
            .count() as u64
    };
    for role in [Role::Control, Role::Civ] {
        let idles = count(role, &|p| {
            matches!(
                p,
                Packet::Control {
                    op: ControlOp::Idle,
                    ..
                }
            )
        });
        let pings = count(role, &|p| matches!(p, Packet::Ping(q) if !q.is_reply));
        assert!(
            idles.abs_diff(span / IDLE_MS) <= 2,
            "{role:?} idles {idles}"
        );
        assert!(
            pings.abs_diff(span / PING_MS) <= 2,
            "{role:?} pings {pings}"
        );
    }
    let renewals: Vec<Packet> = w.sent[from..]
        .iter()
        .filter_map(|d| wire::decode(d.role, &d.bytes).ok())
        .filter(|p| matches!(p, Packet::Token(t) if t.request() == Some(TokenRequest::Renew)))
        .collect();
    assert_eq!(renewals.len(), 1);
    let Packet::Token(t) = &renewals[0] else {
        unreachable!()
    };
    let info = w.radio.connection_info.as_ref().expect("the request");
    assert_eq!(t.authid[..], info[0x20..0x30]);
    assert_eq!(t.token, sim::TOKEN);
    // the audio socket carries nothing while connected
    assert!(!w.sent.iter().any(|d| d.role == Role::Audio));
}

#[test]
fn teardown_closes_the_stream_returns_the_token_then_disconnects_every_socket() {
    let mut w = connected(SimRadio::new());
    let from = w.radio.received.len();
    assert!(w.close());
    let tail: Vec<(Role, String)> = w.radio.received[from..]
        .iter()
        .filter_map(|(r, b)| Some((*r, wire::decode(*r, b).ok()?)))
        .filter_map(|(r, p)| match p {
            Packet::OpenClose(o) => Some((r, format!("{:?}", o.op))),
            Packet::Token(t) => Some((r, format!("{:?}", t.request()))),
            Packet::Control {
                op: ControlOp::Disconnect,
                ..
            } => Some((r, "Disconnect".into())),
            _ => None,
        })
        .collect();
    assert_eq!(
        tail,
        [
            (Role::Civ, "Close".into()),
            (Role::Control, "Some(Remove)".into()),
            (Role::Audio, "Disconnect".into()),
            (Role::Civ, "Disconnect".into()),
            (Role::Control, "Disconnect".into()),
        ]
    );
    assert_eq!(w.state(), State::Closed);
}

#[test]
fn teardown_after_a_loss_does_not_wait() {
    // The packets are still sent, in case the loss was wrong, but no grace is waited out.
    let mut w = connected(SimRadio::new());
    w.radio.go_silent = true;
    assert!(w.run_until(10_000, |w| w.loss().is_some()));
    let actions = w.session.close(w.now);
    w.perform(actions);
    assert!(w.closed());
}

// upstream: test_session_free_during_reconnect_handshake
// upstream: test_session_free_during_reconnect_backoff
// upstream: test_session_free_without_connect
#[test]
fn a_closed_session_sends_nothing_more() {
    // Closing at any point ends with Closed, and nothing is sent after it, whatever arrives. (No
    // reconnect thread exists to outlive a close.)
    type Setup = Box<dyn Fn(&mut SimRadio)>;
    let scenarios: Vec<(&str, Setup, u64)> = vec![
        ("nothing answers", Box::new(|r| r.go_silent = true), 100),
        (
            "no login reply",
            Box::new(|r| r.drop_login_replies = 1000),
            100,
        ),
        (
            "no capabilities reply",
            Box::new(|r| r.drop_capabilities_replies = 1000),
            100,
        ),
        ("draining", Box::new(|_| {}), 0),
        ("connected", Box::new(|_| {}), 500),
    ];
    for (name, setup, run) in scenarios {
        let mut radio = SimRadio::new();
        setup(&mut radio);
        let mut w = world(radio);
        w.step(run);
        let actions = w.session.close(w.now);
        w.perform(actions);
        assert!(w.run_until(2000, World::closed), "{name}: {:?}", w.events);
        assert_eq!(w.state(), State::Closed);
        let after = w.sent.len();
        w.radio.go_silent = false;
        w.radio.ask_ping = true;
        w.step(3000);
        for packet in sim::valid_from_radio() {
            w.inject(packet.0, &packet.1);
        }
        assert_eq!(w.sent.len(), after, "{name}: sent after Closed");
        // a token went back exactly when one had been granted
        let granted = !matches!(name, "nothing answers" | "no login reply");
        assert_eq!(w.radio.token_removes > 0, granted, "{name}");
    }
}

// upstream: test_session_free_without_connect (the half where nothing listens)
#[test]
fn nothing_answering_fails_after_twenty_tries() {
    let mut radio = SimRadio::new();
    radio.go_silent = true;
    let mut w = world(radio);
    assert!(w.run_until(20_000, World::closed));
    assert_eq!(
        failure(&w),
        Some(ConnectError::NoAnswer(Stage::ControlProbe))
    );
    let probes = requests(&w, Role::Control)
        .iter()
        .filter(|k| *k == "Probe")
        .count() as u32;
    assert_eq!(probes, HANDSHAKE_TRIES);
    assert!(w.now >= u64::from(HANDSHAKE_TRIES) * HANDSHAKE_RESEND_MS);
    let disconnects = requests(&w, Role::Control)
        .iter()
        .filter(|k| *k == "Disconnect")
        .count();
    assert_eq!(disconnects, 1);
}

// upstream: test_session_reconnect_cycles
#[test]
fn reconnect_cycles() {
    // Repeated connect and close against one radio: each close leaves it ready for the next.
    let mut radio = SimRadio::new();
    for cycle in 0..5 {
        let mut w = World::start(radio, sim::config(Model::Ic7610));
        assert!(w.connect(), "cycle {cycle}");
        assert!(w.roundtrip().is_some(), "cycle {cycle}");
        assert!(w.close(), "cycle {cycle}");
        radio = w.radio;
    }
    assert_eq!(radio.token_removes, 5);
}

// upstream: test_session_reconnect_after_dirty_close
#[test]
fn reconnect_after_dirty_close() {
    // A close with a command still unanswered must not wedge the next session: the stale reply the
    // radio flushes at the next stream open is drained, not taken for the answer.
    let mut w = connected(SimRadio::new());
    w.radio.responder = Some(Box::new(|_| None)); // the reply never comes
    assert_eq!(w.civ(&READ_FREQ), Ok(()));
    assert!(w.close());
    let mut radio = w.radio;
    radio.responder = None;
    radio.stale_nak = true;
    let mut next = World::start(radio, sim::config(Model::Ic7610));
    assert!(next.connect());
    assert_eq!(next.roundtrip(), Some(FREQ_REPLY.to_vec()));
}

// ── reconnecting: the session never does it; the policy decides ─────────────────────────────

// upstream: test_silence_is_reported_as_link_timeout
#[test]
fn silence_is_reported_as_link_timeout() {
    let mut w = connected(SimRadio::new());
    w.radio.go_silent = true;
    assert!(w.run_until(10_000, |w| w.loss().is_some()));
    let reason = w.loss().expect("lost").reason;
    assert_eq!(reason, LossReason::LinkTimeout);
    assert_eq!(
        Ladder::new().after(&End::Lost(reason)),
        Next::RetryAfter(1000)
    );
}

// upstream: test_announced_disconnect_is_reported_as_peer
#[test]
fn announced_disconnect_is_reported_as_peer() {
    // A different answer from silence: reconnecting at once would only take the radio back from
    // whoever now holds it, so the policy waits for the operator.
    let mut w = connected(SimRadio::new());
    w.radio.announce_disconnect = true;
    assert!(w.run_until(1000, |w| w.loss().is_some()));
    let reason = w.loss().expect("lost").reason;
    assert_eq!(reason, LossReason::PeerDisconnect);
    assert_eq!(
        Ladder::new().after(&End::Lost(reason)),
        Next::WaitForOperator
    );
}

// upstream: test_no_reconnect_unless_asked
#[test]
fn no_reconnect_unless_asked() {
    // A lost session stays lost: it must not quietly reconnect behind an owner that has not asked.
    let mut w = connected(SimRadio::new());
    w.radio.go_silent = true;
    assert!(w.run_until(10_000, |w| w.loss().is_some()));
    let from = w.sent.len();
    w.radio.go_silent = false;
    w.step(3000);
    let handshake = w.sent[from..].iter().any(|d| {
        matches!(
            wire::decode(d.role, &d.bytes),
            Ok(Packet::Control {
                op: ControlOp::Probe | ControlOp::Ready,
                ..
            } | Packet::Login(_)
                | Packet::ConnectionInfo(_))
        ) || matches!(wire::decode(d.role, &d.bytes),
            Ok(Packet::Token(t)) if t.request() == Some(TokenRequest::Create))
    });
    assert!(!handshake, "the session started a handshake by itself");
    assert!(w.session.loss().is_some());
}

// upstream: test_auto_reconnect_recovers_the_session
#[test]
fn auto_reconnect_recovers_the_session() {
    // With the policy driving it: the old session is closed, and after the ladder's first wait a
    // new one connects to the radio that came back.
    let mut w = connected(SimRadio::new());
    w.radio.go_silent = true;
    assert!(w.run_until(10_000, |w| w.loss().is_some()));
    let mut ladder = Ladder::new();
    let Next::RetryAfter(wait) = ladder.after(&End::Lost(w.loss().unwrap().reason)) else {
        panic!("no retry")
    };
    assert!(w.close());
    w.radio.go_silent = false;
    w.step(wait);
    let mut next = World::start(w.radio, sim::config(Model::Ic7610));
    assert!(next.connect());
    ladder.connected();
    assert!(next.roundtrip().is_some());
}

// upstream: test_repeated_loss_cycles_are_clean
#[test]
fn repeated_loss_cycles_are_clean() {
    for cycle in 0..3 {
        let mut w = connected(SimRadio::new());
        w.radio.go_silent = true;
        assert!(w.run_until(10_000, |w| w.loss().is_some()), "cycle {cycle}");
        assert!(w.close(), "cycle {cycle}");
        // closing a session already gone still sends the close, the token and the disconnects
        let saw = |role: Role, pick: &dyn Fn(&Packet) -> bool| {
            w.radio_saw(role).iter().filter(|p| pick(p)).count()
        };
        assert_eq!(
            saw(Role::Control, &|p| matches!(
                p,
                Packet::Token(t) if t.request() == Some(TokenRequest::Remove)
            )),
            1
        );
        for role in [Role::Control, Role::Civ, Role::Audio] {
            assert_eq!(
                saw(role, &|p| matches!(
                    p,
                    Packet::Control {
                        op: ControlOp::Disconnect,
                        ..
                    }
                )),
                1,
                "cycle {cycle} {role:?}"
            );
        }
    }
}

// ── the configuration, as the radio receives it ─────────────────────────────────────────────

// upstream: test_credentials
#[test]
fn credentials() {
    // Exactly the wire's limit logs in, and the radio receives exactly those characters,
    // obfuscated; one more is refused when the value is made, before anything is sent, rather
    // than truncated into a login failure that reads as a wrong password.
    let password = "0123456789abcdef";
    let login = Login {
        user: User::new("oh-no-a-fake").unwrap(),
        password: Secret::new(password.into()).unwrap(),
    };
    let config = Config::new(std::net::Ipv4Addr::new(192, 0, 2, 10), Model::Ic7610, login);
    let mut w = World::start(SimRadio::new(), config);
    assert!(w.connect());
    let sent = &w.radio.logins[0];
    assert_eq!(sent[0x40..0x50], wire::passcode("oh-no-a-fake").unwrap());
    assert_eq!(sent[0x50..0x60], wire::passcode(password).unwrap());
    assert!(Secret::new("0123456789abcdefg".into()).is_err());
    assert!(User::new("0123456789abcdefg").is_err());
}

// upstream: test_numeric_ranges_enforced
#[test]
fn numeric_ranges_enforced() {
    // The control port defaults to 50001 and cannot be 0 or out of range: the type has no such
    // value. (The receive and transmit latency settings have no counterpart: no audio.)
    let config = sim::config(Model::Ic7610);
    assert_eq!(config.control_port.get(), wire::PORT_CONTROL);
    assert_eq!(NonZeroU16::new(0), None);
}

// upstream: test_defaults_after_init
#[test]
fn defaults_after_init() {
    // No radio index or name is set: the radio is picked by the model's own name.
    let config = sim::config(Model::Ic9700);
    assert_eq!(config.choice, None);
    let mut radio = SimRadio::new();
    radio.radios.insert(0, Entry::new("IC-7610", 0x98));
    radio.radios[1] = Entry::new("IC-9700", 0xa2);
    let mut w = World::start(radio, config);
    assert!(w.connect());
    assert_eq!(connected_radio(&w).name, "IC-9700");
}

#[test]
fn the_password_is_moved_into_the_login() {
    let w = world(SimRadio::new());
    assert!(w.radio.logins.len() == 1);
    assert!(!w.session.holds_password());
    // and nothing the session reports carries it, obfuscated or not
    let obfuscated = wire::passcode(sim::PASSWORD).unwrap();
    let text = format!("{:?}{:?}", w.events, w.sent);
    assert!(!text.contains(sim::PASSWORD));
    assert!(!text.contains(&format!("{:?}", &obfuscated[..4])));
}

#[test]
fn a_datagrams_debug_shows_no_bytes() {
    let d = Datagram {
        role: Role::Control,
        bytes: vec![0xAB; 0x80],
    };
    let text = format!("{d:?}");
    assert_eq!(text, "Datagram { role: Control, len: 128 }");
}

#[test]
fn a_frame_too_long_to_keep_is_refused() {
    let mut w = connected(SimRadio::new());
    let long = vec![0x00; MAX_CIV_FRAME + 1];
    assert_eq!(
        w.civ(&long),
        Err(CivError::TooLong {
            len: MAX_CIV_FRAME + 1
        })
    );
    assert!(matches!(w.civ(&[]), Err(CivError::Frame(_))));
    // nothing went out for either, and no sequence number was used
    assert!(w.roundtrip().is_some());
    assert_eq!(w.radio.civ_commands.len(), 1);
}

// ── the transmit rule's property ────────────────────────────────────────────────────────────

/// A random packet of `len` bytes with a consistent data header.
fn fixed(rng: &mut Rng, len: usize, seq: u16) -> Vec<u8> {
    let mut b = rng.bytes(len);
    put_le32(&mut b, 0, len as u32);
    put_le16(&mut b, 4, 0);
    put_le16(&mut b, 6, seq);
    b
}

/// A datagram such as the radio might send, or might garble.
fn random_datagram(rng: &mut Rng) -> Vec<u8> {
    let ids = Ids {
        sender: sim::RADIO_ID,
        receiver: rng.next() as u32,
    };
    let seq = rng.below(24) as u16;
    match rng.below(14) {
        0 => {
            let ops = [
                ControlOp::Idle,
                ControlOp::Probe,
                ControlOp::Present,
                ControlOp::Disconnect,
                ControlOp::Ready,
            ];
            wire::control(ops[rng.below(5)], seq, ids)
        }
        1 => wire::ping(rng.chance(50), rng.next() as u32, seq, ids),
        2 => {
            let wanted: Vec<u16> = (0..1 + rng.below(4))
                .map(|_| rng.below(24) as u16)
                .collect();
            wire::retransmit(&wanted, ids).unwrap()
        }
        3 => {
            // a range request
            let mut b = fixed(rng, wire::HEADER_LEN + 4, seq);
            put_le16(&mut b, 4, 1);
            let first = rng.below(20) as u16;
            put_le16(&mut b, 0x10, first);
            put_le16(&mut b, 0x12, first + rng.below(6) as u16);
            b
        }
        4 => {
            let mut b = fixed(rng, wire::STATUS_LEN, seq);
            put_le32(
                &mut b,
                0x30,
                if rng.chance(70) { 0 } else { rng.next() as u32 },
            );
            b[0x40] = u8::from(rng.chance(20));
            put_be16(&mut b, 0x42, rng.next() as u16);
            put_be16(&mut b, 0x46, rng.next() as u16);
            b
        }
        5 => {
            let mut b = fixed(rng, wire::LOGIN_RESPONSE_LEN, seq);
            put_le32(&mut b, 0x30, if rng.chance(70) { 0 } else { 1 });
            b
        }
        6 => {
            let count = 1 + rng.below(3);
            let len = caps::FIRST_ENTRY + count * caps::ENTRY_LEN;
            let mut b = fixed(rng, len, seq);
            put_be16(&mut b, 0x40, count as u16);
            for i in 0..count {
                let r = &mut b[caps::FIRST_ENTRY + i * caps::ENTRY_LEN..][..caps::ENTRY_LEN];
                let name = ["IC-7610", "IC-9700", "IC-705", "XX"][i % 4];
                r[0x10..0x30].fill(0);
                r[0x10..0x10 + name.len()].copy_from_slice(name.as_bytes());
                put_le16(r, 0x53, sim::ALL_RATES);
            }
            b
        }
        7 => fixed(rng, wire::TOKEN_LEN, seq),
        8 => {
            let n = 1 + rng.below(40);
            let frame = rng.bytes(n);
            wire::civ(
                &frame,
                [0xc0, 0xc1][rng.below(2)],
                rng.next() as u16,
                seq,
                ids,
            )
            .unwrap()
        }
        9 => {
            // a reply-shaped frame, addressed to the controller
            let mut frame = vec![0xfe, 0xfe, 0xe0, 0x98];
            let n = rng.below(12);
            frame.extend(rng.bytes(n));
            frame.push(0xfd);
            wire::civ(&frame, 0xc1, 0, seq, ids).unwrap()
        }
        10 => wire::openclose(
            [StreamOp::Open, StreamOp::Close][rng.below(2)],
            rng.next() as u16,
            seq,
            ids,
        ),
        11 => {
            let (_, mut b) = sim::valid_from_radio()[rng.below(4)].clone();
            let at = rng.below(b.len());
            b[at] ^= 1 << rng.below(8);
            b
        }
        12 => {
            let n = rng.below(0x60);
            rng.bytes(n)
        }
        _ => {
            let mut b = wire::control(ControlOp::Idle, seq, ids);
            let n = 1 + rng.below(4);
            b.extend(rng.bytes(n));
            b
        }
    }
}

/// What a run's traffic covered, so the property is known to have been exercised.
#[derive(Default)]
struct Coverage {
    civ_frames: usize,
    civ_resends: usize,
    connection_requests: usize,
    ping_replies: usize,
    replays: usize,
}

/// Reads everything the session sent the way the radio would, and checks the transmit rule.
fn check_what_was_sent(w: &World, seed: u64, coverage: &mut Coverage) {
    let handed: BTreeSet<&[u8]> = w.handed.iter().map(Vec::as_slice).collect();
    let mut civ_sequences = BTreeSet::new();
    for d in &w.sent {
        let packet = wire::decode(d.role, &d.bytes)
            .unwrap_or_else(|e| panic!("seed {seed}: a malformed {:?} packet: {e:?}", d.role));
        match (d.role, &packet) {
            (
                Role::Control | Role::Civ,
                Packet::Control {
                    op:
                        ControlOp::Probe | ControlOp::Ready | ControlOp::Idle | ControlOp::Disconnect,
                    ..
                }
                | Packet::Ping(_)
                | Packet::Retransmit(_),
            ) => {}
            (Role::Control, Packet::Login(_) | Packet::Token(_) | Packet::ConnectionInfo(_)) => {}
            (Role::Civ, Packet::OpenClose(_)) => {}
            (Role::Civ, Packet::Civ(c)) => {
                assert!(
                    handed.contains(c.frame.as_slice()),
                    "seed {seed}: a CI-V frame nobody handed over: {:02x?}",
                    c.frame
                );
                assert_eq!(c.reply, 0xc1, "seed {seed}");
                coverage.civ_frames += 1;
                if !civ_sequences.insert(c.header.seq) {
                    coverage.civ_resends += 1;
                }
            }
            (
                Role::Audio,
                Packet::Control {
                    op: ControlOp::Disconnect,
                    ..
                },
            ) => {}
            (role, other) => panic!("seed {seed}: {role:?} carried {other:?}"),
        }
        if let Packet::ConnectionInfo(_) = packet {
            assert_eq!(d.bytes[0x71], 0, "seed {seed}: transmit enable");
            coverage.connection_requests += 1;
        }
        if matches!(packet, Packet::Ping(p) if p.is_reply) {
            coverage.ping_replies += 1;
        }
        // The same rule read from the bytes alone: on the CI-V socket, a data packet that is not
        // a stream open or close carries a frame that was handed over.
        if d.role == Role::Civ && le16(&d.bytes, 4) == 0 && d.bytes.len() > wire::OPENCLOSE_LEN {
            assert_eq!(d.bytes[0x10], 0xc1, "seed {seed}");
            assert!(
                handed.contains(&d.bytes[wire::CIV_HEADER_LEN..]),
                "seed {seed}: {:02x?}",
                &d.bytes[wire::CIV_HEADER_LEN..]
            );
        }
        if d.role == Role::Audio {
            assert_eq!(
                (d.bytes.len(), le16(&d.bytes, 4)),
                (wire::HEADER_LEN, 0x0005),
                "seed {seed}: the audio socket carries only the disconnect"
            );
        }
    }
    // each frame handed over took one sequence number; resends reuse it
    assert!(civ_sequences.len() <= w.handed.len(), "seed {seed}");
}

#[test]
fn no_input_makes_the_session_send_a_civ_command() {
    // The transmit rule's property test. For hundreds of seeded runs, a session in each state
    // (stuck in the first stage, stuck holding a token, draining, connected with frames the test
    // handed it, lost, closing) is fed random datagrams on every socket (well-formed packets of
    // every kind, garbled ones and noise), random socket errors and time. Whatever it sends is
    // read back as the radio would read it: on the control socket only bare control, pings,
    // retransmit requests, the login, tokens and the connection request; on the CI-V socket only
    // bare control, pings, retransmit requests, the stream open and close, and CI-V packets whose
    // frame the test handed it; on the audio socket only the disconnect. Every connection request
    // carries transmit-enable 0. Checked by decoding and again from the raw bytes.
    let mut runs = 0;
    let mut coverage = Coverage::default();
    for seed in 1..=360u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let state = seed % 6;
        let mut radio = SimRadio::new();
        radio.ctrl_tracked = rng.chance(50);
        radio.civ_duplicate_reply = rng.chance(20);
        radio.stale_nak = rng.chance(30);
        match state {
            0 => radio.go_silent = true,
            1 => radio.drop_capabilities_replies = 1000,
            _ => {}
        }
        let mut w = world(radio);
        match state {
            2 => assert_eq!(w.state(), State::Draining),
            3 => {
                assert!(w.connect());
                for _ in 0..1 + rng.below(3) {
                    let mut frame = vec![0xfe, 0xfe, 0x98, 0xe0];
                    let n = 1 + rng.below(8);
                    frame.extend(rng.bytes(n));
                    frame.push(0xfd);
                    let _ = w.civ(&frame);
                }
            }
            4 => {
                assert!(w.connect());
                assert!(w.roundtrip().is_some());
                w.radio.go_silent = true;
                assert!(w.run_until(10_000, |w| w.loss().is_some()));
                w.radio.go_silent = false;
            }
            5 => {
                assert!(w.connect());
                assert!(w.roundtrip().is_some());
                let actions = w.session.close(w.now);
                w.perform(actions);
            }
            _ => {}
        }
        for _ in 0..50 {
            let role = [Role::Control, Role::Civ, Role::Audio][rng.below(3)];
            let bytes = random_datagram(&mut rng);
            w.inject(role, &bytes);
            if rng.chance(5) {
                let role = [Role::Control, Role::Civ, Role::Audio][rng.below(3)];
                let error = [SocketError::Transient, SocketError::Hard][rng.below(2)];
                w.session.on_socket_error(role, error, w.now);
            }
            if rng.chance(8) {
                let mut frame = vec![0xfe, 0xfe, 0x98, 0xe0];
                let n = 1 + rng.below(8);
                frame.extend(rng.bytes(n));
                frame.push(0xfd);
                let _ = w.civ(&frame);
            }
            w.step(rng.below(150) as u64);
            runs += 1;
        }
        let before = w.sent.len();
        // the radio asks for every sequence it could: whatever is stored goes out again
        let ids = Ids {
            sender: sim::RADIO_ID,
            receiver: 0,
        };
        for role in [Role::Control, Role::Civ] {
            let mut ask = wire::control(ControlOp::Idle, 0, ids);
            put_le32(&mut ask, 0, 0x14);
            put_le16(&mut ask, 4, 1);
            ask.extend_from_slice(&[0, 0, 0xff, 0]); // sequences 0 to 255
            w.inject(role, &ask);
        }
        coverage.replays += w.sent.len() - before;
        check_what_was_sent(&w, seed, &mut coverage);
    }
    assert!(runs >= 18_000);
    // The property was exercised, not passed by default: frames went out and came back on
    // request, connection requests were made, the radio's pings were answered.
    assert!(coverage.civ_frames > 300, "{}", coverage.civ_frames);
    assert!(coverage.civ_resends > 50, "{}", coverage.civ_resends);
    assert!(
        coverage.connection_requests > 200,
        "{}",
        coverage.connection_requests
    );
    assert!(coverage.ping_replies > 200, "{}", coverage.ping_replies);
    assert!(coverage.replays > 1000, "{}", coverage.replays);
}

// ── on real sockets ─────────────────────────────────────────────────────────────────────────

// upstream: test_session_handshake_and_civ_roundtrip (over loopback UDP)
#[test]
fn handshake_and_civ_roundtrip_over_sockets() {
    let radio = SocketRadio::start(SimRadio::new());
    let mut client = SocketClient::connect(&radio, sim::config(Model::Ic7610)).expect("sockets");
    assert!(
        client.run_until(Duration::from_secs(10), |c| c
            .events
            .iter()
            .any(|e| matches!(e, Event::Connected(_)))),
        "{:?}",
        client.events
    );
    client.send_civ(&READ_FREQ).expect("sent");
    assert!(client.run_until(Duration::from_secs(5), |c| !c.delivered.is_empty()));
    assert_eq!(client.delivered[0], FREQ_REPLY);
    assert!(client.close());
    // the radio's thread reads what was sent within a few milliseconds
    let end = std::time::Instant::now() + Duration::from_secs(2);
    while !radio.with(|r| r.token_removes == 1 && r.ctrl_disconnects == 1) {
        assert!(std::time::Instant::now() < end, "the radio saw no teardown");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// upstream: test_session_civ_port_refusing_is_socket_error
#[cfg(not(windows))]
#[test]
fn civ_port_refusing_is_socket_error_over_sockets() {
    // The radio's CI-V port closes. Linux and macOS report the refusal (an ICMP port-unreachable
    // reaching a connected UDP socket) as an error on the next send or receive; Windows does not
    // report it for a local peer, which is why the fake-clock version exists. Frames are refused
    // at once, and the session is lost as a socket error, not a timeout.
    let radio = SocketRadio::start(SimRadio::new());
    let mut client = SocketClient::connect(&radio, sim::config(Model::Ic7610)).expect("sockets");
    assert!(client.run_until(Duration::from_secs(10), |c| c
        .events
        .iter()
        .any(|e| matches!(e, Event::Connected(_)))));
    radio.with(|r| r.refuse_civ = true);
    let mut refused = 0;
    let lost = |c: &SocketClient| c.events.iter().any(|e| matches!(e, Event::Lost(_)));
    let end = std::time::Instant::now() + Duration::from_secs(12);
    while !lost(&client) && std::time::Instant::now() < end {
        if client.send_civ(&READ_FREQ) == Err(CivError::SocketFailing) {
            refused += 1;
        }
        client.run_until(Duration::from_millis(100), lost);
    }
    assert!(
        client
            .errors
            .iter()
            .any(|(role, e)| *role == Role::Civ && *e == SocketError::Hard),
        "the system reported no refusal: {:?}",
        client.errors
    );
    assert!(refused > 0);
    assert!(
        client.events.contains(&Event::Lost(Loss {
            reason: LossReason::SocketError,
            socket: Role::Civ
        })),
        "{:?}",
        client.events
    );
}
