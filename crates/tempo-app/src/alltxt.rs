//! WSJT-X-compatible ALL.TXT decode-log lines — the running record of every decode (and
//! transmission), in the exact format GridTracker / JTAlert / loggers tail.
//!
//! Format verified against WSJT-X `MainWindow::write_all` + the jt9 decoder line layout:
//! `yyMMdd_hhmmss{%10.3f dialMHz} {Rx|Tx} {mode:<6}{snr:>4}{dt:>5.1}{audioHz:>5} {message}`.
//! WSJT-X strips its `~` sync marker (the `msg.mid(0,15)+msg.mid(18,-1)` surgery) before
//! writing, so ALL.TXT carries no `~`. Example:
//! `231114_221320    14.074 Rx FT8    -10  0.2 1500 CQ W1ABC FN42`
//!
//! A JS8 frame's line names its speed with JS8Call's letter after the audio offset, where
//! JS8Call's own ALL.TXT line has it ([`js8_all_txt_line`]):
//! `260122_235830     7.078 Rx JS8    -11  0.1  645 A KD2UWR: @HB HEARTBEAT FN30`

use tempo_core::logbook::datetime_utc;

/// The UTC second an FT period STARTED, from its slot index and T/R length — the timestamp
/// WSJT-X stamps on every ALL.TXT line.
///
/// ⭐ WHY THIS EXISTS (field report 2026-08-25, RI1FJL on 10.131). All three writers used
/// `now_unix_secs()` — the wall clock at the moment the line was BUFFERED — and WSJT-X uses the
/// period start. Three visible consequences, all in one operator's log, all reported by the
/// station watching the same QSO from his own shack:
///
///  1. `024515 Tx … RI1FJL KD9TAW EN52` and `024515 Rx … KD9TAW RI1FJL -07` on the SAME second.
///     Transmitting and receiving in one period is impossible, and it read on the air as
///     "no idea how that is even possible on the odd cycle". The Rx line was the boundary
///     decode of the audio captured in period 024500; only the label collided.
///  2. Timestamps that never land on a T/R boundary — 023857, 023901, 023911, 023927, 023930,
///     023942 — so the log cannot be lined up against a WSJT-X ALL.TXT at all ("your timing is
///     different, which makes it hard").
///  3. ONE period's decodes split across SEVERAL stamps, because the early pass and the
///     boundary pass finish at different wall-clock moments: the 023857 batch and the 023901
///     stragglers are both period 023845. To someone diffing the two logs, half a period looks
///     missing ("you don't have that decode").
///
/// ⚠️ ROUNDING THE WALL CLOCK IS NOT THE FIX, and that is the trap. `floor(now / 15)` sends a
/// straggler stamped 023901 to period 023900 — the NEXT period, not the 023845 it came from —
/// and a Tx line buffered a second before its boundary (024544) to the PREVIOUS one. The slot
/// index is the only thing that knows which period the audio actually belonged to.
///
/// FT4's 7.5 s period makes alternate starts land on a half second; truncating matches the
/// whole-second `hhmmss` field WSJT-X writes.
pub fn period_start_unix(slot: u64, period_secs: f64) -> u64 {
    (slot as f64 * period_secs) as u64
}

/// One ALL.TXT line for a decode (`is_tx=false`) or a transmission (`is_tx=true`).
/// `dial_mhz` is the VFO dial frequency; `audio_hz` the in-passband carrier offset.
#[allow(clippy::too_many_arguments)]
pub fn all_txt_line(
    unix: u64,
    dial_mhz: f64,
    is_tx: bool,
    mode: &str,
    snr: i32,
    dt: f32,
    audio_hz: f32,
    message: &str,
) -> String {
    let (y, mo, d, h, mi, se) = datetime_utc(unix);
    format!(
        "{:02}{:02}{:02}_{:02}{:02}{:02}{:>10.3} {} {:<6}{:>4}{:>5.1}{:>5} {}",
        (y as u32) % 100,
        mo,
        d,
        h,
        mi,
        se,
        dial_mhz,
        if is_tx { "Tx" } else { "Rx" },
        mode,
        snr,
        dt,
        audio_hz.round() as i32,
        message.trim(),
    )
}

