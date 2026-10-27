//! The faults the simulator injects, and the guard each one exists to trip.
//!
//! Each fault is a positive control. A guard in Nexus's TCI client is proven by a test that runs it
//! against the simulator with the matching fault and goes red when the guard is removed: a check
//! that has never been seen to fire is not yet a check. The guards are not in this crate.
//! Each variant names the one it is for.
//!
//! Faults combine: each acts on its own part of the session. Where one says "once", it fires for
//! the first matching event in the simulator's life; a reconnect then meets the radio it left.

use std::time::Duration;

use crate::frame::{kind, Header, HEADER_LEN};

/// A fault, set in [`crate::server::Config::faults`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// **Commands split and concatenated.** Everything the server has for a connection at one
    /// moment goes out as ONE text message (the init burst included, so a burst arrives as one
    /// message on a dialect that sends it command by command). With `cut`, the first command that
    /// starts with `cut.starting` is cut `cut.at` bytes in: the message ends there, and the rest of
    /// the command opens the next message, which follows at once.
    ///
    /// *Guard:* the client's codec. Several commands per message are all read (Thetis fixed its
    /// parser for exactly that), and a command cut across messages is never read as a
    /// different, complete command.
    SplitConcat { cut: Option<Cut> },

    /// **No `ready;`.** The init burst ends without it.
    ///
    /// *Guard:* nothing is latched and nothing is admitted before `ready`;
    /// the connect times out and says so.
    NoReady,

    /// **`trx` never echoes.** The radio keys and unkeys as asked (TX_CHRONO included), but no
    /// `trx:` line is sent after the init burst: no echo, no answer to a read.
    ///
    /// *Guard:* the PTT readback (keyed clears only on the server's `trx:<n>,false`
    /// echo). With no echo the client neither believes the key it asked for nor clears its keyed
    /// belief on an unkey; it times out, unkeys again and tells the operator.
    NoEcho,

    /// **`trx` stays true.** `trx:<n>,false;` is answered with `trx:<n>,true;` and changes nothing;
    /// a dropped or closed client does not unkey it either, so a reconnect's burst says
    /// `trx:<n>,true;`. TX_CHRONO keeps coming for a `tci` key.
    ///
    /// *Guard:* the unkey readback and its escalation (the stuck-TX incident's
    /// invariants): keyed is never cleared on the client's own say-so, and an unkey not confirmed
    /// in time is sent again, the link is dropped and the operator is told the radio may still be
    /// transmitting.
    StuckTrx,

    /// **`audio_start` unacknowledged.** Every `audio_start:<trx>;` is ignored: no answer, no
    /// frames, and the stream is not running, so a `tci` key on a dialect with `mic_fallback`
    /// takes the microphone (spec §4.2).
    ///
    /// *Guard:* never a digital over on the SDR's microphone: a `tci` key only
    /// while the server has acknowledged `audio_start`. With the guard removed the key goes out and
    /// the log shows [`crate::Event::MicFallback`].
    NoAudioAck,

    /// **Receive audio stops.** Each receive audio stream stops sending frames `after` it starts,
    /// while the server still counts it as running (it was acknowledged, and a `tci` key still
    /// takes TCI audio).
    ///
    /// *Guard:* the audio rule's second term: a `tci` key only while receive frames arrived within
    /// the last second. Also the receive floor that falls back to the sound card and says why.
    RxAudioStall { after: Duration },

    /// **No TX_CHRONO.** A key that takes TCI audio is accepted and echoed, and the server never
    /// asks for audio.
    ///
    /// *Guard:* after the key, no TX_CHRONO within the window: unkey and say the server is not
    /// taking Nexus's audio.
    NoChrono,

    /// **A late first TX_CHRONO.** Each key's first TX_CHRONO comes `delay` after the echo (on
    /// top of the dialect's `first_chrono`); the rest follow at the cadence from there.
    ///
    /// *Guard:* the chrono window must not fire on a merely slow server below its limit, and
    /// must above it. The over's timing is measured, never assumed.
    ChronoLate { delay: Duration },

    /// **TX_CHRONO in bursts.** Chronos come `burst` at a time, every `burst` periods: the same
    /// rate on average, as a server whose timer is coarser than its period sends them (AetherSDR
    /// polls every 5 ms for a 21 ms period).
    ///
    /// *Guard:* each chrono is answered with exactly the samples it asks for, in order, never
    /// more, and the client's queue neither runs dry nor grows from the bursts.
    ChronoBurst { burst: u32 },

    /// **TX_CHRONO stops mid-over.** Each key's chronos stop `after` its first one. The radio stays
    /// keyed and silent.
    ///
    /// *Guard:* never a silent over: a route that stops taking audio ends the over
    /// the way the Flex route does, and says why.
    ChronoStall { after: Duration },

    /// **`length` as frames.** Every frame the server sends counts `length` in frames (per
    /// channel) instead of sample values, and its TX_CHRONO then asks for `length` frames.
    ///
    /// *Guard:* the frame codec accepts the reading the payload fits and rejects, counts and
    /// survives a frame that fits neither.
    LengthAsFrames,

    /// **Format code 4.** Every receive and IQ frame says `format: 4` for its float32 samples, as
    /// the Go client writes its transmit frames.
    ///
    /// *Guard:* an unknown format is never read as a known one: the frame is rejected and counted,
    /// and the receive floor notices the audio has stopped.
    FormatCode4,

    /// **Another program keys.** `after` the first connection's `ready;`, the SDR keys for another
    /// program (or its own operator): every client sees `trx:<trx>,true;`, and after `hold` it
    /// unkeys with `trx:<trx>,false;`. With `chrono_to_all`, TX_CHRONO goes meanwhile to every
    /// client whose audio stream runs, as a server that broadcasts its chronos would. Once.
    ///
    /// *Guard:* audio only for our key: a chrono during another program's key is
    /// answered with nothing, so the log shows no [`crate::Event::FrameIn`] from the client during
    /// it. Also: `owner_transmitting` tells another program's key from ours.
    ForeignKey {
        after: Duration,
        hold: Duration,
        chrono_to_all: bool,
    },

    /// **The SDR stops while keyed.** `after` the first key the radio accepts, the server
    /// broadcasts `stop;` and stops: no frames of any kind, the transmitter released with no
    /// `trx:false` echo, every key refused until a `start;`. Once.
    ///
    /// *Guard:* `stop;` while keyed halts the over and shows the reason; the client does not
    /// wait for an echo that will not come.
    ServerStop { after: Duration },

    /// **Transmit not allowed.** The burst says `tx_enable:<trx>,false;` for every transceiver,
    /// and every key (`trx`, `tune`, CW, `keyer`) is refused: no echo, nothing keyed, the refusal
    /// logged.
    ///
    /// *Guard:* admission refuses a key while `tx_enable` is false.
    TxEnableFalse,

    /// **A band change restores the band's settings.** Each `vfo` write that moves a transceiver
    /// to another band is echoed, then followed by the band's restored settings, broadcast:
    /// `drive`, `tune_drive` and, with `modulation`, the mode (spec §3.5).
    ///
    /// *Guard:* admission compares the server's `drive` (and `tune_drive` for Tune) with
    /// Nexus's ceiling by readback, so after a restore to 100 the key is refused until the ceiling
    /// is asserted again and read back.
    BandRestore {
        drive: u32,
        tune_drive: u32,
        modulation: Option<String>,
    },

    /// **The 200 ms hold** (spec §3.5, `window` normally 200 ms). A parameter changed by the SDR's
    /// operator ([`crate::Simulator::operator`]) or by one client is held for `window`: a write
    /// to it from any other client meanwhile is ignored (no echo) and logged as
    /// [`crate::Event::Held`]. The holder may go on changing it.
    ///
    /// *Guard:* a write that is not read back is retried by the radio loop as for any rig, never
    /// assumed to have taken.
    Hold200ms { window: Duration },

    /// **A half-open socket.** `after` the first connection's `ready;`, the server goes silent on
    /// it: no messages, no frames, no pongs, nothing read is answered, and the TCP connection stays
    /// open. Once.
    ///
    /// *Guard:* the session's staleness check (ping and receive timeouts) ends a silent session,
    /// and while keyed raises the alarm instead of trusting the silence.
    HalfOpen { after: Duration },

    /// **A disconnect mid-over.** `after` the radio first keys for a client (its key echoed), that
    /// client's TCP connection is dropped with no close frame. With `radio_stays_keyed` the radio
    /// stays keyed whatever the dialect does on a lost client (the worse answer), and the
    /// next connection's burst says `trx:<n>,true;`. Once.
    ///
    /// *Guard:* a lost link never clears the keyed belief: the client halts TX, raises "may
    /// still be transmitting", reconnects at once and sends `trx:<n>,false;` first after `ready`.
    DisconnectMidOver {
        after: Duration,
        radio_stays_keyed: bool,
    },

    /// **An oversized message.** Right after each burst, one message of `bytes` bytes: a text
    /// message holding one unknown command with a huge argument, or with `binary` a receive audio
    /// frame whose payload is that long.
    ///
    /// *Guard:* outside data is bounded: a size limit on messages and arguments,
    /// no panic, and the session goes on or ends cleanly.
    Oversized { bytes: usize, binary: bool },

    /// **Garbage.** Right after each burst, [`garbage_text`] and [`garbage_frames`]: malformed
    /// commands, unknown names, impossible values, frames too short for a header, of unknown types
    /// and formats, and frames that fit neither reading of `length`. The session then goes on as
    /// if nothing had happened.
    ///
    /// *Guard:* an invalid command is ignored (spec §3.1); a bad frame is
    /// rejected and counted; nothing panics; the next good command is still read.
    Garbage,

    /// **A receive-only device.** The burst says `receive_only:true;` and every key is refused,
    /// as [`Fault::TxEnableFalse`] refuses them.
    ///
    /// *Guard:* admission refuses a key while `receive_only` is true.
    ReceiveOnly,

    /// **The radio holds after CW.** A `cw_macros` or `cw_msg` keys the radio as usual, but it
    /// stays keyed when the text is done and through `cw_macros_stop;`, until `trx:<n>,false;`.
    ///
    /// *Guard:* Stop TX mid-word. If the radio still shows our CW keyed just after the stop,
    /// the client sends `trx:<n>,false;`, and if that is not echoed it escalates and tells the
    /// operator.
    HoldsCw,

    /// **An unknown version.** The burst's protocol line says `protocol:<program>,<version>;`
    /// with these values instead of the dialect's.
    ///
    /// *Guard:* behave by what the server does, never by its name: an unknown program
    /// or version is neither refused outright nor assumed to be one the client knows; features are
    /// learned from what the server sends.
    UnknownVersion { program: String, version: String },

    /// **A slow server.** Every text message goes out `delay` late, the burst and every echo
    /// included. Frames keep their cadence.
    ///
    /// *Guard:* the client's waits for a readback are long enough for a slow server and bounded:
    /// past them it says the server did not answer, and it never treats a late echo of an old
    /// write as the answer to a new one.
    Slow { delay: Duration },
}

