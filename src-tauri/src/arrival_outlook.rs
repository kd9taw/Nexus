//! `get_arrival_outlook`: what NOAA expects to reach Earth, kept per product.
//!
//! Four NOAA SWPC products feed it ([`propagation::arrival`] says what each one means): the
//! geomagnetic watch in force, the WSA-Enlil model trace, the last hour of L1 wind propagated to
//! Earth, and the 3-day forecast's rationale. (The scales' forecast days have a tested parser,
//! [`propagation::arrival::parse_scale_outlook`], but nothing shows them yet, so they are not
//! fetched here: the space-weather strip already asks for that file.) Each has its own
//! slot here with its own refresh interval, because they change at very different rates (the L1
//! wind every minute, the model twice a day), and its own backoff, so one product failing never
//! re-asks the others and never costs a request per poll.
//!
//! # The slot rule
//!
//! A product is asked for only when its slot is due: its TTL after a good answer, or after a
//! failure 60 s, then 120 s, 240 s … capped at the TTL. The slot is claimed before the request
//! goes out ([`IN_FLIGHT_SECS`]), so two windows polling at once send one request between them.
//! A failure keeps the last good value; nothing is ever filled in.
//!
//! # What is served
//!
//! Each part only while it can still be honest, and every one fails closed with age:
//! - the watch until its last stormy day ends, and only while our last good bulletin list is
//!   younger than [`WATCH_MAX_AGE_SECS`]: a copy that old may have missed a cancel, and "in force"
//!   is a present-tense claim;
//! - the model run while our copy is younger than [`MODEL_MAX_AGE_SECS`] (the fetch has been
//!   failing for six hours otherwise) AND the run itself is younger than [`MODEL_RUN_MAX_AGE_SECS`]
//!   by its own publication time: a stalled model pipeline keeps serving an old run, and we fetch
//!   it successfully every hour. A run whose time we do not know is not served;
//! - the newest L1 minute while it is younger than
//! [`L1_MAX_AGE_SECS`] (the solar-wind gauge's own rule), and a measured jump at L1 for
//! [`JUMP_KEEP_SECS`] after it (a dated event, so it stays true). Kept in memory only: a restart
//! asks again, which is four small requests.

use std::sync::Mutex;

use propagation::arrival::{
    detect_jump, model_arrival, model_peak, ArrivalOutlook, L1Jump, L1Sample, L1View, ModelPoint,
    ModelView, Rationale, Watch,
};

/// Refresh intervals, seconds: SWPC's own cadence for each product, or slower.
const WATCH_TTL: i64 = 900;
const MODEL_TTL: i64 = 3600;
const L1_TTL: i64 = 300;
const RATIONALE_TTL: i64 = 1800;

/// How long a claimed slot waits for its request before another caller may ask again.
const IN_FLIGHT_SECS: i64 = 60;
/// The first retry after a failure; doubled per failure up to the product's TTL.
const FIRST_RETRY_SECS: i64 = 60;
/// Our last good copy of the watch bulletins may be this old before the watch is withdrawn.
pub(crate) const WATCH_MAX_AGE_SECS: i64 = 6 * 3600;
pub(crate) const MODEL_MAX_AGE_SECS: i64 = 6 * 3600;
/// A model run older than this, by its own publication time, is not shown.
pub(crate) const MODEL_RUN_MAX_AGE_SECS: i64 = 24 * 3600;
pub(crate) const L1_MAX_AGE_SECS: i64 = 30 * 60;
pub(crate) const JUMP_KEEP_SECS: i64 = 6 * 3600;

/// The model trace and its run time (`Last-Modified`).
type ModelRun = (Vec<ModelPoint>, Option<i64>);

/// One product's cache: its last good value, when that came, and when to ask next.
#[derive(Debug)]
pub(crate) struct Slot<T> {
    value: Option<T>,
    fetched_at: Option<i64>,
    next_try: i64,
    failures: u32,
}

impl<T> Slot<T> {
    const fn new() -> Self {
        Self {
            value: None,
            fetched_at: None,
            next_try: 0,
            failures: 0,
        }
    }

    /// True (and the slot is held for [`IN_FLIGHT_SECS`]) when this caller should ask now.
    pub(crate) fn claim(&mut self, now: i64) -> bool {
        if now < self.next_try {
            return false;
        }
        self.next_try = now + IN_FLIGHT_SECS;
        true
    }

    pub(crate) fn ok(&mut self, now: i64, v: T, ttl: i64) {
        self.value = Some(v);
        self.fetched_at = Some(now);
        self.failures = 0;
        self.next_try = now + ttl;
    }

