//! Test support for Nexus's TCI client: a simulated SDR program speaking TCI, and the
//! observe-only recorder testers run to capture a real server's session.
//!
//! TCI is one WebSocket from a program to an SDR program: text commands carry CAT, binary frames
//! carry receive audio, transmit audio and IQ. No operator bench speaks it, so Nexus's client is
//! tested against [`Simulator`]: a TCI server on 127.0.0.1 (an ephemeral port, read back) that
//! models a radio, speaks the dialects of the servers Nexus meets ([`Dialect`]) and injects
//! [`Fault`]s. Every connection, command, frame and decision is logged with its monotonic time in
//! time order ([`Event`], [`Simulator::wait_for`]).
//!
//! ```no_run
//! use tempo_tcisim::{Config, Dialect, Event, Fault, Simulator};
//! use std::time::Duration;
//!
//! // A server that never acknowledges `audio_start`.
//! let sim = Simulator::start(Config::dialect(Dialect::expert_sdr3()).with([Fault::NoAudioAck]))?;
//! // … point the client under test at sim.url() and ask it for a digital over …
//! let keyed = sim.wait_for(Duration::from_secs(2), |log| {
//!     log.iter().any(|l| matches!(l.event, Event::Keyed { .. }))
//! });
//! assert!(!keyed, "the guard refused a tci key the server had not acknowledged audio for");
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! # What the simulated radio does
//!
//! VFO A and B, the panorama centre and IF, mode, filter, split, RIT and XIT, drive and tune drive,
//! PTT with its source, Tune, the CW keyer (macros with PARIS timing and the spec's speed marks,
//! messages with callsign corrections, terminal mode, stop), the straight `keyer`, spots and spot
//! clicks, meters (receive level pushed or polled, transmit power from the drive and SWR), receive
//! audio and IQ in every format the spec lists, and transmit audio pulled with TX_CHRONO at the
//! rate it is consumed. The receiver hears RF carriers that move with the dial, filter and
//! panorama, plain tones, a clip a test supplies, and optionally the transmit audio clients send
//! ([`Receive`]). The SDR's own operator is [`Simulator::operator`]. Details: [`radio`].
//!
//! # The faults, and the guard each one exists to trip
//!
//! Every fault is a positive control for a guard in Nexus's TCI client:
//! the guard's test runs against the fault and must go red when the guard is
//! removed. Details on [`Fault`].
//!
//! | Fault | What the client sees | Guard |
//! |---|---|---|
//! | [`Fault::SplitConcat`] | the burst in one message; one command cut across two messages | the codec reads every command in a message and never reads a cut one as another |
//! | [`Fault::NoReady`] | a burst with no `ready;` | nothing admitted before `ready` |
//! | [`Fault::NoEcho`] | the radio keys and unkeys, and no `trx:` line ever comes | keyed clears only on the echo; escalation |
//! | [`Fault::StuckTrx`] | `trx:false` answered `trx:true`; still keyed after a reconnect | unkey readback and escalation |
//! | [`Fault::NoAudioAck`] | `audio_start` unanswered, no frames; a `tci` key takes the microphone | never a digital over on the SDR's mic |
//! | [`Fault::RxAudioStall`] | receive frames stop while the stream counts as running | receive frames within the last second; the receive floor |
//! | [`Fault::NoChrono`] | a `tci` key echoed and never pulled | unkey when no TX_CHRONO comes |
//! | [`Fault::ChronoLate`] | the first TX_CHRONO late by a set delay | the chrono window at its limit; timing measured |
//! | [`Fault::ChronoBurst`] | TX_CHRONO in groups at the same average rate | each chrono answered with exactly its size |
//! | [`Fault::ChronoStall`] | TX_CHRONO stops mid-over, the radio stays keyed | never a silent over |
//! | [`Fault::LengthAsFrames`] | `length` counted in frames; chronos ask for frames | accept the reading the payload fits |
//! | [`Fault::FormatCode4`] | receive and IQ frames say format 4 | an unknown format rejected and counted |
//! | [`Fault::ForeignKey`] | another program keys; optionally its chronos reach us | audio only for our key |
//! | [`Fault::ServerStop`] | `stop;` while keyed, no unkey echo, keys refused | halt the over and say why |
//! | [`Fault::TxEnableFalse`] | `tx_enable:<n>,false;`, every key refused | admission refuses while `tx_enable` is false |
//! | [`Fault::BandRestore`] | a band change followed by the band's drive (100) | drive readback at or below the ceiling |
//! | [`Fault::Hold200ms`] | another client's write ignored inside the hold | writes confirmed by readback |
//! | [`Fault::HalfOpen`] | silence, no pongs, the socket open | staleness check; alarm while keyed |
//! | [`Fault::DisconnectMidOver`] | the TCP connection dropped while keyed, optionally still keyed after | keyed belief kept, alarm, `trx:false` first after `ready` |
//! | [`Fault::Oversized`] | one huge text or binary message | bounded outside data |
//! | [`Fault::Garbage`] | malformed commands and frames | ignore, count, never panic |
//! | [`Fault::ReceiveOnly`] | `receive_only:true;`, every key refused | admission refuses while `receive_only` is true |
//! | [`Fault::HoldsCw`] | still keyed after the CW and through `cw_macros_stop` | Stop TX mid-word, then `trx:false` and escalation |
//! | [`Fault::UnknownVersion`] | a protocol line naming an unknown program and version | behave by what the server does, never by its name |
//! | [`Fault::Slow`] | every text message late by a set delay | bounded readback waits; a late echo is not a new answer |
//!
//! Besides the faults, behaviour the spec itself states is modelled as it is, because it is a
//! hazard a guard exists for: a `trx:<n>,true,tci` while the client's audio stream is not running
//! takes the SDR's microphone (spec §4.2), and the log says so ([`Event::MicFallback`]).
//!
//! # The dialects
//!
//! [`Dialect::expert_sdr3`] (the TCI 2.0 spec), [`Dialect::thetis`] (2.10.3.14+, ExpertSDR3
//! emulation), [`Dialect::pihpsdr`], [`Dialect::expert_sdr2`] (TCI 1.x as WSJT-X treats it) and
//! [`Dialect::aether_sdr`]: identity, burst order and grouping, PTT source and ownership, echo and
//! TX_CHRONO timing, drive and meter forms, what happens to a lost client's key. Each field says
//! what it is based on; the open questions are marked UNVERIFIED where a profile had to
//! choose. Details: [`dialect`].
//!
//! # The recorder
//!
//! [`record::record`] (the `tcirecord` binary) connects to a real server as one more client and can
//! send only the commands in [`record::ALLOWED`]: its own meter pushes, the receive streams it is
//! asked for, and reads with no argument. Nothing that keys, tunes or changes the radio can be
//! expressed by its command type, its single write path checks the list again, it has no way to
//! send a binary message, and a test pins the list. Addresses and serial numbers are replaced
//! ([`scrub`]) before anything is written, and the file stays with the tester. A recording's init
//! burst replays in the simulator ([`session::Recording::dialect`]).
//!
//! # Provenance
//!
//! Written for Nexus from Expert Electronics' TCI protocol specification, version 2.0 of 12
//! January 2024 (`TCI Protocol.pdf` in github.com/ExpertSDR3/TCI at `b081213ff9`, SHA-256
//! `2abd8d8df553403b28b8c7f0d910c8c777147821737d24cefb28ef08e67d8f09`, MIT licence). The server
//! side here is this crate's own reading of that spec, independent of Nexus's client codec, so a
//! misreading on one side is not mirrored on the other. The dialect profiles record facts about
//! what other servers send, read from piHPSDR (`src/tci.c` at `3b63530267`), AetherSDR (its TCI
//! notes and init burst at `05c5b805`) and WSJT-X 3.0.2's client, as protocol facts only: no code
//! is ported or copied from any of them, nor from Thetis, which was not read. "ExpertSDR",
//! "SunSDR", "Thetis", "piHPSDR" and "AetherSDR" name the programs the profiles stand in for.

pub mod client;
pub mod cw;
pub mod dialect;
pub mod event;
pub mod fault;
pub mod frame;
pub mod radio;
pub mod record;
pub mod scrub;
pub mod server;
pub mod session;
pub mod synth;
pub mod wire;

pub use dialect::Dialect;
pub use event::{Cause, Closer, Event, Keyer, Logged, Mic, Refusal, Source, Why};
pub use fault::{Cut, Fault};
pub use radio::{Meters, Snapshot};
pub use server::{Config, Simulator};
pub use synth::{Carrier, Clip, Receive, Tone};
