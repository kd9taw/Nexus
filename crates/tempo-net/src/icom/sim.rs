//! A simulated network Icom for the tests, and the harnesses that run a session against it.
//!
//! - [`SimRadio`]: the radio's side of the handshake and of the CI-V stream, as a pure value: a
//!   datagram in, the datagrams it answers with out, time passed in. It answers the probes, the
//!   login, the token (with its radio list) and the connection request; numbers its replies as
//!   tracked packets and keeps them for retransmission, when asked to; loses the login or the
//!   capabilities reply (the session must recover it from the gap); ignores a resent request it
//!   already has; sends idles on the CI-V socket in the same sequence as its CI-V frames; sends a
//!   CI-V reply twice with one sequence number; stops the CI-V stream while the control socket
//!   carries on; refuses the CI-V socket; goes silent; sends an unsolicited disconnect, as a radio
//!   does when another client takes it over; answers the connection request with a status error,
//!   as a radio does while it still holds another session's slot; advertises a chosen radio list;
//!   asks for a retransmit and pings on request; and records every datagram it receives.
//! - [`World`]: a session and a radio on one fake clock, with no sockets at all.
//! - [`SocketRadio`] and [`SocketClient`]: the same radio on three real UDP sockets on ephemeral
//!   loopback ports, and a session on three more, for the paths only a real socket shows (a
//!   refused port reported by the system).
//!
//! PORTED from Hamlib (https://github.com/Hamlib/Hamlib, pull request #2178, open when taken),
//! `test/icom_network_mock.c` and `test/icom_network_mock.h` at commit
//! `2e3e4a6add3bd806e828d035200777608d2bdbd5` (2026-10-07), translated from C to Rust.
//! Deliberate differences: the radio is a pure value, and the sockets are a thin wrapper around
//! it on a thread, where upstream's mock is a thread around three sockets; the fake-clock world
//! is Nexus's own; the radio can refuse the login (to show a refused login gives nothing back);
//! the CI-V replies can come from a responder the test supplies; its own pings do not take a
//! number from the CI-V sequence, and a stream close is told apart from an open; the canned
//! frames, identities, addresses and credentials are made-up values; the spectrum frame, the
//! audio script and the withheld audio packet are not taken (no audio at this stage).
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

use std::collections::VecDeque;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::caps::{self, Model, COMMONCAP_MAC};
use super::conf::{Config, Login, Secret, User};
use super::session::{Action, CivError, Datagram, Event, Locals, Session, SocketError, State};
use super::wire::{
    self, put_be16, put_be32, put_le16, put_le32, ControlOp, Ids, Packet, Role, StreamOp,
    TokenRequest,
};

/// The id the simulated radio sends under.
pub(crate) const RADIO_ID: u32 = 0xA1A2_A3A4;
/// Every rate the protocol can express, which is what an IC-7610 advertises.
pub(crate) const ALL_RATES: u16 = 0x8b01;
/// The canned reply to a CI-V command: a frequency read's answer.
pub(crate) const FREQ_REPLY: [u8; 11] = [
    0xfe, 0xfe, 0xe0, 0x98, 0x03, 0x00, 0x60, 0x06, 0x14, 0x00, 0xfd,
];
/// A frequency read, the command the tests send.
pub(crate) const READ_FREQ: [u8; 6] = [0xfe, 0xfe, 0x98, 0xe0, 0x03, 0xfd];
/// The leftover a radio can flush when a stream opens: a NAK.
pub(crate) const STALE_NAK: [u8; 6] = [0xfe, 0xfe, 0xe0, 0x98, 0xfa, 0xfd];
/// The token the radio grants.
pub(crate) const TOKEN: u32 = 0x9999;
/// The test credentials: obvious fakes.
pub(crate) const USER: &str = "test-user";
pub(crate) const PASSWORD: &str = "not-a-password";

/// One radio the simulated server advertises.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) civ: u8,
    pub(crate) rx: u16,
    pub(crate) tx: u16,
}

impl Entry {
    pub(crate) fn new(name: &str, civ: u8) -> Entry {
        Entry {
            name: name.to_string(),
            civ,
            rx: ALL_RATES,
            tx: ALL_RATES,
        }
    }
}

type Responder = Box<dyn FnMut(&[u8]) -> Option<Vec<u8>> + Send>;

