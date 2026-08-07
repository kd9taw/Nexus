//! Differential gate: every Nexus predicate that claims to transcribe a WSJT-X regex is checked
//! against what the REAL Qt regex actually answered, over a 25,365-input corpus.
//!
//! WHY THIS EXISTS. We vendored WSJT-X's DSP and its 77-bit packer byte-identical, but not its
//! sequencer: `stdCall` lives in `widgets/mainwindow.cpp`, a Qt GUI class with no counterpart in
//! our vendor tree. So `message.rs` re-derives by hand what upstream states as a regex. On
//! 2026-08-06 five FT-mode bugs landed on exactly that seam, and the pattern was uniform:
//! **everywhere we transcribed, it was right first time; everywhere we re-derived, it broke.**
//! `is_compound` ("contains a slash") stood in for `stdCall`, and F4CYH/P went on the air with
//! neither a grid nor a report. This file is the instrument that tells transcription from
//! re-derivation, so the next drift is a red gate instead of an on-air report.
//!
//! WHAT IT DOES NOT DO. It answers "did we transcribe faithfully?", never "is the answer right?"
//! Where upstream is itself wrong — the `/P`×`/R` pair that packs as a callsign belonging to
//! nobody, the colliding Tx1/2/3 triple for a hashed DX, the nine class pairs that could not
//! finish — upstream agrees with the bug and this gate is blind. Those are owned by
//! `portable_suffix_air.rs` (does the wire survive?) and `qso_class_pairs_loopback.rs` (does the
//! QSO reach 73?). Three questions, three files; none is sufficient alone.
//!
//! THE ORACLE. `tests/fixtures/wsjtx-callsign-oracle.json`, written by
//! `scripts/gen-wsjtx-callsign-oracle.mjs`: it fetches `widgets/mainwindow.cpp` and `Radio.cpp`
//! from upstream master, EXTRACTS the patterns rather than re-typing them, compiles them into a
//! throwaway Qt harness with their real flags, and commits only the measurements. The fetch
//! happens when a maintainer regenerates; **this test makes no network call**, because a gate
//! that goes red when SourceForge is down is a gate people disable. Provenance (branch tip,
//! per-file sha256, pattern text, flags) rides in the fixture so staleness is detectable.
//!
//! HOW THE COMPARISON IS STATED, and this is the load-bearing design decision. A flat
//! `nexus == upstream` assertion is not the truth and would have to be suppressed within a week:
//! **Nexus normalises its input and upstream does not.** Both Rust predicates begin
//! `call.trim().to_ascii_uppercase()`; `Radio::is_77bit_nonstandard_callsign` matches
//! the callsign-alphabet class against the raw string, case-SENSITIVELY and with no leading-space
//! tolerance at all. So on `"w1aw/w4"` upstream says "not nonstandard" (it is not even in the
//! callsign alphabet) while Nexus says "nonstandard" — thousands of such rows across the 5,233
//! un-normalised inputs here, none of them a defect. The gate therefore asserts two separate,
//! precise things:
//!
//!   1. **Transcription** — on the NORMALISED domain (20,132 inputs: already trimmed, already
//!      uppercase, which is every string a decoded 77-bit frame or a validated Configuration can
//!      produce, and exhaustive over {A,R,P,9,/} to length 6 — where the grammar tops out), the
//!      predicates agree with upstream EXACTLY. Zero tolerance, and this is the real gate.
//!   2. **Normalisation** — off that domain, `nexus(x) == upstream(normalise(x))`. That pins the
//!      normalisation rule itself instead of waving at it, and it fails if someone drops the
//!      `trim()`, swaps it for a different one, or starts normalising something else.
//!
//! What survives both is declared literally in [`DECLARED_DIVERGENCES`] with its reason. Today
//! that is one thing: **Unicode case folding.** Qt builds PCRE2 in UTF mode, so
//! `CaseInsensitiveOption` folds U+017F LONG S to S and U+212A KELVIN SIGN to K, and upstream's
//! `stdCall` accepts `ſ9XYZ`; `to_ascii_uppercase` cannot and does not. Nexus's answer routes
//! such a call down the hashed forms, which is the safe direction, and no decoded frame can
//! contain one (the 77-bit alphabet is 42 ASCII characters) — it takes a hand-typed DX call.
//!
//! THE OTHER FLAGS, measured rather than reasoned about: `ExtendedPatternSyntaxOption` is PCRE
//! `/x` (drop it and `( /R | /P )?` demands literal spaces and the pattern matches nothing at
//! all); Qt compiles PCRE2 without `PCRE2_UCP`, so its `\s` is ASCII-only — NARROWER than Rust's
//! `str::trim`, which eats NBSP and U+2028; and PCRE2's default `$` also matches before a final
//! newline, which is masked inside `stdCall` (its `\s*` eats the newline first) and live in the
//! two Radio.cpp regexes. A naive port is right where it does not matter and wrong where it does.

