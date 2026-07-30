//! Regression cover for the false "new mode on 30m" field report (operator, 2026-07-29).
//!
//! The operator runs a ~11,200-QSO log that is overwhelmingly FT8 and HAS worked
//! Asiatic Russia on 30m — yet the Needed system claimed a new mode there. The
//! fixtures below are the operator's REAL stored rows (call / band / mode exactly as
//! they sit in `log.adi`, mixed-case band labels and all), so the matrix is exercised
//! against the data that produced the report rather than against tidied-up values.
//!
//! Two distinct defects are pinned here:
//!   1. the mode-class table missed real-world ADIF phone tokens (`PH`), so phone
//!      QSOs credited the DIGITAL slot and left a phantom Phone need behind;
//!   2. the NewMode headline named a BAND while the predicate behind it is
//!      entity-wide, which is how a true "never on CW" need read as a false claim
//!      about 30m to an operator who has worked that entity on 30m FT8.
//!
//! A full tag census over all 11,207 rows of that log also rules two suspects OUT,
//! which is why nothing here tests them: `SUBMODE` appears in ZERO records (FT4 is
//! stored as a bare `MODE=FT4`, so the MFSK+SUBMODE=FT4 collapse cannot be the
//! mechanism for this operator), and `DXCC` appears in ZERO records — the entity
//! travels only as the free-text `COUNTRY` string, which the backend ignores in favour
//! of resolving the callsign through cty.dat. The 13 tags actually present are CALL,
//! COUNTRY, BAND, FREQ, MODE, QSO_DATE, TIME_ON, GRIDSQUARE, NAME, RST_*, QTH, STATE,
//! LOTW_QSL_RCVD (+ APP_* upload state).

use propagation::{Band, LogNeeds, ModeClass, NeedKind, NeedTag, OperatorNeeds};

/// The operator's actual Asiatic Russia 30m contacts, verbatim from their logbook —
/// note `30M` and `30m` both occur, every one is FT8, and none carries a SUBMODE.
const REAL_AR_30M: &[(&str, &str, &str, bool)] = &[
    ("R0SR", "30M", "FT8", true),
    ("RA9CUU", "30m", "FT8", true),
    ("UA9AX", "30m", "FT8", true),
    ("RF9C", "30M", "FT8", false),
    ("UA9JTJ", "30M", "FT8", false),
    ("RW9JZ", "30M", "FT8", false),
];

fn ar_log() -> LogNeeds {
    let mut n = LogNeeds::new();
    for (call, band, mode, confirmed) in REAL_AR_30M {
        n.add(call, band, mode, None, None, *confirmed);
    }
    n
}

/// The headline case. A digital spot of an Asiatic Russia station on 30m must not be
/// called a new mode: the operator has six FT8 contacts with that entity on that band.
#[test]
fn worked_entity_band_and_mode_is_not_a_new_mode() {
    let needs = ar_log();
    assert_ne!(
        needs.need("Asiatic Russia", Band::B30, ModeClass::Digital),
        NeedKind::NewMode,
        "30m FT8 Asiatic Russia is worked six times over — it can never be a new mode",
    );
}

/// The mixed-case band labels in the real log must both credit the 30m slot; if only
/// the lowercase rows counted, the entity would read as a new BAND on 30m.
#[test]
fn mixed_case_band_labels_credit_the_same_slot() {
    let mut upper = LogNeeds::new();
    upper.add("RF9C", "30M", "FT8", None, None, false);
    let mut lower = LogNeeds::new();
    lower.add("RF9C", "30m", "FT8", None, None, false);
    for n in [&upper, &lower] {
        assert_ne!(
            n.need("Asiatic Russia", Band::B30, ModeClass::Digital),
            NeedKind::NewBand,
            "a 30M/30m band label must credit the 30m slot either way",
        );
    }
}

