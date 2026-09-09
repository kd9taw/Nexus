//! The publish gate's evidence corpus (§2.5, §8d).
//!
//! `crates/tempo-core/src/bin/fd-rules-check.rs` is the gate
//! `.github/workflows/fd-rules.yml` runs before the rolling `fd-rules` Release
//! that every install downloads. It is a thin shell around
//! `fd_rules::validate` — i.e. `parse_spec`, the very function the shipped app
//! enforces — so a seed the app would refuse cannot be published.
//!
//! **The corpus is older than the gate that reads it.** It was built to keep a
//! SECOND validator, `scripts/check-fd-rules.mjs`, in step with `parse_spec`:
//! every fixture had to get the same verdict and the same reason from both
//! halves. Four rounds of parity fixes each closed some divergence classes and
//! revealed others, because agreeing with the loader byte-for-byte means
//! reimplementing serde's derive behaviour and serde_json's lexer — presence
//! rules, integer widths, surrogate pairing, the recursion limit, `IgnoredAny`
//! scanning for unknown fields, and the exact error text. The node half is gone;
//! there is one implementation, and parity is now true by construction. The
//! fixtures stayed, because what each one PINS is real: the refusal that a
//! specific mutation must produce.
//!
//! The walk drives the GATE BINARY rather than calling `validate` directly. The
//! binary is what the workflow invokes, so its exit code and the reason it
//! prints are part of what has to hold — a gate that finds the fault and exits 0
//! is not a gate.
//!
//! **The `.expect` is the WHOLE message**, not a keyword from it. A bare `dupe`
//! passed while the loader said serde's "missing field `dupe`" — a
//! keyword-sized expectation cannot tell two different refusals apart. The only
//! text a `.expect` may drop is the ` at line N column N` serde appends.
//!
//! Adding a validation rule means adding a fixture here.
//!
//! ⚠️ **A fixture may be one mutation off `accept/seed.json` and no more.** serde
//! reports the FIRST fault it meets, so a fixture carrying two defects pins
//! whichever one is met first — not the one it was written for.
//!
//! ⚠️ **There is a SECOND accept base, and it exists because "one mutation" could not be
//! honoured otherwise.** `accept/relation-priced.json` is `accept/seed.json` with one
//! ruleset converted to CQ WW's point model — an empty `points_by_mode_class` and a
//! populated `relation_points` — which is a coordinated pair of edits by construction: a
//! contest has ONE point table, and the loader refuses a file with both populated (that
//! refusal is its own fixture, one mutation off `seed.json`). The two relation-table
//! fixtures — a missing arm, an unknown relation name — are each one mutation off
//! `relation-priced.json`, which is itself an accept fixture and therefore proved clean by
//! the walk above before either refusal is read.
//!
//! ⚠️ **Some of these mutations are invisible to `JSON.parse`** — `2.0` where `2`
//! belongs, a repeated key, an integer past `u64`, an unpaired `\u` surrogate. A
//! tool that rewrites a fixture through `JSON.parse` + `stringify` would
//! silently erase what it pins.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ROOT: &str = "tests/fixtures/fd-rules-corpus";

/// The gate binary cargo just built for this package — the same one
/// `.github/workflows/fd-rules.yml` runs.
const GATE: &str = env!("CARGO_BIN_EXE_fd-rules-check");

/// Run the gate on one fixture, exactly as the publish workflow runs it.
fn gate(path: &Path) -> Output {
    Command::new(GATE)
        .arg(path)
        .output()
        .unwrap_or_else(|e| panic!("running the gate on {}: {e}", path.display()))
}

/// Every `.json` fixture in one arm of the corpus.
fn corpus(kind: &str) -> Vec<PathBuf> {
    let dir = Path::new(ROOT).join(kind);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("corpus dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
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
fn every_accept_fixture_passes_the_gate() {
    for p in corpus("accept") {
        let out = gate(&p);
        assert!(
            out.status.success(),
            "{} must pass the gate: {}",
            p.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
}

#[test]
fn every_refuse_fixture_is_refused_with_the_expected_reason() {
    for p in corpus("refuse") {
        let want_path = p.with_extension("expect");
        let want = std::fs::read_to_string(&want_path).unwrap_or_else(|e| {
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
        let out = gate(&p);
        // Exit 1, not merely non-zero: that is the contract the workflow step
        // and every shell caller reads.
        assert_eq!(
            out.status.code(),
            Some(1),
            "{} must be refused, and the gate exited {:?}",
            p.display(),
            out.status.code()
        );
        let got = String::from_utf8_lossy(&out.stderr);
        assert!(
            got.contains(want),
            "{}: message {got:?} does not carry {want:?}",
            p.display()
        );
    }
}
