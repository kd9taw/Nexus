//! A characterization pin for the cut-site refactor: every place the radio loop stops the audio
//! and drops PTT leaves the same state behind it.
//!
//! Each of these bodies is today `flush_output(); rig.ptt(false); tx_until_ms = None`, written
//! out once per site behind that site's own "busy" guard (the stream and microphone cuts), or as
//! part of a context teardown (a radio switch, a CAT rebuild, the Test-CAT port hold, an audio
//! rebuild). A refactor is to fold them into helpers; these tests pin, for each site, the three
//! facts it leaves and the order it does them in, so the fold can be checked to change nothing:
//!
//!  1. the sound card was flushed (`MockBackend::flush_calls` went up);
//!  2. the unkey (`T 0`) reached the radio, a rigctld that logs every line it is sent;
//!  3. the loop holds no PTT deadline afterwards (`RadioLoop::tx_until_ms == None`).
//!
//! The order: the mock card writes `flush` into the same log the rigctld writes its lines to
//! (`MockBackend::events`), so the two land in the order they happened, with no clock involved.
//! Each test pins the whole flush/unkey sequence of the tick that cut, which also says the cut
//! happened once and at the site meant: a site's body runs whenever its guard holds, and the
//! guard is asserted to hold as the tick starts.
//!
//! These pin what the code does TODAY. A value here that looks wrong is a finding to report, not
//! something to fix in this file. Sites whose scaffolding lives privately in another test file
//! have their pin beside it, through [`assert_cut`]: CW in `cw_stop.rs`, JTTY in
//! `jtty_transmit.rs`, the computer microphone in `pc_mic.rs`, a TCI program's over in
//! `tci_server.rs`.
//!
//! The hold-extend sites (`extend_hold`) are pinned at the end: an over keyed under a longer hold
//! never shortens it. One pin per site: the CW word, the RTTY stream and message on AFSK and on
//! FSK, the PSK stream, and (beside their scaffolding) the Remote microphone's key in service.rs,
//! a TCI program's in `tci_server.rs` and the computer microphone's in `pc_mic.rs`; each was
//! checked to go red when its own site overwrites the hold instead. The PSK message and the voice
//! keyer are pinned too. APRS sets its hold rather than extending one, and only ever with no hold
//! standing (its guard), so the set never meets an existing hold to replace: that is what is
//! pinned for it.

use super::*;

/// A hold far beyond anything a test runs to: "a long over is on the air".
const LONG_HOLD: f64 = 9_999_999.0;

/// The sound card's flushes and the radio's unkeys in `log` from `from` on, in order.
pub(super) fn cut_order(log: &Mutex<Vec<String>>, from: usize) -> Vec<String> {
    log.lock().unwrap()[from..]
        .iter()
        .filter(|l| *l == "flush" || *l == "T 0")
        .cloned()
        .collect()
}

/// The three facts every cut leaves, and its flush/unkey sequence, read after the tick that cut.
/// `from` is the log's length and `flushes` the card's flush count before the stop.
pub(super) fn assert_cut(
    site: &str,
    log: &Mutex<Vec<String>>,
    from: usize,
    flushes: usize,
    backend: &MockBackend,
    tx_until_ms: Option<f64>,
    order: &[&str],
) {
    let seen = cut_order(log, from);
    assert!(
        backend.flush_calls > flushes,
        "{site}: the sound card was not flushed: {seen:?}"
    );
    assert!(
        seen.iter().any(|l| l == "T 0"),
        "{site}: the unkey never reached the radio: {:?}",
        &log.lock().unwrap()[from..]
    );
    assert_eq!(
        tx_until_ms, None,
        "{site}: the PTT deadline survived the cut"
    );
    assert_eq!(seen, order, "{site}: the flush/unkey sequence");
}

/// A W9XYZ station with every privilege, set up by `setup`, on a logging rigctld whose dial reads
/// `dial_hz` (so the loop's reads never move the station). The card logs its flushes into the
/// rigctld's log. The loop is built on the station's own transport, so it rebuilds nothing unless
/// a test changes the settings.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    log: Arc<Mutex<Vec<String>>>,
    /// The next tick's time, ms.
    t: f64,
}

