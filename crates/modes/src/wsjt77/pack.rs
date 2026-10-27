//! `pack77` from `lib/77bit/packjt77.f90` (the copy vendored in `libtempo/vendor/wsjtx`), ported
//! statement by statement, and the part of `unpack77` that decides whether `genft8` / `genft4`
//! call a message bad.
//!
//! WHAT "THE SAME" MEANS HERE. `genft8` and `genft4` read the 77 characters `pack77` writes, `0`
//! and `1` and, where a field was too wide for its format, gfortran's asterisks, and refuse the
//! message when that read fails or when `unpack77` (transmit side, `nrx = 0`) rejects it. This file
//! returns those 77 characters and that verdict, and nothing else: the text `unpack77` would print
//! is never needed, and the callsign tables `pack77` writes are written by the vendored encoder
//! itself (see the module above).
//!
//! WHERE THE FORTRAN IS NOT A FUNCTION OF THE TEXT, this port does not guess: it answers
//! [`Vendored`], and the caller encodes that message with the vendored encoder, lock and all, as
//! before. That is the case for a local read before anything is stored in it (`pack77_3`'s
//! `nserial` and `irpt`, `pack77_4`'s `c11` and `n12`, `pack77_06`'s `npfx`), a read that stops
//! the program (a list-directed read with no `err=` of a bad value), a read gfortran takes as a
//! repeat count (`5*3`), and a hashed call this message did not put in the table (its lookup
//! depends on what the decoder heard). None of these arise from a message Nexus composes.
//!
//! gfortran's formatted I/O is reproduced where `pack77` relies on it, from measurements of the
//! gfortran 13 runtime the modem links (the cases are in this file's tests): list-directed integer
//! input, `Z6` input and `Bw.w` output.
//!
//! ORIGIN AND LICENCE. Translated into Rust by KD9TAW, 2026-10-14, from `lib/77bit/packjt77.f90` of
//! WSJT-X, © Joe Taylor (K1JT) and the WSJT Development Group, GNU General Public License, version 3
//! (the copy vendored at `libtempo/vendor/wsjtx/`, which carries Nexus's own marked changes).
//! Distributed under the same licence, as part of Nexus (GPL-3.0-only). See NOTICE, "WSJT-X".

/// The vendored encoder must make this message: its result there depends on something this port
/// does not reproduce (see the module header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vendored;

type R<T> = Result<T, Vendored>;

/// A Fortran `character*13` word, blank padded.
type W13 = [u8; 13];

const NTOKENS: i64 = 2_063_592;
const MAX22: i64 = 4_194_304;
const MAXGRID4: i64 = 32_400;
const NZZZ: i64 = 46_656;

/// `csec`: the ARRL sections, in `pack77_03`'s order (index + 1 is the field's value).
const CSEC: [&[u8; 3]; 86] = [
    b"AB ", b"AK ", b"AL ", b"AR ", b"AZ ", b"BC ", b"CO ", b"CT ", b"DE ", b"EB ", b"EMA", b"ENY",
    b"EPA", b"EWA", b"GA ", b"GH ", b"IA ", b"ID ", b"IL ", b"IN ", b"KS ", b"KY ", b"LA ", b"LAX",
    b"NS ", b"MB ", b"MDC", b"ME ", b"MI ", b"MN ", b"MO ", b"MS ", b"MT ", b"NC ", b"ND ", b"NE ",
    b"NFL", b"NH ", b"NL ", b"NLI", b"NM ", b"NNJ", b"NNY", b"TER", b"NTX", b"NV ", b"OH ", b"OK ",
    b"ONE", b"ONN", b"ONS", b"OR ", b"ORG", b"PAC", b"PR ", b"QC ", b"RI ", b"SB ", b"SC ", b"SCV",
    b"SD ", b"SDG", b"SF ", b"SFL", b"SJV", b"SK ", b"SNJ", b"STX", b"SV ", b"TN ", b"UT ", b"VA ",
    b"VI ", b"VT ", b"WCF", b"WI ", b"WMA", b"WNY", b"WPA", b"WTX", b"WV ", b"WWA", b"WY ", b"DX ",
    b"PE ", b"NB ",
];

/// `cmult`: the ARRL RTTY Roundup multipliers, in `pack77_3`'s order.
const CMULT: [&[u8; 3]; 171] = [
    b"AL ", b"AK ", b"AZ ", b"AR ", b"CA ", b"CO ", b"CT ", b"DE ", b"FL ", b"GA ", b"HI ", b"ID ",
    b"IL ", b"IN ", b"IA ", b"KS ", b"KY ", b"LA ", b"ME ", b"MD ", b"MA ", b"MI ", b"MN ", b"MS ",
    b"MO ", b"MT ", b"NE ", b"NV ", b"NH ", b"NJ ", b"NM ", b"NY ", b"NC ", b"ND ", b"OH ", b"OK ",
    b"OR ", b"PA ", b"RI ", b"SC ", b"SD ", b"TN ", b"TX ", b"UT ", b"VT ", b"VA ", b"WA ", b"WV ",
    b"WI ", b"WY ", b"NB ", b"NS ", b"QC ", b"ON ", b"MB ", b"SK ", b"AB ", b"BC ", b"NWT", b"NF ",
    b"LB ", b"NU ", b"YT ", b"PEI", b"DC ", b"DR ", b"FR ", b"GD ", b"GR ", b"OV ", b"ZH ", b"ZL ",
    b"X01", b"X02", b"X03", b"X04", b"X05", b"X06", b"X07", b"X08", b"X09", b"X10", b"X11", b"X12",
    b"X13", b"X14", b"X15", b"X16", b"X17", b"X18", b"X19", b"X20", b"X21", b"X22", b"X23", b"X24",
    b"X25", b"X26", b"X27", b"X28", b"X29", b"X30", b"X31", b"X32", b"X33", b"X34", b"X35", b"X36",
    b"X37", b"X38", b"X39", b"X40", b"X41", b"X42", b"X43", b"X44", b"X45", b"X46", b"X47", b"X48",
    b"X49", b"X50", b"X51", b"X52", b"X53", b"X54", b"X55", b"X56", b"X57", b"X58", b"X59", b"X60",
    b"X61", b"X62", b"X63", b"X64", b"X65", b"X66", b"X67", b"X68", b"X69", b"X70", b"X71", b"X72",
    b"X73", b"X74", b"X75", b"X76", b"X77", b"X78", b"X79", b"X80", b"X81", b"X82", b"X83", b"X84",
    b"X85", b"X86", b"X87", b"X88", b"X89", b"X90", b"X91", b"X92", b"X93", b"X94", b"X95", b"X96",
    b"X97", b"X98", b"X99",
];

// ---- Fortran character semantics -----------------------------------------------------------

/// `len(trim(s))`.
fn len_trim(s: &[u8]) -> usize {
    s.iter().rposition(|&c| c != b' ').map_or(0, |i| i + 1)
}

/// `index(s, c)`: the 1-based position of the first `c`, 0 when there is none.
fn index(s: &[u8], c: u8) -> usize {
    s.iter().position(|&x| x == c).map_or(0, |i| i + 1)
}

/// `index(s, pat)` for a longer pattern.
fn index_str(s: &[u8], pat: &[u8]) -> usize {
    s.windows(pat.len())
        .position(|w| w == pat)
        .map_or(0, |i| i + 1)
}

