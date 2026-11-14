//! ITA2 (Baudot–Murray) 5-bit codec — the character layer of RTTY.
//!
//! Two shift planes (letters/figures) selected by inline LTRS/FIGS codes;
//! US-TTY figures conventions (BEL at FIGS-S, `$` at FIGS-D, `#` at FIGS-H) —
//! the tables fldigi/MMTTY use and operators expect on the air. Encode and
//! decode are separate little state machines so the TX and RX shift states can
//! never entangle.
//!
//! USOS (unshift-on-space) is configurable on both sides, default ON:
//! * decode — a space resets to letters, the convention that rescues a
//!   lost-LTRS garble at the next word boundary;
//! * encode — the encoder mirrors the receiver's unshift and, when leaving
//!   figures, sends the explicit LTRS too so a non-USOS receiver stays in sync
//!   (fldigi transmits the same). With USOS off the shift holds across spaces
//!   (contest "599 599" style).
//!
//! Diddle: an idle transmitter sends LTRS ([`BaudotEncoder::diddle`]) instead
//! of dead carrier; shift codes never print on decode, so idle fill is
//! invisible by construction.
//!
//! Preamble: every transmission opens on [`PREAMBLE_LTRS`] LTRS codes
//! ([`BaudotEncoder::preamble`]), so a far decoder left in figures by whatever
//! it copied last prints our first characters as letters. fldigi does the same
//! (`rtty::tx_process`, `progdefaults.TTY_LTRS`, default 2).

/// 5-bit codes with fixed meaning in both shift planes.
pub const NUL: u8 = 0x00;
pub const LF: u8 = 0x02;
pub const SPACE: u8 = 0x04;
pub const CR: u8 = 0x08;
/// Shift to the figures plane.
pub const FIGS: u8 = 0x1B;
/// Shift to the letters plane; also the idle ("diddle") fill code.
pub const LTRS: u8 = 0x1F;

/// LTRS codes at the head of every transmission — fldigi's count: its
/// `rtty::tx_process` sends `progdefaults.TTY_LTRS` of them when a transmission
/// starts, and `TTY_LTRS` defaults to 2 (`configuration.h`, "0 to 10, number of
/// LTRS bytes send at beginning of xmt"). Matched, so a far station hears the
/// same opening from us as from fldigi.
pub const PREAMBLE_LTRS: usize = 2;

/// Letters plane, indexed by 5-bit code. `None` = NUL and the shift codes
/// (handled by the decoder, never printed).
const LETTERS: [Option<char>; 32] = [
    None, // 0x00 NUL
    Some('E'),
    Some('\n'),
    Some('A'),
    Some(' '),
    Some('S'),
    Some('I'),
    Some('U'),
    Some('\r'),
    Some('D'),
    Some('R'),
    Some('J'),
    Some('N'),
    Some('F'),
    Some('C'),
    Some('K'),
    Some('T'),
    Some('Z'),
    Some('L'),
    Some('W'),
    Some('H'),
    Some('Y'),
    Some('P'),
    Some('Q'),
    Some('O'),
    Some('B'),
    Some('G'),
    None, // 0x1B FIGS
    Some('M'),
    Some('X'),
    Some('V'),
    None, // 0x1F LTRS
];

/// Figures plane, US-TTY conventions (BEL/`$`/`#`/`&` where ITA2 proper has
/// apostrophe/WRU/£/undefined).
const FIGURES: [Option<char>; 32] = [
    None, // 0x00 NUL
    Some('3'),
    Some('\n'),
    Some('-'),
    Some(' '),
    Some('\u{7}'), // BEL
    Some('8'),
    Some('7'),
    Some('\r'),
    Some('$'),
    Some('4'),
    Some('\''),
    Some(','),
    Some('!'),
    Some(':'),
    Some('('),
    Some('5'),
    Some('"'),
    Some(')'),
    Some('2'),
    Some('#'),
    Some('6'),
    Some('0'),
    Some('1'),
    Some('9'),
    Some('?'),
    Some('&'),
    None, // 0x1B FIGS
    Some('.'),
    Some('/'),
    Some(';'),
    None, // 0x1F LTRS
];

