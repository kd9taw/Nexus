//! Forward power on a radio whose Hamlib backend gives no watts (#489): the FT-450D, Hamlib model
//! 1046, on a direct serial link.
//!
//! The keyed poll reads Po with `l RFPOWER_METER_WATTS`. Hamlib 4.7.1's FT-450D declares
//! `RFPOWER_METER` and not `RFPOWER_METER_WATTS` (`rigctl -m 1046 --dump-caps`), so rigctld refuses
//! the read with `RPRT -1` before the radio is asked (`tests/rigctl_parse.c`, `get_level`: a level
//! the rig does not declare, and no extra level by that name, is `-RIG_EINVAL`; measured: a real
//! `rigctld -m 1046` answers `l RFPOWER_METER_WATTS` with `RPRT -1`). Three strikes, and the meter
//! was parked: no PWR, ever. Seven more
//! catalog models are in the same place (FT-450 1027, TS-2000 2014, TS-480 2028, SDRuno 2051,
//! QCX/QDX 2052, (tr)uSDX 2055, SDR Console 2056).
//!
//! Such a radio's Po is read with `l RFPOWER_METER`, a share of the radio's meter, and shown as
//! that share of the radio's rated watts, marked as derived. The rigctld here answers as Hamlib
//! does; the loop is the real `RadioLoop::step` with the radio keyed at the rig (the scene of
//! `yaesu_alc.rs`). What is asserted is what the meter is given, by value, and what was asked.
use super::*;

/// A rigctld for a radio keyed at the rig (`t` answers 1). `watts` is its answer to
/// `l RFPOWER_METER_WATTS`, `meter` its answer to `l RFPOWER_METER`. Every other line gets
/// `RPRT 0`, no reading, so the other meters drop out after their three strikes.
fn po_rigctld(watts: &'static str, meter: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(r) => r,
                Err(_) => continue,
            });
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let l = line.trim().to_string();
                log2.lock().unwrap().push(l.clone());
                let reply = match l.as_str() {
                    "f" => "14074000\n",
                    "t" => "1\n", // the operator is holding the mic key
                    "l RFPOWER_METER_WATTS" => watts,
                    "l RFPOWER_METER" => meter,
                    _ => "RPRT 0\n",
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// An engine set to Hamlib `model`, its radio rated at `rated_watts`.
fn engine_for(model: u32, rated_watts: u32) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        s.rated_watts = rated_watts;
        for p in &mut s.radios {
            p.rated_watts = rated_watts;
        }
        e.apply_settings(s);
    }
    engine
}

/// Four seconds of an over on the rigctld at `addr`: the radio status, and every line sent.
fn an_over(engine: &Arc<Mutex<Engine>>, addr: &str) -> tempo_app::dto::RadioStatus {
    let mut rig = Rig::rigctld(addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let mut t = 0.0;
    for _ in 0..80 {
        t += 50.0;
        state
            .step(
                engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    }
    assert!(
        state.rig_keyed,
        "control: the radio must be keyed, or no meter is read at all"
    );
    engine.lock().unwrap().snapshot().radio
}

/// ⭐ THE FT-450D SHOWS ITS POWER. rigctld refuses the watts; the radio's meter reads 0.45 of its
/// scale; the radio is rated 100 W, so the meter shows 45 W, marked derived. A 200 W rating on
/// the same reading shows 90 W: the figure is the rating's share, not Hamlib's watts.
#[test]
fn a_radio_without_a_watts_reading_shows_its_meter_share_of_its_rated_watts() {
    // `RPRT -1` is rigctld's own refusal (measured); `-11` is a backend's "not available".
    for (refusal, rated, want) in [
        ("RPRT -1\n", 100u32, 45.0f32),
        ("RPRT -1\n", 200, 90.0),
        ("RPRT -11\n", 100, 45.0),
    ] {
        let (addr, log) = po_rigctld(refusal, "0.450000\n");
        let radio = an_over(&engine_for(1046, rated), &addr);
        let sent = log.lock().unwrap().clone();
        assert!(
            radio.tx_po_w.is_some_and(|w| (w - want).abs() < 1e-3),
            "{refusal:?}, rated {rated} W: the meter shows {want} W, got {:?} — {sent:?}",
            radio.tx_po_w
        );
        assert!(
            radio.tx_po_derived,
            "rated {rated} W: the figure is marked derived"
        );
        assert_eq!(
            sent.iter()
                .filter(|l| *l == "l RFPOWER_METER_WATTS")
                .count(),
            1,
            "the refused watts read is not asked again — {sent:?}"
        );
        assert!(
            sent.iter().any(|l| l == "l RFPOWER_METER"),
            "the radio's own meter is read instead — {sent:?}"
        );
    }
}

/// …and a radio that answers watts is read exactly as before: its own watts, not marked derived,
/// and never asked for `RFPOWER_METER`.
#[test]
fn a_radio_that_reports_watts_is_unchanged() {
    let (addr, log) = po_rigctld("45.000000\n", "0.450000\n");
    let radio = an_over(&engine_for(1035, 100), &addr);
    let sent = log.lock().unwrap().clone();
    assert_eq!(radio.tx_po_w, Some(45.0), "{sent:?}");
    assert!(
        !radio.tx_po_derived,
        "the radio's own watts are not derived"
    );
    assert!(
        !sent.iter().any(|l| l == "l RFPOWER_METER"),
        "no other power read is sent — {sent:?}"
    );
}

/// A watts read that merely goes unanswered is a miss, not a refusal: no number, or Hamlib's
/// timeout (`RPRT -5`). The meter is parked after its three strikes as it always was, and nothing
/// else is asked in its place.
#[test]
fn a_watts_read_that_is_not_answered_is_not_a_refusal() {
    for miss in ["RPRT 0\n", "RPRT -5\n"] {
        let (addr, log) = po_rigctld(miss, "0.450000\n");
        let radio = an_over(&engine_for(1046, 100), &addr);
        let sent = log.lock().unwrap().clone();
        assert_eq!(radio.tx_po_w, None, "{miss:?}: {sent:?}");
        assert!(
            !sent.iter().any(|l| l == "l RFPOWER_METER"),
            "{miss:?}: no other power read is sent — {sent:?}"
        );
    }
}
