//! Tempo's proposed calling-frequency band plan.
//!
//! Tempo is a NEW narrow weak-signal text mode (FT1 ~150 Hz, DX1 ~50 Hz), so it
//! must **not** sit on the established FT8 / FT4 / JS8 / WSPR / PSK watering holes
//! (mutual QRM), and it must stay clear of CW activity and the VHF/UHF FM calling
//! / satellite / repeater segments.
//!
//! Every entry here was chosen so that — for a USB signal with the usual ~1500 Hz
//! audio offset, i.e. an emission ~1.5 kHz above the dial — the **emission falls
//! inside the US General-class data privileges** (General has the HF data
//! sub-bands and full privileges on 160 m / 6 m and band-wide data above 50 MHz),
//! and sits clear of the CW calling frequencies. These are **proposed, editable
//! defaults** to coordinate with the community — the operator can override any
//! frequency manually.
//!
//! HF placement = "upper shoulder of the digital cluster" (a few kHz above
//! FT8/JS8/FT4, below WSPR). VHF/UHF = a USB weak-signal calling freq and, where
//! it fits a band-plan digital/experimental segment, an FM-simplex DATA channel
//! for FM-HT users — always offset clear of the FM national calling freqs
//! (146.520 / 446.000 / 223.500), APRS, satellite, and repeater sub-bands.

use serde::{Deserialize, Serialize};

/// One Tempo calling channel: a band, a recommended dial frequency, and the mode
/// the radio should be in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandChannel {
    /// Band label, e.g. "20m", "2m".
    pub band: String,
    /// Grouping for the UI: "HF" | "VHF" | "UHF".
    pub group: String,
    /// Recommended Tempo calling dial frequency (MHz, suppressed carrier).
    pub dial_mhz: f64,
    /// Rig mode for this channel: "USB" (weak-signal) or "FM" (simplex data).
    pub mode: String,
    /// Display label for the selector, e.g. "2 m · FM simplex".
    pub label: String,
    /// Short note: what it sits near / clearance / privilege flag.
    pub note: String,
}

fn ch(band: &str, group: &str, dial_mhz: f64, mode: &str, label: &str, note: &str) -> BandChannel {
    BandChannel {
        band: band.to_string(),
        group: group.to_string(),
        dial_mhz,
        mode: mode.to_string(),
        label: label.to_string(),
        note: note.to_string(),
    }
}

/// The AWARD/ADIF band identity for a band-plan channel token. Channel ids may
/// carry a suffix that distinguishes CHANNELS on one band ("2m-fm", "6m-2",
/// "2m-call", "40m-dx", "80m-eu") — presentation ids, never band identities.
/// The suffix must not reach stored state: `settings.band` feeds
/// `QsoRecord.band`, the ADIF file and every upload verbatim, and the award/
/// interop readers accept only the base label. THE one place the suffix is
/// stripped — call this at the state boundary rather than hand-splitting.
pub fn canonical_band(token: &str) -> String {
    let t = token.trim();
    t.split('-').next().unwrap_or(t).to_string()
}

/// The proposed Tempo band plan — verified US General-legal + CW-clear (judged on
/// the emission ≈ dial + 1.5 kHz). Ordered low band → high.
pub fn band_plan() -> Vec<BandChannel> {
    vec![
        // --- HF (USB weak-signal, "upper shoulder" of the digital cluster) ---
        ch("160m", "HF", 1.8460, "USB", "160 m", "above the whole FT8/JS8 cluster (≤1.843) and PSK31 1.838; emission ~1.8475, ~5.5 kHz above JS8 1.842"),
        ch("80m", "HF", 3.5935, "USB", "80 m", "above the PSK31/RTTY hole 3.580–3.590 (~5 kHz) and below the 3.600 data edge; clear of FT8/FT4 3.573/3.575"),
        ch("40m", "HF", 7.0430, "USB", "40 m", "in the notch between QRP CW 7.040 (~4.5 kHz below emission) and FT4 7.0475 (~3 kHz above); IARU NB segment"),
        ch("30m", "HF", 10.1425, "USB", "30 m", "data half, ~3 kHz above FT4 10.140 / PSK 10.141, ~6 kHz below the 10.150 edge; secondary band — tread lightly"),
        ch("20m", "HF", 14.0905, "USB", "20 m", "the .09 shoulder: ~9 kHz above the 14.074–14.083 cluster, ~3.6 kHz below WSPR 14.0956"),
        ch("17m", "HF", 18.0955, "USB", "17 m", "cramped band — in the only notch (~3 kHz below FT8 18.100, ~1 kHz above QRP CW 18.096), clear of the FT4/JS8/WSPR pileup at 18.104+; DX1 (50 Hz) only"),
        ch("15m", "HF", 21.0905, "USB", "15 m", "~14 kHz above JS8 21.078, ~2.6 kHz below WSPR 21.0946; FT4 is far away at 21.140"),
        ch("12m", "HF", 24.9115, "USB", "12 m", "cramped — in the notch ~2 kHz below FT8 24.915 and ~3 kHz above SKCC CW 24.910, clear of FT4 24.919; DX1 (50 Hz) only"),
        ch("10m", "HF", 28.1000, "USB", "10 m", "roomy; ~20 kHz above the FT8 cluster, ~18 kHz below PSK 28.120 — Technician-accessible (≤200 W)"),
        // --- 6 m (USB; Technician-accessible) ---
        ch("6m", "VHF", 50.3450, "USB", "6 m", "above the FT8/JS8/MSK144 cluster (ends ~50.328), below 50.620 digital — Tech-OK"),
        // --- 2 m ---
        ch("2m", "VHF", 144.2350, "USB", "2 m · SSB/weak-signal", "in the 144.200–144.275 weak-signal segment; clear of SSB call 144.200, FT8 144.174, beacons 144.275+"),
        ch("2m-fm", "VHF", 145.5600, "FM", "2 m · FM simplex (HT)", "in the 145.50–145.80 experimental segment; far from 146.520, APRS 144.39, sat 145.8+ — verify local channel"),
        // --- 1.25 m ---
        ch("1.25m-fm", "VHF", 223.5600, "FM", "1.25 m · FM simplex (HT)", "in the 223.52–223.64 digital segment (purpose-built); ~20 kHz above the 223.540 FM call — verify local channel"),
        ch("1.25m", "VHF", 222.1300, "USB", "1.25 m · SSB/weak-signal", "alt: 222.10–222.15 weak-signal segment, above 222.100 call + FT8 222.065"),
        // --- 70 cm ---
        ch("70cm", "UHF", 432.4500, "USB", "70 cm · SSB/weak-signal", "in 432.40–433.00 mixed-mode; far from SSB call 432.100, sat 435–438, beacons 432.3–432.4"),
        ch("23cm", "UHF", 1296.2000, "USB", "23 cm · SSB/weak-signal", "in the 1296.2 SSB segment; clear of the 1296.100 call, FT8 1296.174, beacons 1296.300+"),
        ch("70cm-fm", "UHF", 445.9500, "FM", "70 cm · FM simplex (HT)", "local-option only — 70 cm has no national digital segment; below 446.000 call. Check your coordinator"),
    ]
}

