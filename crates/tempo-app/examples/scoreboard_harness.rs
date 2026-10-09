//! Dev harness: serve the REAL spectator scoreboard (page, routes and payload builders)
//! from SCRIPTED clubs, so a headless browser can render and measure it without a radio,
//! a second PC or a LAN. Not shipped:
//!
//! `cargo run -p tempo-app --example scoreboard_harness -- [first-port] [page.html]`
//!
//! One server per case, on consecutive ports from `first-port` (default 7380):
//! +0 an Illinois QSO Party host · +1 an ARRL Field Day host · +2 a Winter Field Day host
//! · +3 a position showing the +0 host's board · +4 a position whose host is gone · +5 a
//! station with no club. Each host is frozen mid-event (its clock is the event's start
//! plus a few hours), so every board shows a running contest whatever today's date is.
//! `page.html` serves that file instead of the compiled-in page, for design work.
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tempo_app::fd_scoreboard::{
    build_data_core, build_meta, serve_until, BoardRole, BoardSource, ContestBoard, FdBoardData,
    FdBoardPosition, FdBoardRow, StationBoard, SCOREBOARD_PAGE,
};
use tempo_app::fdevent::{to_wire_fields, ClubLog};
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules;
use tempo_core::fieldday::FdEvent;

/// A board frozen at one instant: the payloads are built once, at `now`.
struct Frozen {
    data: String,
    meta: String,
    page: &'static str,
}

impl BoardSource for Frozen {
    fn data(&self) -> String {
        self.data.clone()
    }
    fn meta(&self) -> String {
        self.meta.clone()
    }
    fn page(&self) -> &'static str {
        self.page
    }
}

fn frozen(d: &FdBoardData, now: u64, page: &'static str) -> Frozen {
    let core = build_data_core(d, now);
    Frozen {
        data: format!("{{\"rev\":1,\"now_unix\":{now},{}", &core[1..]),
        meta: build_meta(d, now),
        page,
    }
}

/// A small deterministic generator, so every run draws the same club.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

fn tents(now: u64) -> Vec<FdBoardPosition> {
    [
        ("aaaa0001", "CW tent", "AA9XYZ", "40m", "CW", 4),
        ("bbbb0002", "SSB tent", "W9XYZ", "20m", "PH", 6),
        ("cccc0003", "Digital", "KD9ABC", "20m", "DIG", 9),
        ("dddd0004", "VHF", "N9VHF", "6m", "PH", 400),
    ]
    .into_iter()
    .map(|(id, label, op, band, mode, ago)| FdBoardPosition {
        id: id.into(),
        label: label.into(),
        operator: op.into(),
        band: band.into(),
        mode: mode.into(),
        last_seen_unix: now - ago,
    })
    .collect()
}

const CALLS: &[&str] = &[
    "K9AAA", "W9BBB", "N9CCC", "KB9DDD", "AA9EE", "W0FF", "K0GG", "N8HH", "W8II", "K4JJ", "VE3KK",
    "W1LL", "K2MM", "N5NN", "W6OO", "KC9PP", "W9QQ", "KD9RR", "N9SS", "K9TT",
];

fn ilqp(page: &'static str) -> Frozen {
    let rs = fd_rules::ruleset_by_id("ilqp", fd_rules::CURRENT_RULES_YEAR).expect("ILQP");
    let start = rs.event_window(2026).start_unix;
    let now = start + 3 * 3600 + 1260;
    let spec = rs.exchange;
    let counties: Vec<&str> = rs.domains[0].values.iter().map(|v| v.0).collect();
    let states = [
        "WI", "IN", "MO", "IA", "OH", "MI", "TX", "CA", "ON", "FL", "NY", "KY",
    ];
    let mut club = ClubLog::for_ruleset(rs, "W9XYZ ILQP");
    let mut g = Lcg(18);
    let bands = ["80m", "40m", "40m", "20m", "20m", "15m", "10m", "2m"];
    let modes = ["CW", "CW", "PH", "PH", "DIG"];
    let ids = ["aaaa0001", "bbbb0002", "cccc0003", "dddd0004"];
    let mut seqs = [0u64; 4];
    for i in 0..260u64 {
        let p = g.next(4);
        seqs[p] += 1;
        let qth = match g.next(10) {
            0..=6 => counties[g.next(counties.len().min(64))],
            7 | 8 => states[g.next(states.len())],
            _ => "DX",
        };
        let call = if i == 41 {
            "W9AWE"
        } else {
            CALLS[g.next(CALLS.len())]
        };
        let mode = modes[g.next(modes.len())];
        let rst = if mode == "PH" { "59" } else { "599" };
        let fields = |q: &str| {
            to_wire_fields(
                &[("RST", rst), ("QTH", q)]
                    .iter()
                    .filter_map(|(k, v)| spec.copied(k, v))
                    .collect::<Vec<_>>(),
            )
        };
        club.merge(
            &tempo_net::fdsync::WireQso {
                pos: ids[p].into(),
                seq: seqs[p],
                call: format!("{call}{}", if g.next(3) == 0 { "/M" } else { "" }),
                class: String::new(),
                sect: String::new(),
                ex: fields(qth),
                mex: fields("MCLN"),
                band: bands[g.next(bands.len())].into(),
                mode: mode.into(),
                sub: if mode == "DIG" {
                    "RTTY".into()
                } else {
                    String::new()
                },
                when: start + 30 + i * 45 + g.next(40) as u64,
                op: ["AA9XYZ", "W9XYZ", "KD9ABC", "N9VHF"][p].into(),
                sat: String::new(),
                sat_fm: false,
            },
            now,
        );
    }
    let session = ContestSession::for_ruleset(
        rs,
        &StationData {
            mycall: "W9XYZ".into(),
            mygrid: "EN50".into(),
            contest_qth_state: "IL".into(),
            contest_qth_county: "MCLN".into(),
            ..Default::default()
        },
    );
    let d = FdBoardData {
        call: "W9XYZ".into(),
        power_mult: 1,
        positions: tents(now),
        contest: Some(ContestBoard {
            session,
            rows: club.rows().to_vec(),
        }),
        ..Default::default()
    };
    frozen(&d, now, page)
}

