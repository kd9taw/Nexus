//! ⛔ TEST CAT ON A RADIO WHOSE OPEN NEXUS REFUSED NEVER SWEEPS ITS PORT, WHICHEVER PATH ANSWERS IT.
//! Nexus refuses to launch rigctld on a serial port whose RTS and DTR it cannot hold low (a Hamlib
//! that cannot confirm the holds, `rigctld_proc::HoldsUnconfirmed`), and the open says so, typed
//! (`CatProbe::refused`). Test CAT reads that flag (`Engine::cat_reprobe_refused`) and skips its
//! baud ladder, whose Hamlib rungs run `rigctl` with no `rts_state`/`dtr_state`: a sweep would open,
//! unheld, the very port the refusal kept shut. Test CAT pressed while the loop waits out the
//! reopen backoff is answered by the plain reprobe, not by a reopen; that answer must carry the
//! open's refusal too.
use super::*;

/// An open refused because Hamlib cannot confirm the line holds, built as `open_cat`'s launch
/// error arm builds it.
fn holds_refused_open(_: &Transport, _: bool) -> RigOpen {
    let e = std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        crate::rigctld_proc::HoldsUnconfirmed,
    );
    (
        Rig::vox(),
        None,
        CatProbe {
            refused: crate::rigctld_proc::is_launch_refusal(&e),
            ..CatProbe::status(Some(false), rigctld_launch_failed(&e))
        },
    )
}

/// The launch's open is refused, the first tick reopens and is refused again, the loop then waits
/// out the reopen backoff, and Test CAT is pressed inside it: no second open, and the answer is the refusal, typed, in the open's own words.
#[test]
fn test_cat_inside_the_reopen_backoff_of_a_holds_refused_open_is_refused_typed() {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = 3073; // IC-7300 through Hamlib
        s.serial_port = "/dev/nexus-test-holds-refused".to_string();
        s.ptt_method = "cat".to_string();
        e.apply_settings(s);
    }
    let settings = engine.lock().unwrap().settings().clone();
    let applied = Transport::from_settings(&settings);
    assert!(
        !hamlib_launch_refused(&applied),
        "premise: no declared handshake refusal, so only the open's own refusal can say it"
    );
    let cfg = RadioConfig {
        rig_model: settings.rig_model,
        ..RadioConfig::default()
    };
    // The launch's open, refused, as `run_radio_loop` takes it.
    let (mut rig, daemon, probe) = holds_refused_open(&applied, true);
    engine
        .lock()
        .unwrap()
        .set_cat_status(probe.ok, probe.detail);
    let mut state = RadioLoop::new(applied, daemon, &cfg);
    state.after_the_launch_open(&rig, probe.ok);
    // The port's presence is not this test's question.
    state.cat_port_check_at = f64::MAX;
    let opens = std::cell::Cell::new(0);
    // While set, a reopen is refused for the holds; cleared, it fails as any launch can (Hamlib
    // missing), which Nexus did not refuse.
    let holds_refused = std::cell::Cell::new(true);
    let mut reopen_rig = |t: &Transport, coexist: bool| {
        opens.set(opens.get() + 1);
        if holds_refused.get() {
            holds_refused_open(t, coexist)
        } else {
            let e = std::io::Error::new(std::io::ErrorKind::NotFound, "rigctld not found");
            (
                Rig::vox(),
                None,
                CatProbe::status(Some(false), rigctld_launch_failed(&e)),
            )
        }
    };
    let step = |state: &mut RadioLoop,
                rig: &mut Rig,
                reopen: &mut dyn FnMut(&Transport, bool) -> RigOpen,
                now: f64| {
        state
            .step(
                &engine,
                &mut MockBackend::new(),
                rig,
                &no_sinks(),
                now,
                &mut mock_reopen_audio(),
                reopen,
                &mut StationSinks::new(),
            )
            .unwrap();
    };
    // The reopen is due at once: it runs, and is refused.
    step(&mut state, &mut rig, &mut reopen_rig, 1_000.0);
    // The open's own words (the status line itself is the tick's, which a later note overwrites).
    let refusal = holds_refused_open(&state.applied, false).2.detail;
    assert!(
        refusal.contains(crate::rigctld_proc::HAMLIB_CANNOT_CONFIRM_HOLDS),
        "premise: the refusal is the holds one: {refusal:?}"
    );
    assert_eq!(opens.get(), 1, "premise: the reopen ran, and was refused");
    assert!(
        state.cat_reopen_at > 1_020.0,
        "premise: the next reopen waits"
    );
    // Test CAT, inside the backoff.
    let asked = engine.lock().unwrap().request_cat_reprobe();
    step(&mut state, &mut rig, &mut reopen_rig, 1_020.0);
    let (answer, refused) = {
        let e = engine.lock().unwrap();
        (e.cat_reprobe_answer(asked), e.cat_reprobe_refused())
    };
    assert_eq!(
        opens.get(),
        1,
        "premise: the reprobe answered, not a second open"
    );
    assert_eq!(
        (answer, refused),
        (Some((Some(false), refusal)), true),
        "Test CAT inside the backoff must answer the open's refusal, typed, so the ladder never \
         sweeps the port unheld"
    );
    // CONTROL: the next reopen fails, but not by Nexus's refusal, and that refusal goes with it:
    // a Test CAT inside the new backoff is the plain failure, untyped, as any failing Test CAT is.
    holds_refused.set(false);
    let due = state.cat_reopen_at;
    step(&mut state, &mut rig, &mut reopen_rig, due);
    assert_eq!(opens.get(), 2, "premise: the second reopen ran");
    let asked = engine.lock().unwrap().request_cat_reprobe();
    step(&mut state, &mut rig, &mut reopen_rig, due + 20.0);
    let e = engine.lock().unwrap();
    assert_eq!(opens.get(), 2, "premise: the reprobe answered again");
    let (ok, detail) = e.cat_reprobe_answer(asked).expect("answered");
    assert_eq!(ok, Some(false));
    assert!(
        !e.cat_reprobe_refused() && !detail.contains("Nexus did not open"),
        "a refusal must not outlive the open it belonged to: {detail:?}"
    );
}
