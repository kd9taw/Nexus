//! The native CI-V daemon — **what listens on the radio's rigctld TCP port** when
//! `icom_native_cat` is on.
//!
//! Nexus's entire CAT stack (`Rig`, `probe_cat`, the dual-radio monitors, the handoff)
//! talks the rigctld TEXT protocol to `127.0.0.1:<port>` and never cares what serves it.
//! [`CivDaemon`] binds that port and answers with [`CivBackend`] — every verb translated
//! to CI-V over the serial engine that owns the COM port. The prize over real rigctld:
//! the same serial stream carries the radio's **scope waveform** (a real RF panadapter)
//! and transceive pushes, which rigctld discards.
//!
//! Everything here is I/O-generic and unit-tested against the in-memory fake radio; only
//! [`CivDaemon::start`] (opening the real COM port) needs the `serial` feature.
//!
//! ## Which receiver a command is for
//!
//! On a radio with two receivers — of the models this daemon drives, the capability table in
//! [`crate::dualrx`] offers a Sub for the IC-7610 and the IC-9700 — a plain CI-V command acts
//! on whichever band is SELECTED, and the selection is front-panel state the operator can
//! move. So every verb that belongs to ONE receiver names it (Main, unless a caller names
//! Sub), and the naming has to survive the whole command. There are three ways it does:
//!
//! - **By name, on the wire (IC-7610).** Icom's band-directed command `29` performs a command
//!   on the named band "regardless of active/inactive" band (IC-7610 CI-V Reference Guide
//!   A7380-7EX-4, p. 9), for every command its table marks
//!   ([`commands::band_directed_supported`]). Nothing is selected, so nothing on the front
//!   panel flickers, and no lock is needed: the command is atomic on the wire. The dial and
//!   the mode, which `29` does not carry, are read AND written on Main the same way, through
//!   `25 00` / `26 00` ([`commands::dial_by_band_name`]). The split verbs keep their own bytes.
//! - **By holding the selection (the IC-9700, and the IC-7610's unmarked commands).** The
//!   IC-9700's reference (A7508-3EX-4) has no command `29`. The broker keeps the selection on
//!   Main and holds it there under the band lock for the length of the command, repairing a
//!   selection a failed restore stranded on Sub first ([`CivBackend::ensure_main`]); a
//!   Sub-named command selects Sub and ALWAYS hands the selection back — the
//!   `set_split_freq` pattern. ⚠️ What it cannot see is the operator selecting Sub at the
//!   front panel — see [`RxAddressing::HeldSelection`] for why it does not look.
//! - **Not at all (every other radio).** One receiver, or no vendor statement of a second:
//!   every command goes out exactly as it did before this existed, byte for byte.
//!
//! ⛔ THE KEYING PATH NAMES NO RECEIVER AND TAKES NO LOCK — PTT (`T`/`t`), CAT CW (`b`) and
//! its stop. The RIG decides which receiver transmits (Main; the uplink, Sub, in satellite
//! mode — IC-9700 Basic Manual pp. 3-2, 7-1), a selection round trip between the go and PTT-on
//! would move key-on timing, and an unkey must never wait behind a selection sequence. The
//! transmit-side CONFIGURATION verbs (XIT, repeater shift and offset, CTCSS) do name the
//! transmitting receiver, Main by default, and the radio loop withholds them from a keyed rig.

use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use super::commands::{self, IcomModel, Mode};
use super::engine::{CivEngine, CivError, CivHandle, Expect};
use super::frame::Frame;
use super::scope::ScopeSweep;
use crate::dualrx::ReceiverId;
use crate::rigctld_server::{serve_connection, RigBackend};

/// How THIS radio lets a command name the receiver it is for — decided once, from the model,
/// never from the bus. See the module note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RxAddressing {
    /// No Sub is offered for this radio ([`crate::dualrx::sub_receiver_offered`] is false:
    /// one receiver, or one nobody has read a vendor statement of a second for — UNKNOWN,
    /// which is not "no", and is not an offer either). Every command goes out exactly as it
    /// always has: no lock, no select, no prefix. A test holds the bytes.
    Single,
    /// Two receivers, and Icom's reference gives this model the band-directed form (`29`) —
    /// the IC-7610. A command the table marks names its receiver on the wire; an unmarked
    /// one falls back to [`Self::HeldSelection`]'s discipline.
    BandDirected(IcomModel),
    /// Two receivers and no band-directed form — the IC-9700. The selection is HELD: the band
    /// lock for the whole command, Main re-asserted where a failed restore stranded it
    /// ([`CivBackend::ensure_main`]), Sub selected and handed back for a Sub-named command.
    ///
    /// ⚠️ WHAT IT CANNOT SEE: the operator selecting Sub at the front panel ("To select the
    /// Main band or Sub band, touch the grayed frequency readout", IC-9700 Basic Manual p. 3-2).
    /// The broker holds the selection IT set and does not re-read it — neither did the dial
    /// verbs before this, so the whole daemon stays consistent: with Sub selected by hand,
    /// every verb follows the panel, exactly as before. Closing it needs a `07 D2` read ahead
    /// of every command and, when the answer is Sub, a select of Main and a select back —
    /// which at the radio loop's poll rates (the dial every 180 ms, the S-meter every 360 ms)
    /// flips the operator's selection over and back several times a second while they are
    /// using it, and lands MAIN DIAL turns on the wrong band in the gaps. That trade is the
    /// operator's to make, not this code's.
    HeldSelection,
}

impl RxAddressing {
    fn for_model(model: Option<IcomModel>) -> Self {
        match model {
            Some(m) if crate::dualrx::sub_receiver_offered(m.hamlib_model()) => {
                if commands::band_directed_form(m) {
                    RxAddressing::BandDirected(m)
                } else {
                    RxAddressing::HeldSelection
                }
            }
            _ => RxAddressing::Single,
        }
    }
}

/// Does this `l`/`L` level belong to ONE receiver? The receive chain does — the S-meter and
/// the front end, DSP and audio stages ([`crate::dualrx::RxStage`]); the rest (power, mic
/// gain, keyer speed, compressor depth, monitor gain, the transmit meters) belongs to the
/// radio's one transmitter and names no receiver.
fn level_names_a_receiver(name: &str) -> bool {
    matches!(
        name,
        "STRENGTH" | "ATT" | "PREAMP" | "RF" | "AGC" | "NR" | "NB" | "AF" | "SQL"
    )
}

/// Does this `u`/`U` function belong to ONE receiver? NB, NR and the two notches are the DSP
/// stage; RIT is a receive offset and ΔTX (`XIT`) the transmitting receiver's offset, both in
/// a per-band register. Compressor, monitor, VOX and satellite mode are the radio's.
fn func_names_a_receiver(token: &str) -> bool {
    matches!(token, "NB" | "NR" | "ANF" | "MN" | "RIT" | "XIT")
}

/// Cross-band split state: on a dual-band Icom (IC-9700/910H) a cross-band
/// pair is NOT `0F` split — `0F` is same-band A/B, and `25 01` writes the
/// unselected VFO of the *current* band (the field-falsified parity-batch
/// assumption). Cross-band is the rig's SATELLITE MODE (`16 5A`): Main =
/// downlink/RX, Sub = uplink/TX, full duplex, TX always out Sub.
struct SatSplit {
    /// Split currently rides satellite mode (we engaged it via `S 1 Sub`).
    engaged: bool,
    /// `16 5A` capability: `Some(true)` = the rig answers the read (it has a
    /// Sub band), `Some(false)` = NAK (an IC-7300 refuses honestly), `None` =
    /// not probed yet. A probe TIMEOUT stays `None` — one busy moment must not
    /// become a permanent "no".
    cap: Option<bool>,
    /// The tuning selection may be stranded on the SUB band: a Main-restore
    /// write failed mid select-write-restore. While this stands, every
    /// selection-dependent verb re-asserts Main first (and refuses when even
    /// that fails) — without it the next `05` would land the downlink in the
    /// Sub band, and satellite-mode TX exits Sub, so the rig would transmit
    /// on the downlink frequency.
    sel_stray: bool,
    /// The OPERATOR had satellite mode on, and we turned it off to run a
    /// same-band A/B split. Distinct from `engaged`, which is satellite mode WE
    /// engaged as the split itself: this is a state we found and borrowed, so
    /// the release puts it back. Never set from a state we did not change.
    op_satmode_off: bool,
}

/// rigctld-protocol backend that translates every verb to CI-V through the engine.
pub struct CivBackend {
    h: CivHandle,
    addr: u8,
    /// WHICH Icom this is — needed because the preamp's wire byte is a POSITION in this
    /// rig's own list, and the attenuator may only be offered the pads this rig has
    /// ([`commands::preamp_steps_db`], [`commands::attenuator_steps_db`]).
    ///
    /// ⚠️ NOT derivable from [`Self::addr`], and that is the reason it is carried. The
    /// CI-V address is user-changeable on the radio's own menu, so two operators can run
    /// different rigs at the same address; a model inferred from the bus would eventually
    /// hand an IC-7300 the 7610's fifteen 3 dB pads. `None` = an Icom this build has no step
    /// list for, and then neither control is offered at all rather than guessed at.
    model: Option<IcomModel>,
    /// When the dial was last READ from the radio (not merely cached). Bounds how long a
    /// timed-out `f` may serve the cache — see [`cache_fresh`].
    last_freq_ok: Mutex<Option<std::time::Instant>>,
    /// Main's dial and mode as the last BY-NAME read returned them (`25 00` / `26 00`,
    /// [`Self::main_dial_by_name`]): what a failed read on that path stands in with, in place
    /// of the engine's state cache. The cache also folds the radio's transceive pushes, and a
    /// push reports the SELECTED band — serving one would put the selection back into the
    /// reading at exactly the moment the bus is busy. Never written on any other radio.
    main_hz: Mutex<Option<u64>>,
    main_mode: Mutex<Option<(Mode, Option<u8>)>>,
    /// Which Icom DATA mode to select for digital operating (1..=3, default 1 = today's
    /// behaviour). Atomic because a settings save must move it under a RUNNING daemon: the
    /// alternative is a CI-V restart to change a menu choice, which drops CAT mid-session.
    data_mode: std::sync::atomic::AtomicU8,
    /// Split state the UI/`s` verb reads back (the rig's `0F` read is skipped — the
    /// last commanded state is authoritative for the session, like the Hamlib cache).
    split: AtomicBool,
    /// True while Nexus itself intends to transmit. Shared with the owning [`CivDaemon`] so the
    /// broker's disconnect fail-safe unkey can skip while Nexus is on the air (its own Rig is a
    /// client here, and a transient reconnect must not steal the over). See `owner_transmitting`.
    tx_intent: Arc<AtomicBool>,
    /// Satellite-mode split state — and the lock is LOAD-BEARING, not
    /// decoration: the daemon serves one shared backend to a thread per TCP
    /// connection, so without it a `f` poll (Nexus's own Rig, or WSJT-X) can
    /// land between `07 D1` and `07 D0` inside a Sub-band write and read the
    /// UPLINK as the dial — which `sat_observe_operator_tune` treats as "the
    /// operator tuned away" and hands the pass back. Every selected-VFO verb
    /// (freq/mode read + write) takes it; PTT deliberately does not (an unkey
    /// must never queue behind a tuning sequence).
    ///
    /// LOCK ORDER: engine mutex → this band mutex, never the reverse. Today
    /// that holds structurally — the broker has no reference to `Engine`, so
    /// no band→engine edge can exist and the graph is acyclic. Keep it that
    /// way: handing the broker (or anything that runs under this lock)
    /// something engine-shaped would let a band-holder block on the engine
    /// mutex while an engine-holder blocks on a CAT verb queued behind this
    /// lock — the cross-thread cousin of the 0.24.3 sat-pick hang.
    band: Mutex<SatSplit>,
    /// How a per-receiver command names its receiver on this radio — see [`RxAddressing`]
    /// and [`Self::on_receiver`].
    rx_addressing: RxAddressing,
}

/// How long a timed-out dial read may still serve the last real reading.
const CIV_CACHE_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Is the cached dial still recent enough to stand in for a read that timed out?
fn cache_fresh(last_ok: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last_ok.is_some_and(|t| now.saturating_duration_since(t) <= CIV_CACHE_GRACE)
}

impl CivBackend {
    pub fn new(
        h: CivHandle,
        addr: u8,
        tx_intent: Arc<AtomicBool>,
        data_mode: u8,
        model: Option<IcomModel>,
    ) -> Self {
        CivBackend {
            h,
            addr,
            model,
            last_freq_ok: Mutex::new(None),
            main_hz: Mutex::new(None),
            main_mode: Mutex::new(None),
            data_mode: std::sync::atomic::AtomicU8::new(data_mode.clamp(1, 3)),
            split: AtomicBool::new(false),
            tx_intent,
            band: Mutex::new(SatSplit {
                engaged: false,
                cap: None,
                sel_stray: false,
                op_satmode_off: false,
            }),
            rx_addressing: RxAddressing::for_model(model),
        }
    }

