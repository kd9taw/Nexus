//! The Stats pane beside the log entry, and the Awards view's needs per band, count the log as
//! the awards do: the pane's grids are filled by the award fold itself, so a slot is worked or
//! confirmed exactly when a contact counts toward DXCC, WAS or WAZ — by each record's own DXCC
//! number and CQ zone ([`propagation::awards::counted_entity`]), with what each award credits as a
//! confirmation (DXCC and WAS: LoTW or a card; WAZ: an Authenticity-Guaranteed eQSL as well).
//!
//! The request behind it: a DXer moving from LOG4OM wanted the DXCC, WAS and WAZ standing of the
//! station being typed, in the same window, and the needed entities on each band.

use propagation::callstats::Slot;
use propagation::{AwardSummary, Awards, CallStatsIndex, QslChannels, RecordFields, StatsGrid};

/// One contact, as the app's award fold hands it over.
#[derive(Clone, Copy)]
struct Q<'a> {
    call: &'a str,
    band: &'a str,
    mode: &'a str,
    state: Option<&'a str>,
    sat: bool,
    rec: RecordFields,
}

fn q<'a>(call: &'a str, band: &'a str, mode: &'a str) -> Q<'a> {
    Q {
        call,
        band,
        mode,
        state: None,
        sat: false,
        rec: RecordFields::default(),
    }
}

impl<'a> Q<'a> {
    fn qsl(mut self, qsl: QslChannels) -> Self {
        self.rec.qsl = qsl;
        self
    }
    fn state(mut self, s: &'a str) -> Self {
        self.state = Some(s);
        self
    }
    fn dxcc(mut self, code: u32) -> Self {
        self.rec.dxcc = Some(code);
        self
    }
    fn lotw_dxcc(mut self, code: u32) -> Self {
        self.rec.lotw_dxcc = Some(code);
        self
    }
    fn cqz(mut self, zone: u8) -> Self {
        self.rec.cqz = Some(zone);
        self
    }
    fn sat(mut self) -> Self {
        self.sat = true;
        self
    }
}

const NONE: QslChannels = QslChannels {
    card: false,
    lotw: false,
    eqsl: false,
    eqsl_ag: false,
    qrz: false,
};
const LOTW: QslChannels = QslChannels { lotw: true, ..NONE };
const CARD: QslChannels = QslChannels { card: true, ..NONE };
const EQSL: QslChannels = QslChannels { eqsl: true, ..NONE };
const EQSL_AG: QslChannels = QslChannels {
    eqsl: true,
    eqsl_ag: true,
    ..NONE
};
const QRZ: QslChannels = QslChannels { qrz: true, ..NONE };

/// The log through the award fold, as the app folds it: `confirmed` is the ARRL's (LoTW or card).
fn fold(log: &[Q]) -> (AwardSummary, CallStatsIndex) {
    let mut a = Awards::with_call_stats();
    for c in log {
        let confirmed = c.rec.qsl.card || c.rec.qsl.lotw;
        a.add_qso(
            c.call,
            c.band,
            c.mode,
            confirmed,
            false,
            c.rec.qsl.card,
            c.state,
            None,
            None,
            c.sat,
            c.rec,
        );
    }
    let index = a
        .take_call_stats()
        .expect("an accumulator made for it keeps the index");
    (a.summary(), index)
}

