//! The fill job — SPEC-2 v3's **D2-A**, the operator's choice for the country and state a
//! contact's call places it in: *save them*.
//!
//! Until Stage 2 the launch filled both into the log IN MEMORY and wrote nothing ("a launch
//! writes nothing", C9), so the store held what was written and the screens showed what was
//! filled — two answers, and every reader of the store would have had to fill as it read. Now
//! the store holds exactly what every screen shows:
//!
//! - every insert fills both before it writes (`StationCore::fill_with`; `Engine::log_qso`
//!   always did);
//! - what an older build left unfilled is filled ONCE, after the window is up, by this job —
//!   and again only when the resolvers' data changes (a new cty.dat or FCC file, a new build),
//!   which is what `fill_ver` in the store's `log_meta` records.
//!
//! # The job
//!
//! 1. Under the Engine lock: the store's read handles, and nothing else.
//! 2. Off the lock, in ONE read transaction: `fill_ver`. The version it was asked to fill for →
//!    done, and the launch has written nothing. Otherwise every contact lacking a country, or a
//!    state where one can apply — a narrow read of a few columns — resolved contact by contact,
//!    still off the lock: a country by the contact's DXCC number where it carries one, else by its
//!    call, as an insert fills it (`station::country_from`).
//! 3. [`crate::logwrite::fill`] writes what was found, a chunk at a time: each chunk's rows read
//!    whole from the store with the lock released, and made under it only where the field is
//!    still empty in the row as it now stands — a change in the writer's bulk lane (a contact
//!    logged meanwhile overtakes it). `fill_ver` goes with the last chunk, once every earlier
//!    one is on disk — so the version is on disk only once every fill is.
//!
//! It runs on a thread of its own, never on the radio loop, and nothing it does under the lock
//! touches the disk, calls a resolver, or passes over the log. On the 1.13 path it does
//! nothing: that path fills as it always has, at the launch, into `log.adi` — its store is in
//! memory and gone with the session, so a `fill_ver` there would name nothing the next launch
//! could read.
//!
//! The resolvers are the caller's, and must be the functions the engine's own are
//! (`Engine::set_dxcc_resolver` / `set_state_resolver`): the command layer hands both the same
//! functions, which is what makes a filled row the row an insert would have written. The one
//! that names a DXCC number's country is the engine's own, taken with its read handles
//! (`Engine::set_dxcc_country_resolver`), so it cannot be another. Whether a
//! state can apply to a contact at all (a US state or a Canadian province: its call places the
//! station there, or nowhere) is the caller's answer too, and it must be yes wherever the state
//! resolver can answer. A contact it rules out is one that resolver never fills, so the job
//! writes what it always wrote and counts only what could take a field. Counting every contact
//! without a state had a log of DX contacts report 0 of thousands filled whenever the job ran.
//!
//! A contact with a hunted park or summit is never given a state from its call: the park places
//! it, and its call only where the activator lives (`station::call_places_state`).

use std::ops::ControlFlow;
use std::sync::Mutex;

use tempo_core::logbook::history::Source;
use tempo_core::logbook::sqlite::{Narrow, Order, Scope};

use crate::engine::{engine_lock, Engine};
use crate::logstore::{LogRows, READ_WAIT};
use crate::station::LogFill;

/// The `log_meta` key naming the resolver data the store's fills come from.
pub const FILL_VER: &str = "fill_ver";

/// What the job reads of each contact: the call, grid and DXCC number the resolvers place it from,
/// the two fields it fills, and the hunted park, whose contact's state the call never fills
/// ([`crate::station::call_places_state`]).
const FILLS: Narrow = Narrow {
    columns: &["call", "grid", "dxcc", "country", "state", "ota_their_ref"],
    uploads: false,
};

