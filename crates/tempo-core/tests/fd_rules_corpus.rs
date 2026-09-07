//! The drift gate (§2.5, §8d). `tempo_core::fd_rules::parse_spec` and
//! `scripts/check-fd-rules.mjs` validate the same rules file, and the node one
//! is the publish gate `.github/workflows/fd-rules.yml` runs before the rolling
//! `fd-rules` Release. Both files' headers have said "keep the two in step"
//! since they were written and nothing enforced it — so a rule could land in
//! one and not the other, and the first symptom would be a seed the app refuses
//! shipping green, or a publish blocked for a reason the app does not actually
//! hold.
//!
//! This corpus is the enforcement: every fixture gets the same verdict from
//! both, and a refusal carries the same reason in both. The node half walks the
//! SAME directory in `scripts/check-fd-rules.test.mjs`.
//!
//! **The `.expect` is the WHOLE message**, not a keyword from it. A bare `dupe`
//! passed while Rust said serde's "missing field `dupe`" and node said a
//! hand-written "missing the `dupe` block" — same verdict, different reason,
//! and the fixture could not see it. The only text a `.expect` may drop is the
//! ` at line N column N` serde appends, which node has no way to know.
//!
//! Adding a validation rule means adding a fixture here. A rule with no fixture
//! is a rule only one validator has. VALUE rules are two independent
//! implementations; the SHAPE rules serde enforces before any of them (which
//! fields are required, which integer fits) are derived from these very
//! `Deserialize` types by `scripts/rust-serde-schema.mjs`, because restating
//! them by hand is what let four mutations through the publish gate.
//!
//! ⚠️ **A fixture may be one mutation off `accept/seed.json` and no more.** serde
//! reports the FIRST fault it meets, and the two halves can meet a different one
//! first, so a fixture carrying two defects can pass while the halves disagree.
//!
//! ⚠️ **Some of these mutations are invisible to `JSON.parse`** — `2.0` where `2`
//! belongs, a repeated key, an integer past `u64`, an unpaired `\u` surrogate. The
//! node half reads the SOURCE TEXT for its shape pass for exactly that reason (see
//! `scripts/rust-serde-schema.mjs`'s header); a tool that rewrites a fixture through
//! `JSON.parse` + `stringify` would silently erase what it pins.
use std::path::Path;
use tempo_core::fd_rules::validate;

const ROOT: &str = "tests/fixtures/fd-rules-corpus";

/// Every `.json` fixture in one arm of the corpus, as `(file name, contents)`.
fn corpus(kind: &str) -> Vec<(String, String)> {
    let dir = Path::new(ROOT).join(kind);
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("corpus dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(e.path()).expect("fixture"),
            )
        })
        .collect();
    out.sort();
    // A corpus walk over an empty (or mistyped) directory passes vacuously. It
    // must not be able to: this is the whole gate's own positive control.
    assert!(
        !out.is_empty(),
        "corpus/{kind} is empty — the walk proves nothing"
    );
    out
}

#[test]
fn every_accept_fixture_validates() {
    for (name, text) in corpus("accept") {
        assert!(
            validate(&text).is_ok(),
            "{name} must validate: {:?}",
            validate(&text).err()
        );
    }
}

#[test]
fn every_refuse_fixture_is_refused_with_the_expected_reason() {
    for (name, text) in corpus("refuse") {
        let want_path = Path::new(ROOT)
            .join("refuse")
            .join(name.replace(".json", ".expect"));
        let want = std::fs::read_to_string(&want_path)
            .unwrap_or_else(|e| panic!("every refuse fixture needs a .expect ({name}): {e}"));
        let want = want.trim();
        assert!(!want.is_empty(), "{name}: empty .expect matches anything");
        let got = validate(&text)
            .expect_err(&format!("{name} must be refused, and it validated instead"));
        assert!(
            got.contains(want),
            "{name}: message {got:?} does not carry {want:?}"
        );
    }
}
