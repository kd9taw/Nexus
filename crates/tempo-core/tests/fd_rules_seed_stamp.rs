//! The bundled rules seed never changes under the stamp it was published with.
//!
//! `src/fd_rules.seed.json` is both the seed every build bundles and the file a push to main
//! publishes to every station (`.github/workflows/fd-rules.yml`), and its top-level `generated`
//! stamp is the only version either side compares. A downloaded file stamped the same as the
//! bundled seed is the one used (fd_rules_install_equal_stamp.rs), and "Check for rules
//! updates" keeps the file it holds when the hosted one carries the same stamp. So a seed edit
//! that keeps its stamp never reaches a station that already downloaded the file of that stamp.
//! It happened once: contest declarations were added under an unmoved stamp.
//!
//! This pins the stamp and a digest of the rest of the file, and fails, saying what to do, when
//! either moves, or when the stamp is not in the fixed `YYYY-MM-DDTHH:MM:SSZ` form that a
//! plain-string comparison needs. The digest is FNV-1a 64 over the parsed JSON without `generated`, serialized
//! again with every object's keys sorted, so line endings, whitespace and key order are not
//! changes, while a value, an array's order or a key added or removed is. The sort is explicit
//! (`sort_all_objects`) so the digest does not depend on whether some crate in the build turns
//! on serde_json's `preserve_order`; in this workspace none does, and the sort does no work.

use std::cmp::Ordering;

const SEED: &str = include_str!("../src/fd_rules.seed.json");

/// The bundled seed's `generated` stamp and the digest of everything else in it. Change them
/// only to what a failure of `the_bundled_seed_changes_only_with_a_later_stamp` prints.
const PINNED_STAMP: &str = "2026-10-26T18:00:00Z";
const PINNED_DIGEST: u64 = 0xb2ecfc4d60cdb934;

/// `Ok` when `seed` carries the pinned stamp and content; otherwise what to do about it. Stamps
/// compare as plain strings, as `fd_rules::install_from` compares them, which orders them only in
/// the fixed `YYYY-MM-DDTHH:MM:SSZ` form, so a stamp in any other form is refused first.
fn check_seed_stamp(pinned_stamp: &str, pinned_digest: u64, seed: &[u8]) -> Result<(), String> {
    let (stamp, digest) = stamp_and_digest(seed)?;
    if !is_fixed_form_utc(&stamp) {
        return Err(format!(
            "fd_rules.seed.json's `generated` is {stamp:?}, not in the YYYY-MM-DDTHH:MM:SSZ form. \
             Stamps compare as plain strings, so a stamp in any other form can order wrongly \
             against the ones stations hold. Write it like 2026-10-12T08:00:00Z, later than \
             {pinned_stamp}, then update both pins in this test: run it again and it prints them."
        ));
    }
    match stamp.as_str().cmp(pinned_stamp) {
        Ordering::Equal if digest == pinned_digest => Ok(()),
        Ordering::Equal => Err(format!(
            "fd_rules.seed.json changed, but its `generated` stamp is still {stamp}. A station \
             that downloaded the rules file stamped {stamp} keeps using that download over a \
             bundled seed with the same stamp, and \"Check for rules updates\" keeps a file \
             whose stamp it already holds, so this change would never reach it; and a push to \
             main that touches the seed publishes it to every station. Move `generated` to a \
             later UTC time, in the same YYYY-MM-DDTHH:MM:SSZ form, then update both pins in \
             this test: run it again and it prints them."
        )),
        Ordering::Greater => Err(format!(
            "fd_rules.seed.json's `generated` moved from {pinned_stamp} to {stamp}. Pin the new \
             stamp and the digest of the seed's content in this test:\n\
             const PINNED_STAMP: &str = \"{stamp}\";\n\
             const PINNED_DIGEST: u64 = {digest:#018x};"
        )),
        Ordering::Less => Err(format!(
            "fd_rules.seed.json's `generated` is {stamp}, earlier than the pinned \
             {pinned_stamp}, so it loses to the rules stations already hold: a downloaded file \
             stamped {pinned_stamp} is used over an older bundled seed, and a build that bundles \
             {pinned_stamp} refuses an older download. Move `generated` to a UTC time later \
             than {pinned_stamp}, even to undo a change, then update both pins in this test: \
             run it again and it prints them."
        )),
    }
}

/// The seed's `generated` stamp, and the digest of everything else in it.
fn stamp_and_digest(seed: &[u8]) -> Result<(String, u64), String> {
    let mut json: serde_json::Value =
        serde_json::from_slice(seed).map_err(|e| format!("fd_rules.seed.json is not JSON: {e}"))?;
    let stamp = json
        .as_object_mut()
        .and_then(|top| top.remove("generated"))
        .and_then(|generated| generated.as_str().map(str::to_owned))
        .ok_or("fd_rules.seed.json has no top-level `generated` string")?;
    json.sort_all_objects();
    Ok((stamp, fnv1a_64(json.to_string().as_bytes())))
}

/// `YYYY-MM-DDTHH:MM:SSZ`: digits where the template has `d`, the separators exactly.
fn is_fixed_form_utc(stamp: &str) -> bool {
    const TEMPLATE: &[u8] = b"dddd-dd-ddTdd:dd:ddZ";
    stamp.len() == TEMPLATE.len()
        && stamp.bytes().zip(TEMPLATE).all(|(c, &t)| match t {
            b'd' => c.is_ascii_digit(),
            sep => c == sep,
        })
}

/// FNV-1a, 64-bit: the same value on every build, platform and toolchain, with no dependency.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn the_bundled_seed_changes_only_with_a_later_stamp() {
    if let Err(what_to_do) = check_seed_stamp(PINNED_STAMP, PINNED_DIGEST, SEED.as_bytes()) {
        panic!("{what_to_do}");
    }
}