/// What one run of the job did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FillOutcome {
    /// The store was already filled for this version: nothing read, nothing written.
    pub current: bool,
    /// Contacts lacking a country, or a state where one can apply, when the job read them: the
    /// contacts that could take a field.
    pub lacking: usize,
    /// Contacts that gained a field.
    pub filled: usize,
}

/// Run the fill job once for resolver data `version` (see the module header), with `country` and
/// `state` the engine's resolvers and `state_applies` whether a state can apply to a call at all.
/// `Ok` with nothing done on the 1.13 path, which fills at the launch.
///
/// ⚠️ It reads the store and takes the Engine lock twice, briefly: call it on a thread of its
/// own, with no lock held — never from the radio loop.
pub fn fill_log_store(
    engine: &Mutex<Engine>,
    version: i64,
    country: &dyn Fn(&str) -> Option<String>,
    state: &dyn Fn(&str, Option<&str>) -> Option<String>,
    state_applies: &dyn Fn(&str) -> bool,
) -> Result<FillOutcome, String> {
    let (rows, number) = {
        let eng = engine_lock(engine);
        if eng.log_on_file() {
            return Ok(FillOutcome::default());
        }
        (eng.log_rows(), eng.station().dxcc_country.clone())
    };
    let LogRows::Store(reads) = rows;
    let ((current, lacking), _) = reads
        .read(READ_WAIT, |db| {
            if db.meta(FILL_VER)? == Some(version) {
                return Ok((true, Vec::new()));
            }
            let mut lacking = Vec::new();
            db.each_narrow(FILLS, Scope::All, Order::Log, &mut |r| {
                let no_state = r.state.is_none() && crate::station::call_places_state(r);
                if let Some(id) = r.id.filter(|_| r.country.is_none() || no_state) {
                    lacking.push((
                        id,
                        r.call.clone(),
                        r.grid.clone(),
                        r.dxcc,
                        r.country.is_none(),
                        no_state,
                    ));
                }
                ControlFlow::Continue(())
            })?;
            Ok((false, lacking))
        })
        .map_err(|e| e.to_string())?;
    if current {
        return Ok(FillOutcome {
            current: true,
            ..FillOutcome::default()
        });
    }
    // A missing country always counts; a missing state only where one can apply. The rest are
    // never resolved: the state resolver answers nothing for them.
    let lacking: Vec<_> = lacking
        .into_iter()
        .filter_map(|(id, call, grid, dxcc, no_country, no_state)| {
            let no_state = no_state && state_applies(&call);
            (no_country || no_state).then_some((id, call, grid, dxcc, no_country, no_state))
        })
        .collect();
    let found = lacking.len();
    let fills: Vec<LogFill> = lacking
        .into_iter()
        .filter_map(|(id, call, grid, dxcc, no_country, no_state)| {
            let country = no_country
                .then(|| {
                    crate::station::country_from(number.as_deref(), Some(country), &call, dxcc)
                })
                .flatten();
            let state = no_state.then(|| state(&call, grid.as_deref())).flatten();
            (country.is_some() || state.is_some()).then_some(LogFill { id, country, state })
        })
        .collect();
    let filled = crate::logwrite::fill(engine, &fills, version)?;
    Ok(FillOutcome {
        current: false,
        lacking: found,
        filled,
    })
}

/// The `log_meta` key that marks the one-time copy of merged contest contacts' received grid,
/// name and QTH into their own fields ([`copy_contest_columns`]) done: `1`.
pub const CONTEST_COLUMNS_COPIED: &str = "contest_columns_copied";

/// What the copy reads of each contact: its own grid, name and QTH, and beside them the contest
/// columns of the same names ([`CONTEST_COPIED_TAGS`](tempo_core::logbook::CONTEST_COPIED_TAGS)).
const COPIES: Narrow = Narrow {
    columns: &["call", "grid", "name", "qth"],
    uploads: false,
};

