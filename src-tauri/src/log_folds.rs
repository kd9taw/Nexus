//! The log's folds and lookups, read from the logbook store — SPEC-2 v3's **C14**.
//!
//! Every pass the command layer makes over the log — the needs model the Needed board,
//! propagation, satellite passes and the pounce thread share; the award summary; the Journey;
//! the log statistics and the Statistics view's timeline; today's hunted parks; a station's last
//! logged grid; the upload health the Settings panel polls; the oldest LoTW upload awaiting its
//! echo; the catch-up sweep's pick; the compound calls the launch seeds the decoder with — reads
//! the store here, through ONE door ([`LogRows`]), and runs the fold it always ran over what it
//! reads.
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
//!    fold is REFUSED ([`Freshness::or_refuse`]) and nothing is kept, so the caller's next ask
//!    reads again. It never answers from the store as it stands: that answer left out the
//!    contact just logged, and the operator saw needs and statistics without it.
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
//! today's contacts alone, which is the whole of their cost. Nor is the Statistics view's
//! timeline, for the same reason: its window's contacts alone, through the same index.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use tempo_app::engine::{engine_lock, Engine};
use tempo_app::logstore::{Freshness, LogRows};
use tempo_core::logbook::query::{LogFold, LogTimeline, TimelineCounter, TimelineRange};
use tempo_core::logbook::sqlite::{call_norm_of, Narrow, Order, Scope};
use tempo_core::logbook::{QsoRecord, AWARD_EXTRAS};

use crate::{qso_is_sat, LogTallies, Tally};

// ── what each pass reads ────────────────────────────────────────────────────
//
// The four confirmation channels ride with any pass that reads `award_confirmed` or
// `confirmed`: the decoder derives both from them, exactly as it does for a whole record.

/// `LogNeeds::add_qso`'s inputs — with the passthrough fields [`AWARD_EXTRAS`] names, read beside
/// them: the needs count each contact as the awards do ([`award_fields`]).
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
        "dxcc",
    ],
    uploads: false,
};

/// [`award_add`]'s inputs — with the passthrough fields [`AWARD_EXTRAS`] names, read beside them.
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
        "ota_their_program",
        "ota_their_ref",
        "prop_mode",
        "dxcc",
        "when_unix",
    ],
    uploads: false,
};

/// The passthrough fields the award fold reads: [`AWARD_EXTRAS`], and the WWFF reference and QSL
/// route its WWFF count and What-to-QSL list read, and the county the US Counties award counts
/// ([`award_extras`]). `STATE` and `CNTY` bring a merged contest contact's exchange state and
/// county, as there.
const AWARDS_EXTRAS: &[&str] = &[
    "CQZ",
    tempo_core::logbook::LOTW_DXCC_FIELD,
    tempo_core::logbook::EQSL_AG_FIELD,
    "WWFF_REF",
    "QSL_VIA",
    "STATE",
    "CNTY",
];

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

/// A callsign, and nothing else: the compound calls the launch seeds.
const CALLS: Narrow = Narrow {
    columns: &["call"],
    uploads: false,
};

/// The log statistics' geography: a callsign and the record's DXCC number — with the passthrough
/// fields [`AWARD_EXTRAS`] names (its CQ zone, and LoTW's DXCC), read beside them.
const GEOGRAPHY: Narrow = Narrow {
    columns: &["call", "dxcc"],
    uploads: false,
};

/// [`hunted_today`]'s inputs.
const HUNTED: Narrow = Narrow {
    columns: &["call", "when_unix", "ota_their_ref"],
    uploads: false,
};