/// Which plane the next code will be read/written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Letters,
    Figures,
}

/// Whether `c` (case-insensitive) has an ITA2 mapping — the TX charset filter.
/// Unmapped characters are silently dropped by [`BaudotEncoder::push_char`]; this
/// lets callers strip them up front so the operator sees exactly what will key.
pub fn encodable(c: char) -> bool {
    let c = c.to_ascii_uppercase();
    LETTERS.contains(&Some(c)) || FIGURES.contains(&Some(c))
}

/// Flatten encoded 5-bit codes to the over-the-air data-bit stream: 5 bits per
/// code, LSB first (ITA2 transmits the low bit first). This is the `data_bits`
/// input both TX paths frame (AFSK sample rendering / FSK keying schedule).
pub fn code_bits(codes: &[u8]) -> Vec<bool> {
    let mut out = Vec::with_capacity(codes.len() * 5);
    for &code in codes {
        for i in 0..5 {
            out.push(code >> i & 1 == 1);
        }
    }
    out
}

/// RX side: 5-bit codes in → printable characters out.
#[derive(Debug, Clone)]
pub struct BaudotDecoder {
    shift: Shift,
    usos: bool,
}

impl BaudotDecoder {
    pub fn new(usos: bool) -> Self {
        Self {
            shift: Shift::Letters,
            usos,
        }
    }

    pub fn shift(&self) -> Shift {
        self.shift
    }

    /// Back to the letters plane (start of a new over).
    pub fn reset(&mut self) {
        self.shift = Shift::Letters;
    }

    /// Decode one 5-bit code. Shift codes and NUL return `None` — which is
    /// exactly what keeps TX diddle idle (LTRS fill) invisible. A space
    /// optionally unshifts (USOS).
    pub fn decode(&mut self, code: u8) -> Option<char> {
        let code = code & 0x1F;
        match code {
            LTRS => {
                self.shift = Shift::Letters;
                None
            }
            FIGS => {
                self.shift = Shift::Figures;
                None
            }
            SPACE => {
                if self.usos {
                    self.shift = Shift::Letters;
                }
                Some(' ')
            }
            _ => match self.shift {
                Shift::Letters => LETTERS[code as usize],
                Shift::Figures => FIGURES[code as usize],
            },
        }
    }
}

/// TX side: characters in → 5-bit codes out, shift codes inserted as needed.
#[derive(Debug, Clone)]
pub struct BaudotEncoder {
    shift: Shift,
    usos: bool,
}

impl BaudotEncoder {
    pub fn new(usos: bool) -> Self {
        Self {
            shift: Shift::Letters,
            usos,
        }
    }

    pub fn shift(&self) -> Shift {
        self.shift
    }

    pub fn reset(&mut self) {
        self.shift = Shift::Letters;
    }

    /// The idle ("diddle") code — LTRS, which also lands both ends in the
    /// letters plane.
    pub fn diddle(&mut self) -> u8 {
        self.shift = Shift::Letters;
        LTRS
    }

    /// The start-of-transmission preamble: [`PREAMBLE_LTRS`] LTRS codes, which
    /// land the far decoder in letters whatever plane it was left in — and this
    /// encoder with it, so the text that follows emits no LTRS of its own.
    pub fn preamble(&mut self, out: &mut Vec<u8>) {
        out.extend([LTRS; PREAMBLE_LTRS]);
        self.shift = Shift::Letters;
    }

    /// Encode a whole transmission: the [`Self::preamble`], then `s`.
    pub fn encode_over(&mut self, s: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(PREAMBLE_LTRS + s.len() + 8);
        self.preamble(&mut out);
        for c in s.chars() {
            self.push_char(c, &mut out);
        }
        out
    }

