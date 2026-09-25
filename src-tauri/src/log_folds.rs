//! The log's folds and lookups, read from the logbook store — SPEC-2 v3's **C14**.
//!
//! Every pass the command layer makes over the log — the needs model the Needed board,
//! propagation, satellite passes and the pounce thread share; the award summary; the Journey;
//! the log statistics; today's hunted parks; a station's last logged grid; the upload health the
//! Settings panel polls; the oldest LoTW upload awaiting its echo; the catch-up sweep's pick; the
//! compound calls the launch seeds the decoder with — reads the store here, through ONE door
//! ([`LogRows`]), and runs the fold it always ran over what it reads.
//!
//! # Four rules each of them keeps
//!
//! 1. **The same Rust fold, fed by SQL.** A fold names the columns it reads ([`Narrow`]) and is
//!    handed records with exactly those filled, one at a time, in log order, by the store's one
//!    decoder. What it computes from them is today's code: `LogNeeds::add_qso`, `Awards::add_qso`,
//!    `journey_model`, `LogStatsAccumulator`, `HuntedActivations::from_log`, the upload health's
//!    own step. A narrower read only ever SELECTS rows (P2: SQL narrows, Rust decides), and
//!    the fold still runs its own test on every row it is handed.
//! 2. **Never under the Engine lock.** Under the lock a fold takes only a watermark and the
//!    log's rows ([`LogRows`]: handles, or a copy of pointers on the 1.13 path); it reads and
//!    folds after the lock is released. A debug build panics if a read runs under an Engine
//!    guard (`tempo_core::logbook::io_fence`).
//! 3. **Read your writes (P4).** A read first waits for every change made before its rows were
//!    taken, so a fold asked for straight after a contact is logged sees the contact. If the
//!    writer is behind (a bulk import, a slow disk) past [`tempo_app::logstore::READ_WAIT`], the
//!    fold answers from the store as it stands — and that answer is NOT kept, so the next ask
//!    reads again.
//! 4. **Kept against the narrowest watermark that is still correct** ([`Tally`]).
//!
//! # The watermarks, and where SPEC-2 v3 §4.4 had two of them wrong
//!
//! A fold over fields no upload stamp, QSL-sent mark or id adoption can change is kept against
//! `index_rev` ([`Engine::log_index_rev`]): the needs model, the awards, the Journey and the
//! statistics survive an upload stamp, where they used to fold the whole log again on every
//! one. Each of them has a test that stamps a row and finds the kept answer reused, and still
//! the fresh answer.
//!
//! ⚠️ The spec kept the confirmation diagnostics on `index_rev` too, and the upload health on
//! `content_rev`. Both read the upload stamps. A stamp is `OpClass::Stamp`, which moves
//! `content_rev` and not `index_rev`, so a diagnosis kept on `index_rev` would go on saying
//! "never uploaded" after the upload. And an import that only ADDS rows is an `Append`, which
//! moves `index_rev` and not `content_rev` — and an imported row can carry stamps (a Nexus
//! export's `APP_TEMPO_UL_*`, or the Accepted stamp the reader makes of `LOTW_QSL_SENT=Y`), so
//! an upload health kept on `content_rev` would miss them. Every change moves `revision`, and
//! that is what the upload health is kept against; the diagnosis is not kept at all, as before.
//!
//! Today's hunted parks are not kept either: they are read through the `qso_recent` index for
//! today's contacts alone, which is the whole of their cost.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use tempo_app::engine::{engine_lock, Engine};
use tempo_app::logstore::{Freshness, LogRows};
use tempo_core::logbook::sqlite::{call_norm_of, Narrow, Order, Scope};
use tempo_core::logbook::QsoRecord;

use crate::{qso_is_sat, LogTallies, Tally};

// ── what each pass reads ────────────────────────────────────────────────────
//
// The four confirmation channels ride with any pass that reads `award_confirmed` or
// `confirmed`: the decoder derives both from them, exactly as it does for a whole record.

/// `LogNeeds::add_qso`'s inputs.
const NEEDS: Narrow = Narrow {
    columns: &[
        "call",
        "band",
        "mode",
        "grid",
        "state",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
        "prop_mode",
    ],
    uploads: false,
};

/// [`award_add`]'s inputs.
const AWARDS: Narrow = Narrow {
    columns: &[
        "call",
        "band",
        "mode",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
        "credit_granted",
        "state",
        "grid",
        "ota_iota",
        "prop_mode",
    ],
    uploads: false,
};

/// [`journey_qso`]'s inputs.
const JOURNEY: Narrow = Narrow {
    columns: &[
        "call",
        "grid",
        "state",
        "band",
        "mode",
        "when_unix",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
        "rst_rcvd",
        "ota_their_program",
        "ota_their_ref",
    ],
    uploads: false,
};

/// A callsign, and nothing else: the statistics, and the compound calls the launch seeds.
const CALLS: Narrow = Narrow {
    columns: &["call"],
    uploads: false,
};

/// [`hunted_today`]'s inputs.
const HUNTED: Narrow = Narrow {
    columns: &["call", "when_unix", "ota_their_ref"],
    uploads: false,
};

/// [`newest_logged_grid`]'s inputs.
const GRIDS: Narrow = Narrow {
    columns: &["call", "grid", "when_unix"],
    uploads: false,
};

/// The upload health's input: the stamps.
const STAMPS: Narrow = Narrow {
    columns: &[],
    uploads: true,
};

/// [`oldest_pending_lotw_date`]'s inputs.
const PENDING: Narrow = Narrow {
    columns: &["when_unix"],
    uploads: true,
};

/// Keep a fold's answer against `mark`, and hand it back — unless the read behind it was stale
/// (the writer had not taken every change made before it was asked), when the answer is handed
/// back and NOT kept, so the next asker reads the store again.
fn keep<K: PartialEq, V>(
    tally: &Tally<K, V>,
    mark: u64,
    key: K,
    value: V,
    fresh: &Freshness,
) -> Arc<V> {
    match fresh {
        Freshness::Current => tally.put(mark, key, value),
        Freshness::Stale(_) => {
            crate::note_log_tally();
            Arc::new(value)
        }
    }
}

/// What a pass says when the store could not be read. A fold that fails says so — it never
/// answers with an empty model, which would call every station on the air a new one.
fn unreadable(e: tempo_core::logbook::sqlite::Error) -> String {
    format!("the logbook could not be read: {e}")
}

/// Run a command's read of the log on the blocking pool: it reads the disk, and it may wait
/// (bounded) for the writer to take the changes made before it was asked (P4) — neither of which
/// may pin a runtime worker the rest of the app shares.
pub(crate) async fn off_the_runtime<T: Send + 'static>(
    read: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(read)
        .await
        .map_err(|e| format!("the logbook read did not finish: {e}"))?
}

// ── the needs model ─────────────────────────────────────────────────────────

/// What the needs model is built from, taken under the engine lock: the kept model when the
/// log's content has not moved since it was folded, or else the log's rows — handles, or a copy
/// of pointers on the 1.13 path — to fold once the lock is released ([`needs_finish`]).
pub(crate) enum NeedsCapture {
    Kept(Arc<propagation::LogNeeds>),
    Fold { mark: u64, rows: LogRows },
}

