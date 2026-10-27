//! ⭐ **Which Logbook copy is which contest contact** — the merge identity a contest log gives
//! each contact it merges (`APP_NEXUS_QID`), and the copies an earlier build left in the Logbook.
//!
//! The identity named the contest and WHERE THE OPERATOR WAS, so the same contact got a second
//! name after a move, and the same contest next year from the same place got the first year's
//! names. Both are tested here by value: the Logbook's row count after a merge.
//!
//! The files in `fixtures/contest-merge/` were WRITTEN BY origin/main (b60b23bf2): a scratch
//! program built against that commit's own `tempo-core` (with its `Cargo.lock`) logged the
//! contacts below, merged them with `merge_into_general`, and saved the journal
//! (`FieldDayLog::journal_adif`) and the Logbook (`Logbook::save`) — so what this build reads is
//! what an operator's disk holds today, never a reading of the old code.
//!
//! - `ilqp-*-kane`: the Illinois QSO Party 2026 from Kane County — two contacts, merged.
//! - `ilqp-*-will`: the same log after the County was changed to Will and the session built again
//!   (a restart): that build's second merge wrote both contacts again under Will, a third
//!   contact from Will was merged, and a fourth was logged and not merged.
//! - `fd-*`: ARRL Field Day 2026, 3A from Wisconsin — two contacts merged; then I moved to
//!   Illinois and a third contact, not merged.
use tempo_core::contest::{install_call_resolver, merge_into_general, CallLocation};
use tempo_core::contest::{merged_row, plan_merge, ContestSession, MergeIdentity, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{FdEvent, FieldDayLog};
use tempo_core::logbook::Logbook;

/// The position the fixtures were merged at.
const POS: &str = "a1b2c3d4";

fn place(call: &str) -> Option<CallLocation> {
    let entity = match call.chars().next()? {
        'W' | 'K' | 'N' => "United States",
        _ => return None,
    };
    Some(CallLocation {
        entity,
        continent: "NA",
        cq_zone: None,
    })
}

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
    });
}

fn party(event: &str, state: &str, county: &str) -> ContestSession {
    resolver();
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    ContestSession::for_ruleset(
        rs,
        &StationData {
            mycall: "W9XYZ".into(),
            contest_qth_state: state.into(),
            contest_qth_county: county.into(),
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{event} refused: {e}"))
}

/// When `event`'s running of `year` starts.
fn start(event: &str, year: u16) -> u64 {
    ruleset_by_id(event, CURRENT_RULES_YEAR)
        .expect("a shipped ruleset")
        .event_window(year)
        .start_unix
}

/// One contact, sent from wherever the session is now.
fn work(log: &mut FieldDayLog, call: &str, qth: &str, when: u64) {
    let spec = log.session.exchange;
    let rx = vec![
        spec.value("RST", "599").expect("RST"),
        spec.value("QTH", qth).expect("QTH"),
    ];
    let tx = log.session.my_exchange.clone();
    assert!(
        log.log_exchange_at(call, rx, tx, "CW", "", 0, when),
        "{call}"
    );
}

/// A fixture file's path.
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/contest-merge")
        .join(name)
}

fn text(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).expect("the fixture")
}

/// The fixture Logbook `name`, read by the real loader from a copy of its own: the loader keeps
/// a backup anchor and a manifest beside the file it reads, which must not land in the fixtures.
fn logbook(name: &str) -> Logbook {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nexus_contest_merge_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    let path = dir.join(name);
    std::fs::copy(fixture(name), &path).expect("the fixture");
    let lb = Logbook::load(&path);
    let _ = std::fs::remove_dir_all(&dir);
    lb
}

/// A session built from `session` with the journal `name` read back into it, as a launch inside
/// the running starting `t0` reads it.
fn restored(session: ContestSession, name: &str, t0: u64) -> FieldDayLog {
    let mut log = FieldDayLog::new("W9XYZ", session, "20m");
    let read = log.restore_journal(&text(name), t0 - 4 * 86_400, t0 + 6 * 3_600);
    assert_eq!(read.unreadable, 0, "{name} reads whole");
    log
}

fn calls(lb: &Logbook) -> Vec<String> {
    lb.records().iter().map(|r| r.call.clone()).collect()
}

