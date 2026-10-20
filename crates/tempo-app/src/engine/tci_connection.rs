//! A radio on the TCI connection, on the engine: what Nexus itself may transmit through an SDR
//! program's TCI server (ExpertSDR3, Thetis, piHPSDR), and what a click on a spot in that
//! program's panadapter may do. The session (`tempo_net::tci::session`) and the CAT daemon
//! (`tempo_audio::tci`) refuse at the key everything the server cannot take: a key before
//! `ready;`, while the program forbids transmit, above the drive Nexus set, a digital over on the
//! program's microphone, Nexus's audio with no route for it. This module holds the station's side:
//! what must be refused before an over is even planned, and what an outside program's click may
//! never do.
//!
//! ## What the connection refuses
//!
//! ⛔ THE SLOT-TIMED MODES WAIT FOR THE BENCH. Every tier of the Digital section (FT8, FT4, FT2,
//! Tempo Fast and Deep, Q65, FST4, FST4W, MSK144, JT65, WSPR and JS8) is slot-timed, and on TCI an
//! over's start is the program's: it keys, then pulls the audio when it is ready for it, so the
//! over starts as late as the program's first pull, which nobody has measured yet; and the
//! mid-over message switch needs the TCI route to replace what it still holds. So on the TCI
//! connection the Digital section transmits nothing yet ([`TCI_SLOT_TIMED_TX_REFUSED`]). The
//! refusal is [`Engine::connection_tx_refusal`]'s, so `tx_allowed` is false with it, every transmit
//! path refuses before PTT exactly as it refuses a frequency outside the licence (nothing is
//! planned, sequenced or written to ALL.TXT, which is why it changes no timing or sequencing), and
//! the snapshot carries its words so the cockpit names the connection, not the licence. It judges
//! the section ASKED (`connection_tx_refusal_as`): the gate asked as Digital from another section
//! is refused too, and asked as another section from Digital is not. Allowing the Digital section
//! is the one TCI arm of `connection_tx_refusal_as`, once the TCI route carries the switch and the
//! bench has timed it.
//!
//! The other sections are the session's to refuse: Phone at the shack keys the program's own
//! microphone and Tune its own carrier; Nexus's audio (RTTY, PSK, SSTV, APRS, the voice keyer)
//! waits for the TCI audio route; CW for the program's CW keyer. Nothing here repeats those rules,
//! and nothing here admits what they refuse. Every other gate (TX armed, a Stop's hold, a watchdog
//! trip, the licence) stands as on any other connection.
//!
//! ## A click on a spot in the program's panadapter
//!
//! When the operator clicks a spot on the SDR program's panadapter, the program tells its clients
//! (`clicked_on_spot`, `rx_clicked_on_spot`) and the session hands the click on. It arrives from
//! outside, so [`Engine::tci_spot_clicked`] tunes and does nothing else: the dial goes to the
//! spot, where Nexus's own spot click puts it, in the section the operator is in, on the TCI radio
//! itself. It never starts a QSO, never arms TX, never clears a Stop's hold, a watchdog trip or a
//! band change's hold, never starts or restarts the watchdog clock, never changes the section, and
//! never moves the dial while anything is on the air. Nor does it ask the window to open a cockpit,
//! as a Work click does: the window's entry to a Phone, CW, RTTY or PSK cockpit arms TX where
//! nothing holds it. A band change it makes halts what a band change halts.

use super::Engine;

/// Why the TCI connection refuses the Digital section's transmissions, as the operator reads it: on
/// the cockpit's TX state, in the JS8 cockpit and in the log.
pub const TCI_SLOT_TIMED_TX_REFUSED: &str =
    "FT8, FT4, JS8 and the other slot-timed modes do not transmit on the TCI connection yet (Beta)";

/// What a click on a spot in the SDR program's panadapter could not do, for the connection log.
const SPOT_NOT_ON_TCI: &str = "spot click ignored: the radio in use is not on the TCI connection";
const SPOT_ON_AIR: &str =
    "spot click ignored: Nexus never moves the dial for one while anything is on the air";
const SPOT_NO_CALL: &str = "spot click ignored: it names no callsign";
const SPOT_OFF_BAND: &str = "spot click ignored: its frequency is on no amateur band";

