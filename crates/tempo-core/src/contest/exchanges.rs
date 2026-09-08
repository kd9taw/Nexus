//! The two exchanges this build ships, as [`ExchangeSpec`] instances.
//!
//! They are byte-for-byte the same exchanges `rtty::seq::CASUAL` and
//! `rtty::seq::FIELD_DAY` described before batch 0 — same slots, same order, same
//! labels, same required flags. What changed is the TYPE: the garble tolerances that
//! used to be baked into the field kinds (`Rst` normalising `TOO`, `FdClass`
//! forgiving `EA`) now live in `rtty::seq`'s own matcher table, where they are true.
//!
//! Batch 1 makes the rules file able to CARRY an exchange block; these two stay the
//! shipped definition until a later batch wires the loaded block through.
use super::spec::{AdifTags, ExchangeSpec, FieldKind, FieldSpec, RoleSelector, RoleSpec};
use crate::fieldday::FdEvent;
use std::sync::OnceLock;

static CASUAL_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        key: "RST",
        // Both halves corroborated by a real write path in this tree:
        // logbook.rs writes <RST_SENT> and <RST_RCVD> and reads them back on import.
        adif: AdifTags {
            rcvd: Some("RST_RCVD"),
            sent: Some("RST_SENT"),
        },
        label: None,
        required: true,
        kind: FieldKind::Rst { digits: 3 },
    },
    FieldSpec {
        key: "NAME",
        // ADIF <NAME> is the CONTACTED station's name, so it is a received-side tag
        // only, and logbook.rs writes it. There is no corroborated sent-side name tag
        // anywhere in this tree (§2.1.1), so the sent half ships absent.
        adif: AdifTags {
            rcvd: Some("NAME"),
            sent: None,
        },
        label: Some("NAME"),
        required: false,
        kind: FieldKind::Text { max_len: 16 },
    },
    FieldSpec {
        key: "QTH",
        // Same shape as NAME: logbook.rs writes <QTH>, and there is no corroborated
        // sent-side QTH tag in this tree.
        adif: AdifTags {
            rcvd: Some("QTH"),
            sent: None,
        },
        label: Some("QTH"),
        required: false,
        kind: FieldKind::Text { max_len: 16 },
    },
];

static CASUAL_ROLES: &[RoleSpec] = &[RoleSpec {
    id: "",
    selector: RoleSelector::Always,
    sends: &["RST", "NAME", "QTH"],
    receives: &["RST", "NAME", "QTH"],
    constant_sent: &[],
}];

static CASUAL: ExchangeSpec = ExchangeSpec {
    name: "casual",
    fields: CASUAL_FIELDS,
    roles: CASUAL_ROLES,
};

/// Casual ragchew: RST required, name and QTH picked up when labeled.
pub fn casual() -> &'static ExchangeSpec {
    &CASUAL
}

static FD_ROLES: &[RoleSpec] = &[RoleSpec {
    id: "",
    selector: RoleSelector::Always,
    sends: &["CLASS", "SECTION"],
    receives: &["CLASS", "SECTION"],
    constant_sent: &[],
}];

/// The class-designator pattern for an event, as an anchored `Pattern` source.
///
/// The two events do NOT share a letter set, and shipping one that covers both
/// would be worse than either: it would accept `2M` as an ARRL Field Day entry
/// and `3A` as a Winter Field Day one, logging a class its sponsor does not
/// define.
///
/// * ARRL Field Day — `A`–`F`. arrl.org/field-day-rules (2026 rules, rev.
///   2026-03-01): Class A club/group portable, B one-or-two-person portable,
///   C mobile, D home stations, E home on emergency power, F EOC.
/// * Winter Field Day — `H`, `I`, `O`, `M`. winterfieldday.org's own rules PDF
///   (`downloads/2026-rules-v3.pdf`, stamped "V3 9.8.25", read in full
///   2026-09-07) states verbatim: *"Class Options: H - Home station… I -
///   Indoor station… O - Outdoor station… M - Mobile / Mobile Stationary"*,
///   with the worked example *"If you have two stations and you are mobile in
///   East Pennsylvania, you are 2M EPA."*
///
/// Ordering inside the bracket is the sponsor's own presentation order, not
/// alphabetical, so the string reads back against the rules text it came from.
const fn class_pattern(event: FdEvent) -> &'static str {
    match event {
        FdEvent::ArrlFd => "^[0-9]{1,2}[ABCDEF]$",
        FdEvent::WinterFd => "^[0-9]{1,2}[HIOM]$",
    }
}