fn field_day(event: FdEvent, page: &'static str) -> Frozen {
    let rs = fd_rules::ruleset(event, fd_rules::CURRENT_RULES_YEAR);
    let start = rs.event_window(2026).start_unix;
    let now = start + 14 * 3600 + 600;
    let sections: Vec<&str> = fd_rules::sections().iter().map(|s| s.code).collect();
    let mut g = Lcg(if event == FdEvent::ArrlFd { 24 } else { 26 });
    let bands = [
        "80m", "40m", "40m", "20m", "20m", "20m", "15m", "10m", "6m", "2m",
    ];
    let modes = ["CW", "PH", "PH", "DIG", "DIG"];
    let ids = ["aaaa0001", "bbbb0002", "cccc0003", "dddd0004"];
    let classes = if event == FdEvent::ArrlFd {
        ["1D", "2A", "3A", "1E"]
    } else {
        ["1O", "2H", "3I", "1M"]
    };
    let mut seqs = [0u64; 4];
    let mut rows = Vec::new();
    for i in 0..640u64 {
        let p = g.next(4);
        seqs[p] += 1;
        rows.push(FdBoardRow {
            posid: ids[p].into(),
            seq: seqs[p],
            call: format!("{}{}", CALLS[g.next(CALLS.len())], i % 7),
            class: classes[g.next(4)].into(),
            section: sections[g.next(sections.len() * 3 / 4)].into(),
            band: bands[g.next(bands.len())].into(),
            mode_class: modes[g.next(modes.len())].into(),
            when_unix: start + 60 + i * 78 + g.next(60) as u64,
            operator: ["AA9XYZ", "W9XYZ", "KD9ABC", "N9VHF"][p].into(),
            // The submode and anything a later build adds to a row stay at their
            // defaults, so this harness keeps compiling as the row grows.
            ..Default::default()
        });
    }
    let d = FdBoardData {
        event,
        call: "W9XYZ".into(),
        class: if event == FdEvent::ArrlFd { "4A" } else { "4O" }.into(),
        section: "IL".into(),
        power_mult: 2,
        claimed: [
            "emergency-power",
            "public-location",
            "public-info-table",
            "w1aw-bulletin",
            "web-submission",
            "social-media",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        positions: tents(now),
        rows,
        ..Default::default()
    };
    frozen(&d, now, page)
}

fn main() {
    let first: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(7380);
    let page: &'static str = match std::env::args().nth(2) {
        Some(path) => Box::leak(
            std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{path}: {e}"))
                .into_boxed_str(),
        ),
        None => SCOREBOARD_PAGE,
    };
    let host = format!("127.0.0.1:{first}");
    // A documentation address (RFC 5737): nothing answers there, as for a host that is off.
    let gone = "192.0.2.10:7373".to_string();
    /// A station's board, with the page this run serves.
    struct Paged<S: BoardSource>(S, &'static str);
    impl<S: BoardSource> BoardSource for Paged<S> {
        fn data(&self) -> String {
            self.0.data()
        }
        fn meta(&self) -> String {
            self.0.meta()
        }
        fn page(&self) -> &'static str {
            self.1
        }
        fn extra(&self, raw_path: &str) -> Option<tempo_app::fd_scoreboard::Response> {
            self.0.extra(raw_path)
        }
    }
    let sources: Vec<(&str, Arc<dyn BoardSource>)> = vec![
        ("Illinois QSO Party host", Arc::new(ilqp(page))),
        (
            "ARRL Field Day host",
            Arc::new(field_day(FdEvent::ArrlFd, page)),
        ),
        (
            "Winter Field Day host",
            Arc::new(field_day(FdEvent::WinterFd, page)),
        ),
        (
            "a position (shows the ILQP host's board)",
            Arc::new(Paged(
                StationBoard::new(|| None, move || BoardRole::Position { host: host.clone() }),
                page,
            )),
        ),
        (
            "a position whose host is gone",
            Arc::new(Paged(
                StationBoard::new(|| None, move || BoardRole::Position { host: gone.clone() }),
                page,
            )),
        ),
        (
            "a station with no club",
            Arc::new(Paged(
                StationBoard::new(
                    || None,
                    || BoardRole::Idle {
                        reason: "no-club",
                        detail: String::new(),
                        contest: "ilqp".into(),
                    },
                ),
                page,
            )),
        ),
    ];
    let mut threads = Vec::new();
    for (i, (what, source)) in sources.into_iter().enumerate() {
        let port = first + i as u16;
        let listener = std::net::TcpListener::bind(("127.0.0.1", port))
            .unwrap_or_else(|e| panic!("{port}: {e}"));
        println!("http://127.0.0.1:{port}/  {what}");
        let shutdown = Arc::new(AtomicBool::new(false));
        threads.push(std::thread::spawn(move || {
            serve_until(listener, source, shutdown)
        }));
    }
    for t in threads {
        let _ = t.join();
    }
}
