//! The ALC meter of a Yaesu FT-991/991A (Hamlib model 1035), FT-710 (1049) and FTDX10 (1042), on
//! the radio's own scale.
//!
//! Both radios give their ALC meter as 0 to 255. Yaesu's FT-991A CAT manual, `RM` READ METER:
//! `RM4;` is answered `RM4` and three digits. Its FT-710 CAT manual: the same three digits, then a
//! fixed `000`. Hamlib 4.7.1's `l ALC` sends that same `RM4;`, but neither radio's backend has an
//! ALC table, so it scales with newcat's default, raw 64 = 1.0 and clamped there. The meter showed
//! four times the radio's reading, and full from a quarter of the radio's bar up.
//!
//! The rigctld here answers both ways of asking as Hamlib and the radio do (see [`alc_rigctld`]).
//! The loop is the real `RadioLoop::step`, with the radio keyed at the rig, the mic-PTT scene of
//! `a_rig_that_answers_its_transmit_meters_keeps_being_polled`. What is asserted is what the meter
//! is given: the engine's `tx_alc`, by value.
use super::*;

/// What Hamlib 4.7.1 answers `l ALC` with on a Yaesu whose backend has no ALC table: newcat's
/// `yaesu_default_alc_cal`, raw 0 = 0.0 and raw 64 = 1.0, clamped past its last point.
fn hamlib_alc(raw: u16) -> f32 {
    (f32::from(raw) / 64.0).min(1.0)
}

/// A rigctld for a radio keyed at the rig (`t` answers 1) whose ALC meter reads `raw`, 0 to 255.
///
/// - `w RM4;` gets the radio's own reply: `RM4`, `raw` in three digits, `p3` (the FT-710's fixed
///   `000`, empty for an FT-991A) and `;`, then a NUL and no newline. That is the shape rigctld's
///   send_cmd hands back (`a_nul_terminated_raw_cat_reply_is_read_instead_of_timing_out`).
/// - `l ALC` gets [`hamlib_alc`] of the same reading, six decimals as rigctld prints a level.
/// - Every other line gets `RPRT 0`, no reading, so the other meters drop out after their three
///   strikes and the rest of the over is ALC reads.
fn alc_rigctld(raw: u16, p3: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
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
                    "f" => "14074000\n".to_string(),
                    "t" => "1\n".to_string(), // the operator is holding the mic key
                    "w RM4;" => format!("RM4{raw:03}{p3};\0"),
                    "l ALC" => format!("{:.6}\n", hamlib_alc(raw)),
                    _ => "RPRT 0\n".to_string(),
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// Four seconds of an over on a radio of Hamlib `model` whose ALC reads `raw`: what the meter is
/// given, and every line the radio's rigctld was sent.
fn alc_after_an_over(model: u32, raw: u16, p3: &'static str) -> (Option<f32>, Vec<String>) {
    let (radio, sent) = radio_after_an_over(model, raw, p3);
    (radio.tx_alc, sent)
}

/// [`alc_after_an_over`], with the whole radio status the meters are drawn from.
fn radio_after_an_over(
    model: u32,
    raw: u16,
    p3: &'static str,
) -> (tempo_app::dto::RadioStatus, Vec<String>) {
    let (addr, log) = alc_rigctld(raw, p3);
    let radio = run_an_over(&engine_for(model), &addr);
    let sent = log.lock().unwrap().clone();
    (radio, sent)
}

/// An engine set to Hamlib `model`.
fn engine_for(model: u32) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        e.apply_settings(s);
    }
    engine
}

/// Four seconds of an over through the rigctld at `addr`: the radio status the meters are drawn
/// from.
fn run_an_over(engine: &Arc<Mutex<Engine>>, addr: &str) -> tempo_app::dto::RadioStatus {
    let model = engine.lock().unwrap().settings().rig_model;
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
        "model {model}: control: the radio must be keyed, or no meter is read at all"
    );
    engine.lock().unwrap().snapshot().radio
}

