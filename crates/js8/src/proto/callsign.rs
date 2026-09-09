//! Callsign packing: the 28-bit base-call field, its special values, and the 50-bit
//! alphanumeric field of compound / heartbeat frames.
//!
//! varicode.cpp read as spec. `packCallsign` (:945-1005): uppercase + trim; the specials
//! (`basecalls`, :214-281) win first; strip `/P` into the portable flag (a flag bit in the
//! Directed frame, which is WHY [`CallRef::Base`] carries NO `/P` — use [`split_portable`]);
//! `3DA0…` → `3D0…` (Swaziland) and `3X<letter>…` → `Q<letter>…` (Guinea) workarounds;
//! 2..=6 characters; the padding permutations in UPSTREAM ORDER matched against
//! `([0-9A-Z ])([0-9A-Z])([0-9])([A-Z ])([A-Z ])([A-Z ])` with the LAST matching permutation
//! winning; then the mixed radix 37·36·10·27·27·27 (`idx − 10` for the three letter slots,
//! space = 26). `unpackCallsign` (:1007-1046) inverts it, restores the workarounds and trims.
//! `packAlphaNumeric50` (:857-943): keep `[A-Z0-9 /@]`, insert a space at slots 3 and 7 unless
//! that character is `/`, pad to 11, radix 39 at slot 0 and 38 elsewhere, `/`-flag bits at
//! 3 and 7 — a 10-character compound call in 50 bits.
//!
//! The validators (`is_base_call`, `is_compound_call`, both below) are transcribed from
//! JS8Call's `isValidCallsign`/`isCompoundCallsign`/`isValidCompoundCallsign`
//! (varicode.cpp:1232-1320), not project-derived approximations — an earlier draft of this
//! file invented an untranscribed ">1 slash" rule that rejected real JS8Call-valid compound
//! calls (multiple slashes are explicitly permitted upstream), caught and reverted after
//! fetching and reading the actual functions. They are still not WSJT-X's rule:
//! `tempo_core::message::is_callsign` is the 77-bit rule and differs (and tempo-core cannot be
//! imported here anyway).

use crate::proto::alphabet::{ALNUM39, GROUPS};

/// `37 * 36 * 10 * 27 * 27 * 27` — one past the largest packed base call (varicode.cpp:209).
pub const NBASECALL: u32 = 37 * 36 * 10 * 27 * 27 * 27;

/// A destination or source as it appears in a Directed frame's 28-bit fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallRef {
    /// A bare base call, uppercase, NO `/P` (portable rides the frame flags).
    Base(String),
    /// `<....>` — the compound placeholder, `NBASECALL + 1`.
    Placeholder,
    /// `@ALLCALL`, `NBASECALL + 2`.
    AllCall,
    /// `@JS8NET`, `NBASECALL + 3`.
    Js8Net,
    /// Index into [`GROUPS`]: `NBASECALL + 4 + idx`.
    Group(u8),
}

impl CallRef {
    /// Accepts `<....>`, `@ALLCALL`, `@JS8NET`, every [`GROUPS`] name, and base calls (which are
    /// uppercased). A call with `/` — `/P` included — is NOT a `CallRef`: split it first.
    pub fn parse(s: &str) -> Option<CallRef> {
        let u = s.trim().to_ascii_uppercase();
        match u.as_str() {
            "<....>" => return Some(CallRef::Placeholder),
            "@ALLCALL" => return Some(CallRef::AllCall),
            "@JS8NET" => return Some(CallRef::Js8Net),
            _ => {}
        }
        if let Some(i) = GROUPS.iter().position(|&g| g == u) {
            return Some(CallRef::Group(i as u8));
        }
        if !u.contains('/') && is_base_call(&u) {
            return Some(CallRef::Base(u));
        }
        None
    }

