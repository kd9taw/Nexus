//! The `ExchangeSpec` family: the SHAPE of a contest exchange, as data.
//!
//! Every type here is `&'static` or a small scalar, so the whole family is `Copy` and
//! a shipped exchange is a `static` with no allocation and no initialisation order.
//! Nothing in this file reads a clock, a setting or the rules table.
//!
//! ⚠️ **A `FieldKind` says what a slot IS, never how a given modem COPIES it.** The
//! RTTY parser's garble tolerances (`TOO` → `599`, the `5NN` cut convention, a
//! lost-FIGS `EA` → `3A`) are artefacts of that modem's decoder and live in
//! `rtty::seq`, which is the only place they are true. A `FieldKind` that carried
//! them would silently rewrite a CW or FT8 log: a station that sends `TOO` on CW
//! sent `TOO`.

/// One contest's exchange: the fields it can carry and the roles that decide who
/// sends and receives which of them.
///
/// `fields` is the union over every role — a field appears once even when two roles
/// both use it, because `FieldSpec` is shared by the sent and received sides and
/// `RoleSpec` is what gives a slot a direction on a given row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeSpec {
    /// Stable internal name (`"casual"`, `"fieldday"`). Not operator-facing.
    pub name: &'static str,
    /// Every field any role sends or receives, in the order the exchange is read.
    pub fields: &'static [FieldSpec],
    /// The roles. One for a symmetric contest (FD/WFD/CQ WW/WPX/SS/VHF), two for
    /// ARRL DX, three for IARU and the QSO parties.
    pub roles: &'static [RoleSpec],
}

/// One slot in an exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec {
    /// SLOT ID, **not an export tag**. `RoleSpec::sends`/`receives`/`constant_sent`,
    /// and later `DupeRule.by_fields`/`by_sent_fields` and `MultiplierRule`, all name
    /// a slot by this string. Internal; never shown to an operator, never exported.
    pub key: &'static str,
    /// The default ADIF tags for this slot — **one per DIRECTION**. A slot has no
    /// direction; ADIF does (`SRX`/`STX`, `ARRL_SECT`/`MY_ARRL_SECT`). Either half
    /// may be `None`, meaning there is no standard ADIF column that way round and the
    /// value rides the private carrier instead of being exported under a tag that
    /// means something else.
    pub adif: AdifTags,
    /// The on-air label that introduces this field (`"NAME SETH"`), or `None` when
    /// the field is positional.
    pub label: Option<&'static str>,
    /// Whether a QSO can be logged without this field.
    pub required: bool,
    /// ⭐ **Where the value this slot SENDS comes from** (§3.4) — `"constant"`,
    /// `"serial"`, `"setting:<name>"`, `"derived:<what>"`, or `""` for a slot no role
    /// sends.
    ///
    /// It is carried on the SPEC rather than left in the rules file because the
    /// session constructor has to fill one value per sent slot, and the only other way
    /// to know which value is to re-derive the source from the `kind` — a second
    /// mapping, in a second place, that the loader's validator does not check. The
    /// loader already refuses a sent slot with no source or an unreadable one
    /// (`fd_rules::is_sent_source`), so what reaches here is a source this build
    /// declared it can supply; carrying it means the constructor honours that
    /// declaration instead of guessing at it.
    pub source: &'static str,
    /// What kind of value the slot holds — the SHAPE, never a modem's tolerance.
    pub kind: FieldKind,
}