impl Scene {
    fn on(dial_hz: u64, setup: impl FnOnce(&mut Engine)) -> Scene {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            setup(&mut e);
        }
        let (addr, log) = mock_rigctld_on(dial_hz, false);
        let mut state = loop_state();
        state.applied = Transport::from_settings(engine.lock().unwrap().settings());
        let mut backend = MockBackend::new();
        backend.events = Some(Arc::clone(&log));
        Scene {
            engine,
            state,
            backend,
            rig: Rig::rigctld(&addr),
            log,
            t: 100.0,
        }
    }

    /// One loop tick at the scene's time, then the clock moves on 20 ms.
    fn tick(&mut self) {
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                self.t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        self.t += 20.0;
    }

    /// Ticks until the scene's clock passes `until`.
    fn run_to(&mut self, until: f64) {
        while self.t <= until {
            self.tick();
        }
    }

    /// "Mid-over" with nothing behind it: the rig keyed through the rigctld and a long hold, the
    /// shape of the existing teardown tests (`audio_rebuild_mid_over_cuts_…`).
    fn keyed_by_hand(&mut self) {
        self.rig.ptt(true).expect("the rigctld takes the key");
        self.state.tx_until_ms = Some(LONG_HOLD);
    }

    /// Where the log and the card stand now, for [`Self::assert_cut`].
    fn mark(&self) -> (usize, usize) {
        (self.log.lock().unwrap().len(), self.backend.flush_calls)
    }

    fn assert_cut(&self, site: &str, (from, flushes): (usize, usize), order: &[&str]) {
        assert_cut(
            site,
            &self.log,
            from,
            flushes,
            &self.backend,
            self.state.tx_until_ms,
            order,
        );
        assert!(!self.rig.keyed, "{site}: the loop believes the rig keyed");
    }
}

// ---- (a) the stream and microphone cuts ----

/// RTTY's abort, behind `rtty_busy_until`: Stop TX mid-over. Flush, then unkey.
#[test]
fn rtty_stop_mid_over_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = Scene::on(14_090_000, |e| {
        e.set_operating_mode("rtty", false);
        e.set_frequency(14.090, "20m", "USB");
        e.rtty_send_text("CQ CQ CQ DE W9XYZ W9XYZ W9XYZ K").unwrap();
    });
    s.run_to(500.0);
    assert!(s.rig.keyed, "premise: the RTTY over keyed");
    assert!(
        s.state.rtty_busy_until > s.t,
        "premise: the over is still keying, so the site's guard holds"
    );
    let mark = s.mark();
    s.engine.lock().unwrap().halt_tx();
    s.tick();
    s.assert_cut("RTTY", mark, &["flush", "T 0"]);
}

/// PSK's abort, behind `psk_busy_until`: Stop TX mid-over. Flush, then unkey.
#[test]
fn psk_stop_mid_over_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = Scene::on(14_070_000, |e| {
        e.set_operating_mode("keyboard", false);
        e.set_frequency(14.070, "20m", "USB");
        e.psk_send_text("CQ CQ CQ DE W9XYZ W9XYZ W9XYZ K").unwrap();
    });
    s.run_to(500.0);
    assert!(s.rig.keyed, "premise: the PSK over keyed");
    assert!(
        s.state.psk_busy_until > s.t,
        "premise: the over is still keying, so the site's guard holds"
    );
    let mark = s.mark();
    s.engine.lock().unwrap().halt_tx();
    s.tick();
    s.assert_cut("PSK", mark, &["flush", "T 0"]);
}

