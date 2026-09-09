//! The private carrier: how a field vector rides one ADIF tag.
//!
//! ADIF's `extra` passthrough is a flat **tag → value** map, and a field vector is a
//! vector of `(key, domain, raw)` triples. A vector of triples does not fit one tag
//! without an encoding, so here is one, and it is deliberately boring:
//!
//! ```text
//! NR:1;PREC:ss_precedence:A;CALL::W9XYZ;CK:74;SEC:arrl_sections:WI
//! ```
//!
//! Slots joined by `;`, each slot `key:domain:raw`, `domain` empty for a non-`Enum`
//! kind. ⚠️ `raw` is not always a constrained token — a casual NAME or QTH is free
//! text — so `%`, `;` and `:` are percent-encoded in **every** component before
//! joining and decoded after splitting. That is the whole codec.
//!
//! *Why not JSON in the field?* ADIF values may contain anything but `<`, so JSON is
//! legal — but it puts `{` and `"` into a value operators paste into other loggers'
//! import dialogs, and the general log's pairs and the journal's triples would need
//! one schema to serve two shapes. Two lines of codec beat a schema.
//!
//! **Which tags this rides is in [`fieldday`](crate::fieldday):** `APP_NEXUS_EX` for a
//! row's received exchange, `APP_NEXUS_MYEX` for its sent one.
use super::spec::{ExchangeSpec, FieldKind, FieldValue};

/// A field vector as one ADIF value.
pub fn encode(fields: &[FieldValue]) -> String {
    fields
        .iter()
        .map(|v| {
            format!(
                "{}:{}:{}",
                esc(v.key),
                esc(v.domain.unwrap_or("")),
                esc(&v.raw)
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// One ADIF value back into a field vector, resolved against the exchange that is
/// running.
///
/// Keys and domain ids are `&'static str` on a [`FieldValue`], so a decoded name has
/// to be resolved to the spec's own — which is also the right semantics: a slot the
/// running exchange does not declare is not a slot, and it is dropped rather than
/// leaked (a hostile journal would otherwise leak one `&'static str` per garbage key).
/// A domain the spec's matching slot does not offer decodes as `None`: this build
/// cannot bucket by a domain it does not carry, and claiming one it does not have
/// would be worse than saying so.
pub fn decode(s: &str, spec: &ExchangeSpec) -> Vec<FieldValue> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for slot in s.split(';') {
        let mut parts = slot.splitn(3, ':');
        let (Some(k), Some(d), Some(raw)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if let Some(v) = resolve(&unesc(k), &unesc(d), &unesc(raw), spec) {
            out.push(v);
        }
    }
    out
}

/// One `(key, domain, raw)` triple resolved against the running exchange — the whole
/// of what [`decode`] does per slot, factored out because the club wire and the host
/// journal carry the same triple in a struct instead of in a string. Two resolvers is
/// how the carrier and the wire come to disagree about what a domain means.
///
/// `None` = the running exchange declares no such slot.
pub fn resolve(key: &str, domain: &str, raw: &str, spec: &ExchangeSpec) -> Option<FieldValue> {
    let f = spec.field(key)?;
    Some(FieldValue {
        key: f.key,
        raw: raw.to_string(),
        domain: if domain.is_empty() {
            None
        } else {
            domain_ids(&f.kind).into_iter().find(|id| *id == domain)
        },
    })
}

/// A PAIR vector as one ADIF value — the general log's shape.
///
/// ⭐ **Why there are two encoders and only one format.** The contest log keeps the
/// matched domain because it is the scoring and export surface (§2.4); the general log
/// does not score, and a matched domain has no ADIF representation to round-trip
/// through, so `ContestFields.sent`/`rcvd` are `(slot, raw)` PAIRS. Rather than invent
/// a second wire shape for them, a pair rides the SAME `key:domain:raw` slot with the
/// domain component **empty** — so one reader can read both, and a value written by
/// either side is legible to the other. The asymmetry is in what is CARRIED, never in
/// how it is spelled.
pub fn encode_pairs(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, raw)| format!("{}::{}", esc(k), esc(raw)))
        .collect::<Vec<_>>()
        .join(";")
}

/// One ADIF value back into a pair vector.
///
/// Unlike [`decode`] there is no [`ExchangeSpec`] to resolve against — the general log
/// does not know which contest a foreign row belonged to, and a row merged from an
/// earlier session of the same contest must still give its slots back. So the key is
/// carried verbatim (trimmed and uppercased, which is the shape every slot id already
/// has) and a slot with a domain is read as its pair, discarding the domain rather
/// than refusing the row.
pub fn decode_pairs(s: &str) -> Vec<(String, String)> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for slot in s.split(';') {
        let mut parts = slot.splitn(3, ':');
        let (Some(k), Some(_domain), Some(raw)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let key = unesc(k).trim().to_ascii_uppercase();
        if key.is_empty() {
            continue;
        }
        out.push((key, unesc(raw)));
    }
    out
}

/// Every domain id a slot's kind can match — one for an `Enum`, one per `Enum` arm for
/// a `OneOf`, none otherwise.
fn domain_ids(kind: &FieldKind) -> Vec<&'static str> {
    match kind {
        FieldKind::Enum { domain } => vec![domain.id],
        FieldKind::OneOf(arms) => arms.iter().flat_map(domain_ids).collect(),
        _ => Vec::new(),
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' => out.push_str("%25"),
            ';' => out.push_str("%3B"),
            ':' => out.push_str("%3A"),
            _ => out.push(c),
        }
    }
    out
}

fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            match &s[i..i + 3] {
                "%25" => {
                    out.push('%');
                    i += 3;
                    continue;
                }
                "%3B" | "%3b" => {
                    out.push(';');
                    i += 3;
                    continue;
                }
                "%3A" | "%3a" => {
                    out.push(':');
                    i += 3;
                    continue;
                }
                _ => {}
            }
        }
        // Not one of our three escapes: copy the character whole (a multi-byte one
        // included — indexing by byte here would split a UTF-8 sequence).
        let ch = s[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::{casual, field_day};
    use crate::fieldday::FdEvent;

    fn fv(key: &'static str, raw: &str, domain: Option<&'static str>) -> FieldValue {
        FieldValue {
            key,
            raw: raw.into(),
            domain,
        }
    }

    #[test]
    fn a_field_day_exchange_round_trips_with_its_domain() {
        let spec = field_day(FdEvent::ArrlFd);
        let v = vec![
            fv("CLASS", "3A", None),
            fv("SECTION", "WI", Some("fd_sections")),
        ];
        let text = encode(&v);
        assert_eq!(text, "CLASS::3A;SECTION:fd_sections:WI");
        assert_eq!(decode(&text, spec), v);
    }

    /// The three separators inside a free-text value. `Text{16}` is real (a casual
    /// NAME or QTH), so this is not a hypothetical.
    #[test]
    fn separators_inside_a_value_survive() {
        let spec = casual();
        let v = vec![fv("QTH", "A;B:C%D", None)];
        let text = encode(&v);
        assert!(
            !text[3..].contains(';') && text.matches(':').count() == 2,
            "the value's separators are escaped, not emitted: {text}"
        );
        assert_eq!(decode(&text, spec), v);
    }

    #[test]
    fn an_empty_vector_and_an_empty_value_round_trip() {
        let spec = casual();
        assert_eq!(encode(&[]), "");
        assert_eq!(decode("", spec), Vec::<FieldValue>::new());
        let v = vec![fv("QTH", "", None)];
        assert_eq!(decode(&encode(&v), spec), v);
    }

    /// A slot the running exchange does not declare is dropped, and a domain it does
    /// not offer decodes as `None` rather than as a claim this build cannot honour.
    #[test]
    fn an_unknown_slot_is_dropped_and_an_unknown_domain_is_not_claimed() {
        let spec = casual();
        let got = decode("NOPE::x;QTH:invented_domain:MADISON", spec);
        assert_eq!(got, vec![fv("QTH", "MADISON", None)]);
    }

    /// Garbage never panics and never invents a slot — the journal is a file on disk
    /// and this build does not get to assume it is well formed.
    #[test]
    fn garbage_decodes_to_nothing_rather_than_panicking() {
        let spec = casual();
        for junk in ["", ";;;", ":", "QTH", "QTH:", "%", "%3", "%%%", "üñ:ï:ç"] {
            let _ = decode(junk, spec);
        }
        assert_eq!(decode(";;;", spec), Vec::<FieldValue>::new());
    }

    /// The general log's half of the codec: a pair rides the same slot with an empty
    /// domain, so the two encoders produce one format and not two.
    #[test]
    fn a_pair_vector_rides_the_same_slot_with_no_domain() {
        let pairs = vec![
            ("CLASS".to_string(), "3A".to_string()),
            ("SECTION".to_string(), "WI".to_string()),
        ];
        assert_eq!(encode_pairs(&pairs), "CLASS::3A;SECTION::WI");
        assert_eq!(decode_pairs("CLASS::3A;SECTION::WI"), pairs);
        // …and the triple encoder's output is legible to the pair reader, minus the
        // domain it deliberately does not carry.
        let triples = vec![fv("SECTION", "WI", Some("fd_sections"))];
        assert_eq!(
            decode_pairs(&encode(&triples)),
            vec![("SECTION".to_string(), "WI".to_string())]
        );
    }

    #[test]
    fn pair_garbage_decodes_to_nothing_rather_than_panicking() {
        for junk in [
            "", ";;;", ":", "QTH", "QTH:", "%", "%3", "%%%", "::x", " :: ",
        ] {
            let _ = decode_pairs(junk);
        }
        assert_eq!(decode_pairs(";;;"), Vec::<(String, String)>::new());
        assert_eq!(decode_pairs("::x"), Vec::<(String, String)>::new());
    }

    proptest::proptest! {
        /// §10's codec property: any value round-trips, including the empty string, a
        /// maximum-length one, and one made entirely of separators.
        #[test]
        fn every_value_round_trips(raw in proptest::string::string_regex("[%;:A-Za-z0-9 ]{0,64}").unwrap()) {
            let spec = casual();
            let v = vec![fv("QTH", &raw, None)];
            proptest::prop_assert_eq!(decode(&encode(&v), spec), v);
        }

        /// The same property for the general log's pairs — the half §10's
        /// "the ADIF round-trip of `ContestFields` is lossless" rests on.
        #[test]
        fn every_pair_round_trips(raw in proptest::string::string_regex("[%;:A-Za-z0-9 ]{0,64}").unwrap()) {
            let pairs = vec![("QTH".to_string(), raw)];
            proptest::prop_assert_eq!(decode_pairs(&encode_pairs(&pairs)), pairs);
        }
    }
}