/// What one run of the copy did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyOutcome {
    /// The store was marked copied already: nothing read, nothing written.
    pub current: bool,
    /// Contacts that took a value into a field of their own.
    pub copied: usize,
    /// The history run that holds every change it made — one Undo of it puts each contact back
    /// as it was. `None` when it changed nothing.
    pub run: Option<String>,
}

/// ★ **Once per store: a contact merged from a contest before the merge wrote a received grid,
/// name and QTH into the record's own fields gets them there** — each only where its own field is
/// blank, copied from the contest column, which stays as it was
/// ([`tempo_core::logbook::with_contest_columns_copied`]). Until then such a contact showed no
/// grid in the Grid column and counted for no VUCC square, though its upload carried the grid.
///
/// Written as the fill job's fills are ([`crate::logwrite::copy_contest_columns`]): ONE run in
/// the history, of its own source ([`Source::ContestColumns`]), which the Logbook lists for one
/// Undo of the whole of it; no upload stamp moves and nothing is queued to a service, because
/// every contact's ADIF carries the same tags and values as before. A copy a crash or a busy log
/// cut short is taken up at the next launch in the SAME run (the one of that source the store
/// holds), so it stays one Undo, for the contacts it had not reached: each contact is copied
/// into at most once, so one the operator undid stays as they left it. Done once: [`CONTEST_COLUMNS_COPIED`] goes into the store with
/// the last change, and a store marked so is not read again. On the 1.13 path it does nothing:
/// that path reads its contacts from `log.adi`, which gives a contest column back in the record's
/// own field already.
///
/// ⚠️ It reads the store and takes the Engine lock: call it on a thread of its own, with no lock
/// held — never from the radio loop.
pub fn copy_contest_columns(engine: &Mutex<Engine>) -> Result<CopyOutcome, String> {
    copy_contest_columns_in_chunks(engine, crate::logwrite::FILL_CHUNK)
}

/// [`copy_contest_columns`], `chunk` contacts to a change.
pub(crate) fn copy_contest_columns_in_chunks(
    engine: &Mutex<Engine>,
    chunk: usize,
) -> Result<CopyOutcome, String> {
    let rows = {
        let eng = engine_lock(engine);
        if eng.log_on_file() {
            return Ok(CopyOutcome::default());
        }
        eng.log_rows()
    };
    let LogRows::Store(reads) = rows;
    let ((current, ids, started), _) = reads
        .read(READ_WAIT, |db| {
            if db.meta(CONTEST_COLUMNS_COPIED)? == Some(1) {
                return Ok((true, Vec::new(), None));
            }
            // Only this copy writes runs of its source: one in the store is this copy's, cut
            // short before its last change.
            let started = db
                .history_latest_run(Some(Source::ContestColumns.code()))?
                .map(|r| r.run);
            // A contact it copied into already is never copied again — whether that change still
            // stands or the operator undid it, which is theirs to keep.
            let done: std::collections::HashSet<_> = match &started {
                Some(run) => db.history_run(run)?.into_iter().map(|e| e.qso_id).collect(),
                None => Default::default(),
            };
            let mut ids = Vec::new();
            db.each_narrow_with(
                COPIES,
                &tempo_core::logbook::CONTEST_COPIED_TAGS,
                Scope::All,
                Order::Log,
                &mut |r| {
                    if let Some(id) = r.id.filter(|id| {
                        !done.contains(id)
                            && tempo_core::logbook::with_contest_columns_copied(r).is_some()
                    }) {
                        ids.push(id);
                    }
                    ControlFlow::Continue(())
                },
            )?;
            Ok((false, ids, started))
        })
        .map_err(|e| e.to_string())?;
    if current {
        return Ok(CopyOutcome {
            current: true,
            ..CopyOutcome::default()
        });
    }
    let run = started.unwrap_or_else(crate::loghistory::new_run);
    let copied = crate::logwrite::copy_contest_columns(engine, &ids, &run, chunk)?;
    Ok(CopyOutcome {
        current: false,
        copied,
        run: (copied > 0).then_some(run),
    })
}
