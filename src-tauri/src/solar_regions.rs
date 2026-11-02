//! `get_solar_regions`: NOAA's numbered active regions for the sun disc on the map.
//!
//! One product (`json/solar_regions.json`, about 100 KB, published once a day), asked for at most
//! hourly through one [`Slot`]: its TTL after a good answer, after a failure 60 s, 120 s, 240 s …
//! capped at the TTL, claimed before the request goes out so two windows send one request between
//! them. A failed or refused answer keeps the last good list. Kept in memory only.
//!
//! WHAT IS SERVED fails closed, decided here on the station's clock
//! ([`propagation::regions::regions_state`]): the regions only while the list is Held — its
//! observed day has begun (within the skew allowance) and is less than three days old. Otherwise
//! no regions, and `why` says which: `stale`, `ahead`, or `none` (nothing held yet). `until` is
//! when a held list stops being drawn, so a screen whose polls fail still drops it on time.

use std::sync::Mutex;

use propagation::regions::{regions_state, regions_until, RegionsState, SolarRegion, SolarRegions};
use serde::Serialize;

use crate::arrival_outlook::Slot;

/// Refresh interval, seconds. NOAA publishes the list once a day.
const REGIONS_TTL: i64 = 3600;

static SLOT: Mutex<Slot<SolarRegions>> = Mutex::new(Slot::new());

/// What the sun disc may draw now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SolarRegionsView {
    /// True only while the list may be drawn.
    pub held: bool,
    /// `held`, `stale`, `ahead`, or `none` (no list held).
    pub why: &'static str,
    /// The observed day (Unix seconds of its 00:00 UTC), when a list is held at all.
    pub observed_day: Option<i64>,
    /// Until when a held list is drawn (Unix seconds); present exactly when `held` is.
    pub until: Option<i64>,
    /// The regions on the visible disc; empty unless `held`.
    pub regions: Vec<SolarRegion>,
}

/// The view of what the slot holds, at `now`.
pub(crate) fn view(kept: Option<&SolarRegions>, now: i64) -> SolarRegionsView {
    let Some(list) = kept else {
        return SolarRegionsView {
            held: false,
            why: "none",
            observed_day: None,
            until: None,
            regions: Vec::new(),
        };
    };
    let state = regions_state(list.observed_day, now);
    let held = state == RegionsState::Held;
    SolarRegionsView {
        held,
        why: match state {
            RegionsState::Held => "held",
            RegionsState::Stale => "stale",
            RegionsState::Ahead => "ahead",
        },
        observed_day: Some(list.observed_day),
        until: held.then(|| regions_until(list.observed_day)),
        regions: if held {
            list.regions.clone()
        } else {
            Vec::new()
        },
    }
}

/// One poll: ask NOAA when the slot is due (`fetch` is the network), then say what may be drawn.
fn poll(
    slot: &Mutex<Slot<SolarRegions>>,
    now: impl Fn() -> i64,
    fetch: impl FnOnce(i64) -> Result<SolarRegions, String>,
) -> Result<SolarRegionsView, String> {
    let due = slot.lock().map_err(|e| e.to_string())?.claim(now());
    if due {
        let got = fetch(now());
        let mut s = slot.lock().map_err(|e| e.to_string())?;
        match got {
            Ok(list) => s.ok(now(), list, REGIONS_TTL),
            Err(_) => s.fail(now(), REGIONS_TTL),
        }
    }
    let s = slot.lock().map_err(|e| e.to_string())?;
    Ok(view(s.kept().map(|(v, _)| v), now()))
}

