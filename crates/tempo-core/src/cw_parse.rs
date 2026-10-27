//! Parse a live CW decode transcript into operating context — the backend of the "CW
//! copilot" that lets an operator who can't read Morse still work CW. Pulls the other
//! station's callsign (candidates, ranked) and exchange (RST + name) out of the decoded
//! text. Whose turn it is, and the key that fits, are [`crate::cw_turn`]'s: it reads the copy
//! over by over, where a window of words cannot tell one contact from the next.
//!
//! Pure + deterministic. Callsign FORMAT validation uses [`crate::message::is_callsign`];
//! whether a prefix is a REAL DXCC prefix is supplied by the caller as a closure, so this
//! crate stays free of the DXCC tables (which live in the `propagation` crate). CW decode
//! is imperfect, so callers CONFIRM a candidate before it's used — nothing here transmits.

use crate::message::{base_call, is_callsign};

/// A candidate worked-station callsign pulled from the decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCandidate {
    /// Base call, uppercased (portable affixes stripped for display/matching).
    pub call: String,
    /// Confidence score (higher = more trustworthy); ranks the chips.
    pub score: u32,
    /// The top pick — pre-highlighted, but the operator still confirms.
    pub best: bool,
}

/// Best-effort exchange fields read from the decode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CwExchange {
    /// Signal report they sent us, e.g. "599" (cut numbers decoded: N→9, T→0).
    pub rst: Option<String>,
    /// Their name, e.g. "BOB" (after NAME/OP).
    pub name: Option<String>,
}

/// The full copilot read of the current decode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CwAssist {
    pub candidates: Vec<CallCandidate>,
    pub exchange: CwExchange,
}

/// How many trailing whitespace-tokens of the transcript to analyze — enough to cover the
/// current over/exchange without letting an earlier QSO in the scrollback dominate.
const TAIL_TOKENS: usize = 60;

/// Decode a CW "cut number" report token to plain digits: T→0, N→9 (the common ones), and
/// pass digits through. Returns None if it isn't a report-shaped token (3 report digits).
pub(crate) fn decut_rst(tok: &str) -> Option<String> {
    let digits: String = tok
        .chars()
        .map(|c| match c {
            'T' => '0',
            'N' => '9',
            d => d,
        })
        .collect();
    // A signal report is three digits (RST), e.g. 599 / 579. Accept exactly that.
    if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit()) {
        Some(digits)
    } else {
        None
    }
}

/// Analyze a CW decode `transcript` (persistent, from `CwStreamDecoder`). `mycall` filters our
/// own call out of the candidates. `is_real_prefix` returns true when a base call resolves to a
/// real DXCC prefix — pass a DXCC resolver from the caller (or `|_| true` in tests / when
/// unavailable).
pub fn analyze(transcript: &str, mycall: &str, is_real_prefix: impl Fn(&str) -> bool) -> CwAssist {
    let all: Vec<String> = transcript.split_whitespace().map(up).collect();
    let start = all.len().saturating_sub(TAIL_TOKENS);
    let toks = &all[start..];

    CwAssist {
        candidates: call_candidates(toks, mycall, &is_real_prefix),
        exchange: exchange(toks),
    }
}

fn up(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// Strict amateur-callsign SHAPE — tighter than [`crate::message::is_callsign`], which (by
/// design, for FT8 free-text) accepts any digit+letter mix and so lets CW cut-number noise
/// through: "599", "5NN", "5NN37", "TU", "73" all pass is_callsign, and "5NN" even RESOLVES
/// to a real prefix (5N = Nigeria), so the DXCC check can't catch it either. A real call is
/// a prefix (1–3 alnum containing a letter) + a call-area digit + a 1–4 letter suffix,
/// optionally /-compound. We require exactly that shape before a token can become a
/// candidate, so garbage is never auto-filled into the log.
pub(crate) fn is_strict_callsign(s: &str) -> bool {
    s.split('/').any(is_strict_base_call)
}

fn is_strict_base_call(base: &str) -> bool {
    if !(3..=8).contains(&base.len()) {
        return false;
    }
    if !base
        .bytes()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return false;
    }
    // Suffix = the trailing run of letters (1–4).
    let suffix = base
        .bytes()
        .rev()
        .take_while(|b| b.is_ascii_uppercase())
        .count();
    if !(1..=4).contains(&suffix) {
        return false;
    }
    // Head = prefix + the call-area digit; it must END in that digit.
    let head = &base[..base.len() - suffix];
    if !head.bytes().last().is_some_and(|b| b.is_ascii_digit()) {
        return false;
    }
    // Prefix = 1–3 chars before the call-area digit, containing at least one letter.
    let prefix = &head[..head.len() - 1];
    (1..=3).contains(&prefix.len()) && prefix.bytes().any(|b| b.is_ascii_uppercase())
}

