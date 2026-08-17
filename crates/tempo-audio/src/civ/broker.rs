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

use std::net::TcpListener;
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

impl CivBackend {
    pub fn new(h: CivHandle, addr: u8, tx_intent: Arc<AtomicBool>) -> Self {
        CivBackend {
            h,
            addr,
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
            Ok(f) => commands::parse_freq(&f)
                .or(self.h.state().freq_hz)
                .unwrap_or(0),
            // Radio busy (a timeout can be one crowded moment): the last transceive/
            // reply is honest recent truth. A DEAD engine gets no such grace — serving
            // the frozen cache would paint a zombie green with a frozen dial.
            Err(CivError::Timeout) => self.h.state().freq_hz.unwrap_or(0),
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
        let data_ok = self.ack(commands::set_data_mode(self.addr, data, None));
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
            // RX DSP levels — 0..1 like mic gain (distinct from the NR/NB on/off funcs).
            "NR" => self.dsp_level(commands::LVL_NR),
            "NB" => self.dsp_level(commands::LVL_NB),
            // AGC as the Hamlib enum int (OFF=0/FAST=2/SLOW=3/MEDIUM=5), translated from the rig's
            // Icom byte so the rigctld side stays Hamlib-native.
            "AGC" => {
                let f = self
                    .read(commands::read_agc(self.addr), 0x16, Some(0x12))
                    .ok()?;
                let civ = commands::parse_agc_civ(&f)?;
                Some(format!("{}", commands::agc_hamlib_from_civ(civ)))
            }
            _ => None,
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
            "NR" => self.set_dsp_level_pct(commands::LVL_NR, value),
            "NB" => self.set_dsp_level_pct(commands::LVL_NB, value),
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
            _ => None,
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
        match token {
            "RIT" => Some(self.ack(commands::set_rit_on(self.addr, on))),
            "XIT" => Some(self.ack(commands::set_dtx_on(self.addr, on))),
            // NB / NR / ANF / COMP / MON / VOX → the 0x16 DSP-function table.
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
        // Icom's ΔTX shares the RIT offset register.
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
    tcp_stop: Arc<AtomicBool>,
    tcp_thread: Option<JoinHandle<()>>,
    /// Shared with the broker backend: set true while Nexus is transmitting so the disconnect
    /// fail-safe unkey doesn't fire on Nexus's own Rig reconnect (the CI-V PTT-flicker fix).
    tx_intent: Arc<AtomicBool>,
}

impl CivDaemon {
    /// Start on an already-open transport (tests use the in-memory fake radio).
    pub fn start_with_io(
        io: Box<dyn super::engine::CivIo>,
        civ_addr: u8,
        tcp_port: u16,
    ) -> std::io::Result<CivDaemon> {
        let engine = CivEngine::start(io, civ_addr);
        let listener = TcpListener::bind(("127.0.0.1", tcp_port))?;
        listener.set_nonblocking(true)?;
        let tx_intent = Arc::new(AtomicBool::new(false));
        let backend: Arc<dyn RigBackend> = Arc::new(CivBackend::new(
            engine.handle(),
            civ_addr,
            tx_intent.clone(),
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
            tcp_stop,
            tcp_thread: Some(tcp_thread),
            tx_intent,
        })
    }

    /// Open the real COM port and start the daemon (the production entry).
    #[cfg(feature = "serial")]
    pub fn start(
        port_name: &str,
        baud: u32,
        civ_addr: u8,
        tcp_port: u16,
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
        Self::start_with_io(Box::new(port), civ_addr, tcp_port)
    }

    /// The CI-V address to drive `model_name` at, when it's a native-capable Icom.
    pub fn civ_addr_for(model_name: &str) -> Option<u8> {
        IcomModel::from_name(model_name).map(IcomModel::default_civ_addr)
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
    /// Best-effort transact; a NAK (unsupported / in fixed mode) is fine.
    pub fn set_scope_span(&self, span_hz: u32) {
        let _ = self.engine.handle().transact(
            commands::set_scope_span(self.civ_addr, self.scope_ms(), span_hz),
            Expect::Ack,
        );
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
    use super::super::engine::tests_support::FakeRadio;
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    fn daemon() -> (CivDaemon, u16) {
        // Race-free enough for tests: bind :0 to learn a free port, drop, rebind.
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let (radio, _push) = FakeRadio::new(0xA2);
        let d = CivDaemon::start_with_io(Box::new(radio), 0xA2, port).unwrap();
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

    fn daemon_with_regs() -> (CivDaemon, u16, Arc<std::sync::Mutex<Regs>>) {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let (radio, _push) = FakeRadio::new(0xA2);
        let regs = radio.regs();
        let d = CivDaemon::start_with_io(Box::new(radio), 0xA2, port).unwrap();
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
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let (radio, _push) = FakeRadio::new(0x94);
        let regs = radio.regs();
        regs.lock().unwrap().no_satmode = true;
        let _d = CivDaemon::start_with_io(Box::new(radio), 0x94, port).unwrap();
        let (mut c, mut rd) = client(port);

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
        let backend = CivBackend::new(engine.handle(), 0xA2, Arc::new(AtomicBool::new(false)));

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
}
