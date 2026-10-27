//! The SDRconnect scope source in the radio loop: chosen in Settings, the loop starts it on the
//! operator's address, tells it the rig's dial so the receiver follows it, says its trouble on the
//! scope's status line, and stops it (IQ off, RF slot back to the radio) when the scope source is
//! the radio again.
//!
//! SDRconnect is the fake from `crate::sdrconnect::fake` on loopback, written from the published
//! "SDRconnect WebSocket API 1.0.3".

use super::*;
use crate::sdrconnect::fake::{Config as FakeConfig, Fake};

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(10);

fn engine_with(source: &str, port: u16, dial_mhz: f64) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.scope_source = source.into();
        s.sdrconnect_host = "127.0.0.1".into();
        s.sdrconnect_port = port;
        s.dial_mhz = dial_mhz;
        e.apply_settings(s);
    }
    engine
}

fn scope_error(engine: &Arc<Mutex<Engine>>) -> Option<String> {
    engine.lock().unwrap().snapshot().radio.scope_error
}

/// Reconcile every 20 ms, as the loop's ticks do, until `done`, or fail saying `what`.
fn tick_until(
    state: &mut RadioLoop,
    engine: &Arc<Mutex<Engine>>,
    what: &str,
    done: impl Fn(&RadioLoop) -> bool,
) {
    let deadline = Instant::now() + WAIT;
    loop {
        state.reconcile_sdrconnect(engine);
        if done(state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what}: {:?}",
            state.sdrconnect.as_ref().map(|(_, _, s)| s.status())
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// ⭐ CHOSEN, THE LOOP DRAWS SDRCONNECT AND THE RECEIVER FOLLOWS THE DIAL; BACK TO THE RADIO, IT
/// STOPS: the scope starts on the operator's address, the receiver is retuned to the rig's dial
/// and SDRconnect's rows fill the RF slot while the radio's own are kept out; the radio chosen
/// again sends `iq_stream_enable false` and hands the slot back.
#[test]
fn the_loop_draws_sdrconnect_while_chosen_and_follows_the_dial() {
    let fake = Fake::start(FakeConfig {
        carriers: vec![7_074_500],
        ..FakeConfig::default()
    });
    let engine = engine_with("sdrconnect", fake.addr().port(), 7.074);
    let mut state = loop_state();
    tick_until(&mut state, &engine, "drawing on the dial", |s| {
        s.sdrconnect.as_ref().is_some_and(|(_, _, scope)| {
            let st = scope.status();
            st.centre == Some(7_074_000) && st.rows > 2
        })
    });
    assert_eq!(
        fake.centre(),
        7_074_000,
        "the receiver was retuned to the dial"
    );
    let row = state.spectrum_feed.row().expect("a row");
    assert_eq!(row.source, crate::sdrconnect::SOURCE);
    assert!(row.lo_hz < 7_074_000.0 && row.hi_hz > 7_074_000.0);
    state.spectrum_feed.publish_rf(tempo_app::dto::Spectrum {
        row: vec![0.9],
        lo_hz: 0.0,
        hi_hz: 1.0,
        source: "civ".into(),
    });
    assert_eq!(
        state.spectrum_feed.row().map(|r| r.source),
        Some(crate::sdrconnect::SOURCE.to_string()),
        "the radio's own panadapter is not shown while SDRconnect is the scope source"
    );
    assert_eq!(scope_error(&engine), None);
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.scope_source = "radio".into();
        e.apply_settings(s);
    }
    state.reconcile_sdrconnect(&engine);
    assert!(state.sdrconnect.is_none());
    let deadline = Instant::now() + WAIT;
    while !fake
        .received()
        .iter()
        .any(|m| m.contains("iq_stream_enable") && m.contains("\"false\""))
    {
        assert!(Instant::now() < deadline, "the IQ is stopped");
        std::thread::sleep(Duration::from_millis(10));
    }
    state.spectrum_feed.publish_rf(tempo_app::dto::Spectrum {
        row: vec![0.9],
        lo_hz: 0.0,
        hi_hz: 1.0,
        source: "civ".into(),
    });
    assert_eq!(
        state.spectrum_feed.row().map(|r| r.source),
        Some("civ".to_string()),
        "the radio's panadapter is the scope's again"
    );
}

/// SDRconnect away is said on the scope's status line with the operator's address, and the line
/// is cleared when the scope source goes back to the radio; the radio's source never starts the
/// scope at all.
#[test]
fn sdrconnect_away_is_said_on_the_status_line_and_cleared_with_the_radio() {
    let port = Fake::start(FakeConfig::default()).addr().port();
    let engine = engine_with("radio", port, 14.074);
    let mut state = loop_state();
    state.reconcile_sdrconnect(&engine);
    assert!(
        state.sdrconnect.is_none(),
        "the radio's own scope as it comes"
    );
    assert_eq!(scope_error(&engine), None);
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.scope_source = "sdrconnect".into();
        e.apply_settings(s);
    }
    tick_until(&mut state, &engine, "said", |_| {
        scope_error(&engine).is_some()
    });
    let said = scope_error(&engine).expect("said");
    assert!(
        said.starts_with(&format!("SDRconnect is not answering at 127.0.0.1:{port}")),
        "{said}"
    );
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.scope_source = "radio".into();
        e.apply_settings(s);
    }
    state.reconcile_sdrconnect(&engine);
    assert_eq!(scope_error(&engine), None);
}
