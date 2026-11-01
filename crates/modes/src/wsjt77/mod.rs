//! WSJT-X's FT8 and FT4 transmit encoder in Rust: the 77-bit packer, CRC-14, LDPC(174,91), the
//! tone maps and the GFSK waveform, a pure function of its input that takes no lock.
//!
//! WHY IT EXISTS. The vendored encoder (`ft8_encode` / `ft4_encode` / `*_gen_wave`) is Fortran
//! with process-global state shared with the decoder, so every call waits on `MODEM_LOCK`, which a
//! decode holds for its whole length. An over built at its key (one that changed after the early
//! pass: a double-click, a new TX frequency or message, the sequencer moving on) therefore went out
//! late by however long the decode overran, and lost its end at the next boundary. WSJT-X's decoder
//! is another process and never delays a key. This builds the same over without the lock.
//!
//! THE SAME OVER, BY CONSTRUCTION AND BY TEST. Each routine is the vendored one ported statement
//! by statement (`packjt77.f90`, `genft8.f90`, `genft4.f90`, `encode174_91.f90`, `crc14.cpp`,
//! `gen_ft8wave.f90`, `gen_ft4wave.f90`); `crates/modes/tests/wsjt77_golden.rs` runs both encoders
//! over every message type and a generated corpus, and any tone that differs fails it. The tones
//! are what goes on the air; the waveform matches to float rounding (see `wave`).
//!
//! WHAT IT DOES NOT DO. `pack77` also writes the callsign tables the decoder reads to print a hashed
//! call (`save_hash_call`, and `unpack77`'s recent-call and hash memos). This port never touches
//! them: the caller hands the message to the vendored encoder after the fact
//! (`ft8::encode_later` / `ft4::encode_later`), which writes exactly what it always did, before the
//! next modem call can read them. And where the Fortran's result is not a function of the text
//! (`pack::Vendored`), these functions answer `None` and the caller uses the vendored encoder.
//!
//! ORIGIN AND LICENCE. Translated into Rust by KD9TAW, 2026-10-14, from WSJT-X, © Joe Taylor
//! (K1JT) and the WSJT Development Group, licensed under the GNU General Public License,
//! version 3: the copies vendored at `libtempo/vendor/wsjtx/` of `lib/ft8/genft8.f90`,
//! `lib/ft4/genft4.f90`, `lib/ft8/encode174_91.f90` and `lib/crc14.cpp`, with the generator table
//! `lib/ft8/ldpc_174_91_c_generator.f90` read as it is (`pack` and `wave` name their own sources).
//! This translation is distributed under the same licence, as part of Nexus (GPL-3.0-only). Its
//! differences from the Fortran are the ones listed above. See NOTICE, "WSJT-X".

pub mod pack;
pub mod wave;

pub use wave::{ft4_wave, ft8_wave};

/// The vendored LDPC(174,91) generator, `ldpc_174_91_c_generator.f90`, read at compile time: 83
/// rows of 91 bits, column `c` (1-based) at bit `91 - c`.
const GENERATOR: [u128; 83] = parse_generator(include_bytes!(
    "../../../../libtempo/vendor/wsjtx/lib/ft8/ldpc_174_91_c_generator.f90"
));

/// Each quoted string of the Fortran `data g/…/` is one row: 23 hex digits, of which the first 91
/// bits are the columns (`encode174_91.f90` reads 4 bits of each digit, 3 of the last).
const fn parse_generator(src: &[u8]) -> [u128; 83] {
    let mut rows = [0u128; 83];
    let mut row = 0;
    let mut i = 0;
    while i < src.len() {
        if src[i] != b'"' {
            i += 1;
            continue;
        }
        let mut v: u128 = 0;
        let mut j = 0;
        while j < 23 {
            let c = src[i + 1 + j];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => panic!("a generator row holds a character that is not a hex digit"),
            };
            v = (v << 4) | d as u128;
            j += 1;
        }
        assert!(src[i + 24] == b'"', "a generator row is not 23 hex digits");
        rows[row] = v >> 1;
        row += 1;
        i += 25;
    }
    assert!(row == 83, "the generator does not have 83 rows");
    rows
}

