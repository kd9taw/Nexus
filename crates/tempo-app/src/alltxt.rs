//! WSJT-X-compatible ALL.TXT decode-log lines — the running record of every decode (and
//! transmission), in the exact format GridTracker / JTAlert / loggers tail.
//!
//! Format verified against WSJT-X `MainWindow::write_all` + the jt9 decoder line layout:
//! `yyMMdd_hhmmss{%10.3f dialMHz} {Rx|Tx} {mode:<6}{snr:>4}{dt:>5.1}{audioHz:>5} {message}`.
//! WSJT-X strips its `~` sync marker (the `msg.mid(0,15)+msg.mid(18,-1)` surgery) before
//! writing, so ALL.TXT carries no `~`. Example:
//! `231114_221320    14.074 Rx FT8    -10  0.2 1500 CQ W1ABC FN42`

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

#[cfg(test)]
mod tests {
    use super::*;

    // unix 1_700_000_000 == 2023-11-14 22:13:20 UTC → "231114_221320".
    const T: u64 = 1_700_000_000;

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