/// Assignment to a `character*N`: truncated, or padded with blanks.
fn assign<const N: usize>(src: &[u8]) -> [u8; N] {
    let mut out = [b' '; N];
    let n = src.len().min(N);
    out[..n].copy_from_slice(&src[..n]);
    out
}

/// Fortran's `==` on characters: the shorter operand is padded with blanks.
fn feq(a: &[u8], b: &[u8]) -> bool {
    let n = a.len().max(b.len());
    (0..n).all(|i| a.get(i).copied().unwrap_or(b' ') == b.get(i).copied().unwrap_or(b' '))
}

/// `adjustr`: trailing blanks moved to the front.
fn adjustr<const N: usize>(s: &[u8; N]) -> [u8; N] {
    let n = len_trim(s);
    let mut out = [b' '; N];
    out[N - n..].copy_from_slice(&s[..n]);
    out
}

/// The character at 1-based position `i` of the string `alphabet` holds `c`, 0 when it is absent
/// (`index(alphabet, c)`), less one: the digit `pack77` takes for a character.
fn digit(alphabet: &[u8], c: u8) -> i64 {
    index(alphabet, c) as i64 - 1
}

// ---- gfortran formatted I/O -------------------------------------------------------------------

/// What a list-directed `read(field,*) n` does to an integer `n` (gfortran 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ld {
    /// `n` takes this value.
    Value(i32),
    /// A null value or a slash: `n` is left as it was, and no error is raised.
    Unchanged,
    /// An error condition (`err=` is taken; with no `err=` the program stops).
    Error,
    /// End of file: the field is blank (`end=` is taken; with no `end=` the program stops).
    End,
}

/// gfortran's list-directed input of one integer from `field` (an internal file of one record).
/// Measured: leading blanks are skipped; a leading `,` or `/` leaves `n` unchanged; an optional
/// sign must be followed by a digit; the digits must end at the field's end, a blank, a `,` or a
/// `/`; anything else, or a value outside 32 bits, is an error. A `*` after the digits is a repeat
/// count, which this port does not reproduce.
fn ld_int(field: &[u8]) -> R<Ld> {
    let mut i = 0;
    while i < field.len() && field[i] == b' ' {
        i += 1;
    }
    if i == field.len() {
        return Ok(Ld::End);
    }
    if field[i] == b',' || field[i] == b'/' {
        return Ok(Ld::Unchanged);
    }
    let negative = field[i] == b'-';
    if field[i] == b'+' || field[i] == b'-' {
        i += 1;
    }
    let start = i;
    let mut value: i64 = 0;
    while i < field.len() && field[i].is_ascii_digit() {
        value = (value * 10 + i64::from(field[i] - b'0')).min(1 << 40);
        i += 1;
    }
    if i == start {
        return Ok(Ld::Error);
    }
    if i < field.len() {
        match field[i] {
            b' ' | b',' | b'/' => {}
            b'*' => return Err(Vendored),
            _ => return Ok(Ld::Error),
        }
    }
    let value = if negative { -value } else { value };
    match i32::try_from(value) {
        Ok(v) => Ok(Ld::Value(v)),
        Err(_) => Ok(Ld::Error),
    }
}

/// One `Z6` input field, in a record whose blanks are all leading (the telemetry word, right
/// adjusted): blanks are ignored, an optional sign may lead, and the rest must be hex digits. A
/// sign that ends the field is an error. All blank reads as zero.
fn z6(field: &[u8]) -> Option<i32> {
    let mut i = 0;
    while i < field.len() && field[i] == b' ' {
        i += 1;
    }
    if i == field.len() {
        return Some(0);
    }
    let negative = field[i] == b'-';
    if field[i] == b'+' || field[i] == b'-' {
        i += 1;
        if i == field.len() {
            return None;
        }
    }
    let mut value: i32 = 0;
    for &c in &field[i..] {
        let d = (c as char).to_digit(16)?;
        value = value * 16 + d as i32;
    }
    Some(if negative { -value } else { value })
}

/// The 77 characters `pack77` writes, as gfortran writes them.
struct C77(Vec<u8>);

impl C77 {
    fn new() -> Self {
        C77(Vec::with_capacity(77))
    }

    /// `Bw.w` output of `v`: `w` binary digits, or `w` asterisks when `v` is negative or needs more
    /// than `w` digits (measured).
    fn b(&mut self, v: i64, w: u32) -> &mut Self {
        if v < 0 || (w < 63 && v >= 1 << w) {
            self.0.extend(std::iter::repeat_n(b'*', w as usize));
        } else {
            self.0.extend(
                (0..w)
                    .rev()
                    .map(|k| if (v >> k) & 1 == 1 { b'1' } else { b'0' }),
            );
        }
        self
    }

    fn done(self) -> [u8; 77] {
        self.0
            .try_into()
            .expect("every pack77 format writes exactly 77 characters")
    }
}

// ---- The callsign hash, and the table the transmit-side unpack reads ------------------------------

/// `ihashcall(c0, m)`: the `m`-bit hash of a callsign's first 11 characters.
fn ihashcall(c0: &[u8], m: u32) -> i64 {
    const C: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ/";
    let mut n8: i64 = 0;
    for i in 0..11 {
        n8 = 38 * n8 + digit(C, c0.get(i).copied().unwrap_or(b' '));
    }
    (47_055_833_459i64.wrapping_mul(n8) as u64 >> (64 - m)) as i64
}

/// The 22-bit hashes this encode stores with `save_hash_call`. The transmit-side `unpack77` looks
/// a hashed call up in the table; for a hash stored here the lookup returns this message's own
/// call (`save_hash_call` overwrites every entry with that hash), and for any other the answer
/// depends on what the decoder has heard.
#[derive(Default)]
struct Ledger {
    saved22: Vec<i64>,
    /// `pack77` read a local it had not assigned (the cases this file's header names): the frame
    /// the Fortran writes is not decided by the text ([`reads_unassigned`]).
    unassigned: bool,
}

impl Ledger {
    /// `save_hash_call(c13, n10, n12, n22)`: the three hashes it stores, or `None` when it returns
    /// before storing (and before setting them).
    fn save(&mut self, c13: &W13) -> Option<(i64, i64, i64)> {
        let mut cw = *c13;
        if cw[0] == b' ' || &cw[..5] == b"<...>" {
            return None;
        }
        if cw[0] == b'<' {
            cw = assign(&c13[1..]);
        }
        let i = index(&cw, b'>');
        if i > 0 {
            cw[i - 1..].fill(b' ');
        }
        if len_trim(&cw) < 3 {
            return None;
        }
        let n22 = ihashcall(&cw, 22);
        self.saved22.push(n22);
        Some((ihashcall(&cw, 10), ihashcall(&cw, 12), n22))
    }
}

// ---- chkcall, split77 ---------------------------------------------------------------------------