/// The nine shapes an exchange field can take. Every researched contest field
/// collapses onto one of these; a tenth means a contest the model does not cover, not
/// a special case to bolt on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A signal report: 2 digits on phone, 3 on CW and digital.
    Rst { digits: u8 },
    /// A serial number. Per-contest is the only scope pass one supports;
    /// [`SerialScope::PerBand`] exists so a rules file that asks for it is refused
    /// by name rather than silently scored as per-contest.
    Serial { scope: SerialScope },
    /// A value that must be a member of a named domain — sections, states, counties,
    /// precedence letters, IARU societies.
    Enum { domain: &'static Domain },
    /// A value matching a pattern: the class designator `"^[0-9]{1,2}[ABCDEF]$"`, a
    /// Sweepstakes 2-digit check `"^[0-9]{2}$"`. Which patterns a given consumer can
    /// actually match is that consumer's business — a consumer with no matcher for a
    /// pattern must refuse it, never approximate it.
    Pattern { re: &'static str },
    /// A bounded integer: a CQ zone `1..=40`, an ITU zone `1..=90`.
    Number { min: u32, max: u32 },
    /// A Maidenhead locator of `chars` characters — 4 for ARRL VHF, 6 elsewhere.
    Grid { chars: u8 },
    /// Free text: ARRL DX power, a casual NAME or QTH.
    Text { max_len: u8 },
    /// A callsign inside the exchange (Sweepstakes).
    Call,
    /// One slot that legitimately carries values from more than one shape —
    /// county-or-state-or-DX, ITU-zone-or-society. Which arm matched is recorded on
    /// the parsed [`FieldValue`], because the arm decides both the multiplier bucket
    /// and the export tag.
    OneOf(&'static [FieldKind]),
}

/// How far a serial number counts before it resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialScope {
    /// One run of numbers for the whole contest. The only scope pass one supports.
    PerContest,
    /// A separate run per band. Nothing in the researched contest set uses this; it
    /// exists so a rules file asking for it can be refused by name.
    PerBand,
}

/// One side of an asymmetric contest: who I am, what I send, what I expect back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleSpec {
    /// `"in_state"`, `"out_of_state"`, `"w_ve"`, `"dx"`, or `""` for the single role
    /// of a symmetric contest.
    pub id: &'static str,
    /// How this role is chosen for the operator.
    pub selector: RoleSelector,
    /// Slot ids, in send order.
    pub sends: &'static [&'static str],
    /// Slot ids, in receive order.
    pub receives: &'static [&'static str],
    /// Sent slots that must carry the SAME value on every row of my log — the
    /// Sweepstakes check, and nothing else in the researched set.
    ///
    /// It lives here, on the send order, and **not** on [`FieldSpec`], because a
    /// `FieldSpec` is shared by sends and receives: the rule constrains the check I
    /// send, while every station I work legitimately sends a different one. Read the
    /// wrong way round, the flag refuses legal contacts.
    pub constant_sent: &'static [&'static str],
}

/// How a [`RoleSpec`] is matched against the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleSelector {
    /// My location is one of these (a section, a state, a province).
    MyLocationIn(&'static [&'static str]),
    /// My entered category matches this string.
    MyCategoryIs(&'static str),
    /// Unconditional — valid only when it is the exchange's ONLY role, because a
    /// second role after it could never be reached.
    Always,
}

/// A named set of legal values for an [`FieldKind::Enum`] slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    /// Stable id (`"arrl_sections"`, `"fd_sections"`, `"oh_counties"`).
    pub id: &'static str,
    /// The ADIF tags a value matched from this domain exports under, per direction.
    /// Both `None` means no ADIF column either way and the value rides the private
    /// carrier.
    pub adif: AdifTags,
    /// `(code, display name)` pairs. The code is what goes on the air.
    pub values: &'static [(&'static str, &'static str)],
}

/// The ADIF tags for one value, **one per direction**.
///
/// The export model has a direction: `CNTY` is the CONTACTED station's county and
/// `MY_CNTY` is mine, so a single tag applied to both sides of a QSO-party log moves
/// the other operator to my state on reimport. `None` means there is no standard ADIF
/// column that way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdifTags {
    /// What THEY sent me: `RST_RCVD`, `ARRL_SECT`, `SRX`, `SRX_STRING`.
    pub rcvd: Option<&'static str>,
    /// What I sent them: `RST_SENT`, `MY_ARRL_SECT`, `STX`, `STX_STRING`.
    pub sent: Option<&'static str>,
}

/// One copied exchange field, with the domain arm that matched it.
///
/// The domain travels on the value rather than being re-derived downstream, because a
/// value can be legal in more than one arm of a [`FieldKind::OneOf`]: re-deriving
/// picks the first arm that matches instead of the arm that was actually confirmed.
/// What was copied is a fact; re-deriving it is a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldValue {
    /// The [`FieldSpec::key`] this value fills.
    pub key: &'static str,
    /// Exactly what was copied, trimmed and uppercased.
    pub raw: String,
    /// WHICH domain arm matched. `None` for non-`Enum` kinds.
    pub domain: Option<&'static str>,
}

impl Domain {
    /// Is `v` a member? Trim + ASCII-uppercase, character for character the same
    /// normalisation `fd_rules::valid_section` applies — the RTTY parser's hardcoded
    /// `valid_section` call becomes this call in batch 0, and a stricter or looser
    /// test would be a behaviour change on the air.
    pub fn contains(&self, v: &str) -> bool {
        let up = v.trim().to_ascii_uppercase();
        self.values.iter().any(|(code, _)| *code == up)
    }
}

