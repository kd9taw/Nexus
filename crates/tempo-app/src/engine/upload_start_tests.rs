//! ⭐ **Saving the first login for QRZ Logbook, ClubLog or eQSL never sends the old log unasked.**
//! Saving a login turns that service's upload on and runs a catch-up of every contact not yet sent
//! there, and an ADIF import marks nothing as sent: an operator who imported 10,000 contacts from
//! another program and then saved a QRZ key started re-sending the whole old log. The first login
//! saved for a service now asks "Send N contacts not yet sent to <service>?", and the default answer,
//! "Only contacts made from now on", puts a start point on that service: a contact made before it
//! is never sent there automatically — not by this save, not by a later one, not after a restart,
//! not from what was waiting to go. "Send them" is what a save did before. A login saved over one
//! already stored asks nothing and changes nothing, so a station that had its logins before sees no
//! difference.

use super::*;
use crate::station::UploadStart;

/// When the operator answers the question in these tests: 2026-09-21 13:46:40 UTC.
const ANSWERED_AT: u64 = 1_790_000_000;

/// The three services with a per-contact upload record, and so a start point.
const BOOK: u8 = upload_legs::QRZ | upload_legs::CLUBLOG | upload_legs::EQSL;

/// Three contacts from 2019, as an import from another program brings them: sent nowhere.
const OLD_LOG: &str = "<EOH>\n\
    <CALL:5>W1OLD<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20190601<TIME_ON:6>120000<EOR>\n\
    <CALL:5>W2OLD<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20190602<TIME_ON:6>120000<EOR>\n\
    <CALL:5>W3OLD<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20190603<TIME_ON:6>120000<EOR>\n";

/// A station with the old log imported, and one contact logged this session before the login was
/// saved (made a minute before the answer), still waiting in the upload queue as a contact logged
/// while every upload is off waits.
fn station_with_an_imported_log() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let (added, ..) = e.import_adif(OLD_LOG);
    assert_eq!(added, 3, "premise: the old log is in");
    assert!(
        e.take_pending_uploads().is_empty(),
        "premise: an import queues nothing"
    );
    let mut waiting = e.qso_record("K9WAIT".into(), None, None);
    waiting.when_unix = ANSWERED_AT - 60;
    e.log_qso(waiting);
    e
}

/// What is waiting to go, by call and owed services; the queue is emptied.
fn waiting(e: &mut Engine) -> Vec<(String, u8)> {
    e.take_pending_uploads()
        .into_iter()
        .map(|p| (p.rec.call, p.legs))
        .collect()
}

/// What a catch-up on `legs` sends (the sweep a saved login runs), by call and owed services.
fn swept(e: &mut Engine, legs: u8) -> Vec<(String, u8)> {
    let _ = e.take_pending_uploads();
    e.requeue_failed_uploads(legs);
    waiting(e)
}

fn owed(calls: &[&str], legs: u8) -> Vec<(String, u8)> {
    calls.iter().map(|c| (c.to_string(), legs)).collect()
}

/// THE DEFAULT. "Only contacts made from now on": nothing made before the answer goes to QRZ, not
/// by the catch-up and not from the queue, while every other service keeps what it is owed. A
/// contact made after the answer goes out as any contact does, and is owed to a later catch-up.
#[test]
fn the_default_answer_sends_nothing_old_to_the_service() {
    let mut e = station_with_an_imported_log();
    let saved = e.first_login_saved(upload_legs::QRZ, true, false, ANSWERED_AT);
    assert_eq!(
        saved.map(|s| UploadStart::of(&s)),
        Some(UploadStart {
            qrz: Some(ANSWERED_AT),
            ..UploadStart::default()
        }),
        "the answer puts QRZ's start point at the moment it was given, and only QRZ's"
    );
    assert_eq!(
        waiting(&mut e),
        owed(&["K9WAIT"], upload_legs::ALL & !upload_legs::QRZ),
        "the contact waiting to go keeps every service but QRZ"
    );
    assert_eq!(
        swept(&mut e, upload_legs::QRZ),
        owed(&[], 0),
        "the catch-up the save runs finds nothing old for QRZ"
    );
    assert_eq!(
        swept(&mut e, upload_legs::CLUBLOG),
        owed(&["W1OLD", "W2OLD", "W3OLD", "K9WAIT"], upload_legs::CLUBLOG),
        "QRZ's answer leaves ClubLog as it was"
    );

    let mut new = e.qso_record("K1NEW".into(), None, None);
    new.when_unix = ANSWERED_AT + 60;
    e.log_qso(new);
    assert_eq!(
        waiting(&mut e),
        owed(&["K1NEW"], upload_legs::ALL),
        "a contact made after the answer goes to QRZ as any contact does"
    );
    assert_eq!(
        swept(&mut e, upload_legs::QRZ),
        owed(&["K1NEW"], upload_legs::QRZ),
        "and, not yet sent, is owed to the next catch-up"
    );
}

