//! A station log for the Remote readers' tests (SPEC-2 v3 C18): a launch on a real logbook
//! store by the shipped open path, the same log on the 1.13 path (no store), seeded synthetic
//! logs whose contacts reach every field a Remote reader reads — calls, countries and notes
//! outside ASCII among them, and many contacts in the same second — and the changes the app
//! makes to a log. Each reader's tests hold what it reads from the store to what its code
//! before C18 made of the log in memory.
use std::sync::{Arc, Mutex};

use tempo_app::engine::Engine;
use tempo_core::logbook::sqlite::Resolved;
use tempo_core::logbook::{QsoRecord, UploadDetail, UploadOutcome};

pub(super) const MY_CALL: &str = "KD9TAW";

/// A folder of the test's own, gone with the value.
pub(super) struct Dir(std::path::PathBuf);
impl Dir {
    pub(super) fn new(tag: &str) -> Dir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let p = std::env::temp_dir().join(format!(
            "nexus-remote-log-{tag}-{}-{}-{nanos}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    pub(super) fn log(&self) -> std::path::PathBuf {
        self.0.join("log.adi")
    }
    pub(super) fn db(&self) -> std::path::PathBuf {
        self.0.join("log.sqlite3")
    }
    /// The `log.adi` of a session on the 1.13 path ([`memory`]), beside the store's own.
    pub(super) fn memory_log(&self) -> std::path::PathBuf {
        self.0.join("memory.adi")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A deterministic generator: a failing case is reproducible from its seed alone.
pub(super) struct Gen(pub(super) u64);
impl Gen {
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub(super) fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub(super) fn chance(&mut self, one_in: u64) -> bool {
        self.next().is_multiple_of(one_in)
    }
    pub(super) fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

/// The resolvers the engine fills an insert with — and the fill job, the same functions.
pub(super) fn test_country(call: &str) -> Option<String> {
    propagation::dxcc::resolve(call).map(|i| i.entity.to_string())
}
pub(super) fn test_state(call: &str, _grid: Option<&str>) -> Option<String> {
    call.starts_with('K').then(|| "WI".to_string())
}

/// Callsigns: many entities, a few worked again and again (what recall and the JS8 roster
/// count), portables and compounds, lower case and stray spaces, and calls outside ASCII —
/// `ſ` and `ı` upper-case INTO ASCII in Rust as in JavaScript, which is the fold a store's
/// ASCII-only `call_norm` cannot see.
pub(super) const CALLS: &[&str] = &[
    "W1AW",
    "W1AW",
    "W1AW",
    "K1ABC",
    "K2DEF",
    "DL1ABC",
    "DL2XYZ",
    "JA1AA",
    "JA1ABC",
    "VK2AB",
    "PY2ABC",
    "G4ABC",
    "VE3AB",
    "ZL1OLD",
    "UA3AA",
    "EA8AB",
    "4X1AB",
    "ZS6AB",
    "KP4AA",
    "3D2AB",
    "K1ABC/P",
    "VP2E/K1ABC",
    "w1aw",
    " W1AW ",
    "dl1abc",
    "K1ſAB",
    "K1ıAB",
    "ÅL1AB",
    "Q0ZZZ",
    "OH2ÖÖ",
];

/// One synthetic contact, as ADIF: every field a Remote reader reads (and a passthrough tag
/// that only a whole record carries), `when` given by the caller so ties can be made.
pub(super) fn synthetic(g: &mut Gen, when: u64) -> String {
    let mut f = String::new();
    let mut tag = |name: &str, val: &str| {
        f.push_str(&format!("<{}:{}>{}", name, val.len(), val));
    };
    tag("CALL", g.pick(CALLS));
    let (y, m, d, hh, mm, ss) = tempo_core::logbook::datetime_utc(when);
    tag("QSO_DATE", &format!("{y:04}{m:02}{d:02}"));
    if !g.chance(12) {
        tag("TIME_ON", &format!("{hh:02}{mm:02}{ss:02}"));
    }
    if g.chance(40) {
        tag("BAND", "2m");
        tag("PROP_MODE", "SAT");
        tag("SAT_NAME", "RS-44");
    } else if g.chance(30) {
        tag("BAND", "odd-import");
    } else if !g.chance(25) {
        tag(
            "BAND",
            g.pick(&[
                "160m", "80m", "40m", "30m", "20m", "17m", "15m", "10m", "6m",
            ]),
        );
    }
    if g.chance(3) {
        tag(
            "FREQ",
            g.pick(&["14.074", "7.142", "21.074", "144.300", "0", "5.357"]),
        );
    }
    let mode = g.pick(&[
        "FT8", "FT8", "SSB", "USB", "LSB", "CW", "RTTY", "MFSK", "BPSK31",
    ]);
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
        match g.below(4) {
            0 => tag("GRIDSQUARE", &format!("{grid}ab")),
            1 => tag("GRIDSQUARE", &grid.to_lowercase()),
            _ => tag("GRIDSQUARE", &grid),
        }
    }
    if g.chance(3) {
        tag(
            "COUNTRY",
            g.pick(&[
                "Germany",
                "United States",
                "Österreich",
                "Türkiye",
                "Curaçao",
                "JAPAN",
                "Alaska",
            ]),
        );
    }
    if g.chance(3) {
        tag("STATE", g.pick(&["WI", "IL", "CA", "MA", "ON", "zz"]));
    }
    for (name, one_in, val) in [
        ("LOTW_QSL_RCVD", 4, "Y"),
        ("QSL_RCVD", 12, "Y"),
        ("EQSL_QSL_RCVD", 8, "Y"),
        ("APP_QRZLOG_STATUS", 15, "C"),
        ("CREDIT_GRANTED", 15, "DXCC"),
        ("IOTA", 30, "NA-001"),
        ("NAME", 3, "Hiram"),
        ("NAME", 20, "  "),
        ("COMMENT", 4, "tnx fer QSO"),
        ("COMMENT", 25, "  "),
        ("NOTES", 6, "Größe: 5 W, dipole"),
        ("NOTES", 20, "   "),
        ("APP_OTHER_LOGGER", 5, "xyz"),
    ] {
        if g.chance(one_in) {
            tag(name, val);
        }
    }
    if g.chance(6) {
        tag("MY_SIG", "POTA");
        tag("MY_SIG_INFO", g.pick(&["US-0001", "US-0002", "us-0001"]));
    }
    if g.chance(6) {
        tag("SIG", "POTA");
        tag("SIG_INFO", g.pick(&["US-0003", "US-0004,US-0005"]));
    }
    if g.chance(7) {
        let outcome = g.pick(&["accepted", "pending", "rejected"]);
        tag("APP_TEMPO_UL_QRZ", &format!("{outcome}|1700000000|"));
    }
    f.push_str("<EOR>\n");
    f
}

/// `n` synthetic contacts, many of them in the same second: times are drawn from a range a
/// quarter the size of the log, so ties are common and the log is not in time order.
pub(super) fn synthetic_log(n: usize, seed: u64) -> String {
    let mut g = Gen(seed | 1);
    let mut s = tempo_core::logbook::adif_header();
    let span = (n / 4).max(1);
    for _ in 0..n {
        let when = 1_700_000_000 + 60 * g.below(span) as u64;
        s.push_str(&synthetic(&mut g, when));
    }
    s
}

/// A launch on the store at `d` — the shipped open path, which converts the `log.adi` it finds
/// there — with the resolvers set before the log is adopted, as the shell's are.
pub(super) fn launch(d: &Dir) -> crate::SharedEngine {
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

/// The same log on the 1.13 path: no store, the log in memory — a session whose store could not
/// be opened, keeping its log in its own `log.adi` in `d` ([`Dir::memory_log`]).
pub(super) fn memory(d: &Dir, adif: &str) -> crate::SharedEngine {
    let mut e = Engine::new(MY_CALL, "EN52", 0);
    e.set_dxcc_resolver(test_country);
    e.set_state_resolver(test_state);
    e.set_log_path(d.memory_log());
    e.import_adif(adif);
    assert!(!e.log_store_open(), "premise: the 1.13 path");
    Arc::new(Mutex::new(e))
}

/// Everything submitted, written — before a test's folder goes.
pub(super) fn settle(e: &crate::SharedEngine) {
    e.lock()
        .unwrap()
        .flush_log_store(std::time::Duration::from_secs(120))
        .expect("written");
}

/// The contact at `at` in log order, as the log in memory holds it.
pub(super) fn record_at(e: &Engine, at: usize) -> QsoRecord {
    QsoRecord::clone(&e.log_records()[at])
}

/// One ADIF record, parsed as the log parses it, with no id.
pub(super) fn parse_one(text: &str) -> QsoRecord {
    let mut log = tempo_core::logbook::Logbook::new();
    log.import_adif(text);
    let mut r = QsoRecord::clone(&log.records()[0]);
    r.id = None;
    r
}

/// The id of the contact at `at`: how a change names its contact (SPEC-2 C16).
pub(super) fn id_at(e: &Engine, at: usize) -> tempo_core::logbook::RecordId {
    e.log_records()[at]
        .id
        .expect("every row the log holds carries an id")
}

/// Replace the contact at `at` with `edited` — the edit an operator makes, by the contact's id.
pub(super) fn edit_at(e: &mut Engine, at: usize, edited: QsoRecord) {
    let id = id_at(e, at);
    assert!(e.update_qso(id, edited), "the edit is taken");
}

/// One random change of the kinds the app makes to the log: logged contacts (some in a second
/// already in the log), imports, edits that move a contact in time or change what a search or
/// a recall matches, deletes, QSL cards and sent marks, satellite tags, the connectors' stamps,
/// a LoTW confirmation with credit, and the fill job.
pub(super) fn random_change(e: &crate::SharedEngine, g: &mut Gen, step: u64) {
    let len = e.lock().unwrap().log_records().len();
    let at = g.below(len);
    let when = 1_700_000_000 + 60 * g.below(64) as u64 + step;
    match g.below(12) {
        0 | 1 => {
            let mut rec = parse_one(&synthetic(g, when));
            rec.state = None;
            rec.country = None;
            e.lock().unwrap().log_qso(rec);
        }
        2 => {
            let _ = e.lock().unwrap().import_adif(&synthetic(g, when));
        }
        3 | 4 if len > 0 => {
            let mut eng = e.lock().unwrap();
            let mut r = record_at(&eng, at);
            match g.below(5) {
                0 => r.call = g.pick(CALLS).to_string(),
                1 => r.band = "40m".into(),
                2 => r.grid = Some("JN58".into()),
                3 => r.country = Some("Österreich".into()),
                // Into a second another contact holds: a tie the window must order by log.
                _ => r.when_unix = 1_700_000_000 + 60 * g.below(64) as u64,
            }
            edit_at(&mut eng, at, r);
        }
        5 if len > 0 => {
            let mut eng = e.lock().unwrap();
            let id = id_at(&eng, at);
            eng.delete_qso(id);
        }
        6 if len > 0 => {
            let mut eng = e.lock().unwrap();
            let id = id_at(&eng, at);
            eng.mark_qsl_card(id, g.chance(2));
        }
        7 if len > 0 => {
            let mut eng = e.lock().unwrap();
            let id = id_at(&eng, at);
            eng.mark_qsl_sent(id, Some(tempo_core::logbook::QslVia::Direct));
        }
        8 if len > 0 => {
            let sat = g.chance(2).then_some("RS-44");
            let mut eng = e.lock().unwrap();
            let id = id_at(&eng, at);
            eng.set_sat_tag(id, sat);
        }
        9 if len > 0 => {
            let mut eng = e.lock().unwrap();
            let pushed = record_at(&eng, at);
            let (outcome, detail) = if g.chance(2) {
                (UploadOutcome::Accepted, None)
            } else {
                (UploadOutcome::Rejected, Some(UploadDetail::RecordRefused))
            };
            eng.stamp_qrz_upload(&pushed, outcome, when as i64, detail);
            eng.stamp_eqsl_upload(&pushed, outcome, when as i64 + 1, detail);
        }
        10 if len > 0 => {
            let mut eng = e.lock().unwrap();
            let r = record_at(&eng, at);
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
}