/// The simulated radio. Behaviours are public fields a test sets; records are public fields a
/// test reads.
pub(crate) struct SimRadio {
    // ── behaviours ──
    pub(crate) radios: Vec<Entry>,
    /// The CI-V and audio ports the status reply names.
    pub(crate) civ_port: u16,
    pub(crate) audio_port: u16,
    pub(crate) no_audio_port: bool,
    /// Answer the connection request with this error (0 = success).
    pub(crate) status_error: u32,
    /// Answer the login with this error (0 = accepted).
    pub(crate) login_error: u32,
    /// Number the control replies and idles as tracked packets, keep them, answer retransmits.
    pub(crate) ctrl_tracked: bool,
    /// Ignore a login, token or connection request whose sequence was already received.
    pub(crate) ctrl_ignore_resends: bool,
    /// Lose this many login replies (each still takes its number).
    pub(crate) drop_login_replies: u32,
    /// Lose this many capabilities replies.
    pub(crate) drop_capabilities_replies: u32,
    /// Idles on the CI-V socket this often, numbered with the CI-V frames (0 = none).
    pub(crate) civ_idle_ms: u64,
    pub(crate) civ_duplicate_reply: bool,
    /// Send nothing on the CI-V socket while the control socket carries on.
    pub(crate) civ_silent: bool,
    pub(crate) civ_no_ping_reply: bool,
    /// The CI-V port refuses packets.
    pub(crate) refuse_civ: bool,
    /// Answer nothing at all.
    pub(crate) go_silent: bool,
    /// Send an unsolicited disconnect, once.
    pub(crate) announce_disconnect: bool,
    /// Ask for a retransmit of this CI-V sequence, once.
    pub(crate) ask_retransmit: Option<u16>,
    /// Ping the client on the CI-V socket, once.
    pub(crate) ask_ping: bool,
    /// Flush a leftover NAK when the CI-V stream opens.
    pub(crate) stale_nak: bool,
    /// Skip this many CI-V sequence numbers before the next reply, once.
    pub(crate) civ_sequence_jump: u16,
    /// Reply with a frame of this many bytes (0 = the canned reply).
    pub(crate) civ_reply_length: usize,
    /// Answers CI-V commands when set; `None` from it sends no reply.
    pub(crate) responder: Option<Responder>,
    // ── records ──
    pub(crate) received: Vec<(Role, Vec<u8>)>,
    pub(crate) connection_info: Option<Vec<u8>>,
    pub(crate) logins: Vec<Vec<u8>>,
    pub(crate) token_removes: u32,
    pub(crate) ctrl_disconnects: u32,
    pub(crate) civ_disconnects: u32,
    pub(crate) audio_disconnects: u32,
    pub(crate) civ_closes: u32,
    pub(crate) civ_commands: Vec<Vec<u8>>,
    pub(crate) ctrl_retransmit_requests: u32,
    pub(crate) civ_retransmit_requests: u32,
    pub(crate) saw_ping_reply: bool,
    /// Datagrams carrying the sequence a retransmit was asked for.
    pub(crate) retransmit_replies: u32,
    // ── state ──
    ctrl_seq: u16,
    civ_seq: u16,
    ping_seq: u16,
    ctrl_sent: VecDeque<(u16, Vec<u8>)>,
    ctrl_seen: Vec<u16>,
    logged_in: bool,
    asked_for: Option<u16>,
    last_ctrl_idle: Option<u64>,
    last_civ_idle: Option<u64>,
    ctrl_client: Option<u32>,
    civ_client: Option<u32>,
}

impl SimRadio {
    /// One IC-7610 offering every rate, as real hardware reports.
    pub(crate) fn new() -> SimRadio {
        SimRadio {
            radios: vec![Entry::new("IC-7610", 0x98)],
            civ_port: wire::PORT_CIV,
            audio_port: wire::PORT_AUDIO,
            no_audio_port: false,
            status_error: 0,
            login_error: 0,
            ctrl_tracked: false,
            ctrl_ignore_resends: false,
            drop_login_replies: 0,
            drop_capabilities_replies: 0,
            civ_idle_ms: 50,
            civ_duplicate_reply: false,
            civ_silent: false,
            civ_no_ping_reply: false,
            refuse_civ: false,
            go_silent: false,
            announce_disconnect: false,
            ask_retransmit: None,
            ask_ping: false,
            stale_nak: false,
            civ_sequence_jump: 0,
            civ_reply_length: 0,
            responder: None,
            received: Vec::new(),
            connection_info: None,
            logins: Vec::new(),
            token_removes: 0,
            ctrl_disconnects: 0,
            civ_disconnects: 0,
            audio_disconnects: 0,
            civ_closes: 0,
            civ_commands: Vec::new(),
            ctrl_retransmit_requests: 0,
            civ_retransmit_requests: 0,
            saw_ping_reply: false,
            retransmit_replies: 0,
            ctrl_seq: 0, // the radio numbers its first reply 0
            civ_seq: 0,
            ping_seq: 0,
            ctrl_sent: VecDeque::new(),
            ctrl_seen: Vec::new(),
            logged_in: false,
            asked_for: None,
            last_ctrl_idle: None,
            last_civ_idle: None,
            ctrl_client: None,
            civ_client: None,
        }
    }