    /// The exact upstream text.
    pub fn render(&self) -> String {
        match self {
            CallRef::Base(s) => s.clone(),
            CallRef::Placeholder => "<....>".to_string(),
            CallRef::AllCall => "@ALLCALL".to_string(),
            CallRef::Js8Net => "@JS8NET".to_string(),
            CallRef::Group(i) => GROUPS.get(*i as usize).copied().unwrap_or("@?").to_string(),
        }
    }

    /// `@`-destinations: ALLCALL, JS8NET and the group table.
    pub fn is_group(&self) -> bool {
        matches!(self, CallRef::AllCall | CallRef::Js8Net | CallRef::Group(_))
    }
}

fn idx(c: u8) -> Option<u32> {
    ALNUM39.iter().position(|&a| a == c).map(|p| p as u32)
}

/// varicode.cpp:987-993: `3DA0…` → `3D0…` (Swaziland) and `3X<letter>…` → `Q<letter>…`
/// (Guinea). Applied before both [`pack28`]'s permutation match and [`is_base_call`]'s shape
/// check — a call like "3DA0AB" only fits `pack_callsign_pattern`'s digit-in-position-3 shape
/// AFTER this substitution ("3D0AB"), so a validator that skips it rejects a real callsign.
fn apply_prefix_workarounds(call: &str) -> String {
    let mut call = call.to_string();
    if let Some(rest) = call.strip_prefix("3DA0") {
        call = format!("3D0{rest}");
    }
    if call.len() > 2 && call.starts_with("3X") && call.as_bytes()[2].is_ascii_uppercase() {
        call = format!("Q{}", &call[2..]);
    }
    call
}

/// varicode.cpp:39 `pack_callsign_pattern` over exactly six bytes.
fn matches_pack_pattern(s: &[u8]) -> bool {
    s.len() == 6
        && (s[0].is_ascii_digit() || s[0].is_ascii_uppercase() || s[0] == b' ')
        && (s[1].is_ascii_digit() || s[1].is_ascii_uppercase())
        && s[2].is_ascii_digit()
        && s[3..].iter().all(|&c| c.is_ascii_uppercase() || c == b' ')
}

/// Base-call packing 37·36·10·27·27·27 (+ 3DA0→3D0, 3Xa→Qa workarounds; specials above).
pub fn pack28(c: &CallRef) -> Option<u32> {
    let call = match c {
        CallRef::Placeholder => return Some(NBASECALL + 1),
        CallRef::AllCall => return Some(NBASECALL + 2),
        CallRef::Js8Net => return Some(NBASECALL + 3),
        CallRef::Group(i) => {
            return (usize::from(*i) < GROUPS.len()).then(|| NBASECALL + 4 + u32::from(*i))
        }
        CallRef::Base(s) => s.trim().to_ascii_uppercase(),
    };
    if call.contains('/') {
        return None; // `/P` is split by the caller; anything else is a compound call (pack50)
    }
    let call = apply_prefix_workarounds(&call);
    let n = call.len();
    if !(2..=6).contains(&n) {
        return None;
    }
    // Padding permutations, upstream order; the LAST match wins (varicode.cpp:975-996).
    let mut perms = vec![call.clone()];
    match n {
        2 => perms.push(format!(" {call}   ")),
        3 => {
            perms.push(format!(" {call}  "));
            perms.push(format!("{call}   "));
        }
        4 => {
            perms.push(format!(" {call} "));
            perms.push(format!("{call}  "));
        }
        5 => {
            perms.push(format!(" {call}"));
            perms.push(format!("{call} "));
        }
        _ => {}
    }
    let matched = perms.iter().rfind(|p| matches_pack_pattern(p.as_bytes()))?;
    let m = matched.as_bytes();
    let mut v = idx(m[0])?;
    v = 36 * v + idx(m[1])?;
    v = 10 * v + idx(m[2])?;
    v = 27 * v + idx(m[3])? - 10;
    v = 27 * v + idx(m[4])? - 10;
    v = 27 * v + idx(m[5])? - 10;
    Some(v)
}