use std::collections::HashMap;
use std::path::PathBuf;
use tempo_core::message::{base_call, is_77bit_nonstandard_call, is_std_call};

/// Inputs where Nexus disagrees with upstream for a reason that is NOT normalisation, with what
/// each side says and why.
///
/// This list is as much the point of the gate as the assertions are: the written record of every
/// place we are not a transcription. It fails **both ways** — a new divergence is red, and so is
/// "fixing" a declared one without deleting its row.
///
/// `(input, upstream_std_call, upstream_nonstandard77, why)`.
const DECLARED_DIVERGENCES: &[(&str, bool, bool, &str)] = &[
    // Qt's CaseInsensitiveOption folds through Unicode; `to_ascii_uppercase` does not. Upstream
    // reads these as standard calls; Nexus reads them as nonstandard and hashes them, which is
    // the safe direction. Unreachable from a decoded frame — the 77-bit alphabet is ASCII — so
    // it takes a hand-typed DX call to reach one. Written with escapes on purpose: these
    // characters are visually identical to the ASCII letters they fold to, which is the whole
    // hazard, and a literal in the source would read as a bug rather than a case.
    (
        "\u{17f}9XYZ",
        true,
        false,
        "U+017F LATIN SMALL LETTER LONG S folds to S under PCRE2 UTF-mode caseless",
    ),
    (
        "\u{212a}1ABC",
        true,
        false,
        "U+212A KELVIN SIGN folds to K — leading position",
    ),
    (
        "W9XY\u{212a}",
        true,
        false,
        "U+212A KELVIN SIGN folds to K — trailing position",
    ),
];

/// `base_call` vs upstream `Radio::base_callsign` — the divergences Nexus OWNS.
///
/// Upstream splits on the FIRST '/' and keeps the LONGER side; Nexus keeps the last
/// callsign-shaped segment. On air Nexus's rule is the better one (`W1AW/PORTABLE` is W1AW
/// working portable, not a station called PORTABLE) but "better" and "the same" are different
/// claims and the doc comment used to make the wrong one. `(input, upstream, nexus)`.
const DECLARED_BASE_DIVERGENCES: &[(&str, &str, &str)] = &[
    // Upstream takes the longer side of the FIRST slash, so an operator announcing how he is
    // operating gets logged under the announcement. Nexus takes the last callsign-shaped
    // segment, so `W1AW/PORTABLE` stays W1AW — which is what the addressed-to-me and
    // from-the-DX tests need, and why the rule was written this way.
    ("W1AW/PORTABLE", "PORTABLE", "W1AW"),
    ("AA1A/QRPP", "QRPP", "AA1A"),
];

struct Oracle {
    inputs: Vec<String>,
    std_call: Vec<bool>,
    nonstandard77: Vec<bool>,
    base_inputs: Vec<String>,
    base: Vec<String>,
    json: serde_json::Value,
}

impl Oracle {
    /// Upstream's verdicts keyed by input, for the normalisation claim — which has to ask what
    /// upstream said about a DIFFERENT string than the one under test.
    fn by_input(&self) -> HashMap<&str, (bool, bool)> {
        self.inputs
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_str(), (self.std_call[i], self.nonstandard77[i])))
            .collect()
    }
}

fn fixture_path() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures",
        "wsjtx-callsign-oracle.json",
    ]
    .iter()
    .collect()
}

fn load() -> Oracle {
    let path = fixture_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let v: serde_json::Value = serde_json::from_str(&raw).expect("oracle fixture is valid JSON");

    let strings = |key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .unwrap_or_else(|| panic!("fixture has no {key} array"))
            .iter()
            .map(|s| s.as_str().expect("fixture entry is a string").to_string())
            .collect()
    };
    let inputs = strings("inputs");
    let verdicts: Vec<char> = v["verdicts"]
        .as_str()
        .expect("fixture has no verdicts string")
        .chars()
        .collect();
    assert_eq!(
        verdicts.len(),
        inputs.len() * 2,
        "verdict string and input list disagree — the fixture is torn"
    );

    Oracle {
        std_call: (0..inputs.len()).map(|i| verdicts[2 * i] == '1').collect(),
        nonstandard77: (0..inputs.len())
            .map(|i| verdicts[2 * i + 1] == '1')
            .collect(),
        inputs,
        base_inputs: strings("baseInputs"),
        base: strings("baseCallsign"),
        json: v,
    }
}