/// "Send them": what saving a login did before the question existed. No start point, the queue
/// untouched, and the catch-up sends every contact not yet sent there.
#[test]
fn send_them_is_what_a_save_did_before() {
    let mut e = station_with_an_imported_log();
    let saved = e.first_login_saved(upload_legs::QRZ, true, true, ANSWERED_AT);
    assert_eq!(
        saved.map(|s| UploadStart::of(&s)),
        Some(UploadStart::default())
    );
    assert_eq!(
        waiting(&mut e),
        owed(&["K9WAIT"], upload_legs::ALL),
        "the waiting contact still owes QRZ"
    );
    assert_eq!(
        swept(&mut e, upload_legs::QRZ),
        owed(&["W1OLD", "W2OLD", "W3OLD", "K9WAIT"], upload_legs::QRZ),
        "the whole backlog is sent, as before"
    );

    // A login cleared and saved again is a first login again: "Send them" now clears the start
    // point an earlier "from now on" put there.
    e.set_upload_start(upload_legs::QRZ, Some(ANSWERED_AT));
    assert!(swept(&mut e, upload_legs::QRZ).is_empty(), "premise");
    e.first_login_saved(upload_legs::QRZ, true, true, ANSWERED_AT + 600);
    assert_eq!(
        swept(&mut e, upload_legs::QRZ),
        owed(&["W1OLD", "W2OLD", "W3OLD", "K9WAIT"], upload_legs::QRZ)
    );
}

/// A login saved over one already stored is no first login: whatever the answer says, nothing
/// changes. Not the start point (none, or one already there), not the queue, not the catch-up.
#[test]
fn a_login_saved_over_a_stored_one_changes_nothing() {
    for send_old in [false, true] {
        let mut e = station_with_an_imported_log();
        assert_eq!(
            e.first_login_saved(BOOK, false, send_old, ANSWERED_AT),
            None,
            "nothing to persist"
        );
        assert_eq!(UploadStart::of(e.settings()), UploadStart::default());
        assert_eq!(waiting(&mut e), owed(&["K9WAIT"], upload_legs::ALL));
        for leg in [upload_legs::QRZ, upload_legs::CLUBLOG, upload_legs::EQSL] {
            assert_eq!(
                swept(&mut e, leg),
                owed(&["W1OLD", "W2OLD", "W3OLD", "K9WAIT"], leg),
                "leg {leg:#b}, send_old {send_old}"
            );
        }

        // …and a start point already there stays where it was.
        e.set_upload_start(upload_legs::EQSL, Some(ANSWERED_AT - 3_600));
        assert_eq!(
            e.first_login_saved(upload_legs::EQSL, false, send_old, ANSWERED_AT),
            None
        );
        assert_eq!(e.settings().eqsl_upload_from, Some(ANSWERED_AT - 3_600));
    }
}