/// Inverse of [`pack28`]; `None` above the last group value.
pub fn unpack28(v: u32) -> Option<CallRef> {
    if v >= NBASECALL {
        return match v - NBASECALL {
            1 => Some(CallRef::Placeholder),
            2 => Some(CallRef::AllCall),
            3 => Some(CallRef::Js8Net),
            g @ 4..=54 => Some(CallRef::Group((g - 4) as u8)),
            _ => None,
        };
    }
    let mut v = v;
    let mut w = [b' '; 6];
    for slot in [5, 4, 3] {
        w[slot] = ALNUM39[(v % 27 + 10) as usize];
        v /= 27;
    }
    w[2] = ALNUM39[(v % 10) as usize];
    v /= 10;
    w[1] = ALNUM39[(v % 36) as usize];
    v /= 36;
    w[0] = ALNUM39[(v as usize).min(ALNUM39.len() - 1)];
    let mut call = String::from_utf8_lossy(&w).into_owned();
    if let Some(rest) = call.strip_prefix("3D0") {
        call = format!("3DA0{rest}");
    }
    if call.len() > 1 && call.starts_with('Q') && call.as_bytes()[1].is_ascii_uppercase() {
        call = format!("3X{}", &call[1..]);
    }
    Some(CallRef::Base(call.trim().to_string()))
}

/// `packAlphaNumeric50` (compound / heartbeat 50-bit field). `None` when the cleaned text does
/// not fit the 11 slots.
pub fn pack50(call: &str) -> Option<u64> {
    let mut word: Vec<u8> = call
        .to_ascii_uppercase()
        .bytes()
        .filter(|&b| {
            b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ' || b == b'/' || b == b'@'
        })
        .collect();
    if word.len() > 3 && word[3] != b'/' {
        word.insert(3, b' ');
    }
    if word.len() > 7 && word[7] != b'/' {
        word.insert(7, b' ');
    }
    if word.len() > 11 {
        return None;
    }
    word.resize(11, b' ');
    let ix = |b: u8| idx(b).map(u64::from);
    let mut v = ix(word[0])?;
    v = v * 38 + ix(word[1])?;
    v = v * 38 + ix(word[2])?;
    v = v * 2 + u64::from(word[3] == b'/');
    v = v * 38 + ix(word[4])?;
    v = v * 38 + ix(word[5])?;
    v = v * 38 + ix(word[6])?;
    v = v * 2 + u64::from(word[7] == b'/');
    v = v * 38 + ix(word[8])?;
    v = v * 38 + ix(word[9])?;
    v = v * 38 + ix(word[10])?;
    Some(v)
}

/// Inverse of [`pack50`]; spaces removed as upstream does (varicode.cpp:941).
pub fn unpack50(v: u64) -> String {
    let mut v = v;
    let mut w = [b' '; 11];
    for slot in [10, 9, 8] {
        w[slot] = ALNUM39[(v % 38) as usize];
        v /= 38;
    }
    w[7] = if v % 2 == 1 { b'/' } else { b' ' };
    v /= 2;
    for slot in [6, 5, 4] {
        w[slot] = ALNUM39[(v % 38) as usize];
        v /= 38;
    }
    w[3] = if v % 2 == 1 { b'/' } else { b' ' };
    v /= 2;
    for slot in [2, 1] {
        w[slot] = ALNUM39[(v % 38) as usize];
        v /= 38;
    }
    w[0] = ALNUM39[(v % 39) as usize];
    w.iter()
        .filter(|&&b| b != b' ')
        .map(|&b| b as char)
        .collect()
}

/// `"W1AW/P"` → `("W1AW", true)`; anything else unchanged with `false`.
pub fn split_portable(call: &str) -> (&str, bool) {
    match call.strip_suffix("/P") {
        Some(base) => (base, true),
        None => (call, false),
    }
}