/// `chkcall(w, bc, cok)` from `lib/chkcall.f90`: the base call `bc` as the routine leaves it, and
/// whether `w` could be a standard or compound callsign.
fn chkcall(w: &W13) -> ([u8; 6], bool) {
    let isdigit = |c: u8| c.is_ascii_digit();
    let isletter = |c: u8| c.is_ascii_uppercase();
    let mut bc: [u8; 6] = assign(&w[..6]);
    let n1 = len_trim(w) as i64;
    if n1 > 11 || [b'.', b'+', b'-', b'?'].iter().any(|&c| index(w, c) >= 1) {
        return (bc, false);
    }
    let i0 = index(w, b'/') as i64;
    if n1 > 6 && i0 <= 0 {
        return (bc, false);
    }
    if (i0 - 1).max(n1 - i0) > 6 {
        return (bc, false);
    }
    if i0 >= 2 && i0 < n1 {
        let (i0, n1) = (i0 as usize, n1 as usize);
        if i0 - 1 <= n1 - i0 {
            bc = assign(&w[i0..n1]);
        }
        if i0 - 1 > n1 - i0 {
            bc = assign(&w[..i0 - 1]);
        }
    }
    let nbc = len_trim(&bc);
    if !isletter(bc[0]) && !isletter(bc[1]) {
        return (bc, false);
    }
    if bc[0] == b'Q' && &bc[..5] != b"QU1RK" {
        return (bc, false);
    }
    let mut i1 = 0;
    if isdigit(bc[1]) {
        i1 = 2;
    }
    if isdigit(bc[2]) {
        i1 = 3;
    }
    if i1 == 0 || i1 == nbc {
        return (bc, false);
    }
    let mut n = 0;
    for &c in &bc[i1..nbc] {
        if !c.is_ascii_uppercase() {
            return (bc, false);
        }
        n += 1;
    }
    (bc, (1..=3).contains(&n))
}

/// `split77`: `msg` upper-cased with its blanks collapsed (in place, as the routine does), its
/// words (13 characters kept of each), and their count, with "CQ <token> <call>" folded into
/// `CQ_<token>`.
fn split77(msg: &mut [u8; 37]) -> (usize, [W13; 19]) {
    let iz = len_trim(msg);
    let mut w = [[b' '; 13]; 19];
    let (mut j, mut k, mut n) = (0usize, 0usize, 0usize);
    let mut c0 = b' ';
    for i in 0..iz {
        if msg[i] == 0 {
            msg[i] = b' ';
        }
        let mut c = msg[i];
        if c == b' ' && c0 == b' ' {
            continue;
        }
        if c != b' ' && c0 == b' ' {
            k += 1;
            n = 0;
        }
        j += 1;
        n += 1;
        if c.is_ascii_lowercase() {
            c -= 32;
        }
        msg[j - 1] = c;
        if n <= 13 {
            w[k - 1][n - 1] = c;
        }
        c0 = c;
    }
    let mut nwords = k;
    if nwords == 0 {
        return (0, w);
    }
    msg[j..].fill(b' ');
    if nwords >= 3 && chkcall(&w[2]).1 && &w[0][..3] == b"CQ " {
        let mut cq = [b' '; 13];
        cq[..3].copy_from_slice(b"CQ_");
        cq[3..].copy_from_slice(&w[1][..10]);
        w[0] = cq;
        let old = w;
        w[1..12].copy_from_slice(&old[2..13]);
        nwords -= 1;
    }
    (nwords, w)
}

// ---- pack28 / unpack28 ----------------------------------------------------------------------------

/// `pack28(c13, n28)`: a special token, a 22-bit hash or a standard call as a 28-bit number. Like
/// the Fortran it rewrites a `<call>` argument to the bare call.
fn pack28(c13: &mut W13, ledger: &mut Ledger) -> R<i64> {
    let n28 = pack28_raw(c13, ledger)?;
    Ok(n28 & ((1 << 28) - 1))
}

fn pack28_raw(c13: &mut W13, ledger: &mut Ledger) -> R<i64> {
    const A1: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const A2: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const A3: &[u8] = b"0123456789";
    const A4: &[u8] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    // (The '3DA0' and '3X' work-arounds assign a local that the standard-call branch below always
    // overwrites, so they change nothing.)
    if &c13[..3] == b"DE " {
        return Ok(0);
    }
    if &c13[..4] == b"QRZ " {
        return Ok(1);
    }
    if &c13[..3] == b"CQ " {
        return Ok(2);
    }
    if &c13[..3] == b"CQ_" {
        let n = len_trim(c13);
        if (4..=7).contains(&n) {
            let nlet = c13[3..n].iter().filter(|c| c.is_ascii_uppercase()).count();
            let nnum = c13[3..n].iter().filter(|c| c.is_ascii_digit()).count();
            if nnum == 3 && nlet == 0 {
                // `read(c13(4:3+nnum),*) nqsy` has no err=: anything but a value stops the program
                // or leaves nqsy undefined.
                return match ld_int(&c13[3..6])? {
                    Ld::Value(nqsy) => Ok(3 + i64::from(nqsy)),
                    _ => Err(Vendored),
                };
            }
            if (1..=4).contains(&nlet) && nnum == 0 {
                let c4 = adjustr(&assign::<4>(&c13[3..n]));
                let m = c4.iter().fold(0i64, |m, &c| {
                    27 * m
                        + if c.is_ascii_uppercase() {
                            i64::from(c - b'A' + 1)
                        } else {
                            0
                        }
                });
                return Ok(3 + 1000 + m);
            }
        }
    }
    if c13[0] == b'<' {
        ledger.save(c13);
        let i2 = index(c13, b'>');
        *c13 = if i2 >= 2 {
            assign(&c13[1..i2 - 1])
        } else {
            [b' '; 13]
        };
        return Ok(NTOKENS + ihashcall(c13, 22));
    }
    let n = len_trim(c13);
    // `do i=n,2,-1 … exit … enddo; iarea=i`: the last digit at or after position 2; 1 when there is
    // none, and n itself when n < 2 (the loop never runs).
    let mut iarea = n;
    while iarea >= 2 && !c13[iarea - 1].is_ascii_digit() {
        iarea -= 1;
    }
    let npdig = c13[..iarea.saturating_sub(1)]
        .iter()
        .filter(|c| c.is_ascii_digit())
        .count();
    let nplet = c13[..iarea.saturating_sub(1)]
        .iter()
        .filter(|c| c.is_ascii_uppercase())
        .count();
    let nslet = c13[iarea.min(n)..n]
        .iter()
        .filter(|c| c.is_ascii_uppercase())
        .count();
    if !(2..=3).contains(&iarea) || nplet == 0 || npdig + 1 >= iarea || nslet > 3 {
        ledger.save(c13);
        return Ok(NTOKENS + ihashcall(c13, 22));
    }
    ledger.save(c13);
    let callsign: [u8; 6] = if iarea == 2 {
        let mut s = [b' '; 6];
        s[1..].copy_from_slice(&c13[..5]);
        s
    } else {
        assign(&c13[..6])
    };
    let i1 = digit(A1, callsign[0]);
    let i2 = digit(A2, callsign[1]);
    let i3 = digit(A3, callsign[2]);
    let i4 = digit(A4, callsign[3]);
    let i5 = digit(A4, callsign[4]);
    let i6 = digit(A4, callsign[5]);
    Ok(36 * 10 * 27 * 27 * 27 * i1
        + 10 * 27 * 27 * 27 * i2
        + 27 * 27 * 27 * i3
        + 27 * 27 * i4
        + 27 * i5
        + i6
        + NTOKENS
        + MAX22)
}