    /// A failure keeps the last good value and retries 60 s, 120 s, 240 s … later, never later
    /// than the TTL.
    pub(crate) fn fail(&mut self, now: i64, ttl: i64) {
        self.failures = self.failures.saturating_add(1);
        let shift = (self.failures - 1).min(10);
        self.next_try = now + (FIRST_RETRY_SECS << shift).min(ttl);
    }
}

/// Every product's slot, plus the newest measured jump at L1.
pub(crate) struct State {
    watch: Slot<Option<Watch>>,
    model: Slot<ModelRun>,
    l1: Slot<Vec<L1Sample>>,
    jump: Option<L1Jump>,
    rationale: Slot<Rationale>,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self {
            watch: Slot::new(),
            model: Slot::new(),
            l1: Slot::new(),
            jump: None,
            rationale: Slot::new(),
        }
    }

    /// Keep a fresh L1 hour, and the jump in it when it is newer than the one kept.
    fn l1_ok(&mut self, now: i64, samples: Vec<L1Sample>) {
        if let Some(j) = detect_jump(&samples) {
            if self.jump.is_none_or(|k| j.at >= k.at) {
                self.jump = Some(j);
            }
        }
        self.l1.ok(now, samples, L1_TTL);
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

/// Which products this call asks for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Due {
    watch: bool,
    model: bool,
    l1: bool,
    rationale: bool,
}

pub(crate) fn claim_due(s: &mut State, now: i64) -> Due {
    Due {
        watch: s.watch.claim(now),
        model: s.model.claim(now),
        l1: s.l1.claim(now),
        rationale: s.rationale.claim(now),
    }
}

/// The answers one call brought back (`None` = not asked).
#[derive(Default)]
pub(crate) struct Fetched {
    watch: Option<Result<Option<Watch>, String>>,
    model: Option<Result<ModelRun, String>>,
    l1: Option<Result<Vec<L1Sample>, String>>,
    rationale: Option<Result<Rationale, String>>,
}

fn fetch(due: Due, now: i64) -> Fetched {
    use propagation::live::arrival as live;
    Fetched {
        watch: due.watch.then(|| live::fetch_watch(now)),
        model: due.model.then(live::fetch_model),
        l1: due.l1.then(live::fetch_l1),
        rationale: due.rationale.then(live::fetch_rationale),
    }
}

fn apply<T>(slot: &mut Slot<T>, r: Option<Result<T, String>>, now: i64, ttl: i64) {
    match r {
        Some(Ok(v)) => slot.ok(now, v, ttl),
        Some(Err(_)) => slot.fail(now, ttl),
        None => {}
    }
}

pub(crate) fn store(s: &mut State, f: Fetched, now: i64) {
    apply(&mut s.watch, f.watch, now, WATCH_TTL);
    apply(&mut s.model, f.model, now, MODEL_TTL);
    match f.l1 {
        Some(Ok(v)) => s.l1_ok(now, v),
        Some(Err(_)) => s.l1.fail(now, L1_TTL),
        None => {}
    }
    apply(&mut s.rationale, f.rationale, now, RATIONALE_TTL);
}

/// What may be shown at `now` (see the header's "What is served").
pub(crate) fn assemble(s: &State, now: i64) -> ArrivalOutlook {
    let watch = match (&s.watch.value, s.watch.fetched_at) {
        (Some(w), Some(at)) if now - at <= WATCH_MAX_AGE_SECS => w.clone(),
        _ => None,
    }
    .filter(|w| w.window.is_some_and(|(_, end)| end > now));
    let model = match (&s.model.value, s.model.fetched_at) {
        (Some((pts, Some(run_at))), Some(fetched_at))
            if now - fetched_at <= MODEL_MAX_AGE_SECS && now - run_at <= MODEL_RUN_MAX_AGE_SECS =>
        {
            Some(ModelView {
                run_at: Some(*run_at),
                fetched_at,
                peak: model_peak(pts, now),
                arrival: model_arrival(pts, now),
            })
        }
        _ => None,
    };
    let latest =
        s.l1.value
            .as_ref()
            .and_then(|v| v.last().copied())
            .filter(|x| now - x.at <= L1_MAX_AGE_SECS);
    let jump = s.jump.filter(|j| now - j.at <= JUMP_KEEP_SECS);
    let l1 = (latest.is_some() || jump.is_some()).then_some(L1View { latest, jump });
    ArrivalOutlook {
        watch,
        model,
        l1,
        rationale: s.rationale.value.clone(),
    }
}