/// Exactly what both Rust predicates do to their input before testing it. Kept as one function
/// so the normalisation claim below is testing the real rule and not a paraphrase of it.
fn normalise(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// Render an input so a failure is debuggable: the divergences live in whitespace and Unicode,
/// and `"W9XYZ "` vs `"W9XYZ\u{a0}"` are indistinguishable printed raw.
fn show(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            _ => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
        }
    }
    out.push('"');
    out
}

fn declared(input: &str) -> Option<&'static (&'static str, bool, bool, &'static str)> {
    DECLARED_DIVERGENCES.iter().find(|d| d.0 == input)
}

/// The fixture is data, and data rots. Fail on a torn or truncated one rather than silently
/// gating on three inputs.
#[test]
fn the_oracle_fixture_is_intact_and_carries_its_provenance() {
    let o = load();
    let v = &o.json;

    assert!(
        o.inputs.len() >= 20_000,
        "the corpus shrank to {} inputs — regenerate, do not trim",
        o.inputs.len()
    );
    assert_eq!(
        o.base_inputs.len(),
        o.base.len(),
        "base_callsign columns disagree"
    );
    assert!(o.base_inputs.len() >= 90, "the base_callsign subset shrank");
    assert_eq!(
        v["provenance"]["revision"].as_str().unwrap_or("").len(),
        40,
        "provenance.revision is not a git sha"
    );
    for f in ["widgets/mainwindow.cpp", "Radio.cpp"] {
        assert_eq!(
            v["provenance"]["sha256"][f].as_str().unwrap_or("").len(),
            64,
            "no sha256 recorded for {f} — staleness would be invisible"
        );
    }

    // The two flags without which the measurement means something else entirely.
    let flags = v["patterns"]["standard_call_re"]["flags"]
        .as_str()
        .expect("stdCall flags recorded");
    assert!(flags.contains("CaseInsensitiveOption"), "flags: {flags}");
    assert!(
        flags.contains("ExtendedPatternSyntaxOption"),
        "flags: {flags}"
    );
    // The pattern TEXT is deliberately not in the fixture (GPL-3.0-only Qt source; the NOTICE
    // says none of it is included here, and that stays true). Its sha256 pins staleness, and
    // this boolean pins the one structural fact the predicate exists for.
    assert!(
        v["patterns"]["standard_call_re"]["has_suffix_alternation"]
            .as_bool()
            .unwrap_or(false),
        "the stdCall pattern lost its suffix alternation — /P and /R are the whole point"
    );
    for k in [
        "standard_call_re",
        "callsign_alphabet_re",
        "strict_standard_callsign_re",
    ] {
        assert_eq!(
            v["patterns"][k]["sha256"].as_str().map(str::len),
            Some(64),
            "{k}: no pattern fingerprint — upstream drift would be invisible"
        );
        assert!(
            v["patterns"][k]["pattern"].is_null(),
            "{k}: upstream pattern TEXT is back in the fixture — that is the licence leak"
        );
    }
    // Both Radio.cpp patterns are case-SENSITIVE and unflagged. That is not a detail: it is the
    // entire reason `is_77bit_nonstandard_call`'s `to_ascii_uppercase` is a divergence and not a
    // no-op, and the normalisation claim below is written around it.
    for k in ["callsign_alphabet_re", "strict_standard_callsign_re"] {
        assert!(
            v["patterns"][k]["flags"].is_null(),
            "{k} gained flags — re-derive the normalisation claim before trusting this fixture"
        );
    }
}

/// **Claim 1 — transcription.** On the normalised domain, `is_std_call` IS `MainWindow::stdCall`.
///
/// This is the predicate that chooses what goes on the air: standard ⇒ the plain Type 1/2 forms,
/// which carry a grid AND a numeric report; nonstandard ⇒ the hashed i3=4 forms, which carry
/// neither. Getting it wrong is not cosmetic, it is a QSO that cannot complete.
#[test]
fn is_std_call_transcribes_upstream_stdcall_on_the_normalised_domain() {
    let o = load();
    let mut unexpected = Vec::new();
    let mut checked = 0usize;

    for (i, input) in o.inputs.iter().enumerate() {
        if normalise(input) != *input {
            continue; // Claim 2's territory
        }
        checked += 1;
        let (ours, theirs) = (is_std_call(input), o.std_call[i]);
        if ours == theirs {
            continue;
        }
        match declared(input) {
            Some(d) => assert_eq!(
                d.1,
                theirs,
                "declared divergence {} records upstream={} but the oracle says {theirs} ({})",
                show(input),
                d.1,
                d.3
            ),
            None => unexpected.push(format!(
                "  {:<28} upstream stdCall={theirs}  nexus is_std_call={ours}",
                show(input)
            )),
        }
    }

    assert!(checked > 5_000, "only {checked} normalised inputs swept");
    assert!(
        unexpected.is_empty(),
        "is_std_call diverged from upstream stdCall on {} of {checked} normalised inputs:\n{}\n\
         Either the transcription drifted, or this is a deliberate divergence — in which case add \
         it to DECLARED_DIVERGENCES with the reason, do not widen the predicate to match.",
        unexpected.len(),
        unexpected.join("\n")
    );
}