/// varicode.cpp:40 `base_callsign_pattern`'s `base` group alone
/// (`([0-9A-Z])?([0-9A-Z])([0-9])([A-Z])?([A-Z])?([A-Z])?`) plus `isValidCallsign`'s rule
/// (:1275-1279): longer than two characters and containing a digit–letter or letter–digit
/// pair. Deliberately NOT the regex's own optional `(?<portable>[/][P])?` group — that lives
/// in [`is_compound_call`], which checks it directly against the RAW string (no workaround),
/// because the real regex never sees the 3DA0/3X substitution either. Callers wanting "is this
/// shape, ignoring the workaround" (i.e. matching the literal upstream regex) use this
/// function directly on their own core string; [`is_base_call`] is "is this shape, WITH the
/// workaround applied" — the two answer different questions on purpose.
fn raw_shape_and_pair_ok(core: &str) -> bool {
    let b = core.as_bytes();
    if b.len() < 3 || b.len() > 6 {
        return false;
    }
    let alnum = |c: u8| c.is_ascii_digit() || c.is_ascii_uppercase();
    let shape_ok = |prefix: usize| {
        b.len() > prefix
            && b[..prefix].iter().all(|&c| alnum(c))
            && b[prefix].is_ascii_digit()
            && b[prefix + 1..].iter().all(|&c| c.is_ascii_uppercase())
            && b.len() - prefix - 1 <= 3
    };
    let pair = b.windows(2).any(|w| {
        (w[0].is_ascii_digit() && w[1].is_ascii_uppercase())
            || (w[0].is_ascii_uppercase() && w[1].is_ascii_digit())
    });
    (shape_ok(1) || shape_ok(2)) && pair
}

/// `raw_shape_and_pair_ok`, with the 3DA0/3X workaround applied first (a call like "3DA0AB"
/// only fits the digit-in-position shape after the substitution). No `split_portable` here
/// (deliberately, unlike an earlier draft): a base call carries no `/` at all — `"KD9TAW/P"`
/// is compound (see [`is_compound_call`]), and stripping `/P` first would wrongly accept it as
/// a bare base call.
pub fn is_base_call(s: &str) -> bool {
    raw_shape_and_pair_ok(&apply_prefix_workarounds(s))
}

/// varicode.cpp:1292-1310 `isCompoundCallsign`, in upstream's own precedence, NOT the
/// project-derived approximation an earlier draft of this file used:
///
/// 1. An exact `basecalls` entry that does not start with `@` — only `"<....>"` — is never
///    compound (:1293-1296).
/// 2. `base_callsign_pattern` fully matching the RAW string (its own optional
///    `(?<portable>[/][P])?` group, no 3DA0/3X substitution — the regex never applies it) means
///    it's a base call, portable or not, never compound (:1298-1300). This is why `"KD9TAW/P"`
///    is NOT compound (`"KD9TAW"` already fits the digit-in-position-2-or-3 shape with no
///    workaround) while `"3DA0AB/P"` STILL IS (`"3DA0AB"` only fits after the substitution,
///    which this step does not apply, so it falls through to step 3 below and stays compound).
/// 3. Otherwise `^compound_callsign_pattern` must match and `isValidCompoundCallsign`
///    (:1232-1258) decides: reject over 9 characters excluding `/`; a slash form is valid
///    unless the text before the FIRST `/` is itself a `basecalls` special (multiple slashes
///    are explicitly fine — there is no slash-count limit upstream, only the length one); an
///    `@`-prefixed name is valid; otherwise a digit-letter/letter-digit pair over 2+ characters
///    is valid.
pub fn is_compound_call(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 11 {
        return false;
    }
    // (1) the lone non-'@' special: "<....>".
    if let Some(c) = CallRef::parse(s) {
        if !matches!(c, CallRef::Base(_)) && !s.starts_with('@') {
            return false;
        }
    }
    // (2) base_callsign_pattern's own shape, /P included, checked on the RAW string.
    let raw_core = s.strip_suffix("/P").unwrap_or(s);
    if raw_shape_and_pair_ok(raw_core) {
        return false;
    }
    // (3) isValidCompoundCallsign, verbatim.
    let body_ok = b.iter().enumerate().all(|(i, &c)| {
        c.is_ascii_digit() || c.is_ascii_uppercase() || c == b'/' || (i == 0 && c == b'@')
    });
    if !body_ok || b.len() - b.iter().filter(|&&c| c == b'/').count() > 9 {
        return false;
    }
    if let Some(slash) = s.find('/') {
        return CallRef::parse(&s[..slash]).is_none_or(|c| matches!(c, CallRef::Base(_)));
    }
    if s.starts_with('@') {
        return true;
    }
    b.len() > 2
        && b.windows(2).any(|w| {
            (w[0].is_ascii_digit() && w[1].is_ascii_uppercase())
                || (w[0].is_ascii_uppercase() && w[1].is_ascii_digit())
        })
}