/// Qt's `qRound` on a non-negative `float`, the rounding WSJT-X applies to a JTTY frequency for
/// its columns: half a hertz added in single precision, then truncated.
pub(crate) fn q_round(d: f32) -> i32 {
    if d >= 0.0 {
        (d + 0.5) as i32
    } else {
        (d - 0.5) as i32
    }
}

/// One ALL.TXT line for a finished JTTY message, as WSJT-X 3.3.0-beta4 writes it. WSJT-X
/// logs the message's decode-pane line through the same `MainWindow::write_all` every mode
/// uses, with a JTTY branch (`widgets/mainwindow.cpp` 16191-16264; the pane line is
/// `Jtty::formatJttyDecodeLine`, `widgets/JttyMessages.hpp` 473-485, and the branch reads it
/// back with `Jtty::parseDecodeLine`, 416-437). Described here, not transcribed:
///
/// 1. The pane line: the frequency rounded and right-aligned in 4, a space, the SNR
///    right-aligned in 3, two spaces, the text.
/// 2. The mode column is the mode padded to 6 (`JTTY  `), EXCEPT that `write_all` still looks
///    at character 19 of the line it is given for JT65's `#` and JT9's `@` decode markers,
///    a test older than JTTY. On a JTTY pane line that character is the text's tenth, so a
///    message whose tenth character is `#` (what the encoder sends for a character outside
///    the alphabet) is filed as `JT65  `; `@` is not in the JTTY alphabet. Kept: this file
///    has to read the same as WSJT-X's.
/// 3. The JTTY branch splits the pane line at its first space: the frequency moves to a
///    5-wide column after `   0  0.0` (the DT and SNR columns of the other modes, which JTTY
///    does not fill), and the rest of the line, trimmed (the SNR, two spaces, the text),
///    follows after one space.
/// 4. The stamp is when the message's first frame started (its sequence start), to the
///    second; the dial is the reception's, in MHz to three places.
///
/// Example: `261012_221503    14.090 Rx JTTY     0  0.0 1500 -15  CQ K1ABC CQ`.
pub fn jtty_rx_line(start_ms: i64, dial_hz: u64, freq_hz: f32, snr: i32, text: &str) -> String {
    let pane = format!("{:>4} {:>3}  {}", q_round(freq_hz), snr, text);
    let mode = match pane.chars().nth(19) {
        Some('#') => "JT65  ",
        Some('@') => "JT9   ",
        _ => "JTTY  ",
    };
    let pane = pane.trim();
    let (freq, rest) = pane.split_once(char::is_whitespace).unwrap_or((pane, ""));
    let (y, mo, d, h, mi, se) = datetime_utc(start_ms.div_euclid(1000).max(0) as u64);
    let line = format!(
        "{:02}{:02}{:02}_{:02}{:02}{:02}{:>10.3} Rx {mode}   0  0.0{:>5} {}",
        (y as u32) % 100,
        mo,
        d,
        h,
        mi,
        se,
        dial_hz as f64 / 1e6,
        freq,
        rest.trim(),
    );
    line.trim().to_string()
}

/// One ALL.TXT line for a JTTY segment going out, as WSJT-X writes it when the segment starts on
/// the air (`updateJttyTransmitDisplay` → `write_all("Tx", text)`): the time it started, to the
/// second; the dial; `Tx`; the mode column with the same character-19 test as the `Rx` line, here
/// on the sent text itself; `   0  0.0`; the Tx offset in 5; a space; the text. Filler is never
/// logged (the caller sends none).
///
/// Example: `261013_221503    14.090 Tx JTTY     0  0.0 1500 CQ K1ABC CQ`.
pub fn jtty_tx_line(at_ms: i64, dial_hz: u64, tx_hz: f32, text: &str) -> String {
    let mode = match text.chars().nth(19) {
        Some('#') => "JT65  ",
        Some('@') => "JT9   ",
        _ => "JTTY  ",
    };
    let (y, mo, d, h, mi, se) = datetime_utc(at_ms.div_euclid(1000).max(0) as u64);
    format!(
        "{:02}{:02}{:02}_{:02}{:02}{:02}{:>10.3} Tx {mode}   0  0.0{:>5} {}",
        (y as u32) % 100,
        mo,
        d,
        h,
        mi,
        se,
        dial_hz as f64 / 1e6,
        q_round(tx_hz),
        text,
    )
}