    fn ids(client: u32) -> Ids {
        Ids {
            sender: RADIO_ID,
            receiver: client,
        }
    }

    /// Sends a control-socket packet the way the radio sends its tracked ones: with the next
    /// number, kept for retransmission. `drop` loses it on the way (still numbered and kept).
    /// Without `ctrl_tracked` it goes out untracked, with sequence 0.
    fn ctrl_send(&mut self, mut packet: Vec<u8>, drop: bool, out: &mut Vec<(Role, Vec<u8>)>) {
        if self.ctrl_tracked {
            let seq = self.ctrl_seq;
            self.ctrl_seq = seq.wrapping_add(1);
            put_le16(&mut packet, 6, seq);
            self.ctrl_sent.push_back((seq, packet.clone()));
            if self.ctrl_sent.len() > 16 {
                self.ctrl_sent.pop_front();
            }
        }
        if !drop {
            out.push((Role::Control, packet));
        }
    }

    /// A datagram from the client on the socket `role`; returns what the radio sends back.
    pub(crate) fn receive(&mut self, role: Role, bytes: &[u8], _now: u64) -> Vec<(Role, Vec<u8>)> {
        self.received.push((role, bytes.to_vec()));
        let mut out = Vec::new();
        if self.go_silent || (role == Role::Civ && (self.civ_silent || self.refuse_civ)) {
            return out;
        }
        let Ok(packet) = wire::decode(role, bytes) else {
            return out;
        };
        let header = *packet.header();
        let client = header.ids.sender;
        if let Some(asked) = self.asked_for {
            if role == Role::Civ && header.seq == asked && !matches!(packet, Packet::Ping(_)) {
                self.retransmit_replies += 1;
            }
        }
        match role {
            Role::Control => self.service_control(packet, client, &mut out),
            Role::Civ => self.service_civ(packet, client, &mut out),
            Role::Audio => {
                if let Packet::Control {
                    op: ControlOp::Disconnect,
                    ..
                } = packet
                {
                    self.audio_disconnects += 1;
                }
            }
        }
        out
    }

    fn service_control(&mut self, packet: Packet, client: u32, out: &mut Vec<(Role, Vec<u8>)>) {
        self.ctrl_client = Some(client);
        let ids = Self::ids(client);
        let seq = packet.header().seq;
        if self.ctrl_ignore_resends
            && matches!(
                packet,
                Packet::Login(_) | Packet::Token(_) | Packet::ConnectionInfo(_)
            )
        {
            if self.ctrl_seen.contains(&seq) {
                return;
            }
            self.ctrl_seen.push(seq);
        }
        match packet {
            Packet::Control {
                op: ControlOp::Disconnect,
                ..
            } => self.ctrl_disconnects += 1,
            Packet::Control {
                op: ControlOp::Probe,
                ..
            } => out.push((Role::Control, wire::control(ControlOp::Present, 0, ids))),
            Packet::Control {
                op: ControlOp::Ready,
                ..
            } => out.push((Role::Control, wire::control(ControlOp::Ready, 0, ids))),
            Packet::Retransmit(r) => {
                self.ctrl_retransmit_requests += 1;
                for range in r.wanted {
                    let mut s = range.first;
                    loop {
                        if let Some((_, p)) = self.ctrl_sent.iter().find(|(q, _)| *q == s) {
                            out.push((Role::Control, p.clone()));
                        }
                        if s == range.last {
                            break;
                        }
                        s = s.wrapping_add(1);
                    }
                }
            }
            Packet::Token(t) if t.request() == Some(TokenRequest::Remove) => {
                self.token_removes += 1
            }
            Packet::Login(_) => {
                self.logins
                    .push(self.received.last().expect("this login").1.clone());
                let mut lr = vec![0u8; wire::LOGIN_RESPONSE_LEN];
                put_le32(&mut lr, 0, wire::LOGIN_RESPONSE_LEN as u32);
                put_be32(&mut lr, 8, RADIO_ID);
                put_be32(&mut lr, 12, client);
                put_be16(&mut lr, 0x1a, 0x1234);
                put_be32(&mut lr, 0x1c, TOKEN);
                put_le32(&mut lr, 0x30, self.login_error);
                lr[0x40..0x44].copy_from_slice(b"FTTH");
                let drop = self.drop_login_replies > 0;
                self.drop_login_replies = self.drop_login_replies.saturating_sub(1);
                self.ctrl_send(lr, drop, out);
                self.logged_in = true;
            }
            // Any other token is answered with the radio list, as upstream's mock does.
            Packet::Token(_) => {
                let cp = self.capabilities(client);
                let drop = self.drop_capabilities_replies > 0;
                self.drop_capabilities_replies = self.drop_capabilities_replies.saturating_sub(1);
                self.ctrl_send(cp, drop, out);
            }
            Packet::ConnectionInfo(_) => {
                self.connection_info = Some(self.received.last().expect("this request").1.clone());
                let mut st = vec![0u8; wire::STATUS_LEN];
                put_le32(&mut st, 0, wire::STATUS_LEN as u32);
                put_be32(&mut st, 8, RADIO_ID);
                put_be32(&mut st, 12, client);
                put_le32(&mut st, 0x30, self.status_error);
                put_be16(&mut st, 0x42, self.civ_port);
                let audio = if self.no_audio_port {
                    0
                } else {
                    self.audio_port
                };
                put_be16(&mut st, 0x46, audio);
                self.ctrl_send(st, false, out);
            }
            Packet::Ping(p) if !p.is_reply => {
                out.push((Role::Control, wire::ping(true, p.time, p.header.seq, ids)))
            }
            _ => {}
        }
    }

