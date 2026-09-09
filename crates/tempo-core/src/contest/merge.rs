//! The general-log seam: a contest log's rows become `QsoRecord`s, once.
//!
//! ⭐ **This is the ONLY path by which a contest contact becomes a general-log record,
//! and that is why it needed a ruling before it could be written.** A Field Day contact
//! never reaches the general upload queue — `fd_log_contact` writes the contest log and
//! nothing else, pinned by a test that says so — and the reason is that the general log
//! path enqueues every record it writes to every connector. Merging 800 contest rows
//! through it would not enqueue 800: both sinks are bounded ring buffers that drop from
//! the front at 256, so the morning's contacts would be discarded silently. A slow
//! upload is a chore; a queue that quietly eats two-thirds of a contest is a data-loss
//! report.
//!
//! So the merge here **writes and does not enqueue**. It hands its caller the records it
//! wrote; whether any of them are queued for upload is the session's
//! [`UploadPolicy`](super::session::UploadPolicy) — per session, default OFF — and the
//! decision belongs to the one caller that owns a connector queue, not to this module.
//!
//! ⭐ **Idempotence, and the mechanism rather than the promise.** Every row carries
//! `APP_NEXUS_QID` = `"<session>:<posid>:<seq>"`. `(posid, seq)` is already the
//! club-sync row identity — monotonic per position and restored across restart — so
//! nothing new had to be invented to make re-running the merge safe. A row whose qid is
//! already in the general log is skipped. This holds **across a restart** only because
//! the ADIF parser consumes the tag back into the record rather than leaving it in
//! `extra`; a merge-twice test that never drops the `Logbook` would pass with no reader
//! at all.
use super::render::sent_exchange;
use super::spec::FieldKind;
use crate::fieldday::{FieldDayLog, LoggedQso};
use crate::logbook::{ContestFields, Logbook, QslRcvd, QslSent, QsoRecord, UploadState};

/// What one merge did (§3.2).
#[derive(Debug, Clone, Default)]
pub struct MergeReport {
    /// The records actually written, in log order — handed back rather than counted so
    /// the caller can persist them and, if the session says so, queue them, without
    /// re-deriving which rows were new.
    pub written: Vec<QsoRecord>,
    /// Rows whose qid was already in the general log. On a second run of the same
    /// merge this is every row, and `written` is empty.
    pub already: usize,
    /// Rows with no stable identity — see [`merge_into_general`]. Reported, never
    /// silently dropped and never silently duplicated.
    pub refused: usize,
}

impl MergeReport {
    /// How many rows this merge wrote.
    pub fn added(&self) -> usize {
        self.written.len()
    }
}

/// Merge a contest log into the general log, skipping every row already there.
///
/// ⚠️ **A free function rather than a method on [`ContestSession`](super::ContestSession),
/// and the deviation from §3.2's sketch is mechanical.** The log already carries its own
/// session (`FieldDayLog.session`), so a method taking both a `&self` session and a log
/// would admit a caller that passes a session the log does not belong to — and the qid
/// this whole function turns on is built from the session id. One argument, one session,
/// no way to disagree.
///
/// `posid` is the local position's machine id (the club-sync half of a row identity). It
/// lives in settings, above this crate, so it is passed in.
///
/// **Rows with `seq == 0` are refused, not merged.** Every such row would build the same
/// qid, so merging them is either a silent duplicate on the second run or a silent
/// collapse on the first; refusing and reporting is the only honest third answer. Live
/// logging stamps a sequence and a journal restore backfills one, so this is the
/// defensive path for a journal that has been hand-edited or truncated — which is a file
/// on disk, and this build does not get to assume it is well formed.
pub fn merge_into_general(log: &FieldDayLog, posid: &str, into: &mut Logbook) -> MergeReport {
    let mut report = MergeReport::default();
    let mut seen: std::collections::HashSet<String> = into
        .records()
        .iter()
        .filter_map(|r| r.contest.as_deref())
        .map(|c| c.qid.clone())
        .filter(|q| !q.is_empty())
        .collect();
    for q in log.qsos() {
        if q.seq == 0 {
            report.refused += 1;
            continue;
        }
        let qid = format!("{}:{}:{}", log.session.id, posid, q.seq);
        if seen.contains(&qid) {
            report.already += 1;
            continue;
        }
        // A qid written by THIS run counts as seen too — the set is what the function
        // means by "already there", and a set that only knew about the log it started
        // with would be true of the first duplicate and false of the second.
        seen.insert(qid.clone());
        let rec = record_for(log, q, qid);
        into.add(rec.clone());
        report.written.push(rec);
    }
    report
}