    /// Append the code(s) for `c` (upper-cased; chars with no ITA2 mapping are
    /// dropped, as fldigi drops them), inserting shift codes as needed.
    pub fn push_char(&mut self, c: char, out: &mut Vec<u8>) {
        let c = c.to_ascii_uppercase();
        if c == ' ' {
            // USOS: the receiver will unshift after this space, so mirror it —
            // and when leaving figures send the explicit LTRS too, so a
            // non-USOS receiver stays in sync.
            if self.usos {
                if self.shift == Shift::Figures {
                    out.push(LTRS);
                }
                self.shift = Shift::Letters;
            }
            out.push(SPACE);
            return;
        }
        let ltr = LETTERS.iter().position(|&x| x == Some(c));
        let fig = FIGURES.iter().position(|&x| x == Some(c));
        let (code, want) = match (ltr, fig) {
            (Some(i), Some(_)) => (i as u8, None), // CR/LF live in both planes
            (Some(i), None) => (i as u8, Some(Shift::Letters)),
            (None, Some(i)) => (i as u8, Some(Shift::Figures)),
            (None, None) => return,
        };
        if let Some(want) = want {
            if self.shift != want {
                out.push(if want == Shift::Letters { LTRS } else { FIGS });
                self.shift = want;
            }
        }
        out.push(code);
    }