/// The needs model's half that runs UNDER the engine lock: a watermark and, only when the model
/// must be folded again, the log's rows. No read of the log.
pub(crate) fn needs_capture(eng: &Engine, kept: &Tally<(), propagation::LogNeeds>) -> NeedsCapture {
    let mark = eng.log_index_rev();
    match kept.get(mark, &()) {
        Some(needs) => NeedsCapture::Kept(needs),
        None => NeedsCapture::Fold {
            mark,
            rows: eng.log_rows(),
        },
    }
}

/// The needs model's half that runs with the engine lock RELEASED: the fold, when the capture
/// needs one, kept for the next reader. The fold is the one it always was — every contact, in
/// log order, into `LogNeeds::add_qso` — so the model is the same model.
pub(crate) fn needs_finish(
    capture: NeedsCapture,
    kept: &Tally<(), propagation::LogNeeds>,
) -> Result<Arc<propagation::LogNeeds>, String> {
    let (mark, rows) = match capture {
        NeedsCapture::Kept(needs) => return Ok(needs),
        NeedsCapture::Fold { mark, rows } => (mark, rows),
    };
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("the needs model's fold");
    let mut needs = propagation::LogNeeds::new();
    let fresh = rows
        .each(NEEDS, Scope::All, Order::Log, &mut |q| {
            // A "needs confirmation" must be award-grade (LoTW/paper), not eQSL.
            needs.add_qso(
                &q.call,
                &q.band,
                &q.mode,
                q.grid.as_deref(),
                q.state.as_deref(),
                q.award_confirmed,
                qso_is_sat(q.prop_mode.as_deref()),
            );
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    Ok(keep(kept, mark, (), needs, &fresh))
}

/// The award-needs model of the whole log, folded again only when the log's content moves, with
/// the log's identity for Remote's staleness check taken under the same lock (see
/// [`crate::PropContext`]).
pub(crate) fn needs_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<(Arc<propagation::LogNeeds>, u64), String> {
    let eng = engine_lock(engine);
    let log = crate::prop_log_identity(&eng);
    let capture = needs_capture(&eng, &tallies.needs);
    drop(eng);
    Ok((needs_finish(capture, &tallies.needs)?, log))
}

// ── the award summary ───────────────────────────────────────────────────────

/// `get_awards`'s summary, folded again only when the log's content or the operator's call (the
/// home entity "First DX" is judged against) has moved. The main window polls it twice a minute.
pub(crate) fn awards_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<propagation::AwardSummary>, String> {
    let eng = engine_lock(engine);
    let (mark, my_call) = (eng.log_index_rev(), eng.settings().mycall.clone());
    if let Some(kept) = tallies.awards.get(mark, &my_call) {
        return Ok(kept);
    }
    let rows = eng.log_rows();
    drop(eng);
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("the awards fold");
    let mut awards = propagation::Awards::new();
    // Tell the accumulator our own entity so "First DX" counts only foreign ones.
    awards.set_home_call(&my_call);
    let fresh = rows
        .each(AWARDS, Scope::All, Order::Log, &mut |q| {
            award_add(&mut awards, q);
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    Ok(keep(
        &tallies.awards,
        mark,
        my_call,
        awards.summary(),
        &fresh,
    ))
}

/// The native award fold over records in hand — the reference Remote's read is held to in its
/// tests.
#[cfg(test)]
pub(crate) fn awards_for_records<R: std::borrow::Borrow<QsoRecord>>(
    records: &[R],
    my_call: &str,
) -> propagation::AwardSummary {
    let mut awards = propagation::Awards::new();
    // Tell the accumulator our own entity so "First DX" counts only foreign ones.
    awards.set_home_call(my_call);
    for q in records {
        award_add(&mut awards, std::borrow::Borrow::borrow(q));
    }
    awards.summary()
}

/// One contact into the award fold — the one step [`awards_kept`] and [`awards_for_records`]
/// both take.
fn award_add(awards: &mut propagation::Awards, q: &QsoRecord) {
    // Award-eligible confirmation only (LoTW/paper) — eQSL doesn't count; plus whether ARRL has
    // granted DXCC-family credit (DXCC / DXCC_BAND / DXCC_MODE / … — real LoTW exports use the
    // granular codes).
    let credited = q.credit_granted.iter().any(|c| c.starts_with("DXCC"));
    awards.add_qso(
        &q.call,
        &q.band,
        &q.mode,
        q.award_confirmed,
        credited,
        // The paper-card channel alone — IOTA's award gate (the IOTA program accepts cards and
        // Club Log matching, never LoTW).
        q.qsl_rcvd.card,
        q.state.as_deref(),
        q.grid.as_deref(),
        q.ota.iota.as_deref(),
        qso_is_sat(q.prop_mode.as_deref()),
    );
}

// ── the log statistics ──────────────────────────────────────────────────────

/// `get_log_stats`' answer — the geographic slice of the log — folded again only when the log's
/// content or the operator's call moves. Every contact's call, read and counted after the lock
/// is released (where a clone of every record used to be taken under it, 165 ms at 150,000).
pub(crate) fn log_stats(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<propagation::LogStats>, String> {
    let eng = engine_lock(engine);
    let (mark, my_call) = (eng.log_index_rev(), eng.settings().mycall.clone());
    if let Some(kept) = tallies.stats.get(mark, &my_call) {
        return Ok(kept);
    }
    let rows = eng.log_rows();
    drop(eng);
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("the log statistics");
    // `compute_log_stats`' own accumulator, a call at a time.
    let mut stats = propagation::stats::LogStatsAccumulator::new(&my_call);
    let fresh = rows
        .each(CALLS, Scope::All, Order::Log, &mut |q| {
            stats.add(&q.call);
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    Ok(keep(&tallies.stats, mark, my_call, stats.summary(), &fresh))
}

// ── the Journey ─────────────────────────────────────────────────────────────

/// What `get_journey`'s model is built from besides the log.
#[derive(PartialEq)]
pub(crate) struct JourneyKey {
    pub(crate) call: String,
    pub(crate) grid: String,
    pub(crate) power_w: Option<f64>,
    pub(crate) streak: bool,
}

/// `get_journey`'s model of the log, rebuilt only when the log's content or a setting it reads
/// has moved. The clock is NOT part of the key: the model finishes the weekly streak and the
/// annual marathon for whatever time it is asked about (`propagation::JourneyModel`), so a kept
/// model answers each once-a-minute poll exactly as a fresh computation would.
pub(crate) fn journey_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<propagation::JourneyModel>, String> {
    let eng = engine_lock(engine);
    let s = eng.settings();
    let key = JourneyKey {
        call: s.mycall.clone(),
        grid: s.mygrid.clone(),
        power_w: s.station_power_w,
        streak: s.journey_streak_enabled,
    };
    let mark = eng.log_index_rev();
    if let Some(kept) = tallies.journey.get(mark, &key) {
        return Ok(kept);
    }
    let rows = eng.log_rows();
    drop(eng);
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("the Journey fold");
    let mut qsos: Vec<propagation::JourneyQso> = Vec::new();
    let fresh = rows
        .each(JOURNEY, Scope::All, Order::Log, &mut |r| {
            qsos.push(journey_qso(r));
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    let grid = (!key.grid.is_empty()).then_some(key.grid.as_str());
    let model = propagation::journey_model(&qsos, &key.call, grid, key.power_w, key.streak);
    Ok(keep(&tallies.journey, mark, key, model, &fresh))
}

/// One logged record as the Journey reads it.
pub(crate) fn journey_qso(r: &QsoRecord) -> propagation::JourneyQso {
    use propagation::model::{Band, ModeClass};
    let program = |name: &str| {
        r.ota
            .their_program
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case(name))
    };
    propagation::JourneyQso {
        call: r.call.clone(),
        grid: r.grid.clone(),
        state: r.state.clone(),
        band: Band::from_label(&r.band),
        mode: ModeClass::from_adif(&r.mode),
        when_unix: r.when_unix as i64,
        // Award-eligible confirmation (LoTW/paper — not eQSL), matching the
        // awards + "first confirmation" semantics.
        confirmed: r.award_confirmed,
        // The Journey "strongest signal" stat is a digital dB SNR concept; parse
        // the numeric report only for DIGITAL QSOs (a phone "59"/CW "599" isn't dB).
        rst_rcvd: if ModeClass::from_adif(&r.mode) == ModeClass::Digital {
            r.rst_rcvd
                .as_deref()
                .and_then(|s| s.trim().parse::<i32>().ok())
        } else {
            None
        },
        pota: program("POTA"),
        sota: program("SOTA"),
        // Hunter ladders count DISTINCT park/summit references.
        pota_ref: if program("POTA") {
            r.ota.their_ref.clone()
        } else {
            None
        },
        sota_ref: if program("SOTA") {
            r.ota.their_ref.clone()
        } else {
            None
        },
    }
}

// ── today's hunted activations ──────────────────────────────────────────────

/// The hunter side of the log as TODAY's activations: which park/summit reference has been
/// worked, from which activator, since 0000Z. The one builder behind both boards that ask —
/// the Needed board's park need and the POTA/SOTA board's "worked today" — so the two cannot
/// disagree about the same activation. `HuntedActivations` owns what counts as an activation
/// (and splits a two-fer); this owns what the log hands it:
///
/// * the activator's BASE call, because propagation has no callsign parser in the default build
///   and the log writes `K1ABC/P` where the spot says `K1ABC`;
/// * only contacts since 0000Z on the station clock (`now - now % 86_400`). An earlier one can
///   never match a question asked now or later, so only today's contacts are read — through the
///   `qso_recent` index, whatever the log's size.
///
/// ⚠️ It reads the store: never under the Engine lock. Take `rows` ([`Engine::log_rows`]) under
/// it.
pub(crate) fn hunted_today(
    rows: &LogRows,
    now: i64,
) -> Result<propagation::HuntedActivations, String> {
    // A clock before 1970 keeps every row; `needed` answers "not hunted" for it regardless.
    let today = u64::try_from(now).map_or(0, |n| n - n % 86_400);
    let mut worked = Vec::new();
    rows.each(HUNTED, Scope::Since(today), Order::Log, &mut |q| {
        // The rule as it always was, on every row the scope names.
        if q.when_unix >= today {
            if let Some(r) = q.ota.their_ref.as_deref().map(str::trim) {
                if !r.is_empty() {
                    worked.push((
                        r.to_string(),
                        tempo_core::message::base_call(&q.call),
                        q.when_unix,
                    ));
                }
            }
        }
        ControlFlow::Continue(())
    })
    .map_err(unreadable)?;
    Ok(propagation::HuntedActivations::from_log(worked))
}

// ── a station's last logged grid ────────────────────────────────────────────

/// The square `peer` last gave: the newest logged contact with that callsign (exact, case
/// aside) that carries a grid a VUCC square can be read from — an operator moves, and the last
/// square they gave is the current one. Of two contacts in the same second the later in the log
/// wins, as it always has. Only that callsign's contacts are read, through the `qso_callhist`
/// index: the store's normalised call holds every contact the exact test below can accept.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn newest_logged_grid(rows: &LogRows, peer: &str) -> Result<Option<String>, String> {
    let mut best: Option<(u64, String)> = None;
    rows.each(
        GRIDS,
        Scope::CallNorm(&call_norm_of(peer)),
        Order::Log,
        &mut |q| {
            if q.call.eq_ignore_ascii_case(peer) {
                if let Some(g) = q
                    .grid
                    .as_deref()
                    .filter(|g| propagation::geo::is_logged_grid(g))
                {
                    if best.as_ref().is_none_or(|(w, _)| q.when_unix >= *w) {
                        best = Some((q.when_unix, g.trim().to_uppercase()));
                    }
                }
            }
            ControlFlow::Continue(())
        },
    )
    .map_err(unreadable)?;
    Ok(best.map(|(_, g)| g))
}

// ── the upload health ───────────────────────────────────────────────────────

/// Last real outcome per connector, off the persisted per-QSO upload stamps — see
/// [`tempo_core::logbook::Logbook::upload_health`], whose step this runs over the store's
/// stamps. The Settings panel polls it every 5 s, so it is kept against the log's `revision`:
/// every change, stamps and appends included, since both can bring a stamp (see the module
/// header).
pub(crate) fn upload_health_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<tempo_core::logbook::UploadHealth>, String> {
    let (mark, rows) = {
        let eng = engine_lock(engine);
        let mark = eng.log_revision();
        if let Some(kept) = tallies.upload_health.get(mark, &()) {
            return Ok(kept);
        }
        (mark, eng.log_rows())
    };
    let mut health = tempo_core::logbook::UploadHealth::default();
    let fresh = rows
        .each(STAMPS, Scope::All, Order::Log, &mut |q| {
            health.add(q);
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    Ok(keep(&tallies.upload_health, mark, (), health, &fresh))
}

// ── the oldest LoTW upload awaiting its echo ────────────────────────────────

/// UTC date (`YYYY-MM-DD`) of the oldest QSO with an in-flight (Pending) LoTW upload — the
/// lower bound for the own-QSO pull, as
/// [`tempo_core::logbook::Logbook::oldest_pending_lotw_date`] computes it. `None` → nothing in
/// flight, so the sync skips the own-echo step.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn oldest_pending_lotw_date(rows: &LogRows) -> Result<Option<String>, String> {
    let mut oldest: Option<u64> = None;
    rows.each(PENDING, Scope::All, Order::Log, &mut |q| {
        if let Some(when) = tempo_core::logbook::lotw_pending_since(q) {
            oldest = Some(oldest.map_or(when, |o| o.min(when)));
        }
        ControlFlow::Continue(())
    })
    .map_err(unreadable)?;
    Ok(oldest.map(tempo_core::logbook::lotw_pull_date))
}

// ── the compound calls the launch seeds the decoder with ────────────────────

/// The log's newest compound calls — distinct, newest first, at most fifty — that
/// [`Engine::seed_hash_table`] encodes into the decoder's hash table. Each encode is one FFI
/// round trip, so the work is capped, and the read stops at the fiftieth.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn compound_calls_to_seed(rows: &LogRows) -> Result<Vec<String>, String> {
    use tempo_core::message::is_compound;
    let mut seen = std::collections::HashSet::new();
    let mut calls = Vec::new();
    rows.each(CALLS, Scope::All, Order::NewestFirst, &mut |rec| {
        let call = rec.call.trim().to_uppercase();
        if is_compound(&call) && seen.insert(call.clone()) {
            calls.push(call);
        }
        if calls.len() >= 50 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .map_err(unreadable)?;
    Ok(calls)
}

#[cfg(test)]
mod tests {
    //! Every fold, read from the store, held to what the code before C14 made of the log in
    //! memory — reproduced below VERBATIM as the oracles — at 150,000 contacts and over seeded
    //! random runs of the changes the app makes, fills included; each asked straight after a
    //! contact is logged (P4) with the Engine lock free while it waits; and the kept answers
    //! reused across an upload stamp exactly where that is correct.
    use super::*;
    use crate::remote_service::stored_log_tests::{caught_up, StoredLog};
    use propagation::model::{Band, ModeClass};
    use propagation::OperatorNeeds;
    use tempo_core::logbook::sqlite::{Resolved, WriteHold};
    use tempo_core::logbook::{UploadDetail, UploadOutcome};

    const MY_CALL: &str = "KD9TAW";
    /// The instant every fold is asked at: 2026-08-29 04:00Z.
    const NOW: i64 = 1_788_000_000;

    /// A folder of the test's own, gone with the value.
    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let p = std::env::temp_dir().join(format!(
                "nexus-folds-{tag}-{}-{}-{nanos}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn log(&self) -> std::path::PathBuf {
            self.0.join("log.adi")
        }
        fn db(&self) -> std::path::PathBuf {
            self.0.join("log.sqlite3")
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A deterministic generator: a failing case is reproducible from its seed alone.
    struct Gen(u64);
    impl Gen {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
        fn chance(&mut self, one_in: u64) -> bool {
            self.next().is_multiple_of(one_in)
        }
        fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
            xs[self.below(xs.len())]
        }
    }

    /// The resolvers the engine fills an insert with — and the fill job, the same functions.
    fn test_country(call: &str) -> Option<String> {
        propagation::dxcc::resolve(call).map(|i| i.entity.to_string())
    }
    fn test_state(call: &str, _grid: Option<&str>) -> Option<String> {
        call.starts_with('K').then(|| "WI".to_string())
    }

    /// Callsign stems across many entities — the US and its states, Alaska and Hawaii, Canada,
    /// Europe, Asia, Oceania, the Americas — a portable, a compound, one no table resolves, and
    /// spellings that differ from another only by case and whitespace.
    const STEMS: &[&str] = &[
        "K", "W", "N", "KL7", "KH6", "VE3", "VE7", "DL", "G", "F", "JA", "VK", "ZL", "PY", "LU",
        "UA", "EA", "I", "ON", "OH", "SM", "9A", "4X", "ZS", "5B", "KP4", "3D2", "Q",
    ];

    /// One synthetic ADIF record, with what every fold reads: bands (and none), modes (and a
    /// promoted submode), grids of both widths, US states, all four confirmation channels,
    /// DXCC credit, IOTA, POTA/SOTA hunts — many of them TODAY, which is what the hunted parks
    /// read — satellite contacts, digital reports, the four connectors' stamps with their
    /// details, and the Accepted stamp an imported `LOTW_QSL_SENT=Y` makes.
    fn synthetic(i: usize, g: &mut Gen) -> String {
        let mut f = String::new();
        let mut tag = |name: &str, val: &str| {
            f.push_str(&format!("<{}:{}>{}", name, val.len(), val));
        };
        let stem = g.pick(STEMS);
        let call = match g.below(40) {
            0 => format!("{stem}{}ABC/P", g.below(10)),
            1 => format!("VP2E/{stem}{}AA", g.below(10)),
            2 => format!("{}{}zz", stem.to_ascii_lowercase(), g.below(10)),
            _ => format!(
                "{stem}{}{}",
                g.below(10),
                ["AB", "XYZ", "AA", "QQ", "ABC"][g.below(5)]
            ),
        };
        tag("CALL", &call);
        let today = g.chance(25);
        let when = if today {
            NOW as u64 - NOW as u64 % 86_400 + g.below(14_000) as u64
        } else {
            1_420_000_000 + g.below(360_000_000) as u64
        };
        let (y, m, d, hh, mm, ss) = tempo_core::logbook::datetime_utc(when);
        tag("QSO_DATE", &format!("{y:04}{m:02}{d:02}"));
        if !g.chance(9) {
            tag("TIME_ON", &format!("{hh:02}{mm:02}{ss:02}"));
        }
        let sat = g.chance(60);
        if sat {
            tag("BAND", "2m");
            tag("PROP_MODE", "SAT");
            tag("SAT_NAME", "RS-44");
        } else if !g.chance(15) {
            tag(
                "BAND",
                g.pick(&[
                    "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m",
                ]),
            );
        }
        let mode = g.pick(&["FT8", "FT8", "SSB", "CW", "RTTY", "MFSK", "USB"]);
        tag("MODE", mode);
        if mode == "MFSK" {
            tag("SUBMODE", "FT4");
        }
        if !g.chance(3) {
            let grid = format!(
                "{}{}{}{}",
                (b'A' + g.below(18) as u8) as char,
                (b'A' + g.below(18) as u8) as char,
                g.below(10),
                g.below(10)
            );
            if g.chance(3) {
                tag("GRIDSQUARE", &format!("{grid}ab"));
            } else {
                tag("GRIDSQUARE", &grid);
            }
        }
        if call.starts_with(['K', 'W', 'N']) && g.chance(2) {
            tag("STATE", g.pick(&["WI", "IL", "CA", "TX", "MA", "NY", "OH"]));
        }
        tag(
            "RST_RCVD",
            if matches!(mode, "FT8" | "MFSK" | "RTTY") {
                g.pick(&["-12", "+03", "-20", "0"])
            } else {
                "59"
            },
        );
        for (name, one_in, val) in [
            ("LOTW_QSL_RCVD", 5, "Y"),
            ("QSL_RCVD", 20, "Y"),
            ("EQSL_QSL_RCVD", 10, "Y"),
            ("APP_QRZLOG_STATUS", 15, "C"),
            ("CREDIT_GRANTED", 20, "DXCC"),
            ("IOTA", 50, "NA-001"),
            ("LOTW_QSL_SENT", 20, "Y"),
        ] {
            if g.chance(one_in) {
                tag(name, val);
            }
        }
        if g.chance(if today { 2 } else { 20 }) {
            tag("SIG", "POTA");
            let park = format!("US-{:04}", g.below(40));
            if g.chance(8) {
                tag("SIG_INFO", &format!("{park},US-{:04}", g.below(40)));
            } else {
                tag("SIG_INFO", &park);
            }
        } else if g.chance(60) {
            tag("SOTA_REF", "W7A/MN-001");
        }
        for (name, outcomes) in [
            (
                "APP_TEMPO_UL_LOTW",
                &["pending", "accepted", "rejected"][..],
            ),
            ("APP_TEMPO_UL_QRZ", &["accepted", "authfail", "duplicate"]),
            ("APP_TEMPO_UL_CLUBLOG", &["accepted", "rejected"]),
            ("APP_TEMPO_UL_EQSL", &["accepted", "pending"]),
        ] {
            if g.chance(6) {
                let outcome = outcomes[g.below(outcomes.len())];
                let at = 1_600_000_000 + g.below(100_000_000);
                let detail = if outcome == "rejected" { "record" } else { "" };
                tag(name, &format!("{outcome}|{at}|{detail}"));
            }
        }
        let _ = i;
        f.push_str("<EOR>\n");
        f
    }

    fn synthetic_log(n: usize, seed: u64) -> String {
        let mut g = Gen(seed | 1);
        let mut s = tempo_core::logbook::adif_header();
        for i in 0..n {
            s.push_str(&synthetic(i, &mut g));
        }
        s
    }

    /// A launch on the store at `d` — the shipped open path, which converts a `log.adi` it
    /// finds there — with the resolvers set before the log is adopted, as the shell's are.
    fn launch(d: &Dir) -> crate::SharedEngine {
        let opened = tempo_app::logstore::open_with(
            &d.log(),
            Arc::new(|_| Resolved::default()),
            None,
            tempo_core::logbook::mirror::MirrorOptions {
                debounce: std::time::Duration::from_millis(5),
                max_delay: std::time::Duration::from_millis(50),
                accepted: None,
            },
        )
        .expect("the store opens");
        let mut e = Engine::new(MY_CALL, "EN52", 0);
        e.set_dxcc_resolver(test_country);
        e.set_state_resolver(test_state);
        e.attach_log_store(opened);
        assert!(e.log_store_open(), "premise: the store owns the log");
        Arc::new(Mutex::new(e))
    }

    fn settle(e: &crate::SharedEngine) {
        e.lock()
            .unwrap()
            .flush_log_store(std::time::Duration::from_secs(120))
            .expect("written");
    }

    /// The log the oracles below fold: the store's rows ([`StoredLog`]). These tests hold the old
    /// fold against the new one over the same rows; whether the store holds what the old write
    /// path wrote (P6) is the Stage-1 lockstep suite's job (tempo-app's `stage1_tests`).
    fn records(e: &crate::SharedEngine) -> Vec<Arc<QsoRecord>> {
        e.lock().unwrap().stored_log()
    }

    // ── the oracles: the code before C14, verbatim ──────────────────────────

    /// `needs_finish`'s fold as C12 left it, over the log in memory.
    fn needs_old(records: &[Arc<QsoRecord>]) -> propagation::LogNeeds {
        let mut needs = propagation::LogNeeds::new();
        for q in records {
            // A "needs confirmation" must be award-grade (LoTW/paper), not eQSL.
            needs.add_qso(
                &q.call,
                &q.band,
                &q.mode,
                q.grid.as_deref(),
                q.state.as_deref(),
                q.award_confirmed,
                qso_is_sat(q.prop_mode.as_deref()),
            );
        }
        needs
    }

    /// `awards_for_records` as it was.
    fn awards_old<R: std::borrow::Borrow<tempo_core::logbook::QsoRecord>>(
        records: &[R],
        my_call: &str,
    ) -> propagation::AwardSummary {
        let mut awards = propagation::Awards::new();
        // Tell the accumulator our own entity so "First DX" counts only foreign ones.
        awards.set_home_call(my_call);
        for q in records {
            let q: &tempo_core::logbook::QsoRecord = std::borrow::Borrow::borrow(q);
            // Award-eligible confirmation only (LoTW/paper) — eQSL doesn't count; plus
            // whether ARRL has granted DXCC-family credit (DXCC / DXCC_BAND /
            // DXCC_MODE / … — real LoTW exports use the granular codes).
            let credited = q.credit_granted.iter().any(|c| c.starts_with("DXCC"));
            awards.add_qso(
                &q.call,
                &q.band,
                &q.mode,
                q.award_confirmed,
                credited,
                // The paper-card channel alone — IOTA's award gate (the IOTA program
                // accepts cards and Club Log matching, never LoTW).
                q.qsl_rcvd.card,
                q.state.as_deref(),
                q.grid.as_deref(),
                q.ota.iota.as_deref(),
                qso_is_sat(q.prop_mode.as_deref()),
            );
        }
        awards.summary()
    }

    /// `hunted_activations` as it was.
    fn hunted_old<R: std::borrow::Borrow<tempo_core::logbook::QsoRecord>>(
        records: &[R],
        now: i64,
    ) -> propagation::HuntedActivations {
        // A clock before 1970 keeps every row; `needed` answers "not hunted" for it regardless.
        let today = u64::try_from(now).map_or(0, |n| n - n % 86_400);
        propagation::HuntedActivations::from_log(records.iter().filter_map(|q| {
            let q: &tempo_core::logbook::QsoRecord = std::borrow::Borrow::borrow(q);
            if q.when_unix < today {
                return None;
            }
            let r = q.ota.their_ref.as_deref()?.trim();
            (!r.is_empty()).then(|| {
                (
                    r.to_string(),
                    tempo_core::message::base_call(&q.call),
                    q.when_unix,
                )
            })
        }))
    }

    /// `newest_logged_grid` as it was.
    fn grid_old(records: &[Arc<tempo_core::logbook::QsoRecord>], peer: &str) -> Option<String> {
        let mut best: Option<(u64, String)> = None;
        for q in records {
            if q.call.eq_ignore_ascii_case(peer) {
                if let Some(g) = q
                    .grid
                    .as_deref()
                    .filter(|g| propagation::geo::is_logged_grid(g))
                {
                    if best.as_ref().is_none_or(|(w, _)| q.when_unix >= *w) {
                        best = Some((q.when_unix, g.trim().to_uppercase()));
                    }
                }
            }
        }
        best.map(|(_, g)| g)
    }

    /// `Logbook::upload_health` as it was.
    fn health_old(records: &[Arc<QsoRecord>]) -> tempo_core::logbook::UploadHealth {
        let mut h = tempo_core::logbook::UploadHealth::default();
        for r in records {
            for (src, status) in [
                (&mut h.lotw, &r.upload.lotw),
                (&mut h.eqsl, &r.upload.eqsl),
                (&mut h.qrz, &r.upload.qrz),
                (&mut h.clublog, &r.upload.clublog),
            ] {
                let Some(s) = status.as_ref().filter(|s| s.when_unix > 0) else {
                    continue;
                };
                if s.outcome.is_sent() {
                    if src.last_success_unix.is_none_or(|w| s.when_unix > w) {
                        src.last_success_unix = Some(s.when_unix);
                    }
                } else if src.last_failure_unix.is_none_or(|w| s.when_unix > w) {
                    src.last_failure_unix = Some(s.when_unix);
                    src.last_failure_detail = s.detail;
                }
            }
        }
        h
    }

    /// `Logbook::oldest_pending_lotw_date` as it was.
    fn pending_old(records: &[Arc<QsoRecord>]) -> Option<String> {
        records
            .iter()
            .filter(|r| {
                matches!(
                    r.upload.lotw.as_ref().map(|s| s.outcome),
                    Some(UploadOutcome::Pending)
                )
            })
            .map(|r| r.when_unix)
            .min()
            .map(|unix| {
                let (y, m, d, ..) = tempo_core::logbook::datetime_utc(unix);
                format!("{y:04}-{m:02}-{d:02}")
            })
    }

    /// `Engine::seed_hash_table`'s walk as it was — the calls it encoded, in order.
    fn seed_old(records: &[Arc<QsoRecord>]) -> Vec<String> {
        use tempo_core::message::is_compound;
        let mut seen = std::collections::HashSet::new();
        let mut encoded = Vec::new();
        for rec in records.iter().rev() {
            let call = rec.call.trim().to_uppercase();
            if !is_compound(&call) || !seen.insert(call.clone()) {
                continue;
            }
            encoded.push(call);
            if seen.len() >= 50 {
                break;
            }
        }
        encoded
    }

    /// `journey_kept`'s fold as it was: every record through `journey_qso` (moved to this module
    /// unchanged), then `journey_model`.
    fn journey_old(records: &[Arc<QsoRecord>], key: &JourneyKey) -> propagation::JourneyModel {
        let qsos: Vec<propagation::JourneyQso> = records.iter().map(|r| journey_qso(r)).collect();
        let grid = (!key.grid.is_empty()).then_some(key.grid.as_str());
        propagation::journey_model(&qsos, &key.call, grid, key.power_w, key.streak)
    }

    /// Everything a reader can observe of a needs model, as one comparable value (C12's own
    /// measure): every set it exposes, sorted, and its verdict for every worked entity (and two
    /// it never worked) on every band in every mode class.
    fn observed(n: &propagation::LogNeeds) -> String {
        fn sorted<T: std::fmt::Debug>(items: impl Iterator<Item = T>) -> String {
            let mut v: Vec<String> = items.map(|x| format!("{x:?}")).collect();
            v.sort();
            v.join(",")
        }
        let mut out = vec![
            sorted(n.worked_entity_names().iter()),
            sorted(n.worked_grids_sat().iter()),
            sorted(n.confirmed_grids_sat().iter()),
            sorted(n.worked_zones().iter()),
            sorted(n.worked_grids().iter()),
            sorted(n.worked_states().iter()),
            sorted(n.confirmed_zones().iter()),
            sorted(n.confirmed_grids().iter()),
            sorted(n.confirmed_states().iter()),
        ];
        let mut entities: Vec<String> = n.worked_entity_names().iter().cloned().collect();
        entities.extend(["Japan".to_string(), "Nowhere".to_string()]);
        entities.sort();
        for e in &entities {
            for b in Band::ALL {
                for m in [ModeClass::Cw, ModeClass::Phone, ModeClass::Digital] {
                    out.push(format!("{e}/{b:?}/{m:?}={:?}", n.need(e, b, m)));
                }
            }
        }
        out.join("\n")
    }

    /// The diagnosis of records in hand, as the command made it before the store: the log in
    /// memory, a DXCC lookup per contact, no reconcile summaries.
    fn diagnosis_old(records: &[Arc<QsoRecord>]) -> String {
        let entities: Vec<Option<String>> = records.iter().map(|r| test_country(&r.call)).collect();
        let report = tempo_core::diagnostics::diagnose(
            records,
            &entities,
            &[],
            NOW,
            &tempo_core::diagnostics::DiagCfg::default(),
        );
        format!("{report:?}")
    }

    // ── one answer per fold, from the store and from the oracle ─────────────

    /// One fold's two answers: whether they are equal (by the type's own equality where it has
    /// one, never by a text that could order a set differently), and each as text for a report.
    struct Answer {
        fold: &'static str,
        equal: bool,
        store: String,
        memory: String,
    }

    fn answer<T: PartialEq + std::fmt::Debug>(fold: &'static str, store: T, memory: T) -> Answer {
        Answer {
            fold,
            equal: store == memory,
            store: format!("{store:?}"),
            memory: format!("{memory:?}"),
        }
    }

    /// Every fold's answer read from the store, beside the oracle's answer over the log in
    /// memory — so a comparison names the fold that differs.
    fn every_fold(e: &crate::SharedEngine, tallies: &LogTallies) -> Vec<Answer> {
        let held = records(e);
        let rows = e.lock().unwrap().log_rows();
        assert!(
            matches!(rows, LogRows::Store(_)),
            "premise: the store's rows"
        );
        let key = {
            let eng = e.lock().unwrap();
            let s = eng.settings();
            JourneyKey {
                call: s.mycall.clone(),
                grid: s.mygrid.clone(),
                power_w: s.station_power_w,
                streak: s.journey_streak_enabled,
            }
        };
        let journey_json = |m: &propagation::JourneyModel| {
            serde_json::to_string(&m.summary(NOW)).expect("serialises")
        };
        let mut peers: Vec<String> = held.iter().map(|r| r.call.clone()).take(12).collect();
        peers.push("N0NE".into());
        let grids = |look: &dyn Fn(&str) -> Option<String>| {
            peers
                .iter()
                .map(|p| (p.clone(), look(p)))
                .collect::<Vec<_>>()
        };
        let diagnosis = {
            let inputs = e.lock().unwrap().confirmation_diagnostics_inputs();
            let (report, n) = inputs.diagnose(NOW, test_country).expect("reads");
            assert_eq!(n, held.len());
            format!("{report:?}")
        };
        vec![
            answer(
                "needs",
                observed(&needs_kept(e, tallies).expect("reads").0),
                observed(&needs_old(&held)),
            ),
            answer(
                "awards",
                awards_kept(e, tallies).expect("reads").as_ref().clone(),
                awards_old(&held, MY_CALL),
            ),
            answer(
                "statistics",
                log_stats(e, tallies).expect("reads").as_ref().clone(),
                propagation::compute_log_stats(
                    &held.iter().map(|q| q.call.as_str()).collect::<Vec<_>>(),
                    MY_CALL,
                ),
            ),
            answer(
                "journey",
                journey_json(&journey_kept(e, tallies).expect("reads")),
                journey_json(&journey_old(&held, &key)),
            ),
            answer(
                "hunted",
                hunted_today(&rows, NOW).expect("reads"),
                hunted_old(&held, NOW),
            ),
            answer(
                "grid",
                grids(&|p| newest_logged_grid(&rows, p).expect("reads")),
                grids(&|p| grid_old(&held, p)),
            ),
            answer(
                "upload health",
                upload_health_kept(e, tallies)
                    .expect("reads")
                    .as_ref()
                    .clone(),
                health_old(&held),
            ),
            answer(
                "oldest pending",
                oldest_pending_lotw_date(&rows).expect("reads"),
                pending_old(&held),
            ),
            answer(
                "seed calls",
                compound_calls_to_seed(&rows).expect("reads"),
                seed_old(&held),
            ),
            answer("diagnosis", diagnosis, diagnosis_old(&held)),
        ]
    }

    fn assert_every_fold_agrees(e: &crate::SharedEngine, tallies: &LogTallies, what: &str) {
        for a in every_fold(e, tallies) {
            assert!(
                a.equal,
                "{what}: the {} read from the store differs from the log in memory\n\
                 store:  {:.600}\nmemory: {:.600}",
                a.fold, a.store, a.memory
            );
        }
    }

    /// ★ GOLDEN AT 150,000: every fold read from the store is the fold the code before C14 made
    /// of the log in memory — the needs model through everything it exposes, the award summary,
    /// the statistics, the Journey, today's hunted parks, thirteen stations' last grids, the
    /// upload health, the oldest pending LoTW upload, the compound calls the launch seeds, and
    /// the diagnosis — over 150,000 synthetic contacts converted by the shipped open path.
    #[test]
    fn at_150k_every_fold_read_from_the_store_is_the_fold_of_the_log_in_memory() {
        let d = Dir::new("golden");
        std::fs::write(d.log(), synthetic_log(150_000, 0x0C14_0150_0000)).unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let held = records(&e);
        assert_eq!(held.len(), 150_000, "premise: every contact converted");
        let answers = every_fold(&e, &tallies);
        // Premises, so no fold agreed on emptiness.
        let need_text = &answers[0].store;
        assert!(need_text.matches("Satisfied").count() > 10 && need_text.contains("Atno"));
        let awards = awards_kept(&e, &tallies).unwrap();
        assert!(
            awards.dxcc_worked > 20 && awards.dxcc_confirmed > 5,
            "{awards:?}"
        );
        assert_ne!(
            hunted_old(&held, NOW),
            propagation::HuntedActivations::default(),
            "premise: parks were hunted today"
        );
        let health = health_old(&held);
        assert!(health.lotw.last_success_unix.is_some() && health.qrz.last_failure_unix.is_some());
        assert!(
            pending_old(&held).is_some(),
            "premise: a LoTW upload is pending"
        );
        assert!(
            !seed_old(&held).is_empty(),
            "premise: compound calls are logged"
        );
        for a in answers {
            assert!(
                a.equal,
                "the {} at 150k differs\nstore:  {:.600}\nmemory: {:.600}",
                a.fold, a.store, a.memory
            );
        }
        settle(&e);
    }

    /// One random change of the kinds the app makes to the log — see the tempo-app store tests'
    /// twin — including the fill job.
    /// The id of the contact at `at`: how a change names its contact (SPEC-2 C16).
    fn id_at(e: &Engine, at: usize) -> tempo_core::logbook::RecordId {
        e.stored_log()[at]
            .id
            .expect("every row the log holds carries an id")
    }

    fn random_change(e: &crate::SharedEngine, g: &mut Gen, step: u64) {
        let stem = g.pick(STEMS);
        let call = format!("{stem}{}{}", g.below(4), ["AB", "XYZ"][g.below(2)]);
        let when = NOW as u64 - 3_600 + step * 37;
        let len = e.lock().unwrap().stored_log().len();
        let at = g.below(len);
        match g.below(11) {
            0 | 1 => {
                let mut rec = parse_one(&synthetic(0, g));
                rec.id = None;
                rec.call = call;
                rec.when_unix = when;
                rec.state = None;
                rec.country = None;
                e.lock().unwrap().log_qso(rec);
            }
            2 => {
                let _ = e.lock().unwrap().import_adif(&synthetic(0, g));
            }
            3 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let mut r = QsoRecord::clone(&eng.stored_log()[at]);
                match g.below(3) {
                    0 => r.band = "40m".into(),
                    1 => r.call = call,
                    _ => r.grid = Some("FN42".into()),
                }
                let id = id_at(&eng, at);
                eng.update_qso(id, r);
            }
            4 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.delete_qso(id);
            }
            5 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.mark_qsl_card(id, g.chance(2));
            }
            6 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.mark_qsl_sent(id, Some(tempo_core::logbook::QslVia::Direct));
            }
            7 if len > 0 => {
                let sat = g.chance(2).then_some("RS-44");
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.set_sat_tag(id, sat);
            }
            8 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let pushed = QsoRecord::clone(&eng.stored_log()[at]);
                let (outcome, detail) = if g.chance(2) {
                    (UploadOutcome::Accepted, None)
                } else {
                    (UploadOutcome::Rejected, Some(UploadDetail::RecordRefused))
                };
                eng.stamp_qrz_upload(&pushed, outcome, when as i64, detail);
                eng.stamp_clublog_upload(&pushed, outcome, when as i64 + 1, detail);
                eng.stamp_eqsl_upload(&pushed, outcome, when as i64 + 2, detail);
            }
            9 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let r = QsoRecord::clone(&eng.stored_log()[at]);
                let text = tempo_core::logbook::adif_record(&r)
                    .replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC<EOR>");
                let _ = eng.merge_lotw_report(&text);
            }
            _ => {
                let _ = tempo_app::logfill::fill_log_store(
                    e,
                    (step % 4) as i64,
                    &test_country,
                    &test_state,
                );
            }
        }
        // Back once the store holds the change, so the folds asked next are asked about it rather
        // than about the disk's speed ([`caught_up`]); P4 itself is the test below that holds the
        // writer.
        caught_up(e);
    }

    /// ★ THE PROPERTY: over 16 seeded runs of 30 random changes each — logged contacts, imports,
    /// edits (a call corrected among them), deletes, QSL cards, QSL-sent marks, satellite tags,
    /// the three connectors' stamps, LoTW confirmations with credit, and the fill job — after
    /// EVERY change, every fold read from the store is the fold of the log in memory, the kept
    /// answers included. Seeded, so a failure names its case.
    #[test]
    fn after_every_change_every_fold_read_from_the_store_is_the_fold_of_the_log_in_memory() {
        for seed in 1..=16u64 {
            let d = Dir::new(&format!("prop-{seed}"));
            std::fs::write(d.log(), synthetic_log(40, seed * 7_919)).unwrap();
            let e = launch(&d);
            let tallies = LogTallies::default();
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            assert_every_fold_agrees(&e, &tallies, &format!("seed {seed}, at launch"));
            for step in 0..30 {
                random_change(&e, &mut g, step);
                assert_every_fold_agrees(&e, &tallies, &format!("seed {seed}, step {step}"));
            }
            settle(&e);
        }
    }

    /// Ask `fold` straight after `contact` is logged — with the store's write lock held
    /// elsewhere, so the contact is not on disk yet — and check from another thread, while the
    /// fold waits, that the Engine lock is free; then let the write through. What the fold
    /// answered, and whether it had to wait.
    fn asked_straight_after<T: Send>(
        d: &Dir,
        e: &crate::SharedEngine,
        contact: QsoRecord,
        fold: impl FnOnce() -> T + Send,
    ) -> T {
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(contact);
        std::thread::scope(|s| {
            let asked = s.spawn(fold);
            std::thread::sleep(std::time::Duration::from_millis(250));
            assert!(
                !asked.is_finished(),
                "the fold waits for the contact's write"
            );
            for _ in 0..5 {
                assert!(
                    e.try_lock().is_ok(),
                    "the Engine lock is free while the fold waits"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            drop(hold);
            asked.join().unwrap()
        })
    }

    /// A contact to log in the P4 checks: FT8 on 20 m, now, with a grid.
    fn contact(call: &str, when: u64) -> QsoRecord {
        let mut r = parse_one(&format!(
            "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260829<TIME_ON:6>030000\
             <GRIDSQUARE:4>RH91<EOR>",
            call.len()
        ));
        r.when_unix = when;
        r
    }

    /// One ADIF record, parsed as the log parses it, with no id.
    fn parse_one(text: &str) -> QsoRecord {
        let mut log = tempo_core::logbook::Logbook::new();
        log.import_adif(text);
        let mut r = QsoRecord::clone(&log.records()[0]);
        r.id = None;
        r
    }

    /// ★ P4, PER FOLD: a fold asked for straight after a contact is logged — while the contact
    /// is still on its way to disk — includes it, and waits for it with the Engine lock free
    /// (checked from another thread while it waits). Each fold is asked about a contact of its
    /// own: a new entity (St Helena) for the needs, awards, statistics and Journey; a park hunted today;
    /// a grid; a stamp; a pending LoTW upload; a compound call; one more contact to diagnose.
    #[test]
    fn a_fold_asked_straight_after_a_logged_contact_includes_it() {
        let d = Dir::new("p4");
        std::fs::write(d.log(), synthetic_log(200, 0xFEED)).unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let helena = test_country("ZD7AA").expect("premise: cty.dat places ZD7");
        let worked = |n: &propagation::LogNeeds| n.worked_entity_names().contains(&helena);
        assert!(
            !worked(&needs_kept(&e, &tallies).unwrap().0),
            "premise: {helena} is new"
        );

        let needs = asked_straight_after(&d, &e, contact("ZD7AA", NOW as u64), || {
            needs_kept(&e, &tallies).unwrap().0
        });
        assert!(worked(&needs), "the needs model has the contact");

        let awards = asked_straight_after(&d, &e, contact("ZD7AB", NOW as u64), || {
            awards_kept(&e, &tallies).unwrap()
        });
        assert_eq!(*awards, awards_old(&records(&e), MY_CALL), "the awards");

        let before = log_stats(&e, &tallies).unwrap().total;
        let stats = asked_straight_after(&d, &e, contact("ZD7AC", NOW as u64), || {
            log_stats(&e, &tallies).unwrap()
        });
        assert_eq!(stats.total, before + 1, "the statistics count it");

        let journey = asked_straight_after(&d, &e, contact("ZD7AD", NOW as u64), || {
            journey_kept(&e, &tallies).unwrap().summary(NOW).total_qsos
        });
        assert_eq!(journey as usize, records(&e).len(), "the Journey counts it");

        let mut park = contact("K9PRK", NOW as u64);
        park.ota.their_program = Some("POTA".into());
        park.ota.their_ref = Some("US-9999".into());
        let hunted = asked_straight_after(&d, &e, park, || {
            let rows = e.lock().unwrap().log_rows();
            hunted_today(&rows, NOW).unwrap()
        });
        assert!(
            !hunted.needed("US-9999", "K9PRK", NOW),
            "today's park is hunted"
        );

        let grid = asked_straight_after(&d, &e, contact("K9GRD", NOW as u64), || {
            let rows = e.lock().unwrap().log_rows();
            newest_logged_grid(&rows, "K9GRD").unwrap()
        });
        assert_eq!(grid.as_deref(), Some("RH91"), "its grid is found");

        let mut stamped = contact("K9STP", NOW as u64);
        stamped.upload.qrz = Some(tempo_core::logbook::UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_999_999_999,
            detail: None,
        });
        let health = asked_straight_after(&d, &e, stamped, || {
            upload_health_kept(&e, &tallies).unwrap()
        });
        assert_eq!(
            health.qrz.last_success_unix,
            Some(1_999_999_999),
            "its stamp counts"
        );

        let mut pending = contact("K9PND", 1_000_000_000);
        pending.upload.lotw = Some(tempo_core::logbook::UploadStatus {
            outcome: UploadOutcome::Pending,
            when_unix: 1_000_000_100,
            detail: None,
        });
        let oldest = asked_straight_after(&d, &e, pending, || {
            let rows = e.lock().unwrap().log_rows();
            oldest_pending_lotw_date(&rows).unwrap()
        });
        assert_eq!(
            oldest.as_deref(),
            Some("2001-09-09"),
            "its pending upload is the oldest"
        );

        let seeded = asked_straight_after(&d, &e, contact("K9SED/P", NOW as u64), || {
            let rows = e.lock().unwrap().log_rows();
            compound_calls_to_seed(&rows).unwrap()
        });
        assert_eq!(
            seeded.first().map(String::as_str),
            Some("K9SED/P"),
            "newest first"
        );

        let diagnosed = asked_straight_after(&d, &e, contact("K9DGN", NOW as u64), || {
            let inputs = e.lock().unwrap().confirmation_diagnostics_inputs();
            inputs.diagnose(NOW, test_country).unwrap().1
        });
        assert_eq!(
            diagnosed,
            records(&e).len(),
            "every contact diagnosed, the new one too"
        );
        settle(&e);
    }

    /// ★ THE KEPT ANSWERS, ACROSS AN UPLOAD STAMP. The needs model, the awards, the statistics
    /// and the Journey read nothing a stamp changes: after one, each is reused — no fold — and
    /// still the fresh answer. The upload health reads the stamps: after one it folds again, and
    /// the new answer has the stamp. The control is an edit, which moves all of them.
    #[test]
    #[cfg(debug_assertions)]
    fn the_kept_answers_survive_an_upload_stamp_exactly_where_that_is_correct() {
        let d = Dir::new("stamp-reuse");
        std::fs::write(d.log(), synthetic_log(120, 0x57A4)).unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let folds = || {
            crate::LOG_TALLIES.with(|c| c.set(0));
            let _ = needs_kept(&e, &tallies).unwrap();
            let _ = awards_kept(&e, &tallies).unwrap();
            let _ = log_stats(&e, &tallies).unwrap();
            let _ = journey_kept(&e, &tallies).unwrap();
            crate::LOG_TALLIES.with(|c| c.get())
        };
        let health_folds = || {
            crate::LOG_TALLIES.with(|c| c.set(0));
            let h = upload_health_kept(&e, &tallies).unwrap();
            (crate::LOG_TALLIES.with(|c| c.get()), h)
        };
        assert_eq!(folds(), 4, "each folds once");
        assert_eq!(health_folds().0, 1);
        assert_eq!(folds(), 0, "and not again while nothing moves");
        assert_eq!(health_folds().0, 0);

        {
            let mut eng = e.lock().unwrap();
            let pushed = QsoRecord::clone(&eng.stored_log()[3]);
            assert!(eng.stamp_qrz_upload(&pushed, UploadOutcome::Accepted, 2_000_000_000, None));
        }
        // Asked once the store holds the stamp: a fold that reads it answers from the store as it
        // stands if the writer is past READ_WAIT, and then keeps nothing ([`caught_up`]).
        caught_up(&e);
        assert_eq!(
            folds(),
            0,
            "a stamp: the needs, awards, statistics and Journey are reused"
        );
        let (refolded, health) = health_folds();
        assert_eq!(
            refolded, 1,
            "the upload health reads the stamps, so it folds again"
        );
        assert_eq!(
            health.qrz.last_success_unix,
            Some(2_000_000_000),
            "and has the stamp"
        );
        assert_every_fold_agrees(&e, &tallies, "after the stamp: every kept answer is fresh");

        // The control: an edit moves every kept answer.
        {
            let mut eng = e.lock().unwrap();
            let mut edited = QsoRecord::clone(&eng.stored_log()[5]);
            edited.band = "6m".into();
            let id = id_at(&eng, 5);
            assert!(eng.update_qso(id, edited));
        }
        caught_up(&e);
        assert_eq!(folds(), 4, "control: an edit folds each again");
        assert_every_fold_agrees(&e, &tallies, "after the edit");
        settle(&e);
    }

    /// ★ POSITIVE CONTROL for the fence the tests above pass through: a pass over the log's rows
    /// made while this thread holds an Engine guard is a panic in a debug build.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "io_fence: a pass over the log's rows is a pass over the whole log")]
    fn a_pass_over_the_rows_under_the_engine_lock_is_refused() {
        let d = Dir::new("fence");
        std::fs::write(d.log(), synthetic_log(5, 0x5)).unwrap();
        let e = launch(&d);
        let eng = engine_lock(&e);
        let rows = eng.log_rows();
        let _ = hunted_today(&rows, NOW);
    }
}