/// `callok(w)` (from WSJT-X 3.0.2, outside the module): could `w` be a real callsign?
fn callok(w: &[u8]) -> bool {
    let n = len_trim(w);
    if n < 3 || w[0] == b'Q' {
        return false;
    }
    let mut i0 = n;
    while i0 >= 1 && !w[i0 - 1].is_ascii_digit() {
        i0 -= 1;
    }
    if i0 != 2 && i0 != 3 {
        return false;
    }
    let pfx: [u8; 2] = assign(&w[..i0 - 1]);
    let sfx: [u8; 3] = assign(&w[i0..n]);
    let islet = |c: &u8| c.is_ascii_alphabetic();
    if pfx[..len_trim(&pfx)].iter().filter(|c| islet(c)).count() == 0 {
        return false;
    }
    let ns = len_trim(&sfx);
    sfx[..ns].iter().filter(|c| islet(c)).count() >= ns
}

/// `unpack28(n28, c13, success)`, as far as the transmit-side `unpack77` needs it: the call's
/// text (a lookup returns this message's own `<call>`, whose spelling never matters here) and
/// the success flag.
fn unpack28(n28: i64, ledger: &Ledger) -> R<(Vec<u8>, bool)> {
    const C1: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const C2: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const C3: &[u8] = b"0123456789";
    const C4: &[u8] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut ok = true;
    let mut c13: Vec<u8> = 'decoded: {
        if n28 < NTOKENS {
            match n28 {
                0 => break 'decoded b"DE".to_vec(),
                1 => break 'decoded b"QRZ".to_vec(),
                2 => break 'decoded b"CQ".to_vec(),
                3..=1002 => break 'decoded format!("CQ_{:03}", n28 - 3).into_bytes(),
                1003..=532_443 => {
                    let n = n28 - 1003;
                    let i1 = n / (27 * 27 * 27);
                    let r = n - 27 * 27 * 27 * i1;
                    let i2 = r / (27 * 27);
                    let r = r - 27 * 27 * i2;
                    let i3 = r / 27;
                    let i4 = r - 27 * i3;
                    let s: Vec<u8> = [i1, i2, i3, i4].iter().map(|&i| C4[i as usize]).collect();
                    let s = &s[s.iter().position(|&c| c != b' ').unwrap_or(s.len())..];
                    let mut out = b"CQ_".to_vec();
                    out.extend_from_slice(s);
                    break 'decoded out;
                }
                _ => {}
            }
        }
        let n22 = n28 - NTOKENS;
        if n22 < MAX22 {
            // `hash22`: no stored hash is negative; a stored one is this message's own call.
            if n22 < 0 {
                break 'decoded b"<...>".to_vec();
            }
            if ledger.saved22.contains(&n22) {
                break 'decoded b"<CALL>".to_vec();
            }
            return Err(Vendored);
        }
        let mut n = n22 - MAX22;
        let i1 = n / (36 * 10 * 27 * 27 * 27);
        n -= 36 * 10 * 27 * 27 * 27 * i1;
        let i2 = n / (10 * 27 * 27 * 27);
        n -= 10 * 27 * 27 * 27 * i2;
        let i3 = n / (27 * 27 * 27);
        n -= 27 * 27 * 27 * i3;
        let i4 = n / (27 * 27);
        n -= 27 * 27 * i4;
        let i5 = n / 27;
        let i6 = n - 27 * i5;
        let raw = [
            C1[i1 as usize],
            C2[i2 as usize],
            C3[i3 as usize],
            C4[i4 as usize],
            C4[i5 as usize],
            C4[i6 as usize],
        ];
        let s = raw[raw.iter().position(|&c| c != b' ').unwrap_or(6)..].to_vec();
        if callok(&s) {
            s
        } else {
            ok = false;
            b"QU1RK".to_vec()
        }
    };
    let i0 = index(&c13, b' ');
    if i0 != 0 && i0 < len_trim(&c13) {
        c13 = b"QU1RK".to_vec();
        ok = false;
    }
    Ok((c13, ok))
}

// ---- The message-type packers, in pack77's order ------------------------------------------------

/// `pack77_01`: DXpedition mode, "K1ABC RR73; W9XYZ <KH1/KH7Z> -11".
fn pack77_01(nwords: usize, w: &mut [W13; 19], ledger: &mut Ledger) -> R<Option<[u8; 77]>> {
    if nwords != 5 || !feq(&w[1], b"RR73;") || w[3][0] != b'<' || index(&w[3], b'>') < 1 {
        return Ok(None);
    }
    let n = match ld_int(&w[4])? {
        Ld::Value(v) => v,
        Ld::Unchanged | Ld::Error => -99,
        Ld::End => return Err(Vendored),
    };
    if n == -99 {
        return Ok(None);
    }
    // `n5=(n+30)/2` is 32-bit: past i32::MAX - 30 it wraps, which this port does not reproduce.
    if n > i32::MAX - 30 {
        return Err(Vendored);
    }
    let n5 = ((n + 30) / 2).clamp(0, 31) as i64;
    if !chkcall(&w[0]).1 || !chkcall(&w[2]).1 {
        return Ok(None);
    }
    let n28a = pack28(&mut w[0], ledger)?;
    let n28b = pack28(&mut w[2], ledger)?;
    ledger.save(&w[3]);
    let i2 = index(&w[3], b'>');
    let c13: W13 = if i2 >= 2 {
        assign(&w[3][1..i2 - 1])
    } else {
        [b' '; 13]
    };
    let n10 = ihashcall(&c13, 10);
    let mut c = C77::new();
    c.b(n28a, 28)
        .b(n28b, 28)
        .b(n10, 10)
        .b(n5, 5)
        .b(1, 3)
        .b(0, 3);
    Ok(Some(c.done()))
}

/// `pack77_03`: ARRL Field Day, "WA9XYZ KA1ABC R 16A EMA".
fn pack77_03(nwords: usize, w: &mut [W13; 19], ledger: &mut Ledger) -> R<Option<[u8; 77]>> {
    if !(4..=5).contains(&nwords) {
        return Ok(None);
    }
    if !chkcall(&w[0]).1 || !chkcall(&w[1]).1 {
        return Ok(None);
    }
    let Some(isec) = CSEC
        .iter()
        .position(|s| s[..] == w[nwords - 1][..3])
        .map(|i| i as i64 + 1)
    else {
        return Ok(None);
    };
    if nwords == 5 && !feq(&w[2], b"R") {
        return Ok(None);
    }
    let class = w[nwords - 2];
    let j = len_trim(&class).saturating_sub(1);
    let ntx = match ld_int(&class[..j])? {
        Ld::Value(v) => i64::from(v),
        _ => -1,
    };
    if !(1..=32).contains(&ntx) {
        return Ok(None);
    }
    let nclass = i64::from(class[j]) - i64::from(b'A');
    let m = len_trim(&w[nwords - 1]);
    if !(2..=3).contains(&m) {
        return Ok(None);
    }
    let (mut n3, mut intx) = (3, ntx - 1);
    if intx >= 16 {
        n3 = 4;
        intx = ntx - 17;
    }
    let n28a = pack28(&mut w[0], ledger)?;
    let n28b = pack28(&mut w[1], ledger)?;
    let ir = i64::from(&w[2][..2] == b"R ");
    let mut c = C77::new();
    c.b(n28a, 28)
        .b(n28b, 28)
        .b(ir, 1)
        .b(intx, 4)
        .b(nclass, 3)
        .b(isec, 7)
        .b(n3, 3)
        .b(0, 3);
    Ok(Some(c.done()))
}

