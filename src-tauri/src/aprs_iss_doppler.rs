//! The APRS cockpit's ISS channel: where the ISS is from this station, once a second, for the
//! engine's Doppler correction (`tempo_app::engine::aprs_iss`).
//!
//! A thread of its own ("nexus-aprs-iss") wakes every [`EVERY`]. While the ISS channel is the APRS
//! pick in force (`Engine::aprs_iss_wants_sky`) it looks the ISS up in the satellite tracker's own
//! element store ([`crate::tle_snapshot`], by NORAD number, under the same 30-day acting limit
//! every other tuning surface uses, [`crate::tle_act_gate`]) and runs the tracker's SGP4 geometry
//! ([`propagation::sat::look_at`], [`propagation::sat::range_rate`]) from the operator's grid,
//! with the engine lock released. The engine decides what, if anything, the radio is told; this
//! thread never writes to a radio. Off the ISS channel it only reads one flag a second.

use std::time::{Duration, Instant};

use propagation::sat::{self, Tle};
use tempo_app::engine::aprs_iss::IssSky;
use tempo_app::engine::engine_lock;

use crate::{now_unix, tle_act_gate, tle_snapshot, SharedEngine};

/// How often the ISS is looked up while its channel is selected: the receive correction moves at
/// most once a second.
pub const EVERY: Duration = Duration::from_secs(1);

/// The ISS's catalogue number. Matched by number, never by name, so a renamed element set still
/// finds it and no other bird is mistaken for it.
const ISS_NORAD: u32 = 25544;

/// Where the ISS is from `grid` at `now` (unix s), from `tles`.
pub fn iss_sky(tles: &[Tle], grid: &str, now: i64) -> IssSky {
    let Some(obs) = propagation::geo::maidenhead_to_latlon(grid.trim()) else {
        return IssSky::NoGrid;
    };
    let Some(tle) = tles
        .iter()
        .find(|t| sat::norad_id(&t.line1) == Some(ISS_NORAD))
    else {
        return IssSky::NoElements;
    };
    if tle_act_gate(tle, now).is_err() {
        return IssSky::StaleElements;
    }
    match (sat::look_at(tle, obs, now), sat::range_rate(tle, obs, now)) {
        (Some((_, el)), Some((_, rate))) if el > 0.0 => IssSky::Up {
            range_rate_km_s: rate,
        },
        (Some(_), Some(_)) => IssSky::BelowHorizon,
        // The model failed for this instant: there is nothing honest to tune by.
        _ => IssSky::NoElements,
    }
}

/// Start the thread, once per run.
pub fn spawn(engine: SharedEngine) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let started = std::thread::Builder::new()
            .name("nexus-aprs-iss".into())
            .spawn(move || run(engine));
        // Nexus opens without the ISS correction rather than not at all; the channel then
        // behaves as it did before (parked on 145.825) and the cockpit says the correction is
        // starting.
        if let Err(e) = started {
            tempo_core::applog::warn(
                "startup",
                &format!("the APRS ISS Doppler correction not started ({e})"),
            );
        }
    });
}

fn run(engine: SharedEngine) {
    drive(&engine, EVERY, |grid| {
        iss_sky(&tle_snapshot(), grid, now_unix())
    });
}