/// Everything "coming": NOAA's watch, the model run, the measured wind at L1
/// and the forecasters' rationale, each with its own time. See the module header.
#[tauri::command]
pub(crate) async fn get_arrival_outlook() -> Result<ArrivalOutlook, String> {
    let now = crate::now_unix();
    let due = {
        let mut s = STATE.lock().map_err(|e| e.to_string())?;
        claim_due(&mut s, now)
    };
    if due != Due::default() {
        // Blocking HTTP: on the blocking pool (see `get_aurora`).
        let fetched = tauri::async_runtime::spawn_blocking(move || fetch(due, now))
            .await
            .map_err(|e| e.to_string())?;
        let mut s = STATE.lock().map_err(|e| e.to_string())?;
        store(&mut s, fetched, crate::now_unix());
    }
    let s = STATE.lock().map_err(|e| e.to_string())?;
    Ok(assemble(&s, crate::now_unix()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use propagation::arrival::WatchDay;

    const T0: i64 = 1_793_404_800; // 2026-10-31 00:00 UTC

    fn watch(end: i64) -> Watch {
        Watch {
            serial: 1134,
            code: "WATA20".into(),
            issued: T0 - 3600,
            days: vec![WatchDay { day: T0, g: 1 }],
            comment: None,
            window: Some((T0, end)),
            max_g: 1,
        }
    }

    fn l1(at: i64) -> L1Sample {
        L1Sample {
            at,
            arrives: at + 3300,
            speed: Some(400.0),
            bt: Some(5.0),
        }
    }

    /// The slot rule: a good answer is kept for the TTL; a failure retries 60, 120, 240 s later,
    /// capped at the TTL, and a claimed slot is not asked for again by a second caller.
    #[test]
    fn a_slot_caches_for_its_ttl_and_backs_off_after_a_failure() {
        let mut s: Slot<u8> = Slot::new();
        assert!(s.claim(T0), "a cold slot is due");
        assert!(!s.claim(T0 + 1), "a claimed slot is not asked twice");
        s.ok(T0, 7, 900);
        assert!(!s.claim(T0 + 899), "asked again inside its TTL");
        assert!(s.claim(T0 + 900));
        s.fail(T0 + 900, 900);
        assert_eq!(s.value, Some(7), "a failure dropped the last good value");
        assert!(!s.claim(T0 + 959), "retried before the first backoff");
        assert!(s.claim(T0 + 960));
        s.fail(T0 + 960, 900);
        assert!(!s.claim(T0 + 1079));
        assert!(s.claim(T0 + 1080), "the second retry waits 120 s");
        for _ in 0..20 {
            s.fail(T0 + 2000, 900);
        }
        assert!(!s.claim(T0 + 2899), "the backoff passed the TTL");
        assert!(s.claim(T0 + 2900), "the backoff is capped at the TTL");
        s.ok(T0 + 2900, 8, 900);
        s.fail(T0 + 3800, 900);
        assert!(s.claim(T0 + 3860), "a good answer resets the backoff");
    }

    /// Polled once a second for ten minutes against a product that always fails, a slot asks a
    /// handful of times, never once per poll.
    #[test]
    fn a_failing_product_is_not_asked_once_per_poll() {
        let mut s = State::new();
        let mut asks = 0;
        for t in 0..600 {
            let due = claim_due(&mut s, T0 + t);
            if due.l1 {
                asks += 1;
            }
            let f = Fetched {
                l1: due.l1.then(|| Err("down".to_string())),
                ..Fetched::default()
            };
            store(&mut s, f, T0 + t);
        }
        // Asked at 0, 60, 180, 420 (then 300 s apart from the cap at the L1 TTL).
        assert_eq!(asks, 4);
    }

    #[test]
    fn nothing_fetched_serves_nothing() {
        assert_eq!(assemble(&State::new(), T0), ArrivalOutlook::default());
    }

    #[test]
    fn a_watch_is_served_until_its_window_ends() {
        let mut s = State::new();
        // A copy fetched an hour before the window ends, so only the window decides here (the
        // copy's own age limit is the next test).
        s.watch.ok(T0 + 82_800, Some(watch(T0 + 86_400)), WATCH_TTL);
        assert!(assemble(&s, T0 + 86_399).watch.is_some());
        assert!(assemble(&s, T0 + 86_400).watch.is_none());
    }

    /// Fails closed with age: a bulletin list we last got more than six hours ago may have missed
    /// a cancel, so its watch is no longer said to be in force (positive control at six hours).
    #[test]
    fn a_watch_from_a_copy_older_than_six_hours_is_withdrawn() {
        let mut s = State::new();
        s.watch.ok(T0, Some(watch(T0 + 3 * 86_400)), WATCH_TTL);
        s.watch.fail(T0 + 3600, WATCH_TTL); // the next fetches fail; the copy is kept…
        assert!(assemble(&s, T0 + 6 * 3600).watch.is_some());
        assert!(
            assemble(&s, T0 + 6 * 3600 + 1).watch.is_none(),
            "…but past six hours it is not served"
        );
    }

    fn model_with_run(s: &mut State, fetched: i64, run_at: Option<i64>) {
        let pts = vec![ModelPoint {
            at: fetched + 7 * 3600,
            speed: 500.0,
            density: 3.0,
            cloud: false,
        }];
        s.model.ok(fetched, (pts, run_at), MODEL_TTL);
    }

    /// The run's own age decides, not our download: a stalled model pipeline serves an old run
    /// that we fetch "fresh" every hour.
    #[test]
    fn a_model_run_older_than_a_day_or_of_unknown_age_is_hidden() {
        let mut s = State::new();
        model_with_run(&mut s, T0, Some(T0 - 24 * 3600));
        assert!(
            assemble(&s, T0).model.is_some(),
            "a run exactly a day old is shown"
        );
        model_with_run(&mut s, T0, Some(T0 - 24 * 3600 - 1));
        assert!(
            assemble(&s, T0).model.is_none(),
            "a run over a day old, fetched just now, was shown"
        );
        model_with_run(&mut s, T0, None);
        assert!(
            assemble(&s, T0).model.is_none(),
            "a run of unknown age was shown"
        );
    }

    #[test]
    fn a_model_copy_older_than_six_hours_is_hidden() {
        let mut s = State::new();
        let pts = vec![ModelPoint {
            at: T0 + 7 * 3600,
            speed: 500.0,
            density: 3.0,
            cloud: false,
        }];
        s.model.ok(T0, (pts, Some(T0 - 600)), MODEL_TTL);
        let m = assemble(&s, T0 + MODEL_MAX_AGE_SECS)
            .model
            .expect("a fresh copy");
        assert_eq!((m.run_at, m.fetched_at), (Some(T0 - 600), T0));
        assert_eq!(m.peak.map(|p| p.speed), Some(500.0));
        assert!(assemble(&s, T0 + MODEL_MAX_AGE_SECS + 1).model.is_none());
    }

    /// The newest L1 minute is "now" for 30 minutes and not a second longer.
    #[test]
    fn the_newest_l1_minute_is_served_for_thirty_minutes() {
        let mut s = State::new();
        s.l1_ok(T0, (0..10).map(|i| l1(T0 - 540 + i * 60)).collect());
        let at = |t: i64| assemble(&s, t).l1.and_then(|v| v.latest).map(|x| x.at);
        assert_eq!(at(T0 + 30 * 60), Some(T0));
        assert_eq!(at(T0 + 30 * 60 + 1), None);
    }

    /// A later jump replaces an earlier one.
    #[test]
    fn a_newer_jump_replaces_an_older_one() {
        let mut s = State::new();
        let stepped = |start: i64| -> Vec<L1Sample> {
            (0..20)
                .map(|i| {
                    let mut x = l1(start + i * 60);
                    if i >= 12 {
                        x.speed = Some(560.0);
                    }
                    x
                })
                .collect()
        };
        s.l1_ok(T0 + 1200, stepped(T0));
        s.l1_ok(T0 + 9000, stepped(T0 + 7800));
        let j = assemble(&s, T0 + 9000).l1.and_then(|v| v.jump);
        assert_eq!(j.map(|j| j.at), Some(T0 + 7800 + 12 * 60));
    }

    /// The newest L1 minute goes quiet after 30 min; a measured jump stays for six hours.
    #[test]
    fn l1_minutes_age_out_and_a_jump_is_kept() {
        let mut s = State::new();
        let mut hour: Vec<L1Sample> = (0..20).map(|i| l1(T0 + i * 60)).collect();
        for x in &mut hour[12..] {
            x.speed = Some(560.0);
        }
        s.l1_ok(T0 + 1200, hour);
        let v = assemble(&s, T0 + 1200).l1.unwrap();
        assert_eq!(v.latest.map(|x| x.at), Some(T0 + 19 * 60));
        assert_eq!(v.jump.map(|j| j.at), Some(T0 + 12 * 60));
        // A later, steady hour keeps the jump it does not repeat.
        s.l1_ok(T0 + 5000, (0..20).map(|i| l1(T0 + 4000 + i * 60)).collect());
        let v = assemble(&s, T0 + 5000).l1.unwrap();
        assert_eq!(v.jump.map(|j| j.at), Some(T0 + 12 * 60));
        let late = assemble(&s, T0 + 12 * 60 + JUMP_KEEP_SECS + 1);
        assert!(
            late.l1.is_none(),
            "a stale minute and an old jump are not served"
        );
    }
}