/// A seed small enough to read whole, stamped like the bundled one.
const SYNTHETIC: &str = r#"{
  "schema": 2,
  "generated": "2026-10-12T08:00:00Z",
  "rulesets": [{ "event": "arrlfd", "points": 1 }, { "event": "wfd", "points": 2 }]
}"#;
const SYNTHETIC_STAMP: &str = "2026-10-12T08:00:00Z";

/// `SYNTHETIC` with each `(from, to)` applied. Each `from` must be there, so an edit can never
/// quietly change nothing and pass a test that wanted a change.
fn edited(edits: &[(&str, &str)]) -> String {
    edits.iter().fold(SYNTHETIC.to_owned(), |seed, (from, to)| {
        assert!(
            seed.contains(from),
            "fixture anchor: {from:?} is in the seed"
        );
        seed.replace(from, to)
    })
}

fn digest_of(seed: &str) -> u64 {
    stamp_and_digest(seed.as_bytes())
        .expect("the synthetic seed reads")
        .1
}

#[test]
fn a_seed_at_its_pinned_stamp_and_digest_passes() {
    let pinned = digest_of(SYNTHETIC);
    assert_eq!(
        check_seed_stamp(SYNTHETIC_STAMP, pinned, SYNTHETIC.as_bytes()),
        Ok(())
    );
}

#[test]
fn a_content_change_under_the_same_stamp_fails_and_says_to_move_the_stamp() {
    let changed = edited(&[(r#""points": 1"#, r#""points": 3"#)]);
    let why = check_seed_stamp(SYNTHETIC_STAMP, digest_of(SYNTHETIC), changed.as_bytes())
        .expect_err("content changed under the pinned stamp");
    assert!(
        why.contains("changed, but its `generated` stamp is still 2026-10-12T08:00:00Z"),
        "{why}"
    );
    assert!(
        why.contains("Move `generated` to a later UTC time"),
        "{why}"
    );
}

#[test]
fn a_later_stamp_fails_until_both_pins_follow_it() {
    let later = "2026-10-13T00:00:00Z";
    let pinned = digest_of(SYNTHETIC);
    // The stamp moved alone, and with the content: either way the new pair must be pinned.
    for moved in [
        edited(&[(SYNTHETIC_STAMP, later)]),
        edited(&[
            (SYNTHETIC_STAMP, later),
            (r#""points": 1"#, r#""points": 3"#),
        ]),
    ] {
        let why =
            check_seed_stamp(SYNTHETIC_STAMP, pinned, moved.as_bytes()).expect_err("a later stamp");
        let pins = format!(
            "const PINNED_STAMP: &str = \"{later}\";\nconst PINNED_DIGEST: u64 = {:#018x};",
            digest_of(&moved)
        );
        assert!(why.ends_with(&pins), "{why}");
        assert_eq!(
            check_seed_stamp(later, digest_of(&moved), moved.as_bytes()),
            Ok(()),
            "pinned as printed, it passes"
        );
    }
}

#[test]
fn an_earlier_stamp_fails_even_with_the_content_unchanged() {
    let earlier = edited(&[(SYNTHETIC_STAMP, "2026-10-11T23:59:59Z")]);
    let why = check_seed_stamp(SYNTHETIC_STAMP, digest_of(SYNTHETIC), earlier.as_bytes())
        .expect_err("an earlier stamp");
    assert!(
        why.contains("is 2026-10-11T23:59:59Z, earlier than the pinned 2026-10-12T08:00:00Z"),
        "{why}"
    );
    assert!(
        why.contains("Move `generated` to a UTC time later than 2026-10-12T08:00:00Z"),
        "{why}"
    );
}

#[test]
fn a_stamp_not_in_the_fixed_form_fails_and_names_the_form() {
    // Stamps compare as plain strings, which orders them only in the fixed form:
    // "2026-10-12T9:00:00Z" sorts after "2026-10-12T10:00:00Z", because '9' > '1'.
    for odd in [
        "2026-10-12T9:00:00Z",
        "2026-10-12 08:00:00Z",
        "2026-10-12T08:00:00+00:00",
        "2026-10-12",
    ] {
        let seed = edited(&[(SYNTHETIC_STAMP, odd)]);
        let why = check_seed_stamp(SYNTHETIC_STAMP, digest_of(SYNTHETIC), seed.as_bytes())
            .expect_err("a stamp in another form");
        assert!(
            why.contains("not in the YYYY-MM-DDTHH:MM:SSZ form"),
            "{odd}: {why}"
        );
    }
}

#[test]
fn the_digest_reads_the_content_not_its_layout_key_order_or_stamp() {
    let base = digest_of(SYNTHETIC);
    let compact = serde_json::from_str::<serde_json::Value>(SYNTHETIC)
        .unwrap()
        .to_string();
    for same in [
        SYNTHETIC.replace('\n', "\r\n"),
        compact,
        edited(&[(
            r#""event": "wfd", "points": 2"#,
            r#""points": 2, "event": "wfd""#,
        )]),
        edited(&[(SYNTHETIC_STAMP, "2027-01-01T00:00:00Z")]),
    ] {
        assert_eq!(digest_of(&same), base, "not a content change: {same}");
    }
    for changed in [
        edited(&[(r#""points": 2"#, r#""points": 20"#)]),
        edited(&[(
            r#"[{ "event": "arrlfd", "points": 1 }, { "event": "wfd", "points": 2 }]"#,
            r#"[{ "event": "wfd", "points": 2 }, { "event": "arrlfd", "points": 1 }]"#,
        )]),
        edited(&[(r#""schema": 2,"#, r#""schema": 2, "note": "","#)]),
    ] {
        assert_ne!(digest_of(&changed), base, "a content change: {changed}");
    }
}
