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

/// ARRL / Winter Field Day: class + section, both required.
///
/// ⚠️ This LOADS THE RULES TABLE (for the section domain) and therefore carries
/// [`crate::fd_rules::ruleset`]'s ordering rule — never call it before the startup
/// `fd_rules::install_from`, or the bundled seed is locked in for the session.
/// Its live callers are in `Engine::set_rtty_auto`, an operator action.
///
/// The class pattern's letter set is TODAY's, for both events: the shipped RTTY
/// parser matches `ABCDEF` for ARRL FD and Winter FD alike. A code comment in that
/// parser guesses WFD "would pass HIOM"; a code comment is not a source, and changing
/// the letter set is a behaviour change that needs the sponsor's own page read first.
pub fn field_day() -> &'static ExchangeSpec {
    static S: OnceLock<ExchangeSpec> = OnceLock::new();
    S.get_or_init(|| {
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
                    re: "^[0-9]{1,2}[ABCDEF]$",
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
        for spec in [casual(), field_day()] {
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

    /// The class pattern must carry TODAY's letter set. `rtty/seq.rs` guesses in a
    /// comment that a WFD schema "would pass HIOM" — that is a code comment, not a
    /// source, and the shipped parser uses ABCDEF for both events. Changing it is a
    /// behaviour change that needs winterfieldday.org read first (Ruling B1-D).
    #[test]
    fn the_field_day_class_pattern_is_todays_letter_set() {
        let f = field_day().field("CLASS").expect("CLASS slot");
        assert_eq!(
            f.kind,
            FieldKind::Pattern {
                re: "^[0-9]{1,2}[ABCDEF]$"
            }
        );
        assert!(f.required);
        assert_eq!(f.label, None, "the class is positional, not labeled");
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