/// The loop, every `every`, with where the ISS is (`sky_from` the grid) passed in. The tick clock
/// is monotonic: a wall-clock step must not stretch or cut the engine's waits. A panic in a tick
/// (SGP4 on a malformed element set, say) stops the correction rather than leaving it frozen
/// mid-pass: the engine is told, so a frame waiting on the transmit frequency is dropped, never
/// keyed later, and the cockpit says the correction stopped.
fn drive(engine: &SharedEngine, every: Duration, sky_from: impl Fn(&str) -> IssSky) {
    loop {
        std::thread::sleep(every);
        let tick = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let grid = {
                let eng = engine_lock(engine);
                if !eng.aprs_iss_wants_sky() {
                    return;
                }
                eng.settings().mygrid.clone()
            };
            let sky = sky_from(&grid);
            let mut eng = engine_lock(engine);
            let keyed = eng.aprs_iss_keyed();
            eng.aprs_iss_tick(sky, Instant::now(), keyed);
        }));
        if tick.is_err() {
            tempo_core::applog::warn(
                "aprs",
                "the APRS ISS Doppler correction stopped: its tick panicked",
            );
            engine_lock(engine).aprs_iss_driver_stopped();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The 2008 ISS element set (NORAD 25544) the tracker's own tests use.
    const ISS_L1: &str = "1 25544U 98067A   08264.51782528 -.00002182  00000-0 -11606-4 0  2927";
    const ISS_L2: &str = "2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537";
    const ISS_EPOCH_UNIX: i64 = 1_221_913_539;
    const EN52: &str = "EN52";

    fn iss() -> Tle {
        Tle {
            name: "ISS (ZARYA)".into(),
            line1: ISS_L1.into(),
            line2: ISS_L2.into(),
        }
    }

    fn rate(sky: IssSky) -> f64 {
        match sky {
            IssSky::Up { range_rate_km_s } => range_rate_km_s,
            other => panic!("expected the ISS up, got {other:?}"),
        }
    }

    /// A known pass over EN52: rising it approaches (hear it high, send low), setting it
    /// recedes (the reverse), by up to the ~3.6 kHz an ISS pass makes at 145.825 MHz.
    #[test]
    fn a_known_pass_gives_the_right_sign_and_size_each_end() {
        let obs = propagation::geo::maidenhead_to_latlon(EN52).unwrap();
        let pass = sat::passes(&iss(), obs, ISS_EPOCH_UNIX, 24)
            .into_iter()
            .find(|p| p.max_el_deg > 20.0)
            .expect("a good ISS pass in 24 h");
        let rising = rate(iss_sky(&[iss()], EN52, pass.aos_unix + 30));
        let setting = rate(iss_sky(&[iss()], EN52, pass.los_unix - 30));
        assert!(rising < -4.0, "approaching after AOS: {rising} km/s");
        assert!(setting > 4.0, "receding before LOS: {setting} km/s");
        let up = tempo_core::doppler::simplex_legs(145_825_000, rising);
        assert!(
            (2_000..=3_700).contains(&up.downlink_shift_hz),
            "rising: listen high, {} Hz",
            up.downlink_shift_hz
        );
        assert!(up.uplink_shift_hz < -2_000, "rising: send low");
        let down = tempo_core::doppler::simplex_legs(145_825_000, setting);
        assert!(down.downlink_shift_hz < -2_000 && down.uplink_shift_hz > 2_000);
    }

    /// A tick that panics stops the correction: the waiting frame is dropped, never keyed once
    /// the radio later takes the uplink, and the cockpit says the correction stopped.
    #[test]
    fn a_panicking_tick_stops_the_correction_and_drops_a_waiting_frame() {
        let mut e = tempo_app::engine::Engine::new("W9XYZ", "EN61", 0);
        e.set_license_class("technician");
        e.set_operating_mode("phone", false);
        e.aprs_tune(145.825).unwrap();
        e.set_cat_status(Some(true), String::new());
        let approaching = IssSky::Up {
            range_rate_km_s: -6.0,
        };
        assert!(e.aprs_iss_tick(approaching, Instant::now(), false));
        let takes = |e: &mut tempo_app::engine::Engine| {
            let hz = e.settings().dial_hz();
            e.rig_dial_applied(hz);
            e.rig_freq_read(hz);
        };
        takes(&mut e);
        e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[]).unwrap();
        assert!(e.poll_aprs_tx().is_none(), "precondition: the frame waits");
        let engine: SharedEngine = std::sync::Arc::new(std::sync::Mutex::new(e));
        drive(&engine, Duration::ZERO, |_| panic!("malformed elements"));
        let mut e = engine_lock(&engine);
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("stopped"));
        assert!(e.aprs_tx_notice().is_some(), "dropped without a word");
        takes(&mut e);
        assert!(e.poll_aprs_tx().is_none(), "keyed after the driver died");
    }

    #[test]
    fn below_the_horizon_without_elements_or_a_grid_it_says_so() {
        let obs = propagation::geo::maidenhead_to_latlon(EN52).unwrap();
        let pass = sat::passes(&iss(), obs, ISS_EPOCH_UNIX, 24)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            iss_sky(&[iss()], EN52, pass.aos_unix - 600),
            IssSky::BelowHorizon
        );
        assert!(matches!(
            iss_sky(&[iss()], EN52, pass.aos_unix + 60),
            IssSky::Up { .. }
        ));
        assert_eq!(iss_sky(&[], EN52, ISS_EPOCH_UNIX), IssSky::NoElements);
        assert_eq!(iss_sky(&[iss()], "", ISS_EPOCH_UNIX), IssSky::NoGrid);
        assert_eq!(
            iss_sky(&[iss()], EN52, ISS_EPOCH_UNIX + 40 * 86_400),
            IssSky::StaleElements
        );
    }
}
