//! The Engine's journals on their own restore and write path, when the file there cannot be
//! read (`tempo_core::keep_aside`): it is kept byte for byte under a dated name and never
//! written over, the snapshot says where it is, a journal whose only news is fields restores as
//! it always has, and no journal is a first run. "Written over" is tested in a folder the
//! writers CAN write to — a file left in place because every dated name is taken — so a writer
//! that forgot to ask would really replace it.

use super::*;
use crate::dto::KeptFile;
use crate::engine::remote_logging::CurrentQsoLogOutcome;
use crate::engine::tests::dec_snr;
use std::path::{Path, PathBuf};

/// 2026-09-30 14:22:33 UTC, and the dated name it gives.
const NOW: i64 = 1_790_778_153;
const STAMP: &str = "20260930-142233";

/// A unique scratch directory; the name carries the test's own label.
fn scratch(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nexus-kept-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Where `keep_aside` puts `file` (`pending_qso.json` → `pending_qso.unreadable-<STAMP>.json`).
fn aside(dir: &Path, file: &str) -> PathBuf {
    let (stem, ext) = file.rsplit_once('.').expect("a file with an extension");
    dir.join(format!("{stem}.unreadable-{STAMP}.{ext}"))
}

/// Take every dated name `keep_aside` could use for `file`, so it must leave the file where it
/// is — in a folder every writer can write to.
fn take_every_aside_name(dir: &Path, file: &str) {
    let (stem, ext) = file.rsplit_once('.').expect("a file with an extension");
    std::fs::write(aside(dir, file), b"taken").unwrap();
    for n in 2..=100 {
        std::fs::write(
            dir.join(format!("{stem}.unreadable-{STAMP}-{n}.{ext}")),
            b"taken",
        )
        .unwrap();
    }
}

/// Every file in `dir` whose bytes are exactly `bytes`.
fn holding(dir: &Path, bytes: &[u8]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| std::fs::read(p).ok().as_deref() == Some(bytes))
        .collect();
    found.sort();
    found
}

/// The snapshot's kept files under `dir` (the list is the whole process's, and tests run in
/// parallel).
fn kept_under(e: &Engine, dir: &Path) -> Vec<KeptFile> {
    e.snapshot()
        .kept_files
        .into_iter()
        .filter(|k| Path::new(&k.path).starts_with(dir))
        .collect()
}

fn kept(store: &str, path: &Path, kept_in_place: bool) -> KeptFile {
    KeptFile {
        store: store.into(),
        path: path.to_string_lossy().into_owned(),
        kept_in_place,
    }
}

/// A contact as the log would hold it.
fn contact(call: &str) -> QsoRecord {
    let field = |tag: &str, value: &str| format!("<{tag}:{}>{value}", value.len());
    let text = [
        field("CALL", call),
        field("BAND", "20m"),
        field("MODE", "FT8"),
        field("FREQ", "14.074"),
        field("QSO_DATE", "20260930"),
        field("TIME_ON", "142233"),
    ]
    .concat();
    let mut lb = tempo_core::logbook::Logbook::new();
    lb.import_adif(&(text + "<EOR>"));
    QsoRecord::clone(&lb.records()[0])
}

fn engine_on(path: &Path) -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.settings.prompt_to_log = true;
    e.set_pending_qso_path(path.to_path_buf());
    e
}

/// A confirm-before-log journal as this build writes it, holding W9XYZ.
fn a_pending_qso_journal() -> String {
    let dir = scratch("made");
    let path = dir.join("made.json");
    let mut e = engine_on(&path);
    e.load_pending_qso(contact("W9XYZ"));
    let text = std::fs::read_to_string(&path).expect("the hold is journaled");
    std::fs::remove_dir_all(&dir).unwrap();
    text
}

// ── pending_qso.json: the contacts waiting in the "Log this QSO?" popup ─────────────────────

