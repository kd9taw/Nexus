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

use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use super::commands::{self, IcomModel, Mode};
use super::engine::{CivEngine, CivError, CivHandle, Expect};
use super::frame::Frame;
use super::scope::ScopeSweep;
use crate::rigctld_server::{serve_connection, RigBackend};

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
    /// hand an IC-7300 the 7610's 6/12/18 dB pads. `None` = an Icom this build has no step
    /// list for, and then neither control is offered at all rather than guessed at.
    model: Option<IcomModel>,
    /// When the dial was last READ from the radio (not merely cached). Bounds how long a
    /// timed-out `f` may serve the cache — see [`cache_fresh`].
    last_freq_ok: Mutex<Option<std::time::Instant>>,
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
            data_mode: std::sync::atomic::AtomicU8::new(data_mode.clamp(1, 3)),
            split: AtomicBool::new(false),
            tx_intent,
            band: Mutex::new(SatSplit {
                engaged: false,
                cap: None,
                sel_stray: false,
                op_satmode_off: false,
            }),
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

    /// Read a `14 <sub>` DSP level as a 0..1 fraction string (the rigctld level convention).
    fn dsp_level(&self, sub: u8) -> Option<String> {
        let f = self
            .read(commands::read_dsp_level(self.addr, sub), 0x14, Some(sub))
            .ok()?;
        let raw = commands::parse_dsp_level_raw(&f, sub)?;
        Some(format!("{:.2}", f64::from(raw) / 255.0))
    }
    /// Set a `14 <sub>` DSP level from a 0..1 fraction string.
    fn set_dsp_level_pct(&self, sub: u8, value: &str) -> Option<bool> {
        let frac: f64 = value.parse().ok()?;
        let percent = (frac.clamp(0.0, 1.0) * 100.0).round() as u8;
        Some(self.ack(commands::set_dsp_level(self.addr, sub, percent)))
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
        let reply = if self.ensure_main(&mut g) {
            self.read(commands::read_mode(self.addr), 0x04, None)
                .ok()
                .and_then(|f| commands::parse_mode(&f))
        } else {
            // Selection possibly stranded on Sub: the read would serve the
            // uplink's mode. Fall to the state cache like any failed read.
            None
        };
        let st = self.h.state();
        let (mode, _filter) = match reply {
            Some(m) => m,
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

    fn set_ptt(&self, on: bool) -> bool {
        self.ack(commands::set_ptt(self.addr, on))
    }

    fn set_vfo(&self, vfo: &str) -> bool {
        match commands::select_vfo(self.addr, vfo) {
            Some(f) => self.ack(f),
            None => false,
        }
    }

    /// The rig's own pads and preamps, so a client reading `\dump_state` learns which
    /// values exist instead of trying them. Empty for a model this build has no list for —
    /// see [`CivBackend::model`].
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

    fn level(&self, name: &str) -> Option<String> {
        match name {
            "STRENGTH" => {
                let f = self
                    .read(commands::read_smeter(self.addr), 0x15, Some(0x02))
                    .ok()?;
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
                let f = self
                    .read(
                        commands::read_attenuator(self.addr),
                        commands::ATT_CMD,
                        None,
                    )
                    .ok()?;
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
                let f = self
                    .read(
                        commands::read_preamp(self.addr),
                        0x16,
                        Some(commands::FUNC_PREAMP),
                    )
                    .ok()?;
                let idx = commands::parse_preamp_index(&f)?;
                Some(commands::preamp_db_for_index(model, idx)?.to_string())
            }
            // AGC as the Hamlib enum int (OFF=0/FAST=2/SLOW=3/MEDIUM=5), translated from the rig's
            // Icom byte so the rigctld side stays Hamlib-native.
            "AGC" => {
                let f = self
                    .read(commands::read_agc(self.addr), 0x16, Some(0x12))
                    .ok()?;
                let civ = commands::parse_agc_civ(&f)?;
                Some(format!("{}", commands::agc_hamlib_from_civ(civ)))
            }
            // The fractional `0x14 <sub>` family — AF gain, RF gain, squelch, NR, NB and
            // compressor DEPTH — all 0..1 like mic gain, and all distinct from the
            // NR/NB/COMP on/off FUNCS on `0x16`. `COMP` here is the knob; `COMP_METER`
            // above is the TX meter, and they are answered by name before this arm.
            // One token table (`commands::level_sub`) serves this and the setter below, so
            // the two cannot drift apart or transpose a pair.
            _ => commands::level_sub(name).and_then(|sub| self.dsp_level(sub)),
        }
    }

    fn set_level(&self, name: &str, value: &str) -> Option<bool> {
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
                Some(self.ack(commands::set_agc(
                    self.addr,
                    commands::agc_civ_from_hamlib(hamlib),
                )))
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
                Some(self.ack(commands::set_attenuator_db(self.addr, db)))
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
                Some(self.ack(commands::set_preamp_index(self.addr, idx)))
            }
            // The same `0x14 <sub>` family as the getter, off the same table.
            _ => commands::level_sub(name).and_then(|sub| self.set_dsp_level_pct(sub, value)),
        }
    }

    fn func(&self, token: &str) -> Option<bool> {
        // DSP / audio funcs share CI-V command 0x16; the token → sub-command map lives in
        // commands::func_sub. RIT/XIT are separate registers with no simple read here.
        let sub = commands::func_sub(token)?;
        let f = self
            .read(commands::read_dsp_func(self.addr, sub), 0x16, Some(sub))
            .ok()?;
        commands::parse_dsp_func(&f, sub)
    }

    fn set_func(&self, token: &str, on: bool) -> Option<bool> {
        // No ΔTX, no ΔTX switch: `21 02` is not a command this radio has, so it is not sent.
        // `None` is `RPRT -11`, the answer Hamlib gives for the same radio.
        if token == "XIT" && !self.has_delta_tx() {
            return None;
        }
        match token {
            "RIT" => Some(self.ack(commands::set_rit_on(self.addr, on))),
            "XIT" => Some(self.ack(commands::set_dtx_on(self.addr, on))),
            // NB / NR / ANF / MN / COMP / MON / VOX → the 0x16 DSP-function table.
            _ => commands::func_sub(token)
                .map(|sub| self.ack(commands::set_dsp_func(self.addr, sub, on))),
        }
    }

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

    fn set_rit(&self, hz: i32) -> Option<bool> {
        Some(self.ack(commands::set_rit_offset(self.addr, hz)))
    }

    fn set_xit(&self, hz: i32) -> Option<bool> {
        // ⛔ Icom's ΔTX shares the RIT offset register, which is exactly why a radio with no
        // ΔTX must refuse here: on an IC-9700 `21 00` IS the RIT offset, and an XIT written
        // into it lands on the receiver's clarifier and never on the transmitter.
        if !self.has_delta_tx() {
            return None;
        }
        Some(self.ack(commands::set_rit_offset(self.addr, hz)))
    }

    fn set_rptr_shift(&self, shift: &str) -> Option<bool> {
        Some(self.ack(commands::set_duplex(self.addr, shift)))
    }

    fn set_rptr_offset(&self, hz: i64) -> Option<bool> {
        // Cmd 0D, 3-byte BCD in 100 Hz units (confirmed IC-9700 ref: 600 kHz → 00 60 00).
        // The offset magnitude is unsigned; direction comes from the duplex shift (`R`).
        Some(self.ack(commands::set_rptr_offset(self.addr, hz.unsigned_abs())))
    }

    fn set_ctcss(&self, tenths: u32) -> Option<bool> {
        if tenths == 0 {
            return Some(self.ack(commands::set_tone_func(self.addr, false)));
        }
        let tone = self.ack(commands::set_repeater_tone(self.addr, tenths));
        let func = self.ack(commands::set_tone_func(self.addr, true));
        Some(tone && func)
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

        // ATTENUATOR — the operator's dB in, BCD dB on the bus, the same dB back out.
        // The IC-7610's three pads are what make this a real test: 6 dB encodes identically
        // under BCD and raw hex, 12 and 18 do not.
        for (db, wire) in [(6u8, 0x06u8), (12, 0x12), (18, 0x18), (0, 0x00)] {
            assert_eq!(
                backend.set_level("ATT", &db.to_string()),
                Some(true),
                "set ATT {db}"
            );
            assert_eq!(regs.lock().unwrap().att_raw, wire, "ATT {db} dB on the bus");
            assert_eq!(
                backend.level("ATT").as_deref(),
                Some(db.to_string().as_str()),
                "read ATT {db} back"
            );
        }
        // A pad this rig does not have is REFUSED, not rounded to a neighbour. 10 dB is the
        // IC-9700's pad, not the 7610's — quietly substituting 12 would attenuate by an
        // amount the operator did not choose.
        assert_eq!(
            backend.set_level("ATT", "10"),
            Some(false),
            "10 dB is not a 7610 pad"
        );
        assert_eq!(
            regs.lock().unwrap().att_raw,
            0x00,
            "a refused pad never reaches the bus"
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
}