/// The **standard WSJT-X FT8 dial frequencies** — so that on the FT8 tier a band
/// pick lands you on the canonical watering hole (14.074 etc.) where the FT8 world
/// calls, not Nexus's native off-cluster channel. USB, suppressed-carrier dials.
pub fn ft8_band_plan() -> Vec<BandChannel> {
    let n = "standard FT8 calling frequency (WSJT-X default)";
    vec![
        ch("160m", "HF", 1.840, "USB", "160 m · FT8", n),
        ch("80m", "HF", 3.573, "USB", "80 m · FT8", n),
        // ⚠️ NOT `n` — 60 m has no WSJT-X default to be "the" standard. Upstream's
        // `models/FrequencyList.cpp` ships NO 5 MHz row at all, so this value is ours.
        // 5.3715 is the USB dial for the US 60 m channel centred on 5373.0 kHz
        // (suppressed-carrier dial = centre − 1.5 kHz), operator's choice 2026-08-05.
        // 60 m is CHANNELISED in the US and several other administrations and the channels
        // differ country to country — an operator outside the US must check their own plan.
        ch(
            "60m",
            "HF",
            5.3715,
            "USB",
            "60 m · FT8",
            "US 60 m channel at 5373.0 kHz centre (dial = centre - 1.5 kHz); 60 m is channelised \
             and the channels differ by country - check your own band plan",
        ),
        ch("40m", "HF", 7.074, "USB", "40 m · FT8", n),
        ch("30m", "HF", 10.136, "USB", "30 m · FT8", n),
        ch("20m", "HF", 14.074, "USB", "20 m · FT8", n),
        ch("17m", "HF", 18.100, "USB", "17 m · FT8", n),
        ch("15m", "HF", 21.074, "USB", "15 m · FT8", n),
        ch("12m", "HF", 24.915, "USB", "12 m · FT8", n),
        ch("10m", "HF", 28.074, "USB", "10 m · FT8", n),
        ch("6m", "VHF", 50.313, "USB", "6 m · FT8", n),
        // ⚠️ 4 m IS NOT A WORLDWIDE BAND, and it is the only one in this table that
        // is not. It exists on a CEPT secondary basis (footnote ECA9, 69.9–70.5 MHz)
        // in IARU Region 1 only, with national edges that differ by tens of kHz —
        // 70.154 is inside DL's 70.150–70.210 by 4 kHz and OUTSIDE CT, I and LY
        // altogether — and the US has NO 4 m allocation at any class. The dial is
        // WSJT-X's own R1 default (`models/FrequencyList.cpp`, flagged `preferred`;
        // it is row 52 of `paritylab/wsjtx_freqs.txt`, the table this plan is
        // generated from, and was simply dropped in transcription — the other three
        // 4 m rows from that file ship in the MSK144/JT65/WSPR plans below). The note
        // is the tooltip an operator reads before keying, so it says so.
        ch("4m", "VHF", 70.154, "USB", "4 m · FT8", "standard FT8 calling frequency (WSJT-X default) — IARU Region 1 only, no US allocation; 4 m band edges vary widely by country, confirm yours before transmitting"),
        ch("2m", "VHF", 144.174, "USB", "2 m · FT8", n),
        ch("70cm", "UHF", 432.065, "USB", "70 cm · FT8", n),
        ch("23cm", "UHF", 1296.174, "USB", "23 cm · FT8", n),
    ]
}

/// The **standard WSJT-X FT4 dial frequencies**. USB, suppressed-carrier dials.
pub fn ft4_band_plan() -> Vec<BandChannel> {
    let n = "standard FT4 calling frequency (WSJT-X default)";
    vec![
        ch("80m", "HF", 3.575, "USB", "80 m · FT4", n),
        ch("40m", "HF", 7.0475, "USB", "40 m · FT4", n),
        ch("30m", "HF", 10.140, "USB", "30 m · FT4", n),
        ch("20m", "HF", 14.080, "USB", "20 m · FT4", n),
        ch("17m", "HF", 18.104, "USB", "17 m · FT4", n),
        ch("15m", "HF", 21.140, "USB", "15 m · FT4", n),
        ch("12m", "HF", 24.919, "USB", "12 m · FT4", n),
        ch("10m", "HF", 28.180, "USB", "10 m · FT4", n),
        ch("6m", "VHF", 50.318, "USB", "6 m · FT4", n),
        ch("2m", "VHF", 144.170, "USB", "2 m · FT4", n),
    ]
}

/// The **standard RTTY activity frequencies** — the classic watering holes where
/// RTTY actually happens (contest/DX activity windows), so a band pick in the RTTY
/// cockpit lands in the action like WSJT-X's per-mode dials do. Dials are LSB per
/// the RTTY convention (mark = higher RF / lower audio); the cockpit's rig-mode
/// policy handles FSK-mode rigs separately.
pub fn rtty_band_plan() -> Vec<BandChannel> {
    vec![
        ch("160m", "HF", 1.838, "LSB", "160 m · RTTY", "RTTY is rare here; shared with PSK31 1.838 — listen first"),
        ch("80m", "HF", 3.590, "LSB", "80 m · RTTY", "the classic 3.580–3.600 RTTY window; 3.585/3.590 are the DX calling spots — 3.580 itself is PSK31's home"),
        ch("40m", "HF", 7.080, "LSB", "40 m · RTTY (US)", "US activity 7.080–7.100 (ARRL RTTY/data 7.080–7.125)"),
        ch("40m-dx", "HF", 7.045, "LSB", "40 m · RTTY (EU/DX)", "IARU R1 digimode window 7.040–7.047, DX calling 7.040/7.045; WSPR sits at 7.0386 — stay high"),
        ch("30m", "HF", 10.142, "LSB", "30 m · RTTY", "10.140–10.150 data half; secondary band — tread lightly"),
        ch("20m", "HF", 14.083, "LSB", "20 m · RTTY", "the 14.080–14.090 RTTY window, above the FT4 cluster at 14.080"),
        ch("17m", "HF", 18.105, "LSB", "17 m · RTTY", "18.100–18.108 window, above FT8 18.100's audio cluster"),
        ch("15m", "HF", 21.083, "LSB", "15 m · RTTY", "the 21.080–21.100 RTTY window, above JS8 21.078"),
        ch("12m", "HF", 24.920, "LSB", "12 m · RTTY", "the 24.920–24.925 RTTY window (IARU digimodes 24.915–24.929), clear of FT8 24.915"),
        ch("10m", "HF", 28.083, "LSB", "10 m · RTTY", "the 28.080–28.100 RTTY window — Technician-accessible"),
    ]
}

