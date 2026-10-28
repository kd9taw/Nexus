//! The Logbook's "Update from callbook" as the UI asks for it: a preview, a start, the progress, and a
//! cancel ([`tempo_app::callbook_update`], [`tempo_app::logtools::callbook_fill`]).
//!
//! **The same lookup the callsign card makes.** A call the lookup cache answers fresh is taken from
//! it; any other is asked of the callbooks ([`crate::callbook_lookup_live`]) — QRZ with the account
//! and stored password the card uses, then HamQTH and callook.info as the card falls through to
//! them — and the answer kept in the cache. Nothing here reads, stores or asks for a credential.
//! Unlike the card it does not hand its answers to the engine's in-memory list for the contacts
//! being logged now: a few hundred old calls would push out the ones the operator just looked up.
//!
//! ⚠️ **A failed lookup is a failure, never an older answer** ([`ask`]). The card answers from an
//! expired kept answer when the network fails; the update must not, or a QRZ that turns the
//! sign-in down would be asked (and turn it down) for every call to the cap, each "found" in data
//! older than the operator's expiry.
//!
//! **Paced and capped** by the operator's settings (`callbook_update_pace_secs`,
//! `callbook_update_daily_cap`). The fallbacks are paced the same: each call is ONE paced turn, in
//! which QRZ is asked first and HamQTH, then callook.info (a US call), only when the one before
//! has no record, so no service is asked more often than one call per pace (the call and then its
//! base for a stroked call, which the cap counts as two). The cap counts every turn's lookups,
//! whichever service answered. The station's own tally of the day's network lookups lives in the
//! cache file ([`tempo_core::callbook_cache::CallbookCache::note_lookups`]), and QRZ's own count of
//! the account's lookups ([`note_qrz_session`]) holds the cap too when an answer carries it; the
//! last one is kept in the same file with its time, so the next preview and run start from it. A
//! sign-in to QRZ or HamQTH is a request too ([`note_login`]), and counts.
//!
//! **What the services said** is noted by the lookups themselves ([`note_qrz_answer`],
//! [`note_login`], [`note_hamqth_refused`], [`note_qrz_session`]...) and taken after each of the
//! update's lookups. They are the station's own account's, so a card lookup made at the same time
//! says the same about it.
//!
//! **QRZ's own rules** (its XML spec): ONE sign-in per session, the key reused until QRZ says it
//! expired ([`crate::qrz_lookup_attempt`]); its `agent` on every request ([`crate::qrz_agent`]); its
//! `<Message>` and `<Error>` shown to the operator (the status's and the end's `qrz_said`); its
//! "Connection refused" stops the run, and QRZ is not asked again by any lookup for a day
//! ([`crate::callbook_lookup_live`]); and an account without the XML subscription (`<SubExp>`
//! `non-subscriber`) is left out of the update, which goes on with HamQTH and callook.info.
//!
//! **One run.** The lookups run on a thread of their own; when they stop — every call answered,
//! cancelled, the cap reached, QRZ refusing or a lookup failed — the answers in hand are filled as
//! ONE run of the history, which the toast's Undo takes back. A later run takes the calls already
//! answered from the cache at no cost. A large fill waits while the radio transmits (shown as
//! waiting, and Stop ends the wait), and a fill stopped part-way still hands back its run.
//! The fill happens only once the lookups end: if the app quits mid-run nothing is filled, which
//! loses no contact data, and the answers already fetched are in the cache, so a re-run fills them
//! at no network cost.
//!
//! **Its end is reported once, wherever the operator is.** The end is kept until the Logbook takes
//! it ([`callbook_update_take_end`]): the dialog may have been closed long before the run ended,
//! and the Logbook, always there, shows the toast and its Undo and reloads the list.
//!
//! **Where the station is, only on the last 12 months' contacts** (operator ruling): the fill
//! gives an older contact the answer without its location ([`logtools::LOCATION_FIELDS`]), and the
//! preview counts the contacts that leaves without one.
//!
//! **Where it runs.** The preview, the progress and the cancel run at the station from either
//! computer; the start is the shack's own, as a bulk edit's is (`command_classes`): it changes
//! many contacts at once, and the lookups always run at the station with its own account.

use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tempo_app::callbook_update::{self, Asked, Callbook, Limits, Looked, Progress, Stop};
use tempo_app::engine::engine_lock;
use tempo_app::logtools;
use tempo_core::logbook::callbook::CallbookAnswer;
use tempo_core::logbook::RecordId;

use crate::log_queries::LogQueries;
use crate::log_tools::{id_of, run_dto, target_ids, BulkTarget, ToolRunDto};
use crate::{SharedCallbookCache, SharedEngine, SharedHamQthSession, SharedQrzSession};

/// What the callbooks said since it was last taken: noted by the lookups, taken by the update after
/// each of its lookups.
#[derive(Debug, Default, PartialEq)]
struct Heard {
    /// QRZ's own count of the account's lookups in the last 24 hours, from the last answer that
    /// carried one (`<Session><Count>`).
    qrz_count: Option<u32>,
    /// QRZ answered with a record.
    qrz_record: bool,
    /// QRZ answered with none: an unknown call, or a refusal.
    qrz_no_record: bool,
    /// Sign-ins to QRZ or HamQTH.
    logins: usize,
    /// HamQTH would not take a session it had just issued.
    hamqth_refused: bool,
    /// QRZ's last `<Message>` and `<Error>`, as the operator reads them ([`screen_words`]).
    qrz_said: Option<String>,
    /// QRZ's `<SubExp>` says the account has no XML subscription.
    qrz_non_subscriber: bool,
    /// QRZ refused service ("Connection refused"), now or within the day before.
    qrz_refused: bool,
    /// QRZ would not take a session key it had just issued.
    qrz_fresh_key_refused: bool,
    /// callook.info did not answer (its daily maintenance).
    callook_unavailable: bool,
    /// A request went to QRZ or HamQTH, which the cap counts.
    counted_request: bool,
}