/// Where [`Fault::SplitConcat`] cuts a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    /// The first command whose text starts with this (lower case) is cut.
    pub starting: String,
    /// Bytes of it that stay in the first message.
    pub at: usize,
}

/// The fault's name, for the log.
impl Fault {
    pub fn name(&self) -> &'static str {
        match self {
            Fault::SplitConcat { .. } => "SplitConcat",
            Fault::NoReady => "NoReady",
            Fault::NoEcho => "NoEcho",
            Fault::StuckTrx => "StuckTrx",
            Fault::NoAudioAck => "NoAudioAck",
            Fault::RxAudioStall { .. } => "RxAudioStall",
            Fault::NoChrono => "NoChrono",
            Fault::ChronoLate { .. } => "ChronoLate",
            Fault::ChronoBurst { .. } => "ChronoBurst",
            Fault::ChronoStall { .. } => "ChronoStall",
            Fault::LengthAsFrames => "LengthAsFrames",
            Fault::FormatCode4 => "FormatCode4",
            Fault::ForeignKey { .. } => "ForeignKey",
            Fault::ServerStop { .. } => "ServerStop",
            Fault::TxEnableFalse => "TxEnableFalse",
            Fault::BandRestore { .. } => "BandRestore",
            Fault::Hold200ms { .. } => "Hold200ms",
            Fault::HalfOpen { .. } => "HalfOpen",
            Fault::DisconnectMidOver { .. } => "DisconnectMidOver",
            Fault::Oversized { .. } => "Oversized",
            Fault::Garbage => "Garbage",
            Fault::ReceiveOnly => "ReceiveOnly",
            Fault::HoldsCw => "HoldsCw",
            Fault::UnknownVersion { .. } => "UnknownVersion",
            Fault::Slow { .. } => "Slow",
        }
    }
}

