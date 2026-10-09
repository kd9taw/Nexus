//! The faults the simulator injects, and the guard each one exists to trip.
//!
//! Each fault is a positive control. A guard in Nexus's Flex client is proven by a test that runs
//! it against the simulator with the matching fault and goes red when the guard is removed: a
//! check that has never been seen to fire is not yet a check. The guards themselves are not in
//! this crate. Each variant names the one it is for, by what the guard does and where the spec or
//! the port plan requires it.

use std::time::Duration;

/// A fault, set in [`crate::server::Config::faults`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// **Reordered or lost VITA packets.** Packets of the synthetic stream `stream_id` are dropped
    /// or swapped in transit. Indices count the stream's packets from 0 in the order they were
    /// generated (an FFT frame is several packets). The 4-bit packet count is assigned before the
    /// fault acts, as on a real network: a drop leaves a gap in the counts, and a swap of `k`
    /// sends packet `k + 1` before packet `k`.
    ///
    /// *Guard:* receive-side continuity on the packet count. A lost DAX audio packet is replaced
    /// by its own duration of silence and a late one is dropped, never spliced in
    /// (`tempo_net::flexvita::VitaSequence` today). FFT frame assembly never emits a frame that is
    /// missing a fragment (the coverage check of `tempo_net::flex::vita::FftAssembler`).
    /// Spec §6.7 names reordered VITA as one of these controls.
    Vita {
        stream_id: u32,
        drop: Vec<usize>,
        swap: Vec<usize>,
    },

    /// **Split TCP lines.** Every line the radio sends that contains `containing` goes out in
    /// pieces, cut at these byte offsets into the line and its terminator (none: in the middle).
    /// Each later piece waits until the client sends its next command, or `max_hold` passes, so
    /// a client reads the first piece on its own. No other line is ever sent between the pieces:
    /// a split only moves delivery boundaries, as TCP does.
    ///
    /// *Guard:* the session's line assembly, which never returns a partial line as a value (spec
    /// §10.5, "A partial reply is never a reading"). A client that parses each read as whole
    /// lines reads `RF_frequency=14.07` where the radio said `14.074000`.
    SplitLine {
        containing: String,
        cuts: Vec<usize>,
        max_hold: Duration,
    },

    /// **Reordered replies.** The reply to a command whose text equals `command` is held back
    /// and sent after the reply to the next command. Its statuses go out as usual.
    ///
    /// *Guard:* replies matched by sequence number, never by arrival order (spec §10.5, the same
    /// row as the split line).
    ReorderReply { command: String },

    /// **A dropped ping.** Pings `first` to `first + count - 1` on a connection (counted from 1)
    /// are treated as lost: no reply, and they do not refresh the radio's keepalive. After
    /// `keepalive enable`, the simulator closes a session that has sent no surviving ping for
    /// [`crate::server::Config::keepalive_timeout`], as the radio does after 15 s
    /// (TCPIP-keepalive).
    ///
    /// *Guard:* the client's keepalive. Five missed ping replies end the session and one does not
    /// (port plan §4.5), and during an over a missed ping unkeys locally before anything else
    /// (spec §6.3; §10.5, "A radio-side backstop").
    DropPings { first: usize, count: usize },

    /// **`tx=1` that never clears.** `xmit 0` is acknowledged with success, but the interlock
    /// never leaves TRANSMITTING: none of the rule's statuses is sent, and the radio stays keyed
    /// under that handle. A later connection's `sub tx all` reports the transmitter still held by
    /// the old handle.
    ///
    /// *Guard:* the unkey readback (port plan §3.4). Keyed is cleared only by the interlock
    /// sequence (UNKEY_REQUESTED, READY, then READY with `tx_client_handle=0x00000000`), never by
    /// the reply to `xmit 0`. When the 5-second transition deadline passes, the client sends
    /// `xmit 0` again, closes the session and tells the operator the radio did not confirm the
    /// unkey (spec §10.5, "Wall-clock watchdog": "`tx=1` never clears → escalation").
    StuckTransmit,

    /// **A tune carrier that never drops.** `transmit tune 0` is acknowledged with success, but
    /// the carrier stays up: none of the rule's statuses is sent, and the radio stays tuning under
    /// that handle, also after its connection closes. A later connection's `sub tx all` reports
    /// `transmit tune=1` and the transmitter held by the old handle.
    ///
    /// *Guard:* the readback per kind, the tune's profile. A tune counts as ended only when the
    /// transmit status says `tune=0` and the interlock is idle again, never on the reply to
    /// `transmit tune 0`. When the stop is not proven within the deadline, the client sends
    /// `transmit tune 0` again and `xmit 0`, closes the session and tells the operator.
    StuckTune,

    /// **A foreign client's slice and pan.** Another GUI client is on the radio: its `client`,
    /// `slice`, `display pan` and `display waterfall` status lines follow the session's own
    /// answers to `sub client all`, `sub slice all` and `sub pan all`, and `sub tx all` reports
    /// what [`Foreign`] says about the transmitter.
    ///
    /// *Guard:* ownership by `client_handle`. Another client's slice, pan and waterfall are shown
    /// but never retuned, adopted or removed (the port's ownership policies, port plan §2.1), and
    /// Nexus keys only when the TX slice is its own and the interlock names no transmitting
    /// client, or names Nexus (port plan §4.6; spec §10.5, "Whose transmitter"). The evidence is
    /// the transcript: no command naming the foreign slice, pan or waterfall
    /// ([`Foreign::is_named_by`]), and no `xmit 1` while the transmitter is not ours to take.
    ForeignClient(Foreign),

    /// **A disconnect mid-over.** The first time a client keys (`xmit 1` answered with success),
    /// the simulator closes that TCP session `after` the interlock reports TRANSMITTING. Whether
    /// a real radio unkeys a client whose connection drops is not established (port plan §3.4
    /// leaves it to the bench); `radio_stays_keyed` plays the worse answer, and the next
    /// connection then sees the transmitter held by the dropped handle.
    ///
    /// *Guard:* losing the session while keyed. The client unkeys locally before anything else,
    /// stops feeding DAX TX, reconnects on its ladder without swapping the connection under a
    /// keyed transmitter, and restores the audio routing at the next connect (spec §10.5, "No
    /// connection swap while keyed" and "Audio routing").
    DisconnectMidOver {
        after: Duration,
        radio_stays_keyed: bool,
    },

    /// **Another program feeds DAX transmit audio.** Another client (SmartSDR's DAX, typically)
    /// is on the radio with its own `dax_tx` stream: its `client` line and its stream's status
    /// follow the session's answer to `sub client all`. The handle, stream id, client id and
    /// program name are invented: what a client can know from the wire is the stream and whose it
    /// is.
    ///
    /// *Guard:* coexistence (operator ruling, 2026-10-03: "when SmartSDR's DAX is also
    /// connected, never write the flag"). Nexus never writes `transmit set dax`, never creates
    /// its own DAX transmit stream and never sends a DAX TX packet beside it; its transmit audio
    /// stays on the sound card route (port plan §3.2, the DAX TX and `transmit set dax` rows).
    ForeignDaxTx(ForeignDax),

    /// **A refused DAX transmit stream.** `stream create type=dax_tx` is answered with an error
    /// ([`REFUSED`]) and no stream.
    ///
    /// *Guard:* no route without a stream. Nexus does not point the transmitter at DAX and sends
    /// no DAX TX packet, and with native audio on a digital over is refused at the key rather
    /// than sent on the radio's mic. (The older native path re-routed the transmitter to a
    /// stream whose create had failed, leaving the mic dead and nothing on the air: the
    /// 2026-08-17 Flex audit.)
    DaxTxRefused,

    /// **A DAX receive stream the radio drops.** `after` the first `stream create type=dax_rx`
    /// on a connection, the radio reports `stream_id` removed (a profile load or a slice teardown
    /// does this on a real radio, as AetherSDR records).
    ///
    /// *Guard:* the refcounted DAX broker. A stream removed while a slice still holds its channel
    /// is created again after the recreate delay (port plan §2.1, the broker row).
    DropDaxRx { stream_id: u32, after: Duration },
}

