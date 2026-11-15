//! ⭐ **The custom-contest definition corpus** — every refusal an operator's own contest file can
//! meet, pinned by the WHOLE message ([`custom::parse`]), in the pattern of the rules-file corpus
//! (`fd_rules_corpus.rs`).
//!
//! Every refuse fixture is ONE mutation off `accept/wae-shaped.json`: a definition carrying two
//! defects pins whichever is met first, not the one it was written for.
//!
//! ⚠️ **Nothing is installed in this binary.** A definition's token is checked against the
//! installed custom contests too, so an install here could turn an accept fixture into a refusal.
use std::path::{Path, PathBuf};
use tempo_core::fd_rules::custom;

const ROOT: &str = "tests/fixtures/custom-contest-corpus";
/// 2026-11-14T00:00Z — the clock a one-off date is bounded by.
const NOW: u64 = 1_794_614_400;

fn corpus(kind: &str) -> Vec<PathBuf> {
    let dir = Path::new(ROOT).join(kind);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("corpus dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "corpus/{kind} is empty — the walk proves nothing"
    );
    out
}

#[test]
fn every_accept_fixture_parses_and_compiles() {
    for p in corpus("accept") {
        let text = std::fs::read_to_string(&p).expect("fixture");
        let def = custom::parse(&text, NOW)
            .unwrap_or_else(|e| panic!("{} must be accepted: {e}", p.display()));
        let rs = custom::compile(&def, NOW)
            .unwrap_or_else(|e| panic!("{} must compile: {e}", p.display()));
        assert!(rs.custom, "{}", p.display());
        assert_eq!(rs.event, def.id);
    }
}

#[test]
fn every_refuse_fixture_is_refused_with_the_expected_reason() {
    for p in corpus("refuse") {
        let want = std::fs::read_to_string(p.with_extension("expect")).unwrap_or_else(|e| {
            panic!(
                "every refuse fixture needs a .expect ({}): {e}",
                p.display()
            )
        });
        let want = want.trim();
        assert!(
            !want.is_empty(),
            "{}: empty .expect matches anything",
            p.display()
        );
        let text = std::fs::read_to_string(&p).expect("fixture");
        let got = custom::parse(&text, NOW).expect_err(&format!("{} must be refused", p.display()));
        assert!(
            got.contains(want),
            "{}: message {got:?} does not carry {want:?}",
            p.display()
        );
    }
}