/// The **standard SSTV calling frequencies** — where images actually appear,
/// including the ISS downlink (the biggest SSTV driver: ARISS events transmit
/// PD120 on 145.800 FM). Phone-segment dials, phone sideband conventions.
pub fn sstv_band_plan() -> Vec<BandChannel> {
    vec![
        ch("160m", "HF", 1.890, "LSB", "160 m · SSTV", "the conventional 160 m image frequency — rare, and a winter-night band"),
        ch("80m", "HF", 3.845, "LSB", "80 m · SSTV (US)", "NA SSTV calling frequency"),
        ch("80m-eu", "HF", 3.730, "LSB", "80 m · SSTV (EU)", "EU SSTV calling (IARU R1 image centre 3.735) — below US General phone; Extra-class or DX"),
        ch("40m", "HF", 7.171, "LSB", "40 m · SSTV (US)", "US SSTV calling frequency"),
        ch("40m-eu", "HF", 7.165, "LSB", "40 m · SSTV (EU)", "EU/IARU R1 SSTV calling"),
        ch("20m", "HF", 14.230, "USB", "20 m · SSTV", "THE worldwide SSTV calling frequency — where images actually appear"),
        // ⚠️ THE OVERFLOW FREQUENCIES ARE CHANNELS, NOT PROSE. 14.233 and 14.236 were
        // named only inside 14.230's note, so an operator working the overflow (the field
        // report came from 14.236) found their own frequency nowhere in the app. If a
        // watering hole is real enough to sit on, it is real enough to be pickable.
        ch("20m-alt", "HF", 14.233, "USB", "20 m · SSTV (alt)", "the first overflow when 14.230 is busy"),
        ch("20m-alt2", "HF", 14.236, "USB", "20 m · SSTV (alt 2)", "the second overflow — commonly used during contests and nets"),
        ch("17m", "HF", 18.160, "USB", "17 m · SSTV", "the conventional 17 m image frequency"),
        ch("15m", "HF", 21.340, "USB", "15 m · SSTV", "worldwide 15 m SSTV calling"),
        ch("12m", "HF", 24.975, "USB", "12 m · SSTV", "the conventional 12 m image frequency"),
        // ⚠️ NOT a Technician frequency. US Technicians hold 10 m phone/image only
        // 28.300–28.500, so 28.680 is General and above — the privilege filter is
        // per-BAND, and a Tech is shown this channel. Saying otherwise on screen
        // would be the app itself giving wrong licensing advice.
        ch("10m", "HF", 28.680, "USB", "10 m · SSTV", "worldwide 10 m SSTV calling (General and above — Technicians have 10 m image only 28.300–28.500)"),
        ch("6m", "VHF", 50.680, "USB", "6 m · SSTV", "the 6 m image frequency; activity follows sporadic-E openings"),
        ch("2m", "VHF", 145.800, "FM", "2 m · ISS downlink", "ARISS events transmit PD120 images here — the SSTV event of the year, FM"),
        ch("2m-call", "VHF", 144.500, "FM", "2 m · SSTV calling", "terrestrial VHF SSTV calling (regional conventions vary — check locally)"),
    ]
}

/// The band/calling plan for the active tier.
///
/// ⭐ EVERY WSJT-X MODE HAS ITS OWN FREQUENCIES, and they are NOT the FT8 ones.
/// An earlier version of this function mapped all six new tiers to
/// `ft8_band_plan()` with a comment calling it a knowing placeholder. It was
/// worse than the comment admitted:
///   * MSK144 has NO HF PRESENCE AT ALL — it is 6 m/4 m/2 m/70 cm meteor
///     scatter. Handing it 14.074 points the rig at a band the mode is not used
///     on.
///   * Q65 is likewise VHF+ (6 m through 1.2 cm); its HF entries do not exist.
///   * FST4/FST4W are LF/MF — 2200 m, 630 m, 160 m — the opposite end of the
///     spectrum from FT8.
///   * WSPR's HF frequencies differ from FT8's everywhere (20 m is 14.0956, not
///     14.074), so an operator on "20 m WSPR" would have been listening to FT8.
///   * JT65 is 14.076 on 20 m, not 14.074.
///
/// The per-mode plans below are GENERATED from WSJT-X's own frequency table —
/// see `paritylab/parse_wsjtx_freqs.py`, which reads the FrequenciesForRegionModes
/// blob out of a real WSJT-X.ini — rather than typed from memory. Roughly ninety
/// amateur frequencies is exactly the volume where one transposed digit hides in
/// a table and looks authoritative.
pub fn band_plan_for(tier: crate::dto::Tier) -> Vec<BandChannel> {
    use crate::dto::Tier;
    match tier {
        Tier::Ft8 => ft8_band_plan(),
        Tier::Ft4 => ft4_band_plan(),
        Tier::Q65 => q65_band_plan(),
        Tier::Msk144 => msk144_band_plan(),
        Tier::Fst4 => fst4_band_plan(),
        Tier::Fst4w => fst4w_band_plan(),
        Tier::Jt65 => jt65_band_plan(),
        Tier::Wspr => wspr_band_plan(),
        // TempoFast/TempoDeep use Nexus's native off-cluster plan — new narrow
        // modes that must avoid mutual QRM with the WSJT-X watering holes.
        Tier::TempoFast | Tier::TempoDeep => band_plan(),
    }
}

