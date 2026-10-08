//! Per-socket sequence bookkeeping: the replay buffer that answers the radio's retransmit
//! requests, and the receive tracker that notices the radio's lost packets and asks for them.
//! Pure: no sockets, time passed in as milliseconds on a monotonic clock.
//!
//! **The replay buffer** ([`TxBuffer`]) keeps the last [`SEQBUF_MAX`] tracked packets this end
//! sent, by sequence number, so a retransmit request can be answered with the very bytes that
//! went out. The oldest slot is overwritten when it is full; a packet over [`SEQBUF_PKTMAX`] is
//! not kept; entries older than the purge age are dropped.
//!
//! **The receive tracker** ([`RxTracker`]) watches the radio's sequence numbers. A forward jump
//! leaves the numbers in between missing; [`RxTracker::due`] hands them out for a retransmit
//! request, again every period, at most [`RETRANSMIT_MAX`] times each. A copy of a packet already
//! received is a duplicate and is dropped, so a repeated CI-V reply is never taken for the answer
//! to the next command. A packet that never arrived before is never a duplicate, even from
//! behind. A jump wider than [`MISSING_FLUSH`], more than that many missing, or a sender that has
//! started numbering again (a packet [`RESTART_DISTANCE`] behind, or [`RESTART_RUN`] new ones in
//! a row from behind) is a resync: the missing set cannot be recovered and tracking starts again.
//!
//! PORTED from Hamlib (https://github.com/Hamlib/Hamlib, pull request #2178, open when taken),
//! `rigs/icom/network_seqbuf.c` and `rigs/icom/network_seqbuf.h` at commit
//! `2e3e4a6add3bd806e828d035200777608d2bdbd5` (2026-10-07), translated from C to Rust.
//! Deliberate differences: time is passed in as unsigned milliseconds, and a missing sequence
//! never asked for is "due now" by an explicit state rather than a timestamp a million
//! milliseconds in the past; a stored packet is a slice of its own length rather than a fixed
//! 1500-byte slot; the replay buffer reports a packet it cannot keep as a typed error; the
//! tracker's state is private, with its counts readable.
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
//! Modified by KD9TAW, 2026-10-07: translated from C to Rust and changed as listed
//! above. These changes are licensed under the GNU General Public License, version 3 only, as Nexus
//! is (see COPYING), so this file as a whole is distributed under GPL version 3.

use std::fmt;

/// Tracked packets kept per socket, and the most missing sequences tracked.
pub const SEQBUF_MAX: usize = 256;
/// The largest packet the replay buffer keeps.
pub const SEQBUF_PKTMAX: usize = 1500;
/// How many times one missing sequence is asked for before it is given up.
pub const RETRANSMIT_MAX: u32 = 4;
/// A forward jump wider than this, or more missing than this, is a resync.
pub const MISSING_FLUSH: u16 = 50;
/// A packet this far behind the newest is a sender that started numbering again.
pub const RESTART_DISTANCE: u16 = SEQBUF_MAX as u16;
/// This many new packets in a row from behind the newest is a sender that started again.
pub const RESTART_RUN: u32 = 4;

/// Whether `a` is strictly after `b`, across the 16-bit wrap.
fn after(a: u16, b: u16) -> bool {
    (a.wrapping_sub(b) as i16) > 0
}

/// A packet the replay buffer could not keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge {
    pub len: usize,
}

impl fmt::Display for TooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a {}-byte packet is too large to keep for a retransmit",
            self.len
        )
    }
}

struct Stored {
    seq: u16,
    at_ms: u64,
    bytes: Vec<u8>,
}

/// The packets this end sent, kept for the other end's retransmit requests.
///
/// Its `Debug` shows how many packets it holds, never their bytes: one of them can be the login,
/// whose credential fields are only obfuscated.
pub struct TxBuffer {
    slots: Vec<Option<Stored>>,
    head: usize,
}

impl fmt::Debug for TxBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TxBuffer")
            .field("held", &self.len())
            .finish()
    }
}