/// One ALL.TXT line for a JS8 frame heard (`is_tx=false`) or keyed: [`all_txt_line`]'s, mode
/// `JS8`, with the letter JS8Call 3.0.3 writes for the frame's `speed` (`submodeChar`,
/// DecodedText.cpp:33-48: Normal A, Fast B, JS8 40 C, Slow E, JS8 60 I) after the audio offset,
/// where JS8Call's own line has it (`DecodedText::string`, :323-342). JS8Call's line for its own
/// transmission carries no letter (`write_transmit_entry`, mainwindow.cpp:6994-7019); a Tx line
/// here names its speed as an Rx line does.
#[allow(clippy::too_many_arguments)]
pub fn js8_all_txt_line(
    unix: u64,
    dial_mhz: f64,
    is_tx: bool,
    speed: modes::Js8Speed,
    snr: i32,
    dt: f32,
    audio_hz: f32,
    message: &str,
) -> String {
    let message = format!("{} {}", speed.letter(), message.trim());
    all_txt_line(unix, dial_mhz, is_tx, "JS8", snr, dt, audio_hz, &message)
}

/// One decode's line: a JS8 frame's with its speed's letter ([`js8_all_txt_line`]), any other
/// mode's as [`all_txt_line`] writes it under `mode`.
pub fn decode_line(unix: u64, dial_mhz: f64, mode: &str, d: &modes::Decode) -> String {
    match d.mode {
        Some(modes::ModeKind::Js8 { speed }) => js8_all_txt_line(
            unix, dial_mhz, false, speed, d.snr, d.dt, d.freq, &d.message,
        ),
        _ => all_txt_line(unix, dial_mhz, false, mode, d.snr, d.dt, d.freq, &d.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JTTY segment's ALL.TXT line, byte for byte as WSJT-X's `write_all("Tx", …)` builds it:
    /// `   0  0.0`, the Tx offset in five, a space, the text as sent.
    #[test]
    fn a_jtty_tx_line_is_wsjtxs_byte_for_byte() {
        let at = T as i64 * 1000 + 999;
        assert_eq!(
            jtty_tx_line(at, 14_090_000, 1500.0, "CQ K1ABC CQ"),
            "231114_221320    14.090 Tx JTTY     0  0.0 1500 CQ K1ABC CQ"
        );
        assert_eq!(
            jtty_tx_line(at, 7_078_000, 2345.4, "W1AW 599 001"),
            "231114_221320     7.078 Tx JTTY     0  0.0 2345 W1AW 599 001"
        );
        // The marker test reads the sent text's twentieth character.
        assert_eq!(
            jtty_tx_line(at, 14_090_000, 1500.0, "ABCDEFGHIJKLMNOPQRS#T"),
            "231114_221320    14.090 Tx JT65     0  0.0 1500 ABCDEFGHIJKLMNOPQRS#T"
        );
    }

    // unix 1_700_000_000 == 2023-11-14 22:13:20 UTC → "231114_221320".
    const T: u64 = 1_700_000_000;

    /// A JTTY decode's ALL.TXT line, byte for byte as WSJT-X's `write_all` writes it from the
    /// decode-pane line (see [`jtty_rx_line`]): the first frame's start to the second (the
    /// 0.75 s does not round up), the dial to three places, `JTTY` padded to six, the empty DT
    /// and SNR columns, the frequency in five, then the pane line's SNR and two spaces.
    #[test]
    fn a_jtty_rx_line_is_wsjtxs_byte_for_byte() {
        let at = T as i64 * 1000 + 750;
        assert_eq!(
            jtty_rx_line(at, 14_090_000, 1500.2, -15, "CQ K1ABC CQ"),
            "231114_221320    14.090 Rx JTTY     0  0.0 1500 -15  CQ K1ABC CQ"
        );
        // A positive SNR is not signed, a one-digit one is not padded once the pane line is
        // split (the trim takes the padding), and a low dial pads to ten.
        assert_eq!(
            jtty_rx_line(at, 7_078_000, 987.5, 5, "K1ABC 599 0123"),
            "231114_221320     7.078 Rx JTTY     0  0.0  988 5  K1ABC 599 0123"
        );
        assert_eq!(
            jtty_rx_line(at, 144_160_000, 2501.0, 12, "TU"),
            "231114_221320   144.160 Rx JTTY     0  0.0 2501 12  TU"
        );
        // A long message is logged whole, never wrapped as the pane wraps it.
        let long = "HELLO FROM THE SHACK IN EN52 RUNNING 100 W TO A VERTICAL ANTENNA";
        assert_eq!(
            jtty_rx_line(at, 14_090_000, 1500.0, -3, long),
            format!("231114_221320    14.090 Rx JTTY     0  0.0 1500 -3  {long}")
        );
    }

    /// WSJT-X's mode column tests character 19 of the pane line for the JT65 and JT9 decode
    /// markers, so a JTTY text whose tenth character is `#` is filed as JT65 (see
    /// [`jtty_rx_line`]). The control: the same `#` one place either side is not.
    #[test]
    fn a_jtty_rx_line_keeps_wsjtxs_marker_column() {
        let at = T as i64 * 1000;
        assert_eq!(
            jtty_rx_line(at, 14_090_000, 1500.0, -9, "UR RST 59#"),
            "231114_221320    14.090 Rx JT65     0  0.0 1500 -9  UR RST 59#"
        );
        for text in ["UR RST 5#9", "UR RST 599#"] {
            assert_eq!(
                jtty_rx_line(at, 14_090_000, 1500.0, -9, text),
                format!("231114_221320    14.090 Rx JTTY     0  0.0 1500 -9  {text}")
            );
        }
    }

    #[test]
    fn q_round_is_qts_rounding_of_a_frequency() {
        assert_eq!(q_round(1499.5), 1500);
        assert_eq!(q_round(1499.49), 1499);
        assert_eq!(q_round(1499.99), 1500);
        assert_eq!(q_round(0.0), 0);
    }

    #[test]
    fn rx_line_matches_wsjtx_layout_exactly() {
        let line = all_txt_line(T, 14.074, false, "FT8", -10, 0.2, 1500.0, "CQ W1ABC FN42");
        assert_eq!(
            line,
            "231114_221320    14.074 Rx FT8    -10  0.2 1500 CQ W1ABC FN42"
        );
    }

    #[test]
    fn tx_line_uses_tx_marker_and_zeroed_metrics() {
        let line = all_txt_line(T, 14.074, true, "FT8", 0, 0.0, 1200.0, "W1ABC KD9TAW R-09");
        assert_eq!(
            line,
            "231114_221320    14.074 Tx FT8      0  0.0 1200 W1ABC KD9TAW R-09"
        );
    }

    /// A decode's line: any other mode's as WSJT-X writes it, a JS8 frame's with its speed's
    /// letter between the offset and the message; a keyed JS8 frame's too.
    #[test]
    fn a_js8_line_carries_its_speeds_letter_and_no_other_line_changes() {
        use modes::Js8Speed;
        let decode = |mode| modes::Decode {
            message: "CQ W1ABC FN42".into(),
            sync: 5.0,
            snr: -10,
            dt: 0.2,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode,
            raw: None,
        };
        for mode in [Some(modes::ModeKind::Ft8), None] {
            assert_eq!(
                decode_line(T, 14.074, "FT8", &decode(mode)),
                "231114_221320    14.074 Rx FT8    -10  0.2 1500 CQ W1ABC FN42"
            );
        }
        for (speed, letter) in [
            (Js8Speed::Slow, 'E'),
            (Js8Speed::Normal, 'A'),
            (Js8Speed::Fast, 'B'),
            (Js8Speed::Turbo, 'C'),
            (Js8Speed::Ultra, 'I'),
        ] {
            assert_eq!(
                decode_line(
                    T,
                    14.078,
                    "JS8",
                    &decode(Some(modes::ModeKind::Js8 { speed }))
                ),
                format!("231114_221320    14.078 Rx JS8    -10  0.2 1500 {letter} CQ W1ABC FN42")
            );
        }
        assert_eq!(
            js8_all_txt_line(
                T,
                14.078,
                true,
                Js8Speed::Fast,
                0,
                0.0,
                1500.0,
                "KD9TAW: HI "
            ),
            "231114_221320    14.078 Tx JS8      0  0.0 1500 B KD9TAW: HI"
        );
    }

    #[test]
    fn positive_snr_and_ft4_and_message_trim() {
        let line = all_txt_line(T, 7.074, false, "FT4", 12, -0.3, 800.0, "  CQ DX VK3ABC  ");
        assert_eq!(
            line,
            "231114_221320     7.074 Rx FT4     12 -0.3  800 CQ DX VK3ABC"
        );
    }

    // ── period stamping (the 2026-08-25 ALL.TXT timing report) ──

    /// THE REPORTED SYMPTOM, as an assertion: a Tx and an Rx that are handled in the same wall
    /// clock second must land on DIFFERENT periods, because they are.
    ///
    /// At the boundary that starts slot N the app does two things at once: it keys the over FOR
    /// slot N, and it decodes the audio captured during slot N-1. Stamped with the wall clock
    /// both lines read the same second — which is what put `024515 Tx` and `024515 Rx` side by
    /// side in the operator's log and made the QSO unreadable to the station following it.
    #[test]
    fn a_tx_and_the_boundary_decode_beside_it_are_one_period_apart() {
        // Slot N: 024515 == unix 1787617515 (FT8, 15 s periods).
        const N: u64 = 1787617515 / 15;
        let tx = period_start_unix(N, 15.0);
        let rx = period_start_unix(N - 1, 15.0);
        assert_eq!(
            tx, 1787617515,
            "the over is stamped with the period it keys in"
        );
        assert_eq!(
            rx, 1787617500,
            "its boundary decode is the PREVIOUS period's audio"
        );
        assert_eq!(
            tx - rx,
            15,
            "exactly one T/R period apart, never the same second"
        );
    }

    /// Every stamp lands on a T/R boundary — the property that lets the file be diffed against
    /// a WSJT-X ALL.TXT at all. The operator's file had 023857 / 023901 / 023911 / 023927 /
    /// 023930, none of which are on one.
    #[test]
    fn every_period_start_lands_on_a_boundary() {
        for slot in 0..400u64 {
            assert_eq!(period_start_unix(slot, 15.0) % 15, 0, "FT8 slot {slot}");
        }
        // FT4's 7.5 s period alternates whole/half seconds; the half truncates, as WSJT-X's
        // whole-second field does.
        assert_eq!(period_start_unix(2, 7.5), 15);
        assert_eq!(period_start_unix(3, 7.5), 22); // 22.5 → 22
    }

    /// BOTH PASSES OF ONE PERIOD GET ONE STAMP. The early pass and the boundary pass finish
    /// seconds apart and are dispatched with different slot indices (audio slot + 1 for both,
    /// by construction) — so they must resolve to the SAME period, or one period's decodes
    /// scatter across the file and read as missing.
    #[test]
    fn the_early_and_boundary_passes_of_one_period_share_a_stamp() {
        // Audio period 023845; both passes carry index = audio slot + 1.
        const AUDIO: u64 = 1787617425 / 15; // 023845
        let early = period_start_unix((AUDIO + 1) - 1, 15.0);
        let boundary = period_start_unix((AUDIO + 1) - 1, 15.0);
        assert_eq!(early, boundary);
        assert_eq!(early, 1787617425);
    }

    /// THE POSITIVE CONTROL, and the reason this is not done by rounding the clock: flooring a
    /// wall clock puts a late straggler in the wrong period and a slightly-early Tx line in the
    /// previous one. The slot index gets both right.
    #[test]
    fn rounding_the_wall_clock_would_get_both_edges_wrong() {
        const AUDIO_START: u64 = 1787617425; // 023845
        let slot = AUDIO_START / 15;
        // A straggler observed at 023901 — 16 s after the period began.
        let observed_late = AUDIO_START + 16;
        assert_eq!(
            observed_late - observed_late % 15,
            AUDIO_START + 15,
            "control: flooring the clock lands a straggler in the NEXT period"
        );
        assert_eq!(
            period_start_unix(slot, 15.0),
            AUDIO_START,
            "the slot index does not"
        );
        // A Tx line buffered one second BEFORE its own boundary (the operator's 024544).
        let tx_slot = (AUDIO_START + 15) / 15;
        let observed_early = AUDIO_START + 14;
        assert_eq!(
            observed_early - observed_early % 15,
            AUDIO_START,
            "control: flooring the clock lands it in the PREVIOUS period"
        );
        assert_eq!(period_start_unix(tx_slot, 15.0), AUDIO_START + 15);
    }
}