/// Q65 calling/beacon frequencies, taken verbatim from WSJT-X's own
/// frequency table (see the module note). 14 entries.
pub fn q65_band_plan() -> Vec<BandChannel> {
    let n = "WSJT-X Q65 calling frequency (from WSJT-X's default table)";
    vec![
        ch("6m", "VHF", 50.211000, "USB", "6 m · Q65", n),
        ch("6m-2", "VHF", 50.275000, "USB", "6 m · Q65", n),
        ch("2m", "VHF", 144.116000, "USB", "2 m · Q65", n),
        ch("1.25m", "VHF", 222.065000, "USB", "1.25 m · Q65", n),
        ch("70cm", "VHF", 432.065000, "USB", "70cm · Q65", n),
        ch("33cm", "VHF", 902.065000, "USB", "33cm · Q65", n),
        ch("23cm", "VHF", 1296.065000, "USB", "23cm · Q65", n),
        ch("13cm", "VHF", 2301.065000, "USB", "13cm · Q65", n),
        ch("13cm-2", "VHF", 2304.065000, "USB", "13cm · Q65", n),
        ch("13cm-3", "VHF", 2320.065000, "USB", "13cm · Q65", n),
        ch("9cm", "VHF", 3400.065000, "USB", "9cm · Q65", n),
        ch("6cm", "VHF", 5760.200000, "USB", "6cm · Q65", n),
        ch("3cm", "VHF", 10368.200000, "USB", "3cm · Q65", n),
        ch("1.25cm", "VHF", 24048.200000, "USB", "1.25cm · Q65", n),
    ]
}

/// MSK144 calling/beacon frequencies, taken verbatim from WSJT-X's own
/// frequency table (see the module note). 6 entries.
pub fn msk144_band_plan() -> Vec<BandChannel> {
    let n = "WSJT-X MSK144 calling frequency (from WSJT-X's default table)";
    vec![
        ch("6m", "VHF", 50.260000, "USB", "6 m · MSK144", n),
        ch("6m-2", "VHF", 50.380000, "USB", "6 m · MSK144", n),
        ch("4m", "VHF", 70.230000, "USB", "4 m · MSK144", n),
        ch("2m", "VHF", 144.150000, "USB", "2 m · MSK144", n),
        ch("2m-2", "VHF", 144.360000, "USB", "2 m · MSK144", n),
        ch("70cm", "VHF", 432.360000, "USB", "70cm · MSK144", n),
    ]
}

/// FST4 calling/beacon frequencies, taken verbatim from WSJT-X's own
/// frequency table (see the module note). 3 entries.
pub fn fst4_band_plan() -> Vec<BandChannel> {
    let n = "WSJT-X FST4 calling frequency (from WSJT-X's default table)";
    vec![
        ch("2200m", "HF", 0.136000, "USB", "2200 m · FST4", n),
        ch("630m", "HF", 0.474200, "USB", "630 m · FST4", n),
        ch("160m", "HF", 1.839000, "USB", "160 m · FST4", n),
    ]
}

/// FST4W calling/beacon frequencies, taken verbatim from WSJT-X's own
/// frequency table (see the module note). 3 entries.
pub fn fst4w_band_plan() -> Vec<BandChannel> {
    let n = "WSJT-X FST4W beacon frequency (from WSJT-X's default table)";
    vec![
        ch("2200m", "HF", 0.136000, "USB", "2200 m · FST4W", n),
        ch("630m", "HF", 0.474200, "USB", "630 m · FST4W", n),
        ch("160m", "HF", 1.836800, "USB", "160 m · FST4W", n),
    ]
}

/// JT65 calling/beacon frequencies, taken verbatim from WSJT-X's own
/// frequency table (see the module note). 22 entries.
pub fn jt65_band_plan() -> Vec<BandChannel> {
    let n = "WSJT-X JT65 calling frequency (from WSJT-X's default table)";
    vec![
        ch("160m", "HF", 1.838000, "USB", "160 m · JT65", n),
        ch("80m", "HF", 3.570000, "USB", "80 m · JT65", n),
        ch("40m", "HF", 7.076000, "USB", "40 m · JT65", n),
        ch("30m", "HF", 10.138000, "USB", "30 m · JT65", n),
        ch("20m", "HF", 14.076000, "USB", "20 m · JT65", n),
        ch("17m", "HF", 18.102000, "USB", "17 m · JT65", n),
        ch("15m", "HF", 21.076000, "USB", "15 m · JT65", n),
        ch("12m", "HF", 24.917000, "USB", "12 m · JT65", n),
        ch("10m", "HF", 28.076000, "USB", "10 m · JT65", n),
        ch("6m", "VHF", 50.276000, "USB", "6 m · JT65", n),
        ch("6m-2", "VHF", 50.310000, "USB", "6 m · JT65", n),
        ch("4m", "VHF", 70.102000, "USB", "4 m · JT65", n),
        ch("2m", "VHF", 144.120000, "USB", "2 m · JT65", n),
        ch("1.25m", "VHF", 222.065000, "USB", "1.25 m · JT65", n),
        ch("70cm", "VHF", 432.065000, "USB", "70cm · JT65", n),
        ch("33cm", "VHF", 902.065000, "USB", "33cm · JT65", n),
        ch("23cm", "VHF", 1296.065000, "USB", "23cm · JT65", n),
        ch("13cm", "VHF", 2301.065000, "USB", "13cm · JT65", n),
        ch("13cm-2", "VHF", 2304.065000, "USB", "13cm · JT65", n),
        ch("13cm-3", "VHF", 2320.065000, "USB", "13cm · JT65", n),
        ch("9cm", "VHF", 3400.065000, "USB", "9cm · JT65", n),
        ch("6cm", "VHF", 5760.065000, "USB", "6cm · JT65", n),
    ]
}

/// WSPR calling/beacon frequencies, taken verbatim from WSJT-X's own
/// frequency table (see the module note). 16 entries.
pub fn wspr_band_plan() -> Vec<BandChannel> {
    let n = "WSPR beacon frequency (from WSJT-X's default table)";
    vec![
        ch("2200m", "HF", 0.136000, "USB", "2200 m · WSPR", n),
        ch("630m", "HF", 0.474200, "USB", "630 m · WSPR", n),
        ch("160m", "HF", 1.836600, "USB", "160 m · WSPR", n),
        ch("80m", "HF", 3.568600, "USB", "80 m · WSPR", n),
        ch("40m", "HF", 7.038600, "USB", "40 m · WSPR", n),
        ch("30m", "HF", 10.138700, "USB", "30 m · WSPR", n),
        ch("20m", "HF", 14.095600, "USB", "20 m · WSPR", n),
        ch("17m", "HF", 18.104600, "USB", "17 m · WSPR", n),
        ch("15m", "HF", 21.094600, "USB", "15 m · WSPR", n),
        ch("12m", "HF", 24.924600, "USB", "12 m · WSPR", n),
        ch("10m", "HF", 28.124600, "USB", "10 m · WSPR", n),
        ch("6m", "VHF", 50.293000, "USB", "6 m · WSPR", n),
        ch("4m", "VHF", 70.091000, "USB", "4 m · WSPR", n),
        ch("2m", "VHF", 144.489000, "USB", "2 m · WSPR", n),
        ch("70cm", "VHF", 432.300000, "USB", "70cm · WSPR", n),
        ch("23cm", "VHF", 1296.500000, "USB", "23cm · WSPR", n),
    ]
}