/// The voice keyer's abort (no busy guard: the abort alone): Stop TX mid-message.
#[test]
fn voice_keyer_stop_mid_message_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = Scene::on(14_250_000, |e| {
        e.set_operating_mode("phone", false);
        e.set_frequency(14.250, "20m", "USB");
        e.send_voice(vec![0.05f32; 120_000]).unwrap(); // 10 s
    });
    s.run_to(500.0);
    assert!(s.rig.keyed, "premise: the message keyed");
    assert!(
        s.state.voice_until_ms.is_some_and(|u| u > s.t),
        "premise: the message is still on the air"
    );
    let mark = s.mark();
    s.engine.lock().unwrap().halt_tx();
    s.tick();
    s.assert_cut("voice keyer", mark, &["flush", "T 0"]);
}

/// A Phone station on 20 m, a 60 s picture queued (still going at every step here).
fn sstv_scene() -> Scene {
    Scene::on(14_290_000, |e| {
        e.set_frequency(14.290, "20m", "USB");
        e.set_operating_mode("phone", false);
        e.sstv_send(vec![0.2f32; 720_000], "PD-120".to_string())
            .unwrap();
    })
}

/// SSTV's abort, behind `sstv_feed`: Stop TX mid-picture.
#[test]
fn sstv_stop_mid_picture_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = sstv_scene();
    s.run_to(500.0);
    assert!(s.rig.keyed, "premise: the picture keyed");
    assert!(
        s.state.sstv_feed.is_some(),
        "premise: the picture is feeding"
    );
    let mark = s.mark();
    s.engine.lock().unwrap().halt_tx();
    s.tick();
    s.assert_cut("SSTV Stop TX", mark, &["flush", "T 0"]);
    assert!(
        s.state.sstv_feed.is_none(),
        "SSTV Stop TX: the feed survived"
    );
}

/// SSTV's TX-off cut (the same body, no abort): leaving Phone mid-picture lowers the latch
/// without arming the SSTV abort.
#[test]
fn sstv_leaving_phone_mid_picture_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = sstv_scene();
    s.run_to(500.0);
    assert!(s.rig.keyed, "premise: the picture keyed");
    assert!(
        s.state.sstv_feed.is_some(),
        "premise: the picture is feeding"
    );
    let mark = s.mark();
    s.engine
        .lock()
        .unwrap()
        .set_operating_mode("digital", false);
    s.tick();
    s.assert_cut("SSTV leaving Phone", mark, &["flush", "T 0"]);
    assert!(
        s.state.sstv_feed.is_none(),
        "SSTV leaving Phone: the feed survived"
    );
}

/// SSTV's orphaned feed: another stop already unkeyed and cleared the hold under a live feed.
/// Today it flushes and sends NO second unkey (the double command a switch's handoff exists to
/// prevent), so it is not one of the three-fact cuts: pinned as it is.
#[test]
fn sstv_orphaned_feed_flushes_and_sends_no_unkey() {
    let mut s = sstv_scene();
    s.run_to(500.0);
    assert!(
        s.state.sstv_feed.is_some(),
        "premise: the picture is feeding"
    );
    // The other stop: its own unkey, and the hold gone.
    s.rig.ptt(false).expect("the rigctld takes the unkey");
    s.state.tx_until_ms = None;
    let (from, flushes) = s.mark();
    s.tick();
    assert!(
        s.backend.flush_calls > flushes,
        "the orphan was not flushed"
    );
    assert_eq!(cut_order(&s.log, from), ["flush"], "flush only, no unkey");
    assert!(s.state.sstv_feed.is_none(), "the feed survived");
    assert_eq!(s.state.tx_until_ms, None);
}