/// ⭐ **The same party next year, from the same county, is merged.** Each contact's identity was
/// `<contest>:<county>:<position>:<number>` and the numbers start at 1 again, so next year's
/// contacts carried last year's identities and the merge skipped every one of them as already
/// in the Logbook.
#[test]
fn the_same_party_next_year_from_the_same_county_is_merged() {
    let mut lb = Logbook::new();
    let t26 = start("ilqp", 2026);
    let mut y26 = FieldDayLog::new("W9XYZ", party("ilqp", "IL", "KANE"), "20m");
    work(&mut y26, "K9AAA", "COOK", t26 + 300);
    work(&mut y26, "W9BBB", "LAKE", t26 + 900);
    assert_eq!(merge_into_general(&y26, POS, &mut lb).added(), 2);

    let t27 = start("ilqp", 2027);
    let mut y27 = FieldDayLog::new("W9XYZ", party("ilqp", "IL", "KANE"), "20m");
    work(&mut y27, "N9CCC", "COOK", t27 + 300);
    work(&mut y27, "K9DDD", "LAKE", t27 + 900);
    let r = merge_into_general(&y27, POS, &mut lb);
    assert_eq!(
        (r.added(), r.already),
        (2, 0),
        "next year's contacts are new contacts"
    );
    assert_eq!(calls(&lb), ["K9AAA", "W9BBB", "N9CCC", "K9DDD"]);
}

/// CONTROL: two different contests, the same position and the same numbers, never share an
/// identity — each is merged.
#[test]
fn two_contests_with_the_same_numbers_are_each_merged() {
    let t = start("ilqp", 2026);
    let mut il = FieldDayLog::new("W9XYZ", party("ilqp", "IL", "KANE"), "20m");
    work(&mut il, "K9AAA", "COOK", t + 300);
    let mut ny = FieldDayLog::new("W9XYZ", party("nyqp", "IL", ""), "20m");
    work(&mut ny, "K2AAA", "ALB", t + 600);
    let mut lb = Logbook::new();
    assert_eq!(merge_into_general(&il, POS, &mut lb).added(), 1);
    assert_eq!(merge_into_general(&ny, POS, &mut lb).added(), 1);
    assert_eq!(calls(&lb), ["K9AAA", "K2AAA"]);
}

/// ⭐ **A log merged by origin/main, and the County changed since**: this build reads that
/// build's journal into a session built in Will and merges into that build's Logbook — and
/// writes nothing, because both contacts are there under the Kane names.
#[test]
fn contacts_merged_by_the_previous_build_are_not_written_again_after_a_county_change() {
    let t0 = start("ilqp", 2026);
    let mut log = restored(party("ilqp", "IL", "WILL"), "ilqp-journal-kane.adi", t0);
    assert_eq!(
        log.qsos()
            .iter()
            .map(|q| q.call.as_str())
            .collect::<Vec<_>>(),
        ["K9AAA", "W9BBB"],
        "PREMISE: the journal's two contacts"
    );
    let mut lb = logbook("ilqp-logbook-kane.adi");
    assert_eq!(
        calls(&lb),
        ["K9AAA", "W9BBB"],
        "PREMISE: that build's Logbook"
    );
    let r = merge_into_general(&log, POS, &mut lb);
    assert_eq!((r.added(), r.already), (0, 2), "both are already there");
    assert_eq!(calls(&lb), ["K9AAA", "W9BBB"]);

    // CONTROL: a contact logged now, from Will, is merged, once.
    work(&mut log, "N9CCC", "DUPG", t0 + 4_000);
    let r = merge_into_general(&log, POS, &mut lb);
    assert_eq!((r.added(), r.already), (1, 2));
    assert_eq!(calls(&lb), ["K9AAA", "W9BBB", "N9CCC"]);
}