#[tauri::command]
pub(crate) async fn get_solar_regions() -> Result<SolarRegionsView, String> {
    // Blocking HTTP: the whole poll runs on the blocking pool (see `get_aurora`). No lock is held
    // across the request.
    tauri::async_runtime::spawn_blocking(|| {
        poll(
            &SLOT,
            crate::now_unix,
            propagation::live::solar_regions::fetch_solar_regions,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// 2026-10-30 02:46 UTC.
    const NOW: i64 = 1_793_328_360;
    const DAY29: i64 = 1_793_232_000; // 2026-10-29 00:00 UTC

    fn list(day: i64) -> SolarRegions {
        SolarRegions {
            observed_day: day,
            regions: vec![SolarRegion {
                region: 4564,
                lat_deg: 5.0,
                lon_deg: 68.0,
                location: Some("N05E68".into()),
                mag_class: Some("A".into()),
                spot_class: Some("Axx".into()),
                area: Some(10),
                number_spots: Some(1),
                m_flare_probability: Some(1),
                x_flare_probability: Some(1),
            }],
        }
    }

    #[test]
    fn a_held_list_is_served_with_its_day_and_until() {
        let v = view(Some(&list(DAY29)), NOW);
        assert!(v.held);
        assert_eq!(v.why, "held");
        assert_eq!(v.observed_day, Some(DAY29));
        assert_eq!(v.until, Some(DAY29 + 3 * 86_400));
        assert_eq!(v.regions.len(), 1);
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["observedDay"], DAY29);
        assert_eq!(j["regions"][0]["latDeg"], 5.0);
        assert_eq!(j["regions"][0]["magClass"], "A");
    }

    #[test]
    fn a_stale_or_ahead_list_serves_no_regions_and_says_why() {
        let stale = view(Some(&list(DAY29)), DAY29 + 3 * 86_400);
        assert_eq!((stale.held, stale.why, stale.until), (false, "stale", None));
        assert!(stale.regions.is_empty(), "a stale list drew regions");
        assert_eq!(
            stale.observed_day,
            Some(DAY29),
            "the row names the day it is from"
        );
        let held = view(Some(&list(DAY29)), DAY29 + 3 * 86_400 - 1);
        assert!(held.held, "CONTROL: one second earlier it is held");
        let ahead = view(Some(&list(DAY29)), DAY29 - 121);
        assert_eq!((ahead.held, ahead.why), (false, "ahead"));
        assert!(ahead.regions.is_empty());
        let none = view(None, NOW);
        assert_eq!(
            (none.held, none.why, none.observed_day),
            (false, "none", None)
        );
    }

    /// The slot rule with a product that always fails: polled every second for two hours, NOAA
    /// is asked a handful of times (60, 120, 240 … s, capped at the hour), never once a poll.
    #[test]
    fn a_failing_product_backs_off_and_is_not_asked_once_per_poll() {
        let slot = Mutex::new(Slot::new());
        let t = Cell::new(NOW);
        let asks = Cell::new(0);
        for _ in 0..7200 {
            let v = poll(
                &slot,
                || t.get(),
                |_| {
                    asks.set(asks.get() + 1);
                    Err("down".into())
                },
            )
            .unwrap();
            assert_eq!(v.why, "none");
            t.set(t.get() + 1);
        }
        // 0, 60, 180, 420, 900, 1860, 3780, 7380 → 7 asks inside 7200 s.
        assert_eq!(asks.get(), 7, "asked {} times", asks.get());
    }

    #[test]
    fn a_good_answer_is_kept_for_the_hour_and_kept_through_a_failure() {
        let slot = Mutex::new(Slot::new());
        let asks = Cell::new(0);
        let ok = |_: i64| -> Result<SolarRegions, String> { Ok(list(DAY29)) };
        let v = poll(
            &slot,
            || NOW,
            |n| {
                asks.set(asks.get() + 1);
                ok(n)
            },
        )
        .unwrap();
        assert!(v.held);
        // Inside the hour: not asked again.
        let v = poll(
            &slot,
            || NOW + 3599,
            |_| {
                asks.set(asks.get() + 1);
                Err("x".into())
            },
        )
        .unwrap();
        assert_eq!(asks.get(), 1, "asked again inside its hour");
        assert!(v.held);
        // At the hour it is asked; the failure keeps the list.
        let v = poll(
            &slot,
            || NOW + 3600,
            |_| {
                asks.set(asks.get() + 1);
                Err("x".into())
            },
        )
        .unwrap();
        assert_eq!(asks.get(), 2);
        assert!(v.held, "a failure dropped the list it held");
        // A kept list still ages out on the clock, failures or not.
        let v = poll(&slot, || DAY29 + 3 * 86_400, |_| Err("x".into())).unwrap();
        assert!(!v.held);
        assert_eq!(v.why, "stale");
    }
}