/// The text messages [`Fault::Garbage`] sends, in order. None of them is a well-formed command
/// the spec defines with valid values: each is malformed, unknown, or out of range, so a client
/// must ignore every one.
pub fn garbage_text() -> Vec<String> {
    vec![
        String::new(),
        ";".into(),
        ":;".into(),
        ":::,,,;;;".into(),
        "vfo".into(),
        "vfo:0,0,abc;".into(),
        "vfo:0,0,99999999999999999999999999;".into(),
        "vfo:-1,0,7074000;".into(),
        "vfo:99,0,7074000;".into(),
        "trx:0,maybe;".into(),
        "trx:0,perhaps,tci,extra,args,here;".into(),
        "modulation:0,;".into(),
        "drive:0,250;".into(),
        "drive:0,-5;".into(),
        "\u{1}\u{2}\u{7f};".into(),
        "ünïcödé:1,2;".into(),
        "x_unknown_command:1,2,3;".into(),
        format!("{};", "z".repeat(1000)),
        "rx_channel_sensors:0,0,not-a-number;".into(),
        "tx_sensors:0,,,,;".into(),
        "protocol;".into(),
    ]
}

/// The binary frames [`Fault::Garbage`] sends, in order: none is a valid receive or IQ frame.
pub fn garbage_frames() -> Vec<Vec<u8>> {
    let audio = |f: &dyn Fn(&mut Header)| {
        let mut h = Header {
            receiver: 0,
            sample_rate: 48_000,
            format: 3,
            length: 256,
            kind: kind::RX_AUDIO,
            channels: 2,
            ..Header::default()
        };
        f(&mut h);
        h
    };
    vec![
        Vec::new(),
        vec![0xFF; 10],
        vec![0; HEADER_LEN - 1],
        // A header with an unknown stream type.
        audio(&|h| h.kind = 99).frame(&[0; 1024]),
        // A known type in an unknown format.
        audio(&|h| h.format = 7).frame(&[0; 1024]),
        // A length that no payload this size could fit.
        audio(&|h| h.length = u32::MAX).frame(&[0; 16]),
        // 256 float32 values would be 1024 bytes as values or 2048 as frames: 300 values fit neither.
        audio(&|h| h.length = 256).frame(&[0; 1200]),
        // Not a whole number of float32 samples.
        audio(&|h| h.length = 1).frame(&[0; 7]),
        // A transceiver that does not exist.
        audio(&|h| h.receiver = 7).frame(&[0; 2048]),
        // No channels, no rate.
        audio(&|h| {
            h.channels = 0;
            h.sample_rate = 0;
        })
        .frame(&[0; 1024]),
        // An IQ frame with an odd channel count.
        audio(&|h| {
            h.kind = kind::IQ;
            h.channels = 3;
        })
        .frame(&[0; 1024]),
    ]
}