/// [`log_timeline`]'s inputs.
const TIMELINE: Narrow = Narrow {
    columns: &["mode", "when_unix"],
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

/// The contact's time alone.
const WHEN: Narrow = Narrow {
    columns: &["when_unix"],
    uploads: false,
};

/// Keep a fold's answer against `mark`, and hand it back — unless the read behind it was stale
/// (the writer had not taken every change made before it was asked), when the fold is refused
/// ([`Freshness::or_refuse`]) and nothing is kept, so the next asker reads the store again.
fn keep<K: PartialEq, V>(
    tally: &Tally<K, V>,
    mark: u64,
    key: K,
    value: V,
    fresh: Freshness,
) -> Result<Arc<V>, String> {
    fresh.or_refuse()?;
    Ok(tally.put(mark, key, value))
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
        .each_with(NEEDS, AWARD_EXTRAS, Scope::All, Order::Log, &mut |q| {
            needs_add(&mut needs, q);
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    keep(kept, mark, (), needs, fresh)
}

/// One record into the needs model.
fn needs_add(needs: &mut propagation::LogNeeds, q: &QsoRecord) {
    // A "needs confirmation" must be award-grade (LoTW/paper), not eQSL.
    needs.add_qso(
        &q.call,
        &q.band,
        &q.mode,
        q.grid.as_deref(),
        // The state its upload carries: a merged contest contact's exchange, unless the
        // operator has edited it (`record_field`).
        tempo_core::logbook::record_field(q, "STATE"),
        q.award_confirmed,
        qso_is_sat(q.prop_mode.as_deref()),
        award_fields(q),
    );
}

/// The award-needs model of the whole log, folded again only when the log's content moves.
pub(crate) fn needs_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<propagation::LogNeeds>, String> {
    let eng = engine_lock(engine);
    let capture = needs_capture(&eng, &tallies.needs);
    drop(eng);
    needs_finish(capture, &tallies.needs)
}

// ── the award summary, and the Stats pane's index ────────────────────────────

/// What the award fold keeps: the summary `get_awards` serves, and the Stats pane's index of the
/// same contacts, filled by the same fold at the points it credits each award
/// ([`propagation::CallStatsIndex`]). One pass makes both, so the pane's grids can never count a
/// contact differently from the Awards view.
pub(crate) struct AwardsKept {
    pub(crate) summary: Arc<propagation::AwardSummary>,
    pub(crate) call_stats: Arc<propagation::CallStatsIndex>,
    /// The whole-log DXCC chart and the What-to-QSL list, from the same fold.
    pub(crate) chart: Arc<propagation::DxccChart>,
    pub(crate) qsl_wants: Arc<Vec<propagation::QslWant>>,
    /// The WPX prefixes and WAE countries worked per band and mode class, for the Needed board's
    /// opt-in chips ([`award_need_chips`]).
    pub(crate) wpx_wae: Arc<propagation::WpxWaeWorked>,
}

/// `get_awards`'s summary, folded again only when the log's content or the operator's call (the
/// home entity "First DX" is judged against) has moved. The main window polls it twice a minute.
pub(crate) fn awards_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<propagation::AwardSummary>, String> {
    Ok(awards_fold_kept(engine, tallies)?.summary.clone())
}

/// The Stats pane's index ([`get_call_stats`](crate::get_call_stats)), kept with the summary it
/// is folded beside: asking it of an unchanged log costs no read of the log.
pub(crate) fn call_stats_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<propagation::CallStatsIndex>, String> {
    Ok(awards_fold_kept(engine, tallies)?.call_stats.clone())
}

/// `get_dxcc_chart`'s chart, kept with the summary it is folded beside.
pub(crate) fn dxcc_chart_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<propagation::DxccChart>, String> {
    Ok(awards_fold_kept(engine, tallies)?.chart.clone())
}

/// `get_what_to_qsl`'s list, kept with the summary it is folded beside.
pub(crate) fn what_to_qsl_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<Vec<propagation::QslWant>>, String> {
    Ok(awards_fold_kept(engine, tallies)?.qsl_wants.clone())
}

/// The Needed board's opt-in WPX and WAE chips on `alerts` ([`propagation::mark_award_needs`]),
/// from the award fold the Awards view counts with. `wpx` and `wae` are the operator's two
/// settings; with both off, as they ship, the rows come back untouched and the fold is not asked
/// for at all.
///
/// The chips only decorate the board: if the fold is refused (the writer is behind,
/// [`Freshness::or_refuse`]) the rows come back without them, and the next poll asks again.
pub(crate) fn award_need_chips(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
    mut alerts: Vec<propagation::NeedAlert>,
    wpx: bool,
    wae: bool,
) -> Vec<propagation::NeedAlert> {
    if !wpx && !wae {
        return alerts;
    }
    if let Ok(kept) = awards_fold_kept(engine, tallies) {
        propagation::mark_award_needs(&mut alerts, &kept.wpx_wae, wpx, wae);
    }
    alerts
}

/// The Stats pane's answer for `call` typed on `band` in `mode`, from the kept index
/// ([`call_stats_kept`]). The station has no record, so where it is comes from `state_of` — the
/// licence index in the app ([`crate::us_state_hint`]), as for a station heard.
pub(crate) fn call_stats_for(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
    call: &str,
    band: &str,
    mode: &str,
    state_of: impl Fn(&str) -> Option<String>,
) -> Result<propagation::CallStats, String> {
    let index = call_stats_kept(engine, tallies)?;
    let call = call.trim().to_ascii_uppercase();
    Ok(index.stats_for(&call, band, mode, state_of(&call).as_deref()))
}

/// The award fold, kept against the log's content and the operator's call.
fn awards_fold_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<AwardsKept>, String> {
    let eng = engine_lock(engine);
    let (mark, my_call) = (eng.log_index_rev(), eng.settings().mycall.clone());
    if let Some(kept) = tallies.awards.get(mark, &my_call) {
        return Ok(kept);
    }
    let rows = eng.log_rows();
    drop(eng);
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("the awards fold");
    let mut awards = propagation::Awards::with_call_stats();
    // Tell the accumulator our own entity so "First DX" counts only foreign ones.
    awards.set_home_call(&my_call);
    let fresh = rows
        .each_with(AWARDS, AWARDS_EXTRAS, Scope::All, Order::Log, &mut |q| {
            award_add(&mut awards, q);
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    let kept = AwardsKept {
        call_stats: Arc::new(awards.take_call_stats().unwrap_or_default()),
        summary: Arc::new(awards.summary()),
        chart: Arc::new(awards.dxcc_chart()),
        qsl_wants: Arc::new(awards.what_to_qsl()),
        wpx_wae: Arc::new(awards.wpx_wae_worked()),
    };
    keep(&tallies.awards, mark, my_call, kept, fresh)
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

/// What a contact's record carries for the awards beyond its call: its DXCC number, LoTW's where
/// that differs, its CQ zone, when it was made, and which channels confirmed it (eQSL's
/// Authenticity-Guaranteed mark among them). The one reading [`award_add`], the needs model, the
/// log statistics and Remote's folds of them all take, so each counts a contact as the awards do.
pub(crate) fn award_fields(q: &QsoRecord) -> propagation::RecordFields {
    propagation::RecordFields {
        dxcc: q.dxcc,
        lotw_dxcc: tempo_core::logbook::record_lotw_dxcc(q),
        cqz: tempo_core::logbook::record_cq_zone(q),
        when_unix: q.when_unix,
        qsl: propagation::QslChannels {
            card: q.qsl_rcvd.card,
            lotw: q.qsl_rcvd.lotw,
            eqsl: q.qsl_rcvd.eqsl,
            eqsl_ag: tempo_core::logbook::record_eqsl_ag(q),
            qrz: q.qsl_rcvd.qrz,
        },
    }
}

/// What a contact's record carries for the awards counted beside DXCC: its WWFF reference (ADIF
/// `WWFF_REF`, else `SIG_INFO` beside `SIG`=WWFF) and its QSL route (`QSL_VIA`).
pub(crate) fn award_extras(q: &QsoRecord) -> propagation::ContactExtras<'_> {
    let extra = |name: &str| {
        q.extra
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    };
    let sig_wwff = q
        .ota
        .their_program
        .as_deref()
        .is_some_and(|p| p.trim().eq_ignore_ascii_case("WWFF"));
    propagation::ContactExtras {
        wwff: extra("WWFF_REF").or_else(|| q.ota.their_ref.as_deref().filter(|_| sig_wwff)),
        qsl_via: extra("QSL_VIA"),
        // Where the record keeps it, a merged contest contact's exchange included.
        cnty: tempo_core::logbook::record_field(q, "CNTY"),
    }
}

/// One contact into the award fold — the one step [`awards_kept`] and [`awards_for_records`]
/// both take.
fn award_add(awards: &mut propagation::Awards, q: &QsoRecord) {
    // Award-eligible confirmation only (LoTW/paper) — eQSL doesn't count; plus whether ARRL has
    // granted DXCC-family credit (DXCC / DXCC_BAND / DXCC_MODE / … — real LoTW exports use the
    // granular codes).
    let credited = q.credit_granted.iter().any(|c| c.starts_with("DXCC"));
    awards.add_contact(
        &q.call,
        &q.band,
        &q.mode,
        q.award_confirmed,
        credited,
        // The paper-card channel alone, which the IOTA card shows beside its LoTW-or-card count.
        q.qsl_rcvd.card,
        // The state its upload carries (`record_field`), as the WAS needs and custom awards read.
        tempo_core::logbook::record_field(q, "STATE"),
        q.grid.as_deref(),
        q.ota.iota.as_deref(),
        qso_is_sat(q.prop_mode.as_deref()),
        award_fields(q),
        award_extras(q),
    );
}

// ── award status ────────────────────────────────────────────────────────────

/// One award's status over the log ([`propagation::award_status`]): entities submitted and
/// granted, and one contact per entity ready to submit — its newest confirmed one where `newest`,
/// else its oldest. A pass over WHOLE records (the credit codes and the county ride beside the
/// fields a fold reads), off the Engine lock, refused when the store has not taken every change
/// made before it was asked. Kept nowhere: it is asked when the operator opens the list.
pub(crate) fn award_status(
    engine: &Mutex<Engine>,
    kind: propagation::award_status::AwardKind,
    newest: bool,
) -> Result<propagation::award_status::AwardStatus, String> {
    let rows = engine_lock(engine).log_rows();
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("the award status");
    let mut fold = propagation::award_status::StatusFold::new(kind, newest);
    rows.each_record(tempo_app::logstore::READ_WAIT, &mut |q| {
        if let Some(id) = q.id {
            let id = id.to_string();
            fold.add(&status_contact(q, &id));
        }
        ControlFlow::Continue(())
    })
    .map_err(unreadable)?
    .or_refuse()?;
    Ok(fold.finish())
}

/// A whole record as the award status reads it, by the readings the award fold takes
/// ([`award_fields`], [`award_add`]).
pub(crate) fn status_contact<'a>(
    q: &'a QsoRecord,
    id: &'a str,
) -> propagation::award_status::StatusContact<'a> {
    propagation::award_status::StatusContact {
        id,
        call: &q.call,
        when_unix: q.when_unix,
        band: &q.band,
        mode: &q.mode,
        award_confirmed: q.award_confirmed,
        sat: qso_is_sat(q.prop_mode.as_deref()),
        state: tempo_core::logbook::record_field(q, "STATE"),
        cnty: tempo_core::logbook::record_field(q, "CNTY"),
        iota: q.ota.iota.as_deref(),
        rec: award_fields(q),
        granted: &q.credit_granted,
        submitted: &q.credit_submitted,
    }
}

// ── the custom awards ───────────────────────────────────────────────────────

/// The operator's custom awards' standing ([`propagation::custom`]), folded again only when the
/// log's content or the definitions move. A pass over WHOLE records: a field award may name any
/// ADIF field the log holds. No definitions, no pass.
pub(crate) fn custom_awards_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
    defs: Vec<propagation::custom::CustomAward>,
) -> Result<Arc<Vec<propagation::custom::CustomAwardProgress>>, String> {
    let eng = engine_lock(engine);
    let mark = eng.log_index_rev();
    if let Some(kept) = tallies.custom.get(mark, &defs) {
        return Ok(kept);
    }
    let rows = eng.log_rows();
    drop(eng);
    let mut fold = propagation::custom::CustomAwards::new(&defs);
    let mut fresh = Freshness::Current;
    if !fold.is_empty() {
        tempo_core::logbook::io_fence::whole_log_off_engine_lock("the custom awards fold");
        fresh = rows
            .each_record(tempo_app::logstore::READ_WAIT, &mut |q| {
                custom_add(&mut fold, q);
                ControlFlow::Continue(())
            })
            .map_err(unreadable)?;
    }
    keep(&tallies.custom, mark, defs, fold.progress(), fresh)
}

/// One whole record into the custom awards' fold.
pub(crate) fn custom_add(fold: &mut propagation::custom::CustomAwards, q: &QsoRecord) {
    // The references a contact carries: its park or summit, its island group, and the WWFF,
    // POTA and SOTA fields the log keeps beside a modelled one.
    let mut refs: Vec<&str> = Vec::new();
    refs.extend(q.ota.their_ref.as_deref());
    refs.extend(q.ota.iota.as_deref());
    for (k, v) in &q.extra {
        if matches!(
            k.as_str(),
            "WWFF_REF" | "POTA_REF" | "SOTA_REF" | "SIG_INFO"
        ) {
            refs.push(v);
        }
    }
    // A field by its ADIF name: the modelled ones the log keeps as fields, else wherever the
    // record keeps it (`record_field`) — a merged contest contact's county, section or class is
    // in its contest columns, not beside the fields.
    let field = |name: &str| -> Option<String> {
        let some = |v: Option<&str>| v.map(str::to_string);
        match name {
            "CALL" => Some(q.call.clone()),
            "BAND" => Some(q.band.clone()),
            "MODE" => Some(q.mode.clone()),
            "COUNTRY" => some(q.country.as_deref()),
            "DXCC" => q.dxcc.map(|d| d.to_string()),
            "IOTA" => some(q.ota.iota.as_deref()),
            "SIG" => some(q.ota.their_program.as_deref()),
            "SIG_INFO" => some(q.ota.their_ref.as_deref()),
            "PROP_MODE" => some(q.prop_mode.as_deref()),
            "SAT_NAME" => some(q.sat_name.as_deref()),
            "OPERATOR" => some(q.operator.as_deref()),
            "STATION_CALLSIGN" => some(q.station_callsign.as_deref()),
            // A VHF contest's sent grid is its exchange's `MY_GRIDSQUARE`, which no stamp
            // shadows (the merge stamps none beside it).
            "MY_GRIDSQUARE" => some(
                q.my_grid
                    .as_deref()
                    .or_else(|| tempo_core::logbook::record_field(q, name)),
            ),
            other => some(tempo_core::logbook::record_field(q, other)),
        }
    };
    fold.add(&propagation::custom::CustomContact {
        call: &q.call,
        band: &q.band,
        mode: &q.mode,
        grid: tempo_core::logbook::record_field(q, "GRIDSQUARE"),
        refs: &refs,
        field: &field,
        lotw_card: q.award_confirmed,
        any: q.confirmed,
    });
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
    // `compute_log_stats`' own accumulator, a contact at a time, with what its record carries.
    let mut stats = propagation::stats::LogStatsAccumulator::new(&my_call);
    let fresh = rows
        .each_with(GEOGRAPHY, AWARD_EXTRAS, Scope::All, Order::Log, &mut |q| {
            stats.add(&q.call, award_fields(q));
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    keep(&tallies.stats, mark, my_call, stats.summary(), fresh)
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
        .each_with(JOURNEY, &["STATE"], Scope::All, Order::Log, &mut |r| {
            qsos.push(journey_qso(r));
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    let grid = (!key.grid.is_empty()).then_some(key.grid.as_str());
    let model = propagation::journey_model(&qsos, &key.call, grid, key.power_w, key.streak);
    keep(&tallies.journey, mark, key, model, fresh)
}

// ── POTA activator achievements ─────────────────────────────────────────────

/// What [`pota_activator_kept`] reads: what keys a contact to an activation
/// (`tempo_core::logbook::Activations`) and what tells its contacts apart by POTA's rule.
const ACTIVATIONS: Narrow = Narrow {
    columns: &[
        "call",
        "band",
        "mode",
        "when_unix",
        "ota_my_program",
        "ota_my_ref",
        "station_callsign",
        "operator",
    ],
    uploads: false,
};

/// The log's POTA activator standing per callsign, a portable `/P` merged into the plain call
/// (`tempo_core::pota_awards::add`). Kept against `index_rev`: no stamp
/// changes a park, a call, a band, a mode or a time.
pub(crate) fn pota_activator_kept(
    engine: &Mutex<Engine>,
    tallies: &LogTallies,
) -> Result<Arc<Vec<tempo_core::pota_awards::ActivatorStats>>, String> {
    let eng = engine_lock(engine);
    let mark = eng.log_index_rev();
    if let Some(kept) = tallies.pota_activator.get(mark, &()) {
        return Ok(kept);
    }
    let rows = eng.log_rows();
    drop(eng);
    tempo_core::logbook::io_fence::whole_log_off_engine_lock("the POTA activator fold");
    let mut found = tempo_core::logbook::Activations::default();
    let fresh = rows
        .each_with(ACTIVATIONS, &[], Scope::All, Order::Log, &mut |r| {
            tempo_core::pota_awards::add(&mut found, r);
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?;
    let stats = tempo_core::pota_awards::by_callsign(&found.finish());
    keep(&tallies.pota_activator, mark, (), stats, fresh)
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
        // The state its upload carries: a merged contest contact's exchange (`record_field`).
        state: tempo_core::logbook::record_field(r, "STATE").map(str::to_string),
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
    .map_err(unreadable)?
    .or_refuse()?;
    Ok(propagation::HuntedActivations::from_log(worked))
}

// ── the Statistics view's timeline ──────────────────────────────────────────

/// `get_log_timeline`'s answer at `now`: contacts per mode over the past day, 30 days or year,
/// in UTC as the log is ([`TimelineCounter`] holds the window rule). Only the window's contacts
/// are read — through the `qso_recent` index, whatever the log's size — and the counter still
/// tests every row it is handed. Not kept: the window moves with the clock, and its read is the
/// whole of its cost.
///
/// ⚠️ It reads the store: never under the Engine lock. Take `rows` ([`Engine::log_rows`]) under
/// it.
pub(crate) fn log_timeline(
    rows: &LogRows,
    range: TimelineRange,
    now: i64,
) -> Result<LogTimeline, String> {
    // A clock before 1970 is asked about 1970.
    let mut timeline = TimelineCounter::new(range, u64::try_from(now).unwrap_or(0));
    rows.each(
        TIMELINE,
        Scope::Since(timeline.since()),
        Order::Log,
        &mut |q| {
            timeline.add(q);
            ControlFlow::Continue(())
        },
    )
    .map_err(unreadable)?
    .or_refuse()?;
    Ok(timeline.finish())
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
    .map_err(unreadable)?
    .or_refuse()?;
    Ok(best.map(|(_, g)| g))
}

// ── a station's last logged county or state ─────────────────────────────────

/// [`newest_logged_qth`]'s inputs: enough to choose the row; the received exchange of the
/// one chosen is read whole afterwards, since a narrow read never carries an exchange.
const QTHS: Narrow = Narrow {
    columns: &["call", "state", "contest_id", "when_unix"],
    uploads: false,
};

/// Where the log last had `peer`, for the contest entry's QTH box: the newest logged contact
/// with that callsign (exact, case aside) that says. A row of this contest, any year's (its
/// `CONTEST_ID` one of `contest_ids`), says what it received in `slot`; any other row says its
/// `STATE`. Another contest's received slot is never read: the same slot id holds another
/// sponsor's counties there (a California "LAKE" is not Illinois's). The value is raw, as
/// logged; the strip fills it only if it is one of the slot's own codes. With the time of that
/// contact, for the strip's source tag. Of two contacts in the same second the later in the
/// log wins, as for [`newest_logged_grid`]. Only that callsign's contacts are read, through
/// the `qso_callhist` index, and only this contest's rows are read whole, newest first, until
/// one received the slot.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn newest_logged_qth(
    rows: &LogRows,
    peer: &str,
    contest_ids: &[String],
    slot: &str,
) -> Result<Option<(String, u64)>, String> {
    // Newest first: (when, Ok(state)) for an ordinary row, (when, Err(id)) for this contest's.
    let mut found: Vec<(u64, Result<String, tempo_core::logbook::RecordId>)> = Vec::new();
    rows.each_with(
        QTHS,
        &["STATE"],
        Scope::CallNorm(&call_norm_of(peer)),
        Order::Log,
        &mut |q| {
            if q.call.eq_ignore_ascii_case(peer) {
                let ours = q
                    .contest
                    .as_deref()
                    .is_some_and(|c| contest_ids.contains(&c.contest_id));
                // Another row's state as its upload carries it (`record_field`): a merged
                // contest contact's exchange STATE.
                match (ours, q.id, tempo_core::logbook::record_field(q, "STATE")) {
                    (true, Some(id), _) => found.push((q.when_unix, Err(id))),
                    (false, _, Some(st)) if !st.is_empty() => {
                        found.push((q.when_unix, Ok(st.to_string())))
                    }
                    _ => {}
                }
            }
            ControlFlow::Continue(())
        },
    )
    .map_err(unreadable)?
    .or_refuse()?;
    // Stable, so of two in the same second the later in log order stays first once reversed.
    found.sort_by_key(|(when, _)| *when);
    for (when, said) in found.into_iter().rev() {
        match said {
            Ok(state) => return Ok(Some((state, when))),
            Err(id) => {
                let (whole, fresh) = rows.rows_by_ids(&[id]).map_err(unreadable)?;
                fresh.or_refuse()?;
                let received = whole
                    .iter()
                    .filter_map(|r| r.contest.as_deref())
                    .flat_map(|c| c.rcvd.iter())
                    .find(|(k, v)| k == slot && !v.trim().is_empty())
                    .map(|(_, v)| v.trim().to_string());
                if let Some(v) = received {
                    return Ok(Some((v, when)));
                }
            }
        }
    }
    Ok(None)
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
    keep(&tallies.upload_health, mark, (), health, fresh)
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
    .map_err(unreadable)?
    .or_refuse()?;
    Ok(oldest.map(tempo_core::logbook::lotw_pull_date))
}

/// When the log's oldest contact was made, the year a whole LoTW download asks back to
/// (`crate::lotw_history_by_year`). `None` for an empty log.
///
/// ⚠️ It reads the store: never under the Engine lock.
pub(crate) fn oldest_contact_unix(rows: &LogRows) -> Result<Option<u64>, String> {
    let mut oldest: Option<u64> = None;
    rows.each(WHEN, Scope::All, Order::Log, &mut |q| {
        oldest = Some(oldest.map_or(q.when_unix, |o| o.min(q.when_unix)));
        ControlFlow::Continue(())
    })
    .map_err(unreadable)?
    .or_refuse()?;
    Ok(oldest)
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
    .map_err(unreadable)?
    .or_refuse()?;
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
    use crate::remote_service::stored_log_tests::StoredLog;
    use propagation::model::Band;
    use propagation::OperatorNeeds;
    use tempo_app::logstore::NOT_ANSWERED;
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
                // Since the needs count by the record's own numbers, the oracle reads them off
                // the whole record — the store's narrow read must carry them to match it.
                award_fields(q),
            );
        }
        needs
    }

    /// `log_stats`' fold over the log in memory: `compute_log_stats`' accumulator, each contact
    /// with what its whole record carries — which the store's narrow read must hand over.
    fn log_stats_old(records: &[Arc<QsoRecord>], my_call: &str) -> propagation::LogStats {
        let mut stats = propagation::stats::LogStatsAccumulator::new(my_call);
        for q in records {
            stats.add(&q.call, award_fields(q));
        }
        stats.summary()
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
            awards.add_contact(
                &q.call,
                &q.band,
                &q.mode,
                q.award_confirmed,
                credited,
                // The paper-card channel alone, which the IOTA card shows beside its
                // LoTW-or-card count.
                q.qsl_rcvd.card,
                q.state.as_deref(),
                q.grid.as_deref(),
                q.ota.iota.as_deref(),
                qso_is_sat(q.prop_mode.as_deref()),
                // Since the awards count by the record's own numbers, the oracle reads them off
                // the whole record — the store's narrow read must carry them to match it.
                award_fields(q),
                // …and the WWFF reference and QSL route likewise.
                award_extras(q),
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
    /// it never worked) on every band in each mode below.
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
                // Each class a source can name, and the exact modes these logs hold plus one they
                // never do: the mode need is judged per mode.
                for m in [
                    "CW", "Phone", "Digital", "FT8", "FT4", "SSB", "RTTY", "MFSK", "FM",
                ] {
                    out.push(format!("{e}/{b:?}/{m}={:?}", n.need(e, b, m)));
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

    /// The timeline's three windows.
    const RANGES: [TimelineRange; 3] = [
        TimelineRange::PastDay,
        TimelineRange::Past30Days,
        TimelineRange::PastYear,
    ];

    /// The timeline over the whole log in memory: every contact handed to the counter, which
    /// decides the window itself — where the store reads only the window's, off its index.
    fn timeline_old(records: &[Arc<QsoRecord>], range: TimelineRange) -> LogTimeline {
        tempo_core::logbook::query::reference::fold(
            records,
            TimelineCounter::new(range, NOW as u64),
        )
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
                observed(&needs_kept(e, tallies).expect("reads")),
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
                log_stats_old(&held, MY_CALL),
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
                "timeline",
                RANGES.map(|range| log_timeline(&rows, range, NOW).expect("reads")),
                RANGES.map(|range| timeline_old(&held, range)),
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
        for range in RANGES {
            assert!(
                !timeline_old(&held, range).modes.is_empty(),
                "premise: contacts in the {range:?} window"
            );
        }
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
                    &|_: &str| true,
                );
            }
        }
        // Back once the store holds the change, so the folds asked next are asked about it rather
        // than about the disk's speed ([`StoredLog::caught_up`]); P4 itself is the test below
        // that holds the writer.
        e.caught_up();
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
    /// answered, and whether it had to wait. Its caller's part too: a fold refused because the
    /// write had still not landed when its wait ran out (a loaded machine) is asked again.
    fn asked_straight_after<T: Send>(
        d: &Dir,
        e: &crate::SharedEngine,
        contact: QsoRecord,
        mut fold: impl FnMut() -> Result<T, String> + Send,
    ) -> T {
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(contact);
        std::thread::scope(|s| {
            let asked = s.spawn(move || answered(&mut fold));
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

    /// `fold`'s answer, asked again while it is refused for a change still on its way to disk
    /// ([`NOT_ANSWERED`]), for up to a test's budget ([`tempo_app::test_util::TEST_WAIT`]). Any
    /// other refusal fails, and the first answer is the one returned: never a retry until green.
    fn answered<T>(fold: &mut impl FnMut() -> Result<T, String>) -> T {
        let deadline = std::time::Instant::now() + tempo_app::test_util::TEST_WAIT;
        loop {
            match fold() {
                Ok(answer) => return answer,
                Err(why)
                    if why.starts_with(NOT_ANSWERED) && std::time::Instant::now() < deadline => {}
                Err(why) => panic!("the fold was refused: {why}"),
            }
        }
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

    /// Contacts whose records carry what the awards count by beyond the call — LoTW-confirmed,
    /// 20 m CW. The numbers are cty.csv's and TrustedQSL's.
    fn by_record_log() -> String {
        let row = |call: &str, date: &str, fields: &str| {
            format!(
                "<CALL:{}>{call}<QSO_DATE:8>{date}<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
                 <LOTW_QSL_RCVD:1>Y{fields}<EOR>\n",
                call.len()
            )
        };
        [
            // Navassa on a US 1×1 call, its zone with it.
            row("K1N", "20150210", "<DXCC:3>182<CQZ:1>8"),
            // A logger that wrote the call's US number; LoTW's Baker & Howland kept beside it.
            row("K1B", "20021201", "<DXCC:3>291<APP_NEXUS_LOTW_DXCC:2>20"),
            row("K3J", "20010101", "<DXCC:3>123"),
            // The German Democratic Republic, deleted in 1990.
            row("Y24UK", "19890101", "<DXCC:3>229"),
            // A W1 licensee in California (zone 3), and a KP4 licensee in Florida.
            row("K1ABC", "20200101", "<CQZ:1>3<STATE:2>CA"),
            row("KP4ABC", "20200102", "<DXCC:3>291<STATE:2>FL"),
        ]
        .concat()
    }

    /// ★ The contest entry's pre-fill from the whole log: the NEWEST contact with the call that
    /// says where the station was. This contest's rows (any year) say their received slot, and
    /// another contest's never do (its QTH is another sponsor's county: a California LAKE), so
    /// such a row says only its STATE. Each answer by value.
    #[test]
    fn the_newest_logged_qth_is_this_contests_slot_or_a_state_never_another_contests_slot() {
        let row = |call: &str, date: &str, fields: String| {
            format!(
                "<CALL:{}>{call}<QSO_DATE:8>{date}<TIME_ON:6>123400<BAND:3>40m<MODE:2>CW{fields}<EOR>\n",
                call.len()
            )
        };
        let tag = |name: &str, val: &str| format!("<{name}:{}>{val}", val.len());
        let party = |id: &str, qth: &str| {
            let ex = tempo_core::contest::carrier::encode_pairs(&[
                ("RST".to_string(), "599".to_string()),
                ("QTH".to_string(), qth.to_string()),
            ]);
            format!("{}{}", tag("CONTEST_ID", id), tag("APP_NEXUS_EX", &ex))
        };
        let log = [
            // K9AAA: last year's party from DuPage, then a ragchew giving a state, then this
            // contest's party from Cook — the newest, so Cook.
            row("K9AAA", "20251018", party("IL-QSO-PARTY", "DUPA")),
            row("K9AAA", "20260101", tag("STATE", "IL")),
            row("K9AAA", "20261018", party("IL-QSO-PARTY", "COOK")),
            // K9BBB: last year's party from Kane, then a California party from (CA) Lake: the
            // newest row is another contest's, which says nothing, so last year's KANE.
            row("K9BBB", "20251018", party("IL-QSO-PARTY", "KANE")),
            row("K9BBB", "20261003", party("CA-QSO-PARTY", "LAKE")),
            // K9CCC: an ordinary contact giving a state after this contest's row.
            row("K9CCC", "20251018", party("IL-QSO-PARTY", "WILL")),
            row("K9CCC", "20260301", tag("STATE", "WI")),
            // K9DDD: only another contest's row: nothing.
            row("K9DDD", "20261003", party("CA-QSO-PARTY", "MARN")),
        ]
        .concat();
        let d = Dir::new("logged-qth");
        std::fs::write(
            d.log(),
            format!("{}{log}", tempo_core::logbook::adif_header()),
        )
        .unwrap();
        let e = launch(&d);
        let rows = e.lock().unwrap().log_rows();
        let ids = ["IL-QSO-PARTY".to_string()];
        let qth = |call: &str| {
            answered(&mut || newest_logged_qth(&rows, call, &ids, "QTH")).map(|(v, _)| v)
        };
        assert_eq!(
            qth("K9AAA").as_deref(),
            Some("COOK"),
            "this contest's newest"
        );
        assert_eq!(qth("k9aaa").as_deref(), Some("COOK"), "case aside");
        assert_eq!(
            qth("K9BBB").as_deref(),
            Some("KANE"),
            "never another contest's slot"
        );
        assert_eq!(qth("K9CCC").as_deref(), Some("WI"), "a newer state");
        assert_eq!(qth("K9DDD"), None, "another contest's row says nothing");
        assert_eq!(qth("K9ZZZ"), None, "a call never logged");
        let (_, when) = answered(&mut || newest_logged_qth(&rows, "K9AAA", &ids, "QTH")).unwrap();
        assert_eq!(
            when, 1_792_326_840,
            "the contact's own time (2026-10-18 12:34 UTC)"
        );
        // Another contest running: K9AAA's newest row that says is its state.
        let other = ["CA-QSO-PARTY".to_string()];
        let in_ca = answered(&mut || newest_logged_qth(&rows, "K9AAA", &other, "QTH"));
        assert_eq!(in_ca.map(|(v, _)| v).as_deref(), Some("IL"));
    }

    /// ★ The Journey and the contest entry's last-state lookup take a merged contest contact's
    /// state as its upload carries it — the state its exchange sent, read from the store beside
    /// the columns they read — never the one looked up from its call.
    #[test]
    fn the_journey_and_the_last_state_lookup_read_the_exchanges_state() {
        let d = Dir::new("merged-state");
        std::fs::write(d.log(), tempo_core::logbook::adif_header()).unwrap();
        let e = launch(&d);
        let mut r = merged_contest_contact();
        r.call = "K9NQP".into();
        r.state = Some("NJ".into());
        let c = r.contest.as_deref_mut().unwrap();
        c.contest_id = "NAQP-CW".into();
        c.adif = vec![("STATE".into(), "NY".into())];
        r.id = Some(tempo_core::logbook::RecordId::Provisional {
            hash: 77,
            ordinal: 0,
        });
        tempo_core::logbook::sqlite::LogDb::open(&d.db())
            .unwrap()
            .insert(&r, Resolved::default())
            .unwrap();
        let rows = e.lock().unwrap().log_rows();
        let ids = ["IL-QSO-PARTY".to_string()];
        let said = answered(&mut || newest_logged_qth(&rows, "K9NQP", &ids, "QTH"));
        assert_eq!(said.map(|(v, _)| v).as_deref(), Some("NY"));
        let mut journey = Vec::new();
        answered(&mut || {
            journey.clear();
            rows.each_with(JOURNEY, &["STATE"], Scope::All, Order::Log, &mut |q| {
                journey.push(journey_qso(q).state);
                ControlFlow::Continue(())
            })
            .map_err(unreadable)?
            .or_refuse()
        });
        assert_eq!(journey, [Some("NY".to_string())]);
    }

    /// ★ The award fold reads what each record carries beyond its call — its DXCC number, CQ zone
    /// and LoTW's DXCC — from the store as the whole record holds it, and counts by them: the
    /// answer the fold over whole records gives, and each count by value. (The golden runs above
    /// are blind to this: their synthetic contacts carry none of the three.)
    #[test]
    fn the_awards_read_from_the_store_count_by_each_records_own_numbers() {
        let d = Dir::new("by-record");
        std::fs::write(
            d.log(),
            format!("{}{}", tempo_core::logbook::adif_header(), by_record_log()),
        )
        .unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let held = records(&e);
        assert_eq!(held.len(), 6, "premise: every contact converted");
        let kept = awards_kept(&e, &tallies).unwrap();
        assert_eq!(
            *kept,
            awards_for_records(&held, MY_CALL),
            "the store's narrow read must carry what the whole record does"
        );
        // Navassa, Baker & Howland (LoTW's), Johnston, the United States; and the GDR in total.
        assert_eq!((kept.dxcc_confirmed, kept.dxcc_confirmed_total), (4, 5));
        // Zones 8 (K1N's record), 5 (K1B, K3J), 14 (Y2), 3 (K1ABC's record) and 8 (KP4).
        assert_eq!(kept.waz_confirmed, 4);
        // CA, and FL by the KP4 contact's DXCC 291.
        assert_eq!(kept.was.confirmed, 2);
        assert_eq!(
            kept.dxcc_disagreements
                .iter()
                .map(|x| (x.call.as_str(), x.logged_dxcc, x.lotw_dxcc))
                .collect::<Vec<_>>(),
            [("K1B", 291, 20)]
        );
    }

    /// ★ The Stats pane reads the store as the Awards view does: its index is the award fold's own,
    /// kept beside the summary, and equals the index the fold over whole records builds. What a
    /// record carries beyond its call reaches it — the DXCC number (Navassa for K1N's 2015 contact),
    /// and eQSL's Authenticity-Guaranteed mark, a passthrough the store keeps apart from the row,
    /// which confirms a WAZ slot and no DXCC one. Where the station is comes from the app's
    /// licence index; asked again of an unchanged log, the kept index answers without a read.
    #[test]
    fn the_stats_pane_reads_the_store_as_the_awards_do() {
        let d = Dir::new("stats-pane");
        std::fs::write(
            d.log(),
            format!(
                "{}{}{}{}",
                tempo_core::logbook::adif_header(),
                by_record_log(),
                "<CALL:6>JA1XYZ<QSO_DATE:8>20200105<TIME_ON:6>123400<BAND:3>20m<MODE:3>FT8\
                 <EQSL_QSL_RCVD:1>Y<APP_EQSL_AG:1>Y<EOR>\n",
                "<CALL:6>VK2ABC<QSO_DATE:8>20200106<TIME_ON:6>123400<BAND:3>20m<MODE:3>FT8\
                 <EQSL_QSL_RCVD:1>Y<EOR>\n",
            ),
        )
        .unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let held = records(&e);
        assert_eq!(held.len(), 8, "premise: every contact converted");
        assert_eq!(
            *awards_kept(&e, &tallies).unwrap(),
            awards_for_records(&held, MY_CALL),
            "the summary from the store is the whole records' (the needs per band among it)"
        );
        let whole = {
            let mut awards = propagation::Awards::with_call_stats();
            for q in &held {
                award_add(&mut awards, q);
            }
            awards.take_call_stats().unwrap()
        };
        let from_store = call_stats_kept(&e, &tallies).unwrap();
        let state_of = |c: &str| (c == "W6XYZ").then(|| "CA".to_string());
        for call in [
            "KP1AA", "K1N", "KH1AA", "W6XYZ", "W4XYZ", "JA1ABC", "VK2XYZ", "DL1ABC",
        ] {
            assert_eq!(
                from_store.stats_for(call, "20m", "FT8", state_of(call).as_deref()),
                whole.stats_for(call, "20m", "FT8", state_of(call).as_deref()),
                "{call}"
            );
        }
        let ask = |call: &str| call_stats_for(&e, &tallies, call, "20m", "CW", state_of).unwrap();
        use propagation::callstats::Slot;
        let (navassa, japan, australia, w6) =
            (ask("KP1AA"), ask("JA1ABC"), ask("VK2XYZ"), ask(" w6xyz "));
        let dxcc = navassa.dxcc.as_ref().unwrap();
        assert_eq!(
            (dxcc.label.as_str(), dxcc.rows[0][5], navassa.new_one),
            ("Navassa Island", Slot::Confirmed, false)
        );
        // K1N's contact came back from LoTW: the country's row and the call's say so.
        assert_eq!(
            (navassa.country_qsl.unwrap().lotw, ask("K1N").call_qsl.lotw),
            (true, true)
        );
        // 20 m (column 5), FT8 (the Digital row): the AG card confirms the zone and not the entity.
        assert_eq!(
            (
                japan.dxcc.as_ref().unwrap().rows[2][5],
                japan.waz.as_ref().unwrap().label.as_str(),
                japan.waz.as_ref().unwrap().rows[2][5],
                japan.country_qsl.unwrap().eqsl_ag,
            ),
            (Slot::Worked, "25", Slot::Confirmed, true)
        );
        assert_eq!(
            (
                australia.waz.as_ref().unwrap().rows[2][5],
                australia.country_qsl.unwrap().eqsl_ag
            ),
            (Slot::Worked, false)
        );
        // K1ABC, logged in California (CQZ 3), fills California's WAS slot; the state is the
        // licence index's for the call typed.
        let ca = w6
            .was
            .as_ref()
            .expect("the licence index places W6XYZ in California");
        assert_eq!(
            (w6.call.as_str(), ca.label.as_str(), ca.rows[0][5]),
            ("W6XYZ", "CA", Slot::Confirmed)
        );
        assert!(ask("W4XYZ").was.is_none() && ask("W4XYZ").was_family);
        assert!(
            Arc::ptr_eq(&from_store, &call_stats_kept(&e, &tallies).unwrap()),
            "an unchanged log is not folded again"
        );
    }

    /// [`by_record_log`] and two contacts whose records carry no number, which count by their
    /// calls: a Sicilian call (Italy, as the ARRL credits Sicily) and a Connecticut one.
    fn by_record_log_and_calls() -> String {
        format!(
            "{}{}{}",
            by_record_log(),
            "<CALL:6>IT9ABC<QSO_DATE:8>20200103<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
             <LOTW_QSL_RCVD:1>Y<EOR>\n",
            "<CALL:4>W1AW<QSO_DATE:8>20200104<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
             <LOTW_QSL_RCVD:1>Y<STATE:2>CT<EOR>\n",
        )
    }

    /// A JTTY contact counts as JTTY on the Needed board under either spelling a log may hold it
    /// in: an older log's bare `MODE=JTTY`, and ADIF 3.1.8's `MODE=MFSK SUBMODE=JTTY`, which is
    /// how Nexus writes one. A station heard in JTTY on a band and entity each log has worked in
    /// JTTY is no new mode; heard in JS8, which neither log holds, it is.
    #[test]
    fn a_jtty_contact_counts_as_jtty_under_either_spelling() {
        for spelling in ["<MODE:4>JTTY", "<MODE:4>MFSK<SUBMODE:4>JTTY"] {
            let d = Dir::new("needs-jtty");
            std::fs::write(
                d.log(),
                format!(
                    "{}<CALL:5>K1ABC<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m{spelling}\
                     <LOTW_QSL_RCVD:1>Y<EOR>\n",
                    tempo_core::logbook::adif_header(),
                ),
            )
            .unwrap();
            let e = launch(&d);
            assert_eq!(
                records(&e)[0].mode,
                "JTTY",
                "premise: {spelling} is stored as JTTY"
            );
            let needs = needs_kept(&e, &LogTallies::default()).unwrap();
            let new_mode = |mode: &str| {
                propagation::needalert::score_slots(
                    "K1XYZ",
                    "20m",
                    mode,
                    None,
                    None,
                    &*needs,
                    &needs.slots(),
                )
                .is_some_and(|a| a.tags.contains(&propagation::NeedTag::NewMode))
            };
            assert!(!new_mode("JTTY"), "{spelling}: JTTY is worked");
            assert!(new_mode("JS8"), "{spelling}: JS8 is not");
        }
    }

    /// ★ The Needed board judges what the log holds by the entity, zone and state the awards
    /// count each contact toward — its record's DXCC number (LoTW's first) and CQ zone, else its
    /// call — read from the store as the whole record holds them. Each station below is heard on
    /// 20 m CW, where every contact in the log was made and confirmed by LoTW. A spot has no
    /// record, so it is placed by its call alone.
    #[test]
    fn the_needed_board_counts_each_contact_as_the_awards_do() {
        let d = Dir::new("needs-by-record");
        std::fs::write(
            d.log(),
            format!(
                "{}{}",
                tempo_core::logbook::adif_header(),
                by_record_log_and_calls()
            ),
        )
        .unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let held = records(&e);
        assert_eq!(held.len(), 8, "premise: every contact converted");
        let needs = needs_kept(&e, &tallies).unwrap();
        assert_eq!(
            observed(&needs),
            observed(&needs_old(&held)),
            "the store's narrow read must carry what the whole record does"
        );
        for (call, entity) in [
            ("KP1AA", "Navassa Island"),
            ("KH1AA", "Baker & Howland Islands"),
            ("I1ABC", "Italy"),
            ("DL1ABC", "Fed. Rep. of Germany"),
            ("K1N", "United States"),
        ] {
            assert_eq!(
                propagation::dxcc::resolve(call).map(|i| i.entity),
                Some(entity),
                "premise: cty.dat places {call}"
            );
        }
        let heard = |call: &str, state: Option<&str>| propagation::Heard {
            call: call.into(),
            band: "20m".into(),
            mode: "CW".into(),
            freq_mhz: None,
            admitted_at: None,
            evidence: None,
            grid: None,
            us_state: state.map(|s| (s.to_string(), propagation::StateSource::License)),
        };
        let spots = [
            // Navassa: K1N's record, zone 8 with it.
            heard("KP1AA", None),
            // Baker & Howland: LoTW's number on K1B, over the 291 the log holds. The zone is
            // the call's (K1B's record carries none), as WAZ counts it: 5, so 31 is new.
            heard("KH1AA", None),
            // Italy: the Sicilian contact, by its call.
            heard("I1ABC", None),
            // Germany: Y24UK was the German Democratic Republic, deleted — no current entity.
            heard("DL1ABC", None),
            // Florida: KP4ABC's record says the United States.
            heard("K4ABC", Some("FL")),
            // Today's K1N, placed by its call: the United States, never Navassa.
            heard("K1N", None),
            // Control: Connecticut, by W1AW's call, which carries no number.
            heard("W1XYZ", Some("CT")),
        ];
        let mut board: Vec<(String, String, Vec<propagation::NeedTag>)> =
            propagation::rank_needs(&spots, &*needs, &needs.slots())
                .into_iter()
                .map(|a| (a.call, a.entity, a.tags))
                .collect();
        board.sort_by(|a, b| a.0.cmp(&b.0));
        use propagation::NeedTag::{NewEntity, NewZone};
        assert_eq!(
            board,
            [
                (
                    "DL1ABC".into(),
                    "Fed. Rep. of Germany".into(),
                    vec![NewEntity]
                ),
                (
                    "KH1AA".into(),
                    "Baker & Howland Islands".into(),
                    vec![NewZone]
                ),
            ]
        );
        assert!(
            !needs
                .worked_entity_names()
                .contains("GERMAN DEMOCRATIC REPUBLIC"),
            "a deleted entity is never a need"
        );
    }

    /// ★ The log statistics' geography counts each contact by the entity and zone the awards
    /// count it toward, read from the store: DX or domestic against the operator's own entity
    /// (the United States), each continent's entities, the zones.
    #[test]
    fn the_log_statistics_count_each_contact_as_the_awards_do() {
        let d = Dir::new("stats-by-record");
        std::fs::write(
            d.log(),
            format!(
                "{}{}",
                tempo_core::logbook::adif_header(),
                by_record_log_and_calls()
            ),
        )
        .unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let stats = log_stats(&e, &tallies).unwrap();
        assert_eq!((stats.total, stats.resolved), (8, 8));
        // Domestic: K1ABC, KP4ABC (its record's 291) and W1AW. DX: Navassa, Baker & Howland,
        // Johnston, the German Democratic Republic, Italy.
        assert_eq!((stats.domestic, stats.dx), (3, 5));
        let continents: Vec<(&str, usize, usize)> = stats
            .by_continent
            .iter()
            .map(|c| (c.continent.as_str(), c.qsos, c.entities))
            .collect();
        assert_eq!(continents, [("NA", 6, 4), ("EU", 2, 2)]);
        let zones: Vec<(u8, usize)> = stats.by_zone.iter().map(|z| (z.zone, z.qsos)).collect();
        // K1N's record says 8 and K1ABC's 3; the rest are their calls' zones.
        assert_eq!(zones, [(3, 1), (5, 3), (8, 2), (14, 1), (15, 1)]);
    }

    /// ★ WWFF and What-to-QSL read what the store holds: a WWFF reference by `WWFF_REF` or by
    /// `SIG`=WWFF, and the QSL route — the store's narrow read answers as the whole records do.
    #[test]
    fn the_store_reads_what_wwff_and_what_to_qsl_count() {
        let d = Dir::new("awards-more");
        std::fs::write(
            d.log(),
            format!(
                "{}{}{}{}",
                tempo_core::logbook::adif_header(),
                "<CALL:6>DL1ABC<QSO_DATE:8>20260101<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
                 <WWFF_REF:9>DLFF-0001<QSL_VIA:6>DL1QSL<EOR>\n",
                "<CALL:4>W1AW<QSO_DATE:8>20260102<TIME_ON:6>120000<BAND:3>40m<MODE:3>SSB\
                 <SIG:4>WWFF<SIG_INFO:8>KFF-0001<LOTW_QSL_RCVD:1>Y<EOR>\n",
                "<CALL:6>JA1ABC<QSO_DATE:8>20260103<TIME_ON:6>120000<BAND:3>15m<MODE:3>FT8\
                 <QSL_VIA:6>JA1QSL<EOR>\n",
            ),
        )
        .unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let summary = awards_kept(&e, &tallies).unwrap();
        assert_eq!((summary.wwff.worked, summary.wwff.entities), (2, 2));
        let wants = what_to_qsl_kept(&e, &tallies).unwrap();
        let routes: Vec<(&str, Option<&str>)> = wants
            .iter()
            .map(|w| (w.call.as_str(), w.qsl_via.as_deref()))
            .collect();
        // Both are new ones; Germany's contact fills one more slot (its WAE country).
        assert_eq!(
            routes,
            [("DL1ABC", Some("DL1QSL")), ("JA1ABC", Some("JA1QSL"))]
        );
        let mut whole = propagation::Awards::new();
        whole.set_home_call(MY_CALL);
        for q in records(&e) {
            award_add(&mut whole, &q);
        }
        assert_eq!(
            *wants,
            whole.what_to_qsl(),
            "the store and the whole records agree"
        );
        assert_eq!(*dxcc_chart_kept(&e, &tallies).unwrap(), whole.dxcc_chart());
        assert_eq!(*summary, whole.summary());
    }

    /// ★ A custom award counts from the whole records: a field award reads a field the log
    /// keeps verbatim (`CNTY`), a reference award a park, and the standing is folded again when
    /// the definitions change.
    #[test]
    fn custom_awards_count_from_the_whole_records() {
        use propagation::custom::{CustomAward, CustomKind};
        let d = Dir::new("custom-awards");
        std::fs::write(
            d.log(),
            format!(
                "{}{}{}",
                tempo_core::logbook::adif_header(),
                "<CALL:4>W1AW<QSO_DATE:8>20260101<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
                 <CNTY:11>CT,HARTFORD<LOTW_QSL_RCVD:1>Y<EOR>\n",
                "<CALL:5>K1ABC<QSO_DATE:8>20260102<TIME_ON:6>120000<BAND:3>40m<MODE:3>SSB\
                 <SIG:4>POTA<SIG_INFO:7>US-1234<EOR>\n",
            ),
        )
        .unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let counties = CustomAward {
            name: "Counties".into(),
            kind: CustomKind::Field,
            field: "CNTY".into(),
            targets: vec!["CT,Hartford".into(), "MA,Essex".into()],
            ..CustomAward::default()
        };
        let parks = CustomAward {
            name: "Parks".into(),
            kind: CustomKind::Reference,
            targets: vec!["US-1234".into()],
            ..CustomAward::default()
        };
        let p = custom_awards_kept(&e, &tallies, vec![counties.clone()]).unwrap();
        assert_eq!((p[0].worked, p[0].confirmed, p[0].total), (1, 1, 2));
        let p = custom_awards_kept(&e, &tallies, vec![counties, parks]).unwrap();
        assert_eq!(p.len(), 2, "new definitions fold again");
        assert_eq!((p[1].worked, p[1].confirmed), (1, 0));
        assert!(custom_awards_kept(&e, &tallies, Vec::new())
            .unwrap()
            .is_empty());
    }

    /// A contest contact as the merge puts it in the Logbook: what the other station sent stays
    /// in the contest's own columns — the zone in CQ WW, the county, section and class elsewhere.
    fn merged_contest_contact() -> QsoRecord {
        let mut log = tempo_core::logbook::Logbook::new();
        log.import_adif(
            "<CALL:4>W1AW<QSO_DATE:8>20261024<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <LOTW_QSL_RCVD:1>Y<EOR>\n",
        );
        let mut r = QsoRecord::clone(&log.records()[0]);
        r.contest = Some(Box::new(tempo_core::logbook::ContestFields {
            contest_id: "CQ-WW-CW".into(),
            adif: vec![
                ("MY_CQ_ZONE".into(), "4".into()),
                ("CQZ".into(), "3".into()),
                ("CNTY".into(), "CA,SANTA CLARA".into()),
                ("ARRL_SECT".into(), "SCV".into()),
                ("CLASS".into(), "2A".into()),
            ],
            ..Default::default()
        }));
        r
    }

    /// ★ The US Counties award counts the county a merged contest contact's exchange carries, read
    /// the way the award fold reads the log (the store's narrow read) and as a whole record, and
    /// the award status reads it the same: one county worked, not confirmed (LoTW alone).
    #[test]
    fn the_counties_award_counts_a_merged_contest_contacts_county() {
        let rec = merged_contest_contact();
        let mut db = tempo_core::logbook::sqlite::LogDb::open_in_memory().unwrap();
        db.insert(&rec, tempo_core::logbook::sqlite::Resolved::default())
            .unwrap();
        let mut narrow = propagation::Awards::new();
        db.each_narrow_with(AWARDS, AWARDS_EXTRAS, Scope::All, Order::Log, &mut |q| {
            award_add(&mut narrow, q);
            ControlFlow::Continue(())
        })
        .unwrap();
        let whole = awards_for_records(&db.load_all().unwrap(), "KD9TAW").usaca;
        let narrow = narrow.summary().usaca;
        assert_eq!(
            (narrow.worked, narrow.confirmed, narrow.unplaced),
            (1, 0, 0)
        );
        assert_eq!(narrow, whole);
        let id = "x";
        let c = status_contact(&rec, id);
        assert_eq!(
            propagation::award_status::entity_of(propagation::award_status::AwardKind::Usaca, &c)
                .as_deref(),
            Some("CA,Santa Clara")
        );
    }

    /// ★ WAZ counts the zone the other station SENT in a merged CQ WW contact, read the way the
    /// award folds read the log — the store's narrow read — and the way a whole record is read.
    /// A W1 call operating from California sends zone 3; the country file's zone for W1 is 5,
    /// which is what the contact used to be credited with.
    #[test]
    fn waz_counts_the_zone_a_merged_contest_contact_was_sent() {
        let rec = merged_contest_contact();
        let mut db = tempo_core::logbook::sqlite::LogDb::open_in_memory().unwrap();
        db.insert(&rec, tempo_core::logbook::sqlite::Resolved::default())
            .unwrap();
        let zone = |q: &QsoRecord| {
            propagation::awards::counted_zone(
                propagation::dxcc::resolve(&q.call).as_ref(),
                &award_fields(q),
            )
        };
        let mut read = Vec::new();
        for (narrow, extras) in [(NEEDS, AWARD_EXTRAS), (AWARDS, AWARDS_EXTRAS)] {
            db.each_narrow_with(narrow, extras, Scope::All, Order::Log, &mut |q| {
                read.push(zone(q));
                ControlFlow::Continue(())
            })
            .unwrap();
        }
        read.push(zone(&db.load_all().unwrap()[0]));
        assert_eq!(read, [Some(3); 3]);
    }

    /// ★ A custom award on a field a contest exchange carries — the county, section or class —
    /// counts the merged contest contacts that carry it.
    #[test]
    fn a_custom_award_counts_a_merged_contest_contacts_exchange() {
        use propagation::custom::{CustomAward, CustomAwards, CustomKind};
        let on = |field: &str, target: &str| CustomAward {
            name: field.into(),
            kind: CustomKind::Field,
            field: field.into(),
            targets: vec![target.into()],
            ..CustomAward::default()
        };
        let defs = [
            on("CNTY", "CA,Santa Clara"),
            on("ARRL_SECT", "SCV"),
            on("CLASS", "2A"),
            on("CQZ", "3"),
        ];
        let mut fold = CustomAwards::new(&defs);
        custom_add(&mut fold, &merged_contest_contact());
        let worked: Vec<(String, usize)> = fold
            .progress()
            .into_iter()
            .map(|p| (p.name, p.worked))
            .collect();
        assert_eq!(
            worked,
            [
                ("CNTY".to_string(), 1),
                ("ARRL_SECT".to_string(), 1),
                ("CLASS".to_string(), 1),
                ("CQZ".to_string(), 1)
            ]
        );
    }

    /// ★ WAS, the WAS needs and a custom award on STATE count the state a merged contest
    /// contact's exchange carries — the state its upload carries — and not the resolver's guess
    /// beside it; and once the operator edits State, all of them count the edit. Read the way
    /// the folds read the log (the store's narrow reads) and as a whole record.
    #[test]
    fn every_state_reader_counts_the_state_the_upload_carries() {
        use propagation::custom::{CustomAward, CustomAwards, CustomKind};
        let merged = |own: &str, sent: &str| {
            let mut r = merged_contest_contact();
            r.state = Some(own.into());
            r.contest
                .as_mut()
                .unwrap()
                .adif
                .push(("STATE".into(), sent.into()));
            r
        };
        let counted = |r: &QsoRecord| {
            let upload = tempo_core::logbook::adif_record(r);
            let was = awards_for_records(&[r], "KD9TAW").was;
            let mut db = tempo_core::logbook::sqlite::LogDb::open_in_memory().unwrap();
            db.insert(r, Resolved::default()).unwrap();
            let mut narrow_was = Vec::new();
            let mut awards = propagation::Awards::new();
            db.each_narrow_with(AWARDS, AWARDS_EXTRAS, Scope::All, Order::Log, &mut |q| {
                award_add(&mut awards, q);
                ControlFlow::Continue(())
            })
            .unwrap();
            narrow_was.push(awards.summary().was.needed);
            let mut needs = propagation::LogNeeds::new();
            db.each_narrow_with(NEEDS, AWARD_EXTRAS, Scope::All, Order::Log, &mut |q| {
                needs_add(&mut needs, q);
                ControlFlow::Continue(())
            })
            .unwrap();
            let defs = [CustomAward {
                name: "states".into(),
                kind: CustomKind::Field,
                field: "STATE".into(),
                targets: vec!["NY".into(), "NJ".into(), "PA".into()],
                ..CustomAward::default()
            }];
            let mut custom = CustomAwards::new(&defs);
            custom_add(&mut custom, r);
            (
                upload,
                was.needed,
                narrow_was.remove(0),
                needs
                    .worked_states()
                    .iter()
                    .map(|(state, _)| state.clone())
                    .collect::<Vec<_>>(),
                custom.progress()[0].worked,
            )
        };
        // One state each, confirmed: WAS needs every state but the one it counted.
        let check = |r: &QsoRecord, want: &str| {
            let (upload, was, narrow, needs, custom) = counted(r);
            assert_eq!(upload.matches("<STATE:").count(), 1, "{upload}");
            assert!(upload.contains(&format!("<STATE:2>{want}")), "{upload}");
            for needed in [&was, &narrow] {
                assert_eq!(needed.len(), 49, "{needed:?}");
                assert!(!needed.iter().any(|s| s == want), "{want}: {needed:?}");
            }
            assert_eq!(needs, [want], "the WAS needs");
            assert_eq!(custom, 1);
        };
        // Unedited: the resolver said NJ, the station sent NY.
        check(&merged("NJ", "NY"), "NY");
        // The operator's edit, as the Logbook's edit writes it: the field and the exchange.
        check(&merged("PA", "PA"), "PA");
    }

    /// ★ IOTA credits LoTW-matched contacts and QSL cards, never eQSL: a group confirmed only by
    /// LoTW counts, one confirmed only by eQSL does not, and a card still does. The card count
    /// stays beside it.
    #[test]
    fn iota_counts_lotw_and_card_confirmations_not_eqsl() {
        let row = |call: &str, iota: &str, rcvd: &str| {
            parse_one(&format!(
                "<CALL:{}>{call}<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20260101<TIME_ON:6>120000\
                 <IOTA:6>{iota}{rcvd}<EOR>",
                call.len()
            ))
        };
        let records = [
            row("VE1AAA", "NA-010", "<LOTW_QSL_RCVD:1>Y"),
            row("VE1BBB", "NA-011", "<EQSL_QSL_RCVD:1>Y"),
            row("VE1CCC", "NA-012", "<QSL_RCVD:1>Y"),
        ];
        let s = awards_for_records(&records, "W1AW");
        assert_eq!(s.iota.worked, 3, "three groups worked");
        assert_eq!(
            s.iota.confirmed, 2,
            "the LoTW and the card groups count; the eQSL one does not"
        );
        assert_eq!(s.iota.card_confirmed, 1, "one group on a card");
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
            !worked(&needs_kept(&e, &tallies).unwrap()),
            "premise: {helena} is new"
        );

        let needs = asked_straight_after(&d, &e, contact("ZD7AA", NOW as u64), || {
            needs_kept(&e, &tallies)
        });
        assert!(worked(&needs), "the needs model has the contact");

        let awards = asked_straight_after(&d, &e, contact("ZD7AB", NOW as u64), || {
            awards_kept(&e, &tallies)
        });
        assert_eq!(*awards, awards_old(&records(&e), MY_CALL), "the awards");

        let before = log_stats(&e, &tallies).unwrap().total;
        let stats = asked_straight_after(&d, &e, contact("ZD7AC", NOW as u64), || {
            log_stats(&e, &tallies)
        });
        assert_eq!(stats.total, before + 1, "the statistics count it");

        let journey = asked_straight_after(&d, &e, contact("ZD7AD", NOW as u64), || {
            journey_kept(&e, &tallies).map(|model| model.summary(NOW).total_qsos)
        });
        assert_eq!(journey as usize, records(&e).len(), "the Journey counts it");

        let mut park = contact("K9PRK", NOW as u64);
        park.ota.their_program = Some("POTA".into());
        park.ota.their_ref = Some("US-9999".into());
        let hunted = asked_straight_after(&d, &e, park, || {
            let rows = e.lock().unwrap().log_rows();
            hunted_today(&rows, NOW)
        });
        assert!(
            !hunted.needed("US-9999", "K9PRK", NOW),
            "today's park is hunted"
        );

        let ft8_this_hour = |t: &LogTimeline| {
            t.modes
                .iter()
                .find(|m| m.label == "FT8")
                .map_or(0, |m| m.counts[23])
        };
        let rows = e.lock().unwrap().log_rows();
        let before = ft8_this_hour(&log_timeline(&rows, RANGES[0], NOW).unwrap());
        let timeline = asked_straight_after(&d, &e, contact("K9TML", NOW as u64), || {
            let rows = e.lock().unwrap().log_rows();
            log_timeline(&rows, RANGES[0], NOW)
        });
        assert_eq!(
            ft8_this_hour(&timeline),
            before + 1,
            "the timeline counts it"
        );

        let grid = asked_straight_after(&d, &e, contact("K9GRD", NOW as u64), || {
            let rows = e.lock().unwrap().log_rows();
            newest_logged_grid(&rows, "K9GRD")
        });
        assert_eq!(grid.as_deref(), Some("RH91"), "its grid is found");

        let mut stamped = contact("K9STP", NOW as u64);
        stamped.upload.qrz = Some(tempo_core::logbook::UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_999_999_999,
            detail: None,
        });
        let health = asked_straight_after(&d, &e, stamped, || upload_health_kept(&e, &tallies));
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
            oldest_pending_lotw_date(&rows)
        });
        assert_eq!(
            oldest.as_deref(),
            Some("2001-09-09"),
            "its pending upload is the oldest"
        );

        let seeded = asked_straight_after(&d, &e, contact("K9SED/P", NOW as u64), || {
            let rows = e.lock().unwrap().log_rows();
            compound_calls_to_seed(&rows)
        });
        assert_eq!(
            seeded.first().map(String::as_str),
            Some("K9SED/P"),
            "newest first"
        );

        let diagnosed = asked_straight_after(&d, &e, contact("K9DGN", NOW as u64), || {
            let inputs = e.lock().unwrap().confirmation_diagnostics_inputs();
            inputs.diagnose(NOW, test_country).map(|(_, n)| n)
        });
        assert_eq!(
            diagnosed,
            records(&e).len(),
            "every contact diagnosed, the new one too"
        );
        settle(&e);
    }

    /// ★ P4 WHEN THE WRITER IS BEHIND: every pass asked while the contact logged before it is
    /// still on its way to disk — held there for the whole of `READ_WAIT` — is REFUSED, never
    /// answered without the contact; asked again once the store holds it, each answers with it.
    /// What a slow disk did before: the pass answered from the store as it stood, and the
    /// operator saw needs and statistics that left out the contact just logged.
    #[test]
    fn a_fold_whose_wait_runs_out_is_refused_never_answered_without_the_contact() {
        let d = Dir::new("p4-behind");
        std::fs::write(d.log(), synthetic_log(200, 0xFEED)).unwrap();
        let e = launch(&d);
        let tallies = LogTallies::default();
        let before = log_stats(&e, &tallies).unwrap().total;
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(contact("ZD7AA", NOW as u64));
        let rows = || e.lock().unwrap().log_rows();
        // Every pass at once, each saying what it answered: they all wait on the one held write.
        let asked: Vec<(&str, Result<String, String>)> = std::thread::scope(|s| {
            let passes = [
                (
                    "the needs model",
                    s.spawn(|| {
                        needs_kept(&e, &tallies)
                            .map(|n| format!("{} entities", n.worked_entity_names().len()))
                    }),
                ),
                (
                    "the awards",
                    s.spawn(|| awards_kept(&e, &tallies).map(|a| format!("{} contacts", a.qsos))),
                ),
                (
                    "the statistics",
                    s.spawn(|| log_stats(&e, &tallies).map(|st| format!("{} contacts", st.total))),
                ),
                (
                    "the Journey",
                    s.spawn(|| {
                        journey_kept(&e, &tallies)
                            .map(|j| format!("{} contacts", j.summary(NOW).total_qsos))
                    }),
                ),
                (
                    "today's parks",
                    s.spawn(|| hunted_today(&rows(), NOW).map(|_| "today's parks".to_string())),
                ),
                (
                    "the timeline",
                    s.spawn(|| {
                        log_timeline(&rows(), TimelineRange::PastDay, NOW)
                            .map(|t| format!("{} modes", t.modes.len()))
                    }),
                ),
                (
                    "a grid",
                    s.spawn(|| newest_logged_grid(&rows(), "ZD7AA").map(|g| format!("{g:?}"))),
                ),
                (
                    "the upload health",
                    s.spawn(|| upload_health_kept(&e, &tallies).map(|_| "the health".to_string())),
                ),
                (
                    "the oldest pending upload",
                    s.spawn(|| oldest_pending_lotw_date(&rows()).map(|o| format!("{o:?}"))),
                ),
                (
                    "the compound calls",
                    s.spawn(|| compound_calls_to_seed(&rows()).map(|c| format!("{c:?}"))),
                ),
                (
                    "the diagnosis",
                    s.spawn(|| {
                        let inputs = e.lock().unwrap().confirmation_diagnostics_inputs();
                        inputs
                            .diagnose(NOW, test_country)
                            .map(|(_, n)| format!("{n} contacts"))
                    }),
                ),
            ];
            passes
                .map(|(pass, asked)| (pass, asked.join().unwrap()))
                .into()
        });
        drop(hold);
        let answered: Vec<String> = asked
            .iter()
            .filter_map(|(pass, a)| a.as_ref().ok().map(|what| format!("{pass} ({what})")))
            .collect();
        assert!(
            answered.is_empty(),
            "answered without the contact: {}",
            answered.join(", ")
        );
        for (pass, answer) in &asked {
            let why = answer.as_ref().unwrap_err();
            assert!(why.starts_with(NOT_ANSWERED), "{pass}: {why}");
        }
        e.caught_up();
        assert_eq!(
            log_stats(&e, &tallies).unwrap().total,
            before + 1,
            "asked again, the statistics count it"
        );
        assert_every_fold_agrees(&e, &tallies, "asked again once the store holds the contact");
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
        // stands if the writer is past READ_WAIT, and then keeps nothing
        // ([`StoredLog::caught_up`]).
        e.caught_up();
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
        e.caught_up();
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

    /// The Statistics timeline timed on a 100,000-contact log in the store —
    /// `cargo test --manifest-path src-tauri/Cargo.toml --lib --features radio --release --
    /// --ignored --nocapture timeline_bench` (`TIMELINE_BENCH_ROWS` sets the size). The contacts
    /// are spread evenly over the year before [`NOW`], so the year's window reads every one of
    /// them — the most any window can read — the 30 days about one in twelve, the day one in 365.
    #[test]
    #[ignore = "a release-build bench, run by hand"]
    fn timeline_bench() {
        let n: usize = std::env::var("TIMELINE_BENCH_ROWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100_000);
        let modes = [
            "FT8", "FT8", "FT8", "CW", "SSB", "FT4", "RTTY", "USB", "FM", "PSK31",
        ];
        let mut adif = tempo_core::logbook::adif_header();
        for i in 0..n {
            let when = NOW as u64 - i as u64 * 365 * 86_400 / n as u64;
            let (y, m, day, hh, mm, ss) = tempo_core::logbook::datetime_utc(when);
            let call = format!("K{}AB{}", i % 10, i % 1000);
            let mode = modes[i % modes.len()];
            adif.push_str(&format!(
                "<CALL:{}>{call}<BAND:3>20m<MODE:{}>{mode}<QSO_DATE:8>{y:04}{m:02}{day:02}\
                 <TIME_ON:6>{hh:02}{mm:02}{ss:02}<EOR>\n",
                call.len(),
                mode.len()
            ));
        }
        let d = Dir::new("timeline-bench");
        std::fs::write(d.log(), adif).unwrap();
        let e = launch(&d);
        settle(&e);
        let rows = e.lock().unwrap().log_rows();
        for range in RANGES {
            let mut took = Vec::new();
            let mut counted = 0;
            for _ in 0..7 {
                let t = std::time::Instant::now();
                let timeline = log_timeline(&rows, range, NOW).expect("reads");
                took.push(t.elapsed().as_secs_f64() * 1000.0);
                counted = timeline.modes.iter().flat_map(|m| &m.counts).sum::<usize>();
            }
            let first = took[0];
            took.sort_by(f64::total_cmp);
            println!(
                "TIMELINE_BENCH rows={n} {range:?}: {counted} contacts in the window, first {first:.1} \
                 ms, median of 7 {:.1} ms",
                took[3]
            );
        }
    }
}