/// ARRL / Winter Field Day: class + section, both required.
///
/// ⚠️ This LOADS THE RULES TABLE (for the section domain) and therefore carries
/// [`crate::fd_rules::ruleset`]'s ordering rule — never call it before the startup
/// `fd_rules::install_from`, or the bundled seed is locked in for the session.
/// Its live callers are in `Engine::set_rtty_auto`, an operator action.
///
/// **Per-event by necessity, not by taste.** The class letter sets are disjoint
/// (see [`class_pattern`]), so a single shared exchange cannot be correct for
/// both: until 2026-09-07 this function returned one spec matching `ABCDEF` for
/// both events, which meant the RTTY auto-sequencer could not complete a single
/// Winter Field Day contact — none of H/I/O/M satisfied the required CLASS slot,
/// so the QSO stalled unlogged. The SECTION half is genuinely shared: both
/// events use the ARRL/RAC sections plus `MX` and `DX` (WFD rules, "Location
/// Identifier": *"Mexico stations will use MX, and all other stations outside of
/// the US will use DX."*).
pub fn field_day(event: FdEvent) -> &'static ExchangeSpec {
    static ARRL: OnceLock<ExchangeSpec> = OnceLock::new();
    static WFD: OnceLock<ExchangeSpec> = OnceLock::new();
    let cell = match event {
        FdEvent::ArrlFd => &ARRL,
        FdEvent::WinterFd => &WFD,
    };
    cell.get_or_init(|| {
        let section_domain = crate::fd_rules::fd_sections_domain();
        let fields: &'static [FieldSpec] = Box::leak(Box::new([
            FieldSpec {
                key: "CLASS",
                // fieldday.rs's ADIF exporter already writes <CLASS>; nothing in this
                // tree names a sent-side class tag, so that half ships absent.
                adif: AdifTags {
                    rcvd: Some("CLASS"),
                    sent: None,
                },
                label: None,
                required: true,
                kind: FieldKind::Pattern {
                    re: class_pattern(event),
                },
            },
            FieldSpec {
                key: "SECTION",
                // The section slot exports under its DOMAIN's tags, so the two can
                // never disagree about which column a copied section lands in.
                adif: section_domain.adif,
                label: None,
                required: true,
                kind: FieldKind::Enum {
                    domain: section_domain,
                },
            },
        ]));
        ExchangeSpec {
            name: "fieldday",
            fields,
            roles: FD_ROLES,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::{FieldKind, RoleSelector};

    /// §2.5, enforced here on the two SHIPPED exchanges a batch before the rules
    /// loader enforces it on downloaded ones. A `sends`/`receives` entry naming a slot
    /// that does not exist is a rules bug that must never reach a runtime lookup.
    #[test]
    fn every_role_names_only_declared_slots_and_always_is_alone() {
        for spec in [
            casual(),
            field_day(FdEvent::ArrlFd),
            field_day(FdEvent::WinterFd),
        ] {
            assert!(!spec.roles.is_empty(), "{}: no roles", spec.name);
            for r in spec.roles {
                for key in r.sends.iter().chain(r.receives).chain(r.constant_sent) {
                    assert!(
                        spec.field(key).is_some(),
                        "{}: role {:?} names unknown slot {key}",
                        spec.name,
                        r.id
                    );
                }
                for key in r.constant_sent {
                    assert!(
                        r.sends.contains(key),
                        "{}: constant_sent {key} is not sent",
                        spec.name
                    );
                }
                assert!(
                    r.receives.len() <= 5,
                    "{}: role {:?} receives > 5 fields",
                    spec.name,
                    r.id
                );
                if matches!(r.selector, RoleSelector::Always) {
                    assert_eq!(
                        spec.roles.len(),
                        1,
                        "{}: Always must be the only role",
                        spec.name
                    );
                }
            }
        }
    }

    /// The class pattern is the SPONSOR'S letter set, per event, and the two are
    /// disjoint. Sources are quoted on [`class_pattern`]; the Winter Field Day
    /// half is `downloads/2026-rules-v3.pdf` ("V3 9.8.25") read in full
    /// 2026-09-07. Until that read, both events shipped `ABCDEF`, which made
    /// every Winter Field Day contact unloggable through the RTTY sequencer.
    #[test]
    fn each_event_carries_its_own_sponsors_class_letters() {
        let arrl = field_day(FdEvent::ArrlFd).field("CLASS").expect("CLASS");
        assert_eq!(
            arrl.kind,
            FieldKind::Pattern {
                re: "^[0-9]{1,2}[ABCDEF]$"
            }
        );
        let wfd = field_day(FdEvent::WinterFd).field("CLASS").expect("CLASS");
        assert_eq!(
            wfd.kind,
            FieldKind::Pattern {
                re: "^[0-9]{1,2}[HIOM]$"
            }
        );
        assert_ne!(
            arrl.kind, wfd.kind,
            "the two events do not share a class set"
        );
        for f in [arrl, wfd] {
            assert!(f.required);
            assert_eq!(f.label, None, "the class is positional, not labeled");
        }
    }

    /// The SECTION half genuinely IS shared — both sponsors use the ARRL/RAC
    /// sections plus `MX`/`DX`. Pinned so a later per-event split does not
    /// quietly fork a domain that has no reason to fork.
    #[test]
    fn both_events_share_one_section_domain() {
        let a = field_day(FdEvent::ArrlFd)
            .field("SECTION")
            .expect("SECTION");
        let w = field_day(FdEvent::WinterFd)
            .field("SECTION")
            .expect("SECTION");
        assert_eq!(a.kind, w.kind);
        assert_eq!(a.adif, w.adif);
    }

    #[test]
    fn the_casual_exchange_keeps_its_labels_and_optionality() {
        let s = casual();
        assert_eq!(s.name, "casual");
        assert!(s.field("RST").unwrap().required);
        assert_eq!(s.field("NAME").unwrap().label, Some("NAME"));
        assert!(!s.field("NAME").unwrap().required);
        assert_eq!(s.field("QTH").unwrap().label, Some("QTH"));
        assert!(!s.field("QTH").unwrap().required);
    }
}