/// `crc14` (`boost::augmented_crc<14, 0x2757>`) over the 12 bytes `encode174_91` hands it: the
/// 77 message bits, then 19 zero bits.
fn crc14(msg: &[u8; 77]) -> u16 {
    let mut rem: u16 = 0;
    for bit in msg
        .iter()
        .map(|&b| u16::from(b))
        .chain(std::iter::repeat_n(0, 19))
    {
        let top = (rem >> 13) & 1;
        rem = ((rem << 1) | bit) & 0x3FFF;
        if top == 1 {
            rem ^= 0x2757;
        }
    }
    rem
}

/// `encode174_91`: the 91 bits (message and CRC-14), then the 83 parity checks.
fn codeword(msg: &[u8; 77]) -> [u8; 174] {
    let crc = crc14(msg);
    let mut cw = [0u8; 174];
    cw[..77].copy_from_slice(msg);
    for k in 0..14 {
        cw[77 + k] = ((crc >> (13 - k)) & 1) as u8;
    }
    let m = cw[..91]
        .iter()
        .fold(0u128, |v, &b| (v << 1) | u128::from(b));
    for (i, row) in GENERATOR.iter().enumerate() {
        cw[91 + i] = ((row & m).count_ones() & 1) as u8;
    }
    cw
}

/// `genft8` from the 77 bits: three 7x7 Costas arrays and 58 Gray-coded 3-bit symbols.
fn ft8_map(msg: &[u8; 77]) -> Vec<i32> {
    const COSTAS: [i32; 7] = [3, 1, 4, 0, 6, 5, 2];
    const GRAY: [i32; 8] = [0, 1, 3, 2, 5, 6, 4, 7];
    let cw = codeword(msg);
    let mut itone = vec![0i32; 79];
    itone[..7].copy_from_slice(&COSTAS);
    itone[36..43].copy_from_slice(&COSTAS);
    itone[72..].copy_from_slice(&COSTAS);
    for j in 0..58 {
        let k = if j < 29 { 7 + j } else { 14 + j };
        let b = &cw[3 * j..3 * j + 3];
        itone[k] = GRAY[usize::from(b[0] * 4 + b[1] * 2 + b[2])];
    }
    itone
}

/// `genft4` from the 77 bits: scrambled by `rvec`, four 4x4 sync arrays and 87 2-bit symbols.
fn ft4_map(msg: &[u8; 77]) -> Vec<i32> {
    const RVEC: [u8; 77] = [
        0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1,
        0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1,
        0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1,
    ];
    const SYNC: [[i32; 4]; 4] = [[0, 1, 3, 2], [1, 0, 2, 3], [2, 3, 1, 0], [3, 2, 0, 1]];
    let mut scrambled = *msg;
    for (b, r) in scrambled.iter_mut().zip(RVEC) {
        *b ^= r;
    }
    let cw = codeword(&scrambled);
    let data: Vec<i32> = (0..87)
        .map(|i| match 2 * cw[2 * i] + cw[2 * i + 1] {
            2 => 3,
            3 => 2,
            is => i32::from(is),
        })
        .collect();
    let mut itone = Vec::with_capacity(103);
    for (block, sync) in SYNC.iter().enumerate() {
        itone.extend_from_slice(sync);
        if block < 3 {
            itone.extend_from_slice(&data[29 * block..29 * block + 29]);
        }
    }
    itone
}

/// The 37 characters `ft8_encode` / `ft4_encode` copy from a C string: at most 37 bytes, up to
/// the first NUL, blank padded. `None` for a byte outside printable ASCII, which no message Nexus
/// sends holds and whose handling (gfortran's `ichar`, its tab and blank rules) is not ported.
fn c_message(msg: &str) -> Option<[u8; 37]> {
    let mut m = [b' '; 37];
    for (slot, &b) in m
        .iter_mut()
        .zip(msg.as_bytes().iter().take_while(|&&b| b != 0))
    {
        *slot = b;
    }
    m.iter().all(|b| (0x20..0x7F).contains(b)).then_some(m)
}

/// The 79 tones the vendored `ft8_encode` makes of `msg`, all zero where `genft8` calls it a bad
/// message (that entry point refuses nothing: it returns those zeros as 79 tones). `None` where the
/// vendored result is not a function of the text: encode it with `ft8::encode`.
pub fn ft8_tones(msg: &str) -> Option<Vec<i32>> {
    let msg37 = c_message(msg)?;
    match pack::pack(&msg37) {
        Err(pack::Vendored) => None,
        Ok(None) => Some(vec![0; 79]),
        Ok(Some(bits)) => Some(ft8_map(&bits)),
    }
}