/// The Remote microphone's hard end, behind `mic_keyed`: Stop TX mid-over.
#[test]
fn remote_mic_stop_mid_over_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = MicScene::new();
    let (addr, log) = mock_rigctld_on(14_250_000, false);
    {
        let mut e = s.engine.lock().unwrap();
        e.set_license_class("extra");
        e.set_frequency(14.250, "20m", "USB");
    }
    s.rig = Rig::rigctld(&addr);
    s.backend.events = Some(Arc::clone(&log));
    // `MicScene::keyed`, on this rig: press at 0 ms, audio from 20 ms.
    s.hold.hold(MIC_PRESS, s.mono(0.0));
    s.step(0.0);
    let mut t = 20.0;
    while t <= 600.0 {
        s.tick(t, true);
        t += 20.0;
    }
    assert!(s.rig.keyed, "premise: the page's audio keyed the rig");
    assert!(s.state.mic_keyed, "premise: the loop keyed the over");
    let (from, flushes) = (log.lock().unwrap().len(), s.backend.flush_calls);
    s.engine.lock().unwrap().halt_tx();
    s.tick(t, true);
    assert_cut(
        "Remote microphone",
        &log,
        from,
        flushes,
        &s.backend,
        s.state.tx_until_ms,
        &["flush", "T 0"],
    );
    assert!(!s.rig.keyed);
}

// ---- (b) the context teardowns ----

/// The Test-CAT port hold's teardown: flush, unkey through the still-alive link, then the port is
/// handed away (the rig becomes a control-less VOX rig).
#[test]
fn the_test_cat_port_hold_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = Scene::on(14_090_000, |e| e.set_frequency(14.090, "20m", "USB"));
    s.tick();
    s.keyed_by_hand();
    let mark = s.mark();
    s.engine.lock().unwrap().hold_cat_port();
    s.tick();
    s.assert_cut("Test-CAT port hold", mark, &["flush", "T 0"]);
    assert!(s.state.cat_hold_active, "premise: the port was handed away");
    assert!(!s.rig.has_control(), "the rig still holds the CAT link");
    assert!(!s.state.tuning_keyed && !s.state.manual_ptt_applied);
}

/// The audio rebuild's teardown: a new output device saved mid-over. The flush goes to the OLD
/// card, which the rebuild then replaces (`mock_reopen_audio` opens a fresh `MockBackend`, with no
/// flushes and no event log), so fact 1 is read from the old card's `flush` in the log, not from
/// `flush_calls`.
#[test]
fn an_audio_rebuild_mid_over_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = Scene::on(14_090_000, |e| e.set_frequency(14.090, "20m", "USB"));
    s.tick();
    s.keyed_by_hand();
    let (from, _) = s.mark();
    {
        let mut e = s.engine.lock().unwrap();
        let mut st = e.settings().clone();
        st.audio_out = "Different Speakers".to_string();
        e.apply_settings(st);
    }
    s.tick();
    assert!(
        s.backend.events.is_none(),
        "premise: the rebuild replaced the card"
    );
    assert_eq!(
        cut_order(&s.log, from),
        ["flush", "T 0"],
        "audio rebuild: the old card's flush, then the unkey"
    );
    assert_eq!(s.state.tx_until_ms, None, "the PTT deadline survived");
    assert!(!s.rig.keyed, "the loop believes the rig keyed");
}

/// `unkey_before_letting_go` from a CAT rebuild: another CAT port saved mid-over. The old link
/// is unkeyed before it is let go; the reopened rig (`mock_reopen_rig`: VOX) sends nothing.
#[test]
fn a_cat_rebuild_mid_over_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = Scene::on(14_090_000, |e| {
        e.set_frequency(14.090, "20m", "USB");
        let mut st = e.settings().clone();
        st.rig_model = 2014;
        st.ptt_method = "cat".into();
        st.serial_port = "/dev/tempo-test-a".into();
        e.apply_settings(st);
    });
    s.state = {
        let applied = Transport::from_settings(s.engine.lock().unwrap().settings());
        let cfg = RadioConfig {
            rig_model: applied.rig_model,
            ..RadioConfig::default()
        };
        RadioLoop::new(applied, None, &cfg)
    };
    s.tick();
    assert!(s.rig.has_control(), "premise: the loop kept the rigctld");
    s.keyed_by_hand();
    let mark = s.mark();
    {
        let mut e = s.engine.lock().unwrap();
        let mut st = e.settings().clone();
        st.serial_port = "/dev/tempo-test-b".into();
        e.apply_settings(st);
    }
    s.tick();
    assert!(
        !s.rig.has_control(),
        "premise: the loop rebuilt its link (to the mock's VOX rig)"
    );
    s.assert_cut("CAT rebuild", mark, &["flush", "T 0"]);
}