    /// The advertised radio list (MAC-mode identities, as an IC-7610 reports them; made-up
    /// addresses).
    fn capabilities(&self, client: u32) -> Vec<u8> {
        let len = caps::FIRST_ENTRY + self.radios.len() * caps::ENTRY_LEN;
        let mut cp = vec![0u8; len];
        put_le32(&mut cp, 0, len as u32);
        put_be32(&mut cp, 8, RADIO_ID);
        put_be32(&mut cp, 12, client);
        put_be16(&mut cp, 0x40, self.radios.len() as u16);
        for (i, radio) in self.radios.iter().enumerate() {
            let r = &mut cp[caps::FIRST_ENTRY + i * caps::ENTRY_LEN..][..caps::ENTRY_LEN];
            put_le16(r, 0x07, COMMONCAP_MAC);
            r[0x0a..0x10].copy_from_slice(&[0x02, 0x00, 0x5e, 0x00, 0x53, 0x00]);
            r[0x0f] = i as u8;
            r[0x10..0x10 + radio.name.len()].copy_from_slice(radio.name.as_bytes());
            r[0x30..0x30 + 11].copy_from_slice(b"ICOM_VAUDIO");
            put_le16(r, 0x50, 0x073f); // Ethernet
            r[0x52] = radio.civ;
            put_le16(r, 0x53, radio.rx);
            put_le16(r, 0x55, radio.tx);
            put_be32(r, 0x5a, 19200);
        }
        cp
    }

    fn civ_packet(&mut self, frame: &[u8], reply: u8, client: u32) -> Vec<u8> {
        let seq = self.civ_seq;
        self.civ_seq = seq.wrapping_add(1);
        wire::civ(frame, reply, 0, seq, Self::ids(client)).expect("a CI-V packet")
    }