    /// The band/satellite-split lock. Poison-recovering: a panicked client
    /// thread must never wedge every other connection's CAT.
    fn band(&self) -> MutexGuard<'_, SatSplit> {
        self.band
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Does this radio have ΔTX (Icom's XIT)? See [`commands::has_delta_tx`]. An unknown
    /// model keeps the behaviour it always had.
    fn has_delta_tx(&self) -> bool {
        self.model.is_none_or(commands::has_delta_tx)
    }

    /// Does this radio read and write Main's dial and mode BY NAME
    /// ([`commands::dial_by_band_name`] — the IC-7610)? Only a radio offered a Sub has a
    /// selection to escape: a [`RxAddressing::Single`] radio reads and writes with exactly the
    /// bytes it always did.
    fn main_dial_by_name(&self) -> bool {
        self.rx_addressing != RxAddressing::Single
            && self.model.is_some_and(commands::dial_by_band_name)
    }

    /// Main's dial by name (`25 00`), for [`RigBackend::freq_hz`] on a radio that
    /// [`Self::main_dial_by_name`]. The `03` path's answers exactly, with one difference: what
    /// stands in for a read that fails is Main's own last reading ([`Self::main_hz`]), never
    /// the engine's cache. Callers hold the band lock.
    fn main_freq_by_name(&self) -> u64 {
        let f = commands::read_band_freq(self.addr, commands::BAND_MAIN);
        match self.read(f, 0x25, Some(commands::BAND_MAIN)) {
            Ok(f) => {
                *self.last_freq_ok.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(std::time::Instant::now());
                let mut last = self.main_hz.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(hz) = commands::parse_band_freq(&f, commands::BAND_MAIN) {
                    *last = Some(hz);
                }
                last.unwrap_or(0)
            }
            // One crowded moment, bounded by the same grace as the `03` path's cache.
            Err(CivError::Timeout) => {
                let t = *self.last_freq_ok.lock().unwrap_or_else(|e| e.into_inner());
                if cache_fresh(t, std::time::Instant::now()) {
                    self.main_hz
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .unwrap_or(0)
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }

    /// Main's mode by name (`26 00`) — `None` when the read fails, and then [`RigBackend::mode`]
    /// serves Main's last reading ([`Self::main_mode`]) rather than the engine's cache.
    /// Callers hold the band lock.
    fn main_mode_by_name(&self) -> Option<(Mode, Option<u8>)> {
        let f = commands::read_band_mode(self.addr, commands::BAND_MAIN);
        let m = self
            .read(f, 0x26, Some(commands::BAND_MAIN))
            .ok()
            .and_then(|f| commands::parse_band_mode(&f, commands::BAND_MAIN))?;
        *self.main_mode.lock().unwrap_or_else(|e| e.into_inner()) = Some(m);
        Some(m)
    }

    /// Select a band/VFO by token (`Main`/`Sub`), acked. Callers hold the band lock.
    fn select(&self, vfo: &str) -> bool {
        commands::select_vfo(self.addr, vfo).is_some_and(|f| self.ack(f))
    }

    /// Re-assert the Main selection when a failed restore may have stranded
    /// it on Sub ([`SatSplit::sel_stray`]). True = the selection is known to
    /// be Main. On success the dial is re-read so the engine's state cache
    /// drops any uplink a stray-period read folded in. Callers hold the band
    /// lock.
    fn ensure_main(&self, g: &mut SatSplit) -> bool {
        if !g.sel_stray {
            return true;
        }
        if !self.select("Main") {
            return false;
        }
        g.sel_stray = false;
        let _ = self.read(commands::read_freq(self.addr), 0x03, None);
        true
    }

    /// Release a split that rides satellite mode (`16 5A 00`). On failure the
    /// session state STANDS — the rig is still in satellite mode, and clearing
    /// `engaged` anyway would let the next teardown fire `0F 00` at it (or
    /// route an A/B split's TX dial into the Sub band).
    fn release_sat_split(&self, g: &mut SatSplit) -> bool {
        let ok = self.ack(commands::set_dsp_func(
            self.addr,
            commands::FUNC_SATMODE,
            false,
        ));
        if ok {
            g.engaged = false;
            self.split.store(false, Ordering::Relaxed);
        }
        ok
    }

    /// Take the rig OUT of a satellite mode the OPERATOR put it in, so a
    /// same-band A/B split can work at all. `false` = the split must not go
    /// ahead. Callers hold the band lock.
    ///
    /// ⚠️ NEEDS-BENCH (IC-9700 — field report 2026-08-16, V/V pass).
    ///
    /// [`SatSplit::engaged`] covers only the satellite mode WE engaged as a
    /// cross-band split. This is the other half, and the operator's normal
    /// habit produces it: he works passes with SAT on at the front panel. On
    /// this family satellite mode is CROSS-BAND BY CONSTRUCTION — Main and Sub
    /// are different bands — and it fixes TX on Sub. A same-band pass (V/V,
    /// U/U) cannot be expressed that way at all, so it rides `0F` A/B split;
    /// and `0F 01` fired at a rig still in satellite mode asks for a TX VFO
    /// that is not where the rig would transmit.
    ///
    /// Only a rig we KNOW is in satellite mode is touched: a NAK (no Sub band —
    /// the IC-7300 family) and a busy/timed-out read both leave everything
    /// alone, so no terrestrial pile-up split pays for this. A refusal to leave
    /// it, though, refuses the split: firing `0F 01` anyway would transmit on a
    /// band the operator never chose, silently.
    fn clear_operator_satmode(&self, g: &mut SatSplit) -> bool {
        if g.op_satmode_off {
            return true; // already borrowed — re-probing per correction is bus noise
        }
        if !matches!(self.read_satmode(), Ok(Some(true))) {
            return true; // not in it, has no such mode, or the link is busy: nothing to do
        }
        if !self.ack(commands::set_dsp_func(
            self.addr,
            commands::FUNC_SATMODE,
            false,
        )) {
            super::diag::note(
                "same-band split: the rig is in SATELLITE mode and would not leave it — \
                 nothing sent; turn SATELLITE off at the front panel",
            );
            return false;
        }
        g.op_satmode_off = true;
        super::diag::note(
            "same-band split: the rig was in SATELLITE mode, which is cross-band only — \
             turned it off for this pass; it goes back on when the split is released",
        );
        true
    }

    /// Put back the satellite mode [`Self::clear_operator_satmode`] borrowed.
    /// Callers hold the band lock. ⚠️ NEEDS-BENCH with the same report.
    ///
    /// The flag is spent either way. Held through a REFUSED restore it would
    /// make the next split skip its probe on a rig that may well still be in
    /// satellite mode; cleared, the next split re-probes and re-decides from
    /// what the rig actually says. The operator is told, because a front-panel
    /// state we changed and could not change back is theirs to finish.
    fn restore_operator_satmode(&self, g: &mut SatSplit) {
        if !g.op_satmode_off {
            return;
        }
        g.op_satmode_off = false;
        if self.ack(commands::set_dsp_func(
            self.addr,
            commands::FUNC_SATMODE,
            true,
        )) {
            super::diag::note(
                "split released — SATELLITE mode put back on, as the operator had it",
            );
        } else {
            super::diag::note(
                "split released — the rig would not go back into SATELLITE mode; \
                 turn SATELLITE back on at the front panel",
            );
        }
    }

    /// Read `16 5A` (satellite mode) back from the rig.
    fn read_satmode(&self) -> Result<Option<bool>, CivError> {
        self.read(
            commands::read_dsp_func(self.addr, commands::FUNC_SATMODE),
            0x16,
            Some(commands::FUNC_SATMODE),
        )
        .map(|f| commands::parse_dsp_func(&f, commands::FUNC_SATMODE))
    }

    /// Engage satellite mode as the cross-band split (`S 1 Sub`). Returns true
    /// only when the rig's own `16 5A` read-back confirms it.
    fn engage_sat_split(&self, g: &mut SatSplit) -> bool {
        // Capability by READING, not by model string: an IC-910H answers, an
        // IC-7300 NAKs honestly. Cached; a timeout retries next attempt.
        let cap = match g.cap {
            Some(c) => c,
            None => match self.read_satmode() {
                Ok(v) => {
                    let c = v.is_some();
                    g.cap = Some(c);
                    c
                }
                Err(CivError::Nak) => {
                    g.cap = Some(false);
                    false
                }
                Err(_) => false, // busy/dead: fail THIS attempt, cache nothing
            },
        };
        if !cap {
            return false;
        }
        if g.engaged {
            // Re-asserted every correction by the split one-shot — but the
            // operator switching satellite mode OFF on the front panel is them
            // taking the rig back. Never fight the knob: report it honestly
            // instead of re-engaging over their choice.
            return match self.read_satmode() {
                Ok(Some(false)) => {
                    g.engaged = false;
                    self.split.store(false, Ordering::Relaxed);
                    false
                }
                // On (or a busy/unparseable moment): the split stands.
                _ => true,
            };
        }
        // Engaging satellite mode swaps BOTH bands to the rig's stored
        // satellite VFOs — the downlink the retune just wrote would be
        // clobbered. Read the dial first, engage + verify, then put the
        // downlink back on Main.
        if !self.ensure_main(g) {
            return false; // a stray selection would read SUB as "the dial"
        }
        let dial = self
            .read(commands::read_freq(self.addr), 0x03, None)
            .ok()
            .and_then(|f| commands::parse_freq(&f));
        let ok = self.ack(commands::set_dsp_func(
            self.addr,
            commands::FUNC_SATMODE,
            true,
        )) && self.read_satmode() == Ok(Some(true));
        if !ok {
            // The set may still have LANDED (an acked set whose verify reply
            // was lost, or a timed-out set the rig acted on): without an undo
            // the rig would sit in satellite mode on its stored satellite
            // VFOs while the session believes nothing happened — and the
            // eventual teardown would fire `0F 00`, stranding it there. Back
            // the set out and put the dial back, best-effort, then fail
            // honestly.
            let _ = self.ack(commands::set_dsp_func(
                self.addr,
                commands::FUNC_SATMODE,
                false,
            ));
            if let Some(hz) = dial {
                let _ = self.ack(commands::set_freq(self.addr, hz));
            }
            return false;
        }
        if let Some(hz) = dial {
            // Pin the selection to Main explicitly rather than trusting where
            // the mode flip leaves it, then restore the downlink dial there.
            // A refused pin marks the selection stray instead of writing
            // blind — the write would land wherever the flip left things (the
            // next verb's `ensure_main` repairs it, and the next Doppler
            // correction rewrites the dial).
            if self.select("Main") {
                let _ = self.ack(commands::set_freq(self.addr, hz));
            } else {
                g.sel_stray = true;
            }
        }
        g.engaged = true;
        self.split.store(true, Ordering::Relaxed);
        true
    }

    fn ack(&self, f: Frame) -> bool {
        self.h.transact(f, Expect::Ack).is_ok()
    }
    fn read(&self, f: Frame, cmd: u8, sub: Option<u8>) -> Result<Frame, CivError> {
        self.h.transact(f, Expect::Reply { cmd, sub })
    }

    /// Read a `15 <sub>` transmit meter and format it through `cal` (raw 0–255 → engineering
    /// unit) to `decimals` places. Returns None if the rig doesn't answer (e.g. not keyed).
    fn tx_meter(&self, sub: u8, cal: fn(u16) -> f32, decimals: usize) -> Option<String> {
        let f = self
            .read(commands::read_meter(self.addr, sub), 0x15, Some(sub))
            .ok()?;
        let raw = commands::parse_meter_raw(&f, sub)?;
        Some(format!("{:.*}", decimals, cal(raw)))
    }

    /// Read a `14 <sub>` DSP level as a 0..1 fraction string (the rigctld level convention),
    /// from receiver `who` — or from the radio itself, `None` ([`Self::read_from`]).
    fn dsp_level(&self, who: Option<ReceiverId>, sub: u8) -> Option<String> {
        let f = self.read_from(
            who,
            commands::read_dsp_level(self.addr, sub),
            0x14,
            Some(sub),
        )?;
        let raw = commands::parse_dsp_level_raw(&f, sub)?;
        Some(format!("{:.2}", f64::from(raw) / 255.0))
    }
    /// Set a `14 <sub>` DSP level from a 0..1 fraction string, on `who` ([`Self::ack_on`]).
    fn set_dsp_level_pct(&self, who: Option<ReceiverId>, sub: u8, value: &str) -> Option<bool> {
        let frac: f64 = value.parse().ok()?;
        let percent = (frac.clamp(0.0, 1.0) * 100.0).round() as u8;
        Some(self.ack_on(who, commands::set_dsp_level(self.addr, sub, percent)))
    }

    // ---- which receiver a command is for (the module note) ----

    /// Send `frames` to receiver `rx`, in order, each the way this radio lets a command name
    /// its receiver — THE one place a per-receiver verb reaches the wire. Every frame is sent
    /// whatever the one before it answered (a rig that refuses a tone's switch must still get
    /// the tone), and each result comes back in order.
    ///
    /// `None` = the receiver could not be reached at all: a Sub named on a radio this build
    /// offers none for, or a selection the rig would not move — the refusal `set_freq` makes
    /// over a stranded selection, for the same reason: a command that lands on the wrong
    /// receiver is worse than one that does not land.
    ///
    /// ⛔ Never call this from the keying path. It may take the band lock and put a selection
    /// round trip on the wire, and neither may ever sit between the go and PTT-on, or in
    /// front of an unkey.
    fn on_receiver(
        &self,
        rx: ReceiverId,
        frames: Vec<(Frame, Expect)>,
    ) -> Option<Vec<Result<Frame, CivError>>> {
        let named_model = match self.rx_addressing {
            // One receiver: exactly the bytes this always sent. There is no Sub to name.
            RxAddressing::Single => {
                return (rx == ReceiverId::Main).then(|| {
                    frames
                        .into_iter()
                        .map(|(f, e)| self.h.transact(f, e))
                        .collect()
                });
            }
            RxAddressing::BandDirected(m) => Some(m),
            RxAddressing::HeldSelection => None,
        };
        let band = match rx {
            ReceiverId::Main => commands::BAND_MAIN,
            ReceiverId::Sub => commands::BAND_SUB,
        };
        let by_name =
            |f: &Frame| named_model.is_some_and(|m| commands::band_directed_supported(m, f));
        let send_all = |frames: Vec<(Frame, Expect)>| -> Vec<Result<Frame, CivError>> {
            frames
                .into_iter()
                .map(|(f, e)| {
                    if by_name(&f) {
                        self.transact_on_band(band, f, e)
                    } else {
                        self.h.transact(f, e)
                    }
                })
                .collect()
        };
        // Every frame names its receiver on the wire: nothing to select, nothing to hold.
        if frames.iter().all(|(f, _)| by_name(f)) {
            return Some(send_all(frames));
        }
        // Otherwise HOLD the selection for the whole exchange, so no other client's
        // select-and-restore can move it mid-command (the daemon serves a thread per client).
        let mut g = self.band();
        if !self.ensure_main(&mut g) {
            return None; // stranded on Sub and the rig will not move it: refuse, never guess
        }
        match rx {
            ReceiverId::Main => Some(send_all(frames)),
            ReceiverId::Sub => {
                let out = self.select("Sub").then(|| send_all(frames));
                // ALWAYS hand the selection back, even when the Sub select itself failed: a
                // select that timed out may still have landed.
                let restored = self.restore_main(&mut g);
                out.filter(|_| restored)
            }
        }
    }

    /// Hand the selection back to Main after a Sub-named sequence, and say whether it went —
    /// the tail `set_split_freq` and `set_split_mode` each spell out inline. A refused restore
    /// is REMEMBERED as [`SatSplit::sel_stray`], never shrugged off; a good one re-reads the
    /// dial so the engine's state cache holds Main's frequency again, in case a transceive
    /// push during the Sub window folded the Sub's in. Callers hold the band lock.
    fn restore_main(&self, g: &mut SatSplit) -> bool {
        let restored = self.select("Main");
        g.sel_stray = !restored;
        if restored {
            let _ = self.read(commands::read_freq(self.addr), 0x03, None);
        }
        restored
    }

    /// One command in band-directed form ([`commands::band_directed`]), its reply unwrapped so
    /// the ordinary decoders read it. An ack stays an ack: Icom drops the prefix from those.
    fn transact_on_band(&self, band: u8, f: Frame, expect: Expect) -> Result<Frame, CivError> {
        let expect = match expect {
            Expect::Reply { cmd, sub } => Expect::ReplyOnBand { band, cmd, sub },
            other => other,
        };
        self.h
            .transact(commands::band_directed(band, &f), expect)
            .map(commands::band_directed_reply)
    }

    /// One read from receiver `who` — or, `None`, from the radio itself (a transmitter level
    /// or meter), sent exactly as it always was.
    fn read_from(
        &self,
        who: Option<ReceiverId>,
        f: Frame,
        cmd: u8,
        sub: Option<u8>,
    ) -> Option<Frame> {
        match who {
            None => self.read(f, cmd, sub).ok(),
            Some(rx) => self
                .on_receiver(rx, vec![(f, Expect::Reply { cmd, sub })])?
                .pop()?
                .ok(),
        }
    }

    /// One acked set on receiver `who` — or, `None`, on the radio itself.
    fn ack_on(&self, who: Option<ReceiverId>, f: Frame) -> bool {
        match who {
            None => self.ack(f),
            Some(rx) => self
                .on_receiver(rx, vec![(f, Expect::Ack)])
                .is_some_and(|r| r.iter().all(Result::is_ok)),
        }
    }

    /// WHO a level command is for, when `rx` asked: the receiver, for a receive-chain level;
    /// the radio (`Some(None)`), for a transmitter level asked of Main; nobody (`None`) for a
    /// transmitter level asked of the Sub, which has no transmitter of its own to report.
    fn level_target(rx: ReceiverId, names_a_receiver: bool) -> Option<Option<ReceiverId>> {
        if names_a_receiver {
            Some(Some(rx))
        } else {
            (rx == ReceiverId::Main).then_some(None)
        }
    }

    // ---- the per-receiver verbs, for a named receiver. `RigBackend` passes Main. ----

    /// [`RigBackend::level`] for receiver `rx`.
    fn level_on(&self, rx: ReceiverId, name: &str) -> Option<String> {
        let who = Self::level_target(rx, level_names_a_receiver(name))?;
        match name {
            "STRENGTH" => {
                let f = self.read_from(who, commands::read_smeter(self.addr), 0x15, Some(0x02))?;
                let raw = commands::parse_smeter_raw(&f)?;
                Some(format!(
                    "{}",
                    commands::smeter_db_rel_s9(raw).round() as i32
                ))
            }
            "RFPOWER" => {
                let f = self
                    .read(commands::read_rf_power(self.addr), 0x14, Some(0x0A))
                    .ok()?;
                let raw = commands::parse_rf_power_raw(&f)?;
                Some(format!("{:.2}", f64::from(raw) / 255.0))
            }
            "MICGAIN" => {
                let f = self
                    .read(commands::read_mic_gain(self.addr), 0x14, Some(0x0B))
                    .ok()?;
                let raw = commands::parse_mic_gain_raw(&f)?;
                Some(format!("{:.2}", f64::from(raw) / 255.0))
            }
            // Transmit meters (0x15 read family). Values are already in engineering units:
            // SWR ratio, ALC 0..1, Po in watts, COMP in dB. Meaningful only while keyed.
            "SWR" => self.tx_meter(commands::METER_SWR, commands::swr_from_raw, 2),
            "ALC" => self.tx_meter(commands::METER_ALC, commands::alc_frac_from_raw, 3),
            // Answer BOTH tokens with true watts: Hamlib's plain RFPOWER_METER is a normalized
            // 0..1 fraction while _WATTS is watts, and Nexus polls _WATTS so the reading is watts
            // on any rig. The native daemon has only the one calibrated Po meter, so it serves
            // watts for either name (a Hamlib rig lacking _WATTS returns None → the row hides).
            "RFPOWER_METER" | "RFPOWER_METER_WATTS" => {
                self.tx_meter(commands::METER_PO, commands::po_watts_from_raw, 1)
            }
            "COMP_METER" => self.tx_meter(commands::METER_COMP, commands::comp_db_from_raw, 1),
            // ATTENUATOR and PREAMP — INTEGER DECIBELS, not the 0..1 fractions everything
            // else on this surface deals in, and each on a register of its own. Both answer
            // `None` (→ `RPRT -11`, and the poll then hides the control) when this build has
            // no step list for the rig, because neither value can be interpreted without one.
            "ATT" => {
                let model = self.model?;
                if commands::attenuator_steps_db(model).is_empty() {
                    return None;
                }
                let f = self.read_from(
                    who,
                    commands::read_attenuator(self.addr),
                    commands::ATT_CMD,
                    None,
                )?;
                Some(commands::parse_attenuator_db(&f)?.to_string())
            }
            // The rig answers with a POSITION; the operator is shown the LABEL that position
            // has on THIS rig. A position the list cannot explain is no reading at all rather
            // than a number passed through — see `preamp_db_for_index`.
            "PREAMP" => {
                let model = self.model?;
                if commands::preamp_steps_db(model).is_empty() {
                    return None;
                }
                let f = self.read_from(
                    who,
                    commands::read_preamp(self.addr),
                    0x16,
                    Some(commands::FUNC_PREAMP),
                )?;
                let idx = commands::parse_preamp_index(&f)?;
                Some(commands::preamp_db_for_index(model, idx)?.to_string())
            }
            // AGC as the Hamlib enum int (OFF=0/FAST=2/SLOW=3/MEDIUM=5), translated from the rig's
            // Icom byte so the rigctld side stays Hamlib-native.
            "AGC" => {
                let f = self.read_from(who, commands::read_agc(self.addr), 0x16, Some(0x12))?;
                let civ = commands::parse_agc_civ(&f)?;
                Some(format!("{}", commands::agc_hamlib_from_civ(civ)))
            }
            // The fractional `0x14 <sub>` family — AF gain, RF gain, squelch, NR, NB and
            // compressor DEPTH — all 0..1 like mic gain, and all distinct from the
            // NR/NB/COMP on/off FUNCS on `0x16`. `COMP` here is the knob; `COMP_METER`
            // above is the TX meter, and they are answered by name before this arm.
            // One token table (`commands::level_sub`) serves this and the setter below, so
            // the two cannot drift apart or transpose a pair.
            _ => commands::level_sub(name).and_then(|sub| self.dsp_level(who, sub)),
        }
    }

    /// [`RigBackend::set_level`] for receiver `rx`.
    fn set_level_on(&self, rx: ReceiverId, name: &str, value: &str) -> Option<bool> {
        let who = Self::level_target(rx, level_names_a_receiver(name))?;
        match name {
            "RFPOWER" => {
                let frac: f64 = value.parse().ok()?;
                let percent = (frac.clamp(0.0, 1.0) * 100.0).round() as u8;
                Some(self.ack(commands::set_rf_power(self.addr, percent)))
            }
            "MICGAIN" => {
                let frac: f64 = value.parse().ok()?;
                let percent = (frac.clamp(0.0, 1.0) * 100.0).round() as u8;
                Some(self.ack(commands::set_mic_gain(self.addr, percent)))
            }
            "AGC" => {
                // Value is the Hamlib AGC enum int; translate to the rig's Icom byte.
                let hamlib: u8 = value.parse().ok()?;
                Some(self.ack_on(
                    who,
                    commands::set_agc(self.addr, commands::agc_civ_from_hamlib(hamlib)),
                ))
            }
            "KEYSPD" => {
                let wpm: u32 = value.parse().ok()?;
                Some(self.ack(commands::set_keyer_speed_wpm(self.addr, wpm)))
            }
            // ⚠️ THE TWO INTEGER-DECIBEL LEVELS, and they must never reach the percent path
            // below. `L ATT 12` down that path becomes "12 %" and then a level byte; the
            // operator asks for a 12 dB pad and the radio is sent something else entirely.
            //
            // A value this rig has no step for is REFUSED (`Some(false)` → `RPRT -1`, "the
            // rig said no"), never rounded to the nearest pad it does have: an attenuator is
            // a list of a particular radio's own choices, and substituting a neighbour
            // changes the front end by an amount nobody asked for. A value that is not a
            // whole number of decibels is refused the same way rather than truncated — and
            // refused rather than answered `None`, which would latch the whole control
            // unsupported over one malformed line.
            "ATT" => {
                let model = self.model?;
                let steps = commands::attenuator_steps_db(model);
                if steps.is_empty() {
                    return None;
                }
                let Ok(db) = value.trim().parse::<u8>() else {
                    return Some(false);
                };
                if db != 0 && !steps.contains(&db) {
                    return Some(false);
                }
                Some(self.ack_on(who, commands::set_attenuator_db(self.addr, db)))
            }
            "PREAMP" => {
                let model = self.model?;
                if commands::preamp_steps_db(model).is_empty() {
                    return None;
                }
                let Ok(db) = value.trim().parse::<u8>() else {
                    return Some(false);
                };
                // The LABEL the operator picked → the POSITION the rig is told to select.
                let Some(idx) = commands::preamp_index_for_db(model, db) else {
                    return Some(false);
                };
                Some(self.ack_on(who, commands::set_preamp_index(self.addr, idx)))
            }
            // The same `0x14 <sub>` family as the getter, off the same table.
            _ => commands::level_sub(name).and_then(|sub| self.set_dsp_level_pct(who, sub, value)),
        }
    }

    /// [`RigBackend::func`] for receiver `rx`.
    fn func_on(&self, rx: ReceiverId, token: &str) -> Option<bool> {
        // DSP / audio funcs share CI-V command 0x16; the token → sub-command map lives in
        // commands::func_sub. RIT/XIT are separate registers with no simple read here.
        let sub = commands::func_sub(token)?;
        let who = Self::level_target(rx, func_names_a_receiver(token))?;
        let f = self.read_from(
            who,
            commands::read_dsp_func(self.addr, sub),
            0x16,
            Some(sub),
        )?;
        commands::parse_dsp_func(&f, sub)
    }

    /// [`RigBackend::set_func`] for receiver `rx`.
    fn set_func_on(&self, rx: ReceiverId, token: &str, on: bool) -> Option<bool> {
        // No ΔTX, no ΔTX switch: `21 02` is not a command this radio has, so it is not sent,
        // whichever receiver is named. `None` is `RPRT -11`, the answer Hamlib gives for the
        // same radio.
        if token == "XIT" && !self.has_delta_tx() {
            return None;
        }
        let f = match token {
            "RIT" => commands::set_rit_on(self.addr, on),
            "XIT" => commands::set_dtx_on(self.addr, on),
            // NB / NR / ANF / MN / COMP / MON / VOX → the 0x16 DSP-function table.
            _ => commands::set_dsp_func(self.addr, commands::func_sub(token)?, on),
        };
        let who = Self::level_target(rx, func_names_a_receiver(token))?;
        Some(self.ack_on(who, f))
    }

    /// [`RigBackend::set_vfo`] for receiver `rx`. `VFOA`/`VFOB` pick a VFO OF A RECEIVER —
    /// each band carries its own pair (IC-9700 CI-V Reference Guide, `07 00`/`07 01`) — so
    /// they name `rx`. `Main`/`Sub` ARE the selection, whoever asks: a client moving it, sent
    /// exactly as asked — under the band lock on a two-receiver radio, so it can never land
    /// inside another client's select-and-restore and be undone by that sequence's restore.
    fn set_vfo_on(&self, rx: ReceiverId, vfo: &str) -> bool {
        let Some(f) = commands::select_vfo(self.addr, vfo) else {
            return false;
        };
        if !matches!(vfo.to_ascii_uppercase().as_str(), "MAIN" | "SUB") {
            return self.ack_on(Some(rx), f);
        }
        if self.rx_addressing == RxAddressing::Single {
            return self.ack(f);
        }
        let _held = self.band();
        self.ack(f)
    }

    /// The CTCSS tone on receiver `rx` — the transmitting one, Main by default: `1B 00` sets
    /// the tone, `16 42` switches it on (both attempted whatever the first answered); tone
    /// `0` only switches it off.
    fn set_ctcss_on(&self, rx: ReceiverId, tenths: u32) -> Option<bool> {
        let frames = if tenths == 0 {
            vec![(commands::set_tone_func(self.addr, false), Expect::Ack)]
        } else {
            vec![
                (commands::set_repeater_tone(self.addr, tenths), Expect::Ack),
                (commands::set_tone_func(self.addr, true), Expect::Ack),
            ]
        };
        Some(
            self.on_receiver(rx, frames)
                .is_some_and(|r| r.iter().all(Result::is_ok)),
        )
    }
}

impl RigBackend for CivBackend {
    fn owner_transmitting(&self) -> bool {
        self.tx_intent.load(Ordering::Relaxed)
    }

    fn freq_hz(&self) -> u64 {
        let mut g = self.band(); // never read the dial mid Main/Sub sequence
        if !self.ensure_main(&mut g) {
            // The selection may be stranded on Sub (a failed restore): a read
            // now would serve the UPLINK as the dial — and the state cache
            // may hold it too. 0 = no honest reading, same as a dead engine.
            return 0;
        }
        if self.main_dial_by_name() {
            // IC-7610: MAIN's dial by name, whichever band the operator has selected.
            return self.main_freq_by_name();
        }
        match self.read(commands::read_freq(self.addr), 0x03, None) {
            Ok(f) => {
                *self.last_freq_ok.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(std::time::Instant::now());
                commands::parse_freq(&f)
                    .or(self.h.state().freq_hz)
                    .unwrap_or(0)
            }
            // Radio busy (a timeout can be one crowded moment): the last transceive/
            // reply is honest recent truth — FOR A MOMENT. ⚠️ Unbounded, this cache was a
            // lie that never expired (the overnight-radio review, 2026-09-02): a rig switched
            // OFF with its port still present times out on every `03`, and `f` kept serving
            // the last dial — a plausible nonzero number — so the loop's breaker never
            // tripped and the pill stayed green over a dead radio while every write failed.
            // Past [`CIV_CACHE_GRACE`] the honest answer is 0, exactly as for a dead engine.
            Err(CivError::Timeout) => {
                let last = *self.last_freq_ok.lock().unwrap_or_else(|e| e.into_inner());
                if cache_fresh(last, std::time::Instant::now()) {
                    self.h.state().freq_hz.unwrap_or(0)
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }

    fn mode(&self) -> (String, u32) {
        let mut g = self.band(); // the `04` read hits the SELECTED band
        let by_name = self.main_dial_by_name(); // …the IC-7610's `26 00` names Main
        let reply = if !self.ensure_main(&mut g) {
            // Selection possibly stranded on Sub: the read would serve the
            // uplink's mode. Fall back like any failed read (below).
            None
        } else if by_name {
            self.main_mode_by_name()
        } else {
            self.read(commands::read_mode(self.addr), 0x04, None)
                .ok()
                .and_then(|f| commands::parse_mode(&f))
        };
        let st = self.h.state();
        let (mode, _filter) = match reply {
            Some(m) => m,
            // By name, a failed read serves Main's last by-name reading — never the cache,
            // whose transceive pushes report the SELECTED band (see `main_hz`).
            None if by_name => self
                .main_mode
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .unwrap_or((Mode::Usb, None)),
            None => (st.mode.unwrap_or(Mode::Usb), st.filter),
        };
        // Report soundcard-digital as PKTUSB/PKTLSB/PKTFM, the names the rest of Nexus
        // speaks. FM-D belongs here for the same reason the other two do — the app compares
        // this read-back against the mode it commanded, and a rig answering a bare "FM" to a
        // commanded PKTFM reads as a mode mismatch.
        //
        // ⚠️ All three arms need `data_mode`, which the state cache only ever learns from a
        // RECEIVED `1A 06` frame (a transceive push): nothing here solicits one, so in
        // practice this reports the plain mode today. Adding FM keeps the three consistent
        // rather than leaving one to answer differently the day something does read it.
        let name = match (mode, st.data_mode.unwrap_or(false)) {
            (Mode::Usb, true) => "PKTUSB".to_string(),
            (Mode::Lsb, true) => "PKTLSB".to_string(),
            (Mode::Fm, true) => "PKTFM".to_string(),
            (m, _) => m.name().to_string(),
        };
        (name, 0) // passband unreported (0 = unknown to Hamlib clients)
    }

    /// Is the transmitter keyed, by anyone? The radio has ONE transmitter, so this names no
    /// receiver and takes no lock — `1C 00` carries no band-directed mark on the IC-7610
    /// (A7380-7EX-4 p. 9) and the IC-9700 has no such form. Exactly the bytes it always sent.
    fn ptt(&self) -> bool {
        self.read(commands::read_ptt(self.addr), 0x1C, Some(0x00))
            .ok()
            .and_then(|f| commands::parse_ptt(&f))
            .or(self.h.state().ptt)
            .unwrap_or(false)
    }

    fn split(&self) -> bool {
        if self.band().engaged {
            // Report what the rig IS, not what we last commanded: the operator
            // can leave satellite mode from the front panel, and the `s` verb
            // must say so. Read failures (one busy moment) keep the last state.
            return self.read_satmode().ok().flatten().unwrap_or(true);
        }
        self.split.load(Ordering::Relaxed)
    }

    fn vfo(&self) -> String {
        // In satellite mode every sequence hands the selection back to Main —
        // that is the printed truth beside the split state.
        if self.band().engaged {
            "Main".to_string()
        } else {
            "VFOA".to_string()
        }
    }

    fn set_freq(&self, hz: u64) -> bool {
        let mut g = self.band(); // the `05` write hits the SELECTED band
                                 // A write with the selection stranded on Sub would land the downlink
                                 // in the uplink's band — re-assert Main first, refuse otherwise.
        if self.main_dial_by_name() {
            // IC-7610: MAIN's dial by name (`25 00`), whichever band the operator has selected —
            // the receiver `f` reads. Same refusal over a selection Nexus stranded.
            return self.ensure_main(&mut g)
                && self.ack(commands::set_band_freq(self.addr, commands::BAND_MAIN, hz));
        }
        self.ensure_main(&mut g) && self.ack(commands::set_freq(self.addr, hz))
    }

    fn set_mode(&self, mode: &str, _passband_hz: u32) -> bool {
        let mut g = self.band(); // the `06` write hits the SELECTED band
        if !self.ensure_main(&mut g) {
            return false; // same stray-selection refusal as `set_freq`
        }
        // PKT*/DATA-* = base mode + DATA mode on; every plain mode turns DATA off.
        let up = mode.to_ascii_uppercase();
        let (base, data) = match up.as_str() {
            "PKTUSB" | "DATA-U" | "PKT-U" => (Mode::Usb, true),
            "PKTLSB" | "DATA-L" | "PKT-L" => (Mode::Lsb, true),
            // FM-D, and it is the whole IC-9700 half of the SSTV-on-FM fix: the `1A 06`
            // DATA verb below has always been wired, it had simply never been paired with
            // `Mode::Fm` — so the only FM this daemon could command was one with DATA
            // actively turned OFF, i.e. the modulator handed back to the mic. An SSTV
            // picture sent that way radiates nothing.
            //
            // ⚠️ PLAIN "FM" IS DELIBERATELY NOT IN THIS ARM. It falls through to
            // `Mode::from_name` → `(Mode::Fm, false)`, so APRS, repeater voice and every
            // other FM user still gets DATA explicitly OFF, exactly as before. That
            // guarantee is pinned by the test below; do not "simplify" the two into one.
            "PKTFM" | "FM-D" | "PKT-FM" => (Mode::Fm, true),
            _ => match Mode::from_name(&up) {
                Some(m) => (m, false),
                None => return false,
            },
        };
        if self.main_dial_by_name() {
            // IC-7610: MAIN's mode by name — ONE `26 00` frame that leaves the radio where the
            // `06` + `1A 06` pair below did (`commands::set_band_mode`): a plain mode with DATA
            // off and its default filter, a DATA mode with the operator's D1–D3 and FIL1.
            let n = data.then(|| self.data_mode.load(std::sync::atomic::Ordering::Relaxed));
            return self.ack(commands::set_band_mode(
                self.addr,
                commands::BAND_MAIN,
                base,
                n,
            ));
        }
        let mode_ok = self.ack(commands::set_mode(self.addr, base, None));
        // Data-mode set: tolerate a NAK when turning it OFF (some rigs NAK a redundant
        // off) but require the ACK when turning it ON — FT8 must actually get USB-D.
        // The operator's DATA mode (D1/D2/D3), not a hard 1 — see `set_data_mode_n`. Turning
        // data OFF is still just off.
        let data_ok = if data {
            let n = self.data_mode.load(std::sync::atomic::Ordering::Relaxed);
            self.ack(commands::set_data_mode_n(self.addr, n, None))
        } else {
            self.ack(commands::set_data_mode(self.addr, false, None))
        };
        mode_ok && (data_ok || !data)
    }

    /// ⛔ KEY AND UNKEY — ONE FRAME, NO RECEIVER, NO LOCK, and that is deliberate on a
    /// two-receiver radio too. The RIG decides which receiver transmits (Main; the uplink, Sub,
    /// in satellite mode), so there is nothing to name; and anything added here would sit
    /// between the go and PTT-on (moving key-on timing) or in front of an unkey (a carrier the
    /// operator cannot drop while a selection sequence finishes). Pinned by
    /// `nothing_new_rides_between_the_go_and_ptt_on` and
    /// `an_unkey_never_waits_on_the_band_lock`.
    fn set_ptt(&self, on: bool) -> bool {
        self.ack(commands::set_ptt(self.addr, on))
    }

    fn set_vfo(&self, vfo: &str) -> bool {
        self.set_vfo_on(ReceiverId::Main, vfo)
    }

    /// The rig's own pads and preamps, so a client reading `\dump_state` learns which
    /// values exist instead of trying them. Empty for a model this build has no list for —
    /// see [`CivBackend::model`].
    ///
    /// These describe MAIN's front end — the receiver `\dump_state` reports (D7: Main owns
    /// the radio's stages). No wire traffic, so nothing to hold. Whether a Sub's pads match is
    /// the capability model's question, not this list's.
    fn preamp_steps_db(&self) -> Vec<u8> {
        self.model
            .map(|m| commands::preamp_steps_db(m).to_vec())
            .unwrap_or_default()
    }
    fn attenuator_steps_db(&self) -> Vec<u8> {
        self.model
            .map(|m| commands::attenuator_steps_db(m).to_vec())
            .unwrap_or_default()
    }

    // ⭐ THE RECEIVE-SIDE VERBS NAME A RECEIVER — Main, the one this single-receiver surface
    // describes ([`crate::dualrx::ReceiverId::Main`]). Each body lives in its `_on` twin, which a
    // caller naming the Sub will use; see the module note for how the naming is carried.

    fn level(&self, name: &str) -> Option<String> {
        self.level_on(ReceiverId::Main, name)
    }

    fn set_level(&self, name: &str, value: &str) -> Option<bool> {
        self.set_level_on(ReceiverId::Main, name, value)
    }

    /// A level for a NAMED receiver (`L Sub AF 0.50`) — the door the cockpit's Sub controls come
    /// in by. The work is the same per-receiver verb the plain `set_level` above routes Main
    /// through; on a radio this build offers no Sub for it refuses, sending nothing.
    fn set_receiver_level(&self, rx: ReceiverId, name: &str, value: &str) -> Option<bool> {
        CivBackend::set_level_on(self, rx, name, value)
    }

    fn func(&self, token: &str) -> Option<bool> {
        self.func_on(ReceiverId::Main, token)
    }

    fn set_func(&self, token: &str, on: bool) -> Option<bool> {
        self.set_func_on(ReceiverId::Main, token, on)
    }

    /// CAT CW KEYS THE TRANSMITTER, so it is the keying path and names no receiver: `17` carries
    /// no band-directed mark (A7380-7EX-4 p. 4), the rig sends the CW on whichever receiver
    /// transmits, and a selection round trip in front of the first chunk would delay key-on.
    /// Exactly the bytes it always sent — as is `stop_morse`, an unkey that must never wait.
    fn send_morse(&self, text: &str) -> Option<bool> {
        // Chunk to the rig's per-frame CW text limit; all chunks must ack.
        let bytes: Vec<u8> = text.bytes().filter(u8::is_ascii).collect();
        if bytes.is_empty() {
            return Some(false);
        }
        let ok = bytes.chunks(commands::MORSE_CHUNK).all(|c| {
            let chunk = String::from_utf8_lossy(c);
            self.ack(commands::send_morse(self.addr, &chunk))
        });
        Some(ok)
    }

    fn stop_morse(&self) -> Option<bool> {
        Some(self.ack(commands::stop_morse(self.addr)))
    }

    // `28 00 00`, the Voice TX memory STOP of each model's own reference (see
    // `commands::voice_tx_stop_defined`). A radio switch sends `\stop_voice_mem` to the radio it
    // leaves, and a logger can send it through the broker; on a model with no such stop this
    // stays not-implemented (`RPRT -11`), as it was for every model before.
    fn stop_voice_mem(&self) -> Option<bool> {
        if !self.model.is_some_and(commands::voice_tx_stop_defined) {
            return None;
        }
        Some(self.ack(commands::stop_voice_tx(self.addr)))
    }

    fn set_split(&self, on: bool, tx_vfo: &str) -> Option<bool> {
        let mut g = self.band();
        // TX on the SUB BAND = the rig's satellite mode, not `0F` (same-band
        // A/B split, which cannot be cross-band on this family). Any other
        // TX-VFO token keeps the shipped `0F` path byte-identical.
        if on && tx_vfo.eq_ignore_ascii_case("sub") {
            return Some(self.engage_sat_split(&mut g));
        }
        if g.engaged {
            // Any other split request while the split rides satellite mode
            // ends the session FIRST — leaving it is releasing satellite
            // mode, and an A/B request (`S 1 VFOB`: WSJT-X Split-Operation
            // mid-pass) must never fire `0F` at a rig still in it, or
            // `set_split_freq` keeps routing the A/B TX dial into the Sub
            // band and TX leaves on the downlink band. A refused release
            // refuses the whole request.
            if !self.release_sat_split(&mut g) {
                return Some(false);
            }
            if !on {
                return Some(true); // released — never 0F 00 at this rig
            }
        }
        // The A/B split is SAME-BAND by construction on this family, so a rig
        // the operator left in (cross-band) satellite mode has to come out of
        // it first — and go back in when we hand the split back.
        if on && !self.clear_operator_satmode(&mut g) {
            return Some(false);
        }
        let ok = self.ack(commands::set_split(self.addr, on));
        if ok {
            self.split.store(on, Ordering::Relaxed);
            if !on {
                self.restore_operator_satmode(&mut g);
            }
        }
        Some(ok)
    }

    fn set_split_freq(&self, hz: u64) -> Option<bool> {
        let mut g = self.band();
        if !self.ensure_main(&mut g) {
            // `25 01` writes the unselected VFO of the CURRENT band — with a
            // stray selection either path would write the wrong register.
            return Some(false);
        }
        if !g.engaged {
            return Some(self.ack(commands::set_unselected_freq(self.addr, hz)));
        }
        // Satellite mode: the TX dial lives in the SUB band. Select-write-
        // verify-restore, atomic under the band lock. Success ONLY when the
        // rig's own read-back returns the frequency we sent — per LAW, what
        // was DONE, never what was computed.
        let ok = self.select("Sub")
            && self.ack(commands::set_freq(self.addr, hz))
            && self
                .read(commands::read_freq(self.addr), 0x03, None)
                .ok()
                .and_then(|f| commands::parse_freq(&f))
                == Some(hz);
        // ALWAYS hand the selection back to Main — even mid-failure — and
        // re-read the dial so the engine's state cache holds MAIN's frequency
        // again (the Sub read above folded the uplink into it; a later timeout
        // fallback must never serve the uplink as the dial). A REFUSED
        // restore is remembered, not shrugged off: the selection is stray
        // until `ensure_main` repairs it, and the cache re-read is skipped
        // (it would fold Sub's dial in a second time).
        let restored = self.select("Main");
        g.sel_stray = !restored;
        if restored {
            let _ = self.read(commands::read_freq(self.addr), 0x03, None);
        }
        Some(ok && restored)
    }

    fn set_split_mode(&self, mode: &str, _passband_hz: i32) -> Option<bool> {
        let mut g = self.band();
        let Some(m) = Mode::from_name(mode) else {
            return Some(false);
        };
        if !g.engaged {
            // ⚠️ NEEDS-BENCH (IC-9700 — field report 2026-08-16, V/U FM pass
            // transmitted LSB). This used to answer `None` (`RPRT -11`, "not
            // implemented"), which meant the A/B split — the shape every V/V
            // pass and every terrestrial pile-up rides — had NO way to set its
            // TX VFO's mode at all. `26 01` is that way: it addresses the
            // current band's unselected VFO directly, the same register `25 01`
            // writes the frequency into, so no VFO swap and no selection to
            // restore. Unacked ⇒ `Some(false)`, and the caller says so out loud
            // ("put VFO B in FM by hand") rather than leaving the operator to
            // discover it on the air.
            return Some(self.ack(commands::set_unselected_mode(self.addr, m)));
        }
        if !self.ensure_main(&mut g) {
            return Some(false); // same stray-selection refusal as the freq
        }
        // The uplink sideband (`X`, the inverting-bird LSB): command it on the
        // Sub band, selection restored, same discipline as the frequency —
        // including the remembered stray selection on a refused restore.
        let ok = self.select("Sub") && self.ack(commands::set_mode(self.addr, m, None));
        let restored = self.select("Main");
        g.sel_stray = !restored;
        if restored {
            let _ = self.read(commands::read_freq(self.addr), 0x03, None);
        }
        Some(ok && restored)
    }

    /// RIT — a RECEIVE offset, so Main's. `21` has no band-directed mark on the IC-7610
    /// (A7380-7EX-4 p. 9), so it rides the held selection on both two-receiver radios.
    fn set_rit(&self, hz: i32) -> Option<bool> {
        Some(self.ack_on(
            Some(ReceiverId::Main),
            commands::set_rit_offset(self.addr, hz),
        ))
    }

    // ⭐ THE TRANSMIT-SIDE CONFIGURATION VERBS name the TRANSMITTING receiver — Main by default
    // (D1: the transmit gate judges the TX source; outside satellite mode that is Main, "you
    // can transmit on only the Main band", IC-9700 Basic Manual p. 3-2). They are settings, not
    // keying: the radio loop withholds XIT and the repeater push from a keyed rig, so the band
    // lock they may take can never sit between the go and PTT-on, nor in front of an unkey.
    //
    // ⚠️ In a satellite pass the transmitting receiver is the SUB (the uplink). Where the
    // IC-9700 keeps its ΔTX, duplex and tone registers in satellite mode is not stated in its
    // manuals, so these stay on Main — where they have always landed — rather than on a guess;
    // `Engine::fm_repeater_config` records the uplink-tone half of that as NEEDS-BENCH.

    fn set_xit(&self, hz: i32) -> Option<bool> {
        // ⛔ Icom's ΔTX shares the RIT offset register, which is exactly why a radio with no
        // ΔTX must refuse here: on an IC-9700 `21 00` IS the RIT offset, and an XIT written
        // into it lands on the receiver's clarifier and never on the transmitter.
        if !self.has_delta_tx() {
            return None;
        }
        Some(self.ack_on(
            Some(ReceiverId::Main),
            commands::set_rit_offset(self.addr, hz),
        ))
    }

    fn set_rptr_shift(&self, shift: &str) -> Option<bool> {
        Some(self.ack_on(
            Some(ReceiverId::Main),
            commands::set_duplex(self.addr, shift),
        ))
    }

    fn set_rptr_offset(&self, hz: i64) -> Option<bool> {
        // Cmd 0D, 3-byte BCD in 100 Hz units (confirmed IC-9700 ref: 600 kHz → 00 60 00).
        // The offset magnitude is unsigned; direction comes from the duplex shift (`R`).
        Some(self.ack_on(
            Some(ReceiverId::Main),
            commands::set_rptr_offset(self.addr, hz.unsigned_abs()),
        ))
    }

    fn set_ctcss(&self, tenths: u32) -> Option<bool> {
        self.set_ctcss_on(ReceiverId::Main, tenths)
    }
}

/// The running native daemon: the CI-V serial engine + a stoppable rigctld TCP server.
pub struct CivDaemon {
    engine: CivEngine,
    /// The radio's CI-V address — kept for the Drop-time safety key-up.
    civ_addr: u8,
    /// Where the rigctld listener is bound: `tcp_port`, or the port the OS chose for 0.
    local_addr: SocketAddr,
    tcp_stop: Arc<AtomicBool>,
    tcp_thread: Option<JoinHandle<()>>,
    /// Shared with the broker backend: set true while Nexus is transmitting so the disconnect
    /// fail-safe unkey doesn't fire on Nexus's own Rig reconnect (the CI-V PTT-flicker fix).
    tx_intent: Arc<AtomicBool>,
    /// Can a command sent through this daemon name the SUB receiver — see
    /// [`Self::names_receivers`].
    names_receivers: bool,
}

impl CivDaemon {
    /// Start on an already-open transport (tests use the in-memory fake radio).
    /// `data_mode` is REQUIRED here for the same reason it is on [`start`]: it reached the wire
    /// through nothing at all when it was a setter.
    pub fn start_with_io(
        io: Box<dyn super::engine::CivIo>,
        civ_addr: u8,
        tcp_port: u16,
        data_mode: u8,
        model: Option<IcomModel>,
    ) -> std::io::Result<CivDaemon> {
        let engine = CivEngine::start(io, civ_addr);
        let listener = TcpListener::bind(("127.0.0.1", tcp_port))?;
        let local_addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let tx_intent = Arc::new(AtomicBool::new(false));
        let backend: Arc<dyn RigBackend> = Arc::new(CivBackend::new(
            engine.handle(),
            civ_addr,
            tx_intent.clone(),
            data_mode,
            model,
        ));
        let tcp_stop = Arc::new(AtomicBool::new(false));
        let tcp_thread = {
            let stop = tcp_stop.clone();
            std::thread::Builder::new()
                .name("civ-daemon-tcp".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                // WINDOWS GOTCHA: WinSock accept() INHERITS the listener's
                                // non-blocking mode (Linux does not — so tests never saw this).
                                // Our listener is non-blocking (the loop polls tcp_stop), so
                                // without this reset every accepted connection's first idle
                                // read hit WouldBlock, serve_connection's line loop treated it
                                // as an error and closed the connection after ~one command.
                                // Nexus's own Rig client then churned reconnects (os error
                                // 10053) — and when the dropped connection had just asserted
                                // PTT (`T 1`), the disconnect fail-safe unkeyed the radio: the
                                // IC-9700 native-CI-V "PTT flicker".
                                let _ = stream.set_nonblocking(false);
                                let _ = stream.set_nodelay(true);
                                let b = Arc::clone(&backend);
                                std::thread::spawn(move || serve_connection(stream, b));
                            }
                            // Transient accept errors (an aborted pending connection —
                            // WSAECONNRESET on Windows) must NOT kill the listener: a
                            // healthy daemon would turn permanently connection-refused.
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(50));
                            }
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(50));
                            }
                        }
                    }
                })
                .expect("spawn civ-daemon-tcp")
        };
        super::diag::note("CivDaemon created (new serial engine + rigctld TCP)");
        Ok(CivDaemon {
            engine,
            civ_addr,
            local_addr,
            tcp_stop,
            tcp_thread: Some(tcp_thread),
            tx_intent,
            names_receivers: RxAddressing::for_model(model) != RxAddressing::Single,
        })
    }

    /// Open the real COM port and start the daemon (the production entry).
    #[cfg(feature = "serial")]
    /// `data_mode` is the operator's D1/D2/D3 choice — a REQUIRED argument on purpose.
    ///
    /// ⚠️ It shipped as a `set_data_mode_pref` setter first, and nothing ever called it: the
    /// setting saved, the picker moved, the CI-V command supported it, and the wire never saw
    /// it. A setter is easy to forget; a parameter cannot be. Caught by issue triage the same
    /// day it was written, before release.
    pub fn start(
        port_name: &str,
        baud: u32,
        civ_addr: u8,
        tcp_port: u16,
        data_mode: u8,
        model: Option<IcomModel>,
    ) -> std::io::Result<CivDaemon> {
        let mut port = serialport::new(port_name, baud)
            .timeout(super::engine::READ_TIMEOUT)
            .open()
            .map_err(std::io::Error::other)?;
        // The native daemon keys NOTHING — it speaks CI-V and lets the rig do PTT — so both
        // control lines would sit asserted for the whole session on an interface wired to key
        // from either. Same rule as every other port Nexus opens; see
        // `control_line::idle_both_lines`. (This path carries real data at the operator's
        // baud, so it cannot go through `open_control_line_port` and its baud ladder.)
        crate::control_line::idle_both_lines(&mut port);
        Self::start_with_io(Box::new(port), civ_addr, tcp_port, data_mode, model)
    }

    /// The CI-V address to drive `model_name` at, when it's a native-capable Icom.
    pub fn civ_addr_for(model_name: &str) -> Option<u8> {
        IcomModel::from_name(model_name).map(IcomModel::default_civ_addr)
    }

    /// The address the rigctld listener is bound to. A test starts on port 0 and reads the
    /// port here: learning it by binding `:0` and letting go first leaves it free for anything
    /// on the box to take before the daemon binds it.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// False once the serial engine died (port unplugged / denied).
    pub fn is_alive(&self) -> bool {
        self.engine.is_alive()
    }

    /// Can a command sent through this daemon NAME THE SUB (`L Sub …`) — i.e. is this a radio
    /// the capability table offers a Sub for, which the daemon then addresses per receiver
    /// (band-directed on an IC-7610, a held selection on an IC-9700). The radio loop reports it
    /// to the engine, which offers the cockpit's Sub controls only where it is true: a control
    /// that cannot reach its receiver is worse than one that is not drawn. Fixed for the
    /// daemon's life — it is the model's addressing, not a reading.
    pub fn names_receivers(&self) -> bool {
        self.names_receivers
    }

    /// Newest completed scope sweep (latest-wins; `None` until the next arrives).
    pub fn take_scope_row(&self) -> Option<ScopeSweep> {
        self.engine.take_scope_row()
    }

    /// Stream the radio's scope waveform (on for the ACTIVE radio, off for monitors —
    /// the stream would otherwise crowd a monitor's slow poll off the serial link).
    pub fn set_scope_enabled(&self, on: bool) {
        self.engine.set_scope_enabled(on);
    }

    /// Tell the broker whether Nexus itself is transmitting, so the disconnect fail-safe unkey
    /// stands down while we're on the air (a reconnect of Nexus's own Rig must not drop the over).
    /// The service loop calls this each tick with its keyed state.
    pub fn set_tx_intent(&self, on: bool) {
        self.tx_intent.store(on, Ordering::Relaxed);
    }

    /// Flip the rig's DATA mode (`1A 06`) — the TUNE path uses this so a plain-USB Icom
    /// modulates the tune tone from the USB codec (data OFF = mic source = zero RF).
    /// Best-effort single transact; NAKs (rig already there) are fine.
    pub fn set_data_mode(&self, on: bool) {
        let _ = self.engine.handle().transact(
            commands::set_data_mode(self.civ_addr, on, None),
            Expect::Ack,
        );
    }

    /// Main/Sub selector byte for the scope-CONTROL commands: `Some(0x00)` (Main) on dual-scope
    /// rigs, `None` (omit) on single-scope rigs. The stream is already pinned to Main by
    /// `scope_stream_frames`, so controlling the Main scope is what the operator sees.
    fn scope_ms(&self) -> Option<u8> {
        super::scope::scope_is_dual(self.civ_addr).then_some(0x00)
    }

    /// Set the rig's scope SPAN (`27 15`) — the ± half-width in Hz (rig table 2.5k..500k).
    ///
    /// ⚠️ RETURNS THE RIG'S ANSWER, and issue #275 is why it no longer swallows it. "A NAK
    /// (unsupported / in fixed mode) is fine" was this function's own comment, and it was fine
    /// for the daemon and invisible to the operator: on an IC-7300 with the scope in Fixed mode
    /// every span button did nothing, silently, three layers deep (here, and again in the
    /// cockpit's `.catch(() => {})`). A refusal is a FACT ABOUT THE RADIO and belongs in front
    /// of the person holding it.
    ///
    /// The caller decides what to say; `Nak` (the rig rejected it, `FA`) and `Timeout` (nobody
    /// answered) have different cures and must not be collapsed into one message.
    ///
    /// ⚠️ NEEDS-BENCH, and the refusal RULE is not from a vendor document. The frame layout is
    /// read off Hamlib (see `commands`), and "a span is only taken in Center mode" is the
    /// field report's claim, not something confirmed here against Icom's CI-V reference. What
    /// this code does is report what the rig said, which is true either way.
    pub fn set_scope_span(&self, span_hz: u32) -> Result<(), CivError> {
        self.engine
            .handle()
            .transact(
                commands::set_scope_span(self.civ_addr, self.scope_ms(), span_hz),
                Expect::Ack,
            )
            .map(|_| ())
    }

    /// Set the rig's scope REFERENCE level (`27 19`), in tenths of a dB (−200..+200).
    pub fn set_scope_ref(&self, ref_tenths_db: i32) {
        let _ = self.engine.handle().transact(
            commands::set_scope_ref(self.civ_addr, self.scope_ms(), ref_tenths_db),
            Expect::Ack,
        );
    }

    /// Set the rig's scope CENTER/FIXED mode (`27 14`): `true` = fixed (band-edge), `false` =
    /// center (follow the dial).
    pub fn set_scope_center_mode(&self, fixed: bool) {
        let _ = self.engine.handle().transact(
            commands::set_scope_center_mode(self.civ_addr, self.scope_ms(), fixed),
            Expect::Ack,
        );
    }

    /// READ the rig's scope CENTER/FIXED mode (`27 14`). `Some(true)` = fixed, `Some(false)` =
    /// center, `None` when the rig did not answer or answered something else.
    ///
    /// ⚠️ ONLY USED TO EXPLAIN A REFUSAL, never to decide one. `set_scope_span` used to report
    /// every `NG` as "your scope is not in Center mode" — the most likely cause stated as a fact.
    /// An operator whose scope WAS in Center was sent to check a setting that was already right
    /// (2026-09-18, IC-7300, photo showed CENTER lit), and the real cause stayed behind our guess.
    /// `None` is a perfectly good answer here: it means we still do not know, and the caller must
    /// say so rather than fall back to the guess this exists to retire.
    pub fn scope_center_mode(&self) -> Option<bool> {
        self.engine
            .handle()
            .transact(
                commands::read_scope_center_mode(self.civ_addr, self.scope_ms()),
                Expect::Reply {
                    cmd: 0x27,
                    sub: Some(0x14),
                },
            )
            .ok()
            .as_ref()
            .and_then(commands::parse_scope_center_mode)
    }
}

