//! Where the rotator is pointing, for a browser that has a heading on screen.
//!
//! The desktop's own readout is `read_rotator`: resolve the rotctld address from Settings, ask the
//! daemon, and show "—" when there is no answer. This is that read and nothing else — no address,
//! no model, no port and no file path leaves here, and there is no write of any kind.
//!
//! ⛔ **UNKNOWN IS NOT ZERO.** A station with no rotator configured, and one whose rotctld does not
//! answer, both read as `azimuthDeg: null`; `configured` tells the browser which of the two it is,
//! so the strip can say "not answering" where the desktop says it. A fabricated bearing is the
//! same class of defect as a dial that reads 0 Hz because nothing was read — and a mast the
//! operator cannot see from the shack is exactly where a confident wrong number does harm.
//!
//! Nothing here runs on a timer. The read happens only when a browser asks, on the blocking query
//! worker (never the live-instrument loop), and `Publisher::read` shares one capture across every
//! observer for the desktop's own 2 s poll interval, so two browsers watching cost one rotctld
//! exchange rather than two. The exchange itself is bounded by `tempo_audio::rotator`'s own
//! connect and poll deadlines.
use serde_json::{json, Value};
use std::sync::TryLockError;

pub(super) fn read(
    engine: &crate::SharedEngine,
) -> Result<(Vec<Value>, usize, Value), &'static str> {
    let addr = match tempo_app::engine::engine_try_lock(engine) {
        Ok(e) => crate::effective_rotator_addr(e.settings()),
        Err(TryLockError::WouldBlock) => return Err("applicationBusy"),
        Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
    };
    // The Engine lock is released before the daemon is asked: a rotctld that has gone quiet must
    // never park the station's engine behind it.
    let Some(addr) = addr else {
        return Ok((
            Vec::new(),
            0,
            json!({ "configured": false, "azimuthDeg": Value::Null }),
        ));
    };
    let azimuth = azimuth(&addr).filter(|az| az.is_finite() && (0.0..360.0).contains(az));
    Ok((
        Vec::new(),
        0,
        json!({ "configured": true, "azimuthDeg": azimuth }),
    ))
}

#[cfg(all(not(test), feature = "radio"))]
fn azimuth(addr: &str) -> Option<f64> {
    tempo_audio::rotator::read_azimuth(addr)
}

/// Without the radio feature there is no rotctld client compiled in at all, so the station is
/// honest about having no reading rather than inventing one.
#[cfg(all(not(test), not(feature = "radio")))]
fn azimuth(_addr: &str) -> Option<f64> {
    None
}

/// Under test the reading comes from an in-process fake at that address; an address with no fake
/// reads as no answer, so a test can never open a socket to a real rotator.
#[cfg(test)]
fn azimuth(addr: &str) -> Option<f64> {
    test_rotor::read(addr)
}

#[cfg(test)]
pub mod test_rotor {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static FAKES: Mutex<Option<HashMap<String, Option<f64>>>> = Mutex::new(None);

    /// Stage what the rotctld at `addr` answers: `Some(az)` a bearing, `None` silence.
    pub fn install(addr: &str, azimuth: Option<f64>) {
        FAKES
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(addr.into(), azimuth);
    }

    pub(super) fn read(addr: &str) -> Option<f64> {
        FAKES.lock().unwrap().as_ref()?.get(addr).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Publisher, Request};
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    const ID: &str = "10000000-0000-4000-8000-000000000001";

    fn engine(host: &str) -> crate::SharedEngine {
        let settings = tempo_app::settings::Settings {
            rotator_host: host.into(),
            ..Default::default()
        };
        Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            settings,
        )))
    }

    fn page(engine: &crate::SharedEngine) -> Value {
        let request: Request =
            serde_json::from_value(json!({ "requestId": ID, "collection": "rotator",
            "cursor": null, "search": "", "unconfirmed": false, "after": null }))
            .unwrap();
        assert!(request.valid());
        serde_json::from_str(
            &Publisher::default()
                .read(&request, engine, None, Instant::now())
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_station_with_no_rotator_reads_as_unconfigured_and_never_as_a_bearing() {
        let value = page(&engine(""));
        assert_eq!(value["meta"]["source"]["configured"], json!(false));
        assert_eq!(value["meta"]["source"]["azimuthDeg"], Value::Null);
        assert_eq!(value["rows"], json!([]));
        assert_eq!(value["total"], 0);
    }

    #[test]
    fn a_configured_rotator_reads_its_bearing_and_a_silent_one_reads_unknown() {
        // Tests share one process and run in parallel, so each stages its own address.
        test_rotor::install("127.0.0.1:14533", Some(212.5));
        let live = page(&engine("127.0.0.1:14533"));
        assert_eq!(live["meta"]["source"]["configured"], json!(true));
        assert_eq!(live["meta"]["source"]["azimuthDeg"], json!(212.5));
        // The same station with the daemon silent: UNKNOWN, never the zero a missing read would be.
        test_rotor::install("127.0.0.1:14533", None);
        let silent = page(&engine("127.0.0.1:14533"));
        assert_eq!(silent["meta"]["source"]["configured"], json!(true));
        assert_eq!(silent["meta"]["source"]["azimuthDeg"], Value::Null);
        // A daemon answering nonsense is unknown too: out of range is not a bearing.
        for bad in [f64::NAN, -1.0, 360.0, f64::INFINITY] {
            test_rotor::install("127.0.0.1:14533", Some(bad));
            assert_eq!(
                page(&engine("127.0.0.1:14533"))["meta"]["source"]["azimuthDeg"],
                Value::Null,
                "{bad}"
            );
        }
    }

    #[test]
    fn the_read_is_argument_free_and_a_busy_engine_is_not_an_unconfigured_rotator() {
        let request = |patch: (&str, Value)| {
            let mut value = json!({ "requestId": ID, "collection": "rotator", "cursor": null, "search": "", "unconfirmed": false, "after": null });
            value[patch.0] = patch.1;
            serde_json::from_value::<Request>(value)
        };
        assert!(request(("search", json!(""))).unwrap().valid());
        for patch in [
            ("search", json!("W1AW")),
            ("unconfirmed", json!(true)),
            ("after", json!(1)),
            ("cursor", json!(format!("{ID}:1"))),
        ] {
            assert!(!request(patch.clone()).unwrap().valid(), "{patch:?}");
        }
        assert!(request(("azimuthDeg", json!(0))).is_err());
        let engine = engine("127.0.0.1:14534");
        let held = engine.lock().unwrap();
        assert_eq!(read(&engine), Err("applicationBusy"));
        drop(held);
        // Positive control: the same read answers once the lock is free.
        assert!(read(&engine).is_ok());
    }
}