/// varicode.cpp:1322-1328 `isGroupAllowed`: operators may not transmit to `@APRSIS` or `@JS8NET`.
pub fn may_transmit_to(c: &CallRef) -> bool {
    match c {
        CallRef::Js8Net => false,
        CallRef::Group(i) => GROUPS.get(*i as usize) != Some(&"@APRSIS"),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nbasecall_and_the_special_range_fit_28_bits() {
        assert_eq!(NBASECALL, 37 * 36 * 10 * 27 * 27 * 27);
        const { assert!(NBASECALL + 54 < (1 << 28)) };
    }

    #[test]
    fn pack28_vectors_from_varicode_cpp_semantics() {
        // Computed by walking varicode.cpp:945-1005 (permutations, regex, mixed radix) by hand.
        let cases: [(&str, u32); 12] = [
            ("KD9TAW", 144_467_410),
            ("W1AW", 261_410_543),
            ("K1JT", 259_055_063),
            ("KD9TA", 144_467_414),
            ("3DA0AB", 23_816_483),
            ("3XA1BC", 186_221_672),
            ("W1A", 261_410_651),
            ("F5RXL", 258_155_570),
            ("LZ1ABC", 155_712_242),
            ("2E0ABC", 16_927_409),
            ("A1", 257_099_345),
            ("W12ABC", 226_984_385),
        ];
        for (call, v) in cases {
            assert_eq!(pack28(&CallRef::Base(call.to_string())), Some(v), "{call}");
            assert_eq!(unpack28(v), Some(CallRef::Base(call.to_string())), "{v}");
        }
        assert_eq!(
            pack28(&CallRef::Base("kd9taw".into())),
            Some(144_467_410),
            "upstream uppercases"
        );
        assert_eq!(
            pack28(&CallRef::Base("VK".into())),
            None,
            "no digit in position 3 of any permutation"
        );
        assert_eq!(
            pack28(&CallRef::Base("KD9TAW/P".into())),
            None,
            "split_portable first — Base carries no /P"
        );
        assert_eq!(
            pack28(&CallRef::Base("KD9TAW/QRP".into())),
            None,
            "compound calls go through pack50"
        );
        assert_eq!(pack28(&CallRef::Base("".into())), None);
    }

    #[test]
    fn specials_pack_above_nbasecall_in_upstream_order() {
        assert_eq!(pack28(&CallRef::Placeholder), Some(NBASECALL + 1));
        assert_eq!(pack28(&CallRef::AllCall), Some(NBASECALL + 2));
        assert_eq!(pack28(&CallRef::Js8Net), Some(NBASECALL + 3));
        assert_eq!(pack28(&CallRef::Group(0)), Some(NBASECALL + 4));
        assert_eq!(pack28(&CallRef::Group(50)), Some(NBASECALL + 54));
        assert_eq!(pack28(&CallRef::Group(51)), None);
        assert_eq!(unpack28(NBASECALL + 1), Some(CallRef::Placeholder));
        assert_eq!(unpack28(NBASECALL + 2), Some(CallRef::AllCall));
        assert_eq!(unpack28(NBASECALL + 45), Some(CallRef::Group(41)));
        assert_eq!(unpack28(NBASECALL + 45).unwrap().render(), "@HB");
        assert_eq!(unpack28(NBASECALL + 55), None);
        assert_eq!(unpack28(u32::MAX), None);
    }

    #[test]
    fn parse_and_render_are_inverse_and_exact() {
        for (s, want) in [
            ("<....>", CallRef::Placeholder),
            ("@ALLCALL", CallRef::AllCall),
            ("@JS8NET", CallRef::Js8Net),
            ("@APRSIS", CallRef::Group(29)),
            ("@POTA", CallRef::Group(48)),
            ("KD9TAW", CallRef::Base("KD9TAW".into())),
            ("kd9taw", CallRef::Base("KD9TAW".into())),
        ] {
            let c = CallRef::parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(c, want, "{s}");
            assert_eq!(c.render(), s.to_ascii_uppercase(), "{s}");
        }
        for bad in ["", "@NOSUCH", "KD9TAW/P", "KD9TAW/QRP", "HELLO", "W", "12"] {
            assert_eq!(CallRef::parse(bad), None, "{bad:?}");
        }
        assert!(CallRef::AllCall.is_group());
        assert!(CallRef::Js8Net.is_group());
        assert!(CallRef::Group(3).is_group());
        assert!(!CallRef::Placeholder.is_group());
        assert!(!CallRef::Base("W1AW".into()).is_group());
    }

    #[test]
    fn pack50_vectors_and_round_trip() {
        // varicode.cpp:857-943 packAlphaNumeric50 / unpackAlphaNumeric50.
        let cases: [(&str, u64); 7] = [
            ("KN4CRD/QRP", 358_399_795_421_803),
            ("VE3/LB9YHX", 545_579_025_695_551),
            ("@RACES", 673_343_688_580_748),
            ("KD9TAW", 353_886_015_991_948),
            ("W1AW", 557_100_718_697_932),
            ("@ALLCALL", 665_697_326_060_246),
            ("3DA0AB/P", 58_243_596_414_376),
        ];
        for (s, v) in cases {
            assert_eq!(pack50(s), Some(v), "{s}");
            assert!(v < (1 << 50));
            assert_eq!(unpack50(v), s, "{v}");
        }
        assert_eq!(
            pack50("kn4crd/qrp"),
            pack50("KN4CRD/QRP"),
            "uppercased first"
        );
        assert_eq!(pack50("KN4CRD/QRP/EXTRA"), None, "more than 11 slots");
        // NOT Some(0): real packAlphaNumeric50 has no empty-input special case — an empty
        // string still pads to 11 SPACES (ALNUM39 index 36, not 0) and packs that. Verified
        // against varicode.cpp:874-895 directly (fetched, sha256-pinned via B1.5's generator)
        // and by hand: 11 slots of index 36 through the same radix chain as every other call.
        assert_eq!(pack50(""), Some(642_997_345_742_028));
        // Symmetric: unpack(0) is NOT "" either — every slot decodes to alphanumeric[0] = '0'
        // (0 % any radix is 0), giving nine '0' characters after the two flag-slot spaces
        // (which decode to ' ' and get stripped, same as unpack50's real "replace(' ', '')").
        assert_eq!(unpack50(0), "000000000");
    }

    #[test]
    fn portable_split_and_the_two_validators() {
        assert_eq!(split_portable("W1AW/P"), ("W1AW", true));
        assert_eq!(split_portable("W1AW"), ("W1AW", false));
        assert_eq!(split_portable("W1AW/QRP"), ("W1AW/QRP", false));
        // varicode.cpp:40 base regex + isValidCallsign's "> 2 chars and a digit-letter pair" rule.
        // NOT mutually exclusive with is_compound_call for every entry: "3DA0AB" and "3XA1BC"
        // (workaround-only shapes) genuinely satisfy BOTH predicates upstream too — real
        // isCompoundCallsign's base_callsign_pattern check never applies the 3DA0/3X
        // substitution either, so "3DA0AB" falls through to the compound path and its own
        // digit-letter pair ('3','D') makes isValidCompoundCallsign accept it. Verified against
        // varicode.cpp:1292-1310 directly — this is upstream's own overlap, not a defect here.
        for good in [
            "KD9TAW", "W1AW", "K1JT", "F5RXL", "2E0ABC", "W12ABC", "W1A", "A1B",
        ] {
            assert!(is_base_call(good), "{good}");
            assert!(!is_compound_call(good), "{good}");
        }
        for workaround_only in ["3DA0AB", "3XA1BC"] {
            assert!(is_base_call(workaround_only), "{workaround_only}");
            assert!(
                is_compound_call(workaround_only),
                "{workaround_only}: real JS8Call's own base_callsign_pattern never applies the \
                 workaround either, so this genuinely satisfies both predicates upstream"
            );
        }
        for bad in [
            "",
            "W",
            "A1",
            "KD9TAW/P",
            "KD9TAW/QRP",
            "HELLO",
            "@POTA",
            "12345",
            "ABCDEF",
            "KD9TAWX",
        ] {
            assert!(!is_base_call(bad), "{bad:?}");
        }
        // varicode.cpp:1232-1258 isValidCompoundCallsign: ≤ 9 chars excluding '/', group or slash
        // form or a digit-letter pair, never a base call. Multiple slashes ARE permitted
        // upstream (isValidCompoundCallsign has no slash-count limit beyond the length rule) —
        // "A/B/C/D/E/F" is 11 chars / 5 slashes, 11-5=6 ≤ 9, and "A" (the text before the FIRST
        // slash) is not a basecalls special, so real JS8Call accepts it.
        for good in [
            "KD9TAW/QRP",
            "VE3/LB9YHX",
            "@POTA",
            "3DA0AB/P",
            "@ALLCALL",
            "@DX/NA",
            "A/B/C/D/E/F",
        ] {
            assert!(is_compound_call(good), "{good}");
        }
        for bad in [
            "KD9TAW",
            "HELLO",
            "",
            "ABCDEFGHIJ/K",
            "W1AW/ABCDEFGHIJ",
            "A1CDEFGHIJ",
            "@ALLCALL/X",
        ] {
            assert!(!is_compound_call(bad), "{bad:?}");
        }
        // "KD9TAW/P" is NOT compound (unlike "3DA0AB/P"): varicode.cpp's isCompoundCallsign
        // checks base_callsign_pattern FIRST, and that pattern's own optional `(?<portable>[/]
        // [P])?` group absorbs a trailing /P — "KD9TAW" alone already fits the digit-in-
        // position-2-or-3 shape with no workaround, so "KD9TAW/P" matches base_callsign_pattern
        // fully and upstream calls it a (portable) base call, never compound. "3DA0AB" does NOT
        // fit that shape without the Swaziland workaround, and the regex never applies it, so
        // "3DA0AB/P" falls through to the compound path instead and stays compound (see above).
        assert!(
            !is_compound_call("KD9TAW/P"),
            "matches base_callsign_pattern's own /P group"
        );
        assert!(
            is_base_call("KD9TAW"),
            "the /P-stripped core is a real base call"
        );
    }

    #[test]
    fn transmit_guard_refuses_aprsis_and_js8net() {
        // mainwindow.cpp:4043 / :4068 via varicode.cpp isGroupAllowed.
        assert!(!may_transmit_to(&CallRef::Js8Net));
        assert!(!may_transmit_to(&CallRef::parse("@APRSIS").unwrap()));
        assert!(may_transmit_to(&CallRef::AllCall));
        assert!(may_transmit_to(&CallRef::parse("@POTA").unwrap()));
        assert!(may_transmit_to(&CallRef::Base("W1AW".into())));
        assert!(may_transmit_to(&CallRef::Placeholder));
    }
}