/// …and the Logbook that build had ALREADY doubled (its second merge, under Will) grows by the
/// one contact it never merged, and by nothing else.
#[test]
fn a_logbook_the_previous_build_doubled_grows_by_the_unmerged_contact_only() {
    let t0 = start("ilqp", 2026);
    let log = restored(party("ilqp", "IL", "WILL"), "ilqp-journal-will.adi", t0);
    assert_eq!(
        log.qsos()
            .iter()
            .map(|q| q.call.as_str())
            .collect::<Vec<_>>(),
        ["K9AAA", "W9BBB", "N9CCC", "K9DDD"],
        "PREMISE: four contacts, the fourth never merged"
    );
    let mut lb = logbook("ilqp-logbook-will.adi");
    let before = calls(&lb);
    assert_eq!(
        before,
        ["K9AAA", "W9BBB", "K9AAA", "W9BBB", "N9CCC"],
        "PREMISE: that build's Logbook, the first two written twice"
    );
    let r = merge_into_general(&log, POS, &mut lb);
    assert_eq!((r.added(), r.already), (1, 3));
    assert_eq!(calls(&lb)[before.len()..], ["K9DDD"]);
    // Nothing already in the Logbook was rewritten: the same rows, the same names.
    let reread = logbook("ilqp-logbook-will.adi");
    for (now, then) in lb.records().iter().zip(reread.records()) {
        assert_eq!(now.contest, then.contest, "{}", then.call);
    }
}

/// ⭐ **Field Day, the same way**: merged from Wisconsin by origin/main, I moved to Illinois, a
/// third contact. This build, in a session built in Illinois, writes the third and only it.
#[test]
fn a_field_day_log_merged_by_the_previous_build_is_not_written_again_after_i_moved() {
    let fd = tempo_core::fd_rules::ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR);
    let t0 = fd.event_window(2026).start_unix;
    let log = restored(
        ContestSession::field_day(FdEvent::ArrlFd, "3A", "IL"),
        "fd-journal-il.adi",
        t0,
    );
    assert_eq!(
        log.qsos()
            .iter()
            .map(|q| q.call.as_str())
            .collect::<Vec<_>>(),
        ["K1ABC", "W1AW", "K2DEF"],
        "PREMISE: three contacts, the third from Illinois"
    );
    let mut lb = logbook("fd-logbook-wi.adi");
    assert_eq!(calls(&lb), ["K1ABC", "W1AW"]);
    let r = merge_into_general(&log, POS, &mut lb);
    assert_eq!((r.added(), r.already), (1, 2));
    assert_eq!(calls(&lb), ["K1ABC", "W1AW", "K2DEF"]);
}

/// ⭐ **A Logbook edit or delete of a copy merged under either name reaches its contest row**:
/// every copy in that build's doubled Logbook — the Kane names and the Will names — names the
/// row it was merged from, and at another position none does.
#[test]
fn every_copy_the_previous_build_wrote_names_its_own_row() {
    let t0 = start("ilqp", 2026);
    let log = restored(party("ilqp", "IL", "WILL"), "ilqp-journal-will.adi", t0);
    let lb = logbook("ilqp-logbook-will.adi");
    let named = |posid: &str| -> Vec<(String, Option<u64>)> {
        lb.records()
            .iter()
            .map(|r| {
                let qid = &r.contest.as_deref().expect("merged").qid;
                (qid.clone(), merged_row(&log, posid, qid, r.when_unix))
            })
            .collect()
    };
    let want = |seqs: [Option<u64>; 5]| -> Vec<(String, Option<u64>)> {
        [
            "IL QSO Party:KANE:a1b2c3d4:1",
            "IL QSO Party:KANE:a1b2c3d4:2",
            "IL QSO Party:WILL:a1b2c3d4:1",
            "IL QSO Party:WILL:a1b2c3d4:2",
            "IL QSO Party:WILL:a1b2c3d4:3",
        ]
        .iter()
        .map(|q| q.to_string())
        .zip(seqs)
        .collect()
    };
    assert_eq!(
        named(POS),
        want([Some(1), Some(2), Some(1), Some(2), Some(3)])
    );
    assert_eq!(
        named("e5f6a7b8"),
        want([None; 5]),
        "another position's numbers"
    );
}

/// ⭐ **An earlier build's names carry no year, and last year's never hide this year's
/// contacts.** The same party from the same county a year on: its first two contacts have the
/// numbers that build's Kane copies name, and they are new contacts.
#[test]
fn the_previous_builds_copies_from_last_year_hide_nothing_this_year() {
    let t27 = start("ilqp", 2027);
    let mut y27 = FieldDayLog::new("W9XYZ", party("ilqp", "IL", "KANE"), "20m");
    work(&mut y27, "N9CCC", "COOK", t27 + 300);
    work(&mut y27, "K9DDD", "LAKE", t27 + 900);
    let mut lb = logbook("ilqp-logbook-kane.adi");
    let r = merge_into_general(&y27, POS, &mut lb);
    assert_eq!((r.added(), r.already), (2, 0));
    assert_eq!(calls(&lb), ["K9AAA", "W9BBB", "N9CCC", "K9DDD"]);
}