impl Default for TxBuffer {
    fn default() -> Self {
        TxBuffer::new()
    }
}

impl TxBuffer {
    pub fn new() -> TxBuffer {
        TxBuffer {
            slots: (0..SEQBUF_MAX).map(|_| None).collect(),
            head: 0,
        }
    }

    /// Keeps a sent packet, overwriting the oldest slot when full.
    pub fn add(&mut self, seq: u16, packet: &[u8], now_ms: u64) -> Result<(), TooLarge> {
        if packet.len() > SEQBUF_PKTMAX {
            return Err(TooLarge { len: packet.len() });
        }
        self.slots[self.head] = Some(Stored {
            seq,
            at_ms: now_ms,
            bytes: packet.to_vec(),
        });
        self.head = (self.head + 1) % SEQBUF_MAX;
        Ok(())
    }

    /// The bytes sent under `seq`, if still held.
    pub fn get(&self, seq: u16) -> Option<&[u8]> {
        self.slots
            .iter()
            .flatten()
            .find(|s| s.seq == seq)
            .map(|s| s.bytes.as_slice())
    }

    /// Drops everything at least `age_ms` old.
    pub fn purge(&mut self, now_ms: u64, age_ms: u64) {
        for slot in &mut self.slots {
            if slot
                .as_ref()
                .is_some_and(|s| now_ms.saturating_sub(s.at_ms) >= age_ms)
            {
                *slot = None;
            }
        }
    }

    /// How many packets are held.
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What a received sequence number turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxResult {
    /// The next one, or one past a gap.
    New,
    /// One that was missing, arriving late.
    Recovered,
    /// Already received: discard the packet.
    Duplicate,
    /// New, but the gaps can no longer be recovered: the tracker has started again from it.
    Resync,
}

#[derive(Debug, Clone, Copy)]
struct Missing {
    seq: u16,
    retries: u32,
    /// When it was last asked for; `None` until the first request, which is due at once.
    asked_ms: Option<u64>,
}

/// The receive side's gap tracking.
#[derive(Debug, Clone)]
pub struct RxTracker {
    started: bool,
    last: u16,
    missing: Vec<Missing>,
    /// The most recent sequence received for each low byte, so a packet is called a duplicate
    /// only when that very number arrived before.
    received: [Option<u16>; 256],
    /// New packets in a row from behind the newest.
    behind_run: u32,
}

impl Default for RxTracker {
    fn default() -> Self {
        RxTracker::new()
    }
}

impl RxTracker {
    pub fn new() -> RxTracker {
        RxTracker {
            started: false,
            last: 0,
            missing: Vec::new(),
            received: [None; 256],
            behind_run: 0,
        }
    }

    /// Forgets everything: the next packet starts tracking afresh.
    pub fn reset(&mut self) {
        self.started = false;
        self.missing.clear();
        self.behind_run = 0;
        self.received = [None; 256];
    }

    /// Starts tracking a stream whose first packet will carry `first`, so losing that very packet
    /// is noticed from the gap the next one leaves.
    pub fn expect(&mut self, first: u16) {
        self.reset();
        self.started = true;
        self.last = first.wrapping_sub(1);
    }

    fn mark_received(&mut self, seq: u16) {
        self.received[usize::from(seq & 0xff)] = Some(seq);
    }

    fn was_received(&self, seq: u16) -> bool {
        self.received[usize::from(seq & 0xff)] == Some(seq)
    }

    fn find(&self, seq: u16) -> Option<usize> {
        self.missing.iter().position(|m| m.seq == seq)
    }

    fn add_missing(&mut self, seq: u16) {
        if self.find(seq).is_some() || self.missing.len() >= SEQBUF_MAX {
            return;
        }
        self.missing.push(Missing {
            seq,
            retries: 0,
            asked_ms: None,
        });
    }