static HEARD: Mutex<Heard> = Mutex::new(Heard {
    qrz_count: None,
    qrz_record: false,
    qrz_no_record: false,
    logins: 0,
    hamqth_refused: false,
    qrz_said: None,
    qrz_non_subscriber: false,
    qrz_refused: false,
    qrz_fresh_key_refused: false,
    callook_unavailable: false,
    counted_request: false,
});

/// The tests that note or take [`HEARD`] run one at a time.
#[cfg(test)]
static HEARD_TESTS: Mutex<()> = Mutex::new(());

fn heard() -> std::sync::MutexGuard<'static, Heard> {
    lock(&HEARD)
}

/// Note what one QRZ answer's `<Session>` says ([`crate::qrz_try_lookup`], [`crate::qrz_login`]):
/// its count, its words, and whether the account is a subscriber.
pub(crate) fn note_qrz_session(session: &tempo_core::qrz::QrzSession) {
    let mut h = heard();
    if let Some(c) = session.count {
        h.qrz_count = Some(c);
    }
    let words: Vec<String> = [&session.message, &session.error]
        .into_iter()
        .flatten()
        .map(|w| screen_words(w))
        .filter(|w| !w.is_empty())
        .collect();
    if !words.is_empty() {
        h.qrz_said = Some(words.join(" — "));
    }
    h.qrz_non_subscriber |= session.non_subscriber();
}