/// The 103 tones the vendored `ft4_encode` makes of `msg`, empty where it refuses a bad message.
/// `None` where the vendored result is not a function of the text: encode it with `ft4::encode`.
pub fn ft4_tones(msg: &str) -> Option<Vec<i32>> {
    match pack::pack(&ft4_message(msg)?) {
        Err(pack::Vendored) => None,
        Ok(None) => Some(Vec::new()),
        Ok(Some(bits)) => Some(ft4_map(&bits)),
    }
}

/// The 37 characters `genft4` packs: [`c_message`]'s, its leading blanks stripped.
fn ft4_message(msg: &str) -> Option<[u8; 37]> {
    let mut message = c_message(msg)?;
    // genft4 strips leading blanks with `message=message(i+1:)` on its loop index, so a second
    // blank in a row takes the character after it too. Kept as it is.
    for i in 1..=37 {
        if message[0] != b' ' {
            break;
        }
        let mut next = [b' '; 37];
        next[..37 - i].copy_from_slice(&message[i..]);
        message = next;
    }
    Some(message)
}

/// Why the FT8 / FT4 encoder cannot send a text: Nexus never keys such an over
/// ([`ft8_refusal`], [`ft4_refusal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `genft8` / `genft4` call it a bad message (`*** bad message ***`): FT8's encoder answers
    /// with 79 zero tones, one steady carrier for 12.64 s, and FT4's with none.
    BadMessage,
    /// The packer builds its frame from a field it never set ([`pack::reads_unassigned`]), so
    /// what would go on the air is not decided by the text. WSJT-X 3.0.2 made `W9XYZ K1ABC TNX
    /// 73` a bad message this way (`packjt77.f90` 1353-1357, 1369-1380: the serial is set only
    /// after a 5x9 report, and the report read fails out of the routine with the frame unwritten)
    /// and `W9XYZ/M K1ABC TNX` a frame naming `<...>`, nobody (1412-1456: neither call is in
    /// brackets, so the hash and the call it writes were never set).
    NoFrame,
    /// A character outside printable ASCII, for the modes whose encoder stops the program on a
    /// frame `pack77` did not write ([`q65_refusal`]): this port does not judge such text, and
    /// `W9XYZ K1ABC TNXÉ 73` stops it. WSJT-X's Tx boxes take only `[- @A-Za-z0-9+./?#<>;$]`
    /// (`widgets/mainwindow.cpp`, `message_alphabet`).
    Alphabet,
}

/// Why FT8 cannot send `msg`, or `None` where it can. Where only the vendored encoder can tell
/// (`ft8_tones` is `None`), `Ft8Mode::encode` refuses its bad message by its all-zero tones.
pub fn ft8_refusal(msg: &str) -> Option<Refusal> {
    refusal(&c_message(msg)?)
}

/// [`ft8_refusal`] for FT4, whose encoder strips the text's leading blanks first.
pub fn ft4_refusal(msg: &str) -> Option<Refusal> {
    refusal(&ft4_message(msg)?)
}

fn refusal(msg37: &[u8; 37]) -> Option<Refusal> {
    if pack::reads_unassigned(msg37) {
        return Some(Refusal::NoFrame);
    }
    matches!(pack::pack(msg37), Ok(None)).then_some(Refusal::BadMessage)
}

/// Why Q65 cannot send `msg`, or `None` where it can. `genq65` reads the frame `pack77` wrote with
/// no `err=`, so a text `pack77` writes no frame for stops the whole process the first time
/// (`Bad value during integer read`, `genq65.f90` 29-32) and sends the previous message's frame after
/// that ([`pack::writes_frame`]). Only that is refused: a frame `unpack77` calls bad goes out as
/// it does from WSJT-X, which sends `genq65`'s tones whatever they are. A text starting `@` is
/// `genq65`'s single test tone, never packed (and the entry point refuses it).
pub fn q65_refusal(msg: &str) -> Option<Refusal> {
    if msg.starts_with('@') {
        return None;
    }
    frame_refusal(c_message(msg))
}