/// Whether a fault in `faults` matches `pick`.
pub(crate) fn has(faults: &[Fault], pick: impl Fn(&Fault) -> bool) -> bool {
    faults.iter().any(pick)
}

/// The first value `pick` finds in `faults`.
pub(crate) fn find<T>(faults: &[Fault], pick: impl Fn(&Fault) -> Option<T>) -> Option<T> {
    faults.iter().find_map(pick)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{fit, Fit, SampleType};
    use crate::wire;

    #[test]
    fn no_garbage_line_is_a_well_formed_spec_command_with_valid_values() {
        // Every line either fails to parse or names something the spec does not define, or gives a
        // value the spec forbids. Checked by hand per line; the counts pin the corpus.
        let text = garbage_text();
        assert_eq!(text.len(), 21);
        let mut commands = 0;
        for line in &text {
            commands += wire::parse(line).commands.len();
        }
        // The lines that do parse as commands (unknown names or invalid values): pinned, so a line
        // added to the corpus is looked at here.
        assert_eq!(commands, 14, "{text:?}");
        for line in &text {
            for c in wire::parse(line).commands {
                assert_ne!(c.name, "ready", "{line}");
                if c.name == "trx" {
                    assert!(
                        c.args.get(1).and_then(|a| wire::boolean(a)).is_none(),
                        "{line}"
                    );
                }
            }
        }
    }

    #[test]
    fn no_garbage_frame_is_a_valid_stream_frame() {
        for (i, bytes) in garbage_frames().iter().enumerate() {
            let Some(h) = Header::decode(bytes) else {
                continue; // too short to be a frame at all
            };
            let payload = &bytes[HEADER_LEN..];
            let valid = matches!(h.kind, kind::RX_AUDIO | kind::IQ)
                && matches!(h.channels, 1 | 2)
                && h.sample_rate > 0
                && h.receiver < 2
                && SampleType::from_code(h.format).is_some_and(|t| {
                    payload.len().is_multiple_of(t.bytes())
                        && fit(h.length, h.channels, payload.len() / t.bytes()) != Fit::Neither
                });
            assert!(!valid, "garbage frame {i} is valid: {h:?}");
        }
    }

    #[test]
    fn every_fault_has_its_own_name() {
        let all = [
            Fault::SplitConcat { cut: None },
            Fault::NoReady,
            Fault::NoEcho,
            Fault::StuckTrx,
            Fault::NoAudioAck,
            Fault::RxAudioStall {
                after: Duration::ZERO,
            },
            Fault::NoChrono,
            Fault::ChronoLate {
                delay: Duration::ZERO,
            },
            Fault::ChronoBurst { burst: 2 },
            Fault::ChronoStall {
                after: Duration::ZERO,
            },
            Fault::LengthAsFrames,
            Fault::FormatCode4,
            Fault::ForeignKey {
                after: Duration::ZERO,
                hold: Duration::ZERO,
                chrono_to_all: false,
            },
            Fault::ServerStop {
                after: Duration::ZERO,
            },
            Fault::TxEnableFalse,
            Fault::BandRestore {
                drive: 100,
                tune_drive: 100,
                modulation: None,
            },
            Fault::Hold200ms {
                window: Duration::ZERO,
            },
            Fault::HalfOpen {
                after: Duration::ZERO,
            },
            Fault::DisconnectMidOver {
                after: Duration::ZERO,
                radio_stays_keyed: false,
            },
            Fault::Oversized {
                bytes: 1,
                binary: false,
            },
            Fault::Garbage,
            Fault::ReceiveOnly,
            Fault::HoldsCw,
            Fault::UnknownVersion {
                program: String::new(),
                version: String::new(),
            },
            Fault::Slow {
                delay: Duration::ZERO,
            },
        ];
        let names: std::collections::BTreeSet<&str> = all.iter().map(Fault::name).collect();
        assert_eq!(names.len(), all.len());
    }
}