    /// Observes a received data-packet sequence number.
    pub fn observe(&mut self, seq: u16, _now_ms: u64) -> RxResult {
        if !self.started {
            self.started = true;
            self.last = seq;
            self.behind_run = 0;
            self.mark_received(seq);
            return RxResult::New;
        }

        // At or behind the newest: a retransmit asked for, a copy of something already here, or
        // a sender that started counting again.
        if !after(seq, self.last) {
            if let Some(index) = self.find(seq) {
                self.missing.swap_remove(index);
                self.mark_received(seq);
                self.behind_run = 0;
                return RxResult::Recovered;
            }
            if self.was_received(seq) {
                return RxResult::Duplicate;
            }
            // Never received: deliver it. A lone one is a packet given up on that turned up after
            // all; far behind, or several in a row, it is a restart, and gap tracking has to
            // start again from here.
            self.behind_run += 1;
            if self.last.wrapping_sub(seq) > RESTART_DISTANCE || self.behind_run >= RESTART_RUN {
                self.reset();
                self.started = true;
                self.last = seq;
                self.mark_received(seq);
                return RxResult::Resync;
            }
            self.mark_received(seq);
            return RxResult::New;
        }

        self.behind_run = 0;
        self.mark_received(seq);

        // A forward jump: everything between the newest and this one is missing.
        let gap = seq.wrapping_sub(self.last);
        if gap > MISSING_FLUSH {
            // Too wide to recover packet by packet.
            self.missing.clear();
            self.last = seq;
            return RxResult::Resync;
        }
        let mut s = self.last.wrapping_add(1);
        while s != seq {
            self.add_missing(s);
            s = s.wrapping_add(1);
        }
        self.last = seq;
        if self.missing.len() > usize::from(MISSING_FLUSH) {
            RxResult::Resync
        } else {
            RxResult::New
        }
    }

    /// Marks `seq` received (removes it from the missing set).
    pub fn received(&mut self, seq: u16) {
        if let Some(index) = self.find(seq) {
            self.missing.swap_remove(index);
        }
    }

    /// Up to `max` missing sequences due for a (re)request: never asked, or last asked at least
    /// `period_ms` ago. Each one handed out counts a retry; one past [`RETRANSMIT_MAX`] is given
    /// up instead.
    pub fn due(&mut self, now_ms: u64, period_ms: u64, max: usize) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.missing.len() && out.len() < max {
            let m = self.missing[i];
            if m.asked_ms
                .is_some_and(|at| now_ms.saturating_sub(at) < period_ms)
            {
                i += 1;
                continue;
            }
            if m.retries >= RETRANSMIT_MAX {
                // Give up on this one; index i now holds a different entry.
                self.missing.swap_remove(i);
                continue;
            }
            out.push(m.seq);
            let m = &mut self.missing[i];
            m.retries += 1;
            m.asked_ms = Some(now_ms);
            i += 1;
        }
        out
    }

    /// How many sequences are missing.
    pub fn missing(&self) -> usize {
        self.missing.len()
    }

    /// The newest sequence seen, once tracking has started.
    pub fn last(&self) -> Option<u16> {
        self.started.then_some(self.last)
    }
}

#[cfg(test)]
mod tests {
    //! Translated from upstream's `test/test_icom_network_seqbuf.c`, all twelve cases, each named
    //! after the case it translates.
    use super::*;

    // upstream: test_txbuf_add_get
    #[test]
    fn txbuf_add_get() {
        let mut tb = TxBuffer::new();
        assert_eq!(tb.add(1, &[0xAA, 0xBB, 0xCC], 0), Ok(()));
        assert_eq!(tb.add(2, &[0x11, 0x22], 0), Ok(()));
        assert_eq!(tb.get(1), Some(&[0xAA, 0xBB, 0xCC][..]));
        assert_eq!(tb.get(2), Some(&[0x11, 0x22][..]));
        assert_eq!(tb.get(99), None);
    }

    // upstream: test_txbuf_purge
    #[test]
    fn txbuf_purge() {
        let mut tb = TxBuffer::new();
        tb.add(7, &[0x5], 1000).unwrap();
        assert!(tb.get(7).is_some());
        tb.purge(5000, 10000); // not old enough
        assert!(tb.get(7).is_some());
        tb.purge(12000, 10000); // now older than 10 s
        assert!(tb.get(7).is_none());
    }