/// The reply code [`Fault::DaxTxRefused`] answers with: an error. Which code a radio uses for a
/// refused create is not established here.
pub const REFUSED: &str = "5000002C";

/// Whether a fault withholds the statuses of `command`'s rule, so the radio acknowledges the
/// command and does nothing else: [`Fault::StuckTransmit`] for `xmit 0`, [`Fault::StuckTune`] for
/// `transmit tune 0`. The simulator asks this, and so does a test that answers for the radio on its
/// own clock, so the two cannot disagree.
pub fn withholds(faults: &[Fault], command: &str) -> bool {
    faults.iter().any(|f| {
        matches!(
            (f, command),
            (Fault::StuckTransmit, "xmit 0") | (Fault::StuckTune, "transmit tune 0")
        )
    })
}

/// The other program of [`Fault::ForeignDaxTx`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignDax {
    pub handle: u32,
    /// Its `dax_tx` stream's id.
    pub stream: u32,
}

impl Default for ForeignDax {
    fn default() -> Self {
        ForeignDax {
            handle: 0x5C0F_0002,
            stream: 0x8400_0001,
        }
    }
}

impl ForeignDax {
    /// Its client line and its transmit stream's status, after `sub client all`.
    pub(crate) fn lines(&self) -> Vec<String> {
        let (h, id) = (self.handle, self.stream);
        vec![
            format!(
                "S{h:08X}|client 0x{h:08X} connected local_ptt=0 \
                 client_id=4B7E1A20-0000-4000-8000-00000000D001 program=DAX station=Shack-PC"
            ),
            format!(
                "S{h:08X}|stream 0x{id:08X} type=dax_tx client_handle=0x{h:08X} ip=192.168.1.30"
            ),
        ]
    }
}