/// Where CW ACTIVITY concentrates on each band (the general-CW / QRP / SKCC watering holes),
/// so the CW cockpit parks the operator IN the action instead of on the dead band edge (the
/// 20 m CW segment starts at 14.000, but nobody works there — activity is ~14.030+). The
/// caller clamps this to the licensed CW-segment start, so it never drops below privileges.
pub fn cw_activity_mhz(band: &str) -> Option<f64> {
    Some(match band {
        "160m" => 1.810,
        "80m" => 3.550,
        "40m" => 7.030,
        "30m" => 10.110,
        "20m" => 14.030,
        "17m" => 18.080,
        "15m" => 21.030,
        "12m" => 24.900,
        "10m" => 28.030,
        "6m" => 50.090, // 6 m CW calling frequency
        "2m" => 144.050,
        "1.25m" => 222.050,
        "70cm" => 432.050,
        "23cm" => 1296.050,
        _ => return None,
    })
}

/// The Tempo channel whose dial matches `dial_mhz` (within 500 Hz), if any — used
/// by the UI to highlight the active band channel.
pub fn channel_for_dial(dial_mhz: f64) -> Option<BandChannel> {
    band_plan()
        .into_iter()
        .find(|c| (c.dial_mhz - dial_mhz).abs() < 0.0005)
}

/// The amateur band label for an ARBITRARY dial frequency (MHz) — for live VFO read-back,
/// where the operator may tune anywhere on a band, not just the band-plan watering holes.
/// `None` if the frequency is off any ham band.
pub fn band_for_dial(dial_mhz: f64) -> Option<&'static str> {
    let b = match dial_mhz {
        f if (1.8..2.0).contains(&f) => "160m",
        f if (3.5..4.0).contains(&f) => "80m",
        f if (5.3..5.5).contains(&f) => "60m",
        f if (7.0..7.3).contains(&f) => "40m",
        f if (10.1..10.15).contains(&f) => "30m",
        f if (14.0..14.35).contains(&f) => "20m",
        f if (18.06..18.17).contains(&f) => "17m",
        f if (21.0..21.45).contains(&f) => "15m",
        f if (24.89..24.99).contains(&f) => "12m",
        f if (28.0..29.7).contains(&f) => "10m",
        f if (50.0..54.0).contains(&f) => "6m",
        f if (70.0..71.0).contains(&f) => "4m",
        f if (144.0..148.0).contains(&f) => "2m",
        f if (222.0..225.0).contains(&f) => "1.25m",
        f if (420.0..450.0).contains(&f) => "70cm",
        f if (902.0..928.0).contains(&f) => "33cm",
        f if (1240.0..1300.0).contains(&f) => "23cm",
        // The microwave bands (Batch 3, the QO-100 field report): ADIF's registered
        // band names and edges, because `settings.band` reaches the ADIF BAND field and
        // LoTW validates it — "13cm" spans BOTH US segments (2300–2310 / 2390–2450, the
        // gap is privilege-gated, not band-gated), and 5760 is ADIF "6cm", not the
        // colloquial "5cm". US TX privileges above 23 cm live in `privileges.rs` and are
        // NOT implied by a label existing here — 9 cm carries a label (EU/QO-100-class
        // operating, honest logging) with no US segments at all (the 3.3–3.5 GHz
        // amateur allocation was removed).
        f if (2300.0..2450.0).contains(&f) => "13cm",
        f if (3300.0..3500.0).contains(&f) => "9cm",
        f if (5650.0..5925.0).contains(&f) => "6cm",
        f if (10_000.0..10_500.0).contains(&f) => "3cm",
        f if (24_000.0..24_250.0).contains(&f) => "1.25cm",
        // Everything higher (47 GHz+) stays None on purpose: `ab_cross_band_refusal`
        // and the typed-dial commits fail closed on an unnamed dial, and that guard
        // must keep holding for bands this table still does not know.
        _ => return None,
    };
    Some(b)
}

#[cfg(test)]
mod tests {
    /// 4 m is the one band Nexus ships that exists ALMOST EVERYWHERE EXCEPT THE US
    /// (IARU Region 1 only; CEPT footnote ECA9), so it is the band where a
    /// US-table-shaped assumption shows up. Drive the resolution the operator
    /// actually performs — pick 4 m on a tier, get a dial — rather than restating
    /// the constants.
    #[test]
    fn the_4m_band_exists_end_to_end() {
        use crate::dto::Tier;
        // (band, mode) → dial, through the same dispatch the cockpit uses.
        let dial = |t: Tier| -> Option<f64> {
            band_plan_for(t)
                .into_iter()
                .find(|c| canonical_band(&c.band) == "4m")
                .map(|c| c.dial_mhz)
        };
        // The four WSJT-X modes with a 70 MHz row in their default table, and only
        // those four (WSJT-X `models/FrequencyList.cpp`; `paritylab/wsjtx_freqs.txt`).
        assert_eq!(dial(Tier::Ft8), Some(70.154));
        assert_eq!(dial(Tier::Jt65), Some(70.102));
        assert_eq!(dial(Tier::Msk144), Some(70.230));
        assert_eq!(dial(Tier::Wspr), Some(70.091));
        // FT4 and Q65 have NO 4 m frequency upstream — an invented one would point
        // an operator at a dial nobody watches, in a band whose national edges vary.
        assert_eq!(dial(Tier::Ft4), None);
        assert_eq!(dial(Tier::Q65), None);
        // Every one of those dials must name itself back as 4 m, or the label the
        // ADIF/interop wire carries disagrees with the dial the rig is on.
        for f in [70.091, 70.102, 70.154, 70.230] {
            assert_eq!(super::band_for_dial(f), Some("4m"), "{f} MHz is 4 m");
        }
    }