/// Telemetry (0.5), tried only for a message of one word: its first 18 characters, right
/// adjusted, read as three 6-digit hex numbers.
enum Telemetry {
    Packed([u8; 77]),
    /// The read succeeded and the first number is 2^23 or more: free text, at once.
    FreeText,
    /// Not telemetry: on to the other packers.
    Next,
}

fn telemetry(msg: &[u8; 37]) -> Telemetry {
    let i0 = index(msg, b' ');
    let word: &[u8] = if i0 >= 2 { &msg[..i0 - 1] } else { &[] };
    let c18 = adjustr(&assign::<18>(word));
    let mut ntel = [-99i64; 3];
    let mut read_ok = true;
    for (k, field) in c18.chunks(6).enumerate() {
        match z6(field) {
            Some(v) => ntel[k] = i64::from(v),
            None => {
                read_ok = false;
                break;
            }
        }
    }
    if read_ok && ntel[0] >= 1 << 23 {
        return Telemetry::FreeText;
    }
    if ntel.iter().all(|&v| v >= 0) {
        let mut c = C77::new();
        c.b(ntel[0], 23)
            .b(ntel[1], 24)
            .b(ntel[2], 24)
            .b(5, 3)
            .b(0, 3);
        return Telemetry::Packed(c.done());
    }
    Telemetry::Next
}

fn is_grid4(g: &[u8]) -> bool {
    len_trim(&g[..4]) == 4
        && (b'A'..=b'R').contains(&g[0])
        && (b'A'..=b'R').contains(&g[1])
        && g[2].is_ascii_digit()
        && g[3].is_ascii_digit()
}

fn grid4_value(g: &[u8]) -> i64 {
    i64::from(g[0] - b'A') * 18 * 10 * 10
        + i64::from(g[1] - b'A') * 10 * 10
        + i64::from(g[2] - b'0') * 10
        + i64::from(g[3] - b'0')
}

/// The integer a WSPR power field holds; its one or two characters were checked to be digits.
fn read_dbm(field: &W13) -> R<i64> {
    match ld_int(field)? {
        Ld::Value(v) => Ok(i64::from(v)),
        _ => Err(Vendored),
    }
}

/// `nint(0.3*idbm)` on a default real, the power clamped to 0..60 first.
fn dbm_code(idbm: i64) -> i64 {
    (0.3f32 * idbm.clamp(0, 60) as f32).round() as i64
}

/// `pack77_06`: the WSPR forms (types 1 and 2; type 3 needs a caller's hint `genft8` never gives).
fn pack77_06(nwords: usize, w: &mut [W13; 19], ledger: &mut Ledger) -> R<Option<[u8; 77]>> {
    const A2: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let m1 = len_trim(&w[0]);
    let m2 = len_trim(&w[1]);
    let m3 = len_trim(&w[2]);
    if nwords == 3 && (3..=6).contains(&m1) && m2 == 4 && m3 <= 2 {
        if !is_grid4(&w[1]) || !w[2][0].is_ascii_digit() {
            return Ok(None);
        }
        if m3 == 2 && !w[2][1].is_ascii_digit() {
            return Ok(None);
        }
        let n28 = pack28(&mut w[0], ledger)?;
        let igrid4 = grid4_value(&w[1]);
        let idbm = dbm_code(read_dbm(&w[2])?);
        let mut c = C77::new();
        c.b(n28, 28)
            .b(igrid4, 15)
            .b(idbm, 5)
            .b(0, 1)
            .b(0, 1)
            .b(0, 21)
            .b(6, 3)
            .b(0, 3);
        return Ok(Some(c.done()));
    }
    if nwords == 2 && (5..=10).contains(&m1) && m2 <= 2 {
        let word = w[0];
        let i1 = index(&word, b'/');
        if i1 < 2 || i1 == m1 || !w[1][0].is_ascii_digit() {
            return Ok(None);
        }
        if i1 + 3 == m1 && !word[m1 - 1].is_ascii_digit() {
            return Ok(None);
        }
        if m2 == 2 && !w[1][1].is_ascii_digit() {
            return Ok(None);
        }
        let (bcall, ok) = chkcall(&word);
        if !ok {
            return Ok(None);
        }
        let d = |c: u8| digit(A2, c);
        let npfx = if i1 <= 4 {
            let mut p = d(word[0]);
            if i1 >= 3 {
                p = 36 * p + d(word[1]);
            }
            if i1 == 4 {
                p = 36 * p + d(word[2]);
            }
            p
        } else {
            let p = match m1 - i1 {
                1 => d(word[i1]),
                2 => 36 * d(word[i1]) + d(word[i1 + 1]),
                3 => {
                    if !word[i1 + 2].is_ascii_digit() {
                        return Ok(None);
                    }
                    36 * 10 * d(word[i1]) + 10 * d(word[i1 + 1]) + d(word[i1 + 2])
                }
                // npfx is never assigned for a longer suffix.
                _ => {
                    ledger.unassigned = true;
                    return Err(Vendored);
                }
            };
            p + NZZZ
        };
        let n28 = pack28(&mut assign(&bcall), ledger)?;
        let idbm = dbm_code(read_dbm(&w[1])?);
        let mut c = C77::new();
        c.b(n28, 28)
            .b(npfx, 16)
            .b(idbm, 5)
            .b(1, 1)
            .b(0, 21)
            .b(6, 3)
            .b(0, 3);
        return Ok(Some(c.done()));
    }
    Ok(None)
}