/// The other GUI client of [`Fault::ForeignClient`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreign {
    pub handle: u32,
    /// Its slice's index.
    pub slice: u8,
    /// Its panadapter's id, which is also the FFT stream id.
    pub pan: u32,
    /// Its waterfall's id.
    pub waterfall: u32,
    /// Its slice is the TX slice. A slice the client under test creates then reports `tx=0`.
    pub tx_slice: bool,
    /// It holds the transmitter: the interlock reports TRANSMITTING with its handle.
    pub transmitting: bool,
}

impl Default for Foreign {
    fn default() -> Self {
        Foreign {
            handle: 0x7A3C_0001,
            slice: 1,
            pan: 0x4000_0001,
            waterfall: 0x4200_0001,
            tx_slice: true,
            transmitting: false,
        }
    }
}

impl Foreign {
    /// The lines this client adds after the session's answer to `sub <topic> all`. The client
    /// line carries an invented station name and client id, which the recorder must strip.
    pub(crate) fn lines(&self, topic: &str) -> Vec<String> {
        let h = self.handle;
        let (pan, wf) = (self.pan, self.waterfall);
        match topic {
            "client" => vec![format!(
                "S{h:08X}|client 0x{h:08X} connected local_ptt=1 \
                 client_id=9D2E4F60-0000-4000-8000-00000000F001 program=SmartSDR-Win station=Shack-PC"
            )],
            "slice" => vec![format!(
                "S{h:08X}|slice {} in_use=1 RF_frequency=7.200000 mode=LSB filter_lo=-2700 \
                 filter_hi=-100 active=1 tx={} client_handle=0x{h:08X} pan=0x{pan:08X} \
                 index_letter=B rxant=ANT1 txant=ANT1",
                self.slice,
                u8::from(self.tx_slice)
            )],
            "pan" => vec![
                format!(
                    "S{h:08X}|display pan 0x{pan:08X} client_handle=0x{h:08X} waterfall=0x{wf:08X} \
                     center=7.200000 bandwidth=0.200000 x_pixels=1280 y_pixels=600 fps=25 \
                     min_dbm=-135.00 max_dbm=-40.00 rxant=ANT1 wide=0 band=40"
                ),
                format!(
                    "S{h:08X}|display waterfall 0x{wf:08X} client_handle=0x{h:08X} \
                     panadapter=0x{pan:08X} line_duration=80 auto_black=1 black_level=15 color_gain=50"
                ),
            ],
            "tx" if self.transmitting => vec![interlock_line(h, "TRANSMITTING", "SW")],
            "tx" if self.tx_slice => vec![interlock_line(0, "READY", "")],
            _ => Vec::new(),
        }
    }