    /// ⚠️ THE US OPERATOR HAS NO 4 m ALLOCATION AT ALL — no class, no segment, no
    /// exception. The 4 m channels are shipped for the app's Region-1 users, so the
    /// guarantee is that they stay *pickable* while remaining *unkeyable* for a US
    /// class: the existing TX lockout answers, the same way it does for a Technician
    /// on 20 m phone. Nothing here may become `true` without an FCC allocation.
    #[test]
    fn no_us_license_class_may_key_a_4m_channel() {
        use crate::privileges::{phone_home, segment_start, tx_allowed};
        use crate::settings::{LicenseClass, OperatingMode};
        let us = [
            LicenseClass::Technician,
            LicenseClass::General,
            LicenseClass::Extra,
        ];
        // Judge the EMISSION, not the dial: USB data sits ~1.5 kHz above the dial.
        let off = 0.0015;
        for class in us {
            for f in [70.091, 70.102, 70.154, 70.230] {
                assert!(
                    !tx_allowed(class, f + off, OperatingMode::Digital),
                    "{class:?} must not key 4 m data at {f}"
                );
            }
            assert!(!tx_allowed(class, 70.200, OperatingMode::Phone));
            assert!(!tx_allowed(class, 70.200, OperatingMode::Cw));
            // No home dial either — the band drops out of every picker rather than
            // publishing a start the transmit gate would then refuse.
            for m in [
                OperatingMode::Digital,
                OperatingMode::Cw,
                OperatingMode::Phone,
            ] {
                assert_eq!(segment_start(class, "4m", m), None, "{class:?} 4 m {m:?}");
            }
            assert_eq!(phone_home(class, "4m"), None);
        }
        // The non-US class keys it — that is who the band is for.
        assert!(tx_allowed(
            LicenseClass::Open,
            70.154 + off,
            OperatingMode::Digital
        ));
    }

    #[test]
    fn the_23cm_band_exists_end_to_end() {
        // IC-9700 support: dial→band, the FT8 channel at the verified 1296.174,
        // and a CW dial — a band missing any of these is invisible to QSY/UI.
        assert_eq!(super::band_for_dial(1296.174), Some("23cm"));
        assert_eq!(super::cw_activity_mhz("23cm"), Some(1296.05));
        let has_ft8_23 = super::ft8_band_plan()
            .iter()
            .any(|c| c.band == "23cm" && (c.dial_mhz - 1296.174).abs() < 1e-6);
        assert!(has_ft8_23, "23cm FT8 channel at 1296.174");
    }

    use super::*;

    /// Every channel Nexus can put an operator on: every TIER's plan, plus the
    /// two plans no tier dispatches to.
    ///
    /// ⚠️ THE TIER HALF IS CLOSED; THE OTHER TWO ARE NOT, AND THE DIFFERENCE IS
    /// THE POINT. Driving `band_plan_for` over `Tier::ALL` means a NEW TIER
    /// cannot slip past these tests: `band_plan_for`'s own `match` is
    /// exhaustive, so adding a variant fails to compile until it is dispatched,
    /// and `Tier::ALL` is asserted complete against that match below. That is a
    /// real guarantee.
    ///
    /// `rtty_band_plan` and `sstv_band_plan` are appended BY NAME because no
    /// tier reaches them — RTTY and SSTV are sections, not tiers. So a plan
    /// added for some future section IS guarded only if somebody adds it here,
    /// exactly as the whole list used to be. Said plainly rather than left to
    /// be assumed: the previous version of this comment claimed one census
    /// guarded everything, and it guarded whatever the list happened to hold.
    fn every_shipped_channel() -> Vec<BandChannel> {
        let mut plans: Vec<Vec<BandChannel>> = crate::dto::Tier::ALL
            .iter()
            .map(|t| band_plan_for(*t))
            .collect();
        plans.push(rtty_band_plan());
        plans.push(sstv_band_plan());
        plans.into_iter().flatten().collect()
    }

    /// `Tier::ALL` really is every variant — the thing `every_shipped_channel`'s
    /// closure claim rests on. The exhaustive `match` is what fails to compile
    /// when a variant is added without being listed.
    #[test]
    fn tier_all_lists_every_tier() {
        use crate::dto::Tier;
        for t in Tier::ALL {
            #[allow(clippy::match_single_binding)]
            match t {
                Tier::TempoFast
                | Tier::TempoDeep
                | Tier::Ft8
                | Tier::Ft4
                | Tier::Fst4
                | Tier::Fst4w
                | Tier::Q65
                | Tier::Msk144
                | Tier::Jt65
                | Tier::Wspr => {}
            }
        }
        assert_eq!(
            Tier::ALL.len(),
            10,
            "a tier was added or removed — update Tier::ALL and the match above, \
             then re-check every test that drives every_shipped_channel()"
        );
    }

    #[test]
    fn every_channel_token_canonicalises_to_a_plain_band_label() {
        // Walk every token every plan ships: the canonical form must carry no
        // channel suffix, be non-empty, and — when the channel's dial maps to a
        // band at all — agree with the dial-derived band. This is the census
        // that keeps the presentation-id / band-identity split honest as
        // channels are added.
        for c in every_shipped_channel() {
            let canon = canonical_band(&c.band);
            assert!(!canon.is_empty(), "{}: empty canonical band", c.band);
            assert!(
                !canon.contains('-'),
                "{}: canonical band must carry no channel suffix",
                c.band
            );
            if let Some(by_dial) = band_for_dial(c.dial_mhz) {
                assert_eq!(
                    canon, by_dial,
                    "{} @ {}: canonical band must agree with the dial's band",
                    c.band, c.dial_mhz
                );
            }
        }
    }

    #[test]
    fn no_shipped_channel_reaches_the_interop_wire_as_a_band_it_is_not_on() {
        // ⚠️ THE REACHABILITY HALF of `tempo_net::band_for_interop`'s rule, and
        // the reason that rule is not speculative. The N1MM `<contactinfo>`
        // `band`, the N3FJP `fldBand` and the N3FJP band report all carry a
        // METRE count, and every one of them gets it from `settings.band` — a
        // band-plan channel's own label, canonicalised (`tune_dial`/`pick_band`
        // call `canonical_band` at the state boundary and nothing else touches
        // it). By THREE routes, not one: the band report reads it straight off
        // the snapshot (`snap.radio.band`, which `set_radio` mirrors); the
        // Field Day emitter reads it by way of `FieldDayLog::band`, which
        // `sync_fd_band` keeps equal to it and which stamps every contact; and
        // the STANDING N1MM broadcast reads the `QsoRecord` the cockpit log
        // strips build from that same band. Full census in `band_for_interop`'s
        // own doc.
        //
        // So every label in the census below is one the wire can carry. The
        // centimetre ones are the point: the alpha-strip that used to be the
        // catch-all left 13 cm as "13" and 3 cm as "3" — bare numbers a
        // metres-bucketed field cannot read as anything but metres. Q65 ships
        // 13/9/5/3/1.2 cm channels TODAY (JT65 the first three), so this is a
        // census, not a hypothetical; it fails the moment the rule or a channel
        // moves.
        let mut cm_seen = Vec::new();
        for c in every_shipped_channel() {
            let band = canonical_band(&c.band);
            let wire = tempo_net::band_for_interop(&band);
            match band.strip_suffix("cm") {
                Some(n) => {
                    let n: f64 = n
                        .parse()
                        .unwrap_or_else(|_| panic!("{band}: unparsable cm"));
                    assert_eq!(
                        wire,
                        format!("{}", n / 100.0),
                        "{band} must reach the interop wire in metres"
                    );
                    assert_ne!(
                        wire,
                        n.to_string(),
                        "{band} goes out as the bare number {n}"
                    );
                    cm_seen.push(band);
                }
                // A metre label is already the metre count — strip the unit only.
                None => assert_eq!(
                    wire,
                    band.trim_end_matches(|ch: char| ch.is_alphabetic()),
                    "{band} must reach the interop wire unchanged"
                ),
            }
        }
        cm_seen.sort();
        cm_seen.dedup();
        // Named so a reader can see WHICH microwave channels are in play — and
        // so deleting them all (rather than fixing the wire) cannot quietly turn
        // the loop above into a no-op.
        assert_eq!(
            cm_seen,
            ["1.25cm", "13cm", "23cm", "33cm", "3cm", "6cm", "70cm", "9cm"],
            "the centimetre channels Nexus ships"
        );
    }