/// A service's words as the operator is shown them: one line, at most 200 characters.
pub(crate) fn screen_words(w: &str) -> String {
    let line: String = w
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    line.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

/// Note QRZ's refusal of service, now or still holding.
pub(crate) fn note_qrz_refused() {
    heard().qrz_refused = true;
}

/// Note QRZ turning down a session key it had just issued.
pub(crate) fn note_qrz_fresh_key_refused() {
    heard().qrz_fresh_key_refused = true;
}

/// Note callook.info not answering.
pub(crate) fn note_callook_unavailable() {
    heard().callook_unavailable = true;
}

/// Note a request to QRZ or HamQTH.
pub(crate) fn note_counted_request() {
    heard().counted_request = true;
}

/// Note what QRZ answered a lookup on a live session: a record, or none.
pub(crate) fn note_qrz_answer(record: bool) {
    let mut h = heard();
    if record {
        h.qrz_record = true;
    } else {
        h.qrz_no_record = true;
    }
}

/// Note a sign-in request to QRZ or HamQTH.
pub(crate) fn note_login() {
    let mut h = heard();
    h.logins += 1;
    h.counted_request = true;
}

/// Note HamQTH turning down a session it had just issued.
pub(crate) fn note_hamqth_refused() {
    heard().hamqth_refused = true;
}

fn take_heard() -> Heard {
    std::mem::take(&mut *heard())
}

/// What the update says when HamQTH turns down its own session: asked again, it would sign in
/// again for every call.
const HAMQTH_REFUSED: &str =
    "HamQTH would not accept a session it had just issued. Check the HamQTH username and password.";

/// What the update says when QRZ refuses service: its spec says a sign-in will not work for a day.
const QRZ_REFUSED: &str =
    "QRZ refused service for this account (\"Connection refused\"). By QRZ's \
                           own rules a sign-in will not work for at least 24 hours, so Nexus will \
                           not ask QRZ again until then. Check the account at QRZ.";

/// What the update says when QRZ turns down a session key it had just issued.
const QRZ_FRESH_KEY_REFUSED: &str = "QRZ would not accept a session it had just issued. Check the \
                                     QRZ username, password and XML subscription.";

/// What the update says when the account has no QRZ XML subscription and no other callbook is
/// set up.
const QRZ_SUBSCRIPTION_ALONE: &str = "A QRZ XML subscription is needed for bulk updates, and \
                                      neither HamQTH nor callook.info is set up.";

/// The update in progress, or the last one's outcome. Managed state.
#[derive(Default)]
pub struct CallbookUpdate {
    status: Mutex<UpdateStatusDto>,
    /// The last update's end until the Logbook takes it ([`callbook_update_take_end`]): reported
    /// once.
    unreported: Mutex<Option<UpdateEndDto>>,
    cancel: AtomicBool,
    /// The day's network lookups, when the cache file cannot be opened: kept for the session.
    fallback_tally: Mutex<Vec<i64>>,
}

/// How the last update ended.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEndDto {
    /// `done`, `cancelled`, `limit`, `refused` (QRZ refusing lookups) or `failed`.
    pub stop: String,
    /// Why a lookup failed, or why the fill could not be made.
    pub error: Option<String>,
    /// QRZ's last `<Message>`/`<Error>`, shown as QRZ's spec asks.
    pub qrz_said: Option<String>,
    /// QRZ was left out: the account has no XML subscription.
    pub qrz_skipped: bool,
    /// The calls the callbook does not know.
    pub unknown: Vec<String>,
    /// Calls not reached: the rest, to be picked up by another run.
    pub not_looked_up: usize,
    /// The fill's run: what the toast's Undo takes. `None` when nothing was filled.
    pub made: Option<ToolRunDto>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusDto {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub from_cache: usize,
    pub looked_up: usize,
    /// Network lookups in the last 24 hours, as the cap reads them, and the cap.
    pub spent: usize,
    pub cap: usize,
    /// The lookups are done, and the fill waits for the radio to stop transmitting.
    pub waiting_for_tx: bool,
    /// QRZ's last `<Message>`/`<Error>` this run, shown as QRZ's spec asks.
    pub qrz_said: Option<String>,
    /// QRZ is left out of this run: the account has no XML subscription (`<SubExp>`), so the
    /// other callbooks answer.
    pub qrz_skipped: bool,
    /// The last update's outcome, once it has ended.
    pub ended: Option<UpdateEndDto>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The kept answer for `call` while it is fresh, as the log takes it. Never the network.
pub(crate) fn kept(
    cache: &SharedCallbookCache,
    call: &str,
    max_age: i64,
) -> Option<CallbookAnswer> {
    let (dto, fetched_at) = cache.get(call)?;
    tempo_core::callbook_cache::CachedAnswer {
        answer: String::new(),
        fetched_at,
    }
    .is_fresh(crate::now_unix(), max_age)
    .then(|| crate::callbook_answer(&dto))
}

/// The day's network lookups as the cap reads them: the cache file's tally, or the session's —
/// or QRZ's own count when it kept a larger one in the last day.
fn spent(app: &AppHandle) -> usize {
    let now = crate::now_unix();
    let cache = app.state::<SharedCallbookCache>();
    let qrz = cache
        .store()
        .and_then(|s| s.lock().ok().and_then(|s| s.qrz_count(now).ok().flatten()))
        .unwrap_or(0);
    if let Some(n) = cache
        .store()
        .and_then(|s| s.lock().ok().and_then(|s| s.lookups_spent(now).ok()))
    {
        return n.max(qrz);
    }
    let job = app.state::<CallbookUpdate>();
    let tally = lock(&job.fallback_tally);
    tally.iter().filter(|&&at| at > now - 86_400).count()
}

/// ★ One network lookup of `call` for the update: asked of the callbooks (`live`) whatever the
/// cache holds, and the answer kept in the cache. Any failure but "not in the callbook" is the
/// failure: an older kept answer never stands in for it (see the module header).
async fn ask<F, Fut>(cache: &SharedCallbookCache, call: &str, now: i64, live: F) -> Looked
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<tempo_app::dto::QrzLookupDto, String>>,
{
    match live().await {
        Ok(dto) => {
            cache.put(call, &dto, now);
            Looked::Found(Box::new(crate::callbook_answer(&dto)))
        }
        Err(e) if crate::is_callbook_not_found(&e, call) => Looked::Unknown,
        Err(e) => Looked::Failed(e),
    }
}

/// One lookup's answer with what the services said while it was made: HamQTH turning down its own
/// session stops the run, as it would sign in again for every call.
///
/// QRZ refusing service, or turning down its own fresh session, stops the run too. callook.info not
/// answering leaves the call for later, uncounted when no other service was asked.
fn asked(looked: Looked, heard: Heard) -> Asked {
    let stop = if heard.qrz_refused {
        Some(QRZ_REFUSED)
    } else if heard.qrz_fresh_key_refused {
        Some(QRZ_FRESH_KEY_REFUSED)
    } else if heard.hamqth_refused {
        Some(HAMQTH_REFUSED)
    } else {
        None
    };
    let later = heard.callook_unavailable && matches!(looked, Looked::Failed(_));
    Asked {
        looked: match stop {
            Some(why) => Looked::Failed(why.into()),
            None if later => Looked::Later,
            None => looked,
        },
        qrz_count: heard.qrz_count.map(|c| c as usize),
        qrz_without_record: heard.qrz_no_record && !heard.qrz_record,
        logins: heard.logins,
        uncounted: stop.is_none() && later && !heard.counted_request,
    }
}

/// Whether the update leaves QRZ out from now on: once an answer says the account has no XML
/// subscription — without one, QRZ's spec says access "is primarily intended for testing and
/// troubleshooting purposes only".
fn skips_qrz(skipping: bool, heard: &Heard) -> bool {
    skipping || heard.qrz_non_subscriber
}

/// The station's callbook as the update's loop asks it.
struct StationCallbook {
    app: AppHandle,
    max_age: i64,
    /// QRZ is left out: the account has no XML subscription.
    skip_qrz: bool,
    /// HamQTH or callook.info is set up, to answer without QRZ.
    others: bool,
}

impl Callbook for StationCallbook {
    fn kept(&mut self, call: &str) -> Option<CallbookAnswer> {
        kept(&self.app.state::<SharedCallbookCache>(), call, self.max_age)
    }

    /// The call, then its base for a stroked call: what the lookup may ask.
    fn cost(&self, call: &str) -> usize {
        crate::callbook_candidates(call).len()
    }

    fn spent(&mut self) -> usize {
        spent(&self.app)
    }

    fn forget_spent(&mut self, n: usize) {
        let cache = self.app.state::<SharedCallbookCache>();
        let forgot = cache
            .store()
            .and_then(|s| s.lock().ok().map(|s| s.forget_lookups(n).is_ok()))
            .unwrap_or(false);
        if !forgot {
            let job = self.app.state::<CallbookUpdate>();
            let mut tally = lock(&job.fallback_tally);
            let keep = tally.len().saturating_sub(n);
            tally.truncate(keep);
        }
    }

    fn note_spent(&mut self, n: usize) {
        let now = crate::now_unix();
        let cache = self.app.state::<SharedCallbookCache>();
        let noted = cache
            .store()
            .and_then(|s| s.lock().ok().map(|s| s.note_lookups(n, now).is_ok()))
            .unwrap_or(false);
        if !noted {
            let job = self.app.state::<CallbookUpdate>();
            lock(&job.fallback_tally).extend(std::iter::repeat_n(now, n));
        }
    }

    fn live(&mut self, call: &str) -> Asked {
        if self.skip_qrz && !self.others {
            return Asked {
                uncounted: true,
                ..Asked::plain(Looked::Failed(QRZ_SUBSCRIPTION_ALONE.into()))
            };
        }
        let _ = take_heard();
        let engine = self.app.state::<SharedEngine>().inner().clone();
        let qrz = self.app.state::<SharedQrzSession>().inner().clone();
        let hamqth = SharedHamQthSession(self.app.state::<SharedHamQthSession>().0.clone());
        let cache = self.app.state::<SharedCallbookCache>();
        let now = crate::now_unix();
        let ask_qrz = !self.skip_qrz;
        let looked = tauri::async_runtime::block_on(ask(&cache, call, now, || {
            crate::callbook_lookup_live(call, &engine, &qrz, &hamqth, &cache, ask_qrz)
        }));
        let heard = take_heard();
        if let (Some(store), Some(count)) = (cache.store(), heard.qrz_count) {
            if let Ok(store) = store.lock() {
                let _ = store.note_qrz_count(count as usize, now);
            }
        }
        self.skip_qrz = skips_qrz(self.skip_qrz, &heard);
        {
            let job = self.app.state::<CallbookUpdate>();
            let mut status = lock(&job.status);
            status.qrz_skipped = self.skip_qrz;
            if heard.qrz_said.is_some() {
                status.qrz_said.clone_from(&heard.qrz_said);
            }
        }
        asked(looked, heard)
    }

    fn wait(&mut self, d: Duration) -> bool {
        let job = self.app.state::<CallbookUpdate>();
        let step = Duration::from_millis(100);
        let mut left = d;
        while !left.is_zero() {
            if job.cancel.load(Ordering::SeqCst) {
                return false;
            }
            let nap = left.min(step);
            std::thread::sleep(nap);
            left -= nap;
        }
        !job.cancel.load(Ordering::SeqCst)
    }

    fn cancelled(&self) -> bool {
        self.app
            .state::<CallbookUpdate>()
            .cancel
            .load(Ordering::SeqCst)
    }
}

/// What an update of the contacts would do: the calls, those already kept, the time the rest would
/// take at the pace, the day's lookups left under the cap, and per field how many contacts the fill
/// could give it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreviewDto {
    /// The contacts previewed: what `callbook_update_start` is handed back.
    pub ids: Vec<String>,
    pub selected: usize,
    pub missing: usize,
    pub calls: usize,
    pub kept: usize,
    /// Calls the cache does not answer: each a network lookup.
    pub to_look_up: usize,
    /// What those may cost against the cap: a stroked call is asked again for its base.
    pub lookups_needed: usize,
    pub pace_secs: u64,
    /// The least time the lookups take at the pace, in seconds.
    pub estimate_secs: u64,
    pub spent: usize,
    pub cap: usize,
    /// QRZ's own count of the account's lookups, from its last answer in the last day.
    pub qrz_count: Option<usize>,
    pub could_change: usize,
    /// `[ADIF field, contacts]`, most first: exact for kept answers, at most for the rest.
    pub fields: Vec<(String, usize)>,
    /// Whether a callbook is set up at all (a QRZ or HamQTH username, or callook.info on).
    pub configured: bool,
    /// The services each call is asked of, in order, as set up: `qrz`, `hamqth`, `callook`
    /// (US calls only).
    pub services: Vec<&'static str>,
    /// Contacts older than 12 months left without a location field the answer would have given.
    pub location_too_old: usize,
}

/// The callbooks [`crate::callbook_lookup_live`] asks, in its order, as the settings set them up.
fn services(s: &tempo_app::settings::Settings) -> Vec<&'static str> {
    let mut out = Vec::new();
    if !s.qrz_username.trim().is_empty() {
        out.push("qrz");
    }
    if !s.hamqth_username.trim().is_empty() {
        out.push("hamqth");
    }
    if s.callook_lookup {
        out.push("callook");
    }
    out
}