    /// Whether a client command names this client's slice, pan or waterfall: a `slice` verb or
    /// `filt` with its slice index, or any token carrying its pan or waterfall id. A coarse check
    /// for test assertions, not a guard.
    pub fn is_named_by(&self, command: &str) -> bool {
        let ids = [
            format!("0x{:08x}", self.pan),
            format!("0x{:08x}", self.waterfall),
        ];
        let lower = command.to_ascii_lowercase();
        if lower
            .split(|c: char| c.is_whitespace() || c == '=' || c == ',')
            .any(|t| ids.iter().any(|id| id == t))
        {
            return true;
        }
        let words: Vec<&str> = lower.split_whitespace().collect();
        let index = self.slice.to_string();
        match words.as_slice() {
            ["slice", _verb, n, ..] => *n == index,
            ["filt", n, ..] => *n == index,
            _ => false,
        }
    }
}

/// An interlock state line in the documented shape (SmartSDR-Status-Responses):
/// `interlock tx_client_handle=<handle> state=<state> reason=<reason> source=<source>
/// tx_allowed=<0|1> amplifier=<handles>`.
pub(crate) fn interlock_line(owner: u32, state: &str, source: &str) -> String {
    format!(
        "S0|interlock tx_client_handle=0x{owner:08X} state={state} reason= source={source} \
         tx_allowed=1 amplifier="
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_foreign_object_is_recognised_in_any_command_that_names_it() {
        let f = Foreign::default();
        for named in [
            "slice set 1 mode=USB",
            "slice tune 1 7.1",
            "slice remove 1",
            "filt 1 100 2900",
            "display pan set 0x40000001 xpixels=100",
            "display panafall remove 0x42000001",
            "slice m 7.1 pan=0x40000001",
            "stream create type=dax_iq daxiq_channel=1 pan=0X40000001",
        ] {
            assert!(f.is_named_by(named), "{named:?} not recognised");
        }
        for not_named in [
            "slice set 0 mode=USB",
            "slice list",
            "slice create pan=0x40000000",
            "filt 0 100 2900",
            "display pan set 0x40000000 xpixels=100",
            "display pan set 0x400000011 xpixels=100",
            "xmit 1",
        ] {
            assert!(
                !f.is_named_by(not_named),
                "{not_named:?} wrongly recognised"
            );
        }
    }

    #[test]
    fn each_stuck_fault_withholds_its_own_stop_and_nothing_else() {
        let none: &[Fault] = &[];
        let both = [Fault::StuckTransmit, Fault::StuckTune];
        for command in ["xmit 0", "transmit tune 0"] {
            assert!(!withholds(none, command), "{command} without a fault");
            assert!(withholds(&both, command), "{command}");
        }
        assert!(withholds(&[Fault::StuckTransmit], "xmit 0"));
        assert!(!withholds(&[Fault::StuckTransmit], "transmit tune 0"));
        assert!(withholds(&[Fault::StuckTune], "transmit tune 0"));
        assert!(!withholds(&[Fault::StuckTune], "xmit 0"));
        for other in ["xmit 1", "transmit tune 1", "cwx clear", "atu start"] {
            assert!(!withholds(&both, other), "{other}");
        }
    }

    #[test]
    fn the_foreign_lines_follow_the_documented_shapes() {
        let f = Foreign {
            transmitting: true,
            ..Foreign::default()
        };
        let slice = &f.lines("slice")[0];
        assert!(slice.starts_with("S7A3C0001|slice 1 in_use=1 "));
        assert!(slice.contains(" tx=1 client_handle=0x7A3C0001 pan=0x40000001 "));
        let pans = f.lines("pan");
        assert!(pans[0].starts_with("S7A3C0001|display pan 0x40000001 client_handle=0x7A3C0001 "));
        assert!(pans[1].contains("panadapter=0x40000001"));
        assert_eq!(
            f.lines("tx"),
            vec![
                "S0|interlock tx_client_handle=0x7A3C0001 state=TRANSMITTING reason= source=SW \
                 tx_allowed=1 amplifier="
            ]
        );
        assert!(f.lines("gps").is_empty());
    }
}