    #[test]
    fn cw_activity_is_inside_the_band_and_off_the_edge() {
        // 20 m CW activity sits above the dead 14.000 edge and inside the CW segment.
        let f = cw_activity_mhz("20m").unwrap();
        assert!(
            f > 14.0 && f < 14.15,
            "20m CW activity {f} should be in the CW segment"
        );
        // Every HF band the picker offers has a CW activity centre.
        for b in [
            "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m",
        ] {
            assert!(
                cw_activity_mhz(b).is_some(),
                "{b} needs a CW activity frequency"
            );
        }
        assert!(cw_activity_mhz("bogus").is_none());
    }

    #[test]
    fn plan_is_nonempty_and_well_formed() {
        let plan = band_plan();
        assert!(plan.len() >= 14, "expect HF + VHF/UHF channels");
        for c in &plan {
            assert!(
                c.dial_mhz > 1.0 && c.dial_mhz < 1400.0, // 23 cm tops the plan (1296)
                "{} dial sane",
                c.band
            );
            assert!(c.mode == "USB" || c.mode == "FM", "{} mode USB/FM", c.band);
            assert!(matches!(c.group.as_str(), "HF" | "VHF" | "UHF"));
        }
    }

    /// ⚠️ "EVERY DIGITAL-TIER CHANNEL COMMANDS USB" IS TRUE OF THE WSJT-X TIERS
    /// AND FALSE OF THE TEMPO ONES — pinned because the Satellites section's log
    /// strip folds the COMMANDED SIDEBAND into the record's ADIF mode
    /// (`adifModeFromStation`), and the CHANGELOG plus docs/guide/satellites.md
    /// describe what that produces. On a WSJT-X tier it is `SSB`; on the three
    /// FM simplex channels this plan ships it is `FM`. Both name an analogue
    /// voice mode for a data-mode contact — one defect — but a doc that says
    /// only "it records SSB" is wrong for a third of the Tempo VHF/UHF plan.
    #[test]
    fn the_tempo_plan_ships_fm_channels_and_the_wsjtx_tiers_do_not() {
        use crate::dto::Tier;
        let fm: Vec<&str> = band_plan()
            .iter()
            .filter(|c| c.mode == "FM")
            .map(|c| canonical_band(&c.band))
            .map(|b| match b.as_str() {
                "2m" => "2m",
                "1.25m" => "1.25m",
                "70cm" => "70cm",
                other => panic!("a new FM channel on {other} — check the guide's wording"),
            })
            .collect();
        assert_eq!(
            fm,
            ["2m", "1.25m", "70cm"],
            "the FM simplex channels the mode-fold docs name"
        );
        for tier in [
            Tier::Ft8,
            Tier::Ft4,
            Tier::Q65,
            Tier::Jt65,
            Tier::Msk144,
            Tier::Fst4,
            Tier::Fst4w,
            Tier::Wspr,
        ] {
            for c in band_plan_for(tier) {
                assert_eq!(
                    c.mode, "USB",
                    "{tier:?} {} commands {} — the SSB claim needs revisiting",
                    c.band, c.mode
                );
            }
        }
    }

    #[test]
    fn known_dials_round_trip_to_channels() {
        assert_eq!(channel_for_dial(14.0905).unwrap().band, "20m");
        assert_eq!(channel_for_dial(50.3450).unwrap().band, "6m");
        assert_eq!(channel_for_dial(145.5600).unwrap().mode, "FM");
        assert!(
            channel_for_dial(14.074).is_none(),
            "FT8 dial is not a Tempo-native channel"
        );
    }

    #[test]
    fn tier_aware_plan_uses_standard_ft8_ft4_dials() {
        use crate::dto::Tier;
        // FT8 tier → the standard 14.074 watering hole (where the FT8 world calls).
        let ft8_20 = band_plan_for(Tier::Ft8)
            .into_iter()
            .find(|c| c.band == "20m")
            .unwrap();
        assert!((ft8_20.dial_mhz - 14.074).abs() < 1e-9, "FT8 20m = 14.074");
        // FT4 tier → 14.080.
        let ft4_20 = band_plan_for(Tier::Ft4)
            .into_iter()
            .find(|c| c.band == "20m")
            .unwrap();
        assert!((ft4_20.dial_mhz - 14.080).abs() < 1e-9, "FT4 20m = 14.080");
        // FT1/DX1 keep the native off-cluster plan (must avoid mutual QRM).
        let ft1_20 = band_plan_for(Tier::TempoFast)
            .into_iter()
            .find(|c| c.band == "20m")
            .unwrap();
        assert!(
            (ft1_20.dial_mhz - 14.0905).abs() < 1e-9,
            "FT1 20m stays native .0905"
        );
        // The full standard set is present (13 + 4 m + 23 cm for the IC-9700 class).
        assert_eq!(ft8_band_plan().len(), 15);
        assert!(ft8_band_plan().iter().all(|c| c.mode == "USB"));
    }