/// The longest callsign a click may name, as a cluster writes them (`VP2V/K1ABC/P` is 12).
const SPOT_CALL_MAX: usize = 20;

impl Engine {
    /// The SDR program's operator clicked the spot `call` at `hz` on its panadapter: tune the TCI
    /// radio to it, as Nexus's own spot click places the dial, and nothing else (the module
    /// header). `Err` says why the click changed nothing, for the caller to log.
    pub fn tci_spot_clicked(&mut self, call: &str, hz: i64) -> Result<(), &'static str> {
        if !crate::settings::rig_conn_is_tci(&self.settings.rig_conn) {
            return Err(SPOT_NOT_ON_TCI);
        }
        if self.on_air() {
            return Err(SPOT_ON_AIR);
        }
        let call = call.trim().to_ascii_uppercase();
        if call.len() > SPOT_CALL_MAX || !tempo_core::message::looks_like_call(&call) {
            return Err(SPOT_NO_CALL);
        }
        let spot_mhz = hz as f64 / 1e6;
        let band = crate::bandplan::band_for_dial(spot_mhz).ok_or(SPOT_OFF_BAND)?;
        // The radio whose panadapter it is: the band's routing never hands the click to another.
        self.route_target = Some(None);
        // A soundcard CW keyer radiates a pitch off its dial: the dial sits a pitch off the spot.
        let dial = spot_mhz - self.soundcard_cw_pitch_offset_mhz(spot_mhz);
        self.set_frequency(dial, band, "USB");
        tempo_core::applog::info(
            "cat",
            &format!(
                "TCI: tuned to {call} at {spot_mhz:.6} MHz, a spot clicked in the SDR program"
            ),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Tier;
    use crate::engine::now_unix_secs;
    use crate::settings::{RadioProfile, Settings, ICOM_LAN_TX_REFUSED};

    /// A General's station at launch on one radio, the TCI connection's own model (7), on `conn`:
    /// "tci", or "serial" as the control. TX is off and nothing holds it. The CAT share may key, so
    /// a program's PTT through it is a path too.
    fn on(conn: &str) -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let profile = RadioProfile {
            id: 0,
            rig_model: 7,
            rig_conn: conn.into(),
            ..Default::default()
        };
        e.apply_restored_settings(Settings {
            // The station's identity, which the FT paths validate at the keying boundary.
            mycall: "W9XYZ".into(),
            mygrid: "EN61".into(),
            rig_model: 7,
            rig_conn: conn.into(),
            cat_broker_ptt: true,
            radios: vec![profile],
            active_radio: 0,
            ..Settings::default()
        });
        e.set_license_class("general");
        assert!(!e.tx_enabled(), "premise: TX is off at launch");
        e
    }

    /// [`on`], in `section` at `mhz` on 20 m, TX armed by the operator.
    fn at(conn: &str, section: &str, mhz: f64) -> Engine {
        let mut e = on(conn);
        e.set_operating_mode(section, false);
        e.set_frequency(mhz, "20m", "USB");
        e.set_tx_enabled_by_operator(true);
        e
    }

    /// [`on`] at `tier`, its CQ running as Call CQ starts it (TX armed).
    fn cq_run(conn: &str, tier: Tier) -> Engine {
        let mut e = on(conn);
        e.set_tier(tier);
        e.set_mode("qso-run").expect("Call CQ");
        assert!(e.tx_enabled(), "premise: Call CQ arms TX");
        e
    }

    /// The first JS8 slot after now.
    fn next_js8_slot(e: &Engine) -> u64 {
        let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
        tempo_core::timing::now_unix_ms() as u64 / period_ms + 1
    }