/// An unreadable journal is kept, byte for byte, beside the new one; the next held contact
/// starts a new journal and does not touch the kept one; the snapshot names where it is.
fn assert_pending_qso_kept(what: &str, bytes: Vec<u8>) {
    let dir = scratch("pendingqso");
    let path = dir.join("pending_qso.json");
    std::fs::write(&path, &bytes).unwrap();

    let mut e = engine_on(&path);
    e.restore_pending_qso_journal(NOW);
    assert_eq!(e.pending_log(), None, "{what}: nothing is read out of it");
    e.load_pending_qso(contact("K1ABC"));

    assert_eq!(
        holding(&dir, &bytes),
        vec![aside(&dir, "pending_qso.json")],
        "{what}: the unreadable journal must survive the next held contact, byte for byte, moved aside"
    );
    assert_eq!(
        kept_under(&e, &dir),
        vec![kept("pendingQso", &aside(&dir, "pending_qso.json"), false)],
        "{what}: the screen is told where it is"
    );
    let mut relaunched = engine_on(&path);
    relaunched.restore_pending_qso_journal(NOW + 1);
    assert_eq!(
        relaunched.pending_log().map(|q| q.call.as_str()),
        Some("K1ABC"),
        "{what}: the new journal is this build's"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pending_qso_journal_cut_short_is_kept_and_never_written_over() {
    let whole = a_pending_qso_journal().into_bytes();
    assert_pending_qso_kept("cut short", whole[..whole.len() * 3 / 5].to_vec());
}

/// A value this build does not know: a grid provenance from a later build.
#[test]
fn a_pending_qso_journal_with_a_value_this_build_does_not_know_is_kept_and_never_written_over() {
    let journal = a_pending_qso_journal();
    let unknown = journal.replacen(
        "\"gridSource\":\"lookedUp\"",
        "\"gridSource\":\"heardOnAir\"",
        1,
    );
    assert!(
        unknown.contains("heardOnAir"),
        "the fixture must carry the unknown value: {journal}"
    );
    assert_pending_qso_kept("an unknown value", unknown.into_bytes());
}

/// A journal that cannot be moved aside stays where it is, and no later hold, discard or
/// confirmation writes over it or removes it.
#[test]
fn a_pending_qso_journal_that_cannot_be_moved_is_never_written_over_or_removed() {
    let dir = scratch("pendingqso-stuck");
    let path = dir.join("pending_qso.json");
    let bytes = b"[{\"call\":\"W9XYZ\"".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    take_every_aside_name(&dir, "pending_qso.json");

    let mut e = engine_on(&path);
    e.restore_pending_qso_journal(NOW);
    e.load_pending_qso(contact("K1ABC"));
    let key = e
        .pending_qso_log_key()
        .expect("the contact is held in memory");
    assert!(
        e.discard_pending_log(&key),
        "the operator can still discard it"
    );

    assert_eq!(
        std::fs::read(&path).ok(),
        Some(bytes),
        "the unreadable journal must never be written over or removed"
    );
    assert_eq!(
        kept_under(&e, &dir),
        vec![kept("pendingQso", &path, true)],
        "the screen is told it was left in place"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Remote's hold and discard go through their own writers: a rename of a prepared journal
/// over the file, and a removal. Neither may touch a journal kept in place.
#[test]
fn the_remotes_hold_and_discard_never_touch_a_pending_qso_journal_kept_in_place() {
    let dir = scratch("pendingqso-remote");
    let path = dir.join("pending_qso.json");
    let bytes = b"[{\"call\":\"W9XYZ\"".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    take_every_aside_name(&dir, "pending_qso.json");
    let mut e = engine_on(&path);
    e.restore_pending_qso_journal(NOW);
    e.set_tier(crate::dto::Tier::Ft8);
    e.set_log_path(dir.join("log.adi"));
    e.call_station_with_grid("W9XYZ", Some("EN37"));
    e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
    e.qso_start_unix = Some(1_700_000_000);

    let CurrentQsoLogOutcome::Pending(write) = e.log_current_qso_for_sync() else {
        panic!("the contact must be held")
    };
    let pending = e.pending_log_identity().expect("held in memory");
    let prepared = write.prepare().expect("a temporary journal beside it");
    assert!(
        e.publish_pending_qso_journal(prepared).is_err(),
        "the prepared journal is not renamed over the kept one"
    );
    assert!(e.discard_pending_log_for_sync(&pending).is_ok());
    assert_eq!(
        std::fs::read(&path).ok(),
        Some(bytes),
        "the unreadable journal must never be written over or removed"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A journal from a newer build that only ADDED fields restores as it always has.
#[test]
fn a_pending_qso_journal_whose_only_news_is_fields_restores_as_it_always_has() {
    let dir = scratch("pendingqso-fields");
    let path = dir.join("pending_qso.json");
    let journal = a_pending_qso_journal().replacen("{", "{\"aNewField\":7,", 1);
    assert!(journal.contains("aNewField"));
    std::fs::write(&path, &journal).unwrap();
    let mut e = engine_on(&path);
    e.restore_pending_qso_journal(NOW);
    assert_eq!(
        (
            e.pending_log().map(|q| q.call.as_str()),
            kept_under(&e, &dir)
        ),
        (Some("W9XYZ"), vec![]),
        "restored as it always has, with nothing to say"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// No journal at all is a first run.
#[test]
fn no_pending_qso_journal_is_a_first_run() {
    let dir = scratch("pendingqso-none");
    let path = dir.join("pending_qso.json");
    let mut e = engine_on(&path);
    e.restore_pending_qso_journal(NOW);
    assert_eq!(
        (e.pending_log(), kept_under(&e, &dir)),
        (None, vec![]),
        "no journal: nothing held and nothing to say"
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing made");
    std::fs::remove_dir_all(&dir).unwrap();
}

// ── fieldday_backup_<posid>.adi: the Field Day or contest log's backup ───────────────────────

const FD_JOURNAL: &str = "fieldday_backup_ab12cd34.adi";

/// An engine in Field Day on the journal at `path`, entered the way a launch enters it.
fn fd_engine(path: &Path) -> Engine {
    let mut s = Engine::new("W9XYZ", "EN61", 0).settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    let mut e = Engine::with_settings(s);
    e.set_fd_log_path(path.to_path_buf());
    e.restore_field_day_if_enabled();
    assert!(e.snapshot().field_day.is_some(), "premise: in Field Day");
    e
}

fn fd_count(e: &Engine) -> usize {
    e.snapshot().field_day.map_or(0, |f| f.qso_count)
}

/// Log a contest contact and wait for its journal write, as an operator's logging command does.
fn log_fd(e: &mut Engine, call: &str) {
    assert!(
        e.fd_log_manual(call, "2A", "EMA", "CW").unwrap(),
        "{call} logs"
    );
    e.journal_mark().wait();
}

/// A Field Day journal as this build writes it: three contacts.
fn a_field_day_journal() -> Vec<u8> {
    let dir = scratch("fdmade");
    let path = dir.join(FD_JOURNAL);
    let mut e = fd_engine(&path);
    for call in ["K1ABC", "N0GHI", "W7XYZ"] {
        log_fd(&mut e, call);
    }
    let bytes = std::fs::read(&path).expect("the journal is written");
    std::fs::remove_dir_all(&dir).unwrap();
    bytes
}

/// The file in `dir` holding `bytes`, which must be the journal's dated set-aside name.
fn kept_fd_journal(dir: &Path, bytes: &[u8], what: &str) -> PathBuf {
    let found = holding(dir, bytes);
    assert_eq!(
        found.len(),
        1,
        "{what}: the unreadable journal must survive the next contact, byte for byte, moved aside"
    );
    let name = found[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("fieldday_backup_ab12cd34.unreadable-") && name.ends_with(".adi"),
        "{what}: under its dated name, not its own: {name}"
    );
    found[0].clone()
}

/// A journal this build cannot read in full is kept, byte for byte; the contacts it could read
/// come back; the next contact starts a new journal; the snapshot names where the old one is.
fn assert_fd_journal_kept(what: &str, bytes: Vec<u8>, readable: usize) {
    let dir = scratch("fd");
    let path = dir.join(FD_JOURNAL);
    std::fs::write(&path, &bytes).unwrap();

    let mut e = fd_engine(&path);
    assert_eq!(fd_count(&e), readable, "{what}: the contacts it could read");
    log_fd(&mut e, "K9NEW");

    let aside = kept_fd_journal(&dir, &bytes, what);
    assert_eq!(
        kept_under(&e, &dir),
        vec![kept("fieldDay", &aside, false)],
        "{what}: the screen is told where it is"
    );
    let relaunched = fd_engine(&path);
    assert_eq!(
        fd_count(&relaunched),
        readable + 1,
        "{what}: the new journal holds what was read, and the new contact"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_field_day_journal_cut_short_is_kept_and_its_readable_contacts_restored() {
    let whole = a_field_day_journal();
    let text = String::from_utf8(whole).unwrap();
    let last_eor = text.rfind("<EOR>").expect("a record end");
    assert_fd_journal_kept("cut short", text.as_bytes()[..last_eor - 4].to_vec(), 2);
}

/// Bytes that are not UTF-8 used to read as NO journal at all, and the next contact then
/// replaced every contact of the event.
#[test]
fn a_field_day_journal_that_is_not_utf8_is_kept_and_its_contacts_restored() {
    let whole = a_field_day_journal();
    let first_eor = whole
        .windows(5)
        .position(|w| w == b"<EOR>")
        .expect("a record end")
        + 5;
    let mut bad = whole[..first_eor].to_vec();
    bad.push(0xFF);
    bad.extend_from_slice(&whole[first_eor..]);
    assert!(
        String::from_utf8(bad.clone()).is_err(),
        "premise: not UTF-8"
    );
    assert_fd_journal_kept("not UTF-8", bad, 3);
}

/// A journal kept in place (it could not be moved aside) is never written over by a contact,
/// and the contacts of this session still cross a Run/S&P rebuild, in memory.
#[test]
fn a_field_day_journal_kept_in_place_is_never_written_over_and_the_session_survives_a_rebuild() {
    let dir = scratch("fd-stuck");
    let path = dir.join(FD_JOURNAL);
    let bytes = b"<EOH>\n<CALL:5>K1ABC <BAND:3>20m <MODE:2>CW".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    take_every_aside_name(&dir, FD_JOURNAL);
    let kept_where = tempo_core::keep_aside::keep_aside("fieldDay", &path, NOW, "cut short");
    assert!(kept_where.kept_in_place, "premise: left in place");

    let mut e = fd_engine(&path);
    log_fd(&mut e, "K9NEW");
    log_fd(&mut e, "W8ONE");
    e.set_mode("fieldday-sp").expect("the rebuild");
    e.journal_mark().wait();
    assert_eq!(
        std::fs::read(&path).ok(),
        Some(bytes),
        "the unreadable journal must never be written over"
    );
    assert_eq!(
        fd_count(&e),
        2,
        "this session's contacts cross the rebuild in memory"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A journal from a newer build that only ADDED fields restores as it always has.
#[test]
fn a_field_day_journal_whose_only_news_is_fields_restores_as_it_always_has() {
    let dir = scratch("fd-fields");
    let path = dir.join(FD_JOURNAL);
    let text = String::from_utf8(a_field_day_journal())
        .unwrap()
        .replace("<EOR>", "<APP_NEXUS_FUTURE:3>abc <EOR>");
    assert!(text.contains("APP_NEXUS_FUTURE"));
    std::fs::write(&path, &text).unwrap();
    let e = fd_engine(&path);
    assert_eq!(
        (fd_count(&e), kept_under(&e, &dir)),
        (3, vec![]),
        "restored as it always has, with nothing to say"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// No journal at all is a first run.
#[test]
fn no_field_day_journal_is_a_first_run() {
    let dir = scratch("fd-none");
    let e = fd_engine(&dir.join(FD_JOURNAL));
    assert_eq!(
        (fd_count(&e), kept_under(&e, &dir)),
        (0, vec![]),
        "no journal: no contacts and nothing to say"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// ── js8_station.json: the JS8 inbox ──────────────────────────────────────────────────────────

/// A JS8 station journal as this build writes it: one stored message from W1AW.
fn a_js8_journal() -> String {
    let at_ms = tempo_core::timing::now_unix_ms() as u64;
    format!(
        r#"{{"inbox":[{{"id":1,"from":"W1AW","to":"K2DEF","text":"FRIDAY CONTACT","path":[],"state":"store","atMs":{at_ms},"freqHz":1750.0,"snrDb":-10}}],"heard":[],"allcallReplied":[],"nextInboxId":2}}"#
    )
}

fn js8_engine_on(path: &Path) -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_js8_journal_path(path.to_path_buf());
    e
}

fn inbox_from(e: &Engine) -> Vec<String> {
    e.js8_state().inbox.iter().map(|m| m.from.clone()).collect()
}

/// The inbox changes (a message is stored, then read), as the radio loop and the operator
/// change it: the journal is written.
fn change_the_inbox(e: &mut Engine) {
    e.js8_load_journal(&a_js8_journal());
    e.js8_inbox_mark(1, ::js8::proto::station::InboxState::Read)
        .expect("the stored message");
    e.journal_mark().wait();
}

fn assert_js8_kept(what: &str, bytes: Vec<u8>) {
    let dir = scratch("js8");
    let path = dir.join("js8_station.json");
    std::fs::write(&path, &bytes).unwrap();

    let mut e = js8_engine_on(&path);
    e.restore_js8_journal(NOW);
    assert_eq!(
        inbox_from(&e),
        Vec::<String>::new(),
        "{what}: nothing is read out of it"
    );
    change_the_inbox(&mut e);

    assert_eq!(
        holding(&dir, &bytes),
        vec![aside(&dir, "js8_station.json")],
        "{what}: the unreadable journal must survive the next inbox change, byte for byte, moved aside"
    );
    assert_eq!(
        kept_under(&e, &dir),
        vec![kept("js8Inbox", &aside(&dir, "js8_station.json"), false)],
        "{what}: the screen is told where it is"
    );
    let mut relaunched = js8_engine_on(&path);
    relaunched.restore_js8_journal(NOW + 1);
    assert_eq!(
        inbox_from(&relaunched),
        vec!["W1AW".to_string()],
        "{what}: the new journal is this build's"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_js8_journal_cut_short_is_kept_and_never_written_over() {
    let whole = a_js8_journal().into_bytes();
    assert_js8_kept("cut short", whole[..whole.len() * 3 / 5].to_vec());
}

/// A value this build does not know: an inbox state from a later build.
#[test]
fn a_js8_journal_with_a_value_this_build_does_not_know_is_kept_and_never_written_over() {
    let unknown = a_js8_journal().replacen("\"state\":\"store\"", "\"state\":\"forwarded\"", 1);
    assert!(
        unknown.contains("forwarded"),
        "the fixture must carry the unknown value"
    );
    assert_js8_kept("an unknown value", unknown.into_bytes());
}

#[test]
fn a_js8_journal_that_cannot_be_moved_is_never_written_over() {
    let dir = scratch("js8-stuck");
    let path = dir.join("js8_station.json");
    let bytes = b"{\"inbox\":[{\"id\":1,\"from\":\"W1AW\"".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    take_every_aside_name(&dir, "js8_station.json");

    let mut e = js8_engine_on(&path);
    e.restore_js8_journal(NOW);
    change_the_inbox(&mut e);
    assert_eq!(
        std::fs::read(&path).ok(),
        Some(bytes),
        "the unreadable journal must never be written over"
    );
    assert_eq!(
        kept_under(&e, &dir),
        vec![kept("js8Inbox", &path, true)],
        "the screen is told it was left in place"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_js8_journal_whose_only_news_is_fields_restores_as_it_always_has() {
    let dir = scratch("js8-fields");
    let path = dir.join("js8_station.json");
    let journal = a_js8_journal()
        .replacen("{\"inbox\"", "{\"aNewField\":7,\"inbox\"", 1)
        .replacen("\"snrDb\"", "\"aNewEntryField\":true,\"snrDb\"", 1);
    assert!(journal.contains("aNewEntryField"));
    std::fs::write(&path, &journal).unwrap();
    let mut e = js8_engine_on(&path);
    e.restore_js8_journal(NOW);
    assert_eq!(
        (inbox_from(&e), kept_under(&e, &dir)),
        (vec!["W1AW".to_string()], vec![]),
        "restored as it always has, with nothing to say"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn no_js8_journal_is_a_first_run() {
    let dir = scratch("js8-none");
    let mut e = js8_engine_on(&dir.join("js8_station.json"));
    e.restore_js8_journal(NOW);
    assert_eq!(
        (inbox_from(&e), kept_under(&e, &dir)),
        (Vec::<String>::new(), vec![]),
        "no journal: an empty inbox and nothing to say"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// ── pending_msgs.json: the Tempo messages waiting to send ────────────────────────────────────

fn msgs_engine_on(path: &Path) -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_pending_msgs_path(path.to_path_buf());
    e
}

fn waiting_to(e: &Engine) -> Vec<String> {
    e.app
        .export_pending()
        .iter()
        .map(|p| p.to.clone())
        .collect()
}

/// Send a message and wait for the queue's journal write, as the radio loop's queue changes do.
fn send(e: &mut Engine, to: &str) {
    e.send_message(to, "see you on 20m");
    e.journal_mark().wait();
}

/// The queue's journal as this build writes it: one message to W1ABC waiting to send.
fn a_pending_msgs_journal() -> String {
    let dir = scratch("msgsmade");
    let path = dir.join("pending_msgs.json");
    let mut e = msgs_engine_on(&path);
    send(&mut e, "W1ABC");
    let text = std::fs::read_to_string(&path).expect("the queue is journaled");
    std::fs::remove_dir_all(&dir).unwrap();
    text
}

fn assert_pending_msgs_kept(what: &str, bytes: Vec<u8>) {
    let dir = scratch("msgs");
    let path = dir.join("pending_msgs.json");
    std::fs::write(&path, &bytes).unwrap();

    let mut e = msgs_engine_on(&path);
    e.restore_pending_msgs(NOW);
    assert_eq!(
        waiting_to(&e),
        Vec::<String>::new(),
        "{what}: nothing is read out of it"
    );
    send(&mut e, "K1NEW");

    assert_eq!(
        holding(&dir, &bytes),
        vec![aside(&dir, "pending_msgs.json")],
        "{what}: the unreadable queue must survive the next message sent, byte for byte, moved aside"
    );
    assert_eq!(
        kept_under(&e, &dir),
        vec![kept(
            "pendingMsgs",
            &aside(&dir, "pending_msgs.json"),
            false
        )],
        "{what}: the screen is told where it is"
    );
    let mut relaunched = msgs_engine_on(&path);
    relaunched.restore_pending_msgs(NOW + 1);
    assert_eq!(
        waiting_to(&relaunched),
        vec!["K1NEW".to_string()],
        "{what}: the new journal is this build's"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pending_msgs_journal_cut_short_is_kept_and_never_written_over() {
    let whole = a_pending_msgs_journal().into_bytes();
    assert_pending_msgs_kept("cut short", whole[..whole.len() * 3 / 5].to_vec());
}

/// A value this build cannot hold: a message id wider than the one character this build keeps.
#[test]
fn a_pending_msgs_journal_with_a_value_this_build_cannot_hold_is_kept_and_never_written_over() {
    let journal = a_pending_msgs_journal();
    let at = journal.find("\"id\":\"").expect("the id") + "\"id\":\"".len();
    let wider = format!("{}ab{}", &journal[..at], &journal[at + 1..]);
    assert!(
        wider.contains("\"id\":\"ab"),
        "the fixture must carry the wider id: {wider}"
    );
    assert_pending_msgs_kept("a value this build cannot hold", wider.into_bytes());
}

/// A queue that cannot be moved aside is never written over, nor removed when the queue
/// empties.
#[test]
fn a_pending_msgs_journal_that_cannot_be_moved_is_never_written_over_or_removed() {
    let dir = scratch("msgs-stuck");
    let path = dir.join("pending_msgs.json");
    let bytes = b"[{\"to\":\"W1ABC\",\"text\":\"see".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    take_every_aside_name(&dir, "pending_msgs.json");

    let mut e = msgs_engine_on(&path);
    e.restore_pending_msgs(NOW);
    send(&mut e, "K1NEW");
    e.app.restore_pending(Vec::new());
    e.persist_pending_msgs();
    e.journal_mark().wait();
    assert_eq!(
        std::fs::read(&path).ok(),
        Some(bytes),
        "the unreadable queue must never be written over or removed"
    );
    assert_eq!(
        kept_under(&e, &dir),
        vec![kept("pendingMsgs", &path, true)],
        "the screen is told it was left in place"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pending_msgs_journal_whose_only_news_is_fields_restores_as_it_always_has() {
    let dir = scratch("msgs-fields");
    let path = dir.join("pending_msgs.json");
    let journal = a_pending_msgs_journal().replacen("{", "{\"aNewField\":7,", 1);
    assert!(journal.contains("aNewField"));
    std::fs::write(&path, &journal).unwrap();
    let mut e = msgs_engine_on(&path);
    e.restore_pending_msgs(NOW);
    assert_eq!(
        (waiting_to(&e), kept_under(&e, &dir)),
        (vec!["W1ABC".to_string()], vec![]),
        "restored as it always has, with nothing to say"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn no_pending_msgs_journal_is_a_first_run() {
    let dir = scratch("msgs-none");
    let mut e = msgs_engine_on(&dir.join("pending_msgs.json"));
    e.restore_pending_msgs(NOW);
    assert_eq!(
        (waiting_to(&e), kept_under(&e, &dir)),
        (Vec::<String>::new(), vec![]),
        "no journal: nothing waiting and nothing to say"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