impl ExchangeSpec {
    /// The [`FieldSpec`] for a slot id. `sends`/`receives`/`constant_sent` all name
    /// slots by id, so this is the one resolution path.
    pub fn field(&self, key: &str) -> Option<&'static FieldSpec> {
        self.fields.iter().find(|f| f.key == key)
    }

    /// A copied value for one slot, carrying the domain the spec can name **without a
    /// parse** — the sole arm of an [`FieldKind::Enum`], and nothing else.
    ///
    /// `None` for a slot this exchange does not declare: a value with no slot is not a
    /// value, and inventing a `&'static str` key for it is how a garbage journal would
    /// leak one leaked string per garbage tag.
    ///
    /// ⚠️ **`raw` is stored verbatim.** Normalisation belongs to whatever COPIED the
    /// value — `rtty::seq` uppercases as it parses, an FT frame arrives uppercase — and
    /// doing it again here would silently rewrite what a shipped log already holds.
    /// A [`FieldKind::OneOf`] gets `None`: which arm matched is a fact the parse
    /// establishes, and re-deriving it here would be a guess (see [`FieldValue`]).
    pub fn value(&self, key: &str, raw: &str) -> Option<FieldValue> {
        let f = self.field(key)?;
        Some(FieldValue {
            key: f.key,
            raw: raw.to_string(),
            domain: match f.kind {
                FieldKind::Enum { domain } => Some(domain.id),
                _ => None,
            },
        })
    }

    /// A COPIED value for one slot, with the [`FieldKind::OneOf`] arm that matched it
    /// resolved.
    ///
    /// ⚠️ **The difference from [`value`](Self::value) is which side of the parse this
    /// is on, and it is the whole of §2.4.** `value` names the domain a spec can state
    /// WITHOUT a parse — the sole arm of an `Enum` — and deliberately gives a `OneOf`
    /// `None`, because re-deriving an arm downstream picks the first arm that matches
    /// rather than the arm that was actually confirmed. This is the function that DOES
    /// the confirming: it is called where the value is copied (the operator typed it
    /// into the entry strip, a frame carried it), which is the one moment the arm is a
    /// fact rather than a guess. Everything downstream then reads the recorded arm.
    ///
    /// A `OneOf` whose arms are all `Enum` and none of which holds `raw` yields a value
    /// with `domain: None` — the value is out of every declared universe, and saying so
    /// is what lets a multiplier bucket refuse to count it. A non-`Enum` arm (a `Text`
    /// catch-all, which is how TNQP and TXQP express "or anything else") also yields
    /// `None`: it has no domain to name.
    pub fn copied(&self, key: &str, raw: &str) -> Option<FieldValue> {
        let f = self.field(key)?;
        let up = raw.trim().to_ascii_uppercase();
        let domain = match f.kind {
            FieldKind::Enum { domain } => Some(domain.id),
            FieldKind::OneOf(arms) => arms.iter().find_map(|a| match a {
                FieldKind::Enum { domain } if domain.contains(&up) => Some(domain.id),
                _ => None,
            }),
            _ => None,
        };
        Some(FieldValue {
            key: f.key,
            raw: raw.to_string(),
            domain,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_DOMAIN: Domain = Domain {
        id: "test",
        adif: AdifTags {
            rcvd: Some("ARRL_SECT"),
            sent: None,
        },
        values: &[("WI", "Wisconsin"), ("EMA", "Eastern Massachusetts")],
    };

    /// The membership test must match `fd_rules::valid_section`'s normalisation
    /// character for character — Task 0.5 swaps one for the other inside the RTTY
    /// parser, and a stricter or looser domain is a behaviour change.
    #[test]
    fn domain_membership_trims_and_uppercases_like_valid_section() {
        assert!(TEST_DOMAIN.contains("WI"));
        assert!(TEST_DOMAIN.contains("  wi "));
        assert!(TEST_DOMAIN.contains("Wi"));
        assert!(!TEST_DOMAIN.contains("W I"));
        assert!(!TEST_DOMAIN.contains("XX"));
        assert!(!TEST_DOMAIN.contains(""));
    }

    #[test]
    fn a_spec_finds_its_own_fields_and_only_its_own() {
        static F: &[FieldSpec] = &[FieldSpec {
            key: "RST",
            adif: AdifTags {
                rcvd: Some("RST_RCVD"),
                sent: Some("RST_SENT"),
            },
            label: None,
            required: true,
            source: "constant",
            kind: FieldKind::Rst { digits: 3 },
        }];
        static R: &[RoleSpec] = &[RoleSpec {
            id: "",
            selector: RoleSelector::Always,
            sends: &["RST"],
            receives: &["RST"],
            constant_sent: &[],
        }];
        static S: ExchangeSpec = ExchangeSpec {
            name: "t",
            fields: F,
            roles: R,
        };
        assert_eq!(S.field("RST").map(|f| f.key), Some("RST"));
        assert!(S.field("NOPE").is_none());
    }
}