    fn service_civ(&mut self, packet: Packet, client: u32, out: &mut Vec<(Role, Vec<u8>)>) {
        let ids = Self::ids(client);
        match packet {
            Packet::Control {
                op: ControlOp::Probe,
                ..
            } => out.push((Role::Civ, wire::control(ControlOp::Present, 0, ids))),
            Packet::Control {
                op: ControlOp::Ready,
                ..
            } => out.push((Role::Civ, wire::control(ControlOp::Ready, 0, ids))),
            Packet::Control {
                op: ControlOp::Disconnect,
                ..
            } => self.civ_disconnects += 1,
            Packet::Retransmit(_) => self.civ_retransmit_requests += 1,
            Packet::OpenClose(o) if o.op == StreamOp::Open => {
                self.civ_client = Some(client);
                if self.stale_nak {
                    let p = self.civ_packet(&STALE_NAK, 0xc0, client);
                    out.push((Role::Civ, p));
                }
            }
            Packet::OpenClose(_) => self.civ_closes += 1,
            Packet::Civ(c) => {
                self.civ_commands.push(c.frame.clone());
                let reply = match &mut self.responder {
                    Some(responder) => responder(&c.frame),
                    None if self.civ_reply_length >= 6 => {
                        // FE FE E0 98 1A 00 <filler> FD
                        let mut f = vec![0x41u8; self.civ_reply_length];
                        f[..6].copy_from_slice(&[0xfe, 0xfe, 0xe0, 0x98, 0x1a, 0x00]);
                        f[self.civ_reply_length - 1] = 0xfd;
                        Some(f)
                    }
                    None => Some(FREQ_REPLY.to_vec()),
                };
                if self.civ_sequence_jump > 0 {
                    self.civ_seq = self.civ_seq.wrapping_add(self.civ_sequence_jump);
                    self.civ_sequence_jump = 0;
                }
                if let Some(reply) = reply {
                    let p = self.civ_packet(&reply, 0xc1, client);
                    if self.civ_duplicate_reply {
                        out.push((Role::Civ, p.clone()));
                    }
                    out.push((Role::Civ, p));
                }
            }
            Packet::Ping(p) if p.is_reply => self.saw_ping_reply = true,
            Packet::Ping(p) if !self.civ_no_ping_reply => {
                out.push((Role::Civ, wire::ping(true, p.time, p.header.seq, ids)))
            }
            _ => {}
        }
    }

    /// What the radio sends unprompted by now.
    pub(crate) fn tick(&mut self, now: u64) -> Vec<(Role, Vec<u8>)> {
        let mut out = Vec::new();
        if self.go_silent {
            return out;
        }
        // The radio's tracked idles, which reveal a lost reply by the gap. Like the radio, none
        // before the login reply, which is therefore its first tracked packet.
        if let Some(client) = self.ctrl_client {
            if self.ctrl_tracked
                && self.logged_in
                && self
                    .last_ctrl_idle
                    .is_none_or(|t| now.saturating_sub(t) >= 100)
            {
                let idle = wire::control(ControlOp::Idle, 0, Self::ids(client));
                self.ctrl_send(idle, false, &mut out);
                self.last_ctrl_idle = Some(now);
            }
            if self.announce_disconnect {
                self.announce_disconnect = false;
                let mut st = vec![0u8; wire::STATUS_LEN];
                put_le32(&mut st, 0, wire::STATUS_LEN as u32);
                put_be32(&mut st, 8, RADIO_ID);
                put_be32(&mut st, 12, client);
                st[0x40] = 0x01; // the disconnect flag
                out.push((Role::Control, st));
            }
        }
        if let Some(client) = self.civ_client {
            if self.civ_silent || self.refuse_civ {
                return out;
            }
            if self.civ_idle_ms > 0
                && self
                    .last_civ_idle
                    .is_none_or(|t| now.saturating_sub(t) >= self.civ_idle_ms)
            {
                let seq = self.civ_seq;
                self.civ_seq = seq.wrapping_add(1);
                out.push((
                    Role::Civ,
                    wire::control(ControlOp::Idle, seq, Self::ids(client)),
                ));
                self.last_civ_idle = Some(now);
            }
            if let Some(want) = self.ask_retransmit.take() {
                self.asked_for = Some(want);
                out.push((
                    Role::Civ,
                    wire::retransmit(&[want], Self::ids(client)).expect("one sequence"),
                ));
            }
            if self.ask_ping {
                self.ask_ping = false;
                let seq = self.ping_seq;
                self.ping_seq = seq.wrapping_add(1);
                out.push((Role::Civ, wire::ping(false, 0x1234, seq, Self::ids(client))));
            }
        }
        out
    }
}

/// The test configuration: a documentation address, made-up credentials.
pub(crate) fn config(model: Model) -> Config {
    let login = Login {
        user: User::new(USER).expect("a user name"),
        password: Secret::new(PASSWORD.into()).expect("a password"),
    };
    Config::new(Ipv4Addr::new(192, 0, 2, 10), model, login)
}

/// The local addresses the pure world's session is given: loopback, made-up ports.
pub(crate) fn locals() -> Locals {
    let at = |port| SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    Locals {
        control: at(51001),
        civ: at(51002),
        audio: at(51003),
    }
}