/// ⭐ AN EXISTING STATION SEES NO CHANGE. A full settings file stored by an earlier build, logins
/// saved and all three uploads on, loads with no start point, and saving a login over a stored one
/// sends every contact not yet sent, exactly as before; the file saves back without a new key.
#[test]
fn an_existing_stations_settings_file_keeps_every_upload_as_it_was() {
    let text = include_str!("../../tests/fixtures/settings-before-tci-two-radios.json");
    assert!(
        !text.contains("UploadFrom"),
        "premise: the file predates the start points"
    );
    let mut stored = text.to_string();
    for key in ["qrzLogbookUpload", "clublogUpload", "eqslUpload"] {
        let off = format!("\"{key}\": false");
        assert_eq!(
            stored.matches(&off).count(),
            1,
            "premise: {key} is in the file"
        );
        stored = stored.replace(&off, &format!("\"{key}\": true"));
    }
    let dir = std::env::temp_dir().join(format!(
        "nexus-upload-start-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(&path, &stored).unwrap();

    let loaded = Settings::load(&path);
    assert!(
        loaded.qrz_logbook_upload && loaded.clublog_upload && loaded.eqsl_upload,
        "premise: every upload is on"
    );
    assert_eq!(UploadStart::of(&loaded), UploadStart::default());

    let mut e = Engine::with_settings(loaded.clone());
    let (added, ..) = e.import_adif(OLD_LOG);
    assert_eq!(added, 3);
    for leg in [upload_legs::QRZ, upload_legs::CLUBLOG, upload_legs::EQSL] {
        assert_eq!(e.first_login_saved(leg, false, false, ANSWERED_AT), None);
        assert_eq!(
            swept(&mut e, leg),
            owed(&["W1OLD", "W2OLD", "W3OLD"], leg),
            "leg {leg:#b}: the save's catch-up sends what it always did"
        );
    }

    e.settings().save(&path).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(
        !saved.contains("UploadFrom"),
        "saving an existing station's file writes no start point into it"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A restart keeps the answer: the start point is in the settings file, and the catch-up of a login
/// saved after the restart still finds nothing old.
#[test]
fn a_restart_keeps_the_start_point() {
    let mut e = station_with_an_imported_log();
    let saved = e
        .first_login_saved(upload_legs::CLUBLOG, true, false, ANSWERED_AT)
        .expect("a first login changes the settings");
    let dir = std::env::temp_dir().join(format!(
        "nexus-upload-restart-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    saved.save(&path).unwrap();

    let reloaded = Settings::load(&path);
    assert_eq!(reloaded.clublog_upload_from, Some(ANSWERED_AT));
    let mut after = Engine::with_settings(reloaded);
    after.import_adif(OLD_LOG);
    assert!(
        swept(&mut after, upload_legs::CLUBLOG).is_empty(),
        "after a restart, a save's catch-up still sends nothing old to ClubLog"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The hourly QRZ sync adds the contacts QRZ holds that the log lacks. It queues none of them, and
/// with the start points set, a later catch-up sends none of them either.
#[test]
fn the_hourly_qrz_sync_sends_nothing_old() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_upload_start(BOOK, Some(ANSWERED_AT));
    let (added, _) = e.merge_qrz_report(
        "<EOH>\n<CALL:5>K5NEW<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20231114<TIME_ON:6>010000\
         <APP_QRZLOG_STATUS:1>C<EOR>\n",
    );
    assert_eq!(added, 1, "premise: the sync added a contact");
    assert_eq!(waiting(&mut e), owed(&[], 0), "the sync queues nothing");
    assert_eq!(
        swept(&mut e, BOOK),
        owed(&[], 0),
        "and the catch-up sends nothing old"
    );
}

/// ⛔ A settings form, or a save from another computer driving the station, cannot move a start
/// point: it has one writer, the answer to the first login's question, which is asked only at the
/// station (saving a login is refused from afar). A form read before the answer, carrying no start
/// point, would otherwise put the whole old log back in the catch-up's reach.
#[test]
fn a_settings_save_keeps_the_live_start_points() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_upload_start(upload_legs::QRZ, Some(ANSWERED_AT));
    let mut stale = e.settings().clone();
    stale.qrz_logbook_upload_from = None;
    stale.clublog_upload_from = Some(1);
    stale.eqsl_upload_from = Some(2);
    e.apply_settings(stale);
    assert_eq!(
        UploadStart::of(e.settings()),
        UploadStart {
            qrz: Some(ANSWERED_AT),
            ..UploadStart::default()
        }
    );
}