/// Now, as the fill's 12-month location gate reads it.
fn now_secs() -> u64 {
    u64::try_from(crate::now_unix()).unwrap_or(0)
}

/// What "Update from callbook" would do with its contacts. Changes nothing and asks no callbook.
#[tauri::command]
pub async fn callbook_update_preview(
    app: AppHandle,
    state: State<'_, SharedEngine>,
    queries: State<'_, LogQueries>,
    target: BulkTarget,
) -> Result<UpdatePreviewDto, String> {
    let (engine, queries) = (Arc::clone(&state), queries.inner().clone());
    tokio::task::spawn_blocking(move || {
        let ids = target_ids(&engine, &queries, &target)?;
        let (max_age, pace, cap, services) = {
            let e = engine_lock(&engine);
            let s = e.settings();
            (
                s.callbook_cache_max_age_secs(),
                s.callbook_update_pace(),
                s.callbook_update_daily_cap(),
                services(s),
            )
        };
        let cache = app.state::<SharedCallbookCache>();
        let mut lookups_needed = 0;
        let p = logtools::callbook_preview(&engine, &ids, now_secs(), &mut |call| {
            let hit = kept(&cache, call, max_age);
            if hit.is_none() {
                lookups_needed += crate::callbook_candidates(call).len();
            }
            hit
        })?;
        let qrz_count = cache.store().and_then(|s| {
            s.lock()
                .ok()
                .and_then(|s| s.qrz_count(crate::now_unix()).ok().flatten())
        });
        let to_look_up = p.calls.len() - p.kept;
        let mut fields: Vec<(String, usize)> = p.fields.into_iter().collect();
        fields.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(UpdatePreviewDto {
            ids: ids.iter().map(ToString::to_string).collect(),
            selected: p.selected,
            missing: p.missing,
            calls: p.calls.len(),
            kept: p.kept,
            to_look_up,
            lookups_needed,
            pace_secs: pace.as_secs(),
            estimate_secs: pace.as_secs() * to_look_up as u64,
            spent: spent(&app),
            cap,
            qrz_count,
            could_change: p.could_change,
            fields,
            configured: !services.is_empty(),
            services,
            location_too_old: p.location_too_old,
        })
    })
    .await
    .map_err(|e| format!("logbook task failed: {e}"))?
}

/// The word for why an update ended.
fn stop_word(stop: &Stop) -> (&'static str, Option<String>) {
    match stop {
        Stop::Done => ("done", None),
        Stop::Cancelled => ("cancelled", None),
        Stop::Limit => ("limit", None),
        Stop::Refused => ("refused", None),
        Stop::Failed(why) => ("failed", Some(why.clone())),
    }
}