/// A session and a radio on one fake clock. Every datagram arrives at once; time moves in 5 ms
/// steps, each step ticking the radio and polling the session.
pub(crate) struct World {
    pub(crate) session: Session,
    pub(crate) radio: SimRadio,
    pub(crate) now: u64,
    /// Every datagram the session sent, in order.
    pub(crate) sent: Vec<Datagram>,
    /// When each one went.
    pub(crate) sent_at: Vec<u64>,
    pub(crate) delivered: Vec<Vec<u8>>,
    pub(crate) events: Vec<Event>,
    pub(crate) redirects: Vec<(Role, u16)>,
    /// Every frame the test handed to the session.
    pub(crate) handed: Vec<Vec<u8>>,
    /// When the session last had a datagram on each socket.
    pub(crate) last_heard: [Option<u64>; 3],
}

pub(crate) const STEP_MS: u64 = 5;

/// The most actions one [`World::perform`] carries out. The largest real exchange (the radio
/// asking for every stored packet on both sockets, and answering what comes back) is a few
/// thousand.
const PERFORM_BUDGET: u32 = 100_000;

/// Well-formed packets of the kinds the radio sends.
pub(crate) fn valid_from_radio() -> Vec<(Role, Vec<u8>)> {
    let ids = Ids {
        sender: RADIO_ID,
        receiver: 0,
    };
    let mut status = vec![0u8; wire::STATUS_LEN];
    put_le32(&mut status, 0, wire::STATUS_LEN as u32);
    put_be32(&mut status, 8, RADIO_ID);
    status[0x40] = 0x01;
    vec![
        (Role::Control, wire::control(ControlOp::Present, 0, ids)),
        (Role::Civ, wire::control(ControlOp::Idle, 3, ids)),
        (Role::Civ, wire::ping(false, 9, 1, ids)),
        (
            Role::Civ,
            wire::retransmit(&[1, 2], ids).expect("two sequences"),
        ),
        (
            Role::Civ,
            wire::civ(&FREQ_REPLY, 0xc1, 0, 4, ids).expect("a frame"),
        ),
        (Role::Control, status),
    ]
}

impl World {
    pub(crate) fn start(radio: SimRadio, config: Config) -> World {
        let (session, actions) = Session::start(config, locals(), 0);
        let mut w = World {
            session,
            radio,
            now: 0,
            sent: Vec::new(),
            sent_at: Vec::new(),
            delivered: Vec::new(),
            events: Vec::new(),
            redirects: Vec::new(),
            handed: Vec::new(),
            last_heard: [None; 3],
        };
        w.perform(actions);
        w
    }

    /// A datagram to the session, noting when its socket last heard one.
    fn pass_to_session(&mut self, role: Role, bytes: &[u8]) -> Vec<Action> {
        self.last_heard[role as usize] = Some(self.now);
        self.session.on_datagram(role, bytes, self.now)
    }

    /// Carries out the session's actions, and the radio's answers, until nothing is left. An
    /// exchange that never settles (each side answering the other for ever, with no time
    /// passing) fails the test rather than growing the queue until the process runs out of memory.
    pub(crate) fn perform(&mut self, actions: Vec<Action>) {
        let mut queue: VecDeque<Action> = actions.into();
        let mut budget = PERFORM_BUDGET;
        while let Some(action) = queue.pop_front() {
            budget -= 1;
            assert!(
                budget > 0,
                "the session and the radio answered each other {PERFORM_BUDGET} times at {} ms",
                self.now
            );
            match action {
                Action::Send(d) => {
                    self.sent.push(d.clone());
                    self.sent_at.push(self.now);
                    if d.role == Role::Civ && self.radio.refuse_civ {
                        // the system reports the refused port on the socket
                        self.radio.received.push((d.role, d.bytes.clone()));
                        self.session
                            .on_socket_error(Role::Civ, SocketError::Hard, self.now);
                        continue;
                    }
                    for (role, reply) in self.radio.receive(d.role, &d.bytes, self.now) {
                        queue.extend(self.pass_to_session(role, &reply));
                    }
                }
                Action::Redirect { role, port } => self.redirects.push((role, port)),
                Action::Deliver(frame) => self.delivered.push(frame),
                Action::Event(e) => self.events.push(e),
            }
        }
    }

    /// A datagram straight into the session, as if from the radio.
    pub(crate) fn inject(&mut self, role: Role, bytes: &[u8]) {
        let actions = self.pass_to_session(role, bytes);
        self.perform(actions);
    }