/// `pack77_1`: types 1 and 2, the standard message with an optional /R or /P.
fn pack77_1(nwords: usize, w: &mut [W13; 19], ledger: &mut Ledger) -> R<Option<[u8; 77]>> {
    if !(2..=4).contains(&nwords) {
        return Ok(None);
    }
    let (bcall_1, mut ok1) = chkcall(&w[0]);
    let (bcall_2, mut ok2) = chkcall(&w[1]);
    if [&b"DE "[..], b"CQ_", b"CQ "].contains(&&w[0][..3]) || &w[0][..4] == b"QRZ " {
        ok1 = true;
    }
    if w[0][0] == b'<' && index(&w[0], b'>') >= 5 {
        ok1 = true;
    }
    if w[1][0] == b'<' && index(&w[1], b'>') >= 5 {
        ok2 = true;
    }
    if !ok1 || !ok2 {
        return Ok(None);
    }
    if w[0][0] == b'<' && index(&w[1], b'/') > 0 {
        return Ok(None);
    }
    if w[1][0] == b'<' && index(&w[0], b'/') > 0 {
        return Ok(None);
    }
    if nwords == 2 && index(&w[1], b'/') >= 2 {
        return Ok(None);
    }
    let mut ir = 0;
    let mut irpt = None;
    if nwords != 2 {
        let last = w[nwords - 1];
        let (c1, c2) = (last[0], &last[..2]);
        if !is_grid4(&last)
            && c1 != b'+'
            && c1 != b'-'
            && c2 != b"R+"
            && c2 != b"R-"
            && !feq(&last, b"RRR")
            && !feq(&last, b"RR73")
            && !feq(&last, b"73")
        {
            return Ok(None);
        }
        let report = |field: &[u8]| -> R<Option<i64>> {
            Ok(match ld_int(field)? {
                Ld::Value(v) => {
                    let mut v = i64::from(v);
                    if (-50..=-31).contains(&v) {
                        v += 101;
                    }
                    Some(v + 35)
                }
                Ld::Error => None,
                // A sign is never followed by a null or a blank field.
                Ld::Unchanged | Ld::End => return Err(Vendored),
            })
        };
        if c1 == b'+' || c1 == b'-' {
            ir = 0;
            match report(&last)? {
                Some(v) => irpt = Some(v),
                None => return Ok(None),
            }
        } else if c2 == b"R+" || c2 == b"R-" {
            ir = 1;
            match report(&last[1..])? {
                Some(v) => irpt = Some(v),
                None => return Ok(None),
            }
        } else if feq(&last, b"RRR") {
            ir = 0;
            irpt = Some(2);
        } else if feq(&last, b"RR73") {
            ir = 0;
            irpt = Some(3);
        } else if feq(&last, b"73") {
            ir = 0;
            irpt = Some(4);
        }
    }
    // 10:
    let mut w1 = w[0].to_vec();
    w1.push(b' ');
    let mut w2 = w[1].to_vec();
    w2.push(b' ');
    let i1psuffix = index_str(&w1, b"/P ");
    let i2psuffix = index_str(&w2, b"/P ");
    let mut i3 = -1;
    if nwords == 2 || nwords == 3 || (nwords == 4 && &w[2][..2] == b"R ") {
        i3 = 1;
        if i1psuffix >= 4 || i2psuffix >= 4 {
            i3 = 2;
        }
    }
    let mut c13: W13 = assign(&bcall_1);
    if &c13[..3] == b"CQ_" || w[0][0] == b'<' {
        c13 = w[0];
    }
    let n28a = pack28(&mut c13, ledger)?;
    let mut c13: W13 = assign(&bcall_2);
    if w[1][0] == b'<' {
        c13 = w[1];
    }
    let n28b = pack28(&mut c13, ledger)?;
    let ipa = i64::from(i1psuffix >= 4 || index_str(&w1, b"/R ") >= 4);
    let ipb = i64::from(i2psuffix >= 4 || index_str(&w2, b"/R ") >= 4);
    let last = w[nwords - 1];
    let igrid4 = if nwords == 2 {
        ir = 0;
        MAXGRID4 + 1
    } else if is_grid4(&last) {
        ir = i32::from(feq(&w[2], b"R"));
        grid4_value(&last)
    } else {
        // Every form but a grid set irpt above.
        MAXGRID4 + irpt.ok_or(Vendored)?
    };
    if i3 < 0 {
        // The routine still writes c77, with asterisks for i3, and pack77 goes on: a later packer
        // writes over it.
        return Ok(None);
    }
    let mut c = C77::new();
    c.b(n28a, 28)
        .b(ipa, 1)
        .b(n28b, 28)
        .b(ipb, 1)
        .b(i64::from(ir), 1)
        .b(igrid4, 15)
        .b(i3, 3);
    Ok(Some(c.done()))
}

/// `pack77_3`: the ARRL RTTY Roundup, "TU; W9XYZ K1ABC R 579 MA".
fn pack77_3(nwords: usize, w: &mut [W13; 19], ledger: &mut Ledger) -> R<Option<[u8; 77]>> {
    if w[0][0] == b'<' && w[1][0] == b'<' {
        return Ok(None);
    }
    if !(4..=6).contains(&nwords) {
        return Ok(None);
    }
    let tu = feq(&w[0], b"TU;");
    let i1 = usize::from(tu);
    if !chkcall(&w[i1]).1 || !chkcall(&w[i1 + 1]).1 {
        return Ok(None);
    }
    let crpt = &w[nwords - 2][..3];
    if index(crpt, b'-') >= 1 || index(crpt, b'+') >= 1 {
        return Ok(None);
    }
    // nserial is set only when the report looks like 5x9; elsewhere it is an uninitialised local.
    let nserial = if crpt[0] == b'5' && (b'2'..=b'9').contains(&crpt[1]) && crpt[2] == b'9' {
        Some(match ld_int(&w[nwords - 1])? {
            Ld::Value(v) => i64::from(v),
            Ld::Unchanged | Ld::Error => 0,
            Ld::End => return Err(Vendored),
        })
    } else {
        None
    };
    let imult = CMULT
        .iter()
        .position(|m| feq(&m[..], &w[nwords - 1]))
        .map_or(-1, |i| i as i64 + 1);
    let nexch = match nserial {
        Some(ns) => {
            if imult <= 0 && ns <= 0 {
                return Ok(None);
            }
            if imult > 0 {
                8000 + imult
            } else {
                ns
            }
        }
        None if imult > 0 => 8000 + imult,
        None => {
            ledger.unassigned = true;
            return Err(Vendored);
        }
    };
    let itu = usize::from(tu);
    let n28a = pack28(&mut w[itu], ledger)?;
    let n28b = pack28(&mut w[1 + itu], ledger)?;
    let ir = usize::from(&w[2 + itu][..2] == b"R ");
    // `read(w(3+itu+ir),*,err=900) irpt`: a failed read returns with i3 = 3 and c77 not written, a
    // null leaves irpt undefined, and a blank word (past the last) stops the program.
    let irpt = match ld_int(&w[2 + itu + ir])? {
        Ld::Value(v) => ((i64::from(v) - 509) / 10 - 2).clamp(0, 7),
        _ => {
            ledger.unassigned = true;
            return Err(Vendored);
        }
    };
    let mut c = C77::new();
    c.b(itu as i64, 1)
        .b(n28a, 28)
        .b(n28b, 28)
        .b(ir as i64, 1)
        .b(irpt, 3)
        .b(nexch, 13)
        .b(3, 3);
    Ok(Some(c.done()))
}

/// `pack77_4`: type 4, one nonstandard call and one hashed call, "<WA9XYZ> PJ4/KA1ABC RR73".
fn pack77_4(nwords: usize, w: &mut [W13; 19], ledger: &mut Ledger) -> R<Option<[u8; 77]>> {
    const C: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ/";
    if nwords != 2 && nwords != 3 {
        return Ok(None);
    }
    let unbracket = |word: &W13| -> W13 {
        if word[0] == b'<' {
            let n = len_trim(word);
            if n >= 3 {
                assign(&word[1..n - 1])
            } else {
                [b' '; 13]
            }
        } else {
            *word
        }
    };
    let call_1 = unbracket(&w[0]);
    let call_2 = unbracket(&w[1]);
    let (bcall_1, ok1) = chkcall(&call_1);
    let (bcall_2, ok2) = chkcall(&call_2);
    if feq(&call_1, &bcall_1) && feq(&call_2, &bcall_2) && ok1 && ok2 {
        return Ok(None);
    }
    let cq = feq(&w[0], b"CQ");
    let mut i3 = -1;
    let mut icq = 0;
    if cq || (ok1 && ok2) {
        if cq && len_trim(&w[1]) <= 4 {
            return Ok(None);
        }
        i3 = 4;
        if cq {
            icq = 1;
        }
    }
    // n12 is what save_hash_call leaves in it (unset when it returns early), c11 the call sent in full.
    let (iflip, n12, c11): (i64, Option<i64>, [u8; 11]) = if icq == 1 {
        let n12 = ledger.save(&w[1]).map_or(0, |h| h.1);
        (0, Some(n12), adjustr(&assign::<11>(&call_2)))
    } else if w[0][0] == b'<' {
        i3 = 4;
        let n12 = ledger.save(&w[0]).map(|h| h.1);
        (0, n12, adjustr(&assign::<11>(&call_2)))
    } else if w[1][0] == b'<' {
        i3 = 4;
        let n12 = ledger.save(&w[1]).map(|h| h.1);
        (1, n12, adjustr(&assign::<11>(&call_1)))
    } else if i3 == 4 {
        // Neither call is hashed and no CQ: c11 and n12 are never assigned.
        ledger.unassigned = true;
        return Err(Vendored);
    } else {
        // i3 stays -1: pack77 goes on, and a later packer writes over this c77.
        return Ok(None);
    };
    let Some(n12) = n12 else {
        // save_hash_call returned before it set n12.
        ledger.unassigned = true;
        return Err(Vendored);
    };
    let n58 = c11.iter().fold(0i64, |n, &ch| n * 38 + digit(C, ch));
    let mut nrpt = if feq(&w[2], b"RRR") {
        1
    } else if feq(&w[2], b"RR73") {
        2
    } else if feq(&w[2], b"73") {
        3
    } else {
        0
    };
    let iflip = if icq == 1 {
        nrpt = 0;
        0
    } else {
        iflip
    };
    let mut c = C77::new();
    c.b(n12, 12)
        .b(n58, 58)
        .b(iflip, 1)
        .b(nrpt, 2)
        .b(icq, 1)
        .b(i3, 3);
    let mut c77 = c.done();
    // "Clean up any illegal chars".
    for ch in c77.iter_mut().filter(|ch| **ch == b'*') {
        *ch = b'0';
    }
    Ok(Some(c77))
}

