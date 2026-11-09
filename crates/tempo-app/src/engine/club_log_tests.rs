//! The host's club log as its own screen lists it (`Engine::fd_club_log_newest`): the host's
//! alone, as a joined laptop holds no rows; the club's contacts newest first, each with the
//! position that logged it as the host's board names it, no more than
//! `fdevent::CLUB_LOG_SHOWN` of them with the club's count beside them; and, read from the file
//! the operator saves, the Club ADIF's contacts by value.

use super::*;
use crate::logstore::tests::Dir;
use tempo_net::fdsync::{WireQso, PROTO_VERSION};

/// 1800Z, Saturday 27 June 2026: ARRL Field Day's first minute.
const T0: u64 = 1_782_583_200;

/// The key hash a test JOIN under `pos` carries, as the bridge makes one: the hash of a key
/// generated per run, so no test holds a real one.
fn key_hash(pos: &str) -> String {
    use std::hash::BuildHasher;
    static SEED: std::sync::OnceLock<std::hash::RandomState> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(std::hash::RandomState::new);
    let key: String = (0..4u8)
        .map(|i| format!("{:016x}", seed.hash_one((pos, i))))
        .collect();
    crate::fdevent::sha256_hex(&key)
}

/// W9ABC, 3A WI, hosting ARRL Field Day from `d`, with its own position (HQ) and two laptops
/// joined: the CW tent and the SSB tent.
fn host(d: &Dir) -> Engine {
    let mut e = Engine::new("W9ABC", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_host_enable = true;
    s.fd_position_id = "aaaa0001".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("Field Day runs");
    e.fd_host_start(d.0.join("fd_event_test.jsonl"))
        .expect("hosting");
    for (pos, name) in [
        ("aaaa0001", "HQ"),
        ("bbbb0002", "CW tent"),
        ("cccc0003", "SSB tent"),
    ] {
        join(&mut e, pos, name);
    }
    e
}

fn join(e: &mut Engine, pos: &str, name: &str) {
    e.fd_club_join(
        PROTO_VERSION,
        pos,
        name,
        "W9ABC",
        "arrlfd",
        "",
        &key_hash(pos),
    )
    .unwrap_or_else(|why| panic!("{pos} turned away: {why}"));
}

/// One Field Day contact as a position streams it: the class and section they sent, as data,
/// the position's own 3A WI, and the time it was made. A digital contact went out as FT8.
fn row(
    pos: &str,
    seq: u64,
    call: &str,
    (class, section): (&str, &str),
    band: &str,
    mode: &str,
    when: u64,
) -> WireQso {
    let spec = tempo_core::contest::field_day(tempo_core::fieldday::FdEvent::ArrlFd);
    let fields = |class: &str, section: &str| {
        crate::fdevent::to_wire_fields(
            &[("CLASS", class), ("SECTION", section)]
                .iter()
                .filter_map(|(k, v)| spec.value(k, v))
                .collect::<Vec<_>>(),
        )
    };
    WireQso {
        pos: pos.into(),
        seq,
        call: call.into(),
        class: String::new(),
        sect: String::new(),
        ex: fields(class, section),
        mex: fields("3A", "WI"),
        band: band.into(),
        mode: mode.into(),
        sub: if mode == "DIG" {
            "FT8".into()
        } else {
            String::new()
        },
        when,
        op: String::new(),
        sat: String::new(),
        sat_fm: false,
    }
}

/// A distinct call sign for contact `i`: W0AAA, W1AAB, …
fn call(i: u64) -> String {
    let letter = |n: u64| char::from(b'A' + (n % 26) as u8);
    format!("W{}A{}{}", i % 10, letter(i / 26), letter(i))
}

/// ⭐ **The club log is the host's alone.** A laptop that joined a club holds its counts and
/// keys, never its rows ([`crate::fdevent::ClubMirror`]), so it lists nothing; the host lists
/// what its club merged. CONTROL: once it stops hosting, the host lists nothing either.
#[test]
fn the_club_log_is_the_hosts_alone() {
    let mut position = Engine::new("W9ABC", "EN61", 0);
    let mut s = position.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_join_addr = "192.168.1.10:42073".into();
    position.apply_settings(s);
    position.set_mode("fieldday-sp").expect("Field Day runs");
    assert!(position.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    assert_eq!(
        position.fd_club_log_newest(),
        None,
        "a joined laptop lists nothing"
    );

    let d = Dir::new("club-log-host");
    let mut e = host(&d);
    e.fd_club_merge(
        0,
        &row("bbbb0002", 1, "K1ABC", ("2A", "EMA"), "40m", "CW", T0),
    )
    .expect("merged");
    let log = e.fd_club_log_newest().expect("the host lists its club log");
    assert_eq!((log.total, log.rows.len()), (1, 1));
    e.fd_host_stop();
    assert_eq!(e.fd_club_log_newest(), None, "CONTROL: no longer hosting");
}

/// ⭐ **Newest first, no more than `CLUB_LOG_SHOWN`, with the club's count beside them, and each
/// with the position that logged it under the name the host's board shows.** Two positions
/// named alike read apart, as on the board; an unnamed one sends no name, and never its id.
#[test]
fn the_hosts_club_log_lists_the_newest_contacts_by_the_names_its_board_shows() {
    let d = Dir::new("club-log-newest");
    let mut e = host(&d);
    // A fourth laptop named as the CW tent is, and a fifth with no name at all.
    join(&mut e, "dddd0004", "CW tent");
    join(&mut e, "eeee0005", "");
    let positions = ["aaaa0001", "bbbb0002", "cccc0003", "dddd0004", "eeee0005"];
    let shown = crate::fdevent::CLUB_LOG_SHOWN as u64;
    for i in 0..shown + 5 {
        let pos = positions[(i % 5) as usize];
        let q = row(pos, i / 5 + 1, &call(i), ("1A", "WI"), "20m", "CW", T0 + i);
        e.fd_club_merge(0, &q).expect("merged");
    }
    let log = e.fd_club_log_newest().expect("hosting");
    assert_eq!(log.total, shown + 5, "the club's count: every contact");
    let times: Vec<u64> = log.rows.iter().map(|r| r.when_unix).collect();
    let newest: Vec<u64> = (5..shown + 5).rev().map(|i| T0 + i).collect();
    assert_eq!(times, newest, "the newest CLUB_LOG_SHOWN, newest first");
    let names: std::collections::BTreeMap<&str, &str> = log
        .rows
        .iter()
        .map(|r| (r.posid.as_str(), r.pos_name.as_str()))
        .collect();
    assert_eq!(
        names.into_iter().collect::<Vec<_>>(),
        [
            ("aaaa0001", "HQ"),
            ("bbbb0002", "CW tent"),
            ("cccc0003", "SSB tent"),
            ("dddd0004", "CW tent (2)"),
            ("eeee0005", ""),
        ]
    );
    // …each the name its line on the host's own board has.
    let board = e
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .expect("the host's club block")
        .board;
    for r in &log.rows {
        let line = board
            .iter()
            .find(|b| b.posid == r.posid)
            .unwrap_or_else(|| panic!("{} has a line on the board", r.posid));
        assert_eq!(r.pos_name, line.pos_name, "{}", r.posid);
    }
}

/// The file's own on-air mode for a contact: the submode it recorded, else its class's, as the
/// contest log writes it (`LoggedQso::recorded_mode`).
fn on_air(class: &str, submode: &str) -> String {
    match (submode, class) {
        ("", "CW") => "CW".into(),
        ("", "PH") => "SSB".into(),
        ("", _) => "FT8".into(),
        (sub, _) => sub.into(),
    }
}

/// `YYYYMMDD` and `HHMMSS` (UTC), as the file writes a contact's time, in Unix seconds — Howard
/// Hinnant's days-from-civil.
fn unix_of(date: &str, time: &str) -> u64 {
    let n = |s: &str| s.parse::<i64>().expect("digits");
    let (y, m, d) = (n(&date[..4]), n(&date[4..6]), n(&date[6..8]));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days * 86_400 + n(&time[..2]) * 3600 + n(&time[2..4]) * 60 + n(&time[4..6])) as u64
}

/// Each record of an ADIF text as the `(tag, value)` pairs it writes, read by the length each
/// field declares: the file itself, nothing normalised.
fn adif_records(text: &str) -> Vec<Vec<(String, String)>> {
    let body = &text[text.find("<EOH>").expect("an ADIF header") + "<EOH>".len()..];
    body.split("<EOR>")
        .filter(|rec| rec.contains('<'))
        .map(|rec| {
            let mut fields = Vec::new();
            let mut rest = rec;
            while let Some(open) = rest.find('<') {
                let close = open + rest[open..].find('>').expect("a closed tag");
                let (tag, len) = rest[open + 1..close]
                    .split_once(':')
                    .expect("a field with a length");
                let len: usize = len.parse().expect("a length");
                let value = &rest[close + 1..close + 1 + len];
                fields.push((tag.to_ascii_uppercase(), value.to_string()));
                rest = &rest[close + 1 + len..];
            }
            fields
        })
        .collect()
}

/// One contact as the list and the file can both say it: call, band, on-air mode, time, class,
/// section.
type Contact = (String, String, String, u64, String, String);

/// ⭐ **THE HOST'S LIST IS THE CLUB ADIF, BY VALUE**, read from the file the operator saves.
/// Two laptops besides the host's own, a repeat and a removed position: the CW tent and the SSB
/// tent log; the SSB tent works K1ABC, whom the CW tent holds on 40 m CW (the repeat), and sends
/// that row twice (held once); the host removes the SSB tent once its contacts are in, and the
/// contact it sends after that is turned away. Every contact the club holds is listed once, the
/// removed position's with its name; the ones not marked as duplicates are the file's QSOs field
/// for field (call, band, mode, the time to the second, class and section), and the marked one
/// is the contact the file leaves out. CONTROL: the file holds four, so the comparison cannot
/// pass empty, and its times are read right (the first is 1801Z on 27 June 2026).
#[test]
fn the_hosts_club_log_is_the_club_adifs_contacts_by_value() {
    let d = Dir::new("club-log-adif");
    let mut e = host(&d);
    for q in [
        row("aaaa0001", 1, "W1AW", ("1D", "CT"), "20m", "DIG", T0 + 60),
        row("bbbb0002", 1, "K1ABC", ("2A", "EMA"), "40m", "CW", T0 + 125),
        row("cccc0003", 1, "N0XYZ", ("3A", "MN"), "20m", "PH", T0 + 190),
        // The repeat: the CW tent's K1ABC on 40 m CW, worked again at the SSB tent…
        row("cccc0003", 2, "K1ABC", ("2A", "EMA"), "40m", "CW", T0 + 247),
        // …which sends that row a second time.
        row("cccc0003", 2, "K1ABC", ("2A", "EMA"), "40m", "CW", T0 + 247),
        row("bbbb0002", 2, "W2DEF", ("1E", "ENY"), "15m", "CW", T0 + 301),
    ] {
        e.fd_club_merge(0, &q).expect("merged");
    }
    e.fd_club_remove_position("cccc0003").expect("removed");
    let late = row("cccc0003", 3, "W3GHI", ("1A", "EPA"), "20m", "PH", T0 + 360);
    assert!(
        e.fd_club_merge(0, &late).is_err(),
        "a removed position's next contact is turned away"
    );

    let log = e.fd_club_log_newest().expect("hosting");
    let held = e.fd_club_log().expect("hosting").rows().len();
    assert_eq!(
        (log.total, log.rows.len(), held),
        (5, 5, 5),
        "every contact the club holds, once"
    );
    assert!(
        log.rows
            .iter()
            .any(|r| r.call == "N0XYZ" && r.pos_name == "SSB tent"),
        "the removed position's contacts stay listed, with its name"
    );

    let adif = e.fd_club_export(false).expect("the Club ADIF");
    let records = adif_records(&adif);
    let field = |rec: &[(String, String)], tag: &str| -> String {
        rec.iter()
            .find(|(t, _)| t == tag)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    assert!(
        records.iter().all(|rec| field(rec, "SUBMODE").is_empty()),
        "fixture: every mode here is written as a MODE alone"
    );
    assert!(
        records.iter().any(|rec| field(rec, "CALL") == "W1AW"
            && field(rec, "QSO_DATE") == "20260627"
            && field(rec, "TIME_ON") == "180100"),
        "CONTROL: the file's times are read as it writes them: {adif}"
    );
    let mut file: Vec<Contact> = records
        .iter()
        .map(|rec| {
            (
                field(rec, "CALL"),
                field(rec, "BAND"),
                field(rec, "MODE"),
                unix_of(&field(rec, "QSO_DATE"), &field(rec, "TIME_ON")),
                field(rec, "CLASS"),
                field(rec, "ARRL_SECT"),
            )
        })
        .collect();
    let contact = |r: &crate::dto::FdClubLogRow| -> Contact {
        (
            r.call.clone(),
            r.band.clone(),
            on_air(&r.mode, &r.submode),
            r.when_unix,
            r.class.clone(),
            r.section.clone(),
        )
    };
    let (marked, unmarked): (Vec<_>, Vec<_>) = log.rows.iter().partition(|r| r.dupe);
    let mut listed: Vec<Contact> = unmarked.iter().map(|r| contact(r)).collect();
    file.sort();
    listed.sort();
    assert_eq!(file.len(), 4, "CONTROL: the file holds four contacts");
    assert_eq!(
        listed, file,
        "the unmarked contacts are the file's, field for field"
    );
    assert_eq!(
        marked
            .iter()
            .map(|r| (r.posid.as_str(), r.call.as_str(), r.when_unix))
            .collect::<Vec<_>>(),
        [("cccc0003", "K1ABC", T0 + 247)],
        "the repeat is the one marked"
    );
    assert!(
        !file.contains(&contact(marked[0])),
        "…and the one the file leaves out"
    );
}

/// ⭐ **The host's club journal reaches the disk for every row, take-back and put-back a laptop
/// sends**: the bridge syncs it ([`crate::fdevent::ClubJournal::sync`]) once it has released the
/// Engine lock, before the ack goes back, so a row the laptop sees acked survives a hard power
/// cut on the host. Seen through the journal's test seam, by the journal's own path.
#[test]
fn the_bridge_syncs_the_club_journal_for_every_row_it_merges() {
    use crate::fdevent::club_journal_seam::count;
    use tempo_net::fdsync::ClubBackend;
    let d = Dir::new("club-journal-sync");
    let path = d.0.join("fd_event_test.jsonl");
    let e = host(&d);
    let bridge = crate::fdbridge::EngineClubBackend(std::sync::Arc::new(std::sync::Mutex::new(e)));
    assert_eq!(count(&path), 0, "CONTROL: nothing synced before a row");
    let q = row("bbbb0002", 1, "K1ABC", ("2A", "EMA"), "40m", "CW", T0);
    assert_eq!(bridge.merge(0, &q), Ok(1), "merged and acked");
    assert_eq!(count(&path), 1, "the merge syncs the journal");
    assert!(
        std::fs::read_to_string(&path).unwrap().contains("K1ABC"),
        "CONTROL: the synced journal holds the row"
    );
    bridge.retract(0, "bbbb0002", 1).expect("taken back");
    assert_eq!(count(&path), 2, "a take-back syncs it");
    bridge.restore(0, "bbbb0002", 1).expect("put back");
    assert_eq!(count(&path), 3, "a put-back syncs it");
}

/// The sync is a wait on the disk, so it is never made under the Engine lock: a debug build
/// stops one that is (the logbook's fence, `off_engine_lock`).
#[test]
#[should_panic(expected = "a sync of the club journal ran while this thread holds the Engine lock")]
fn a_club_journal_sync_under_the_engine_lock_trips_the_fence() {
    let d = Dir::new("club-journal-fence");
    let shared = std::sync::Arc::new(std::sync::Mutex::new(host(&d)));
    let e = engine_lock(&shared);
    e.fd_club_journal().expect("hosting journals").sync();
}