/// Rank callsign candidates from the recent tokens. A token right after `DE` is the sender
/// (strongest cue); repetition + a real DXCC prefix raise confidence; our own call and
/// bare non-calls are dropped.
fn call_candidates(
    toks: &[String],
    mycall: &str,
    is_real_prefix: &impl Fn(&str) -> bool,
) -> Vec<CallCandidate> {
    use std::collections::HashMap;
    let mine = base_call(mycall);
    let mut scores: HashMap<String, u32> = HashMap::new();
    for (i, t) in toks.iter().enumerate() {
        // Space-split recovery: CW decode routinely drops a gap mid-call ("W1 ABC"), so the
        // clean call the operator SEES never lands as a single token and no chip appears.
        // Rejoin each token with the next and accept the result ONLY when the join is a strict
        // call that resolves to a real DXCC prefix — that prefix gate means two unrelated
        // tokens can never fabricate a bogus callsign into the chips (or the log).
        if i + 1 < toks.len() {
            let next = &toks[i + 1];
            // Only a genuine prefix|suffix split: `t` ends in the call-area digit and `next`
            // is the all-letter suffix ("W1"+"ABC"). This excludes joining a prosign/word to a
            // following call ("DE"+"W1ABC" → "DEW1ABC"), which is itself callsign-shaped.
            let split_shaped = t.bytes().last().is_some_and(|b| b.is_ascii_digit())
                && !next.is_empty()
                && next.bytes().all(|b| b.is_ascii_uppercase());
            if split_shaped {
                let joined = format!("{t}{next}");
                if joined.len() <= 8 && is_strict_callsign(&joined) {
                    let jb = base_call(&joined);
                    if jb != mine && !jb.is_empty() && is_real_prefix(&jb) {
                        let mut s = 3; // recovered split call, real prefix
                        if i > 0 && toks[i - 1] == "DE" {
                            s += 2;
                        }
                        *scores.entry(jb).or_insert(0) += s;
                    }
                }
            }
        }
        if !is_strict_callsign(t) {
            continue;
        }
        let b = base_call(t);
        if b == mine || b.is_empty() {
            continue; // never our own call
        }
        let mut s = 1; // well-formed call
        if is_real_prefix(&b) {
            s += 4; // resolves to a real DXCC prefix → strongest misdecode filter
        }
        if i > 0 && toks[i - 1] == "DE" {
            s += 2; // the sender, by the "DE <call>" convention
        }
        *scores.entry(b).or_insert(0) += s;
    }
    let mut cands: Vec<CallCandidate> = scores
        .into_iter()
        .map(|(call, score)| CallCandidate {
            call,
            score,
            best: false,
        })
        .collect();
    // Highest score first; ties broken by call for determinism.
    cands.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.call.cmp(&b.call)));
    cands.truncate(5);
    if let Some(first) = cands.first_mut() {
        first.best = true;
    }
    cands
}

/// Pull the RST they sent (after UR/RST, cut-number aware) and their name (after NAME/OP).
fn exchange(toks: &[String]) -> CwExchange {
    let mut ex = CwExchange::default();
    for (i, t) in toks.iter().enumerate() {
        match t.as_str() {
            "UR" | "RST" if ex.rst.is_none() => {
                // The report is the next report-shaped token (599, 5NN, …).
                if let Some(r) = toks.get(i + 1).and_then(|n| decut_rst(n)) {
                    ex.rst = Some(r);
                }
            }
            "NAME" | "OP" if ex.name.is_none() => {
                // A name is the next all-alphabetic token (not a callsign / not "IS").
                if let Some(n) = toks.get(i + 1) {
                    if n.len() >= 2
                        && n.chars().all(|c| c.is_ascii_alphabetic())
                        && !is_callsign(n)
                        && n != "IS"
                    {
                        ex.name = Some(n.clone());
                    }
                }
            }
            _ => {}
        }
    }
    ex
}