/// `pack77_5`: EU VHF contest, "<PA3XYZ> <G4ABC/P> R 590003 IO91NP".
fn pack77_5(nwords: usize, w: &mut [W13; 19], ledger: &mut Ledger) -> R<Option<[u8; 77]>> {
    if !(4..=5).contains(&nwords) || w[0][0] != b'<' || w[1][0] != b'<' {
        return Ok(None);
    }
    let nx = match ld_int(&w[nwords - 2])? {
        Ld::Value(v) => i64::from(v),
        Ld::Unchanged | Ld::Error => -1,
        Ld::End => return Err(Vendored),
    };
    if !(520_001..=594_095).contains(&nx) {
        return Ok(None);
    }
    let g = &w[nwords - 1][..6];
    let is_grid6 = len_trim(g) == 6
        && (b'A'..=b'R').contains(&g[0])
        && (b'A'..=b'R').contains(&g[1])
        && g[2].is_ascii_digit()
        && g[3].is_ascii_digit()
        && (b'A'..=b'X').contains(&g[4])
        && (b'A'..=b'X').contains(&g[5]);
    if !is_grid6 {
        return Ok(None);
    }
    let bare = |word: &W13| -> W13 {
        let i2 = index(word, b'>');
        if i2 >= 2 {
            assign(&word[1..i2 - 1])
        } else {
            [b' '; 13]
        }
    };
    ledger.save(&w[0]);
    let n12 = ihashcall(&bare(&w[0]), 12);
    ledger.save(&w[1]);
    let n22 = ihashcall(&bare(&w[1]), 22);
    let ir = i64::from(&w[2][..2] == b"R ");
    let irpt = nx / 10000 - 52;
    let iserial = (nx % 10000).min(2047);
    let igrid6 = i64::from(g[0] - b'A') * 18 * 10 * 10 * 24 * 24
        + i64::from(g[1] - b'A') * 10 * 10 * 24 * 24
        + i64::from(g[2] - b'0') * 10 * 24 * 24
        + i64::from(g[3] - b'0') * 24 * 24
        + i64::from(g[4] - b'A') * 24
        + i64::from(g[5] - b'A');
    let mut c = C77::new();
    c.b(n12, 12)
        .b(n22, 22)
        .b(ir, 1)
        .b(irpt, 3)
        .b(iserial, 11)
        .b(igrid6, 25)
        .b(5, 3);
    Ok(Some(c.done()))
}

/// `packtext77`: free text, the first 13 characters as a 71-bit base-42 number.
fn free_text(msg: &[u8; 37]) -> [u8; 77] {
    const C: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-./?";
    let w = adjustr(&assign::<13>(&msg[..13]));
    let v = w
        .iter()
        .fold(0u128, |v, &c| v * 42 + digit(C, c).max(0) as u128);
    let mut c77 = [b'0'; 77];
    for (k, ch) in c77[..71].iter_mut().enumerate() {
        if (v >> (70 - k)) & 1 == 1 {
            *ch = b'1';
        }
    }
    c77
}

/// `pack77(msg, i3, n3, c77)` as `genft8` and `genft4` call it (no hint).
fn pack77(msg0: &[u8; 37], ledger: &mut Ledger) -> R<[u8; 77]> {
    let mut msg = *msg0;
    if &msg[..3] == b"$DX" {
        // msg = trim(dxbase)//' '//msg(i1+1:), and nothing in Nexus sets dxbase.
        let i1 = index(&msg, b' ');
        let mut m = [b' '; 37];
        let rest = &msg0[i1..];
        let n = rest.len().min(36);
        m[1..1 + n].copy_from_slice(&rest[..n]);
        msg = m;
    }
    let (nwords, mut w) = split77(&mut msg);
    let w = &mut w;
    let cq_like = &msg[..3] == b"CQ " || &msg[..3] == b"DE " || &msg[..4] == b"QRZ ";
    if !cq_like {
        if let Some(c77) = pack77_01(nwords, w, ledger)? {
            return Ok(c77);
        }
        if let Some(c77) = pack77_03(nwords, w, ledger)? {
            return Ok(c77);
        }
        if nwords < 2 {
            match telemetry(&msg) {
                Telemetry::Packed(c77) => return Ok(c77),
                Telemetry::FreeText => return Ok(free_text(&msg)),
                Telemetry::Next => {}
            }
        }
    }
    // 100:
    for packer in [pack77_06, pack77_1, pack77_3, pack77_4, pack77_5] {
        if let Some(c77) = packer(nwords, w, ledger)? {
            return Ok(c77);
        }
    }
    // 800:
    Ok(free_text(&msg))
}

// ---- The transmit side of unpack77 --------------------------------------------------------------

/// The bits `c77[a..=b]` (1-based, as the Fortran formats count them) as a number.
fn bits(c77: &[u8; 77], a: usize, b: usize) -> i64 {
    c77[a - 1..b]
        .iter()
        .fold(0i64, |v, &c| (v << 1) | i64::from(c == b'1'))
}

/// `to_grid4(n, grid4, ok)`: whether `n` names a 4-character grid.
fn to_grid4_ok(n: i64) -> bool {
    n / (18 * 10 * 10) <= 17
}

/// `nint(idbm*10.0/3.0)` on a default real lands in 0..60.
fn dbm_ok(idbm: i64) -> bool {
    ((idbm as f32 * 10.0 / 3.0).round() as i64) <= 60
}