/// Start an update of the contacts the preview counted — `ids`, as the preview answered them —
/// only while they are the `expect_count` the operator confirmed. Answers at once; the progress
/// is `callbook_update_status`. Refused while one is running.
#[tauri::command]
pub async fn callbook_update_start(
    app: AppHandle,
    state: State<'_, SharedEngine>,
    job: State<'_, CallbookUpdate>,
    ids: Vec<String>,
    expect_count: usize,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    let ids: Vec<RecordId> = ids
        .iter()
        .map(|id| id_of(id))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|id| seen.insert(*id))
        .collect();
    if ids.len() != expect_count {
        return Err(logtools::NOT_AS_PREVIEWED.into());
    }
    let engine = Arc::clone(&state);
    let (max_age, limits) = {
        let e = engine_lock(&engine);
        let s = e.settings();
        (
            s.callbook_cache_max_age_secs(),
            Limits {
                pace: s.callbook_update_pace(),
                daily_cap: s.callbook_update_daily_cap(),
            },
        )
    };
    {
        let mut status = lock(&job.status);
        if status.running {
            return Err("An update from the callbook is already running.".into());
        }
        *status = UpdateStatusDto {
            running: true,
            cap: limits.daily_cap,
            ..UpdateStatusDto::default()
        };
    }
    job.cancel.store(false, Ordering::SeqCst);
    std::thread::Builder::new()
        .name("callbook-update".into())
        .spawn(move || {
            let ended = run(&app, &engine, &ids, expect_count, max_age, limits);
            app.state::<CallbookUpdate>().end(ended);
        })
        .map(|_| ())
        .map_err(|e| {
            lock(&job.status).running = false;
            format!("The update could not be started: {e}")
        })
}

/// The update itself, on its own thread: the calls looked up, then what they answered filled as
/// one run.
fn run(
    app: &AppHandle,
    engine: &SharedEngine,
    ids: &[RecordId],
    expect: usize,
    max_age: i64,
    limits: Limits,
) -> UpdateEndDto {
    let calls = match logtools::callbook_preview(engine, ids, now_secs(), &mut |_| None) {
        Ok(p) => p.calls,
        Err(e) => {
            return UpdateEndDto {
                stop: "failed".into(),
                error: Some(e),
                ..UpdateEndDto::default()
            }
        }
    };
    let job = app.state::<CallbookUpdate>();
    lock(&job.status).total = calls.len();
    let others = {
        let e = engine_lock(engine);
        services(e.settings()).iter().any(|s| *s != "qrz")
    };
    let mut callbook = StationCallbook {
        app: app.clone(),
        max_age,
        skip_qrz: false,
        others,
    };
    let found = callbook_update::look_up(&calls, limits, &mut callbook, &mut |p: &Progress| {
        let mut status = lock(&job.status);
        status.done = p.done;
        status.from_cache = p.from_cache;
        status.looked_up = p.looked_up;
        status.spent = p.spent;
    });
    {
        let mut status = lock(&job.status);
        status.done = found.progress.done;
        status.spent = found.progress.spent;
    }
    let (stop, error) = stop_word(&found.stop);
    let (qrz_said, qrz_skipped) = {
        let status = lock(&job.status);
        (status.qrz_said.clone(), status.qrz_skipped)
    };
    let mut end = UpdateEndDto {
        stop: stop.into(),
        error,
        qrz_said,
        qrz_skipped,
        not_looked_up: calls.len() - found.progress.done,
        unknown: found.unknown,
        made: None,
    };
    if found.answers.is_empty() {
        return end;
    }
    let (made, why, durability) =
        logtools::callbook_fill(engine, ids, expect, &found.answers, now_secs(), &mut || {
            wait_for_tx(&job, engine)
        });
    // A fill stopped part-way hands back its run too: the toast's Undo takes what it made.
    if why.is_none() || made.run.is_some() {
        let wait = if durability.turned_back(tempo_app::logstore::DURABLE_WAIT) {
            Err(tempo_app::station::LOG_BUSY.to_string())
        } else {
            durability
                .wait(tempo_app::logstore::DURABLE_WAIT)
                .map_err(crate::durability_failed)
        };
        if let Err(why) = wait {
            end.error = Some(why);
        }
        end.made = Some(run_dto(made));
    }
    if why.is_some() {
        end.error = why;
    }
    end
}

/// The fill's wait while the radio transmits: until transmit is off (`true`), or the operator
/// stops the update (`false`). The status says it waits.
fn wait_for_tx(job: &CallbookUpdate, engine: &SharedEngine) -> bool {
    lock(&job.status).waiting_for_tx = true;
    let off = loop {
        if job.cancel.load(Ordering::SeqCst) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(250));
        if !engine_lock(engine).transmit_needs_the_lock() {
            break true;
        }
    };
    lock(&job.status).waiting_for_tx = false;
    off
}

impl CallbookUpdate {
    /// The update has ended: its status says so, and the end is kept until the Logbook takes it.
    fn end(&self, ended: UpdateEndDto) {
        *lock(&self.unreported) = Some(ended.clone());
        let mut status = lock(&self.status);
        status.running = false;
        status.waiting_for_tx = false;
        status.ended = Some(ended);
    }

    /// The end not reported yet, once: a second take has none.
    fn take_end(&self) -> Option<UpdateEndDto> {
        lock(&self.unreported).take()
    }
}

/// The last update's end, if the Logbook has not reported it yet; once.
#[tauri::command]
pub fn callbook_update_take_end(job: State<'_, CallbookUpdate>) -> Option<UpdateEndDto> {
    job.take_end()
}

/// How the update is getting on, or how the last one ended.
#[tauri::command]
pub fn callbook_update_status(job: State<'_, CallbookUpdate>) -> UpdateStatusDto {
    lock(&job.status).clone()
}