/// **Claim 1 — transcription**, for `is_77bit_nonstandard_call` vs
/// `Radio::is_77bit_nonstandard_callsign`.
///
/// Deliberately COARSER than `stdCall` — it has no `/R`//`P` exemption, so `F4CYH/P` is
/// nonstandard here while standard there. Upstream keeps the two apart and gives them different
/// jobs (this one feeds only the Tx1-elision guard); conflating them is what the 2026-08-06 fix
/// undid, and this gate keeps them apart measurably rather than by comment.
#[test]
fn is_77bit_nonstandard_call_transcribes_upstream_on_the_normalised_domain() {
    let o = load();
    let mut unexpected = Vec::new();
    let mut checked = 0usize;

    for (i, input) in o.inputs.iter().enumerate() {
        if normalise(input) != *input {
            continue;
        }
        checked += 1;
        let (ours, theirs) = (is_77bit_nonstandard_call(input), o.nonstandard77[i]);
        if ours == theirs {
            continue;
        }
        match declared(input) {
            Some(d) => assert_eq!(
                d.2,
                theirs,
                "declared divergence {} records upstream={} but the oracle says {theirs} ({})",
                show(input),
                d.2,
                d.3
            ),
            None => unexpected.push(format!(
                "  {:<28} upstream nonstandard77={theirs}  nexus={ours}",
                show(input)
            )),
        }
    }

    assert!(checked > 5_000, "only {checked} normalised inputs swept");
    assert!(
        unexpected.is_empty(),
        "is_77bit_nonstandard_call diverged from upstream on {} of {checked} normalised inputs:\n{}",
        unexpected.len(),
        unexpected.join("\n")
    );
}

/// **Claim 2 — normalisation.** Off the normalised domain, both predicates answer for
/// `normalise(x)`, exactly.
///
/// Upstream has no such step: `stdCall`'s pattern tolerates leading/trailing ASCII whitespace
/// and folds case itself, while the two Radio.cpp regexes tolerate neither. Nexus's `trim()` is
/// Unicode White_Space (wider than Qt's non-UCP `\s`) and its uppercase is ASCII-only (narrower
/// than PCRE2's UTF-mode fold). All three differences are real and all three are pinned here:
/// drop the `trim()`, swap it for `trim_ascii()`, or start normalising something else, and this
/// goes red with the input that proves it.
#[test]
fn off_the_normalised_domain_both_predicates_answer_for_the_normalised_input() {
    let o = load();
    let table = o.by_input();
    let mut unexpected = Vec::new();
    let (mut checked, mut unresolvable) = (0usize, 0usize);

    for input in &o.inputs {
        let n = normalise(input);
        if n == *input {
            continue;
        }
        // Only checkable when the corpus also measured the normalised form. It nearly always
        // did (the corpus adds the bare form of every padded/lowercased probe), and the count
        // is reported so a shrinking overlap cannot hide.
        let Some(&(up_std, up_nonstd)) = table.get(n.as_str()) else {
            unresolvable += 1;
            continue;
        };
        checked += 1;
        if let Some(d) = declared(&n) {
            let _ = d; // the fold cases are declared against their normalised form
            continue;
        }
        if is_std_call(input) != up_std {
            unexpected.push(format!(
                "  {:<28} → {:<12} stdCall: nexus={} upstream(normalised)={up_std}",
                show(input),
                show(&n),
                is_std_call(input)
            ));
        }
        if is_77bit_nonstandard_call(input) != up_nonstd {
            unexpected.push(format!(
                "  {:<28} → {:<12} nonstandard77: nexus={} upstream(normalised)={up_nonstd}",
                show(input),
                show(&n),
                is_77bit_nonstandard_call(input)
            ));
        }
    }

    assert!(
        checked > 5_000,
        "only {checked} un-normalised inputs were resolvable ({unresolvable} unresolvable)"
    );
    assert!(
        unexpected.is_empty(),
        "{} inputs where Nexus's answer is NOT upstream's answer for the normalised form \
         ({checked} checked, {unresolvable} unresolvable):\n{}\n\
         The normalisation rule changed, or a predicate stopped applying it.",
        unexpected.len(),
        unexpected.join("\n")
    );
}