/// A grid's filled slots, row by row (CW, Phone, Digital): each band's label and its slot,
/// for every slot that is not plain unworked.
fn filled(grid: &StatsGrid) -> [Vec<(&'static str, Slot)>; 3] {
    let bands = [
        "160m", "80m", "60m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "4m", "2m",
    ];
    let row = |r: usize| {
        grid.rows[r]
            .iter()
            .zip(bands)
            .filter(|(s, _)| **s != Slot::Unworked)
            .map(|(s, b)| (b, *s))
            .collect()
    };
    [row(0), row(1), row(2)]
}

const NO_CREDIT: [(&str, Slot); 2] = [("60m", Slot::NoCredit), ("4m", Slot::NoCredit)];

/// DXCC's 60 m and 4 m slots, which earn no credit, among `slots` in band order.
fn dxcc_row(slots: &[(&'static str, Slot)]) -> Vec<(&'static str, Slot)> {
    let order = |b: &str| {
        [
            "160m", "80m", "60m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "4m", "2m",
        ]
        .iter()
        .position(|x| *x == b)
    };
    let mut row: Vec<_> = slots.iter().copied().chain(NO_CREDIT).collect();
    row.sort_by_key(|(b, _)| order(b));
    row
}

/// ★ The DXCC grid is the entity's band × mode slots as DXCC credits them, and the WAZ grid the
/// zone's as WAZ credits them: an eQSL fills a DXCC slot as worked and no more, an
/// Authenticity-Guaranteed one confirms a WAZ slot, 60 m and 4 m earn no DXCC slot, and a
/// satellite contact fills none of DXCC's while it counts for WAZ.
#[test]
fn the_grids_are_the_awards_own_slots() {
    let (s, ix) = fold(&[
        q("DL1ABC", "20m", "CW").qsl(LOTW),
        q("DL1ABC", "20m", "FT8"),
        q("DL2XYZ", "40m", "SSB").qsl(EQSL_AG),
        q("DL3XYZ", "60m", "CW").qsl(LOTW),
        q("DL4XYZ", "2m", "FM").qsl(LOTW).sat(),
        // A later contact in a confirmed slot, itself unconfirmed: the slot stays confirmed.
        q("DL5XYZ", "20m", "CW"),
    ]);
    let st = ix.stats_for("dl9zzz ", "20m", "CW", None);
    assert_eq!(st.call, "DL9ZZZ");
    let dxcc = st.dxcc.as_ref().expect("Germany");
    assert_eq!(
        (dxcc.label.as_str(), dxcc.code),
        ("Fed. Rep. of Germany", Some(230))
    );
    assert_eq!(
        filled(dxcc),
        [
            dxcc_row(&[("20m", Slot::Confirmed)]),
            dxcc_row(&[("40m", Slot::Worked)]),
            dxcc_row(&[("20m", Slot::Worked)]),
        ]
    );
    let waz = st.waz.as_ref().expect("zone 14");
    assert_eq!(waz.label, "14");
    assert_eq!(
        filled(waz),
        [
            vec![("60m", Slot::Confirmed), ("20m", Slot::Confirmed)],
            vec![("40m", Slot::Confirmed), ("2m", Slot::Confirmed)],
            vec![("20m", Slot::Worked)],
        ]
    );
    assert!(st.was.is_none() && !st.was_family);
    // …and the totals beside it are the same contacts' (a control on the fold itself).
    assert_eq!(
        (
            s.dxcc_worked,
            s.dxcc_confirmed,
            s.waz_worked,
            s.waz_confirmed
        ),
        (1, 1, 1, 1)
    );
}

/// The flags before logging: NEW ONE is an entity no contact counts toward; NEW BAND a band
/// with no slot of it (on a band that earns DXCC credit); NEW MODE a mode it was never worked
/// in, FT4 apart from FT8.
#[test]
fn the_flags_say_what_logging_this_contact_would_add() {
    let (_, ix) = fold(&[
        q("DL1ABC", "20m", "CW").qsl(LOTW),
        q("DL1ABC", "20m", "FT8"),
        q("DL3XYZ", "60m", "CW").qsl(LOTW),
        q("DL4XYZ", "2m", "FM").qsl(LOTW).sat(),
    ]);
    let flags = |call: &str, band: &str, mode: &str| {
        let s = ix.stats_for(call, band, mode, None);
        (s.new_one, s.new_band, s.new_mode)
    };
    assert_eq!(
        [
            flags("DL9ZZZ", "20m", "CW"),
            flags("DL9ZZZ", "17m", "CW"),
            flags("DL9ZZZ", "20m", "FT4"),
            flags("DL9ZZZ", "20m", "FT8"),
            // 60 m earns no DXCC credit: there is no slot to be new.
            flags("DL9ZZZ", "60m", "CW"),
            // The satellite contact on 2 m filled no DXCC slot, and FM was never worked.
            flags("DL9ZZZ", "2m", "FM"),
            flags("JA1XYZ", "20m", "CW"),
            // A call that names no entity is new to nothing.
            flags("Q1Q", "20m", "CW"),
        ],
        [
            (false, false, false),
            (false, true, false),
            (false, false, true),
            (false, false, false),
            (false, false, false),
            (false, true, true),
            (true, false, false),
            (false, false, false),
        ]
    );
}

/// Each contact counts for the entity its record names, as in the Awards view: K1N's 2015
/// contact (Navassa, 182) fills Navassa and not the United States, LoTW's number wins over the
/// log's, a Sicilian contact is Italy's, a GDR contact (deleted) fills no current entity, and
/// DXCC 0 fills none. The station typed is placed by its call.
#[test]
fn a_contact_counts_for_the_entity_its_record_names() {
    let (_, ix) = fold(&[
        q("K1N", "20m", "CW").qsl(LOTW).dxcc(182),
        q("K1B", "20m", "CW").qsl(LOTW).dxcc(291).lotw_dxcc(20),
        q("IT9ABC", "20m", "CW").qsl(LOTW),
        q("Y24UK", "20m", "CW").qsl(LOTW).dxcc(229),
        q("W1MM", "20m", "CW").qsl(LOTW).dxcc(0),
    ]);
    let cw20 = |call: &str| {
        let s = ix.stats_for(call, "20m", "CW", None);
        let d = s.dxcc.expect("an entity");
        (d.label, d.rows[0][5], s.new_one)
    };
    assert_eq!(
        [
            cw20("KP1AA"),
            cw20("K1N"),
            cw20("KH1AA"),
            cw20("I1ABC"),
            cw20("IT9ZZZ"),
            cw20("DL1ABC"),
        ],
        [
            ("Navassa Island".into(), Slot::Confirmed, false),
            ("United States".into(), Slot::Unworked, true),
            ("Baker & Howland Islands".into(), Slot::Confirmed, false),
            ("Italy".into(), Slot::Confirmed, false),
            ("Italy".into(), Slot::Confirmed, false),
            ("Fed. Rep. of Germany".into(), Slot::Unworked, true),
        ]
    );
}

/// The source rows: what any contact with the entity, and any with this very call, was
/// confirmed through — every channel, whether the award credits it or not.
#[test]
fn the_source_rows_list_every_channel_for_the_country_and_the_call() {
    let (_, ix) = fold(&[
        q("DL1ABC", "20m", "CW").qsl(LOTW),
        q("DL2XYZ", "40m", "SSB").qsl(EQSL_AG),
        q("DL1ABC", "15m", "FT8").qsl(QRZ),
        q("K1N", "20m", "CW").qsl(CARD).dxcc(182),
        q("W1AW", "20m", "CW").qsl(EQSL),
    ]);
    let rows = |call: &str| {
        let s = ix.stats_for(call, "20m", "CW", None);
        (s.country_qsl, s.call_qsl)
    };
    assert_eq!(
        [rows("DL1ABC"), rows("DL9ZZZ"), rows("K1N"), rows("Q1Q")],
        [
            (
                Some(QslChannels {
                    lotw: true,
                    eqsl: true,
                    eqsl_ag: true,
                    qrz: true,
                    ..NONE
                }),
                QslChannels {
                    lotw: true,
                    qrz: true,
                    ..NONE
                },
            ),
            (
                Some(QslChannels {
                    lotw: true,
                    eqsl: true,
                    eqsl_ag: true,
                    qrz: true,
                    ..NONE
                }),
                NONE,
            ),
            // K1N's card is Navassa's: the United States holds only W1AW's eQSL, while this call's
            // own row has the card.
            (Some(EQSL), CARD),
            (None, NONE),
        ]
    );
}

/// WAS: the state the app places the station in, filled by contacts in the WAS family only
/// (Western Australia is not Washington), the record's DXCC deciding the family, and an eQSL
/// confirming nothing.
#[test]
fn the_was_grid_is_the_states_slots_in_the_was_family_only() {
    let (_, ix) = fold(&[
        q("W1AW", "20m", "CW").qsl(LOTW).state("CT"),
        q("VK6ABC", "20m", "CW").qsl(LOTW).state("WA"),
        q("W7XYZ", "40m", "SSB").state("WA"),
        q("KP4ABC", "20m", "CW").qsl(LOTW).state("FL").dxcc(291),
        q("N1ABC", "15m", "CW").qsl(EQSL_AG).state("ME"),
    ]);
    let was = |call: &str, state: Option<&str>| {
        let s = ix.stats_for(call, "20m", "CW", state);
        (s.was_family, s.was.map(|g| (g.label.clone(), filled(&g))))
    };
    let only = |r: usize, slot: (&'static str, Slot)| {
        let mut rows: [Vec<(&'static str, Slot)>; 3] = Default::default();
        rows[r] = vec![slot];
        rows
    };
    assert_eq!(
        [
            was("W1XYZ", Some("CT")),
            was("W7ABC", Some("WA")),
            was("W4ABC", Some("fl")),
            was("N1XYZ", Some("ME")),
            was("W9XYZ", None),
            was("W9XYZ", Some("XX")),
            was("VK2ABC", Some("WA")),
        ],
        [
            (true, Some(("CT".into(), only(0, ("20m", Slot::Confirmed))))),
            (true, Some(("WA".into(), only(1, ("40m", Slot::Worked))))),
            (true, Some(("FL".into(), only(0, ("20m", Slot::Confirmed))))),
            (true, Some(("ME".into(), only(0, ("15m", Slot::Worked))))),
            (true, None),
            (true, None),
            (false, None),
        ]
    );
}

/// WAZ counts the zone the record carries (a W1 call logged in California is zone 3), and the
/// station typed is placed in its call's zone.
#[test]
fn the_waz_grid_counts_the_zone_the_record_carries() {
    let (_, ix) = fold(&[q("K1ABC", "20m", "CW").qsl(LOTW).cqz(3)]);
    let zone = |call: &str| {
        let g = ix.stats_for(call, "20m", "CW", None).waz.expect("a zone");
        (g.label.clone(), g.rows[0][5])
    };
    assert_eq!(
        [zone("W6XYZ"), zone("W1XYZ")],
        [("3".into(), Slot::Confirmed), ("5".into(), Slot::Unworked)]
    );
}

/// ★ WAZ credits an eQSL from an Authenticity-Guaranteed member, and DXCC and WAS do not: the
/// totals the Awards view shows, beside the pane.
#[test]
fn waz_confirms_an_authenticity_guaranteed_eqsl_and_dxcc_does_not() {
    let (s, _) = fold(&[
        q("JA1XYZ", "20m", "FT8").qsl(EQSL_AG),
        q("VK2ABC", "20m", "FT8").qsl(EQSL),
        q("W1AW", "20m", "FT8").qsl(EQSL_AG).state("CT"),
    ]);
    assert_eq!(
        (
            s.waz_worked,
            s.waz_confirmed,
            s.dxcc_worked,
            s.dxcc_confirmed,
            s.was.worked,
            s.was.confirmed
        ),
        (3, 2, 3, 0, 1, 0)
    );
}

/// ★ The Awards view's needs per band: on each band, what was never worked there and what was
/// worked but not confirmed, for DXCC (bands that earn credit), WAS and WAZ.
#[test]
fn the_needs_per_band_are_never_worked_and_worked_not_confirmed() {
    let (s, _) = fold(&[
        q("DL1ABC", "20m", "CW").qsl(LOTW),
        q("JA1XYZ", "20m", "FT8"),
        q("JA1AAA", "20m", "FT8").qsl(EQSL_AG),
        q("W1AW", "20m", "CW").qsl(LOTW).state("CT"),
        q("W7XYZ", "40m", "SSB").state("WA"),
        // 30 m is no 5-band award band: WAS and WAZ count it per band all the same.
        q("K6ABC", "30m", "CW").qsl(LOTW).state("CA"),
    ]);
    let n = &s.band_needs;
    let bands = |v: &[propagation::BandNeed]| v.iter().map(|b| b.band.clone()).collect::<Vec<_>>();
    assert_eq!(
        bands(&n.dxcc),
        ["160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "2m"]
    );
    // DXCC keeps no band slot on 70/33/23 cm (above); WAS and WAZ list every band Nexus names.
    assert_eq!(bands(&n.was).len(), 16);
    assert_eq!(bands(&n.waz).len(), 16);
    let on = |v: &[propagation::BandNeed], band: &str| {
        let b = v.iter().find(|b| b.band == band).expect("the band");
        (b.unworked.clone(), b.unconfirmed.clone())
    };
    let current = propagation::dxcc::current_dxcc_entities();
    let (unworked, unconfirmed) = on(&n.dxcc, "20m");
    assert_eq!(unconfirmed, ["Japan"]);
    assert_eq!(unworked.len(), current - 3);
    assert!(unworked.iter().any(|e| e == "Italy"));
    assert!(!unworked
        .iter()
        .any(|e| ["Japan", "United States", "Fed. Rep. of Germany"].contains(&e.as_str())));
    let (unworked, unconfirmed) = on(&n.dxcc, "40m");
    assert_eq!(
        (unworked.len(), unconfirmed),
        (current - 1, vec!["United States".to_string()])
    );
    let (unworked, unconfirmed) = on(&n.was, "30m");
    assert_eq!(
        (
            unworked.len(),
            unworked.contains(&"CA".to_string()),
            unconfirmed.len()
        ),
        (49, false, 0)
    );
    let (unworked, unconfirmed) = on(&n.waz, "30m");
    assert_eq!(
        (
            unworked.len(),
            unworked.contains(&"3".to_string()),
            unconfirmed.len()
        ),
        (39, false, 0)
    );
    let (unworked, unconfirmed) = on(&n.was, "20m");
    assert_eq!(
        (
            unworked.len(),
            unworked.contains(&"CT".to_string()),
            unconfirmed.len()
        ),
        (49, false, 0)
    );
    let (unworked, unconfirmed) = on(&n.was, "40m");
    assert_eq!((unworked.len(), unconfirmed), (49, vec!["WA".to_string()]));
    // Zones 14, 25 and 5 on 20 m, 25 confirmed by the AG eQSL; zone 3 (W7) worked on 40 m.
    let (unworked, unconfirmed) = on(&n.waz, "20m");
    assert_eq!((unworked.len(), unconfirmed.len()), (37, 0));
    assert_eq!(&unworked[..4], ["1", "2", "3", "4"]);
    let (unworked, unconfirmed) = on(&n.waz, "40m");
    assert_eq!((unworked.len(), unconfirmed), (39, vec!["3".to_string()]));
    let (unworked, unconfirmed) = on(&n.waz, "80m");
    assert_eq!((unworked.len(), unconfirmed.len()), (40, 0));
}

/// The pane's DXCC grids add up to the Awards view's per-band and per-mode counts, slot for slot
/// — the index IS the award fold's, not a second count beside it.
#[test]
fn the_grids_add_up_to_the_awards_per_band_and_per_mode_counts() {
    let calls = [
        "DL1ABC", "JA1XYZ", "W1AW", "VK2ABC", "I1ABC", "IT9ABC", "F5ABC", "G4ABC", "EA4ABC",
        "ZS6ABC", "PY1ABC", "LU1ABC", "VE3ABC", "XE1ABC", "K1N",
    ];
    let bands = [
        "160m", "80m", "60m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "4m", "2m",
    ];
    let modes = ["CW", "SSB", "FT8", "FT4", "RTTY"];
    let qsls = [LOTW, NONE, CARD, EQSL_AG, QRZ];
    let mut log = Vec::new();
    for (i, call) in calls.iter().enumerate() {
        for j in 0..(i % 5 + 2) {
            let k = i * 7 + j * 3;
            let mut c =
                q(call, bands[k % bands.len()], modes[k % modes.len()]).qsl(qsls[k % qsls.len()]);
            if *call == "K1N" {
                c = c.dxcc(182);
            }
            if k % 11 == 0 {
                c = c.sat();
            }
            log.push(c);
        }
    }
    let (s, ix) = fold(&log);
    let mut grids = std::collections::BTreeMap::new();
    // Every entity the log counts, by a call of its own (K1N's contacts are Navassa's).
    for c in calls.iter().chain(&["KP1AA"]) {
        let g = ix.stats_for(c, "20m", "CW", None).dxcc.expect("an entity");
        grids.insert(g.label.clone(), g);
    }
    let count = |slot: &dyn Fn(Slot) -> bool, cell: &dyn Fn(&StatsGrid) -> Vec<Slot>| {
        grids
            .values()
            .filter(|g| cell(g).into_iter().any(slot))
            .count()
    };
    let worked = |x: Slot| matches!(x, Slot::Worked | Slot::Confirmed);
    let confirmed = |x: Slot| x == Slot::Confirmed;
    for b in &s.bands {
        let at = bands
            .iter()
            .position(|x| *x == b.band)
            .expect("a grid band");
        let column = |g: &StatsGrid| g.rows.iter().map(|r| r[at]).collect::<Vec<_>>();
        assert_eq!(
            (count(&worked, &column), count(&confirmed, &column)),
            (b.worked, b.confirmed),
            "{}",
            b.band
        );
    }
    for (r, m) in ["CW", "Phone", "Digital"].iter().enumerate() {
        let row = |g: &StatsGrid| g.rows[r].clone();
        let award = s.modes.iter().find(|x| x.mode == *m);
        assert_eq!(
            (count(&worked, &row), count(&confirmed, &row)),
            award.map_or((0, 0), |a| (a.worked, a.confirmed)),
            "{m}"
        );
    }
    assert!(s.bands.len() >= 5, "premise: the log spreads over bands");
}

/// The Needed board asks for a zone's confirmation exactly where WAZ still lacks one: an eQSL from an
/// Authenticity-Guaranteed member confirms the zone there too, and a basic eQSL does not.
#[test]
fn the_needed_board_confirms_a_zone_as_waz_does() {
    use propagation::{Band, LogNeeds};
    let mut n = LogNeeds::new();
    for (call, qsl) in [("JA1XYZ", EQSL_AG), ("VK2ABC", EQSL), ("DL1ABC", LOTW)] {
        let rec = RecordFields {
            qsl,
            ..RecordFields::default()
        };
        n.add_qso(
            call,
            "20m",
            "FT8",
            None,
            None,
            qsl.card || qsl.lotw,
            false,
            rec,
        );
    }
    let mut zones: Vec<u8> = n
        .confirmed_zones()
        .iter()
        .filter(|(_, b)| *b == Band::B20)
        .map(|(z, _)| *z)
        .collect();
    zones.sort_unstable();
    // Japan's 25 by the AG card, Germany's 14 by LoTW; Australia's 30 still needs one.
    assert_eq!(zones, [14, 25]);
}