/// Stop the update after the lookup in hand: what it found so far is filled as one run.
#[tauri::command]
pub fn callbook_update_cancel(job: State<'_, CallbookUpdate>) {
    job.cancel.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kept answer counts only while fresh by the operator's expiry, and comes back as the log
    /// takes it (the name by the log strip's rule).
    #[test]
    fn a_kept_answer_counts_only_while_fresh() {
        let cache = SharedCallbookCache::with(
            tempo_core::callbook_cache::CallbookCache::open_in_memory(10).unwrap(),
        );
        let dto = tempo_app::dto::QrzLookupDto {
            call: "W1AW".into(),
            name: Some("Hiram Maxim".into()),
            nickname: Some("Hiram".into()),
            ..Default::default()
        };
        let now = crate::now_unix();
        cache.put("W1AW", &dto, now - 100);
        let a = kept(&cache, "w1aw", 1_000).expect("fresh");
        assert_eq!(
            (a.call.as_str(), a.name.as_deref()),
            ("W1AW", Some("Hiram"))
        );
        assert_eq!(kept(&cache, "W1AW", 50), None, "older than the expiry");
        assert_eq!(kept(&cache, "K1ABC", 1_000), None);
    }

    /// What the lookups note is taken once: a later take without a new lookup has nothing.
    #[test]
    fn what_the_lookups_note_is_taken_once() {
        let _one = lock(&HEARD_TESTS);
        let _ = take_heard();
        let session = |xml: &str| tempo_core::qrz::parse_session(xml);
        note_qrz_session(&session(
            "<QRZDatabase><Session><Key>k</Key><Count>42</Count></Session></QRZDatabase>",
        ));
        note_qrz_session(&session(
            "<QRZDatabase><Session><Key>k</Key></Session></QRZDatabase>",
        ));
        note_qrz_answer(false);
        note_login();
        note_login();
        note_hamqth_refused();
        assert_eq!(
            take_heard(),
            Heard {
                qrz_count: Some(42),
                qrz_record: false,
                qrz_no_record: true,
                logins: 2,
                hamqth_refused: true,
                counted_request: true,
                ..Heard::default()
            },
            "an answer with no count keeps the last"
        );
        assert_eq!(take_heard(), Heard::default());
    }

    /// A scripted QRZ: answers each request by its URL, and records every URL asked.
    struct ScriptedQrz {
        urls: Mutex<Vec<String>>,
        answer: Box<dyn Fn(&str) -> String + Send + Sync>,
    }

    impl ScriptedQrz {
        fn new(answer: impl Fn(&str) -> String + Send + Sync + 'static) -> Self {
            Self {
                urls: Mutex::new(Vec::new()),
                answer: Box::new(answer),
            }
        }
        fn fetch(&self, url: &str) -> Result<String, String> {
            lock(&self.urls).push(url.to_string());
            Ok((self.answer)(url))
        }
        fn logins(&self) -> usize {
            lock(&self.urls)
                .iter()
                .filter(|u| u.contains("username="))
                .count()
        }
    }

    fn qrz_body(session: &str, record: &str) -> String {
        format!("<QRZDatabase><Session>{session}</Session>{record}</QRZDatabase>")
    }

    const RECORD: &str = "<Callsign><call>W1AW</call><fname>Hiram</fname></Callsign>";

    /// ★ ONE QRZ SIGN-IN PER SESSION, AND QRZ'S AGENT ON EVERY REQUEST. Three lookups sign in
    /// once and reuse the key; when QRZ says the session timed out, ONE sign-in again. Every
    /// request, sign-in and lookup, carries `agent=Nexus-<version>`.
    #[test]
    fn a_run_signs_in_to_qrz_once_and_names_itself_on_every_request() {
        let _one = lock(&HEARD_TESTS);
        let timed_out = std::sync::atomic::AtomicBool::new(false);
        let timed_out = Arc::new(timed_out);
        let t = Arc::clone(&timed_out);
        let qrz = ScriptedQrz::new(move |url| {
            if url.contains("username=") {
                qrz_body("<Key>K1</Key>", "")
            } else if t.swap(false, Ordering::SeqCst) {
                qrz_body("<Error>Session Timeout</Error>", "")
            } else {
                qrz_body("<Key>K1</Key>", RECORD)
            }
        });
        let session = crate::SharedQrzSession::default();
        let fetch = |u: &str| qrz.fetch(u);
        for _ in 0..3 {
            let got = crate::qrz_lookup_attempt("W1AW", "AA7BQ", "pw", &session, &fetch);
            assert!(matches!(got, Ok(crate::QrzOutcome::Found(..))));
        }
        assert_eq!(qrz.logins(), 1, "one sign-in for three lookups");
        timed_out.store(true, Ordering::SeqCst);
        let got = crate::qrz_lookup_attempt("W1AW", "AA7BQ", "pw", &session, &fetch);
        assert!(matches!(got, Ok(crate::QrzOutcome::Found(..))));
        assert_eq!(qrz.logins(), 2, "one sign-in again after the timeout");
        let agent = format!("agent=Nexus-{}", env!("CARGO_PKG_VERSION"));
        let urls = lock(&qrz.urls).clone();
        assert_eq!(urls.len(), 7, "{urls:?}");
        assert!(urls.iter().all(|u| u.contains(&agent)), "{urls:?}");
        let _ = take_heard();
    }

    /// QRZ turning down a key it just issued is noted, and stops the update.
    #[test]
    fn qrz_turning_down_its_fresh_key_stops_the_update() {
        let _one = lock(&HEARD_TESTS);
        let _ = take_heard();
        let qrz = ScriptedQrz::new(|url| {
            if url.contains("username=") {
                qrz_body("<Key>K1</Key>", "")
            } else {
                qrz_body("<Error>Invalid session key</Error>", "")
            }
        });
        let fetch = |u: &str| qrz.fetch(u);
        let got = crate::qrz_lookup_attempt(
            "W1AW",
            "AA7BQ",
            "pw",
            &crate::SharedQrzSession::default(),
            &fetch,
        );
        assert!(matches!(got, Ok(crate::QrzOutcome::NeedLogin)));
        let a = asked(Looked::Unknown, take_heard());
        assert_eq!(a.looked, Looked::Failed(QRZ_FRESH_KEY_REFUSED.into()));
    }

    /// ★ "CONNECTION REFUSED" STOPS EVERYTHING FOR A DAY. QRZ refusing the sign-in is a refusal
    /// (not a bad password, not a miss), asked of QRZ once with no second sign-in; the update
    /// stops on it even when another callbook answered the call; and the day's hold, kept in the
    /// cache file, says how long in the operator's words.
    #[test]
    fn qrz_refusing_service_stops_the_run_and_holds_for_a_day() {
        let _one = lock(&HEARD_TESTS);
        let _ = take_heard();
        let qrz = ScriptedQrz::new(|_| qrz_body("<Error>Connection refused</Error>", ""));
        let fetch = |u: &str| qrz.fetch(u);
        let got = crate::qrz_lookup_attempt(
            "W1AW",
            "AA7BQ",
            "pw",
            &crate::SharedQrzSession::default(),
            &fetch,
        );
        let f = got.err().expect("refused");
        assert!(f.refused);
        assert_eq!(
            f.message.into_string(),
            "QRZ refused service for this account: Connection refused"
        );
        assert_eq!(lock(&qrz.urls).len(), 1, "never asked again");
        assert_eq!(
            take_heard().qrz_said.as_deref(),
            Some("Connection refused"),
            "QRZ's words, for the operator"
        );

        // A refusal on a lookup with a key is the same refusal, and is not a miss.
        assert!(matches!(
            crate::qrz_outcome_from_body(&qrz_body("<Error>Connection refused</Error>", "")),
            Err(crate::QrzFailure { refused: true, .. })
        ));

        // The run stops, whatever the call's answer.
        let refused = Heard {
            qrz_refused: true,
            ..Heard::default()
        };
        let found = Looked::Found(Box::default());
        assert_eq!(
            asked(found, refused).looked,
            Looked::Failed(QRZ_REFUSED.into())
        );

        // Kept for a day, in the cache file.
        let cache = SharedCallbookCache::with(
            tempo_core::callbook_cache::CallbookCache::open_in_memory(10).unwrap(),
        );
        let now = 1_000_000;
        assert_eq!(cache.qrz_refused_until(now), None);
        cache.note_qrz_refused(now + 86_400);
        assert_eq!(cache.qrz_refused_until(now + 86_399), Some(now + 86_400));
        assert_eq!(cache.qrz_refused_until(now + 86_400), None);
        assert!(crate::qrz_refused_until_words(now + 86_400, now + 3_600)
            .contains("for about 23 more hours"));
    }

    /// ★ QRZ'S WORDS REACH THE OPERATOR. Its `<Message>` and `<Error>` are noted as one line (no
    /// control characters, at most 200 characters); "not in the callbook" with QRZ's words after
    /// it is still an unknown call.
    #[test]
    fn qrzs_message_and_error_reach_the_operator() {
        let _one = lock(&HEARD_TESTS);
        let _ = take_heard();
        note_qrz_session(&tempo_core::qrz::parse_session(&qrz_body(
            "<Key>k</Key><Message>A subscription is required to obtain the complete \
             data.</Message><Error>Not found: G1SRDD</Error>",
            "",
        )));
        assert_eq!(
            take_heard().qrz_said.as_deref(),
            Some("A subscription is required to obtain the complete data. — Not found: G1SRDD")
        );
        assert_eq!(screen_words("a\nb\u{7}  c"), "a b c");
        assert_eq!(screen_words(&"x".repeat(500)).len(), 200);

        let e = format!(
            "{} QRZ: Not found: G1SRDD",
            crate::callbook_not_found("G1SRDD")
        );
        assert!(crate::is_callbook_not_found(&e, "G1SRDD"));
        assert!(crate::is_callbook_not_found(
            &crate::callbook_not_found("G1SRDD"),
            "G1SRDD"
        ));
        assert!(!crate::is_callbook_not_found(&e, "G1SRD"));
        let mut cb = over_cache(Err(e));
        let run = callbook_update::look_up(&["G1SRDD".to_string()], limits(), &mut cb, &mut |_| {});
        assert_eq!(run.unknown, ["G1SRDD"]);
    }

    /// ★ A NON-SUBSCRIBER IS LEFT OUT OF THE UPDATE. `<SubExp>non-subscriber</SubExp>` turns QRZ
    /// off for the rest of the run, and it stays off; a subscriber's expiry date does not.
    #[test]
    fn a_non_subscriber_account_is_left_out_of_the_update() {
        let _one = lock(&HEARD_TESTS);
        let _ = take_heard();
        let sub = |exp: &str| {
            note_qrz_session(&tempo_core::qrz::parse_session(&qrz_body(
                &format!("<Key>k</Key><SubExp>{exp}</SubExp>"),
                "",
            )));
            take_heard()
        };
        assert!(skips_qrz(false, &sub("non-subscriber")));
        assert!(!skips_qrz(false, &sub("Wed Jan 1 12:34:03 2027")));
        assert!(skips_qrz(true, &Heard::default()), "stays off");
    }

    /// ★ CALLOOK.INFO DOWN FOR A WHILE. Its not answering leaves the call for later, not a
    /// failure that stops the run; uncounted when no other service was asked, counted when one
    /// was.
    #[test]
    fn callook_not_answering_is_later_not_a_failure() {
        let down = || Heard {
            callook_unavailable: true,
            ..Heard::default()
        };
        let a = asked(Looked::Failed(crate::CALLOOK_LATER.into()), down());
        assert_eq!((a.looked, a.uncounted), (Looked::Later, true));
        let after_qrz = Heard {
            counted_request: true,
            ..down()
        };
        let a = asked(Looked::Failed(crate::CALLOOK_LATER.into()), after_qrz);
        assert_eq!((a.looked, a.uncounted), (Looked::Later, false));
        // Not answering is never "not in the callbook".
        assert!(!tempo_core::callook::answered(r#"{"status":"UPDATING"}"#));
    }

    /// The update's callbook over a real cache, its network a script: `answer` is what every
    /// lookup of the callbooks answers.
    struct OverCache {
        cache: SharedCallbookCache,
        max_age: i64,
        answer: Result<tempo_app::dto::QrzLookupDto, String>,
        asked: Vec<String>,
        tally: usize,
    }

    impl Callbook for OverCache {
        fn kept(&mut self, call: &str) -> Option<CallbookAnswer> {
            kept(&self.cache, call, self.max_age)
        }
        fn spent(&mut self) -> usize {
            self.tally
        }
        fn note_spent(&mut self, n: usize) {
            self.tally += n;
        }
        fn forget_spent(&mut self, n: usize) {
            self.tally -= n;
        }
        fn live(&mut self, call: &str) -> Asked {
            self.asked.push(call.to_string());
            let answer = self.answer.clone();
            Asked::plain(tauri::async_runtime::block_on(ask(
                &self.cache,
                call,
                crate::now_unix(),
                || async move { answer },
            )))
        }
        fn wait(&mut self, _: Duration) -> bool {
            true
        }
        fn cancelled(&self) -> bool {
            false
        }
    }

    fn over_cache(answer: Result<tempo_app::dto::QrzLookupDto, String>) -> OverCache {
        OverCache {
            cache: SharedCallbookCache::with(
                tempo_core::callbook_cache::CallbookCache::open_in_memory(10).unwrap(),
            ),
            max_age: 1_000,
            answer,
            asked: Vec::new(),
            tally: 0,
        }
    }

    fn limits() -> Limits {
        Limits {
            pace: Duration::ZERO,
            daily_cap: 250,
        }
    }

    /// ★ AN EXPIRED ANSWER NEVER HIDES A FAILED LOOKUP. W1AW's kept answer is past the expiry, so
    /// it is asked of the callbooks, and the sign-in is turned down: the run stops on that first
    /// failure (K1ABC is never asked, so QRZ is not asked to sign in again), and nothing is
    /// filled from the expired answer.
    #[test]
    fn an_expired_answer_never_stands_in_for_a_failed_lookup() {
        let refused = "QRZ login failed: incorrect password".to_string();
        let mut cb = over_cache(Err(refused.clone()));
        let dto = tempo_app::dto::QrzLookupDto {
            call: "W1AW".into(),
            name: Some("Hiram Maxim".into()),
            ..Default::default()
        };
        cb.cache.put("W1AW", &dto, crate::now_unix() - 10_000);
        let run = callbook_update::look_up(
            &["W1AW".to_string(), "K1ABC".to_string()],
            limits(),
            &mut cb,
            &mut |_| {},
        );
        assert_eq!(run.stop, Stop::Failed(refused));
        assert_eq!(cb.asked, ["W1AW"], "stopped at the first failure");
        assert!(run.answers.is_empty(), "nothing from the expired answer");
        assert_eq!(cb.tally, 1);
    }

    /// The control: a lookup that answers is kept in the cache, fresh, and "not in the callbook"
    /// is an unknown call, not a failure.
    #[test]
    fn an_answer_is_kept_and_not_found_is_unknown() {
        let dto = tempo_app::dto::QrzLookupDto {
            call: "W1AW".into(),
            name: Some("Hiram Maxim".into()),
            ..Default::default()
        };
        let mut cb = over_cache(Ok(dto));
        let run = callbook_update::look_up(&["W1AW".to_string()], limits(), &mut cb, &mut |_| {});
        assert_eq!(run.stop, Stop::Done);
        assert_eq!(run.answers["W1AW"].name.as_deref(), Some("Hiram Maxim"));
        assert!(kept(&cb.cache, "W1AW", 1_000).is_some(), "kept, fresh");

        let mut cb = over_cache(Err(crate::callbook_not_found("Q9QQQ")));
        let run = callbook_update::look_up(&["Q9QQQ".to_string()], limits(), &mut cb, &mut |_| {});
        assert_eq!(
            (run.stop, run.unknown),
            (Stop::Done, vec!["Q9QQQ".to_string()])
        );
    }

    /// What the services said, as the loop reads it: QRZ with no record for the call (and none
    /// for its base) is a no-record answer, a record for either is not; HamQTH turning down its
    /// own session fails the lookup; the sign-ins are counted.
    #[test]
    fn what_the_services_said_reaches_the_loop() {
        let no_record = Heard {
            qrz_no_record: true,
            logins: 1,
            ..Heard::default()
        };
        let a = asked(Looked::Unknown, no_record);
        assert_eq!(a.looked, Looked::Unknown);
        assert!(a.qrz_without_record);
        assert_eq!(a.logins, 1);

        let base_found = Heard {
            qrz_no_record: true,
            qrz_record: true,
            ..Heard::default()
        };
        assert!(!asked(Looked::Unknown, base_found).qrz_without_record);
        assert!(
            !asked(Looked::Unknown, Heard::default()).qrz_without_record,
            "QRZ not asked"
        );

        let hamqth = Heard {
            hamqth_refused: true,
            ..Heard::default()
        };
        assert_eq!(
            asked(Looked::Unknown, hamqth).looked,
            Looked::Failed(HAMQTH_REFUSED.into())
        );
    }

    /// ★ THE END IS REPORTED ONCE, WHENEVER IT IS ASKED. The run ends with no one watching: the
    /// end is kept, the first take has it and the second has nothing, and the status still
    /// shows it for the dialog opened again.
    #[test]
    fn the_end_is_taken_once() {
        let job = CallbookUpdate::default();
        lock(&job.status).running = true;
        assert_eq!(job.take_end(), None, "nothing ended yet");
        let end = UpdateEndDto {
            stop: "limit".into(),
            not_looked_up: 4,
            ..UpdateEndDto::default()
        };
        job.end(end.clone());
        assert_eq!(job.take_end(), Some(end.clone()));
        assert_eq!(job.take_end(), None, "once");
        let status = lock(&job.status).clone();
        assert!(!status.running);
        assert_eq!(status.ended, Some(end));
    }

    /// The one failure the update goes on after is the lookup's own "not in the callbook", word
    /// for word, for the call as asked.
    #[test]
    fn not_found_is_the_lookups_own_words() {
        assert_eq!(
            crate::callbook_not_found("w1aw/p"),
            "W1AW/P is not in the callbook."
        );
    }
}