/// A declared divergence that stopped diverging is a record that has started lying, and a record
/// nobody can trust is worse than none. Every row must still be in the corpus, must still carry
/// upstream's real answer, and must still disagree with ours on at least one column.
#[test]
fn every_declared_divergence_still_diverges() {
    let o = load();
    let table = o.by_input();
    for &(input, up_std, up_nonstd, why) in DECLARED_DIVERGENCES {
        let Some(&(std, nonstd)) = table.get(input) else {
            panic!(
                "declared divergence {} is not in the corpus ({why})",
                show(input)
            );
        };
        assert_eq!(std, up_std, "upstream stdCall for {} ({why})", show(input));
        assert_eq!(
            nonstd,
            up_nonstd,
            "upstream nonstandard77 for {} ({why})",
            show(input)
        );
        assert!(
            is_std_call(input) != std || is_77bit_nonstandard_call(input) != nonstd,
            "{} is declared a divergence but Nexus now agrees with upstream on both columns — \
             delete its row ({why})",
            show(input)
        );
    }
}

/// The two predicates must stay DIFFERENT, and the corpus proves the difference is the one
/// upstream draws. A drive-by "simplification" that made one call the other would sail through
/// the claims above only if upstream agreed — it does not, and the overlap is exactly where the
/// 2026-08-06 bug lived.
#[test]
fn the_two_predicates_are_not_the_same_predicate() {
    let o = load();
    let overlap: Vec<&String> = o
        .inputs
        .iter()
        .enumerate()
        .filter(|(i, s)| o.std_call[*i] && o.nonstandard77[*i] && normalise(s) == ***s)
        .map(|(_, s)| s)
        .collect();
    assert!(
        overlap.len() >= 50,
        "upstream calls only {} normalised inputs both standard AND 77-bit-nonstandard; that \
         overlap is where /P and /R live and it should not be small",
        overlap.len()
    );
    assert!(
        overlap.iter().any(|s| s.as_str() == "F4CYH/P"),
        "F4CYH/P is the canonical member of the overlap and the call that reported the bug"
    );
    for c in ["F4CYH/P", "KD9TAW/R", "W9XYZ/P"] {
        assert!(is_std_call(c), "{c} must be standard — it packs Type 1/2");
        assert!(
            is_77bit_nonstandard_call(c),
            "{c} must be 77-bit-nonstandard — it is the coarser predicate, no suffix exemption"
        );
    }
}

/// `base_call` vs `Radio::base_callsign`.
///
/// **This one is not a transcription, and its doc comment said it was until this gate measured
/// it.** Upstream splits on the FIRST '/' and keeps the LONGER side; Nexus keeps the last
/// callsign-shaped segment. Nexus's rule is the better one on air, but "better" and "the same"
/// are different claims. Every divergence over the curated real-world/synthetic subset is
/// declared, and the gate fails both when a new one appears and when a declared one quietly
/// goes away.
#[test]
fn base_call_diverges_from_upstream_exactly_where_declared() {
    let o = load();
    let mut unexpected = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (i, input) in o.base_inputs.iter().enumerate() {
        let ours = base_call(input);
        let theirs = &o.base[i];
        if &ours == theirs {
            assert!(
                !DECLARED_BASE_DIVERGENCES.iter().any(|d| d.0 == input),
                "{} is declared a base_call divergence but now agrees with upstream — delete \
                 its row",
                show(input)
            );
            continue;
        }
        match DECLARED_BASE_DIVERGENCES.iter().find(|d| d.0 == input) {
            Some(d) => {
                assert_eq!(d.1, theirs.as_str(), "upstream base for {}", show(input));
                assert_eq!(d.2, ours.as_str(), "nexus base for {}", show(input));
                seen.insert(input.as_str());
            }
            None => unexpected.push(format!(
                "    ({:<26} {:<14} {}),",
                format!("{},", show(input)),
                format!("{},", show(theirs)),
                show(&ours)
            )),
        }
    }

    assert!(
        unexpected.is_empty(),
        "base_call diverged from upstream Radio::base_callsign on {} undeclared inputs:\n{}",
        unexpected.len(),
        unexpected.join("\n")
    );
    assert_eq!(
        seen.len(),
        DECLARED_BASE_DIVERGENCES.len(),
        "declared base_call divergences that did not occur — the record is stale"
    );
}