    /// Advances the clock by `ms`, in steps.
    pub(crate) fn step(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now = (self.now + STEP_MS).min(end);
            for (role, bytes) in self.radio.tick(self.now) {
                let actions = self.pass_to_session(role, &bytes);
                self.perform(actions);
            }
            let actions = self.session.poll(self.now);
            self.perform(actions);
        }
    }

    /// Moves the clock by `ms` at once, with nothing in between, then polls once.
    pub(crate) fn jump(&mut self, ms: u64) {
        self.now += ms;
        let actions = self.session.poll(self.now);
        self.perform(actions);
    }

    /// Steps until `done` holds, for at most `max_ms`. Returns whether it held.
    pub(crate) fn run_until(&mut self, max_ms: u64, done: impl Fn(&World) -> bool) -> bool {
        let end = self.now + max_ms;
        while !done(self) {
            if self.now >= end {
                return false;
            }
            self.step(STEP_MS);
        }
        true
    }

    pub(crate) fn connected(&self) -> bool {
        self.events.iter().any(|e| matches!(e, Event::Connected(_)))
    }

    pub(crate) fn closed(&self) -> bool {
        self.events.contains(&Event::Closed)
    }

    /// Runs the handshake; true once connected.
    pub(crate) fn connect(&mut self) -> bool {
        self.run_until(15_000, World::connected)
    }

    /// Hands the session a frame.
    pub(crate) fn civ(&mut self, frame: &[u8]) -> Result<(), CivError> {
        let actions = self.session.send_civ(frame, self.now)?;
        self.handed.push(frame.to_vec());
        self.perform(actions);
        Ok(())
    }

    /// A frequency read and the frame delivered for it, if one comes within a second.
    pub(crate) fn roundtrip(&mut self) -> Option<Vec<u8>> {
        let before = self.delivered.len();
        self.civ(&READ_FREQ).ok()?;
        self.run_until(1000, |w| w.delivered.len() > before);
        self.delivered.get(before).cloned()
    }

    /// Closes the session and runs until it says it has closed.
    pub(crate) fn close(&mut self) -> bool {
        let actions = self.session.close(self.now);
        self.perform(actions);
        self.run_until(2000, World::closed)
    }

    /// The loss the session reported, if any.
    pub(crate) fn loss(&self) -> Option<super::session::Loss> {
        self.events.iter().find_map(|e| match e {
            Event::Lost(loss) => Some(*loss),
            _ => None,
        })
    }

    /// Everything the radio received on `role` that decodes.
    pub(crate) fn radio_saw(&self, role: Role) -> Vec<Packet> {
        self.radio
            .received
            .iter()
            .filter(|(r, _)| *r == role)
            .filter_map(|(r, b)| wire::decode(*r, b).ok())
            .collect()
    }

    pub(crate) fn state(&self) -> State {
        self.session.state()
    }
}

// ── on real sockets ─────────────────────────────────────────────────────────────────────────