/// ⭐ THE FT-991A, THE FT-710 AND THE FTDX10 SHOW THE RADIO'S OWN BAR. Raw 64 is a quarter of the radio's bar
/// and 128 half of it; Nexus showed both as 100 %, red. Asserted by value first, every reading
/// against the bar position, raw / 255, for each radio's own reply shape; then how it was asked.
/// The FTDX10's CAT manual gives the FT-710's reply shape and range, and Hamlib 4.7.1's `ftdx10.c`
/// has no ALC table either, so it showed the same four times.
#[test]
fn an_ft991a_an_ft710_and_an_ftdx10_show_alc_where_the_radios_own_bar_is() {
    for (model, name, p3) in [
        (1035u32, "FT-991A", ""),
        (1049, "FT-710", "000"),
        (1042, "FTDX10", "000"),
    ] {
        let overs: Vec<(u16, Option<f32>, Vec<String>)> = [0u16, 64, 128, 255]
            .into_iter()
            .map(|raw| {
                let (alc, sent) = alc_after_an_over(model, raw, p3);
                (raw, alc, sent)
            })
            .collect();
        let table: Vec<String> = overs
            .iter()
            .map(|(raw, alc, _)| {
                let shown = alc.map_or("nothing".to_string(), |a| format!("{:.1} %", a * 100.0));
                format!(
                    "raw {raw}: bar {:.1} %, meter {shown}",
                    f32::from(*raw) / 2.55
                )
            })
            .collect();
        for (raw, alc, _) in &overs {
            let bar = f32::from(*raw) / 255.0;
            assert!(
                alc.is_some_and(|a| (a - bar).abs() < 1e-6),
                "{name}, raw {raw}: the meter must show where the radio's bar is — {table:?}"
            );
        }
        for (_, _, sent) in &overs {
            assert!(
                sent.iter().any(|l| l == "w RM4;"),
                "{name}: the radio's own ALC value is read — {sent:?}"
            );
            assert!(
                !sent.iter().any(|l| l == "l ALC"),
                "{name}: Hamlib's scaled ALC is not asked for — {sent:?}"
            );
            for meter in ["SWR", "RFPOWER_METER_WATTS", "COMP_METER"] {
                assert!(
                    sent.iter().any(|l| *l == format!("l {meter}")),
                    "{name}: {meter} is still read through Hamlib — {sent:?}"
                );
            }
        }
    }
}

/// A rigctld from before Hamlib 4.6 in front of an FT-991 whose ALC meter reads `raw`, 0 to 255.
///
/// Measured against the real Hamlib 4.5.5 `rigctld -m 1035`, the radio answering `RM4;` with
/// `RM4100;`: `w RM4;` comes back with NOTHING. Its send_cmd waits for a CR or LF the radio never
/// sends, gives up after the FT-991's 2 s timeout and prints no reply at all (4.6 added the `;`
/// terminator for Yaesu and Kenwood; 4.7.1 has it). `l ALC` answers at once, on Hamlib's scale.
/// Every other line gets `RPRT 0`, as in [`alc_rigctld`].
fn hamlib_45_rigctld(raw: u16) -> (String, Arc<Mutex<Vec<String>>>) {
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
                    "f" => "14074000\n".to_string(),
                    "t" => "1\n".to_string(),
                    "w RM4;" => continue, // no reply at all
                    "l ALC" => format!("{:.6}\n", hamlib_alc(raw)),
                    _ => "RPRT 0\n".to_string(),
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// ⭐ AN FT-991 BEHIND ITS OWN OLDER RIGCTLD KEEPS ITS ALC METER (the 1.19.0 regression). Network
/// connection, `127.0.0.1:4532`, where the operator's own rigctld runs: Nexus shares it, and a
/// Hamlib before 4.6 never answers the raw `w RM4;`. Three misses parked the meter, ⊘ "not
/// reported", where 1.18.0's `l ALC` read it. The raw read that never answered on this link gives
/// way to `l ALC`, on Hamlib's scale and so with no zone of the radio's own. By value: raw 32 is
/// 0.5 on Hamlib's scale and 0.125 on the radio's.
#[test]
fn an_ft991_behind_a_rigctld_that_never_answers_raw_cat_reads_alc_through_hamlib() {
    let (addr, log) = hamlib_45_rigctld(32);
    let radio = run_an_over(&engine_for(1035), &addr);
    let sent = log.lock().unwrap().clone();
    assert_eq!(
        (radio.tx_alc, radio.tx_alc_zone),
        (Some(0.5), None),
        "the meter shows Hamlib's ALC, with Hamlib's zone — {sent:?}"
    );
    let first_l_alc = sent.iter().position(|l| l == "l ALC");
    let raw_asks = sent.iter().filter(|l| *l == "w RM4;").count();
    assert_eq!(
        raw_asks, 3,
        "the raw read is tried, three strikes as every meter gets — {sent:?}"
    );
    assert!(
        first_l_alc.is_some_and(|at| !sent[at..].iter().any(|l| l == "w RM4;")),
        "once it has given way, the raw read is not sent again — {sent:?}"
    );
}

/// …and every other radio reads its ALC as it did: another Yaesu (the FTDX101D, whose Hamlib
/// backend has an ALC table of its own) and an Icom (the IC-7300 through Hamlib). Whatever `l ALC` answers is what the meter
/// is given, and the radio is sent no `RM4;`, which on an Icom's CI-V bus is junk. By value
/// first, every reading, as above: raw 0 reads 0 on either scale, so it proves nothing alone.
#[test]
fn every_other_radios_alc_still_reads_through_hamlib() {
    for (model, name) in [(1040u32, "FTDX101D"), (3073, "IC-7300")] {
        let overs: Vec<(u16, Option<f32>, Vec<String>)> = [0u16, 64, 128, 255]
            .into_iter()
            .map(|raw| {
                let (alc, sent) = alc_after_an_over(model, raw, "000");
                (raw, alc, sent)
            })
            .collect();
        let got: Vec<(u16, Option<f32>)> = overs.iter().map(|(raw, alc, _)| (*raw, *alc)).collect();
        let want: Vec<(u16, Option<f32>)> = overs
            .iter()
            .map(|(raw, _, _)| (*raw, Some(hamlib_alc(*raw))))
            .collect();
        assert_eq!(
            got, want,
            "{name}: unchanged, the meter shows Hamlib's own answer (raw, meter)"
        );
        for (_, _, sent) in &overs {
            assert!(
                sent.iter().any(|l| l == "l ALC"),
                "{name}: ALC is still read through Hamlib — {sent:?}"
            );
            assert!(
                !sent.iter().any(|l| l == "w RM4;"),
                "{name}: no raw RM4 is sent to it — {sent:?}"
            );
        }
    }
}

/// A rigctld whose radio answers every line with `RM4064;` (and the NUL), but only after `late`.
fn late_rm4_rigctld(late: std::time::Duration) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
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
                std::thread::sleep(late);
                if stream.write_all(b"RM4064;\0").is_err() {
                    break;
                }
            }
        }
    });
    addr
}