/// …nor another contest's: the New York QSO Party the day before, from the same position, has
/// the same numbers and the same running, and is a different contest.
#[test]
fn the_previous_builds_copies_of_another_contest_hide_nothing() {
    let t = start("nyqp", 2026);
    let mut ny = FieldDayLog::new("W9XYZ", party("nyqp", "IL", ""), "20m");
    work(&mut ny, "K2AAA", "ALB", t + 300);
    work(&mut ny, "K2BBB", "BRX", t + 900);
    let mut lb = logbook("ilqp-logbook-kane.adi");
    let r = merge_into_general(&ny, POS, &mut lb);
    assert_eq!((r.added(), r.already), (2, 0));
}

/// A copy this build merged is its row's whatever its time now says: an operator who corrects
/// the Logbook's clock by a year after merging (a station whose clock was wrong all weekend)
/// does not get the contact written again.
#[test]
fn a_copy_whose_time_was_corrected_in_the_logbook_is_still_its_rows() {
    let t0 = start("ilqp", 2026);
    let mut log = FieldDayLog::new("W9XYZ", party("ilqp", "IL", "KANE"), "20m");
    work(&mut log, "K9AAA", "COOK", t0 + 300);
    let id = MergeIdentity::of(&log, POS, &log.qsos()[0]);
    assert_eq!(id.qid(), "IL QSO Party:2026:a1b2c3d4:1");
    let a_year_on = t0 + 300 + 365 * 86_400;
    let r = plan_merge(&log, POS, [(id.qid().to_string(), a_year_on)]);
    assert_eq!((r.added(), r.already), (0, 1));
    assert_eq!(merged_row(&log, POS, id.qid(), a_year_on), Some(1));
    // CONTROL: the same copy under an earlier build's name is held to its time.
    let r = plan_merge(
        &log,
        POS,
        [(format!("IL QSO Party:KANE:{POS}:1"), a_year_on)],
    );
    assert_eq!((r.added(), r.already), (1, 0));
}

/// What this build's Logbook copy of a merged contact says it is — the name every export and
/// upload carries — and the copies already there keep the names they were written with.
#[test]
fn the_copies_already_merged_keep_their_names_in_every_export() {
    let t0 = start("ilqp", 2026);
    let log = restored(party("ilqp", "IL", "WILL"), "ilqp-journal-will.adi", t0);
    let mut lb = logbook("ilqp-logbook-will.adi");
    let r = merge_into_general(&log, POS, &mut lb);
    assert_eq!(r.added(), 1);
    for (rec, qid) in lb.records().iter().zip([
        "IL QSO Party:KANE:a1b2c3d4:1",
        "IL QSO Party:KANE:a1b2c3d4:2",
        "IL QSO Party:WILL:a1b2c3d4:1",
        "IL QSO Party:WILL:a1b2c3d4:2",
        "IL QSO Party:WILL:a1b2c3d4:3",
        // The contact merged now: the contest and the running, never the county.
        "IL QSO Party:2026:a1b2c3d4:4",
    ]) {
        let tag = format!("<APP_NEXUS_QID:{}>{qid}<", qid.len());
        // The upload's record and the Logbook export's.
        assert!(
            tempo_core::logbook::adif_record(rec).contains(&tag),
            "{qid}"
        );
        assert!(
            tempo_core::logbook::adif_record_own_log(rec).contains(&tag),
            "{qid}"
        );
    }
}

/// A merged contest contact carries the continent its row resolved at log time (ADIF
/// `CONT`), through the Logbook's own file and back.
#[test]
fn a_merged_contact_carries_its_rows_continent() {
    resolver();
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
        "20m",
    );
    assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
    let mut lb = Logbook::new();
    assert_eq!(merge_into_general(&log, POS, &mut lb).added(), 1);
    let text: String = lb
        .records()
        .iter()
        .map(|r| tempo_core::logbook::adif_record_own_log(r))
        .collect();
    let back = &tempo_core::logbook::parse_adif(&text)[0];
    assert_eq!(tempo_core::logbook::extra_field(back, "CONT"), Some("NA"));
}