fn millis(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// The simulated radio on three UDP sockets bound to ephemeral loopback ports, on its own
/// thread. Dropping it stops the thread.
pub(crate) struct SocketRadio {
    pub(crate) radio: Arc<Mutex<SimRadio>>,
    pub(crate) control_port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SocketRadio {
    pub(crate) fn start(mut radio: SimRadio) -> SocketRadio {
        let bind = || {
            let s = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("an ephemeral port");
            s.set_nonblocking(true).expect("non-blocking");
            s
        };
        let control = bind();
        let mut civ = Some(bind());
        let audio = bind();
        let port = |s: &UdpSocket| s.local_addr().expect("a local address").port();
        let control_port = port(&control);
        radio.civ_port = civ.as_ref().map(port).expect("the CI-V socket");
        radio.audio_port = port(&audio);
        let radio = Arc::new(Mutex::new(radio));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, stopping) = (radio.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let start = Instant::now();
            let mut peers: [Option<SocketAddr>; 3] = [None; 3];
            let mut buf = [0u8; 2048];
            while !stopping.load(Ordering::Relaxed) {
                let mut radio = shared.lock().expect("the radio");
                let now = millis(start);
                if radio.refuse_civ {
                    civ = None; // closed: the client's packets to it are refused
                }
                let mut replies = Vec::new();
                for (i, socket) in [Some(&control), civ.as_ref(), Some(&audio)]
                    .into_iter()
                    .enumerate()
                {
                    let Some(socket) = socket else { continue };
                    let role = [Role::Control, Role::Civ, Role::Audio][i];
                    while let Ok((n, from)) = socket.recv_from(&mut buf) {
                        peers[i] = Some(from);
                        replies.extend(radio.receive(role, &buf[..n], now));
                    }
                }
                replies.extend(radio.tick(now));
                drop(radio);
                for (role, bytes) in replies {
                    let i = role as usize;
                    let socket = [Some(&control), civ.as_ref(), Some(&audio)][i];
                    if let (Some(socket), Some(peer)) = (socket, peers[i]) {
                        let _ = socket.send_to(&bytes, peer);
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        SocketRadio {
            radio,
            control_port,
            stop,
            thread: Some(thread),
        }
    }

    pub(crate) fn with<T>(&self, f: impl FnOnce(&mut SimRadio) -> T) -> T {
        f(&mut self.radio.lock().expect("the radio"))
    }
}

impl Drop for SocketRadio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A session on three real UDP sockets, driven from the test's thread.
pub(crate) struct SocketClient {
    pub(crate) session: Session,
    sockets: [UdpSocket; 3],
    start: Instant,
    pub(crate) delivered: Vec<Vec<u8>>,
    pub(crate) events: Vec<Event>,
    /// Errors the sockets reported, as the session was told them.
    pub(crate) errors: Vec<(Role, SocketError)>,
}

impl SocketClient {
    pub(crate) fn connect(radio: &SocketRadio, config: Config) -> io::Result<SocketClient> {
        let open = || -> io::Result<UdpSocket> {
            let s = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
            s.connect((Ipv4Addr::LOCALHOST, radio.control_port))?;
            s.set_nonblocking(true)?;
            Ok(s)
        };
        let sockets = [open()?, open()?, open()?];
        let local = |s: &UdpSocket| -> io::Result<SocketAddrV4> {
            match s.local_addr()? {
                SocketAddr::V4(a) => Ok(a),
                SocketAddr::V6(_) => Err(io::Error::other("not IPv4")),
            }
        };
        let locals = Locals {
            control: local(&sockets[0])?,
            civ: local(&sockets[1])?,
            audio: local(&sockets[2])?,
        };
        let start = Instant::now();
        let (session, actions) = Session::start(config, locals, 0);
        let mut client = SocketClient {
            session,
            sockets,
            start,
            delivered: Vec::new(),
            events: Vec::new(),
            errors: Vec::new(),
        };
        client.perform(actions);
        Ok(client)
    }

    fn now(&self) -> u64 {
        millis(self.start)
    }

    fn report(&mut self, role: Role, e: &io::Error) {
        let error = SocketError::of(e);
        self.errors.push((role, error));
        self.session
            .on_socket_error(role, error, millis(self.start));
    }

    fn perform(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send(d) => {
                    if let Err(e) = self.sockets[d.role as usize].send(&d.bytes) {
                        self.report(d.role, &e);
                    }
                }
                Action::Redirect { role, port } => {
                    if let Err(e) = self.sockets[role as usize].connect((Ipv4Addr::LOCALHOST, port))
                    {
                        self.report(role, &e);
                    }
                }
                Action::Deliver(frame) => self.delivered.push(frame),
                Action::Event(e) => self.events.push(e),
            }
        }
    }

    /// Reads whatever has arrived, then polls.
    pub(crate) fn pump(&mut self) {
        let mut buf = [0u8; 2048];
        for role in [Role::Control, Role::Civ, Role::Audio] {
            loop {
                match self.sockets[role as usize].recv(&mut buf) {
                    Ok(n) => {
                        let now = self.now();
                        let actions = self.session.on_datagram(role, &buf[..n], now);
                        self.perform(actions);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        self.report(role, &e);
                        break;
                    }
                }
            }
        }
        let now = self.now();
        let actions = self.session.poll(now);
        self.perform(actions);
    }

    /// Pumps until `done` holds, for at most `max`.
    pub(crate) fn run_until(
        &mut self,
        max: Duration,
        done: impl Fn(&SocketClient) -> bool,
    ) -> bool {
        let end = Instant::now() + max;
        while !done(self) {
            if Instant::now() >= end {
                return false;
            }
            self.pump();
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }

    pub(crate) fn send_civ(&mut self, frame: &[u8]) -> Result<(), CivError> {
        let now = self.now();
        let actions = self.session.send_civ(frame, now)?;
        self.perform(actions);
        Ok(())
    }

    pub(crate) fn close(&mut self) -> bool {
        let now = self.now();
        let actions = self.session.close(now);
        self.perform(actions);
        self.run_until(Duration::from_secs(2), |c| {
            c.events.contains(&Event::Closed)
        })
    }
}
