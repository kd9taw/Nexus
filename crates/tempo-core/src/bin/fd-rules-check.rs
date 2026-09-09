//! The Field Day rules publish gate: the REAL loader, run over a candidate file.
//!
//! `.github/workflows/fd-rules.yml` runs this before pushing
//! `crates/tempo-core/src/fd_rules.seed.json` to the rolling `fd-rules` Release
//! that every Nexus install downloads — a seed the app would refuse must never
//! ship.
//!
//! It is deliberately a thin shell around [`tempo_core::fd_rules::validate`],
//! which is [`parse_spec`] — the very function the shipped app enforces. What it
//! replaces was a SECOND validator written in JavaScript, kept in step by a
//! parity corpus. Four rounds of parity fixes each closed some divergence
//! classes and revealed others, and the reason is structural: agreeing with the
//! loader byte-for-byte means reimplementing serde's derive behaviour AND
//! serde_json's lexer — presence rules, integer widths, surrogate pairing,
//! negative zero, float formatting, the recursion limit, `IgnoredAny` scanning
//! for unknown fields, and the exact error text. That is a port of two
//! libraries, and ports diverge forever; the last round reddened the gate on the
//! seed's own `_provenance` prose, a file every shipped app accepts. There is
//! now ONE implementation, so parity is true by construction.
//!
//! ```text
//! cargo run -p tempo-core --bin fd-rules-check -- <path>
//! ```
//!
//! Exit 0 = the app would load this file. Exit 1 = it would refuse it, and the
//! reason printed is the loader's OWN message — the same text
//! `RulesInitError::Invalid` carries and the corpus `.expect` files pin
//! (`crates/tempo-core/tests/fixtures/fd-rules-corpus`).
//!
//! ⚠️ The seed the §8d "may ADD a contest, never REMOVE one" check reads is the
//! one compiled INTO this binary, exactly as in the app. Build it from the same
//! checkout as the file under test — which is what the workflow does.

use std::process::ExitCode;

/// The in-repo seed, the file this gate exists to guard.
const DEFAULT_PATH: &str = "crates/tempo-core/src/fd_rules.seed.json";

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_PATH.to_string());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        // Not a verdict on the file's contents, so it does not wear the
        // INVALID prefix — but it is still a failed gate, never a pass.
        Err(e) => {
            eprintln!("fd-rules gate: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match tempo_core::fd_rules::validate(&text) {
        Ok(stats) => {
            eprintln!(
                "ok: {path} · generated {} · newest rules_year {} · {} sections",
                stats.generated, stats.rules_year, stats.sections
            );
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("fd-rules INVALID: {why}");
            ExitCode::FAILURE
        }
    }
}