/// One contest row as a general-log record.
fn record_for(log: &FieldDayLog, q: &LoggedQso, qid: String) -> QsoRecord {
    let spec = log.session.exchange;
    // The SENT side comes from the one renderer, which takes a ROW — never from the
    // session, which holds only what is being sent right now.
    let sent = sent_exchange(q, spec);
    let contest = ContestFields {
        session: log.session.id.clone(),
        contest_id: log.session.contest_id.clone(),
        stx: serial_of(&sent, spec),
        stx_string: joined(sent.iter().map(|v| v.raw.as_str())),
        srx: serial_of(&q.rx, spec),
        srx_string: joined(q.rx.iter().map(|v| v.raw.as_str())),
        sent: pairs(&sent),
        rcvd: pairs(&q.rx),
        qid,
    };
    QsoRecord {
        call: q.call.clone(),
        grid: None,
        country: q.entity.clone(),
        state: None,
        band: q.band.clone(),
        // No frequency: a contest row records the band, and the ADIF writer omits FREQ
        // rather than emit a zero that gets the whole record rejected on import.
        freq_mhz: 0.0,
        freq_rx_mhz: None,
        mode: q.recorded_mode().to_string(),
        // A contest exchange carries no signal report unless its own spec declares one,
        // and neither Field Day event's does. Inventing 599 would be a claim about the
        // air that nobody made.
        rst_sent: None,
        rst_rcvd: None,
        name: None,
        qth: None,
        comment: None,
        notes: None,
        tx_power: None,
        when_unix: q.when_unix,
        // A legacy row with no stamp keeps the placeholder rather than a fabricated
        // midnight — the same rule the ADIF importer applies.
        time_known: q.when_unix > 0,
        time_off_unix: None,
        confirmed: false,
        award_confirmed: false,
        qsl_rcvd: QslRcvd::default(),
        qsl_sent: QslSent::default(),
        credit_granted: Vec::new(),
        credit_submitted: Vec::new(),
        upload: UploadState::default(),
        ota: crate::logbook::Ota::default(),
        dxcc: None,
        prop_mode: None,
        sat_name: None,
        operator: None,
        station_callsign: Some(log.mycall.clone()).filter(|c| !c.trim().is_empty()),
        extra: Vec::new(),
        contest: Some(Box::new(contest)),
    }
}

/// The numeric serial in a field vector, when the exchange declares one — ADIF's `STX`
/// and `SRX` are numbers, and a contest whose exchange is not a serial has neither.
fn serial_of(vals: &[super::spec::FieldValue], spec: &super::spec::ExchangeSpec) -> Option<u32> {
    vals.iter()
        .find(|v| {
            matches!(
                spec.field(v.key).map(|f| &f.kind),
                Some(FieldKind::Serial { .. })
            )
        })
        .and_then(|v| v.raw.trim().parse().ok())
}

/// An exchange as `STX_STRING`/`SRX_STRING`: the raw values, space separated, empties
/// dropped. `None` rather than an empty tag when there is nothing to say.
fn joined<'a>(raws: impl Iterator<Item = &'a str>) -> Option<String> {
    let s = raws
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!s.is_empty()).then_some(s)
}