    #[test]
    fn rtty_and_sstv_plans_pin_the_standard_watering_holes() {
        // RTTY: classic activity windows, LSB convention (mark = higher RF).
        let rtty = rtty_band_plan();
        let r20 = rtty.iter().find(|c| c.band == "20m").unwrap();
        assert!(
            (r20.dial_mhz - 14.083).abs() < 1e-9,
            "20m RTTY in the .080–.090 window"
        );
        assert!(
            rtty.iter().all(|c| c.mode == "LSB"),
            "RTTY channels are LSB"
        );
        // 40 m runs 7.080+ in the US but 7.040–7.047 in IARU R1 / DX — both
        // watering holes must be offered (three-persona rule: DX chasers too).
        assert!(
            rtty.iter()
                .any(|c| c.band == "40m" && (c.dial_mhz - 7.080).abs() < 1e-9),
            "40m US RTTY at 7.080"
        );
        assert!(
            rtty.iter()
                .any(|c| c.band == "40m-dx" && (c.dial_mhz - 7.045).abs() < 1e-9),
            "40m EU/DX RTTY at 7.045"
        );
        // SSTV: 14.230 is THE calling frequency; the ISS downlink must be present
        // (ARISS events are the biggest SSTV driver) and FM.
        let sstv = sstv_band_plan();
        let s20 = sstv.iter().find(|c| c.band == "20m").unwrap();
        assert!((s20.dial_mhz - 14.230).abs() < 1e-9, "20m SSTV = 14.230");
        let iss = sstv.iter().find(|c| c.band == "2m").unwrap();
        assert!(
            (iss.dial_mhz - 145.800).abs() < 1e-9,
            "ISS downlink 145.800"
        );
        assert_eq!(iss.mode, "FM");
        // 80 m SSTV splits US/EU: 3.845 NA and 3.730 EU — both entries, both LSB.
        let s80 = sstv.iter().find(|c| c.band == "80m").unwrap();
        assert!((s80.dial_mhz - 3.845).abs() < 1e-9, "80m US SSTV = 3.845");
        let s80eu = sstv.iter().find(|c| c.band == "80m-eu").unwrap();
        assert!((s80eu.dial_mhz - 3.730).abs() < 1e-9, "80m EU SSTV = 3.730");
        assert_eq!(s80.mode, "LSB");
        assert_eq!(s80eu.mode, "LSB");
        // ⚠️ The 20 m OVERFLOW frequencies are pickable channels, not a note. A field
        // report came from 14.236 — an operator on a real watering hole that existed
        // nowhere in the app, so nothing on screen could name where they were.
        for mhz in [14.233_f64, 14.236] {
            assert!(
                sstv.iter().any(|c| (c.dial_mhz - mhz).abs() < 1e-9),
                "20m SSTV overflow {mhz} must be its own channel"
            );
        }
        // 40 m splits US/EU exactly like 80 m does.
        assert!(
            sstv.iter()
                .any(|c| c.band == "40m-eu" && (c.dial_mhz - 7.165).abs() < 1e-9),
            "40m EU SSTV at 7.165"
        );
        // Every HF band an operator might be sitting on has an SSTV channel, so the
        // view can always name the frequency for the band they are on.
        for band in [
            "160m", "80m", "40m", "20m", "17m", "15m", "12m", "10m", "6m", "2m",
        ] {
            assert!(
                sstv.iter().any(|c| c.band.starts_with(band)),
                "{band} needs an SSTV calling frequency"
            );
        }
    }
}

#[cfg(test)]
mod wsjtx_parity_tests {
    use super::*;
    use crate::dto::Tier;

    /// Anchors taken from WSJT-X's own table. These are not round numbers a typo
    /// would land on by accident, and two of them (WSPR 20 m / 30 m) also appear
    /// as literals in WSJT-X's own source (mainwindow.cpp's wsprFreq), so they
    /// are independently checkable.
    #[test]
    fn per_mode_frequencies_match_wsjtx() {
        let f = |t: Tier, band: &str| -> Option<f64> {
            band_plan_for(t)
                .into_iter()
                .find(|c| c.band == band)
                .map(|c| c.dial_mhz)
        };
        // The two production modes, unchanged.
        assert_eq!(f(Tier::Ft8, "20m"), Some(14.074));
        assert_eq!(f(Tier::Ft4, "20m"), Some(14.080));
        // WSPR is NOT on the FT8 frequency — this is the pair that would have
        // silently pointed a WSPR session at an FT8 watering hole.
        assert_eq!(f(Tier::Wspr, "20m"), Some(14.0956));
        assert_eq!(f(Tier::Wspr, "30m"), Some(10.1387));
        // JT65 sits 2 kHz above FT8 on 20 m.
        assert_eq!(f(Tier::Jt65, "20m"), Some(14.076));
        // Q65 and MSK144 are VHF+ modes.
        assert_eq!(f(Tier::Q65, "2m"), Some(144.116));
        assert_eq!(f(Tier::Msk144, "6m"), Some(50.260));
        assert_eq!(f(Tier::Msk144, "2m"), Some(144.150));
        // FST4/FST4W live at the bottom of the spectrum.
        assert_eq!(f(Tier::Fst4, "630m"), Some(0.4742));
        assert_eq!(f(Tier::Fst4w, "2200m"), Some(0.136));
    }

    /// The regression this whole change exists to prevent: every new tier used to
    /// return ft8_band_plan(). If a future tier is added to Tier and forgotten in
    /// band_plan_for, the match is non-exhaustive and will not compile — but a
    /// lazy `_ => ft8_band_plan()` arm would compile and silently recreate the
    /// bug, so assert the distinctness directly.
    #[test]
    fn no_mode_silently_reuses_the_ft8_plan() {
        let ft8 = ft8_band_plan();
        for t in [
            Tier::Q65,
            Tier::Msk144,
            Tier::Fst4,
            Tier::Fst4w,
            Tier::Jt65,
            Tier::Wspr,
        ] {
            let p = band_plan_for(t);
            assert!(!p.is_empty(), "{t:?} has an empty band plan");
            let same = p.len() == ft8.len()
                && p.iter()
                    .zip(ft8.iter())
                    .all(|(a, b)| a.dial_mhz == b.dial_mhz);
            assert!(!same, "{t:?} is still returning the FT8 band plan");
        }
    }

    /// MSK144 and Q65 are VHF+ modes. A stray HF entry would put the rig on a
    /// band where the mode simply is not used.
    #[test]
    fn vhf_only_modes_carry_no_hf_channels() {
        for t in [Tier::Msk144, Tier::Q65] {
            for c in band_plan_for(t) {
                assert!(
                    c.dial_mhz > 30.0,
                    "{t:?} has an HF channel at {} MHz ({})",
                    c.dial_mhz,
                    c.band
                );
            }
        }
    }

    /// FST4/FST4W are the converse: LF/MF, nothing above 160 m.
    #[test]
    fn fst4_family_is_lf_mf_only() {
        for t in [Tier::Fst4, Tier::Fst4w] {
            for c in band_plan_for(t) {
                assert!(
                    c.dial_mhz < 2.0,
                    "{t:?} has a channel at {} MHz ({}) above the MF range",
                    c.dial_mhz,
                    c.band
                );
            }
        }
    }
}