impl Drop for CivDaemon {
    fn drop(&mut self) {
        // TX SAFETY: a radio keyed via CI-V stays keyed when the port merely closes —
        // send a best-effort key-up FIRST, while the serial engine is still alive.
        // Idempotent (an already-RX radio just acks); one choke point covers every
        // native teardown path: rig rebuilds, monitor recycles, handoff drops, app exit.
        if self.engine.is_alive() {
            super::diag::note("CivDaemon::Drop — safety key-up (a daemon is being torn down)");
            let _ = self
                .engine
                .handle()
                .transact(commands::set_ptt(self.civ_addr, false), Expect::Ack);
        }
        self.tcp_stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.tcp_thread.take() {
            let _ = t.join();
        }
        // engine's Drop stops the serial thread (and closes the port).
    }
}

#[cfg(test)]
mod tests {
    /// The cached dial stands in for ONE crowded moment, never for a radio that is off.
    #[test]
    fn a_timed_out_dial_read_serves_the_cache_only_briefly() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        assert!(
            !super::cache_fresh(None, t0),
            "never read = nothing to serve"
        );
        assert!(super::cache_fresh(Some(t0), t0 + Duration::from_secs(2)));
        assert!(
            !super::cache_fresh(Some(t0), t0 + Duration::from_secs(6)),
            "past the grace the honest dial is 0 and the breaker can see the rig is mute"
        );
    }

    use super::super::engine::tests_support::FakeRadio;
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    fn daemon() -> (CivDaemon, u16) {
        // Port 0, read back from the daemon: never probe `:0` and let go (see `local_addr`).
        let (radio, _push) = FakeRadio::new(0xA2);
        let d =
            CivDaemon::start_with_io(Box::new(radio), 0xA2, 0, 1, Some(IcomModel::Ic9700)).unwrap();
        let port = d.local_addr().port();
        (d, port)
    }

    #[test]
    fn a_rigctld_client_drives_the_fake_radio_end_to_end() {
        let (_d, port) = daemon();
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut rd = BufReader::new(c.try_clone().unwrap());
        let mut line = String::new();

        // Exactly what Rig/probe_cat do: read freq, set freq, read back.
        c.write_all(b"f\n").unwrap();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "145000000\n");

        c.write_all(b"F 144200000\n").unwrap();
        line.clear();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "RPRT 0\n");

        c.write_all(b"f\n").unwrap();
        line.clear();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "144200000\n");

        // S-meter through the extended verb (the fake reports raw 120 = S9 = 0 dB).
        c.write_all(b"l STRENGTH\n").unwrap();
        line.clear();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "0\n");
    }

    /// ⭐ ISSUE #275: A SCOPE SPAN THE RADIO REFUSES HAS TO COME BACK AS A REFUSAL. The daemon
    /// dropped the transact result on the floor ("a NAK … is fine"), so on an IC-7300 with the
    /// scope in Fixed mode the span buttons did nothing, silently, and nothing anywhere in the
    /// app could say why.
    ///
    /// The fixture's rule — Fixed mode NAKs `27 15` — reproduces the reported refusal; it is
    /// the field report's claim, not a reading of Icom's CI-V document. What this test pins is
    /// what Nexus does with a NAK, which is true whatever provokes one.
    #[test]
    fn a_scope_span_the_radio_rejects_is_reported_rather_than_swallowed() {
        let (d, _port, regs) = daemon_with_regs();

        // CONTROL FIRST: in Center mode the same span is accepted, so a later refusal is the
        // rig's answer and not "this path never reaches the radio".
        d.set_scope_center_mode(false);
        assert_eq!(
            d.set_scope_span(25_000),
            Ok(()),
            "control: a span in Center mode is taken"
        );
        assert!(
            !regs.lock().unwrap().scope_fixed,
            "control: the fixture really is in Center mode"
        );

        // ⚠️ AND NEXUS CAN NOW ASK WHY, instead of guessing. `set_scope_span` reported every
        // refusal as "your scope is not in Center mode" — the most likely cause stated as a fact —
        // so an operator whose scope WAS in Center was sent to check a setting that was already
        // correct (2026-09-18, IC-7300; their photo showed CENTER lit) while the real cause stayed
        // hidden. Read in BOTH directions, because a reader that always answers "center" would
        // satisfy a one-sided check and still be useless.
        assert_eq!(
            d.scope_center_mode(),
            Some(false),
            "the rig is in Center and must read back as Center"
        );

        // …and in Fixed mode the radio rejects it.
        d.set_scope_center_mode(true);
        assert_eq!(
            d.scope_center_mode(),
            Some(true),
            "the rig is in Fixed and must read back as Fixed — a read stuck on one answer is no \
             better than the guess it replaces"
        );
        assert_eq!(
            d.set_scope_span(25_000),
            Err(CivError::Nak),
            "a refused span must reach the caller — swallowing it is the whole of #275"
        );

        // Nexus must NOT put the scope back to Center to make the button work (operator,
        // 2026-09-14): Fixed is a deliberate pick, and flipping it would be a bigger surprise.
        assert!(
            regs.lock().unwrap().scope_fixed,
            "the refusal must leave the rig's scope mode exactly where the operator put it"
        );
    }

    #[test]
    fn chk_vfo_answers_so_open_cats_probe_finds_us() {
        let (_d, port) = daemon();
        assert!(crate::rigctld_server::probe_rigctld(
            &format!("127.0.0.1:{port}"),
            Duration::from_millis(800),
        ));
    }

    // ===== cross-band split = SATELLITE MODE, never 0F (the IC-9700 contract) =====
    //
    // Field-falsified assumption these pin: `0F 01` + `25 01` puts the uplink on
    // "the Sub band". It does not — `0F` is same-band A/B split and `25 01`
    // writes the unselected VFO of the CURRENT band. Cross-band on a 9700 is
    // Main/Sub band operation: satellite mode ON, Main = downlink, uplink
    // select-written into Sub, selection returned to Main.

    use super::super::engine::tests_support::Regs;

    /// ⭐ THE WATERFALL'S OWN DATA PATH, which had no test at all until a control went looking.
    ///
    /// The engine routes scope WAVEFORM frames to the assembler and keeps them out of request
    /// matching. That routing line is what puts a sweep on the operator's screen, and nothing
    /// exercised it: the assembler is unit-tested in `scope.rs`, and the daemon is tested through
    /// commands, but no test ever pushed a `27 00` burst at the engine and asked whether a sweep
    /// came out. Breaking the routing outright left all 19 scope-named tests green.
    ///
    /// It matters more now, because the condition got NARROWER: it used to swallow every `27`
    /// frame, and now it tests the sub-command so that `27 14`/`27 15` REPLIES can reach request
    /// matching (without which no scope read can ever resolve). A narrowing is exactly the kind of
    /// change that can silently stop feeding the waterfall.
    #[test]
    fn a_waveform_burst_reaches_the_assembler_and_a_reply_does_not() {
        let (radio, push) = FakeRadio::new(0xA2);
        let d =
            CivDaemon::start_with_io(Box::new(radio), 0xA2, 0, 1, Some(IcomModel::Ic9700)).unwrap();

        // A single-frame burst: 27 00 <main> <seq=1> <total=1>, then the header the assembler
        // requires — mode 00 (Center), centre frequency, ± half-width, out-of-range flag — then
        // the points. A header with a zero span is DISCARDED (`hi_hz <= lo_hz`), so an all-zero
        // frame tests nothing; the sweep has to be one a radio could really send.
        let mut data = vec![0x00, 0x00, 0x01, 0x01, 0x00];
        data.extend_from_slice(&crate::civ::frame::freq_to_bcd(14_150_000)); // centre
        data.extend_from_slice(&crate::civ::frame::freq_to_bcd(10_000)); // ± half-width
        data.push(0x00); // in range
        data.extend_from_slice(&[0x40; 16]); // points
        let f = Frame {
            to: 0x00,
            from: 0xA2,
            cmd: 0x27,
            data,
        };
        push.lock().unwrap().extend(f.to_bytes());

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut got = None;
        while std::time::Instant::now() < deadline {
            if let Some(s) = d.take_scope_row() {
                got = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            got.is_some(),
            "a 27 00 waveform burst must reach the assembler — this is the path that draws the \
             waterfall, and narrowing the router is exactly what could cut it"
        );
    }

    fn daemon_with_regs() -> (CivDaemon, u16, Arc<std::sync::Mutex<Regs>>) {
        let (radio, _push) = FakeRadio::new(0xA2);
        let regs = radio.regs();
        let d =
            CivDaemon::start_with_io(Box::new(radio), 0xA2, 0, 1, Some(IcomModel::Ic9700)).unwrap();
        let port = d.local_addr().port();
        (d, port, regs)
    }

    fn client(port: u16) -> (TcpStream, BufReader<TcpStream>) {
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let rd = BufReader::new(c.try_clone().unwrap());
        (c, rd)
    }

    fn roundtrip(c: &mut TcpStream, rd: &mut BufReader<TcpStream>, cmd: &str) -> String {
        c.write_all(cmd.as_bytes()).unwrap();
        let mut line = String::new();
        rd.read_line(&mut line).unwrap();
        line
    }

    /// #95 ON THE NATIVE PATH: the controls an Icom operator lost by turning native CI-V
    /// ON — the very path they enable to get the panadapter. `MN` (the manual notch) and
    /// `COMP` (compressor DEPTH) reach the rig, and the daemon stops answering `RPRT -11`
    /// for them. That answer is the whole defect: the service's poll latches the control
    /// unsupported, the DTO goes `None`, and the cockpit drops the control — silently, so
    /// the operator cannot tell it from a feature Nexus never built.
    #[test]
    fn native_civ_serves_the_manual_notch_and_the_compressor_depth() {
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);

        // This fixture models a 9700's band/scope registers, not its DSP, so it NAKs these
        // — a SET comes back `RPRT -1`, "the rig refused". What must never come back is
        // `RPRT -11`, "this daemon cannot do that", which is what latches the control off.
        assert_ne!(
            roundtrip(&mut c, &mut rd, "U MN 1\n"),
            "RPRT -11\n",
            "the manual notch must not answer as unimplemented"
        );
        assert_ne!(
            roundtrip(&mut c, &mut rd, "L COMP 0.25\n"),
            "RPRT -11\n",
            "compressor depth must not answer as unimplemented"
        );
        let _ = roundtrip(&mut c, &mut rd, "u MN\n");
        let _ = roundtrip(&mut c, &mut rd, "l COMP\n");

        let r = regs.lock().unwrap();
        let sent = |cmd: u8, data: &[u8]| r.log.iter().any(|(c, d)| *c == cmd && d == data);
        assert!(
            sent(0x16, &[0x48, 0x01]),
            "MN on = 16 48 01; {:02x?}",
            r.log
        );
        assert!(sent(0x16, &[0x48]), "MN read = 16 48; {:02x?}", r.log);
        assert!(
            sent(0x14, &[0x0E, 0x00, 0x63]),
            "COMP 25% = 14 0e 00 63; {:02x?}",
            r.log
        );
        assert!(sent(0x14, &[0x0E]), "COMP read = 14 0e; {:02x?}", r.log);
        // …and NOTHING addressed `14 0D`, the notch-POSITION register. Compressor depth
        // landing there would move a notch the operator never asked to move, and it is the
        // register a `NOTCHF` entry in the percent table would have driven to full scale.
        assert!(
            !r.log
                .iter()
                .any(|(c, d)| *c == 0x14 && d.first() == Some(&0x0D)),
            "the notch register must stay out of this; {:02x?}",
            r.log
        );
    }

    #[test]
    fn a_sub_split_rides_satellite_mode_and_lands_the_uplink_in_the_sub_band() {
        // THE field report, as a wire test: downlink 435.640 on Main, uplink
        // 145.965 must land in the SUB band — with the selection handed back to
        // Main so the dial, the scope and every `f` poll keep the downlink.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);

        // The downlink leg (the ordinary retune) lands on Main.
        assert_eq!(roundtrip(&mut c, &mut rd, "F 435640000\n"), "RPRT 0\n");

        // Split to Sub = satellite mode ON — and NOT `0F` (same-band split).
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert!(r.satmode, "satellite mode engaged (16 5A 01)");
            assert!(!r.split, "0F split is same-band — must NOT be used");
            assert!(
                !r.log.iter().any(|(cmd, _)| *cmd == 0x0F),
                "no 0F frame at all under the satellite-mode contract"
            );
            assert_eq!(
                r.main_hz, 435_640_000,
                "engaging satellite mode must not lose the downlink dial"
            );
        }

        // The uplink: select-written into the SUB band, selection restored.
        assert_eq!(roundtrip(&mut c, &mut rd, "I 145965000\n"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.sub_hz, 145_965_000, "the uplink is in the Sub band");
            assert_eq!(r.main_hz, 435_640_000, "Main (the downlink) untouched");
            assert!(!r.sel_sub, "selection handed back to Main");
            assert_eq!(
                r.unselected_hz, 0,
                "25 01 (unselected VFO of the current band) must not be used"
            );
        }

        // The uplink sideband (`X`, an inverting bird): Sub's mode, Main kept.
        assert_eq!(roundtrip(&mut c, &mut rd, "X LSB 0\n"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.sub_mode, 0x00, "LSB commanded on the Sub band");
            assert_eq!(r.main_mode, 0x01, "Main's mode untouched");
            assert!(!r.sel_sub, "selection restored after the mode write");
        }

        // `s` reports what the rig IS: split on (the 16 5A read-back), Main.
        c.write_all(b"s\n").unwrap();
        let mut l1 = String::new();
        let mut l2 = String::new();
        rd.read_line(&mut l1).unwrap();
        rd.read_line(&mut l2).unwrap();
        assert_eq!(l1, "1\n");
        assert_eq!(l2, "Main\n");

        // And the dial reads back the DOWNLINK, not the uplink.
        assert_eq!(roundtrip(&mut c, &mut rd, "f\n"), "435640000\n");

        // Split off = satellite mode OFF — still no 0F.
        assert_eq!(roundtrip(&mut c, &mut rd, "S 0 Sub\n"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert!(!r.satmode, "satellite mode released (16 5A 00)");
            assert!(!r.log.iter().any(|(cmd, _)| *cmd == 0x0F));
        }
    }

    #[test]
    fn an_ab_split_sets_its_tx_vfos_mode_through_26_01() {
        // ⭐ THE V/U FM FIELD REPORT (IC-9700, 2026-08-16): the TX VFO was in
        // LSB on an FM bird. Two holes in series made that, and this is the
        // second — the A/B split, which is the shape a same-band pass and every
        // terrestrial pile-up ride, had NO mode verb at all. `set_split_mode`
        // answered "not implemented", so whatever an earlier inverting linear
        // pass left in the transmit VFO simply stayed there.
        //
        // ⚠️ NEEDS-BENCH: `26 01` is wired here against the fake radio and the
        // manual, not yet against the operator's rig.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);

        // The scene, exactly as the report describes it: the transmit VFO is
        // holding LSB from the pass before.
        assert_eq!(
            regs.lock().unwrap().unselected_mode,
            0x00,
            "scene: the TX VFO carries the previous pass's LSB"
        );

        // A same-band A/B split, then the uplink's mode.
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 VFOB\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "X FM 0\n"), "RPRT 0\n");

        let r = regs.lock().unwrap();
        assert_eq!(
            r.unselected_mode, 0x05,
            "the TRANSMIT VFO is in FM — the register `06` cannot reach"
        );
        assert_eq!(
            r.main_mode, 0x01,
            "and the RECEIVE VFO is untouched: commanding the uplink's mode on \
             the dial would deafen the operator"
        );
        // The bytes, because this is a class-wide CAT change and the frame is
        // the claim: `26 01 <mode> <data>`, addressed like `25 01` beside it.
        let f = r
            .log
            .iter()
            .find(|(cmd, _)| *cmd == 0x26)
            .expect("a 26 frame was sent");
        assert_eq!(
            f.1,
            vec![0x01, 0x05, 0x00],
            "26 01, FM, DATA off — and no trailing filter byte, so the rig keeps \
             the filter the operator chose"
        );
        assert!(
            !r.sel_sub,
            "no VFO swap: `26 01` addresses the unselected VFO in place"
        );
    }

    #[test]
    fn an_ab_split_takes_a_rig_out_of_the_operators_satellite_mode_and_puts_it_back() {
        // ⚠️ NEEDS-BENCH (IC-9700, field report 2026-08-16).
        //
        // The operator works passes with SATELLITE on at the front panel. On
        // this family satellite mode is CROSS-BAND BY CONSTRUCTION — Main and
        // Sub cannot share a band — so a V/V or U/U pass cannot be expressed
        // that way at all and rides `0F` A/B split instead. Fired at a rig
        // still in satellite mode, `0F 01` asks for a transmit VFO that is not
        // where the rig would transmit.
        //
        // `engaged` covers only the satellite mode WE engaged; this is the
        // other half, and it is the one the operator's own habit produces.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);

        // The operator's front panel: SAT on, and nothing of ours put it there.
        regs.lock().unwrap().satmode = true;
        regs.lock().unwrap().log.clear();

        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 VFOB\n"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert!(!r.satmode, "the rig is taken OUT of satellite mode");
            assert!(r.split, "…and the same-band A/B split is on");
            // ORDER IS THE CLAIM: satellite mode must be gone BEFORE `0F 01`,
            // or the split lands on a rig that is still cross-band.
            let off = r
                .log
                .iter()
                .position(|(cmd, d)| {
                    *cmd == 0x16 && d.first() == Some(&0x5A) && d.get(1) == Some(&0)
                })
                .expect("16 5A 00 was sent");
            let split = r
                .log
                .iter()
                .position(|(cmd, d)| *cmd == 0x0F && d.first() == Some(&0x01))
                .expect("0F 01 was sent");
            assert!(
                off < split,
                "satellite mode off BEFORE the split: {:?}",
                r.log
            );
        }

        // Handing the split back restores what we borrowed — and only what we
        // borrowed. The operator set it; they get it back.
        assert_eq!(roundtrip(&mut c, &mut rd, "S 0 VFOB\n"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert!(r.satmode, "satellite mode put back, as the operator had it");
            assert!(!r.split);
        }

        // ---- THE OTHER DIRECTION, or this proves nothing. A rig that was NOT
        // in satellite mode must never be pushed into one on release: we only
        // restore a state we actually changed.
        {
            let mut r = regs.lock().unwrap();
            r.satmode = false; // the ordinary terrestrial rig, SAT never touched
            r.log.clear();
        }
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 VFOB\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "S 0 VFOB\n"), "RPRT 0\n");
        let r = regs.lock().unwrap();
        assert!(
            !r.satmode,
            "a rig we found in simplex is handed back in simplex"
        );
        assert!(
            !r.log
                .iter()
                .any(|(cmd, d)| *cmd == 0x16 && d.first() == Some(&0x5A) && d.get(1) == Some(&1)),
            "nothing may turn satellite mode ON that did not turn it off: {:?}",
            r.log
        );
    }

    #[test]
    fn a_rig_without_satellite_mode_refuses_a_sub_split_honestly() {
        // An IC-7300 has no Sub band: `16 5A` NAKs. The answer must be an
        // honest RPRT -1 — never a silent fall-back to same-band 0F split,
        // which would transmit the "uplink" into the downlink's own band.
        let (radio, _push) = FakeRadio::new(0x94);
        let regs = radio.regs();
        regs.lock().unwrap().no_satmode = true;
        let d =
            CivDaemon::start_with_io(Box::new(radio), 0x94, 0, 1, Some(IcomModel::Ic7300)).unwrap();
        let (mut c, mut rd) = client(d.local_addr().port());

        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT -1\n");
        let r = regs.lock().unwrap();
        assert!(!r.satmode);
        assert!(!r.split, "no silent same-band split");
        assert!(!r.log.iter().any(|(cmd, _)| *cmd == 0x0F));
    }

    #[test]
    fn an_ab_split_while_satmode_is_engaged_releases_the_session_first() {
        // WSJT-X Split-Operation=Rig fires `S 1 VFOB` + `I <shifted dial>` —
        // and a digital over during a satellite pass is a designed-for
        // scenario. Routing that `I` on the satmode session would clobber the
        // uplink in the Sub band, and satmode TX always exits Sub, so the
        // over would leave on the DOWNLINK band. The A/B request must end the
        // session first: satellite mode released, THEN `0F 01`, and the split
        // TX dial rides `25 01` per the A/B contract.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);
        assert_eq!(roundtrip(&mut c, &mut rd, "F 435640000\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "I 145965000\n"), "RPRT 0\n");

        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 VFOB\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "I 435641500\n"), "RPRT 0\n");
        let r = regs.lock().unwrap();
        assert!(!r.satmode, "the satmode session ended before the A/B split");
        assert!(r.split, "0F 01 engaged for the A/B split");
        let rel = r
            .log
            .iter()
            .position(|(cmd, d)| *cmd == 0x16 && d == &[0x5A, 0x00]);
        let ab = r
            .log
            .iter()
            .position(|(cmd, d)| *cmd == 0x0F && d == &[0x01]);
        assert!(
            rel.unwrap() < ab.unwrap(),
            "release precedes 0F — never 0F at a rig still in satellite mode"
        );
        assert_eq!(r.unselected_hz, 435_641_500, "the A/B TX dial rides 25 01");
        assert_eq!(r.sub_hz, 145_965_000, "the Sub band is NOT clobbered");
    }

    #[test]
    fn an_ab_split_that_cannot_release_satmode_refuses_without_0f() {
        // If the rig will not leave satellite mode, the A/B split must be
        // refused whole — firing `0F 01` anyway would route TX out the Sub
        // band while the client believes it set up a same-band split.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT 0\n");
        regs.lock().unwrap().nak_satmode_set = 1;
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 VFOB\n"), "RPRT -1\n");
        let r = regs.lock().unwrap();
        assert!(r.satmode, "the rig really is still in satellite mode");
        assert!(
            !r.log.iter().any(|(cmd, _)| *cmd == 0x0F),
            "no 0F at a rig still in satellite mode"
        );
    }

    #[test]
    fn a_lost_verify_reply_never_strands_the_rig_in_satellite_mode() {
        // One lost CI-V reply on the engage verify used to leave the rig IN
        // satellite mode (on its stored satellite VFOs) while the session
        // believed nothing happened — the later teardown then fired `0F 00`,
        // stranding satmode behind a cleared split. An engage the verify
        // cannot confirm is backed out and the dial restored.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);
        assert_eq!(roundtrip(&mut c, &mut rd, "F 435640000\n"), "RPRT 0\n");
        // Prime the capability cache with a clean engage/release round-trip
        // so the dropped reply below hits the VERIFY read, not the probe.
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "S 0 Sub\n"), "RPRT 0\n");

        regs.lock().unwrap().drop_satmode_reads = 1;
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT -1\n");
        {
            let r = regs.lock().unwrap();
            assert!(!r.satmode, "the unconfirmed engage was backed out");
            assert_eq!(r.main_hz, 435_640_000, "the downlink dial survived");
        }
        // And nothing is poisoned: the next attempt engages cleanly.
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT 0\n");
        let r = regs.lock().unwrap();
        assert!(r.satmode);
        assert_eq!(r.main_hz, 435_640_000);
    }

    #[test]
    fn a_failed_main_restore_is_reasserted_before_the_next_selected_verb() {
        // A refused `07 D0` used to strand the selection on Sub for good:
        // the next Doppler correction's `05` then wrote the DOWNLINK into
        // the Sub band — and satmode TX exits Sub, so the rig would have
        // transmitted on the downlink frequency. The stray selection is
        // remembered and Main re-asserted before every selection-dependent
        // verb.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);
        assert_eq!(roundtrip(&mut c, &mut rd, "F 435640000\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 Sub\n"), "RPRT 0\n");

        regs.lock().unwrap().nak_main_select = 1;
        // The uplink write reports failure (its restore did not land)...
        assert_eq!(roundtrip(&mut c, &mut rd, "I 145965000\n"), "RPRT -1\n");
        assert!(
            regs.lock().unwrap().sel_sub,
            "the rig really is stuck on Sub"
        );
        // ...but the next dial poll and dial write re-assert Main first:
        // the poll serves the DOWNLINK, and the correction lands on Main.
        assert_eq!(roundtrip(&mut c, &mut rd, "f\n"), "435640000\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "F 435641000\n"), "RPRT 0\n");
        let r = regs.lock().unwrap();
        assert!(!r.sel_sub, "Main re-asserted");
        assert_eq!(r.main_hz, 435_641_000, "the correction landed on Main");
        assert_eq!(r.sub_hz, 145_965_000, "…never in the Sub band");
    }

    #[test]
    fn ab_split_keeps_the_existing_bytes_exactly() {
        // Every non-Sub split is the shipped path, byte for byte: `0F 01` then
        // `25 01` — the 7300-family same-band pile-up split.
        let (_d, port, regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);

        assert_eq!(roundtrip(&mut c, &mut rd, "S 1 VFOB\n"), "RPRT 0\n");
        assert_eq!(roundtrip(&mut c, &mut rd, "I 14235000\n"), "RPRT 0\n");
        let r = regs.lock().unwrap();
        assert!(r.split, "0F 01 (same-band split) as before");
        assert!(!r.satmode, "satellite mode is not involved");
        assert_eq!(r.unselected_hz, 14_235_000, "25 01 as before");
        assert_eq!(r.sub_hz, 435_000_000, "the Sub band untouched");
    }

    /// ⭐ THE ICOM HALF OF THE SSTV-ON-FM FIX (field report, FTDX10 + IC-9700, 2026-08-12).
    ///
    /// `FM` and `FM-D` differ by ONE bit on the wire — the `1A 06` DATA flag — and until this
    /// arm existed the daemon could only ever command the version with that bit turned OFF, so
    /// an SSTV image on an FM repeater was modulated from the MIC jack (no RF) or, once the
    /// engine fell through to the sideband arm, sent as USB-D on an FM channel.
    ///
    /// The second half of this test is the guard the audit asked for on a class-wide CAT
    /// change: every OTHER FM user of this daemon — APRS, repeater voice — must still get DATA
    /// explicitly off. Both directions are asserted, because a "DATA is on when it should be"
    /// test that never checks the off case would pass an arm that turned DATA on for all FM.
    #[test]
    fn fm_d_is_the_fm_mode_byte_plus_the_data_flag_and_plain_fm_still_turns_data_off() {
        let (radio, _push) = FakeRadio::new(0xA2);
        let regs = radio.regs();
        let engine = CivEngine::start(Box::new(radio), 0xA2);
        let backend = CivBackend::new(
            engine.handle(),
            0xA2,
            Arc::new(AtomicBool::new(false)),
            1,
            Some(IcomModel::Ic9700),
        );

        assert!(
            backend.set_mode("PKTFM", 0),
            "the daemon must accept the FM data submode"
        );
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.main_mode, 0x05, "CI-V mode byte 05 = FM (the emission)");
            assert!(
                r.data_mode,
                "…with the DATA flag ON, which is what routes the USB codec to the modulator"
            );
        }
        // The `m` read-back still says "FM" here, and that is NOT this change failing: the
        // state cache only learns the DATA flag from a RECEIVED `1A 06` frame (a transceive
        // push), and nothing in the tree solicits one — so the same is true of PKTUSB today.
        // The reporting arm is added for symmetry with the USB/LSB ones, and is asserted only
        // as far as this fake can honestly prove it.
        assert_eq!(
            backend.mode().0,
            "FM",
            "emission reported from the mode byte"
        );

        assert!(backend.set_mode("FM", 0), "plain FM still works");
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.main_mode, 0x05, "same emission…");
            assert!(
                !r.data_mode,
                "…but DATA OFF: an APRS beacon or a voice repeater over must keep taking its \
                 audio from the mic, exactly as before this change"
            );
        }
        assert_eq!(backend.mode().0, "FM");
    }

    #[test]
    fn a_concurrent_dial_poll_never_reads_the_uplink_mid_sequence() {
        // The daemon serves one shared backend to a thread per TCP connection —
        // a WSJT-X `f` poll landing between `07 D1` and `07 D0` would return
        // the UPLINK as the dial, which `sat_observe_operator_tune` reads as
        // "the operator tuned away" (a silent pass-killer). The band lock must
        // make the select-write-restore sequence atomic against every reader.
        let (radio, _push) = FakeRadio::new(0xA2);
        let engine = CivEngine::start(Box::new(radio), 0xA2);
        let backend = Arc::new(CivBackend::new(
            engine.handle(),
            0xA2,
            Arc::new(AtomicBool::new(false)),
            1,
            Some(IcomModel::Ic9700),
        ));
        assert!(backend.set_freq(435_640_000));
        assert_eq!(backend.set_split(true, "Sub"), Some(true));

        let stop = Arc::new(AtomicBool::new(false));
        let poller = {
            let b = backend.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut seen_uplink = false;
                while !stop.load(Ordering::Relaxed) {
                    let hz = b.freq_hz();
                    if hz == 145_965_000 {
                        seen_uplink = true;
                    }
                }
                seen_uplink
            })
        };
        for _ in 0..10 {
            assert_eq!(backend.set_split_freq(145_965_000), Some(true));
        }
        stop.store(true, Ordering::Relaxed);
        let seen_uplink = poller.join().unwrap();
        assert!(!seen_uplink, "a dial poll must never serve the uplink");
        assert_eq!(backend.freq_hz(), 435_640_000);
    }

    /// THE THREE CONTROLS REACH THE RADIO AND COME BACK — monitor, attenuator, preamp, on
    /// the IC-7610 because it is the rig whose pads and preamps have labels that are NOT
    /// their wire bytes. Driven through the `RigBackend` surface the rigctld verbs land on,
    /// with the fake radio's registers as the witness: what the operator asked for, what the
    /// bus carried, and what the read gives back are three different questions here.
    ///
    /// ⚠️ THE ATTENUATOR IS NOT A FRACTION AND THE PREAMP IS NOT A DECIBEL COUNT. Both would
    /// have "worked" through the generic 0..1 percent path — `L ATT 12` would have arrived as
    /// full scale, `L PREAMP 12` as preamp twelve — and both would have moved the operator's
    /// front end by an amount nobody asked for, which is worse than an unimplemented control.
    #[test]
    fn monitor_attenuator_and_preamp_reach_the_wire_and_read_back() {
        let (radio, _push) = FakeRadio::new(0x98);
        let regs = radio.regs();
        let engine = CivEngine::start(Box::new(radio), 0x98);
        let backend = CivBackend::new(
            engine.handle(),
            0x98,
            Arc::new(AtomicBool::new(false)),
            1,
            Some(IcomModel::Ic7610),
        );

        // MONITOR on/off. This half was ALREADY reachable — `func_sub` has held `MON` =>
        // 0x45 all along — and asserted here because nothing in the tree ever asked for it,
        // which is indistinguishable from it not working.
        assert_eq!(backend.set_func("MON", true), Some(true));
        assert_eq!(backend.func("MON"), Some(true));
        assert_eq!(backend.set_func("MON", false), Some(true));
        assert_eq!(backend.func("MON"), Some(false));

        // MONITOR GAIN — a 0..1 fraction on the level family, like AF/RF/SQL.
        // ⚠️ THREE DISAGREEING VALUES: one setting proves nothing about which register the
        // payload reached, because every level in this family takes the same shape.
        for (frac, raw) in [("0.25", 63u16), ("0.50", 127), ("1.00", 255)] {
            assert_eq!(backend.set_level("MONITOR_GAIN", frac), Some(true));
            let sent = regs
                .lock()
                .unwrap()
                .log
                .iter()
                .rev()
                .find(|(c, d)| *c == 0x14 && d.first() == Some(&0x15))
                .map(|(_, d)| {
                    commands::parse_dsp_level_raw(
                        &crate::civ::frame::Frame {
                            to: 0xE0,
                            from: 0x98,
                            cmd: 0x14,
                            data: d.clone(),
                        },
                        0x15,
                    )
                })
                .expect("a 14 15 frame on the bus");
            assert_eq!(sent, Some(raw), "MONITOR_GAIN {frac}");
        }

        // ATTENUATOR — the operator's dB in, BCD dB on the bus, the same dB back out, for
        // every one of the IC-7610's fifteen pads (A7380-7EX-4 p. 3) and then OFF. 3, 6 and 9
        // encode identically under BCD and raw hex; every pad from 12 up does not. Each goes
        // out as ONE band-directed frame naming Main, as the three pads offered before did.
        for (db, wire) in [
            (3u8, 0x03u8),
            (6, 0x06),
            (9, 0x09),
            (12, 0x12),
            (15, 0x15),
            (18, 0x18),
            (21, 0x21),
            (24, 0x24),
            (27, 0x27),
            (30, 0x30),
            (33, 0x33),
            (36, 0x36),
            (39, 0x39),
            (42, 0x42),
            (45, 0x45),
            (0, 0x00),
        ] {
            let n = regs.lock().unwrap().wire.len();
            assert_eq!(
                backend.set_level("ATT", &db.to_string()),
                Some(true),
                "set ATT {db}"
            );
            assert_eq!(regs.lock().unwrap().att_raw, wire, "ATT {db} dB on the bus");
            assert_eq!(
                hex_frames(&regs.lock().unwrap().wire[n..]),
                [format!("FE FE 98 E0 29 00 11 {wire:02X} FD")],
                "ATT {db}: the frame on the wire"
            );
            assert_eq!(
                backend.level("ATT").as_deref(),
                Some(db.to_string().as_str()),
                "read ATT {db} back"
            );
        }
        // A pad this rig does not have is REFUSED, not rounded to a neighbour: 10 dB is the
        // IC-9700's pad, 4 falls between two of the 7610's, and 48 is one 3 dB step past its
        // last. Substituting a neighbour would attenuate by an amount the operator did not
        // choose.
        for db in ["10", "4", "48"] {
            let n = regs.lock().unwrap().wire.len();
            assert_eq!(
                backend.set_level("ATT", db),
                Some(false),
                "{db} dB is not a 7610 pad"
            );
            assert_eq!(
                regs.lock().unwrap().wire.len(),
                n,
                "a refused pad ({db} dB) never reaches the bus"
            );
        }
        assert_eq!(regs.lock().unwrap().att_raw, 0x00);
        // …and the ladder `\dump_state` declares for this rig is that same one.
        assert_eq!(
            backend.attenuator_steps_db(),
            vec![3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45]
        );

        // PREAMP — the LABEL goes in, the POSITION goes on the bus, the LABEL comes back.
        for (db, idx) in [(12u8, 1u8), (20, 2), (0, 0)] {
            assert_eq!(
                backend.set_level("PREAMP", &db.to_string()),
                Some(true),
                "set PREAMP {db}"
            );
            assert_eq!(
                regs.lock()
                    .unwrap()
                    .funcs
                    .get(&commands::FUNC_PREAMP)
                    .copied(),
                Some(idx),
                "PREAMP {db} dB selects position {idx}"
            );
            assert_eq!(
                backend.level("PREAMP").as_deref(),
                Some(db.to_string().as_str()),
                "read PREAMP {db} back"
            );
        }
        // ⭐ AND THE POSITION IS NEVER THE DECIBELS. Stated as its own assertion because an
        // implementation that sent the dB straight through passes every "12 goes in, 12
        // comes out" round trip while asking the radio for a preamp it does not have.
        assert_ne!(
            regs.lock()
                .unwrap()
                .funcs
                .get(&commands::FUNC_PREAMP)
                .copied(),
            Some(12),
            "the bus carries the preamp's position, never its label"
        );
        assert_eq!(
            backend.set_level("PREAMP", "10"),
            Some(false),
            "not a 7610 preamp"
        );
    }

    /// A RIG WITH NO STEP LIST OFFERS NEITHER CONTROL, rather than a guessed one. The IC-905
    /// has no Hamlib backend to read pads off (NEEDS-BENCH), and `None` is what a build that
    /// does not know the model carries. Both must answer "unimplemented" — `None`, which the
    /// rigctld layer turns into `RPRT -11` and the poll latches as unsupported, so the
    /// control disappears instead of moving the wrong thing.
    #[test]
    fn an_unknown_model_offers_no_attenuator_or_preamp_at_all() {
        let (radio, _push) = FakeRadio::new(0xA2);
        let regs = radio.regs();
        let engine = CivEngine::start(Box::new(radio), 0xA2);
        let backend = CivBackend::new(
            engine.handle(),
            0xA2,
            Arc::new(AtomicBool::new(false)),
            1,
            None,
        );

        assert_eq!(backend.set_level("ATT", "12"), None);
        assert_eq!(backend.level("ATT"), None);
        assert_eq!(backend.set_level("PREAMP", "12"), None);
        assert_eq!(backend.level("PREAMP"), None);
        // Nothing was put on the bus for either.
        let r = regs.lock().unwrap();
        assert!(
            !r.log
                .iter()
                .any(|(c, d)| *c == 0x11
                    || (*c == 0x16 && d.first() == Some(&commands::FUNC_PREAMP))),
            "an unknown model must not command a register it cannot interpret"
        );
        // ⭐ THE POSITIVE CONTROL: the monitor is model-INDEPENDENT (a bare on/off on a
        // register every rig in the family shares), so it must still work here. Without it
        // this test would pass just as well against a backend that had gone entirely deaf.
        drop(r);
        assert_eq!(backend.set_func("MON", true), Some(true));
        assert_eq!(backend.func("MON"), Some(true));
        assert_eq!(backend.set_level("MONITOR_GAIN", "0.50"), Some(true));
    }

    // ===== THE 17 VERBS THAT NAME A RECEIVER (the dual-receiver programme's CAT step) =====
    //
    // The vendor ground truth these pin — the citations are in the module header:
    // - IC-7610: Icom's command `29` names Main or Sub "regardless of active/inactive" band
    //   (CI-V Reference Guide A7380-7EX-4, p. 9 and p. 15; per-command marks pp. 3–4, 8).
    // - IC-9700: NO command `29` (A7508-3EX-4's table ends at `28`), so the selection is
    //   HELD under the band lock — the `set_split_freq` pattern.
    // - Every radio the capability table does not offer a Sub: byte-identical to before.

    /// A backend driven directly (no TCP) over a fake radio at `addr`, and that radio's
    /// register file. The engine rides along because dropping it stops the serial thread.
    fn backend_on(
        addr: u8,
        model: Option<IcomModel>,
    ) -> (CivEngine, Arc<CivBackend>, Arc<Mutex<Regs>>) {
        let (radio, _push) = FakeRadio::new(addr);
        let regs = radio.regs();
        let engine = CivEngine::start(Box::new(radio), addr);
        let b = Arc::new(CivBackend::new(
            engine.handle(),
            addr,
            Arc::new(AtomicBool::new(false)),
            1,
            model,
        ));
        // The engine's own first frame is scope-off housekeeping (`27 11 00`), sent
        // asynchronously on its first loop pass. Wait it out, so a test that snapshots the
        // log to see what ONE verb sent never counts the engine's frame as the verb's.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while regs.lock().unwrap().log.is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "engine housekeeping never ran"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        (engine, b, regs)
    }

    /// Plant DISAGREEING values in the two bands' receive registers — every one different
    /// between Main and Sub — so a reading taken from the wrong receiver shows up by its
    /// number, not by luck.
    ///
    /// | | Main | Sub |
    /// |---|---|---|
    /// | AF / RF / SQL / NR / NB level (`14 01/02/03/06/12`) | 200 / 180 / 20 / 150 / 90 | 50 / 30 / 240 / 10 / 220 |
    /// | AGC (`16 12`) | FAST (01) | SLOW (03) |
    /// | preamp position (`16 02`) | 1 | 2 |
    /// | NB / NR / ANF / MN on (`16 22/40/41/48`) | on / off / on / off | off / on / off / on |
    /// | attenuator (`11`) | 12 dB | 18 dB |
    /// | S-meter (`15 02`) | raw 120 (S9) | raw 60 |
    fn plant_two_receivers(regs: &Arc<Mutex<Regs>>) {
        let mut r = regs.lock().unwrap();
        for (sub, main, other) in [
            (0x01u8, 200u16, 50u16),
            (0x02, 180, 30),
            (0x03, 20, 240),
            (0x06, 150, 10),
            (0x12, 90, 220),
        ] {
            r.levels.insert(sub, main);
            r.sub_levels.insert(sub, other);
        }
        for (sub, main, other) in [
            (0x12u8, 0x01u8, 0x03u8),
            (commands::FUNC_PREAMP, 1, 2),
            (0x22, 1, 0),
            (0x40, 0, 1),
            (0x41, 1, 0),
            (0x48, 0, 1),
        ] {
            r.funcs.insert(sub, main);
            r.sub_funcs.insert(sub, other);
        }
        r.att_raw = 0x12;
        r.sub_att_raw = 0x18;
        assert_ne!(
            r.smeter_raw, r.sub_smeter_raw,
            "the fixture's S-meters disagree"
        );
    }

    /// Where did the LAST `cmd` (whose first data byte is `first`, when given) act — `true`
    /// = on the Sub band. Panics if it never reached the radio at all, which is its own
    /// failure and must not read as "on Main".
    fn last_acted_on_sub(regs: &Arc<Mutex<Regs>>, cmd: u8, first: Option<u8>) -> bool {
        regs.lock()
            .unwrap()
            .acted
            .iter()
            .rev()
            .find(|(_, c, d)| *c == cmd && first.is_none_or(|f| d.first() == Some(&f)))
            .map(|(on_sub, _, _)| *on_sub)
            .unwrap_or_else(|| panic!("{cmd:02x} {first:02x?} never reached the radio"))
    }

    /// Put the IC-9700 fake into a satellite pass and STRAND its selection on Sub the way
    /// the field saw it: an uplink write whose Main restore the rig refused.
    fn strand_on_sub(b: &CivBackend, regs: &Arc<Mutex<Regs>>) {
        regs.lock().unwrap().nak_main_select = 1;
        assert_eq!(
            b.set_split_freq(145_965_000),
            Some(false),
            "scene: the Main restore after the uplink write is refused"
        );
        assert!(
            regs.lock().unwrap().sel_sub,
            "scene: the selection really is stranded on Sub"
        );
    }

    /// ⭐ IC-7610 — THE RECEIVE VERBS NAME MAIN, whatever the front panel has selected.
    ///
    /// The operator has touched the SUB band. Before this step every one of these went out
    /// unqualified, so the radio applied it to the SELECTED band — Sub — and the single-
    /// receiver cockpit showed the Sub's S-meter, AF, NB… as "the radio's". Command `29`
    /// names Main without touching the selection ("Regardless of active/inactive the Main
    /// or Sub band, you can directly specify the Main or Sub band", A7380-7EX-4 p. 9), so
    /// the panel never flickers and the operator's choice of band is left exactly as it was.
    #[test]
    fn an_ic7610_names_main_while_the_panel_has_the_sub_band_selected() {
        let (_e, b, regs) = backend_on(0x98, Some(IcomModel::Ic7610));
        plant_two_receivers(&regs);
        regs.lock().unwrap().sel_sub = true; // the operator selected the Sub band

        // READS — Main's number every time; the Sub's is the other column.
        for (name, main, sub) in [
            ("STRENGTH", "0", "-27"),
            ("AF", "0.78", "0.20"),
            ("RF", "0.71", "0.12"),
            ("SQL", "0.08", "0.94"),
            ("NR", "0.59", "0.04"),
            ("NB", "0.35", "0.86"),
            ("AGC", "2", "3"),
            ("ATT", "12", "18"),
            ("PREAMP", "12", "20"),
        ] {
            assert_eq!(
                b.level(name).as_deref(),
                Some(main),
                "l {name}: Main's reading (the Sub's would be {sub})"
            );
        }
        for (token, main) in [("NB", true), ("NR", false), ("ANF", true), ("MN", false)] {
            assert_eq!(b.func(token), Some(main), "u {token}: Main's state");
        }

        // WRITES — into Main's register, with the Sub's left at its planted value.
        for (name, value, sub, raw) in [
            ("AF", "0.50", 0x01u8, 127u16),
            ("RF", "0.25", 0x02, 63),
            ("SQL", "0.75", 0x03, 191),
            ("NR", "1.00", 0x06, 255),
            ("NB", "0.00", 0x12, 0),
        ] {
            assert_eq!(b.set_level(name, value), Some(true), "L {name}");
            let r = regs.lock().unwrap();
            assert_eq!(r.levels.get(&sub), Some(&raw), "L {name} landed on Main");
            assert_ne!(
                r.sub_levels.get(&sub),
                Some(&raw),
                "L {name} left the Sub alone"
            );
        }
        assert_eq!(b.set_level("AGC", "3"), Some(true)); // SLOW
        assert_eq!(b.set_level("ATT", "6"), Some(true));
        assert_eq!(b.set_level("PREAMP", "20"), Some(true)); // position 2
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.funcs.get(&0x12), Some(&0x03), "AGC on Main");
            assert_eq!(
                r.sub_funcs.get(&0x12),
                Some(&0x03),
                "Sub's AGC was already SLOW"
            );
            assert_eq!(r.att_raw, 0x06, "ATT on Main");
            assert_eq!(r.sub_att_raw, 0x18, "the Sub's pad untouched");
            assert_eq!(
                r.funcs.get(&commands::FUNC_PREAMP),
                Some(&2),
                "preamp on Main"
            );
        }
        for (token, on, sub) in [
            ("NB", false, 0x22u8),
            ("NR", true, 0x40),
            ("ANF", false, 0x41),
            ("MN", true, 0x48),
        ] {
            assert_eq!(b.set_func(token, on), Some(true), "U {token}");
            assert!(
                !last_acted_on_sub(&regs, 0x16, Some(sub)),
                "U {token} landed on Main"
            );
            assert_eq!(regs.lock().unwrap().funcs.get(&sub), Some(&u8::from(on)));
        }
        // The CTCSS tone is per band on this radio too (`1B 00` and `16 42` both carry the
        // mark, A7380-7EX-4 pp. 4, 8) — and on the radio's Main band by default.
        assert_eq!(b.set_ctcss(885), Some(true));
        assert!(
            !last_acted_on_sub(&regs, 0x1B, Some(0x00)),
            "the tone frequency on Main"
        );
        assert!(
            !last_acted_on_sub(&regs, 0x16, Some(0x42)),
            "the tone switch on Main"
        );

        // ⭐ AND THE SELECTION NEVER MOVED: not one select on the wire, Sub still selected.
        let r = regs.lock().unwrap();
        assert!(
            r.sel_sub,
            "the operator's Sub selection is exactly where they left it"
        );
        assert!(
            !r.log.iter().any(|(c, _)| *c == 0x07),
            "a band-directed command needs no select at all: {:02x?}",
            r.log
        );
        let on_sub: Vec<_> = r
            .acted
            .iter()
            .filter(|(s, c, _)| *s && matches!(c, 0x11 | 0x14 | 0x15 | 0x16 | 0x1B))
            .collect();
        assert!(
            on_sub.is_empty(),
            "every receive command acted on Main: {on_sub:02x?}"
        );
    }

    /// ⭐ IC-7610 — THE DIAL AND THE MODE ARE MAIN'S, READ BY NAME, whatever the panel selects.
    ///
    /// The receive verbs above name Main with command `29`; the dial and the mode could not,
    /// because `03`/`04` carry no band-directed mark (A7380-7EX-4 p. 9). So with the operator on
    /// the Sub band, "the radio's" frequency and mode were the SUB's — and a turn of the Sub's
    /// dial read as a QSY of Main. `25 00` / `26 00` name Main in their own first byte ("00:
    /// MAIN 01: SUB", p. 13): the reading no longer depends on the selection, and nothing is
    /// selected to take it. The writes follow in the next test.
    #[test]
    fn an_ic7610_reads_mains_dial_and_mode_by_name_whichever_band_the_panel_selects() {
        let (_e, b, regs) = backend_on(0x98, Some(IcomModel::Ic7610));
        {
            let mut r = regs.lock().unwrap();
            r.main_hz = 14_074_000;
            r.sub_hz = 7_074_000;
            r.main_mode = 0x01; // USB
            r.sub_mode = 0x00; // LSB
        }
        // The Sub selected first: that is where the old reads went wrong.
        for sel_sub in [true, false] {
            regs.lock().unwrap().sel_sub = sel_sub;
            let n = regs.lock().unwrap().wire.len();
            assert_eq!(
                b.freq_hz(),
                14_074_000,
                "f, Sub selected = {sel_sub}: Main's dial (the Sub's is 7.074)"
            );
            assert_eq!(
                b.mode().0,
                "USB",
                "m, Sub selected = {sel_sub}: Main's mode (the Sub's is LSB)"
            );
            let r = regs.lock().unwrap();
            assert_eq!(
                hex_frames(&r.wire[n..]),
                ["FE FE 98 E0 25 00 FD", "FE FE 98 E0 26 00 FD"],
                "Sub selected = {sel_sub}: each read names MAIN, and nothing else goes out"
            );
            assert_eq!(r.sel_sub, sel_sub, "the operator's selection never moved");
        }
    }

    /// ⭐ IC-7610 — A QSY OR A MODE CHANGE FROM NEXUS MOVES MAIN, BY NAME, whatever the panel
    /// selects (operator ruling 2026-09-24, "Write Main by name").
    ///
    /// The dial Nexus reads is Main's (above); the dial it MOVES was the selected band's, because
    /// `05` and `06` act on the selection. With the Sub selected, a QSY moved the Sub while the
    /// reading stayed on Main. `25 00 <freq>` and `26 00 <mode>…` name Main in their own first
    /// byte (A7380-7EX-4 p. 13), so Nexus reads, moves and judges one receiver.
    ///
    /// The mode frame carries what the `06` + `1A 06` pair left on the radio. A plain mode skips
    /// the DATA and filter bytes, which the radio takes as "DATA OFF and the default filter of
    /// the operating mode" (p. 13) — what `06 <mode>` (default filter, p. 10) followed by
    /// `1A 06 00 00` left. A DATA mode carries the operator's D1–D3 and FIL1, the two bytes the
    /// `1A 06` sent.
    #[test]
    fn an_ic7610_writes_mains_dial_and_mode_by_name_whichever_band_the_panel_selects() {
        let (_e, b, regs) = backend_on(0x98, Some(IcomModel::Ic7610));
        {
            let mut r = regs.lock().unwrap();
            r.main_hz = 14_074_000;
            r.sub_hz = 7_074_000;
            r.main_mode = 0x01; // USB
            r.sub_mode = 0x00; // LSB
        }
        // The Sub selected first: that is where the old writes went wrong.
        for (sel_sub, hz, frame) in [
            (true, 14_080_000u64, "FE FE 98 E0 25 00 00 00 08 14 00 FD"),
            (false, 14_085_000, "FE FE 98 E0 25 00 00 50 08 14 00 FD"),
        ] {
            regs.lock().unwrap().sel_sub = sel_sub;
            let n = regs.lock().unwrap().wire.len();
            assert!(b.set_freq(hz), "F, Sub selected = {sel_sub}");
            {
                let r = regs.lock().unwrap();
                assert_eq!(
                    (r.main_hz, r.sub_hz),
                    (hz, 7_074_000),
                    "F, Sub selected = {sel_sub}: MAIN moved, the Sub did not"
                );
                assert_eq!(hex_frames(&r.wire[n..]), [frame], "F on the wire");
                assert_eq!(r.sel_sub, sel_sub, "the operator's selection never moved");
            }
            assert_eq!(b.freq_hz(), hz, "the dial Nexus reads follows its own QSY");

            // Mode: each one MAIN's, each one ONE frame, the Sub's LSB untouched.
            for (mode, main_mode, data_on, frame) in [
                ("CW", 0x03u8, false, "FE FE 98 E0 26 00 03 FD"),
                ("PKTUSB", 0x01, true, "FE FE 98 E0 26 00 01 01 01 FD"),
                ("FM", 0x05, false, "FE FE 98 E0 26 00 05 FD"),
                ("PKTFM", 0x05, true, "FE FE 98 E0 26 00 05 01 01 FD"),
                ("USB", 0x01, false, "FE FE 98 E0 26 00 01 FD"),
            ] {
                let n = regs.lock().unwrap().wire.len();
                assert!(b.set_mode(mode, 0), "M {mode}, Sub selected = {sel_sub}");
                let r = regs.lock().unwrap();
                assert_eq!(hex_frames(&r.wire[n..]), [frame], "M {mode} on the wire");
                assert_eq!(
                    (r.main_mode, r.sub_mode, r.data_mode),
                    (main_mode, 0x00, data_on),
                    "M {mode}, Sub selected = {sel_sub}: MAIN's mode and DATA, the Sub's LSB kept"
                );
                assert_eq!(r.sel_sub, sel_sub, "the operator's selection never moved");
            }
        }
        assert!(
            !regs.lock().unwrap().log.iter().any(|(c, _)| *c == 0x07),
            "a by-name write selects nothing"
        );

        // The operator's DATA mode rides the frame: D2 here, as `1A 06 02 01` carried it.
        let (radio, _push) = FakeRadio::new(0x98);
        let regs2 = radio.regs();
        let engine = CivEngine::start(Box::new(radio), 0x98);
        let b2 = CivBackend::new(
            engine.handle(),
            0x98,
            Arc::new(AtomicBool::new(false)),
            2,
            Some(IcomModel::Ic7610),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while regs2.lock().unwrap().log.is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "engine housekeeping never ran"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let n = regs2.lock().unwrap().wire.len();
        assert!(b2.set_mode("PKTUSB", 0));
        assert_eq!(
            hex_frames(&regs2.lock().unwrap().wire[n..]),
            ["FE FE 98 E0 26 00 01 02 01 FD"],
            "D2 on the wire"
        );
    }

    /// ⛔ THE IC-7610'S SPLIT IS LEFT EXACTLY AS IT WAS. The by-name writes stop at the dial and
    /// the mode. Icom's guide names `25 01` / `26 01` the SUB band (A7380-7EX-4 p. 13) and gives
    /// `0F` split on/off (p. 3), but nowhere says which band a split transmits on — so the split
    /// verbs keep their bytes, including the `25 01` / `26 01` they already send, which on this
    /// radio name the Sub. NEEDS-BENCH.
    #[test]
    fn an_ic7610_split_is_left_exactly_as_it_was() {
        let (_e, b, regs) = backend_on(0x98, Some(IcomModel::Ic7610));
        regs.lock().unwrap().sel_sub = true;
        let n = regs.lock().unwrap().wire.len();
        assert_eq!(b.set_split(true, "VFOB"), Some(true));
        assert_eq!(b.set_split_freq(14_090_000), Some(true));
        assert_eq!(b.set_split_mode("USB", 0), Some(true));
        assert_eq!(b.set_split(false, "VFOA"), Some(true));
        assert_eq!(
            hex_frames(&regs.lock().unwrap().wire[n..]),
            [
                "FE FE 98 E0 16 5A FD",
                "FE FE 98 E0 0F 01 FD",
                "FE FE 98 E0 25 01 00 00 09 14 00 FD",
                "FE FE 98 E0 26 01 01 00 FD",
                "FE FE 98 E0 0F 00 FD",
            ],
            "the IC-7610's split bytes moved"
        );
    }

    /// ⛔ EVERY OTHER RADIO WRITES ITS DIAL AND MODE WITH `05`, `06` AND `1A 06`, BYTE FOR BYTE —
    /// the one-receiver Icoms, a build that does not know its model, and the IC-9700, whose
    /// `25 00` would name its SELECTED VFO (A7508-3EX-4 p. 24). The frames are the ones the tree
    /// before the by-name write (`456fdfdf`) sent, at each radio's own address.
    #[test]
    fn every_other_radio_writes_its_dial_and_mode_exactly_as_before() {
        for (addr, model) in [
            (0x94u8, Some(IcomModel::Ic7300)),
            (0xA4, Some(IcomModel::Ic705)),
            (0xAC, Some(IcomModel::Ic905)),
            (0x94, None),
            (0xA2, Some(IcomModel::Ic9700)),
        ] {
            let (_e, b, regs) = backend_on(addr, model);
            let n = regs.lock().unwrap().wire.len();
            let _ = b.set_freq(14_080_000);
            let _ = b.set_mode("USB", 0);
            let _ = b.set_mode("PKTUSB", 0);
            let _ = b.set_mode("FM", 0);
            assert_eq!(
                hex_frames(&regs.lock().unwrap().wire[n..]),
                [
                    format!("FE FE {addr:02X} E0 05 00 00 08 14 00 FD"),
                    format!("FE FE {addr:02X} E0 06 01 FD"),
                    format!("FE FE {addr:02X} E0 1A 06 00 00 FD"),
                    format!("FE FE {addr:02X} E0 06 01 FD"),
                    format!("FE FE {addr:02X} E0 1A 06 01 01 FD"),
                    format!("FE FE {addr:02X} E0 06 05 FD"),
                    format!("FE FE {addr:02X} E0 1A 06 00 00 FD"),
                ],
                "{model:?}: the dial and mode writes moved"
            );
        }
    }

    /// ⭐ A DIAL READ THAT TIMES OUT SERVES MAIN'S LAST READING — never the engine's cache,
    /// which also folds what the radio pushes, and a push reports the SELECTED band.
    ///
    /// A timeout is one crowded moment, and for [`CIV_CACHE_GRACE`] the last honest reading
    /// stands in for it. On every other radio that reading is the engine's cache: each `03`
    /// reply refreshes it and a transceive push lands in it too, both from the selected band,
    /// so they agree. A `25 00` reply does not fold into it, so on the by-name path the cache
    /// holds only what the radio pushed — with the Sub selected, the Sub's dial. Serving it
    /// would put the selection-following reading back for exactly the moment the bus is busy.
    #[test]
    fn an_ic7610_dial_read_that_times_out_serves_mains_last_reading_not_a_pushed_sub_dial() {
        let (radio, push) = FakeRadio::new(0x98);
        let regs = radio.regs();
        let engine = CivEngine::start(Box::new(radio), 0x98);
        let b = CivBackend::new(
            engine.handle(),
            0x98,
            Arc::new(AtomicBool::new(false)),
            1,
            Some(IcomModel::Ic7610),
        );
        {
            let mut r = regs.lock().unwrap();
            r.main_hz = 14_074_000;
            r.sub_hz = 7_074_000;
            r.main_mode = 0x01; // USB
            r.sub_mode = 0x00; // LSB
            r.sel_sub = true; // the operator is on the Sub band
        }
        assert_eq!(b.freq_hz(), 14_074_000);
        assert_eq!(b.mode().0, "USB");

        // The operator turns the Sub's dial and changes its mode: the radio PUSHES both
        // (transceive `00` / `01`), and the engine folds them into its cache.
        let pushed = |cmd: u8, data: Vec<u8>| {
            crate::civ::frame::Frame {
                to: 0xE0,
                from: 0x98,
                cmd,
                data,
            }
            .to_bytes()
        };
        push.lock().unwrap().extend(pushed(
            0x00,
            crate::civ::frame::freq_to_bcd(7_075_000).to_vec(),
        ));
        push.lock().unwrap().extend(pushed(0x01, vec![0x00, 0x01]));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let st = engine.handle().state();
            // ⭐ THE CONTROL: the cache really holds the Sub's reading — this is what a
            // fallback to it would serve below.
            if st.freq_hz == Some(7_075_000) && st.mode == Some(Mode::Lsb) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the pushes never reached the engine's cache: {st:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }

        // The next dial read and mode read are both lost on the wire.
        regs.lock().unwrap().drop_dial_reads = 2;
        assert_eq!(
            b.freq_hz(),
            14_074_000,
            "a timed-out f serves Main's last reading, not the Sub's pushed 7.075"
        );
        assert_eq!(
            b.mode().0,
            "USB",
            "a lost m serves Main's last mode, not the Sub's pushed LSB"
        );
        assert_eq!(
            regs.lock().unwrap().drop_dial_reads,
            0,
            "both reads were lost"
        );
    }

    /// ⛔ EVERY OTHER RADIO READS ITS DIAL AND MODE WITH `03` AND `04`, BYTE FOR BYTE.
    ///
    /// The one-receiver Icoms, a build that does not know its model, and the IC-9700 — whose
    /// `25 00` names the SELECTED VFO (A7508-3EX-4 p. 24) and so would fix nothing. The frames
    /// are the ones the tree before the by-name read (`bd585821`) sent: the same two at each
    /// radio's own address.
    #[test]
    fn every_other_radio_reads_its_dial_and_mode_with_03_and_04_exactly_as_before() {
        for (addr, model) in [
            (0x94u8, Some(IcomModel::Ic7300)),
            (0xA4, Some(IcomModel::Ic705)),
            (0xAC, Some(IcomModel::Ic905)),
            (0x94, None),
            (0xA2, Some(IcomModel::Ic9700)),
        ] {
            let (_e, b, regs) = backend_on(addr, model);
            let n = regs.lock().unwrap().wire.len();
            let _ = b.freq_hz();
            let _ = b.mode();
            assert_eq!(
                hex_frames(&regs.lock().unwrap().wire[n..]),
                [
                    format!("FE FE {addr:02X} E0 03 FD"),
                    format!("FE FE {addr:02X} E0 04 FD")
                ],
                "{model:?}: the dial and mode reads moved"
            );
        }
    }

    /// ⭐ IC-9700 — A SELECTION STRANDED ON SUB IS PUT BACK ON MAIN BEFORE ANY OF THE 17.
    ///
    /// The IC-9700 has no band-directed form (its reference, A7508-3EX-4, has no command
    /// `29`), so the broker HOLDS the selection, exactly as the dial verbs already did: the
    /// band lock, and `ensure_main` repairing a Main restore the rig refused. Before this
    /// step only the dial verbs did — after a refused restore mid-pass, an S-meter poll, an
    /// NB toggle, a RIT change or a CTCSS tone all acted on the SUB band (the uplink) and
    /// the reading was reported as the downlink's.
    #[test]
    fn an_ic9700_repairs_a_stranded_selection_before_every_receiver_verb() {
        let (_e, b, regs) = backend_on(0xA2, Some(IcomModel::Ic9700));
        plant_two_receivers(&regs);
        assert!(b.set_freq(435_640_000));
        assert_eq!(
            b.set_split(true, "Sub"),
            Some(true),
            "scene: a satellite pass"
        );

        for (name, main) in [
            ("STRENGTH", "0"),
            ("AF", "0.78"),
            ("RF", "0.71"),
            ("SQL", "0.08"),
            ("NR", "0.59"),
            ("NB", "0.35"),
            ("AGC", "2"),
            ("ATT", "12"),
            ("PREAMP", "1"),
        ] {
            strand_on_sub(&b, &regs);
            assert_eq!(
                b.level(name).as_deref(),
                Some(main),
                "l {name}: Main's reading"
            );
            assert!(
                !regs.lock().unwrap().sel_sub,
                "l {name}: selection back on Main"
            );
        }
        for (token, main) in [("NB", true), ("NR", false), ("ANF", true), ("MN", false)] {
            strand_on_sub(&b, &regs);
            assert_eq!(b.func(token), Some(main), "u {token}: Main's state");
            assert!(
                !regs.lock().unwrap().sel_sub,
                "u {token}: selection back on Main"
            );
        }

        // WRITES, the TX-side configuration verbs included: each lands on Main — the
        // receiver that transmits outside a satellite pass (IC-9700 Basic Manual, "you can
        // transmit on only the Main band"). The witness is where the fake saw it LAND.
        type Verb = fn(&CivBackend);
        let writes: [(&str, Verb, u8, Option<u8>); 18] = [
            (
                "L AF",
                |b| {
                    let _ = b.set_level("AF", "0.50");
                },
                0x14,
                Some(0x01),
            ),
            (
                "L RF",
                |b| {
                    let _ = b.set_level("RF", "0.50");
                },
                0x14,
                Some(0x02),
            ),
            (
                "L SQL",
                |b| {
                    let _ = b.set_level("SQL", "0.50");
                },
                0x14,
                Some(0x03),
            ),
            (
                "L NR",
                |b| {
                    let _ = b.set_level("NR", "0.50");
                },
                0x14,
                Some(0x06),
            ),
            (
                "L NB",
                |b| {
                    let _ = b.set_level("NB", "0.50");
                },
                0x14,
                Some(0x12),
            ),
            (
                "L AGC",
                |b| {
                    let _ = b.set_level("AGC", "3");
                },
                0x16,
                Some(0x12),
            ),
            (
                "L ATT",
                |b| {
                    let _ = b.set_level("ATT", "10");
                },
                0x11,
                None,
            ),
            (
                "L PREAMP",
                |b| {
                    let _ = b.set_level("PREAMP", "2");
                },
                0x16,
                Some(0x02),
            ),
            (
                "U NB",
                |b| {
                    let _ = b.set_func("NB", false);
                },
                0x16,
                Some(0x22),
            ),
            (
                "U NR",
                |b| {
                    let _ = b.set_func("NR", true);
                },
                0x16,
                Some(0x40),
            ),
            (
                "U ANF",
                |b| {
                    let _ = b.set_func("ANF", false);
                },
                0x16,
                Some(0x41),
            ),
            (
                "U MN",
                |b| {
                    let _ = b.set_func("MN", true);
                },
                0x16,
                Some(0x48),
            ),
            (
                "U RIT",
                |b| {
                    let _ = b.set_func("RIT", true);
                },
                0x21,
                Some(0x01),
            ),
            (
                "J",
                |b| {
                    let _ = b.set_rit(120);
                },
                0x21,
                Some(0x00),
            ),
            (
                "V VFOB",
                |b| {
                    let _ = b.set_vfo("VFOB");
                },
                0x07,
                Some(0x01),
            ),
            (
                "R",
                |b| {
                    let _ = b.set_rptr_shift("-");
                },
                0x0F,
                Some(0x11),
            ),
            (
                "O",
                |b| {
                    let _ = b.set_rptr_offset(600_000);
                },
                0x0D,
                None,
            ),
            (
                "C",
                |b| {
                    let _ = b.set_ctcss(885);
                },
                0x1B,
                Some(0x00),
            ),
        ];
        for (what, verb, cmd, first) in writes {
            strand_on_sub(&b, &regs);
            verb(&b);
            assert!(
                !last_acted_on_sub(&regs, cmd, first),
                "{what} landed on Main"
            );
            assert!(
                !regs.lock().unwrap().sel_sub,
                "{what}: selection back on Main"
            );
        }
        assert!(
            !last_acted_on_sub(&regs, 0x16, Some(0x42)),
            "C: the tone switch landed on Main too"
        );

        // ⛔ AND NO XIT AT ALL. The IC-9700 has no ΔTX (A7508-3EX-4 lists `21 00` and `21 01`,
        // no `21 02`), so `Z` and `U XIT` are refused before anything reaches the bus — no
        // select, no repair, no `21` frame — whichever receiver is named and wherever the
        // selection sits. `Z` used to ride the held selection onto Main's `21 00`, which on
        // this radio is the RIT offset.
        strand_on_sub(&b, &regs);
        let n = regs.lock().unwrap().log.len();
        assert_eq!(b.set_xit(250), None, "Z: refused, RPRT -11");
        assert_eq!(b.set_func("XIT", true), None, "U XIT: refused, RPRT -11");
        assert_eq!(
            b.set_func_on(ReceiverId::Sub, "XIT", true),
            None,
            "U XIT named on the Sub: refused"
        );
        assert_eq!(
            regs.lock().unwrap().log.len(),
            n,
            "an XIT put a frame on an IC-9700's bus"
        );
        // POSITIVE CONTROL: RIT rides the same register and still reaches the radio, on Main,
        // after the stranded selection is repaired — so the log above could see a frame.
        assert_eq!(b.set_rit(120), Some(true), "J still works");
        assert!(regs.lock().unwrap().log.len() > n, "J reached the bus");
        assert!(
            !last_acted_on_sub(&regs, 0x21, Some(0x00)),
            "J landed on Main"
        );
    }

    /// ⭐ THE BAND LOCK HOLDS FOR THE RECEIVE VERBS, not just the dial.
    ///
    /// The daemon serves one backend to a thread per client. During a pass the uplink is
    /// written select-Sub → write → verify → select-Main; a receive read that lands inside
    /// that window reads the SUB band and reports it as Main's. Before this step only the
    /// dial was held off by the lock — the other 17 verbs walked straight in.
    #[test]
    fn a_receive_read_never_lands_inside_another_threads_sub_window() {
        let (_e, b, regs) = backend_on(0xA2, Some(IcomModel::Ic9700));
        plant_two_receivers(&regs);
        assert!(b.set_freq(435_640_000));
        assert_eq!(b.set_split(true, "Sub"), Some(true));

        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (b, stop) = (b.clone(), stop.clone());
            std::thread::spawn(move || {
                let (mut reads, mut wrong) = (0u32, Vec::new());
                while !stop.load(Ordering::Relaxed) {
                    match b.level("AF") {
                        Some(v) if v != "0.78" => wrong.push(v),
                        Some(_) => reads += 1,
                        None => {}
                    }
                }
                (reads, wrong)
            })
        };
        for _ in 0..25 {
            assert_eq!(b.set_split_freq(145_965_000), Some(true));
            // A breath between uplink writes, as a Doppler steer has: `std`'s mutex is not
            // fair, and a writer re-taking the lock back-to-back starves the reader — which
            // would make "no wrong read" true of a reader that barely read.
            std::thread::sleep(Duration::from_millis(3));
        }
        stop.store(true, Ordering::Relaxed);
        let (reads, wrong) = reader.join().unwrap();
        assert!(
            wrong.is_empty(),
            "a Main AF read served the SUB's value {} times: {wrong:?}",
            wrong.len()
        );
        // CONTROL: the reader really did read, over and over — an idle reader would pass.
        assert!(reads >= 10, "only {reads} reads raced the uplink writes");
        let landed_on_sub = regs
            .lock()
            .unwrap()
            .acted
            .iter()
            .filter(|(s, c, d)| *s && *c == 0x14 && d.first() == Some(&0x01))
            .count();
        assert_eq!(
            landed_on_sub, 0,
            "no AF read ever arrived while Sub was selected"
        );
    }

    /// ⛔ AN UNKEY NEVER WAITS ON THE BAND LOCK. PTT-off and CW-stop are what a stuck
    /// transmitter's release rests on: they name no receiver and take no lock, so a selection
    /// sequence that is slow, wedged or merely in progress cannot hold the carrier up.
    #[test]
    fn an_unkey_never_waits_on_the_band_lock() {
        for (addr, model) in [(0xA2, IcomModel::Ic9700), (0x98, IcomModel::Ic7610)] {
            let (_e, b, regs) = backend_on(addr, Some(model));
            let held = b.band(); // a selection sequence in progress, frozen in place

            let (tx, rx) = std::sync::mpsc::channel();
            {
                let b = b.clone();
                std::thread::spawn(move || {
                    let t0 = std::time::Instant::now();
                    let _ = b.set_ptt(false);
                    let _ = b.stop_morse();
                    let _ = tx.send(t0.elapsed());
                });
            }
            let took = rx
                .recv_timeout(Duration::from_millis(500))
                .unwrap_or_else(|_| {
                    panic!("{model:?}: PTT-off + CW-stop blocked behind the band lock")
                });

            // POSITIVE CONTROL: the same held guard DOES stop a verb that takes the lock, or
            // this test could not have seen a blocked unkey at all.
            let (ctx, crx) = std::sync::mpsc::channel();
            {
                let b = b.clone();
                std::thread::spawn(move || {
                    let _ = b.freq_hz();
                    let _ = ctx.send(());
                });
            }
            assert!(
                crx.recv_timeout(Duration::from_millis(500)).is_err(),
                "{model:?} control: the dial read waits on the held band lock"
            );
            drop(held);
            crx.recv_timeout(Duration::from_secs(3))
                .expect("…and proceeds the moment it is released");

            let r = regs.lock().unwrap();
            assert!(
                r.log.contains(&(0x1C, vec![0x00, 0x00])),
                "{model:?}: PTT-off on the wire"
            );
            assert!(
                r.log.contains(&(0x17, vec![0xFF])),
                "{model:?}: CW-stop on the wire"
            );
            assert!(took < Duration::from_millis(500), "{model:?}: {took:?}");
        }
    }

    /// ⛔ KEY-ON TIMING IS UNCHANGED — nothing new rides between the go and PTT-on.
    ///
    /// The tempting "receiver fix" re-asserts Main before keying, so the carrier leaves on the
    /// right band. It would put a selection round trip — and, with the selection stranded, a
    /// dial re-read — between the slot boundary and the carrier. PTT names no receiver: the
    /// RIG decides which one transmits (Main; the uplink, Sub, in satellite mode).
    #[test]
    fn nothing_new_rides_between_the_go_and_ptt_on() {
        // IC-9700 with the selection STRANDED on Sub: the one state in which a receiver-
        // correcting PTT would have had something to "repair" first.
        let (_e, b, regs) = backend_on(0xA2, Some(IcomModel::Ic9700));
        assert!(b.set_freq(435_640_000));
        assert_eq!(b.set_split(true, "Sub"), Some(true));
        strand_on_sub(&b, &regs);
        let sent_by = |f: &dyn Fn()| {
            let n = regs.lock().unwrap().log.len();
            f();
            regs.lock().unwrap().log[n..].to_vec()
        };
        assert_eq!(
            sent_by(&|| {
                let _ = b.set_ptt(true);
            }),
            vec![(0x1C, vec![0x00, 0x01])],
            "PTT-on is ONE frame"
        );
        assert_eq!(
            sent_by(&|| {
                let _ = b.send_morse("CQ");
            }),
            vec![(0x17, b"CQ".to_vec())],
            "CAT CW keys with ONE frame"
        );
        assert_eq!(
            sent_by(&|| {
                let _ = b.set_ptt(false);
            }),
            vec![(0x1C, vec![0x00, 0x00])]
        );
        assert!(
            regs.lock().unwrap().sel_sub,
            "none of them touched the selection"
        );

        // IC-7610 — `1C` and `17` carry no band-directed mark (A7380-7EX-4 pp. 4, 9): bare.
        let (_e2, b2, regs2) = backend_on(0x98, Some(IcomModel::Ic7610));
        regs2.lock().unwrap().sel_sub = true;
        let n = regs2.lock().unwrap().log.len();
        let _ = b2.set_ptt(true);
        let _ = b2.send_morse("CQ");
        let _ = b2.stop_morse();
        let _ = b2.set_ptt(false);
        assert_eq!(
            regs2.lock().unwrap().log[n..].to_vec(),
            vec![
                (0x1C, vec![0x00, 0x01]),
                (0x17, b"CQ".to_vec()),
                (0x17, vec![0xFF]),
                (0x1C, vec![0x00, 0x00]),
            ]
        );
    }

    /// The script that drives EVERY verb this step touched, in a fixed order with fixed
    /// arguments — the input to the byte-identity test below. The attenuator and preamp are
    /// driven at the rig's own first step, so the script is legal on every model.
    fn drive_every_touched_verb(b: &CivBackend) {
        for name in [
            "STRENGTH",
            "AF",
            "RF",
            "SQL",
            "NR",
            "NB",
            "AGC",
            "ATT",
            "PREAMP",
            "RFPOWER",
            "MICGAIN",
            "COMP",
            "MONITOR_GAIN",
            "SWR",
            "ALC",
            "RFPOWER_METER_WATTS",
            "COMP_METER",
        ] {
            let _ = b.level(name);
        }
        let first = |v: Vec<u8>| v.first().map_or_else(|| "0".to_string(), u8::to_string);
        let (att, pre) = (first(b.attenuator_steps_db()), first(b.preamp_steps_db()));
        for (name, value) in [
            ("AF", "0.25"),
            ("RF", "0.50"),
            ("SQL", "0.10"),
            ("NR", "0.60"),
            ("NB", "0.40"),
            ("AGC", "2"),
            ("ATT", att.as_str()),
            ("PREAMP", pre.as_str()),
            ("RFPOWER", "0.75"),
            ("MICGAIN", "0.30"),
            ("KEYSPD", "25"),
            ("COMP", "0.20"),
            ("MONITOR_GAIN", "0.50"),
        ] {
            let _ = b.set_level(name, value);
        }
        for token in ["NB", "NR", "ANF", "MN", "COMP", "MON", "VOX", "SATMODE"] {
            let _ = b.func(token);
        }
        for (token, on) in [
            ("NB", true),
            ("NR", false),
            ("ANF", true),
            ("MN", false),
            ("COMP", true),
            ("MON", false),
            ("VOX", true),
            ("RIT", true),
            ("XIT", false),
        ] {
            let _ = b.set_func(token, on);
        }
        let _ = b.set_rit(120);
        let _ = b.set_rit(-50);
        // Transmit side.
        let _ = b.owner_transmitting();
        let _ = b.ptt();
        let _ = b.set_ptt(true);
        let _ = b.send_morse("CQ TEST CQ TEST THIS MESSAGE IS LONGER THAN ONE CHUNK");
        let _ = b.stop_morse();
        let _ = b.set_ptt(false);
        let _ = b.set_xit(250);
        let _ = b.set_xit(0);
        for shift in ["+", "-", "None"] {
            let _ = b.set_rptr_shift(shift);
        }
        let _ = b.set_rptr_offset(600_000);
        let _ = b.set_ctcss(885);
        let _ = b.set_ctcss(0);
        // The selection itself, last — it moves the fake's band.
        for vfo in ["VFOA", "VFOB", "Sub", "Main"] {
            let _ = b.set_vfo(vfo);
        }
    }

    fn hex_frames(wire: &[Vec<u8>]) -> Vec<String> {
        wire.iter()
            .map(|f| {
                f.iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    /// ⭐ THE BYTES AN IC-7300 PUT ON THE WIRE BEFORE THIS STEP, for every verb it touched.
    ///
    /// Captured by running [`drive_every_touched_verb`] against the tree this step started
    /// from (`046d493d`), and pasted here verbatim. The first frame is the engine's own
    /// scope-off housekeeping (`27 11 00`), sent before any verb.
    const IC7300_WIRE_BEFORE: &[&str] = &[
        "FE FE 94 E0 27 11 00 FD",
        "FE FE 94 E0 15 02 FD",
        "FE FE 94 E0 14 01 FD",
        "FE FE 94 E0 14 02 FD",
        "FE FE 94 E0 14 03 FD",
        "FE FE 94 E0 14 06 FD",
        "FE FE 94 E0 14 12 FD",
        "FE FE 94 E0 16 12 FD",
        "FE FE 94 E0 11 FD",
        "FE FE 94 E0 16 02 FD",
        "FE FE 94 E0 14 0A FD",
        "FE FE 94 E0 14 0B FD",
        "FE FE 94 E0 14 0E FD",
        "FE FE 94 E0 14 15 FD",
        "FE FE 94 E0 15 12 FD",
        "FE FE 94 E0 15 13 FD",
        "FE FE 94 E0 15 11 FD",
        "FE FE 94 E0 15 14 FD",
        "FE FE 94 E0 14 01 00 63 FD",
        "FE FE 94 E0 14 02 01 27 FD",
        "FE FE 94 E0 14 03 00 25 FD",
        "FE FE 94 E0 14 06 01 53 FD",
        "FE FE 94 E0 14 12 01 02 FD",
        "FE FE 94 E0 16 12 01 FD",
        "FE FE 94 E0 11 20 FD",
        "FE FE 94 E0 16 02 01 FD",
        "FE FE 94 E0 14 0A 01 91 FD",
        "FE FE 94 E0 14 0B 00 76 FD",
        "FE FE 94 E0 14 0C 01 15 FD",
        "FE FE 94 E0 14 0E 00 51 FD",
        "FE FE 94 E0 14 15 01 27 FD",
        "FE FE 94 E0 16 22 FD",
        "FE FE 94 E0 16 40 FD",
        "FE FE 94 E0 16 41 FD",
        "FE FE 94 E0 16 48 FD",
        "FE FE 94 E0 16 44 FD",
        "FE FE 94 E0 16 45 FD",
        "FE FE 94 E0 16 46 FD",
        "FE FE 94 E0 16 5A FD",
        "FE FE 94 E0 16 22 01 FD",
        "FE FE 94 E0 16 40 00 FD",
        "FE FE 94 E0 16 41 01 FD",
        "FE FE 94 E0 16 48 00 FD",
        "FE FE 94 E0 16 44 01 FD",
        "FE FE 94 E0 16 45 00 FD",
        "FE FE 94 E0 16 46 01 FD",
        "FE FE 94 E0 21 01 01 FD",
        "FE FE 94 E0 21 02 00 FD",
        "FE FE 94 E0 21 00 20 01 00 FD",
        "FE FE 94 E0 21 00 50 00 01 FD",
        "FE FE 94 E0 1C 00 FD",
        "FE FE 94 E0 1C 00 01 FD",
        "FE FE 94 E0 17 43 51 20 54 45 53 54 20 43 51 20 54 45 53 54 20 54 48 49 53 20 4D 45 53 53 41 47 45 20 49 FD",
        "FE FE 94 E0 17 53 20 4C 4F 4E 47 45 52 20 54 48 41 4E 20 4F 4E 45 20 43 48 55 4E 4B FD",
        "FE FE 94 E0 17 FF FD",
        "FE FE 94 E0 1C 00 00 FD",
        "FE FE 94 E0 21 00 50 02 00 FD",
        "FE FE 94 E0 21 00 00 00 00 FD",
        "FE FE 94 E0 0F 12 FD",
        "FE FE 94 E0 0F 11 FD",
        "FE FE 94 E0 0F 10 FD",
        "FE FE 94 E0 0D 00 60 00 FD",
        "FE FE 94 E0 1B 00 08 85 FD",
        "FE FE 94 E0 16 42 01 FD",
        "FE FE 94 E0 16 42 00 FD",
        "FE FE 94 E0 07 00 FD",
        "FE FE 94 E0 07 01 FD",
        "FE FE 94 E0 07 D1 FD",
        "FE FE 94 E0 07 D0 FD",
    ];

    /// ⛔ SINGLE-RECEIVER RIGS SEE BYTE-IDENTICAL CI-V. The IC-7300 and IC-705 have one
    /// receiver; the IC-905 and a build with no model are not offered a Sub either (the
    /// capability table has no vendor statement for them — UNKNOWN, which is not a "no",
    /// and not an offer). None of them may see one new, moved or re-worded byte.
    #[test]
    fn single_receiver_rigs_see_byte_identical_ci_v_across_every_touched_verb() {
        let (_e, b, regs) = backend_on(0x94, Some(IcomModel::Ic7300));
        regs.lock().unwrap().no_satmode = true; // a real 7300 NAKs `16 5A`
        drive_every_touched_verb(&b);
        let got = hex_frames(&regs.lock().unwrap().wire);
        assert_eq!(got, IC7300_WIRE_BEFORE, "IC-7300 bytes moved");
        assert_eq!(b.attenuator_steps_db(), vec![20]);
        assert_eq!(b.preamp_steps_db(), vec![1, 2]);

        // IC-705: the same rig family and step lists — the same frames at its own address.
        let at = |addr: u8, frames: &[&str]| -> Vec<String> {
            frames
                .iter()
                .map(|f| {
                    let mut bytes: Vec<String> = f.split(' ').map(str::to_string).collect();
                    bytes[2] = format!("{addr:02X}");
                    bytes.join(" ")
                })
                .collect()
        };
        let (_e, b, regs) = backend_on(0xA4, Some(IcomModel::Ic705));
        regs.lock().unwrap().no_satmode = true;
        drive_every_touched_verb(&b);
        assert_eq!(
            hex_frames(&regs.lock().unwrap().wire),
            at(0xA4, IC7300_WIRE_BEFORE),
            "IC-705 bytes moved"
        );

        // IC-905 and an unknown model: no step list, so no attenuator/preamp frame at all
        // (see `an_unknown_model_offers_no_attenuator_or_preamp_at_all`) — every other
        // frame exactly as the 7300's.
        let no_pads: Vec<&str> = IC7300_WIRE_BEFORE
            .iter()
            .copied()
            .filter(|f| {
                let b: Vec<&str> = f.split(' ').collect();
                !(b[4] == "11" || (b[4] == "16" && b[5] == "02"))
            })
            .collect();
        for (addr, model) in [(0xACu8, Some(IcomModel::Ic905)), (0x94, None)] {
            let (_e, b, regs) = backend_on(addr, model);
            regs.lock().unwrap().no_satmode = true;
            drive_every_touched_verb(&b);
            assert_eq!(
                hex_frames(&regs.lock().unwrap().wire),
                at(addr, no_pads.as_slice()),
                "{model:?} bytes moved"
            );
        }

        // ⚠️ POSITIVE CONTROL — the capture can SEE a routing change. The same script on the
        // IC-7610, whose receive verbs now name Main by command `29`, must put band-directed
        // frames on the wire. Were this to find none, a byte comparison that "passed" above
        // would prove nothing about routing at all.
        let (_e, b, regs) = backend_on(0x98, Some(IcomModel::Ic7610));
        drive_every_touched_verb(&b);
        let directed = regs
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|(c, d)| *c == 0x29 && d.first() == Some(&0x00))
            .count();
        assert!(
            directed > 0,
            "the IC-7610 run carried no `29 00` frame at all"
        );
    }

    /// ⭐ A CALLER CAN NAME THE SUB — and the selection comes back where it was.
    ///
    /// No rigctld client can name a receiver yet (the daemon answers `\chk_vfo` with 0, so no
    /// verb carries a VFO argument); the engine step of this programme is the first caller.
    /// This pins what that caller will get: on the IC-7610 `29 01` and no select at all; on
    /// the IC-9700 select Sub → command → select Main, under the band lock.
    #[test]
    fn a_sub_named_command_reaches_the_sub_and_hands_the_selection_back() {
        // IC-7610, Main selected at the panel: the Sub is named on the wire.
        let (_e, b, regs) = backend_on(0x98, Some(IcomModel::Ic7610));
        plant_two_receivers(&regs);
        assert_eq!(b.level_on(ReceiverId::Sub, "AF").as_deref(), Some("0.20"));
        assert_eq!(
            b.level_on(ReceiverId::Sub, "STRENGTH").as_deref(),
            Some("-27")
        );
        assert_eq!(b.func_on(ReceiverId::Sub, "NR"), Some(true));
        assert_eq!(b.set_level_on(ReceiverId::Sub, "AF", "0.50"), Some(true));
        assert_eq!(b.set_ctcss_on(ReceiverId::Sub, 885), Some(true));
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.sub_levels.get(&0x01), Some(&127), "Sub's AF moved");
            assert_eq!(r.levels.get(&0x01), Some(&200), "Main's AF did not");
            assert!(
                r.log.contains(&(0x29, vec![0x01, 0x14, 0x01])),
                "`29 01 14 01` on the wire"
            );
            assert!(
                !r.log.iter().any(|(c, _)| *c == 0x07),
                "no select at all: {:02x?}",
                r.log
            );
            assert!(!r.sel_sub, "Main still selected");
        }
        assert!(
            last_acted_on_sub(&regs, 0x1B, Some(0x00)),
            "the tone went to the Sub"
        );

        // IC-9700: no band-directed form, so the Sub is SELECTED for the command — and the
        // selection handed back to Main, where the broker keeps it.
        let (_e, b, regs) = backend_on(0xA2, Some(IcomModel::Ic9700));
        plant_two_receivers(&regs);
        let n = regs.lock().unwrap().log.len();
        assert_eq!(b.level_on(ReceiverId::Sub, "AF").as_deref(), Some("0.20"));
        {
            let r = regs.lock().unwrap();
            assert!(!r.sel_sub, "the selection was handed back to Main");
            let sent: Vec<_> = r.log[n..].iter().map(|(c, d)| (*c, d.clone())).collect();
            assert_eq!(
                sent,
                vec![
                    (0x07, vec![0xD1]),
                    (0x14, vec![0x01]),
                    (0x07, vec![0xD0]),
                    (0x03, vec![]), // the dial re-read after the restore
                ],
                "select Sub, the read, select Main — in that order"
            );
        }
        assert!(
            last_acted_on_sub(&regs, 0x14, Some(0x01)),
            "the AF read happened on Sub"
        );

        // ⚠️ A REFUSED RESTORE is a failed exchange, and it is REMEMBERED: the next Main
        // command re-asserts Main first instead of reading the Sub as Main's.
        regs.lock().unwrap().nak_main_select = 1;
        assert_eq!(
            b.level_on(ReceiverId::Sub, "AF"),
            None,
            "restore refused → no reading"
        );
        assert!(regs.lock().unwrap().sel_sub, "scene: stranded on Sub");
        assert_eq!(
            b.level("AF").as_deref(),
            Some("0.78"),
            "the next Main read is Main's"
        );
        assert!(
            !regs.lock().unwrap().sel_sub,
            "…because it put the selection back first"
        );
    }

    /// ⭐ THE PROTOCOL DOOR: `L Sub <level> <value>` through the daemon's own dispatcher reaches
    /// the Sub and only the Sub. This is the path the cockpit's Sub controls ride (Nexus's own
    /// `Rig` client speaks it), so it is pinned end to end here: text in, bytes on the wire,
    /// which band's register moved.
    #[test]
    fn a_sub_level_by_protocol_lands_on_the_sub_and_nowhere_else() {
        use crate::rigctld_server::{handle_command, Handled};
        let send = |b: &CivBackend, line: &str| match handle_command(line, b) {
            Handled::Reply(r) => r,
            Handled::Close => panic!("{line}: closed"),
        };
        // IC-7610: `29 01` names the Sub on the wire; nothing is selected.
        let (_e, b, regs) = backend_on(0x98, Some(IcomModel::Ic7610));
        plant_two_receivers(&regs);
        assert_eq!(send(&b, "L Sub RF 0.500"), "RPRT 0\n");
        assert_eq!(send(&b, "L Sub AF 0.250"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.sub_levels.get(&0x02), Some(&127), "the Sub's RF moved");
            assert_eq!(r.sub_levels.get(&0x01), Some(&63), "the Sub's AF moved");
            assert_eq!(r.levels.get(&0x02), Some(&180), "Main's RF did not");
            assert_eq!(r.levels.get(&0x01), Some(&200), "Main's AF did not");
            assert!(!r.log.iter().any(|(c, _)| *c == 0x07), "no select at all");
            assert!(!r.sel_sub);
        }
        // IC-9700: no band-directed form — the Sub is selected for the write and handed back.
        let (_e, b, regs) = backend_on(0xA2, Some(IcomModel::Ic9700));
        plant_two_receivers(&regs);
        assert_eq!(send(&b, "L Sub RF 0.500"), "RPRT 0\n");
        {
            let r = regs.lock().unwrap();
            assert_eq!(r.sub_levels.get(&0x02), Some(&127), "the Sub's RF moved");
            assert_eq!(r.levels.get(&0x02), Some(&180), "Main's RF did not");
            assert!(!r.sel_sub, "the selection was handed back to Main");
        }
        assert!(
            last_acted_on_sub(&regs, 0x14, Some(0x02)),
            "the RF write acted on Sub"
        );
        // CONTROL: the unqualified verb on the same radio is Main's, as it always was.
        assert_eq!(send(&b, "L RF 0.250"), "RPRT 0\n");
        assert_eq!(
            regs.lock().unwrap().levels.get(&0x02),
            Some(&63),
            "Main's RF"
        );
        assert_eq!(
            regs.lock().unwrap().sub_levels.get(&0x02),
            Some(&127),
            "Sub untouched"
        );

        // One receiver: there is no Sub to name. Refused, and nothing reaches the radio.
        let (_e, b, regs) = backend_on(0x94, Some(IcomModel::Ic7300));
        let n = regs.lock().unwrap().log.len();
        assert_eq!(send(&b, "L Sub AF 0.500"), "RPRT -1\n");
        assert_eq!(regs.lock().unwrap().log.len(), n, "IC-7300: nothing sent");
    }

    /// The daemon says whether it can name the Sub, from the model's addressing — true exactly
    /// where the capability table offers one and the daemon serves the radio.
    #[test]
    fn the_daemon_says_whether_it_can_name_the_sub() {
        for (addr, model, names) in [
            (0x98u8, IcomModel::Ic7610, true),
            (0xA2, IcomModel::Ic9700, true),
            (0x94, IcomModel::Ic7300, false),
        ] {
            let (radio, _push) = FakeRadio::new(addr);
            let d = CivDaemon::start_with_io(Box::new(radio), addr, 0, 1, Some(model)).unwrap();
            assert_eq!(d.names_receivers(), names, "{model:?}");
        }
    }

    /// The transmitter's levels are the RADIO's: a Sub has none of its own to report or to
    /// set, and asking puts nothing on the wire. A single-receiver radio has no Sub at all.
    #[test]
    fn a_sub_named_command_with_nothing_to_name_sends_nothing() {
        for (addr, model) in [
            (0x98, Some(IcomModel::Ic7610)),
            (0xA2, Some(IcomModel::Ic9700)),
        ] {
            let (_e, b, regs) = backend_on(addr, model);
            let n = regs.lock().unwrap().log.len();
            assert_eq!(b.level_on(ReceiverId::Sub, "RFPOWER"), None);
            assert_eq!(b.level_on(ReceiverId::Sub, "SWR"), None);
            assert_eq!(b.set_level_on(ReceiverId::Sub, "MICGAIN", "0.5"), None);
            assert_eq!(b.func_on(ReceiverId::Sub, "VOX"), None);
            assert_eq!(b.set_func_on(ReceiverId::Sub, "COMP", true), None);
            assert_eq!(regs.lock().unwrap().log.len(), n, "{model:?}: nothing sent");
            // CONTROL: the same verbs asked of Main do reach the radio.
            let _ = b.level_on(ReceiverId::Main, "RFPOWER");
            assert!(
                regs.lock().unwrap().log.len() > n,
                "{model:?}: Main's power was asked"
            );
        }
        // One receiver: there is no Sub to name, whatever the verb.
        let (_e, b, regs) = backend_on(0x94, Some(IcomModel::Ic7300));
        let n = regs.lock().unwrap().log.len();
        assert_eq!(b.level_on(ReceiverId::Sub, "AF"), None);
        assert_eq!(b.set_level_on(ReceiverId::Sub, "AF", "0.5"), Some(false));
        assert_eq!(b.set_ctcss_on(ReceiverId::Sub, 885), Some(false));
        assert!(!b.set_vfo_on(ReceiverId::Sub, "VFOB"));
        assert_eq!(regs.lock().unwrap().log.len(), n, "IC-7300: nothing sent");
    }

    /// The lines Nexus's own `Rig::set_xit` sends (`U XIT n`, then `Z <hz>`): both signs, and
    /// a clear.
    const XIT_LINES: [&str; 5] = ["U XIT 1\n", "Z 500\n", "Z -1234\n", "U XIT 0\n", "Z 0\n"];

    /// Drive [`XIT_LINES`] through a native daemon for `model`, then `J 500` (RIT) as the
    /// positive control: RIT rides the same `21` command, so its frame proves the bus log
    /// can see one. Returns the reply to each XIT line and the payload of every `21` frame
    /// the radio received, in order.
    ///
    /// One connection per line, because the server hangs up on a peer after three `RPRT -11`
    /// in a row (`MAX_CONSECUTIVE_UNKNOWN`, its defence against a web page posting to the
    /// port). Nexus's own `Rig::set_xit` stops at the first refusal, so it never sends more.
    fn xit_then_rit(addr: u8, model: IcomModel) -> (Vec<String>, Vec<Vec<u8>>) {
        let (radio, _push) = FakeRadio::new(addr);
        let regs = radio.regs();
        let daemon = CivDaemon::start_with_io(Box::new(radio), addr, 0, 1, Some(model)).unwrap();
        let port = daemon.local_addr().port();
        let send = |line: &str| {
            let (mut c, mut rd) = client(port);
            roundtrip(&mut c, &mut rd, line)
        };
        let replies = XIT_LINES.iter().map(|l| send(l)).collect();
        let _ = send("J 500\n");
        let on_21 = regs
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|(cmd, _)| *cmd == 0x21)
            .map(|(_, d)| d.clone())
            .collect();
        (replies, on_21)
    }

    /// ⛔ THE IC-9700 HAS NO ΔTX, SO NOTHING AN XIT ASKS FOR MAY REACH ITS `21` REGISTERS.
    ///
    /// Icom's CI-V reference for the 9700 (A7508-3EX-4) has `21 00` (RIT frequency) and
    /// `21 01` (RIT on/off), and no `21 02`. The daemon sent XIT the IC-7610's way regardless:
    /// `U XIT 1` became `21 02 01`, a command this radio does not have, and `Z` became
    /// `21 00`, which on a 9700 IS the RIT offset, so an XIT rewrote the receiver's clarifier.
    /// Hamlib refuses both verbs for this model with `RPRT -11`, and so does the daemon.
    #[test]
    fn an_ic9700_puts_no_xit_on_the_wire_because_its_21_00_is_the_rit_offset() {
        let (replies, on_21) = xit_then_rit(0xA2, IcomModel::Ic9700);
        // The bus first, because that is the defect itself: only the RIT control belongs here.
        assert_eq!(
            on_21,
            vec![vec![0x00, 0x00, 0x05, 0x00]],
            "an XIT reached the IC-9700's `21` registers; only `J 500` (RIT) may: {on_21:02x?}"
        );
        for (line, reply) in XIT_LINES.iter().zip(&replies) {
            assert_eq!(
                reply, "RPRT -11\n",
                "{line:?} must answer not-implemented, as Hamlib does for this radio"
            );
        }
    }

    /// THE GUARD, on the same lines: the IC-7610 has ΔTX (A7380-7EX-4 lists `21 02`), and its
    /// XIT reaches the wire byte for byte as it always has, the switch and the shared offset.
    /// The fixture models no `21` register and NAKs each one, so these answer `RPRT -1` (the
    /// rig refused); what must never come back is `RPRT -11`, the IC-9700's refusal.
    #[test]
    fn an_ic7610_keeps_its_xit_on_the_wire_exactly_as_before() {
        let (replies, on_21) = xit_then_rit(0x98, IcomModel::Ic7610);
        assert_eq!(
            on_21,
            vec![
                vec![0x02, 0x01],             // U XIT 1: ΔTX on
                vec![0x00, 0x00, 0x05, 0x00], // Z 500
                vec![0x00, 0x34, 0x12, 0x01], // Z -1234: BCD magnitude, then the sign
                vec![0x02, 0x00],             // U XIT 0: ΔTX off
                vec![0x00, 0x00, 0x00, 0x00], // Z 0
                vec![0x00, 0x00, 0x05, 0x00], // J 500: RIT, the same offset register
            ],
            "{on_21:02x?}"
        );
        for (line, reply) in XIT_LINES.iter().zip(&replies) {
            assert_ne!(reply, "RPRT -11\n", "{line:?} must stay implemented");
        }
    }

    /// Every model the native daemon runs for: `rigmodels::icom_scope_model` maps Hamlib's
    /// 3073/3078/3081/3085/3090 onto exactly these.
    const NATIVE_MODELS: [IcomModel; 5] = [
        IcomModel::Ic7300,
        IcomModel::Ic7610,
        IcomModel::Ic9700,
        IcomModel::Ic705,
        IcomModel::Ic905,
    ];

    /// ★ A VOICE-MEMORY STOP REACHES THE RADIO AS `28 00 00`, on every model the daemon runs.
    /// Each model's Icom reference defines `28 00` as the Voice TX memory with `00` = Stop. The
    /// daemon used to answer `RPRT -11`, so a radio switch's stop, and a logger's through the
    /// broker, never reached a native Icom.
    #[test]
    fn a_voice_memory_stop_reaches_every_native_model_as_28_00_00() {
        for model in NATIVE_MODELS {
            let addr = model.default_civ_addr();
            let (radio, _push) = FakeRadio::new(addr);
            let regs = radio.regs();
            let daemon =
                CivDaemon::start_with_io(Box::new(radio), addr, 0, 1, Some(model)).unwrap();
            let (mut c, mut rd) = client(daemon.local_addr().port());
            assert_eq!(
                roundtrip(&mut c, &mut rd, "\\stop_voice_mem\n"),
                "RPRT 0\n",
                "{model:?}"
            );
            let on_28: Vec<Vec<u8>> = regs
                .lock()
                .unwrap()
                .log
                .iter()
                .filter(|(cmd, _)| *cmd == 0x28)
                .map(|(_, d)| d.clone())
                .collect();
            assert_eq!(
                on_28,
                vec![vec![0x00, 0x00]],
                "{model:?}: exactly `28 00 00` on the wire"
            );
        }
    }

    /// ★ A RADIO SWITCH NEVER PUSHES A NATIVE CONNECTION TOWARD HANG-UP. The server drops a peer
    /// after `MAX_CONSECUTIVE_UNKNOWN` (3) not-implemented replies in a row. A switch sends the
    /// radio being left `T 0`, `\stop_morse` and `\stop_voice_mem`, and none of them may answer
    /// `RPRT -11`, so switches back to back leave the loop's connection up.
    #[test]
    fn back_to_back_switches_leave_the_native_connection_up() {
        let (_d, port, _regs) = daemon_with_regs();
        let (mut c, mut rd) = client(port);
        for _ in 0..3 {
            for line in ["T 0\n", "\\stop_morse\n", "\\stop_voice_mem\n"] {
                let reply = roundtrip(&mut c, &mut rd, line);
                assert_ne!(reply, "RPRT -11\n", "{line:?} fed the hang-up counter");
            }
        }
        assert!(
            roundtrip(&mut c, &mut rd, "f\n")
                .trim()
                .parse::<u64>()
                .is_ok(),
            "the connection is still up and answering"
        );
        // The control, on a fresh connection: three not-implemented verbs in a row DO hang up,
        // so the check above would see one. (No reply to the third; the server closes first.)
        let (mut c, mut rd) = client(port);
        for _ in 0..2 {
            assert_eq!(
                roundtrip(&mut c, &mut rd, "\\send_voice_mem 1\n"),
                "RPRT -11\n"
            );
        }
        assert_eq!(
            roundtrip(&mut c, &mut rd, "\\send_voice_mem 1\n"),
            "",
            "the third not-implemented reply in a row hangs up"
        );
    }
}