/// `unkey_before_letting_go` from a radio switch the handoff has not handed over yet (the engine
/// flipped the active radio after the handoff read it): the unkey goes out now, through the
/// outgoing radio's live link.
#[test]
fn a_switch_ahead_of_its_handoff_flushes_then_unkeys_and_drops_the_hold() {
    let mut s = Scene::on(14_090_000, |e| e.set_frequency(14.090, "20m", "USB"));
    s.tick();
    s.keyed_by_hand();
    let active = s.engine.lock().unwrap().settings().active_radio;
    s.state.handed_over = Some(active + 1);
    let mark = s.mark();
    s.tick();
    assert!(
        s.state.switch_unhanded,
        "premise: the tick saw a switch not yet handed over"
    );
    s.assert_cut("radio switch ahead of its handoff", mark, &["flush", "T 0"]);
}

/// `stop_for_shutdown`: flush, then unkey, through the still-alive rig, and NOTHING of the loop's
/// TX state cleared (the loop is ending). Pinned as it is: the hold, the tune and the operator's
/// PTT flags all survive the call.
#[test]
fn stop_for_shutdown_flushes_then_unkeys_and_clears_no_state() {
    let mut s = Scene::on(14_090_000, |e| e.set_frequency(14.090, "20m", "USB"));
    s.tick();
    s.keyed_by_hand();
    s.state.tuning_keyed = true;
    s.state.manual_ptt_applied = true;
    s.state.tune_started_ms = Some(100.0);
    let (from, flushes) = s.mark();
    let Scene {
        engine,
        state,
        backend,
        rig,
        ..
    } = &mut s;
    state.stop_for_shutdown(engine, backend, rig);
    assert!(s.backend.flush_calls > flushes, "not flushed");
    assert_eq!(cut_order(&s.log, from), ["flush", "T 0"]);
    assert!(!s.rig.keyed, "the loop believes the rig keyed");
    assert_eq!(
        s.state.tx_until_ms,
        Some(LONG_HOLD),
        "the hold is NOT cleared"
    );
    assert!(s.state.tuning_keyed, "the tune flag is NOT cleared");
    assert!(
        s.state.manual_ptt_applied,
        "the operator's PTT flag is NOT cleared"
    );
    assert_eq!(
        s.state.tune_started_ms,
        Some(100.0),
        "the tune start is NOT cleared"
    );
}

// ---- (c) the hold extends ----

/// A W9XYZ loop on a VOX rig, set up by `setup`, its hold already at `hold`: the hold after the
/// tick that keys the over `setup` queued, and whether that tick played anything.
fn hold_after_keying(hold: f64, setup: impl FnOnce(&mut Engine)) -> (Option<f64>, bool) {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        setup(&mut e);
    }
    let (mut backend, mut rig, mut state) = (MockBackend::new(), Rig::vox(), loop_state());
    state.tx_until_ms = Some(hold);
    let mut t = 100.0;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 100.0);
    (state.tx_until_ms, !backend.played.is_empty())
}

/// PSK's one-shot over keyed under a longer hold leaves that hold; under a shorter one, it
/// extends it to the over's own end.
#[test]
fn a_psk_over_never_shortens_a_longer_hold() {
    let psk = |e: &mut Engine| {
        e.set_operating_mode("keyboard", false);
        e.psk_send_text("CQ").unwrap();
    };
    let (hold, played) = hold_after_keying(LONG_HOLD, psk);
    assert!(played, "premise: the PSK over keyed under the hold");
    assert_eq!(hold, Some(LONG_HOLD), "the longer hold was shortened");
    let (hold, played) = hold_after_keying(150.0, psk);
    assert!(played, "control: the PSK over keyed under a short hold");
    assert!(
        hold.is_some_and(|h| h > 150.0),
        "control: a shorter hold is extended to the over's end: {hold:?}"
    );
}