    /// Encode a whole string.
    pub fn encode(&mut self, s: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(s.len() + 8);
        for c in s.chars() {
            self.push_char(c, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(dec: &mut BaudotDecoder, codes: &[u8]) -> String {
        codes.iter().filter_map(|&c| dec.decode(c)).collect()
    }

    fn round_trip(text: &str, usos: bool) -> String {
        let codes = BaudotEncoder::new(usos).encode(text);
        decode_all(&mut BaudotDecoder::new(usos), &codes)
    }

    #[test]
    fn encodes_plain_letters_without_shifts() {
        let codes = BaudotEncoder::new(true).encode("ABC");
        assert_eq!(codes, vec![0x03, 0x19, 0x0E]);
    }

    #[test]
    fn round_trips_callsign_exchange() {
        let t = "CQ CQ DE KD9TAW KD9TAW PSE K";
        assert_eq!(round_trip(t, true), t);
        assert_eq!(round_trip(t, false), t);
    }

    #[test]
    fn round_trips_mixed_figures_and_punctuation() {
        // RST + digits + US punctuation across many shift transitions.
        let t = "UR RST 599 599 QTH DAYTON, OH. HW? BK";
        assert_eq!(round_trip(t, true), t);
        assert_eq!(round_trip(t, false), t);
        let t2 = "WX -12; $4 (50%)"; // '%' has no ITA2 mapping → dropped
        assert_eq!(round_trip(t2, true), "WX -12; $4 (50)");
    }

    #[test]
    fn lowercase_is_uppercased() {
        assert_eq!(round_trip("cq de kd9taw", true), "CQ DE KD9TAW");
    }

    #[test]
    fn cr_lf_need_no_shift() {
        let codes = BaudotEncoder::new(true).encode("A\r\nB");
        assert_eq!(codes, vec![0x03, CR, LF, 0x19]); // no LTRS/FIGS inserted
        assert_eq!(round_trip("A\r\nB", true), "A\r\nB");
    }

    /// CR LF never moves the shift, on either side — fldigi's convention (`rtty.cxx`: its
    /// `baudot_dec` changes plane only on LTRS, FIGS and, with UOSrx, a space; its
    /// `tx_process` sends `\r`/`\n` as bare 0x08/0x02 with no LTRS). So a line break between
    /// two figures groups costs no shift code, and every receiver that follows that
    /// convention — ours included, USOS or not — still prints the second group as figures.
    #[test]
    fn cr_lf_keep_the_figures_plane_like_fldigi() {
        let mut enc = BaudotEncoder::new(true);
        let codes = enc.encode("599\r\n599");
        assert_eq!(
            codes,
            vec![FIGS, 0x10, 0x18, 0x18, CR, LF, 0x10, 0x18, 0x18],
            "one FIGS, then CR LF as bare codes, and no re-shift after them"
        );
        assert_eq!(enc.shift(), Shift::Figures);
        for usos in [true, false] {
            assert_eq!(
                decode_all(&mut BaudotDecoder::new(usos), &codes),
                "599\r\n599",
                "usos={usos}"
            );
        }
        // An over that opens on a line break starts in letters and stays there.
        let codes = BaudotEncoder::new(true).encode("\r\nCQ");
        assert_eq!(codes, vec![CR, LF, 0x0E, 0x17]);
    }

    /// ⭐ EVERY OVER OPENS ON fldigi's LTRS PREAMBLE. A far decoder left in figures (the
    /// last over it copied ended on a report, or a burst of noise read as FIGS) would print
    /// the start of ours as figures. fldigi's `rtty::tx_process` sends
    /// `progdefaults.TTY_LTRS` LTRS codes when a transmission starts — default 2 — and so do
    /// we, ahead of everything else, the over's CR LF included.
    #[test]
    fn an_over_opens_on_the_ltrs_preamble_and_then_needs_no_ltrs() {
        assert_eq!(PREAMBLE_LTRS, 2, "fldigi's TTY_LTRS default");
        for usos in [true, false] {
            // Letters text: the preamble, CR LF, then the letters with no third LTRS — the
            // preamble already put both ends in letters.
            let mut enc = BaudotEncoder::new(usos);
            assert_eq!(
                enc.encode_over("\r\nCQ"),
                vec![LTRS, LTRS, CR, LF, 0x0E, 0x17],
                "usos={usos}"
            );
            // Figures text still shifts once, after the preamble.
            let mut enc = BaudotEncoder::new(usos);
            assert_eq!(
                enc.encode_over("5"),
                vec![LTRS, LTRS, FIGS, 0x10],
                "usos={usos}"
            );
            assert_eq!(enc.shift(), Shift::Figures, "usos={usos}");
            // An encoder still in figures from an earlier over lands in letters on the
            // preamble — and the letters after it need no LTRS of their own.
            let mut out = Vec::new();
            enc.preamble(&mut out);
            assert_eq!(out, vec![LTRS, LTRS], "usos={usos}");
            assert_eq!(enc.shift(), Shift::Letters, "usos={usos}");
            assert_eq!(enc.encode("E"), vec![0x01], "usos={usos}");
            // A decoder left in figures prints the over as sent.
            let mut dec = BaudotDecoder::new(usos);
            dec.decode(FIGS);
            let codes = BaudotEncoder::new(usos).encode_over("RY 599");
            assert_eq!(decode_all(&mut dec, &codes), "RY 599", "usos={usos}");
        }
    }

    #[test]
    fn diddle_idle_never_prints() {
        let mut enc = BaudotEncoder::new(true);
        let mut codes = vec![enc.diddle(), enc.diddle()];
        for c in "TEST 599".chars() {
            enc.push_char(c, &mut codes);
            codes.push(LTRS); // heavy diddle fill between characters
        }
        // Interspersed LTRS resets the plane, so the encoder's own state must
        // be wrong here — decode with a fresh decoder per the raw stream.
        let mut dec = BaudotDecoder::new(true);
        let got = decode_all(&mut dec, &codes);
        // The FIGS shift before '5' survives because push_char re-emits it
        // whenever its state says letters; the diddles between digits break
        // that, so encode digit-by-digit fills would garble — this test pins
        // the DECODER contract: LTRS fill prints nothing.
        assert!(!got.contains('\u{1f}'));
        assert!(got.starts_with("TEST "));
    }

    #[test]
    fn us_figures_conventions() {
        // BEL at FIGS-S, '$' at FIGS-D, '#' at FIGS-H.
        let mut dec = BaudotDecoder::new(true);
        assert_eq!(decode_all(&mut dec, &[FIGS, 0x05, 0x09, 0x14]), "\u{7}$#");
        assert_eq!(round_trip("$5 #1 W!", true), "$5 #1 W!");
    }

    #[test]
    fn usos_mismatch_garbles_the_classic_way() {
        // TX without USOS holds FIGS across the space; an USOS receiver
        // unshifts anyway → the second group reads in letters: 599 → TOO.
        let codes = BaudotEncoder::new(false).encode("599 599");
        let got = decode_all(&mut BaudotDecoder::new(true), &codes);
        assert_eq!(got, "599 TOO");
        // Matched non-USOS ends copy it clean.
        let got = decode_all(&mut BaudotDecoder::new(false), &codes);
        assert_eq!(got, "599 599");
    }

    #[test]
    fn usos_tx_stays_copyable_by_non_usos_rx() {
        // The encoder's explicit LTRS-before-space + FIGS-resend keeps a
        // non-USOS receiver in sync in both directions.
        let codes = BaudotEncoder::new(true).encode("59 61 A1");
        assert_eq!(
            decode_all(&mut BaudotDecoder::new(false), &codes),
            "59 61 A1"
        );
        assert_eq!(
            decode_all(&mut BaudotDecoder::new(true), &codes),
            "59 61 A1"
        );
    }

    #[test]
    fn lost_figs_garbles_until_usos_space_resyncs() {
        // Drop the FIGS shift from "599 A" — the digits garble to letters
        // (TOO) but the USOS space resyncs the stream for what follows.
        let mut codes = BaudotEncoder::new(true).encode("599 A");
        assert_eq!(codes.remove(0), FIGS);
        let got = decode_all(&mut BaudotDecoder::new(true), &codes);
        assert_eq!(got, "TOO A");
    }

    #[test]
    fn encodable_matches_what_the_encoder_keeps() {
        for c in "ABCXYZ abcz0159-?:$#!&()\"'./;,\r\n".chars() {
            assert!(encodable(c), "{c:?} should be encodable");
        }
        for c in "%*+=@[]_~{}|^<>".chars() {
            assert!(!encodable(c), "{c:?} has no ITA2 mapping");
        }
        // The filter and the encoder agree: a filtered string round-trips verbatim.
        let kept: String = "UR 599 (50%) OK*"
            .chars()
            .filter(|&c| encodable(c))
            .collect();
        assert_eq!(kept, "UR 599 (50) OK");
        let codes = BaudotEncoder::new(true).encode(&kept);
        assert_eq!(decode_all(&mut BaudotDecoder::new(true), &codes), kept);
    }

    #[test]
    fn code_bits_are_lsb_first_five_per_code() {
        // 'A' = 0x03 → 1,1,0,0,0 ; 'E' = 0x01 → 1,0,0,0,0.
        assert_eq!(
            code_bits(&[0x03, 0x01]),
            vec![true, true, false, false, false, true, false, false, false, false]
        );
        assert!(code_bits(&[]).is_empty());
    }

    #[test]
    fn nul_prints_nothing_and_state_survives() {
        let mut dec = BaudotDecoder::new(true);
        assert_eq!(dec.decode(NUL), None);
        assert_eq!(dec.decode(FIGS), None);
        assert_eq!(dec.decode(NUL), None);
        assert_eq!(dec.decode(0x10), Some('5')); // still in figures
    }

    #[test]
    fn decoder_reset_returns_to_letters() {
        let mut dec = BaudotDecoder::new(false);
        dec.decode(FIGS);
        assert_eq!(dec.shift(), Shift::Figures);
        dec.reset();
        assert_eq!(dec.decode(0x10), Some('T'));
    }
}