/// Whether `unpack77(c77, 0, msg, success)` succeeds: the message `genft8` and `genft4` send, as
/// opposed to the one they call bad.
fn unpacks(c77: &[u8; 77], ledger: &Ledger) -> R<bool> {
    if c77.iter().any(|&c| c != b'0' && c != b'1') {
        return Ok(false);
    }
    let n3 = bits(c77, 72, 74);
    let i3 = bits(c77, 75, 77);
    let call = |n28: i64| unpack28(n28, ledger);
    Ok(match (i3, n3) {
        // Free text: blank is a failure, and only an all-zero number decodes to blank.
        (0, 0) => c77[..71].contains(&b'1'),
        (0, 1) => {
            let (n28a, n28b) = (bits(c77, 1, 28), bits(c77, 29, 56));
            call(n28a)?.1 && n28a > 2 && call(n28b)?.1 && n28b > 2
        }
        (0, 3) | (0, 4) => {
            let (n28a, n28b) = (bits(c77, 1, 28), bits(c77, 29, 56));
            let isec = bits(c77, 65, 71);
            let a = call(n28a)?.1 && n28a > 2;
            let b = call(n28b)?.1 && n28b > 2;
            (1..=86).contains(&isec) && a && b
        }
        (0, 5) => true,
        (0, 6) => {
            let (j48, j49, j50) = (bits(c77, 48, 48), bits(c77, 49, 49), bits(c77, 50, 50));
            if j50 == 1 {
                // WSPR type 2.
                let npfx = bits(c77, 29, 44);
                let ok = dbm_ok(bits(c77, 45, 49)) && call(bits(c77, 1, 28))?.1;
                ok && (npfx < NZZZ || npfx - NZZZ <= 12_959)
            } else if j49 == 0 {
                // WSPR type 1.
                dbm_ok(bits(c77, 44, 48))
                    && call(bits(c77, 1, 28))?.1
                    && to_grid4_ok(bits(c77, 29, 43))
            } else if j48 == 0 {
                // WSPR type 3, which pack77 never writes for genft8 or genft4.
                return Err(Vendored);
            } else {
                false
            }
        }
        (0, _) => false,
        (1, _) | (2, _) => {
            let (call_1, ok_1) = call(bits(c77, 1, 28))?;
            let (call_2, ok_2) = call(bits(c77, 30, 57))?;
            let ir = bits(c77, 59, 59);
            let igrid4 = bits(c77, 60, 74);
            // msg begins with call_1, "CQ_xxx" printed as "CQ xxx".
            let cq = call_1 == b"CQ" || call_1.starts_with(b"CQ_");
            let shape_ok = if igrid4 <= MAXGRID4 {
                to_grid4_ok(igrid4) && !(cq && ir == 1)
            } else {
                !(cq && igrid4 - MAXGRID4 >= 2)
            };
            let cq_hash = call_1 == b"CQ" && call_2.first() == Some(&b'<');
            ok_1 && ok_2 && shape_ok && !cq_hash
        }
        (3, _) => call(bits(c77, 2, 29))?.1 && call(bits(c77, 30, 57))?.1,
        (4, _) => true,
        (5, _) => bits(c77, 50, 74) <= 18_662_399,
        _ => false,
    })
}

/// Whether `pack77` reads a local it never assigned on its way through `msg37`: `pack77_3`'s
/// serial or report, `pack77_4`'s calls, `pack77_06`'s prefix. What the vendored encoder then
/// sends is whatever that memory held, not anything the text says: WSJT-X 3.0.2's own `genft8`
/// made `W9XYZ/M K1ABC TNX` a frame that decodes as `<...>` and `W9XYZ K1ABC TNX 73` a bad
/// message, and the copy vendored here made the first `<...>` once and `<...> ZZZZZZZZZ/` the
/// next time. The text has no frame of its own.
pub fn reads_unassigned(msg37: &[u8; 37]) -> bool {
    let mut ledger = Ledger::default();
    pack77(msg37, &mut ledger).is_err() && ledger.unassigned
}

/// `pack77` for `genft8` / `genft4`: the 77 message bits, or `None` where those routines call the
/// message bad (and `genft8` sends all-zero tones, `ft4_encode` refuses).
pub fn pack(msg37: &[u8; 37]) -> R<Option<[u8; 77]>> {
    let mut ledger = Ledger::default();
    let c77 = pack77(msg37, &mut ledger)?;
    Ok(if unpacks(&c77, &ledger)? {
        Some(c77.map(|c| c - b'0'))
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list-directed reads, against what gfortran 13 measured for the same fields.
    #[test]
    fn list_directed_integer_reads_match_gfortran() {
        let value = |v| Ok(Ld::Value(v));
        let cases: &[(&str, R<Ld>)] = &[
            ("+05", value(5)),
            ("-11", value(-11)),
            ("5/", value(5)),
            ("3,4", value(3)),
            ("1/2", value(1)),
            ("-0005", value(-5)),
            ("-2147483648", value(i32::MIN)),
            ("2147483647", value(i32::MAX)),
            (",3", Ok(Ld::Unchanged)),
            ("/12", Ok(Ld::Unchanged)),
            ("+", Ok(Ld::Error)),
            ("+,", Ok(Ld::Error)),
            ("1.", Ok(Ld::Error)),
            ("1E5", Ok(Ld::Error)),
            ("1-2", Ok(Ld::Error)),
            ("12+", Ok(Ld::Error)),
            ("5;", Ok(Ld::Error)),
            ("A", Ok(Ld::Error)),
            ("*5", Ok(Ld::Error)),
            ("2147483648", Ok(Ld::Error)),
            ("9999999999/", Ok(Ld::Error)),
            ("", Ok(Ld::End)),
            ("   ", Ok(Ld::End)),
            ("5*3", Err(Vendored)),
        ];
        for (field, want) in cases {
            assert_eq!(ld_int(field.as_bytes()), *want, "{field:?}");
        }
    }

    /// `Z6` fields of a right-adjusted word, against gfortran 13.
    #[test]
    fn z6_fields_match_gfortran() {
        assert_eq!(z6(b"      "), Some(0));
        assert_eq!(z6(b"    73"), Some(0x73));
        assert_eq!(z6(b"FFFFFF"), Some(0xFF_FFFF));
        assert_eq!(z6(b"    -1"), Some(-1));
        assert_eq!(z6(b"    +1"), Some(1));
        assert_eq!(z6(b"     G"), None);
        assert_eq!(z6(b"    1+"), None);
        assert_eq!(z6(b"     -"), None);
    }

    /// A hashed call this message does not store (too short to store, or the literal `<...>`)
    /// is looked up in whatever the table holds: the vendored encoder's alone. One it stores is
    /// this port's.
    #[test]
    fn a_hash_this_message_does_not_store_is_the_vendored_encoders() {
        let msg = |s: &str| assign::<37>(s.as_bytes());
        assert_eq!(pack(&msg("<AB> FN42 37")), Err(Vendored));
        assert_eq!(pack(&msg("<...> W9XYZ -10")), Err(Vendored));
        assert!(matches!(pack(&msg("<ABC> W9XYZ -10")), Ok(Some(_))));
    }

    /// `Bw.w` writes asterisks for a negative value or one too wide (gfortran 13).
    #[test]
    fn b_format_overflow_writes_asterisks() {
        let write = |v, w| {
            let mut c = C77::new();
            c.b(v, w);
            String::from_utf8(c.0).unwrap()
        };
        assert_eq!(write(32_767, 15), "111111111111111");
        assert_eq!(write(32_935, 15), "***************");
        assert_eq!(write(-1, 3), "***");
        assert_eq!(write(7, 3), "111");
        assert_eq!(write(8, 3), "***");
    }
}