/// A field vector as the general log's `(slot, raw)` pairs — the domain deliberately
/// dropped, because the general log does not score and ADIF has nowhere to put one.
fn pairs(vals: &[super::spec::FieldValue]) -> Vec<(String, String)> {
    vals.iter()
        .map(|v| (v.key.to_string(), v.raw.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::{ContestSession, DupeRule};
    use crate::fieldday::FdEvent;

    const FD_RULE: DupeRule = DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &[],
        by_sent_fields: &[],
    };

    /// A unique scratch path under the OS temp dir — the shape `logbook.rs`'s own
    /// tests use, so the restart leg re-reads a REAL file through the real loader.
    fn scratch_adi() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tempo_mergetest_{}_{n}.adi", std::process::id()))
    }

    fn fd_log() -> FieldDayLog {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        assert!(log.log_mode_at("W1AW", "1D", "CT", "PH", 0, 1_782_583_800));
        log
    }

    #[test]
    fn a_merged_row_carries_both_sides_of_its_own_exchange() {
        let log = fd_log();
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "a1b2c3d4", &mut lb);
        assert_eq!((r.added(), r.already, r.refused), (2, 0, 0));
        let rec = &lb.records()[0];
        let c = rec.contest.as_deref().expect("a merged row has provenance");
        assert_eq!(c.contest_id, "ARRL-FIELD-DAY");
        assert_eq!(c.qid, "ARRL-FIELD-DAY:WI:a1b2c3d4:1");
        assert_eq!(c.stx_string.as_deref(), Some("3A WI"), "what I sent");
        assert_eq!(c.srx_string.as_deref(), Some("2A EMA"), "what they sent");
        assert_eq!(
            c.sent,
            vec![
                ("CLASS".to_string(), "3A".to_string()),
                ("SECTION".to_string(), "WI".to_string())
            ]
        );
        assert_eq!(
            c.rcvd,
            vec![
                ("CLASS".to_string(), "2A".to_string()),
                ("SECTION".to_string(), "EMA".to_string())
            ]
        );
        assert_eq!(rec.mode, "CW");
        assert_eq!(rec.station_callsign.as_deref(), Some("W9XYZ"));
    }

    /// ⭐ §3.2 — merging twice writes nothing the second time.
    #[test]
    fn a_second_merge_writes_nothing() {
        let log = fd_log();
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        let again = merge_into_general(&log, "pos", &mut lb);
        assert_eq!((again.added(), again.already), (0, 2));
        assert_eq!(lb.len(), 2, "the general log did not grow");
        // POSITIVE CONTROL: the skip is the QID and not a merge that never writes. A
        // DIFFERENT position id is a different row identity, and those DO land.
        let other = merge_into_general(&log, "other-pos", &mut lb);
        assert_eq!(other.added(), 2);
        assert_eq!(lb.len(), 4);
    }

    /// ⭐ §3.5(3) — the idempotence survives a RESTART, which is the leg that proves
    /// the ADIF reader exists. The in-memory leg above would pass with no reader at all.
    #[test]
    fn the_merge_is_idempotent_across_a_restart() {
        let log = fd_log();
        let path = scratch_adi();
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        lb.save(&path).expect("write log.adi");
        // RESTART: drop the Logbook and re-read the file, exactly as launch does.
        drop(lb);
        let mut reborn = Logbook::load(&path);
        assert_eq!(reborn.len(), 2, "the re-read log has both rows");
        let after = merge_into_general(&log, "pos", &mut reborn);
        assert_eq!((after.added(), after.already), (0, 2));
        assert_eq!(reborn.len(), 2, "no duplicate after a restart");

        // POSITIVE CONTROL: with the qid stripped from the file on disk, the SAME
        // merge DOES duplicate — so the green above is the reader working, not a merge
        // that cannot write.
        let text = std::fs::read_to_string(&path).expect("read back");
        let stripped = strip_tag(&text, "APP_NEXUS_QID");
        assert!(
            !stripped.contains("APP_NEXUS_QID"),
            "the control has to actually remove the tag"
        );
        let blind_path = scratch_adi();
        std::fs::write(&blind_path, &stripped).expect("write stripped");
        let mut blind = Logbook::load(&blind_path);
        assert_eq!(blind.len(), 2);
        assert_eq!(merge_into_general(&log, "pos", &mut blind).added(), 2);
        assert_eq!(blind.len(), 4, "no qid, no idempotence — the control trips");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&blind_path);
    }

    /// Remove one `<TAG:len>value` field from ADIF text, value and all.
    fn strip_tag(text: &str, tag: &str) -> String {
        let needle = format!("<{tag}:");
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(i) = rest.find(&needle) {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            rest = match after.find('<') {
                Some(e) => &after[e..],
                None => "",
            };
        }
        out.push_str(rest);
        out
    }

    /// A row with no club-sync sequence has no stable identity, so it is refused and
    /// counted rather than merged twice or collapsed onto one.
    ///
    /// The shape has to be built by hand, because every production path stamps a
    /// sequence — including a 1.x journal restore, which backfills one (and backfills
    /// even over a journaled `0`). That is the guard being defensive rather than
    /// load-bearing, and it is worth saying so out loud rather than leaving an
    /// untested branch in a merge that writes to the operator's logbook.
    #[test]
    fn an_unstamped_row_is_refused_and_reported() {
        let mut log = fd_log();
        log.qsos_mut()[0].seq = 0;
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!((r.added(), r.already, r.refused), (1, 0, 1));
        assert_eq!(lb.len(), 1, "the stamped row still merged");
        // POSITIVE CONTROL: with the sequence back, the same row merges — so the
        // refusal is the missing identity and not a merge that drops the first row.
        let whole = fd_log();
        let mut lb2 = Logbook::new();
        assert_eq!(merge_into_general(&whole, "pos", &mut lb2).added(), 2);
    }

    /// …and the real journal path never produces one: a 1.x row with no `APP_NEXUS_QSEQ`
    /// backfills a sequence and merges, so a restart onto a pre-sync journal does not
    /// silently lose the morning's contacts to the guard above.
    #[test]
    fn a_1x_journal_row_backfills_its_sequence_and_merges() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.merge_adif(
            "<EOH>\n<CALL:5>K1ABC<CLASS:2>2A<ARRL_SECT:3>EMA<BAND:3>20m<MODE:2>CW\
             <QSO_DATE:8>20260627<TIME_ON:6>180500<EOR>\n",
            0,
        );
        assert_ne!(log.qsos()[0].seq, 0);
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!((r.added(), r.refused), (1, 0));
    }

    /// §18.1 — the destination is a property of the SESSION, and it is OFF.
    #[test]
    fn a_fresh_session_uploads_nowhere() {
        let s = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        assert!(!s.upload.enabled, "default OFF, on every new session");
        assert!(s.upload_destinations().is_empty());
        // Turning the switch on with nowhere to send is still nowhere — a switch and a
        // destination are both required.
        let mut on = s.clone();
        on.upload.enabled = true;
        assert!(on.upload_destinations().is_empty());
        // …and a destination with the switch off is nowhere too.
        let mut named = s.clone();
        named.upload.destinations = vec!["wrl".into()];
        assert!(
            named.upload_destinations().is_empty(),
            "a destination alone must not opt anybody in"
        );
        // POSITIVE CONTROL: both halves together DO name a destination, so the three
        // empties above are the policy and not an accessor that always answers empty.
        let mut both = s;
        both.upload.enabled = true;
        both.upload.destinations = vec!["wrl".into()];
        assert_eq!(both.upload_destinations(), ["wrl".to_string()]);
    }

    /// The hint the operator reads beside that switch must say the one thing the
    /// switch does not control (§18.1): ClubLog's catch-up sweep.
    #[test]
    fn the_session_control_names_the_clublog_exception() {
        let hint = super::super::UPLOAD_CLUBLOG_SWEEP_HINT;
        assert!(hint.contains("ClubLog"), "{hint}");
        assert!(hint.to_lowercase().contains("password"), "{hint}");
    }

    /// §3.1's own test: a merged row is visible to the session-scoped sweep under the
    /// contest's rule, and an ordinary contact is not confused with one.
    #[test]
    fn merged_rows_reach_the_session_scoped_sweep() {
        let log = fd_log();
        let mut lb = Logbook::new();
        merge_into_general(&log, "pos", &mut lb);
        let w = lb.worked_keys_since(1_782_583_000, &FD_RULE);
        assert!(w.exact.contains(&vec![
            "K1ABC".to_string(),
            "20M".to_string(),
            "CW".to_string()
        ]));
        assert!(w.worked_this_session.contains("W1AW"));
        // …and the cutoff really bounds it.
        let none = lb.worked_keys_since(1_900_000_000, &FD_RULE);
        assert!(none.exact.is_empty() && none.worked_this_session.is_empty());
    }
}