/// The voice keyer's message keyed under a longer hold leaves that hold; under a shorter one, it
/// extends it to the message's own end.
#[test]
fn a_voice_message_never_shortens_a_longer_hold() {
    let voice = |e: &mut Engine| {
        e.set_operating_mode("phone", false);
        e.set_frequency(14.250, "20m", "USB");
        e.send_voice(vec![0.05f32; 12_000]).unwrap(); // 1 s
    };
    let (hold, played) = hold_after_keying(LONG_HOLD, voice);
    assert!(played, "premise: the message keyed under the hold");
    assert_eq!(hold, Some(LONG_HOLD), "the longer hold was shortened");
    let (hold, played) = hold_after_keying(150.0, voice);
    assert!(played, "control: the message keyed under a short hold");
    assert!(
        hold.is_some_and(|h| h > 150.0),
        "control: a shorter hold is extended to the message's end: {hold:?}"
    );
}

/// APRS SETS its hold (`now + lead + frame + tail`) rather than extending one, and keys only with
/// no hold standing, so its set never meets a hold to replace: a beacon queued under a hold waits
/// and leaves the hold as it was; with none, the set is the frame's own end.
#[test]
fn an_aprs_beacon_sets_its_hold_only_when_none_stands() {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("general");
        e.set_operating_mode("phone", false);
        e.aprs_tune(144.390).unwrap();
    }
    let (mut backend, mut rig, mut state) = (MockBackend::new(), Rig::vox(), loop_state());
    let mut t = 100.0;
    // The tune rebuilds the rig, and that context change empties the queue: settle first.
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 200.0);
    engine
        .lock()
        .unwrap()
        .aprs_beacon(41.88, -87.63, '/', '>', "one", &[])
        .unwrap();
    state.tx_until_ms = Some(LONG_HOLD);
    let until = t;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, until);
    assert!(backend.played.is_empty(), "the beacon keyed under a hold");
    assert_eq!(state.tx_until_ms, Some(LONG_HOLD), "the hold was replaced");
    // The hold gone, the beacon keys and sets its own.
    state.tx_until_ms = None;
    let at = t;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, at);
    assert!(rig.keyed, "control: the beacon keys with no hold");
    let frame_ms = backend.played.len() as f64 / 12.0;
    assert_eq!(
        state.tx_until_ms,
        Some(at + frame_ms + crate::slot::TX_TAIL_MS),
        "the beacon's hold is its frame's end and the tail"
    );
}

// ---- (d) the stream and message hold extends, one pin per site ----
//
// Each site below extends the hold with `extend_hold`. An over keyed while a longer hold stands
// (another over riding the same transmitter) must leave that hold where it was at every tick:
// written as an overwrite, the site would cut the longer over short. The CONTROL, per site: under
// a short hold the same over extends the hold past it, which says the site ran.

/// `setup`'s over on a VOX rig, the hold standing at `hold` from the first tick: the hold after
/// each tick to `until`, the card (what was played) and the loop (the FSK keyer).
fn holds_through(
    hold: f64,
    until: f64,
    setup: impl FnOnce(&mut Engine),
) -> (Vec<Option<f64>>, MockBackend, RadioLoop) {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        setup(&mut e);
    }
    let (mut backend, mut rig, mut state) = (MockBackend::new(), Rig::vox(), loop_state());
    state.tx_until_ms = Some(hold);
    let mut t = 100.0;
    let mut holds = Vec::new();
    while t <= until {
        let at = t;
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, at);
        holds.push(state.tx_until_ms);
    }
    (holds, backend, state)
}