    // upstream: test_txbuf_overwrite_oldest
    #[test]
    fn txbuf_overwrite_oldest() {
        let mut tb = TxBuffer::new();
        // fill exactly the size, then one more: the first sequence is overwritten
        for i in 0..SEQBUF_MAX {
            tb.add(1000 + i as u16, &[0], 0).unwrap();
        }
        assert!(tb.get(1000).is_some());
        tb.add(9999, &[0], 0).unwrap();
        assert!(tb.get(1000).is_none());
        assert!(tb.get(9999).is_some());
        // a packet too large is not kept
        assert_eq!(
            tb.add(1, &[0u8; SEQBUF_PKTMAX + 1], 0),
            Err(TooLarge {
                len: SEQBUF_PKTMAX + 1
            })
        );
    }

    // upstream: test_txbuf_oversize_rejected
    #[test]
    fn txbuf_oversize_rejected() {
        let mut tb = TxBuffer::new();
        assert!(tb.add(1, &[0u8; SEQBUF_PKTMAX + 8], 0).is_err());
        assert!(tb.add(1, &[0u8; SEQBUF_PKTMAX], 0).is_ok());
    }

    #[test]
    fn the_replay_buffers_debug_shows_no_bytes() {
        // One of the kept packets can be the login, whose credentials are only obfuscated.
        let mut tb = TxBuffer::new();
        tb.add(3, &[0xAB; 8], 0).unwrap();
        let text = format!("{tb:?}");
        assert!(text.contains('1'), "{text}");
        assert!(
            !text.contains("171") && !text.to_lowercase().contains("ab"),
            "{text}"
        );
    }

    // upstream: test_rxtrack_sequential
    #[test]
    fn rxtrack_sequential() {
        let mut rt = RxTracker::new();
        assert_eq!(rt.observe(10, 0), RxResult::New);
        assert_eq!(rt.observe(11, 0), RxResult::New);
        assert_eq!(rt.observe(12, 0), RxResult::New);
        assert_eq!(rt.missing(), 0);
    }

    // upstream: test_rxtrack_gap_and_recover
    #[test]
    fn rxtrack_gap_and_recover() {
        let mut rt = RxTracker::new();
        rt.observe(10, 0);
        rt.observe(13, 0); // 11 and 12 missing
        assert_eq!(rt.missing(), 2);
        // a retransmit of 11 arrives, behind the newest
        assert_eq!(rt.observe(11, 0), RxResult::Recovered);
        assert_eq!(rt.missing(), 1);
        rt.received(12);
        assert_eq!(rt.missing(), 0);
    }

    // upstream: test_rxtrack_due_and_retry_cap
    #[test]
    fn rxtrack_due_and_retry_cap() {
        let mut rt = RxTracker::new();
        rt.observe(10, 0);
        rt.observe(13, 0); // 11 and 12 missing
                           // due at once on the first poll
        assert_eq!(rt.due(0, 100, 8).len(), 2);
        // not due again before the period passes
        assert!(rt.due(50, 100, 8).is_empty());
        // retries 2, 3 and 4 at successive periods
        for k in 2..=u64::from(RETRANSMIT_MAX) {
            assert_eq!(rt.due(k * 100, 100, 8).len(), 2);
        }
        // past the cap: given up, nothing handed out
        assert!(rt.due(1000, 100, 8).is_empty());
        assert_eq!(rt.missing(), 0);
    }

    // upstream: test_rxtrack_flush_on_large_jump
    #[test]
    fn rxtrack_flush_large_jump() {
        let mut rt = RxTracker::new();
        rt.observe(10, 0);
        // a jump past the flush threshold is a resync
        assert_eq!(rt.observe(10 + MISSING_FLUSH + 5, 0), RxResult::Resync);
        assert_eq!(rt.missing(), 0);
    }