    /// ⛔ THE TCI CONNECTION HOLDS THE SLOT-TIMED MODES BEFORE PTT, IN ITS OWN WORDS. In the
    /// Digital section FT8, FT4 and JS8 plan nothing to key, and the lock, the snapshot and Tune
    /// say why: the TCI connection, not the licence. The control is the same radio on its serial
    /// connection, where every one of the same paths goes ahead. Every path is measured before
    /// anything is asserted, so a red names them all.
    #[test]
    fn the_tci_connection_holds_the_slot_timed_modes_before_ptt_and_names_itself() {
        let mut wrong = Vec::new();
        for (conn, tci) in [("serial", false), ("tci", true)] {
            let refused = tci.then(|| TCI_SLOT_TIMED_TX_REFUSED.to_string());
            let mut check = |what: &str, got: String, want: String| {
                if got != want {
                    wrong.push(format!("{conn}: {what}: got {got}, want {want}"));
                }
            };

            let e = at(conn, "digital", 14.074);
            check(
                "the verdict",
                format!("{}", e.tx_allowed()),
                format!("{}", !tci),
            );
            let radio = e.snapshot().radio;
            check(
                "the snapshot's lock",
                format!("{}", radio.tx_allowed),
                format!("{}", !tci),
            );
            check(
                "the snapshot's reason",
                format!("{:?}", radio.tx_refusal),
                format!("{refused:?}"),
            );
            check(
                "the connection's reason",
                format!("{:?}", e.connection_tx_refusal()),
                format!("{:?}", tci.then_some(TCI_SLOT_TIMED_TX_REFUSED)),
            );

            // FT8 and FT4: Call CQ arms, and the slot plans nothing to key.
            for tier in [Tier::Ft8, Tier::Ft4] {
                let mut e = cq_run(conn, tier);
                check(
                    &format!("{tier:?}: the CQ keys"),
                    format!("{}", !e.poll_tx(0).is_empty()),
                    format!("{}", !tci),
                );
            }

            // JS8: the message waits, unplanned.
            let mut e = on(conn);
            e.js8_enter();
            e.set_tx_offset(1500.0);
            e.set_tx_enabled_by_operator(true);
            e.js8_send(None, "HELLO FROM TCI".into()).expect("queues");
            let slot = next_js8_slot(&e);
            check(
                "JS8: the slot keys",
                format!("{}", e.plan_tx(slot).is_some()),
                format!("{}", !tci),
            );

            // Tune in the Digital section.
            let mut e = at(conn, "digital", 14.074);
            e.set_tune(true);
            check(
                "Digital: Tune",
                format!("{}", e.tuning()),
                format!("{}", !tci),
            );
        }
        assert!(
            wrong.is_empty(),
            "the slot-timed modes on TCI:\n{}",
            wrong.join("\n")
        );
    }

    /// The JS8 cockpit's line, when its queue is dropped because the connection refuses the
    /// section, names that connection, as CW's notice does: the TCI connection's words, and the
    /// Icom network's own. CONTROLS: on a serial radio nothing is refused, and outside the licence
    /// the line names the licence.
    #[test]
    fn the_js8_line_names_the_connection_not_the_licence() {
        let line = |conn: &str, mhz: f64| {
            let mut e = on(conn);
            e.js8_enter();
            e.set_frequency(mhz, "20m", "USB");
            e.set_tx_offset(1500.0);
            e.set_tx_enabled_by_operator(true);
            e.js8_send(None, "HELLO FROM TCI".into()).expect("queues");
            e.js8_tick(tempo_core::timing::now_unix_ms() as u64);
            e.js8_state().last_error
        };
        assert_eq!(
            line("tci", 14.078).as_deref(),
            Some(TCI_SLOT_TIMED_TX_REFUSED)
        );
        assert_eq!(
            line("icomlan", 14.078).as_deref(),
            Some(ICOM_LAN_TX_REFUSED)
        );
        assert_eq!(line("serial", 14.078), None, "control: nothing refused");
        // A General's 20 m data starts at 14.025.
        let outside = line("serial", 14.010).unwrap_or_default();
        assert!(
            outside.contains("license privileges"),
            "control: outside the licence the line names it: {outside:?}"
        );
    }