/// The pin and its control for one site: `keyed` says the over went out on that run.
fn assert_never_shortens(
    site: &str,
    until: f64,
    setup: impl Fn(&mut Engine),
    keyed: impl Fn(&MockBackend, &RadioLoop) -> bool,
) {
    let (holds, backend, state) = holds_through(LONG_HOLD, until, &setup);
    assert!(
        keyed(&backend, &state),
        "{site}: premise: the over keyed under the hold"
    );
    if let Some(i) = holds.iter().position(|h| *h != Some(LONG_HOLD)) {
        panic!(
            "{site}: the longer hold was shortened to {:?} at the tick at {} ms",
            holds[i],
            100.0 + 20.0 * i as f64
        );
    }
    let (holds, backend, state) = holds_through(150.0, until, &setup);
    assert!(
        keyed(&backend, &state),
        "{site}: control: the over keyed under a short hold"
    );
    assert!(
        holds.iter().any(|h| h.is_some_and(|h| h > 150.0)),
        "{site}: control: a shorter hold is never extended: {holds:?}"
    );
}

fn played(backend: &MockBackend, _: &RadioLoop) -> bool {
    backend.played.iter().any(|&x| x != 0.0)
}

/// The FSK backend on a keying port that exists only in memory.
#[cfg(feature = "serial")]
fn fsk(e: &mut Engine, port: &str) {
    crate::control_line::fake_ports::install(port);
    let mut s = e.settings().clone();
    s.rtty_backend = "fsk".to_string();
    s.rtty_fsk_port = port.to_string();
    e.apply_settings(s);
}

#[cfg(feature = "serial")]
fn fsk_open(_: &MockBackend, state: &RadioLoop) -> bool {
    state.rtty_keyer.is_some()
}

/// Soundcard CW: each word extends the hold to its end and the tail.
#[test]
fn a_soundcard_cw_word_never_shortens_a_longer_hold() {
    assert_never_shortens(
        "soundcard CW word",
        2_000.0,
        |e| {
            e.set_cw_keyer("soundcard", 600.0);
            e.set_operating_mode("cw", false);
            e.set_frequency(7.03, "40m", "CW");
            e.set_cw_wpm(25);
            e.send_cw("CQ TEST DE");
        },
        played,
    );
}

/// RTTY typed live on the soundcard (AFSK): each chunk extends the hold.
#[test]
fn an_afsk_rtty_stream_never_shortens_a_longer_hold() {
    assert_never_shortens(
        "AFSK RTTY stream",
        1_500.0,
        |e| {
            e.set_operating_mode("rtty", false);
            e.set_rtty_latched(true).unwrap();
            e.rtty_type("CQ CQ").unwrap();
        },
        played,
    );
}

/// An RTTY message on the soundcard (AFSK): its over extends the hold.
#[test]
fn an_afsk_rtty_message_never_shortens_a_longer_hold() {
    assert_never_shortens(
        "AFSK RTTY message",
        1_500.0,
        |e| {
            e.set_operating_mode("rtty", false);
            e.rtty_send_text("CQ TEST").unwrap();
        },
        played,
    );
}

/// RTTY typed live on the FSK keyline: each chunk extends the hold.
#[cfg(feature = "serial")]
#[test]
fn an_fsk_rtty_stream_never_shortens_a_longer_hold() {
    assert_never_shortens(
        "FSK RTTY stream",
        1_500.0,
        |e| {
            fsk(e, "tempo-test-fsk-stream");
            e.set_operating_mode("rtty", false);
            e.set_rtty_latched(true).unwrap();
            e.rtty_type("CQ CQ").unwrap();
        },
        fsk_open,
    );
}

/// An RTTY message on the FSK keyline: its over extends the hold.
#[cfg(feature = "serial")]
#[test]
fn an_fsk_rtty_message_never_shortens_a_longer_hold() {
    assert_never_shortens(
        "FSK RTTY message",
        1_500.0,
        |e| {
            fsk(e, "tempo-test-fsk-message");
            e.set_operating_mode("rtty", false);
            e.rtty_send_text("CQ TEST").unwrap();
        },
        fsk_open,
    );
}

/// PSK typed live: each chunk extends the hold.
#[test]
fn a_psk_stream_never_shortens_a_longer_hold() {
    assert_never_shortens(
        "PSK stream",
        1_500.0,
        |e| {
            e.set_operating_mode("keyboard", false);
            e.set_psk_latched(true).unwrap();
            e.psk_type("CQ CQ").unwrap();
        },
        played,
    );
}