/// [`q65_refusal`] for MSK144: `genmsk_128_90` strips leading blanks as `genft4` does, and sends a
/// bracketed pair (`<K1ABC W9XYZ> R-03`) as a 40-bit short message, never packed.
pub fn msk144_refusal(msg: &str) -> Option<Refusal> {
    if msg.starts_with('@') {
        return None;
    }
    let msg37 = ft4_message(msg);
    if let Some(m) = &msg37 {
        // `if(message(1:1).eq.'<')`: `genmsk40` when a blank comes before the first `>`.
        let i2 = m.iter().position(|&b| b == b'>');
        if m[0] == b'<' && i2.is_some_and(|i2| m[..i2].contains(&b' ')) {
            return None;
        }
    }
    frame_refusal(msg37)
}

/// [`q65_refusal`] for FST4 and FST4W: `genfst4` strips leading blanks as `genft4` does. FST4W's
/// hint only adds the `<CALL> GRID6` form, which `pack77` writes either way.
pub fn fst4_refusal(msg: &str) -> Option<Refusal> {
    frame_refusal(ft4_message(msg))
}

fn frame_refusal(msg37: Option<[u8; 37]>) -> Option<Refusal> {
    let Some(msg37) = msg37 else {
        return Some(Refusal::Alphabet);
    };
    (!pack::writes_frame(&msg37)).then_some(Refusal::NoFrame)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generator's first and last rows, as the Fortran spells them.
    #[test]
    fn the_generator_is_read_from_the_vendored_table() {
        assert_eq!(GENERATOR[0], 0x8329ce11bf31eaf509f27fc >> 1);
        assert_eq!(GENERATOR[82], 0x608cc857594bfbb55d69600 >> 1);
    }

    /// ⛔ The texts the encoder cannot send, by what it does with them, and the same answer for
    /// FT8 and FT4. `CQ <K1ABC> FN42` is a bad message to `genft8` (a CQ never carries a hash);
    /// the others the packer would build from fields it never set: an RTTY Roundup serial after a
    /// report that is no 5x9 (`TNX 73`, `73 GL`, `5NN 001`), an RTTY report that is no number, an
    /// unknown Field Day section, two calls with a nonstandard one and neither in brackets, and a
    /// WSPR suffix of four. Controls: the texts the packer carries, as free text, a hashed
    /// message, an RTTY exchange or a WSPR one, and the bracketed call it cannot look up.
    #[test]
    fn the_encoder_refuses_a_bad_message_and_a_text_with_no_frame() {
        use Refusal::{BadMessage, NoFrame};
        for (msg, why) in [
            ("CQ <K1ABC> FN42", BadMessage),
            ("W9XYZ K1ABC TNX 73", NoFrame),
            ("W9XYZ/M K1ABC TNX 73", NoFrame),
            ("W9XYZ K1ABC 73 GL", NoFrame),
            ("W9XYZ K1ABC 5NN 001", NoFrame),
            ("W9XYZ K1ABC TNX 599 001", NoFrame),
            ("W9XYZ K1ABC 2B XYZ", NoFrame),
            ("W9XYZ/M K1ABC TNX", NoFrame),
            ("W9XYZ/M K1ABC/P", NoFrame),
            ("<...> PJ4/K1ABC RR73", NoFrame),
            ("K1ABC/ABCD 37", NoFrame),
        ] {
            assert_eq!(
                (ft8_refusal(msg), ft4_refusal(msg)),
                (Some(why), Some(why)),
                "{msg:?}"
            );
        }
        for msg in [
            "TNX 73",
            "W9XYZ K1ABC TNX",
            "W9XYZ/M <K1ABC> 73",
            "W9XYZ K1ABC 599 001",
            "W9XYZ K1ABC 2B EMA",
            "PJ4/K1ABC 37",
            "<...> W9XYZ -10",
        ] {
            assert_eq!(
                (ft8_refusal(msg), ft4_refusal(msg)),
                (None, None),
                "{msg:?}"
            );
        }
    }

    /// genft4's leading-blank loop, quirk and all: two blanks lose the character after them.
    #[test]
    fn ft4_strips_leading_blanks_as_genft4_does() {
        assert_eq!(ft4_tones("  ACQ K1ABC FN42"), ft4_tones("CQ K1ABC FN42"));
        assert_eq!(ft4_tones(" CQ K1ABC FN42"), ft4_tones("CQ K1ABC FN42"));
        assert_eq!(ft4_tones("   CQ K1ABC FN42"), ft4_tones("CQ K1ABC FN42"));
    }
}