    /// ⛔ ONLY THE DIGITAL SECTION: on the TCI connection every other section's transmit path goes
    /// ahead in the engine exactly as on the serial one (the TCI session refuses at the key what it
    /// cannot carry yet, in its own words), and nothing names the TCI connection there.
    #[test]
    fn the_tci_connection_holds_no_other_section() {
        let mut wrong = Vec::new();
        for conn in ["serial", "tci"] {
            let mut check = |what: &str, ok: bool| {
                if !ok {
                    wrong.push(format!("{conn}: {what}"));
                }
            };
            let said = |r: Result<(), String>| r.err();

            for (section, mhz) in [
                ("phone", 14.250),
                ("cw", 14.030),
                ("rtty", 14.085),
                ("keyboard", 14.070),
            ] {
                let e = at(conn, section, mhz);
                check(&format!("{section}: the verdict"), e.tx_allowed());
                check(
                    &format!("{section}: no refusal"),
                    e.snapshot().radio.tx_refusal.is_none(),
                );
            }

            // Phone: PTT, Tune, the voice keyer, SSTV, a program's PTT through the CAT share.
            let mut e = at(conn, "phone", 14.250);
            e.set_ptt(true);
            check("Phone: PTT", e.manual_ptt());
            let mut e = at(conn, "phone", 14.250);
            e.set_tune(true);
            check("Phone: Tune", e.tuning());
            let mut e = at(conn, "phone", 14.250);
            check(
                "Phone: voice keyer",
                said(e.send_voice(vec![0.1, 0.2])).is_none(),
            );
            let mut e = at(conn, "phone", 14.230);
            check(
                "Phone: SSTV",
                said(e.sstv_send(vec![0.0f32; 12_000], "PD-120".into())).is_none(),
            );
            let mut e = at(conn, "phone", 14.250);
            check("Phone: CAT share PTT", e.broker_ptt(true));
            let mut e = at(conn, "phone", 14.250);
            check(
                "Phone: APRS",
                said(e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[])).is_none(),
            );

            // CW, RTTY, PSK.
            let mut e = at(conn, "cw", 14.030);
            e.send_cw("TEST");
            check("CW: keyed", e.poll_cw_one().is_some());
            check("CW: no notice", e.cw_keyer_error().is_none());
            let mut e = at(conn, "rtty", 14.085);
            check("RTTY", said(e.rtty_send_text("CQ")).is_none());
            let mut e = at(conn, "keyboard", 14.070);
            check("PSK", said(e.psk_send_text("CQ")).is_none());
        }
        assert!(
            wrong.is_empty(),
            "refused on the TCI connection:\n{}",
            wrong.join("\n")
        );
    }

    /// ⛔ NO GATE GIVES WAY TO THE TCI CONNECTION. On it, in Phone, the licence still refuses (in
    /// the licence's words, not the connection's), TX Off still refuses, and a Stop's hold and a
    /// watchdog trip still keep the section from arming; and the Icom network connection keeps its
    /// own refusal, in every section. CONTROL: the same Phone station inside the licence keys.
    #[test]
    fn every_other_gate_still_holds_on_the_tci_connection() {
        let mut e = at("tci", "phone", 14.200);
        assert!(!e.tx_allowed(), "a General's phone below 14.225");
        assert_eq!(
            e.snapshot().radio.tx_refusal,
            None,
            "the licence's words, not the connection's"
        );
        e.set_ptt(true);
        assert!(!e.manual_ptt(), "PTT outside the licence");

        let mut e = at("tci", "phone", 14.250);
        e.set_ptt(true);
        assert!(e.manual_ptt(), "control: inside the licence PTT keys");

        let mut e = at("tci", "phone", 14.250);
        e.set_tx_enabled_by_operator(false);
        e.set_ptt(true);
        assert!(!e.manual_ptt(), "PTT with TX off");

        let mut e = at("tci", "phone", 14.250);
        e.halt_tx();
        e.set_operating_mode("cw", false);
        e.set_operating_mode("phone", false);
        assert!(!e.tx_enabled(), "a Stop's hold on the TCI connection");

        let mut e = at("tci", "phone", 14.250);
        e.trip_tx_watchdog_for_test();
        e.set_operating_mode("cw", false);
        e.set_operating_mode("phone", false);
        assert!(!e.tx_enabled(), "a watchdog trip on the TCI connection");

        for section in ["digital", "phone", "cw"] {
            let mut e = on("icomlan");
            e.set_operating_mode(section, false);
            assert_eq!(
                e.connection_tx_refusal(),
                Some(ICOM_LAN_TX_REFUSED),
                "the Icom network connection in {section}"
            );
        }
    }

    /// The refusal reads the radio in use and the section the operator is in, now: it follows a
    /// section change and a switch between a serial radio and a TCI one both ways, and so does the
    /// snapshot's reason.
    #[test]
    fn the_refusal_follows_the_section_and_the_radio_in_use() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let radio = |id: u32, conn: &str| RadioProfile {
            id,
            rig_model: 7,
            rig_conn: conn.into(),
            ..Default::default()
        };
        e.apply_restored_settings(Settings {
            mycall: "W9XYZ".into(),
            mygrid: "EN61".into(),
            rig_model: 7,
            rig_conn: "serial".into(),
            radios: vec![radio(0, "serial"), radio(1, "tci")],
            active_radio: 0,
            ..Settings::default()
        });
        e.set_operating_mode("digital", false);
        let mut seen = Vec::new();
        let mut note = |e: &Engine, step: &str| {
            seen.push((step.to_string(), e.snapshot().radio.tx_refusal.is_some()));
        };
        note(&e, "serial, Digital");
        e.set_active_radio(1);
        note(&e, "TCI, Digital");
        e.set_operating_mode("phone", false);
        note(&e, "TCI, Phone");
        e.set_operating_mode("digital", false);
        note(&e, "TCI, Digital again");
        e.set_active_radio(0);
        note(&e, "serial, Digital again");
        let want = [
            ("serial, Digital", false),
            ("TCI, Digital", true),
            ("TCI, Phone", false),
            ("TCI, Digital again", true),
            ("serial, Digital again", false),
        ]
        .map(|(s, r)| (s.to_string(), r));
        assert_eq!(seen, want);
    }

    /// ⛔ THE REFUSAL JUDGES THE SECTION ASKED, NOT THE ONE THE STATION IS IN. On a TCI radio in
    /// Phone, the gate asked as Digital is refused as slot-timed; in Digital, the gate asked as
    /// Phone or as CW is the licence's alone. CONTROLS: the same station on its serial connection
    /// allows all three, so each answer on TCI is the connection's.
    #[test]
    fn the_refusal_judges_the_section_asked_not_the_section_the_station_is_in() {
        use crate::settings::OperatingMode::{Cw, Digital, Phone};
        let mut seen = Vec::new();
        for conn in ["serial", "tci"] {
            // A General's data carrier above 14.074 is inside the privileges, and so is CW there.
            let e = at(conn, "phone", 14.074);
            seen.push((conn, "in Phone, asked as Digital", e.tx_allowed_as(Digital)));
            let e = at(conn, "digital", 14.074);
            seen.push((conn, "in Digital, asked as CW", e.tx_allowed_as(Cw)));
            // A General's phone at 14.250 is inside the privileges.
            let e = at(conn, "digital", 14.250);
            seen.push((conn, "in Digital, asked as Phone", e.tx_allowed_as(Phone)));
        }
        let want = [
            ("serial", "in Phone, asked as Digital", true),
            ("serial", "in Digital, asked as CW", true),
            ("serial", "in Digital, asked as Phone", true),
            ("tci", "in Phone, asked as Digital", false),
            ("tci", "in Digital, asked as CW", true),
            ("tci", "in Digital, asked as Phone", true),
        ];
        assert_eq!(seen, want);
    }

    // ── A click on a spot in the SDR program's panadapter ─────────────────────────────────────

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Holds {
        tx_enabled: bool,
        stop_hold: bool,
        watchdog: bool,
        context_hold: bool,
        watchdog_start: Option<u64>,
    }

    fn holds(e: &Engine) -> Holds {
        Holds {
            tx_enabled: e.tx_enabled,
            stop_hold: e.tx_stop_hold,
            watchdog: e.tx_watchdog,
            context_hold: e.broker_context_hold,
            watchdog_start: e.tx_watchdog_start,
        }
    }

    /// What a click may do to them: lower TX (a band change's halt), set a hold, never the reverse,
    /// and never touch the watchdog clock.
    fn arms_or_clears(before: Holds, after: Holds) -> bool {
        (!before.tx_enabled && after.tx_enabled)
            || (before.stop_hold && !after.stop_hold)
            || (before.watchdog && !after.watchdog)
            || (before.context_hold && !after.context_hold)
            || before.watchdog_start != after.watchdog_start
    }

    type State = (&'static str, fn() -> Engine);

    /// The states a click can find a TCI radio in.
    fn states() -> Vec<State> {
        vec![
            ("launch, TX off", || on("tci")),
            ("Digital, a CQ run armed", || cq_run("tci", Tier::Ft8)),
            ("Phone, armed", || at("tci", "phone", 14.250)),
            ("Phone, armed, the watchdog clock running", || {
                let mut e = at("tci", "phone", 14.250);
                e.tx_watchdog_start = Some(now_unix_secs() - 30);
                e
            }),
            ("after Stop TX", || {
                let mut e = at("tci", "phone", 14.250);
                e.halt_tx();
                e
            }),
            ("after a watchdog trip", || {
                let mut e = at("tci", "phone", 14.250);
                e.trip_tx_watchdog_for_test();
                e
            }),
            ("after a band change", || {
                let mut e = at("tci", "phone", 14.250);
                e.set_frequency(7.200, "40m", "LSB");
                e
            }),
            ("CW, armed", || at("tci", "cw", 14.030)),
        ]
    }

    /// The clicks: on the band the radio is on, and on another.
    const CLICKS: [(&str, i64); 2] = [("K1ABC", 14_025_000), ("VP2V/K1ABC", 7_030_000)];

    /// ⛔ A CLICK ON A SPOT TUNES AND NEVER TRANSMITS. From every state and for every click: no QSO
    /// starts, TX is never armed, no Stop's hold, watchdog trip or band change's hold is cleared,
    /// the watchdog clock never moves, the section stays, and the window is not asked to open a
    /// cockpit. CONTROL: the click did tune, every time, to the spot.
    #[test]
    fn a_click_on_a_spot_in_the_sdr_panadapter_tunes_and_never_transmits() {
        let mut moved = Vec::new();
        let mut tuned = 0;
        for (state, build) in states() {
            for (call, hz) in CLICKS {
                let mut e = build();
                let (before, section, tick) =
                    (holds(&e), e.settings.operating_mode, e.snapshot().work_tick);
                let r = e.tci_spot_clicked(call, hz);
                let after = holds(&e);
                let what = format!("{state}: {call} at {hz} Hz ({r:?})");
                if arms_or_clears(before, after) {
                    moved.push(format!("{what}: {before:?} -> {after:?}"));
                }
                if let Some(dx) = e.qso_dxcall() {
                    moved.push(format!("{what}: a QSO with {dx} started"));
                }
                if e.settings.operating_mode != section {
                    moved.push(format!(
                        "{what}: the section moved to {:?}",
                        e.settings.operating_mode
                    ));
                }
                if e.snapshot().work_tick != tick {
                    moved.push(format!("{what}: the window was asked to open a cockpit"));
                }
                tuned +=
                    usize::from(r.is_ok() && (e.settings.dial_mhz - hz as f64 / 1e6).abs() < 1e-9);
            }
        }
        assert!(
            moved.is_empty(),
            "a spot click moved them:\n{}",
            moved.join("\n")
        );
        assert_eq!(
            tuned,
            states().len() * CLICKS.len(),
            "control: every click tuned to its spot"
        );
    }

    /// The click on a TCI radio in CW with the soundcard keyer: the dial sits a pitch below the
    /// spot on the upper side, where Nexus's own spot click puts it, so the signal lands on the
    /// station.
    #[test]
    fn a_click_in_soundcard_cw_puts_the_dial_a_pitch_off_the_spot() {
        let mut e = at("tci", "cw", 14.030);
        e.settings.cw_keyer = crate::settings::CwKeyerBackend::Soundcard;
        let pitch = f64::from(e.cw_pitch_hz()) / 1e6;
        assert!(pitch > 0.0, "premise: a CW pitch");
        assert_eq!(e.tci_spot_clicked("K1ABC", 14_025_000), Ok(()));
        assert!(
            (e.settings.dial_mhz - (14.025 - pitch)).abs() < 1e-9,
            "the dial: {} MHz",
            e.settings.dial_mhz
        );
    }

    /// The click goes to the radio whose panadapter it is: a second radio the band is routed to
    /// does not take it. CONTROL: the operator's own QSY to the same band is routed there.
    #[test]
    fn a_click_stays_on_the_tci_radio_whatever_the_band_routing_says() {
        let two_radios = || {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            let tci = RadioProfile {
                id: 0,
                rig_model: 7,
                rig_conn: "tci".into(),
                bands: vec!["20m".into()],
                ..Default::default()
            };
            let other = RadioProfile {
                id: 1,
                rig_model: 1042,
                rig_conn: "serial".into(),
                bands: vec!["40m".into()],
                ..Default::default()
            };
            e.apply_restored_settings(Settings {
                mycall: "W9XYZ".into(),
                mygrid: "EN61".into(),
                rig_model: 7,
                rig_conn: "tci".into(),
                radios: vec![tci, other],
                active_radio: 0,
                ..Settings::default()
            });
            e.set_operating_mode("cw", false);
            e.set_frequency(14.030, "20m", "USB");
            assert_eq!(e.settings.active_radio, 0, "premise: on the TCI radio");
            e
        };
        let mut control = two_radios();
        control.set_frequency(7.030, "40m", "USB");
        assert_eq!(
            control.settings.active_radio, 1,
            "control: the operator's own QSY to 40 m is routed to the other radio"
        );
        let mut e = two_radios();
        assert_eq!(e.tci_spot_clicked("K1ABC", 7_030_000), Ok(()));
        assert_eq!(
            (e.settings.active_radio, e.settings.band.as_str()),
            (0, "40m"),
            "the click left the TCI radio"
        );
    }

    /// ⛔ NOT WHILE ANYTHING IS ON THE AIR, NOT OFF THE TCI CONNECTION, NOT OFF THE BANDS, NOT
    /// WITHOUT A CALLSIGN: each such click is refused, says why, and leaves the dial where it was.
    /// CONTROL: the same click, with nothing in the way, tunes.
    #[test]
    fn a_click_that_cannot_be_a_spot_on_this_radio_changes_nothing() {
        type Setup = fn(&mut Engine);
        let cases: [(&str, Setup, &str, i64, &str); 9] = [
            (
                "tuning",
                |e| e.set_tune(true),
                "K1ABC",
                14_025_000,
                SPOT_ON_AIR,
            ),
            (
                "PTT held",
                |e| e.set_ptt(true),
                "K1ABC",
                14_025_000,
                SPOT_ON_AIR,
            ),
            (
                "on a serial radio",
                |e| e.settings.rig_conn = "serial".into(),
                "K1ABC",
                14_025_000,
                SPOT_NOT_ON_TCI,
            ),
            ("off the bands", |_| {}, "K1ABC", 15_000_000, SPOT_OFF_BAND),
            (
                "a negative frequency",
                |_| {},
                "K1ABC",
                -14_025_000,
                SPOT_OFF_BAND,
            ),
            ("no call", |_| {}, " ", 14_025_000, SPOT_NO_CALL),
            ("not a call", |_| {}, "HELLO", 14_025_000, SPOT_NO_CALL),
            ("a command", |_| {}, "TRX:0,TRUE", 14_025_000, SPOT_NO_CALL),
            (
                "too long",
                |_| {},
                "K1ABC/AAAAAAAAAAAAAAAAAAAAAAAA",
                14_025_000,
                SPOT_NO_CALL,
            ),
        ];
        let mut wrong = Vec::new();
        for (what, setup, call, hz, why) in cases {
            let mut e = at("tci", "phone", 14.250);
            setup(&mut e);
            assert_eq!(
                e.on_air(),
                why == SPOT_ON_AIR,
                "premise: {what} puts something on the air, or nothing"
            );
            let dial = e.settings.dial_mhz;
            let r = e.tci_spot_clicked(call, hz);
            if r != Err(why) || e.settings.dial_mhz != dial {
                wrong.push(format!(
                    "{what}: {r:?}, the dial {dial} -> {}",
                    e.settings.dial_mhz
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        let mut e = at("tci", "phone", 14.250);
        assert_eq!(e.tci_spot_clicked("k1abc", 14_225_000), Ok(()), "control");
        assert!(
            (e.settings.dial_mhz - 14.225).abs() < 1e-9,
            "control: tuned"
        );
    }
}