#[cfg(test)]
mod tests {
    use super::*;

    // A permissive DXCC stub for tests: real US/common prefixes resolve, gibberish doesn't.
    fn real(base: &str) -> bool {
        matches!(
            base.chars().next(),
            Some('W' | 'K' | 'N' | 'A' | 'V' | 'G' | 'D' | 'J' | 'F')
        )
    }

    #[test]
    fn extracts_the_cq_caller() {
        let a = analyze("CQ CQ DE W1ABC W1ABC K", "KD9TAW", real);
        assert!(a.candidates.iter().any(|c| c.call == "W1ABC" && c.best));
    }

    #[test]
    fn recovers_a_space_split_call_when_the_prefix_is_real() {
        // CW copy dropped a gap mid-call ("W1 ABC") — the join is a strict call with a real
        // prefix, so it's recovered as a clickable candidate instead of vanishing.
        let a = analyze("CQ DE W1 ABC K", "KD9TAW", real);
        assert!(a.candidates.iter().any(|c| c.call == "W1ABC"));
    }

    #[test]
    fn join_recovery_never_fabricates_an_unresolvable_call() {
        // "Q1"+"ABC" is callsign-SHAPED but Q1 isn't a real prefix → must NOT be recovered,
        // so two random adjacent tokens can never invent a bogus call into the chips/log.
        let a = analyze("DE Q1 ABC", "KD9TAW", real);
        assert!(a.candidates.iter().all(|c| c.call != "Q1ABC"));
    }

    #[test]
    fn de_call_outranks_a_bare_call() {
        // Both are valid calls, but W1ABC follows DE (the sender) → it wins.
        let a = analyze("K5XYZ DE W1ABC", "KD9TAW", real);
        assert_eq!(a.candidates.first().unwrap().call, "W1ABC");
    }

    #[test]
    fn drops_my_own_call_from_candidates() {
        let a = analyze("KD9TAW DE W1ABC UR 599", "KD9TAW", real);
        assert!(a.candidates.iter().all(|c| c.call != "KD9TAW"));
        assert!(a.candidates.iter().any(|c| c.call == "W1ABC"));
    }

    #[test]
    fn reads_report_and_name_with_cut_numbers() {
        let ex = analyze("W1ABC DE K2DEF UR 5NN 5NN NAME BOB BOB", "W1ABC", real).exchange;
        assert_eq!(ex.rst.as_deref(), Some("599")); // 5NN → 599
        assert_eq!(ex.name.as_deref(), Some("BOB"));
    }

    #[test]
    fn empty_decode_has_no_candidates() {
        let a = analyze("", "KD9TAW", real);
        assert!(a.candidates.is_empty());
    }

    #[test]
    fn gibberish_prefix_scores_below_a_real_one() {
        // "Q1ABC" is well-formed but not a real prefix; "W1ABC" resolves → ranks higher.
        let a = analyze("DE Q1ABC W1ABC", "KD9TAW", real);
        assert_eq!(a.candidates.first().unwrap().call, "W1ABC");
    }

    #[test]
    fn rst_and_cut_number_noise_never_becomes_a_candidate() {
        // RST / prosign noise ("599", "5NN", "5NN37", "TU", "73") must never be a callsign,
        // even though is_callsign passes some and "5N" resolves to Nigeria — only W1ABC wins.
        let a = analyze("CQ CQ DE W1ABC 599 5NN 5NN37 TU 73 K", "KD9TAW", real);
        let calls: Vec<&str> = a.candidates.iter().map(|c| c.call.as_str()).collect();
        assert_eq!(calls, vec!["W1ABC"]);
    }

    #[test]
    fn strict_shape_accepts_real_calls_rejects_noise() {
        for good in ["W1AW", "KD9TAW", "2E0ABC", "5N1AB", "VE3AQ"] {
            assert!(
                is_strict_base_call(good),
                "{good} should be a valid callsign"
            );
        }
        assert!(is_strict_callsign("KH6/KD9TAW")); // compound: the call segment wins
        for bad in ["5NN", "599", "5NN37", "TU", "73", "OP", "SETH1"] {
            assert!(!is_strict_base_call(bad), "{bad} should NOT be a callsign");
        }
    }
}