    // upstream: test_rxtrack_sequence_wrap
    #[test]
    fn rxtrack_sequence_wrap() {
        let mut rt = RxTracker::new();
        rt.observe(0xfffe, 0);
        rt.observe(0x0001, 0); // wraps: 0xffff and 0x0000 missing
        assert_eq!(rt.missing(), 2);
        rt.received(0xffff);
        rt.received(0x0000);
        assert_eq!(rt.missing(), 0);
    }

    // upstream: test_rxtrack_duplicates
    #[test]
    fn rxtrack_duplicates() {
        // A packet already received is a duplicate, whether it is the newest again or an older
        // one; a missing one is not.
        let mut rt = RxTracker::new();
        rt.observe(10, 0);
        rt.observe(11, 0);
        rt.observe(14, 0); // 12 and 13 missing
        assert_eq!(rt.observe(14, 0), RxResult::Duplicate);
        assert_eq!(rt.observe(11, 0), RxResult::Duplicate);
        assert_eq!(rt.observe(12, 0), RxResult::Recovered);
        // recovered once; a second copy is a duplicate
        assert_eq!(rt.observe(12, 0), RxResult::Duplicate);
        assert_eq!(rt.missing(), 1);
        assert_eq!(rt.observe(15, 0), RxResult::New);
        // across the wrap
        let mut rt = RxTracker::new();
        rt.observe(0xffff, 0);
        rt.observe(0x0000, 0);
        assert_eq!(rt.observe(0xffff, 0), RxResult::Duplicate);
    }

    // upstream: test_rxtrack_restart_is_resync
    #[test]
    fn rxtrack_restart_is_resync() {
        // A reopened stream: the previous client's leftovers with their old, high numbers, then
        // the new stream counting from zero. None of the new packets is a duplicate, and tracking
        // starts again from them.
        let mut rt = RxTracker::new();
        for seq in 190..=200 {
            rt.observe(seq, 0);
        }
        assert_eq!(rt.observe(1, 0), RxResult::New);
        assert_eq!(rt.observe(2, 0), RxResult::New);
        assert_eq!(rt.observe(3, 0), RxResult::New);
        assert_eq!(rt.observe(4, 0), RxResult::Resync);
        // tracking follows the new stream: a gap there is noticed
        assert_eq!(rt.observe(5, 0), RxResult::New);
        assert_eq!(rt.observe(7, 0), RxResult::New);
        assert_eq!(rt.due(0, 100, 8), [6]);
        // a long old stream: far behind is a restart at once
        let mut rt = RxTracker::new();
        rt.observe(5000, 0);
        assert_eq!(rt.observe(1, 0), RxResult::Resync);
        assert_eq!(rt.observe(2, 0), RxResult::New);
        // one packet given up on and arriving afterwards is delivered, not dropped, and is no
        // restart
        let mut rt = RxTracker::new();
        rt.observe(10, 0);
        rt.observe(13, 0); // 11 and 12 missing
        rt.missing.clear(); // as if given up
        assert_eq!(rt.observe(11, 0), RxResult::New);
        assert_eq!(rt.observe(11, 0), RxResult::Duplicate);
        assert_eq!(rt.observe(14, 0), RxResult::New);
    }

    // upstream: test_rxtrack_expect_first
    #[test]
    fn rxtrack_expect_first() {
        // Expecting the first sequence makes a lost first packet a gap like any other.
        let mut rt = RxTracker::new();
        rt.expect(0);
        assert_eq!(rt.observe(1, 0), RxResult::New);
        assert_eq!(rt.due(0, 100, 8), [0]);
        assert_eq!(rt.observe(0, 0), RxResult::Recovered);
        // and a stream that does start at the expected number has no gap
        rt.expect(0);
        assert_eq!(rt.observe(0, 0), RxResult::New);
        assert_eq!(rt.missing(), 0);
    }

    #[test]
    fn due_hands_out_at_most_max_and_keeps_the_rest() {
        let mut rt = RxTracker::new();
        rt.observe(0, 0);
        rt.observe(11, 0); // 1 to 10 missing
        assert_eq!(rt.due(0, 100, 4).len(), 4);
        assert_eq!(rt.due(0, 100, 100).len(), 6);
        assert_eq!(rt.missing(), 10);
    }
}