/// Every digital spelling a re-import can produce must satisfy the same mode slot,
/// or a contact worked years ago comes back as a "new mode". This operator's log holds
/// FT8/FT4/MFSK/MSK144/RTTY/PSK31/PKT as bare MODE values with no SUBMODE, but a LoTW
/// or WSJT-X file re-imported later can spell FT4 as `MFSK` (the ADIF submode form) —
/// note the importer drops SUBMODE, so `MFSK` is what would land in the record. It has
/// to classify identically, which is what this pins.
#[test]
fn digital_submode_spellings_all_credit_the_digital_slot() {
    for spelling in [
        "FT8", "FT4", "MFSK", "MSK144", "RTTY", "PSK31", "PKT", "JS8", "FT1",
    ] {
        let mut n = LogNeeds::new();
        n.add("RF9C", "30m", spelling, None, None, false);
        assert_ne!(
            n.need("Asiatic Russia", Band::B30, ModeClass::Digital),
            NeedKind::NewMode,
            "{spelling} is a digital contact — it must satisfy the digital mode slot",
        );
    }
}

/// `PH` is what N3FJP-family loggers write for phone, and the operator's log carries
/// eight of them. Classed as Digital they did double damage: they credited a digital
/// mode slot they never earned AND left the phone slot looking unworked.
#[test]
fn ph_is_phone_not_digital() {
    assert_eq!(
        ModeClass::from_adif("PH"),
        ModeClass::Phone,
        "PH is the N3FJP phone token — 8 of the operator's QSOs use it",
    );
    let mut n = LogNeeds::new();
    n.add("RF9C", "30m", "PH", None, None, false);
    assert_eq!(
        n.need("Asiatic Russia", Band::B30, ModeClass::Digital),
        NeedKind::NewMode,
        "a phone contact must NOT satisfy the digital mode slot",
    );
    assert_ne!(
        n.need("Asiatic Russia", Band::B30, ModeClass::Phone),
        NeedKind::NewMode,
        "...and it must satisfy the phone slot",
    );
}

/// The rest of the voice vocabulary that reaches an imported log. Digital-voice modes
/// are voice contacts: counting them as data both fakes a digital credit and hides a
/// real phone need.
#[test]
fn voice_modes_classify_as_phone() {
    for m in [
        "SSB",
        "USB",
        "LSB",
        "AM",
        "FM",
        "PHONE",
        "PH",
        "DV",
        "C4FM",
        "DIGITALVOICE",
        "DSTAR",
        "FUSION",
        "M17",
        "FREEDV",
    ] {
        assert_eq!(
            ModeClass::from_adif(m),
            ModeClass::Phone,
            "{m} is a voice mode"
        );
    }
    // ...and the data modes stay data (guarding against an over-broad phone arm).
    for m in [
        "FT8", "FT4", "RTTY", "PSK31", "MFSK", "JS8", "OLIVIA", "SSTV", "",
    ] {
        assert_eq!(
            ModeClass::from_adif(m),
            ModeClass::Digital,
            "{m} is a data mode"
        );
    }
}

/// THE WORDING DEFECT. `worked_mode` is keyed (entity, mode-class) with no band — a
/// NewMode need means "never worked this entity in this mode class, on ANY band",
/// which is what the per-mode DXCC awards actually count. The headline used to append
/// the band the station happened to be heard on, so a true "never worked Asiatic
/// Russia on CW" need rendered as "New mode — CW Asiatic Russia 30m" and read as a
/// false claim about 30m to an operator with six 30m FT8 contacts there.
#[test]
fn new_mode_headline_does_not_claim_a_band() {
    let needs = ar_log();
    // Asiatic Russia's CQ zone on 30m is already worked, so the zone axis stays quiet
    // and NewMode is the top tag (the headline is built from tags[0]).
    let worked_zones: std::collections::HashSet<(u8, Band)> =
        [(17u8, Band::B30)].into_iter().collect();
    // A CW spot of an Asiatic Russia station on 30m: entity worked, band worked,
    // CW never worked -> a genuine NewMode, entity-wide.
    let alert = propagation::needalert::score(
        "RF9C",
        "30m",
        "CW",
        None,
        None,
        &needs,
        &worked_zones,
        &Default::default(),
        &Default::default(),
    )
    .expect("a never-worked mode class must raise an alert");
    assert!(
        alert.tags.contains(&NeedTag::NewMode),
        "CW is genuinely unworked for this entity: {:?}",
        alert.tags,
    );
    assert!(
        alert.headline.contains("CW"),
        "the headline must name the mode class it is claiming: {}",
        alert.headline,
    );
    assert!(
        !alert.headline.contains("30m"),
        "the NewMode predicate is entity-wide, so the headline must NOT claim a band \
         (the operator HAS worked this entity on 30m): {}",
        alert.headline,
    );
}