/// A tune-up's ALC read is bounded on these radios too. A meter read blocks the loop while the
/// tune carrier plays out of a short lead, so the poll passes the tune's deadline
/// (`TUNE_METER_DEADLINE_MS`) and an answer later than that is given up on. The same late answer
/// IS read with the normal deadline, the control that the fake radio answers at all. A slow
/// transport, so the normal deadline (2.5 s) is far from the late answer (1 s).
#[test]
fn a_tune_ups_alc_read_gives_up_at_the_tune_deadline() {
    let mut rig = Rig::rigctld(&late_rm4_rigctld(std::time::Duration::from_millis(1_000)));
    rig.set_slow_transport(true);
    assert_eq!(
        read_tx_meter(&mut rig, "ALC", 1035, true, None),
        Some(64.0 / 255.0),
        "control: with the normal deadline the late answer is read"
    );
    assert_eq!(
        read_tx_meter(&mut rig, "ALC", 1035, true, Some(TUNE_METER_DEADLINE_MS)),
        None,
        "a tune-up does not wait a second for its ALC"
    );
}

/// The ALC reading carries where the radio's own ALC zone ends, so the bar's colours sit where
/// the radio's meter marks it. The FT-991A's manual puts the zone at "up to half scale deflection"
/// (Operating Manual p. 61), and the FT-710's meter marks the same zone (p. 40); the FTDX10 is read
/// the same way. Every other radio's 1.0 is the top of its zone, as before: no zone is carried.
#[test]
fn the_alc_reading_carries_the_radios_own_zone_edge() {
    let zones: Vec<(&str, Option<f32>, Option<f32>)> = [
        (1035u32, "FT-991A", ""),
        (1049, "FT-710", "000"),
        (1042, "FTDX10", "000"),
        (3073, "IC-7300", ""),
    ]
    .into_iter()
    .map(|(model, name, p3)| {
        let (radio, _) = radio_after_an_over(model, 128, p3);
        (
            name,
            radio.tx_alc.map(|a| (a * 255.0).round()),
            radio.tx_alc_zone,
        )
    })
    .collect();
    assert_eq!(
        zones,
        vec![
            ("FT-991A", Some(128.0), Some(0.5)),
            ("FT-710", Some(128.0), Some(0.5)),
            ("FTDX10", Some(128.0), Some(0.5)),
            // Hamlib's scale for the IC-7300: raw 128 is past 64, so 1.0, and no zone of its own.
            ("IC-7300", Some(255.0), None),
        ]
    );
}
