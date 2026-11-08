//! US (FCC Part 97, ITU Region 2 / contiguous US) amateur transmit privileges by license
//! class — the data + logic behind the transmit lockout and the "jump to the start of my
//! licensed segment" band dropdown. Pure (no IO); heavily tested because it's a legal guard.
//!
//! Verified 2026-06-09 against the ARRL frequency-allocations table + 47 CFR §97.301/§97.305
//! (incl. the 60 m subband effective 2026-02-13). Conventions:
//! - CW (A1A) is allowed across the class's whole authorized span on a band (on a 60 m
//!   channel, only at its centre).
//! - DATA/RTTY (FT8/FT4 etc.) is allowed only in the no-phone lower segments on HF, but
//!   band-wide above 50 MHz (and on 30 m, all-data).
//! - PHONE/image is allowed only in the phone segments.
//!
//! The grandfathered Novice and Advanced classes (no new licences issued, existing ones renew)
//! were added 2026-10-25 from 47 CFR §97.301(c)/(e), §97.305(c) and §97.307(f)(9)/(10); their
//! power caps (§97.313(c)–(e)) are not modelled, as no class's are.
//!
//! `Open` = no restrictions (non-US operators); `tx_allowed` short-circuits to true. Its rows
//! are therefore a PARKING table, not a privilege one: [`OPEN_PARKING`], its own list, chosen
//! so every pick lands inside the amateur allocation of all three IARU regions.

use crate::settings::{LicenseClass, OperatingMode};

/// Every US licence class, in order of privilege, and the one list a test that claims to cover
/// every US class iterates, so a class added here is swept by all of them. `Open` is not a US
/// class: no US privilege model applies to it.
pub const US_CLASSES: [LicenseClass; 5] = [
    LicenseClass::Novice,
    LicenseClass::Technician,
    LicenseClass::General,
    LicenseClass::Advanced,
    LicenseClass::Extra,
];

/// One contiguous frequency segment and which emission types a class may use in it.
#[derive(Debug, Clone, Copy)]
struct Seg {
    lo: f64, // MHz, inclusive
    hi: f64, // MHz, exclusive
    cw: bool,
    data: bool,
    phone: bool,
}

const fn s(lo: f64, hi: f64, cw: bool, data: bool, phone: bool) -> Seg {
    Seg {
        lo,
        hi,
        cw,
        data,
        phone,
    }
}

// VHF/UHF privileges are identical for Technician and above; share them.
//
// ⚠️ EXTEND-ONLY against 47 CFR 97.301(a) — this table is the license-privilege TX
// gate, a load-bearing transmit-path invariant. The microwave rows (Batch 3, the
// QO-100 report) follow the regulation, not the band table: ADIF's "13cm" LABEL spans
// 2300–2450 MHz, but US amateurs hold only 2300–2310 and 2390–2450 — the 2310–2390 gap
// is not amateur spectrum, so the label is one band and the privilege is two segments.
// 9 cm (3300–3500) has NO row at all: the US amateur secondary allocation there was
// removed, so a US class must read TX LOCKED on it even though the band is labelled
// (labels serve tuning and honest logging; the Open class short-circuits above this
// table and is unaffected — QO-100's actual audience).
const VHF: &[Seg] = &[
    s(50.0, 50.1, true, false, false),       // 6 m CW only
    s(50.1, 54.0, true, true, true),         // 6 m all-mode
    s(144.0, 144.1, true, false, false),     // 2 m CW only
    s(144.1, 148.0, true, true, true),       // 2 m all-mode
    s(222.0, 225.0, true, true, true),       // 1.25 m all-mode
    s(420.0, 450.0, true, true, true),       // 70 cm all-mode
    s(902.0, 928.0, true, true, true),       // 33 cm all-mode
    s(1240.0, 1300.0, true, true, true),     // 23 cm all-mode (IC-9700's third band)
    s(2300.0, 2310.0, true, true, true),     // 13 cm lower segment (2310–2390 is NOT amateur)
    s(2390.0, 2450.0, true, true, true),     // 13 cm upper segment (QO-100 uplink territory)
    s(5650.0, 5925.0, true, true, true),     // 6 cm all-mode
    s(10_000.0, 10_500.0, true, true, true), // 3 cm all-mode (QO-100 downlink territory)
    s(24_000.0, 24_250.0, true, true, true), // 1.25 cm all-mode
];

// WHERE A BAND PICK PARKS FOR `Open` — the non-US and undeclared class, which Nexus cannot
// place in a region. ⚠️ A PARKING TABLE, NOT A PRIVILEGE CLAIM: `tx_allowed`, `allowed_spans`,
// `tx_span_allowed`, `cw_only_within`, `phone_segment`, `lsb_phone_allowed` and
// `wide_phone_allowed` all short-circuit `Open` before they read a segment, so all these rows
// decide is the dial and sideband a Phone or CW pick lands on (and so a band is never a pick
// that does nothing). They must never be folded into a US class's rows.
//
// Each band's rows sit inside the amateur allocation every IARU region shares, so the dial a
// pick lands on is amateur spectrum for a UK, a Japanese, an Australian and a US operator alike
// (ITU Radio Regulations Article 5; `open_parks_inside_the_amateur_allocation_of_every_region`
// pins it). The cw/data/phone split only places the phone home: phone starts where the IARU
// Region 1 HF band plan (2023 revision) starts it where that is higher than the shared bottom.
// National edges inside these spans still differ; the band-plan advice the UI shows and the
// operator's own licence carry that, this table cannot.
const OPEN_PARKING: &[Seg] = &[
    // 160 m from 1.810 (Region 1 starts there, the UK and Japan too); phone from 1.843, the
    // Region 1 plan's phone line, so the LSB home is 1.8458 with its passband on 1.843.
    s(1.810, 1.843, true, true, false),
    s(1.843, 1.850, true, true, true),
    // 80 m to 3.800, Region 1's top; phone from 3.600 as in the Region 1 plan.
    s(3.500, 3.600, true, true, false),
    s(3.600, 3.800, true, true, true),
    // 60 m: the WRC-15 secondary segment 5351.5-5366.5 kHz most of the world adopted. Phone
    // parks at 5.363 USB, inside the Region 1 plan's all-mode 5.354-5.366 and clear of the
    // worldwide FT8 dial 5.357, whose signals reach 5.360. CW lifts to 5.3525 through
    // `bandplan::cw_activity_mhz`.
    s(5.3515, 5.363, true, true, false),
    s(5.363, 5.3665, true, true, true),
    // 40 m to 7.200, Region 1's top. Phone from 7.125, which every region's plan allows.
    s(7.000, 7.125, true, true, false),
    s(7.125, 7.200, true, true, true),
    // 30 m: phone is legal under some non-US licences (Australia's among them), so it has a
    // home, at 10.120 USB: above the CW cluster, below FT8 (10.136) and WSPR (10.1387). Every
    // IARU plan keeps 30 m for CW and narrow data, and the UI's band-plan advice says so.
    s(10.100, 10.120, true, true, false),
    s(10.120, 10.150, true, true, true),
    s(14.000, 14.150, true, true, false), // 20 m
    s(14.150, 14.350, true, true, true),
    s(18.068, 18.110, true, true, false), // 17 m
    s(18.110, 18.168, true, true, true),
    s(21.000, 21.200, true, true, false), // 15 m
    s(21.200, 21.450, true, true, true),
    s(24.890, 24.930, true, true, false), // 12 m
    s(24.930, 24.990, true, true, true),
    s(28.000, 28.300, true, true, false), // 10 m
    s(28.300, 29.700, true, true, true),
    // 6 m to 52 MHz, Region 1's amateur top; 50.0-50.1 is CW and beacons in every plan.
    s(50.0, 50.1, true, false, false),
    s(50.1, 52.0, true, true, true),
    // 4 m: IARU Region 1's 69.9-70.5 MHz (CEPT secondary, footnote ECA9) — the US has no 4 m
    // allocation at any class, so this must never reach `extra()` (that would turn
    // `bandplan::no_us_license_class_may_key_a_4m_channel` red). A narrowband bottom slice
    // (beacons, WSPR at 70.091) under the all-mode segment; CW lifts to the 70.200 calling
    // frequency through `bandplan::cw_activity_mhz`. National edges vary by tens of kHz.
    s(70.0, 70.1, true, true, false),
    s(70.1, 70.5, true, true, true),
    // 2 m to 146 MHz, Region 1's top; 144.0-144.1 is CW in every plan.
    s(144.0, 144.1, true, false, false),
    s(144.1, 146.0, true, true, true),
    s(222.0, 225.0, true, true, true), // 1.25 m: Region 2 only
    // 70 cm inside 430-440 MHz, the Region 1 and Japanese allocation; phone from 432.100,
    // the bottom of the Region 1 plan's SSB segment.
    s(430.0, 432.1, true, true, false),
    s(432.1, 440.0, true, true, true),
    s(902.0, 928.0, true, true, true), // 33 cm: Region 2 only
    // 23 cm from 1260, where Japan's allocation starts; phone from 1296.150, the bottom of
    // the Region 1 plan's narrowband SSB segment. CW lifts to 1296.100.
    s(1260.0, 1296.15, true, true, false),
    s(1296.15, 1300.0, true, true, true),
    // 13 cm: 2400-2450 MHz, the part the US (2390-2450), the UK and Japan all hold, and the
    // QO-100 uplink's home.
    s(2400.0, 2450.0, true, true, true),
    // 9 cm: the Region 1 narrowband segment, 3400-3410 MHz. No US class has 9 cm.
    s(3400.0, 3410.0, true, true, true),
    s(5650.0, 5850.0, true, true, true), // 6 cm, to Region 1's top
    s(10_000.0, 10_500.0, true, true, true), // 3 cm
    s(24_000.0, 24_250.0, true, true, true), // 1.25 cm
];

// 60 m (General/Extra): the 5.3515–5.3665 subband, all-mode, + 4 channels, per 47 CFR
// 97.303(h)(3). Each channel is its centre ±1.4 kHz (the 2.8 kHz the rule allows) for data,
// which the gate judges at the emitted tone. Phone is judged as [carrier, carrier +
// 2.8 kHz], and the rule sets its carrier 1.5 kHz below the centre, so its row runs from that
// carrier to the channel's top: the rule's carrier keys, and no passband leaves the channel.
// CW is the rule's "the carrier frequency is set to the center frequency", so its row is the
// centre ± [`SIXTY_CW_TOL_MHZ`]. Phone on 60 m is upper sideband only ([`lsb_phone_allowed`]),
// never AM or FM ([`wide_phone_allowed`]).
// 60 m is channelized — excluded from the band dropdown but enforced by the lockout.
const SIXTY: &[Seg] = &[
    SIXTY_SEGMENT,
    s(5.3306, 5.3334, false, true, false), // ch 5.3320 data
    s(5.3305, 5.3334, false, false, true), // ch 5.3320 phone, carrier 5.3305
    sixty_cw(5.3320),                      // ch 5.3320 CW, at the centre
    s(5.3466, 5.3494, false, true, false), // ch 5.3480 data
    s(5.3465, 5.3494, false, false, true), // ch 5.3480 phone, carrier 5.3465
    sixty_cw(5.3480),                      // ch 5.3480 CW, at the centre
    s(5.3716, 5.3744, false, true, false), // ch 5.3730 data
    s(5.3715, 5.3744, false, false, true), // ch 5.3730 phone, carrier 5.3715
    sixty_cw(5.3730),                      // ch 5.3730 CW, at the centre
    s(5.4036, 5.4064, false, true, false), // ch 5.4050 data
    s(5.4035, 5.4064, false, false, true), // ch 5.4050 phone, carrier 5.4035
    sixty_cw(5.4050),                      // ch 5.4050 CW, at the centre
];

// 2200 m and 630 m (General, Advanced and Extra): 135.7–137.8 kHz and 472–479 kHz, every
// emission, per 47 CFR 97.301(b)–(d) and 97.305(c)
// (https://www.ecfr.gov/current/title-47/chapter-I/subchapter-D/part-97/subpart-D). A Technician
// and a Novice hold neither (97.301(e)). ⚠️ NOT MODELLED, as no power cap is: the 1 W EIRP
// (2200 m) and 5 W EIRP (630 m) limits of 97.313(k)/(l), and the 97.303(g) duty to notify the
// Utilities Technology Council at least 30 days before first transmitting. Those stay the
// operator's.
const LF_MF: &[Seg] = &[
    s(0.1357, 0.1378, true, true, true), // 2200 m all-mode
    s(0.472, 0.479, true, true, true),   // 630 m all-mode
];

/// 60 m's 5.3515–5.3665 MHz segment, all-mode. Every other [`SIXTY`] row is a channel's.
const SIXTY_SEGMENT: Seg = s(5.3515, 5.3665, true, true, true);

/// How far a dial may sit from a 60 m channel's upper-sideband dial, its phone carrier, and still
/// be on it ([`on_sixty_meter_channel_dial`]): 50 Hz either side, and half a hertz more, so no
/// whole-hertz dial lands on an edge, where float rounding would decide it.
const SIXTY_DIAL_TOL_MHZ: f64 = 0.000_050_5;

/// Is `mhz` one of 60 m's four channel dials: a channel's phone carrier, 1.5 kHz under its centre,
/// within [`SIXTY_DIAL_TOL_MHZ`]? The dial an upper-sideband program keys from on a channel
/// (operator ruling, 2026-10-25). Anywhere else inside a channel is not a channel dial, and
/// neither is the 5.3515–5.3665 MHz segment.
pub fn on_sixty_meter_channel_dial(mhz: f64) -> bool {
    SIXTY
        .iter()
        .filter(|seg| seg.phone && (seg.hi <= SIXTY_SEGMENT.lo || seg.lo >= SIXTY_SEGMENT.hi))
        .any(|seg| (mhz - seg.lo).abs() < SIXTY_DIAL_TOL_MHZ)
}

/// How far a CW carrier may sit from a 60 m channel's centre and still key: 5 Hz either side
/// for a whole-hertz frequency. A dial that moves in 10 Hz steps always has a step within 5 Hz
/// of the centre, wherever it started, and nothing narrower guarantees that. The edges sit half
/// a hertz further out, so no whole-hertz frequency lands on one, where float rounding would
/// decide it.
///
/// The gate judges the rig's own keyer at the dial, so it keys from 5 Hz below the centre to
/// 5 Hz above it. The soundcard keyer's tone sits one pitch from its dial, and the gate judges
/// that dial as CW too, so the soundcard keyer cannot key on a channel at all: no tolerance
/// narrower than its pitch (300–1200 Hz) holds both. Both keyers key in the segment as before.
const SIXTY_CW_TOL_MHZ: f64 = 0.000_005_5;

/// A 60 m channel's CW row: its centre ± [`SIXTY_CW_TOL_MHZ`], CW only.
const fn sixty_cw(centre: f64) -> Seg {
    s(
        centre - SIXTY_CW_TOL_MHZ,
        centre + SIXTY_CW_TOL_MHZ,
        true,
        false,
        false,
    )
}

fn technician() -> Vec<Seg> {
    // Technician HF is CW-ONLY on 80/40/15 m — the legacy Novice CW sub-bands. RTTY/data is
    // NOT granted there (§97.301(e); ARRL Volunteer Monitor flags Tech FT8 on these as a
    // violation). 10 m is the ONLY HF band where a Technician may run data (28.0–28.3).
    let mut v = vec![
        s(3.525, 3.600, true, false, false), // 80 m CW only (Tech: no data)
        s(7.025, 7.125, true, false, false), // 40 m CW only (Tech: no data)
        s(21.025, 21.200, true, false, false), // 15 m CW only (Tech: no data)
        s(28.000, 28.300, true, true, false), // 10 m CW/data (Tech DOES get data here)
        s(28.300, 28.500, true, false, true), // 10 m phone (Tech capped at 28.500)
    ];
    v.extend_from_slice(VHF);
    v
}

// The grandfathered classes, 47 CFR 97.301 (https://www.ecfr.gov/current/title-47/chapter-I/
// subchapter-D/part-97/subpart-D/section-97.301), cross-checked against the ARRL's US amateur band
// chart. No new Novice or Advanced licence is issued; existing ones still renew.
//
// ⚠️ TRANSMIT POWER IS NOT MODELLED, for these classes as for the others: the gate judges where
// and in what mode, never how loud. 47 CFR 97.313 caps a Novice at 200 W PEP on its HF segments
// ((c)(2), a Technician too), 25 W PEP on 1.25 m ((d)) and 5 W PEP on 23 cm ((e)); holding to
// those stays the operator's.

fn novice() -> Vec<Seg> {
    // 97.301(e) is ONE table for Novice and Technician, so the HF rows are the Technician's:
    // CW only on 80/40/15 m (97.307(f)(9)), RTTY/data on 10 m below 28.3 MHz and phone from 28.3
    // to 28.5 MHz (97.305(c)). From 28.3 MHz the phone is single sideband only (97.307(f)(10)),
    // which [`wide_phone_allowed`] enforces. Above 30 MHz a Novice holds only 222–225 MHz and
    // 1270–1295 MHz — none of 97.301(a)'s 6 m, 2 m, 70 cm or microwave bands — and every
    // emission 97.305(c) allows there (the 4(v) and 5(iii) rows).
    vec![
        s(3.525, 3.600, true, false, false),   // 80 m CW only
        s(7.025, 7.125, true, false, false),   // 40 m CW only
        s(21.025, 21.200, true, false, false), // 15 m CW only
        s(28.000, 28.300, true, true, false),  // 10 m CW/data
        s(28.300, 28.500, true, false, true),  // 10 m phone (SSB only), capped at 28.500
        s(222.0, 225.0, true, true, true),     // 1.25 m all-mode (25 W PEP, 97.313(d))
        s(1270.0, 1295.0, true, true, true), // 23 cm all-mode, 1270–1295 only (5 W PEP, 97.313(e))
    ]
}

fn advanced() -> Vec<Seg> {
    // 97.301(c): General's HF plus 3.700–3.800, 7.125–7.175, 14.175–14.225 and 21.225–21.275
    // MHz; the modes split where 97.305(c) splits them for every class (data below 7.125, 14.150
    // and 21.200, phone above). 3.500–3.525, 3.600–3.700, 7.000–7.025, 14.000–14.025,
    // 14.150–14.175, 21.000–21.025 and 21.200–21.225 are not Advanced spectrum. 160 m and
    // 30/17/12/10 m are General's rows, 60 m and VHF-up are every class's (97.301(a)).
    let mut v = vec![
        s(1.800, 2.000, true, true, true),    // 160 m all-mode
        s(3.525, 3.600, true, true, false),   // 80 m CW/data
        s(3.700, 4.000, true, false, true),   // 75 m phone (Advanced floor 3.700)
        s(7.025, 7.125, true, true, false),   // 40 m CW/data
        s(7.125, 7.300, true, false, true),   // 40 m phone (Advanced floor 7.125)
        s(10.100, 10.150, true, true, false), // 30 m CW/data (no phone, any class)
        s(14.025, 14.150, true, true, false), // 20 m CW/data
        s(14.175, 14.350, true, false, true), // 20 m phone (Advanced floor 14.175)
        s(18.068, 18.110, true, true, false), // 17 m CW/data
        s(18.110, 18.168, true, false, true), // 17 m phone
        s(21.025, 21.200, true, true, false), // 15 m CW/data
        s(21.225, 21.450, true, false, true), // 15 m phone (Advanced floor 21.225)
        s(24.890, 24.930, true, true, false), // 12 m CW/data
        s(24.930, 24.990, true, false, true), // 12 m phone
        s(28.000, 28.300, true, true, false), // 10 m CW/data
        s(28.300, 29.700, true, false, true), // 10 m phone
    ];
    v.extend_from_slice(LF_MF);
    v.extend_from_slice(SIXTY);
    v.extend_from_slice(VHF);
    v
}

fn general() -> Vec<Seg> {
    let mut v = vec![
        s(1.800, 2.000, true, true, true),    // 160 m all-mode
        s(3.525, 3.600, true, true, false),   // 80 m CW/data
        s(3.800, 4.000, true, false, true),   // 80 m phone
        s(7.025, 7.125, true, true, false),   // 40 m CW/data
        s(7.175, 7.300, true, false, true),   // 40 m phone
        s(10.100, 10.150, true, true, false), // 30 m CW/data (no phone, any class)
        s(14.025, 14.150, true, true, false), // 20 m CW/data
        s(14.225, 14.350, true, false, true), // 20 m phone
        s(18.068, 18.110, true, true, false), // 17 m CW/data
        s(18.110, 18.168, true, false, true), // 17 m phone
        s(21.025, 21.200, true, true, false), // 15 m CW/data
        s(21.275, 21.450, true, false, true), // 15 m phone
        s(24.890, 24.930, true, true, false), // 12 m CW/data
        s(24.930, 24.990, true, false, true), // 12 m phone
        s(28.000, 28.300, true, true, false), // 10 m CW/data
        s(28.300, 29.700, true, false, true), // 10 m phone
    ];
    v.extend_from_slice(LF_MF);
    v.extend_from_slice(SIXTY);
    v.extend_from_slice(VHF);
    v
}

fn extra() -> Vec<Seg> {
    let mut v = vec![
        s(1.800, 2.000, true, true, true),    // 160 m all-mode
        s(3.500, 3.600, true, true, false),   // 80 m CW/data (Extra bottom 3.500)
        s(3.600, 4.000, true, false, true),   // 80 m phone (Extra floor 3.600)
        s(7.000, 7.125, true, true, false),   // 40 m CW/data (Extra bottom 7.000)
        s(7.125, 7.300, true, false, true),   // 40 m phone (Extra floor 7.125)
        s(10.100, 10.150, true, true, false), // 30 m CW/data
        s(14.000, 14.150, true, true, false), // 20 m CW/data (Extra bottom 14.000)
        s(14.150, 14.350, true, false, true), // 20 m phone (Extra floor 14.150)
        s(18.068, 18.110, true, true, false), // 17 m CW/data
        s(18.110, 18.168, true, false, true), // 17 m phone
        s(21.000, 21.200, true, true, false), // 15 m CW/data (Extra bottom 21.000)
        s(21.200, 21.450, true, false, true), // 15 m phone (Extra floor 21.200)
        s(24.890, 24.930, true, true, false), // 12 m CW/data
        s(24.930, 24.990, true, false, true), // 12 m phone
        s(28.000, 28.300, true, true, false), // 10 m CW/data
        s(28.300, 29.700, true, false, true), // 10 m phone
    ];
    v.extend_from_slice(LF_MF);
    v.extend_from_slice(SIXTY);
    v.extend_from_slice(VHF);
    v
}

/// The privilege segments for a class. `Open`'s are [`OPEN_PARKING`], where a pick parks (the
/// lockout never consults them — it short-circuits Open to allowed), never a US privilege.
fn segments(class: LicenseClass) -> Vec<Seg> {
    match class {
        LicenseClass::Technician => technician(),
        LicenseClass::General => general(),
        LicenseClass::Extra => extra(),
        LicenseClass::Novice => novice(),
        LicenseClass::Advanced => advanced(),
        LicenseClass::Open => OPEN_PARKING.to_vec(),
    }
}

fn allows(seg: &Seg, mode: OperatingMode) -> bool {
    match mode {
        OperatingMode::Cw => seg.cw,
        // RTTY and the keyboard modes (PSK31…) are data emissions (§97.305 puts
        // RTTY and data in the same segments).
        OperatingMode::Digital | OperatingMode::Rtty | OperatingMode::Keyboard => seg.data,
        OperatingMode::Phone => seg.phone,
    }
}

/// May this class transmit `mode` at `emission_mhz` (the EMITTED RF, not the dial)? `Open`
/// always may. US classes: the emission must fall in a segment that authorizes the mode.
///
/// ⚠️ `Open` RETURNING `true` OFF EVERY NAMED BAND IS DELIBERATE — POLICY, NOT AN OVERSIGHT
/// (operator ruling, 2026-08-13; it has now been "corrected" toward a blanket off-band
/// refusal twice in review, so the reasoning lives here rather than in a thread).
///
/// The tempting change is "if the dial is off the band plan, refuse". It is wrong, and the
/// UK settles it: the UK 60 m allocation starts at **5.2585 MHz**, while `bandplan::
/// band_for_dial` names only 5.3–5.5 as "60m" — so a UK operator working their own legal
/// allocation is, by our table, off-band. The table is US-centric by construction (it is
/// built from 47 CFR 97.301 and ADIF's registered band names), and `Open` exists precisely
/// for the operators that table does not describe: every non-US licensee, plus anyone who
/// has not declared a class. Refusing off-band TX would block exactly them, on their own
/// legal frequencies, while protecting nobody — a US class already fails closed here on the
/// dial, because no segment matches.
///
/// So the division of labour is: **US classes are gated by this function; `Open` is trusted.**
/// What still protects an `Open` operator from an accidental off-band over is behavioural,
/// not this gate — `Engine::observe_rig_freq` halts transmit when the knob leaves the bands
/// (cutting an over in flight and dropping a latched key). That halt CUTS a transmission; it
/// does not prevent the next one, and it is not supposed to.
pub fn tx_allowed(class: LicenseClass, emission_mhz: f64, mode: OperatingMode) -> bool {
    if matches!(class, LicenseClass::Open) {
        return true;
    }
    segments(class)
        .iter()
        .any(|seg| emission_mhz >= seg.lo && emission_mhz < seg.hi && allows(seg, mode))
}

/// Where `class` may transmit `mode`: the [`segments`] that allow it, as `[lo, hi)` MHz spans,
/// ascending and merged where they touch or overlap — the licence-class band edges the scope
/// draws. `None` for `Open`, which [`tx_allowed`] allows everywhere, so it has no edge to draw.
///
/// THE GATE'S OWN DATA. [`tx_allowed`] judges one frequency against the rows, and
/// [`tx_span_allowed`] judges a signal with a width against these ranges; the test
/// `allowed_spans_agree_with_tx_allowed_for_every_class_and_mode` holds them and the rows to one
/// answer at every segment edge and across every band, so the overlay cannot show a privilege the
/// gate refuses or hide one it grants. Like the gate, it judges an EMISSION frequency, not a dial.
pub fn allowed_spans(class: LicenseClass, mode: OperatingMode) -> Option<Vec<(f64, f64)>> {
    if matches!(class, LicenseClass::Open) {
        return None;
    }
    let mut segs: Vec<(f64, f64)> = segments(class)
        .iter()
        .filter(|seg| allows(seg, mode))
        .map(|seg| (seg.lo, seg.hi))
        .collect();
    segs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(segs.len());
    for (lo, hi) in segs {
        match out.last_mut() {
            // Touching or overlapping: one span. Both ends are half-open like the gate's, so the
            // merged span holds exactly the frequencies its parts held.
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    Some(out)
}

/// May `class` transmit `mode` over the whole span from `lo_mhz` to `hi_mhz` — a signal with a
/// width (a phone passband, an RTTY mark/space pair) where [`tx_allowed`] judges one frequency?
/// Both ends must lie in ONE of the [`allowed_spans`] ranges, the class's rows for `mode` merged
/// where they touch or overlap. Two legal ends do not make the middle legal: a USB passband from
/// the top of the 5348.0 kHz channel ends inside the 5351.5–5366.5 kHz segment, and its middle
/// crosses the 2.1 kHz between them, which is not the class's. A signal across the boundary
/// between two touching rows lies in one range and is judged as it was. `Open` always may.
pub fn tx_span_allowed(class: LicenseClass, lo_mhz: f64, hi_mhz: f64, mode: OperatingMode) -> bool {
    let Some(spans) = allowed_spans(class, mode) else {
        return true;
    };
    let inside = |f: f64, (lo, hi): (f64, f64)| f >= lo && f < hi;
    spans
        .iter()
        .any(|&span| inside(lo_mhz, span) && inside(hi_mhz, span))
}

/// Does any frequency from `lo_mhz` to `hi_mhz` lie where `class` may send only CW: CW allowed,
/// neither data nor phone (a CW-only segment, 50.0–50.1 MHz for every class, a Technician's 80, 40
/// and 15 m)? `Open` has none. Each mode's [`allowed_spans`] is constant between its edges, so
/// the two ends, every edge between them and a point inside each piece answer for the whole span.
pub fn cw_only_within(class: LicenseClass, lo_mhz: f64, hi_mhz: f64) -> bool {
    let (Some(cw), Some(data), Some(phone)) = (
        allowed_spans(class, OperatingMode::Cw),
        allowed_spans(class, OperatingMode::Digital),
        allowed_spans(class, OperatingMode::Phone),
    ) else {
        return false;
    };
    let inside = |f: f64, spans: &[(f64, f64)]| spans.iter().any(|&(lo, hi)| f >= lo && f < hi);
    let mut points = vec![lo_mhz, hi_mhz];
    points.extend(
        cw.iter()
            .chain(&data)
            .chain(&phone)
            .flat_map(|&(lo, hi)| [lo, hi])
            .filter(|&f| f > lo_mhz && f < hi_mhz),
    );
    points.sort_by(f64::total_cmp);
    let middles: Vec<f64> = points.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
    points
        .iter()
        .chain(&middles)
        .any(|&f| inside(f, &cw) && !inside(f, &data) && !inside(f, &phone))
}

/// May `class` transmit a LOWER-sideband phone signal, its passband below the carrier at
/// `carrier_mhz`? Not on 60 m, for any US class: phone there is upper sideband only, on the four
/// channels and in the 5351.5–5366.5 kHz segment alike (operator ruling, 2026-10-14). The rule
/// names no sideband, but it sets a channel's phone carrier 1.5 kHz below the centre, which only
/// an upper-sideband passband puts on the channel. `Open` is trusted, as [`tx_allowed`] trusts it.
pub fn lsb_phone_allowed(class: LicenseClass, carrier_mhz: f64) -> bool {
    matches!(class, LicenseClass::Open)
        || crate::bandplan::band_for_dial(carrier_mhz) != Some("60m")
}

/// May `class` transmit a phone signal that is not ONE sideband, with its carrier at
/// `carrier_mhz`: AM, FM, or a mode word the phone model does not know? Not on 60 m, for any US
/// class, on the four channels and in the 5351.5–5366.5 kHz segment alike (operator ruling,
/// 2026-10-14). "For all 60 m spectrum, Amateur operators shall ensure that their emissions do not
/// occupy more than 2.8 kHz" (47 CFR 97.303(h)(3)), one sideband of voice. AM sends the voice on
/// both sides of its carrier, FM does too whatever its deviation, and a word the model does not
/// know is never trusted to fit. Every other band keeps them, and `Open` is trusted, as
/// [`tx_allowed`] trusts it.
///
/// A Novice and a Technician are refused them from 28.3 to 28.5 MHz too
/// ([`cw_and_ssb_voice_only`]).
pub fn wide_phone_allowed(class: LicenseClass, carrier_mhz: f64) -> bool {
    matches!(class, LicenseClass::Open)
        || (crate::bandplan::band_for_dial(carrier_mhz) != Some("60m")
            && !cw_and_ssb_voice_only(class, carrier_mhz))
}

/// Is `class` held to CW and single-sideband voice with its carrier at `carrier_mhz`? From 28.3 to
/// 28.5 MHz, for a Novice and a Technician: "A station having a control operator holding a Novice
/// Class operator license, a Technician Class operator license, or a Technician Plus Class
/// operator license may only transmit a CW emission using the international Morse code or phone
/// emissions J3E and R3E" (47 CFR 97.307(f)(10)). So AM and FM are refused there
/// ([`wide_phone_allowed`]), and so is a picture: SSTV is an image (J3F), not voice, though it
/// rides the same USB passband (`Engine::sstv_tx_gate`). Half-open like every segment, and it
/// judges only the emission: whether the frequency is the class's is [`tx_allowed`]'s question,
/// and from 28.3 to 28.5 MHz it is these two classes' whole 10 m phone segment. Every other class,
/// `Open` included, is never held to it.
pub fn cw_and_ssb_voice_only(class: LicenseClass, carrier_mhz: f64) -> bool {
    matches!(class, LicenseClass::Novice | LicenseClass::Technician)
        && (28.300..28.500).contains(&carrier_mhz)
}

/// Is upper sideband the only phone `class` may transmit with its carrier at `carrier_mhz`: LSB
/// refused ([`lsb_phone_allowed`]) and every word that is not one sideband refused too
/// ([`wide_phone_allowed`])? 60 m, for every US class. It decides nothing about keying, which those
/// two decide: the Phone cockpit greys its LSB, AM and FM picks where it holds, and the lock names
/// this rule rather than the frequency when the pick is all that stands in the way.
pub fn usb_only_phone(class: LicenseClass, carrier_mhz: f64) -> bool {
    !lsb_phone_allowed(class, carrier_mhz) && !wide_phone_allowed(class, carrier_mhz)
}

/// The lowest frequency (MHz) at which `class` may use `mode` on `band` — where a band
/// dropdown should park the VFO. `None` = the operator has no privilege for that band+mode
/// (the picker then lists the band receive-only). 60 m is excluded for a US class (channelized:
/// its lowest row is a channel, not a segment start); `Open` parks in the WRC-15 segment.
pub fn segment_start(class: LicenseClass, band: &str, mode: OperatingMode) -> Option<f64> {
    if band == "60m" && !matches!(class, LicenseClass::Open) {
        return None;
    }
    segments(class)
        .iter()
        .filter(|seg| allows(seg, mode) && crate::bandplan::band_for_dial(seg.lo) == Some(band))
        .map(|seg| seg.lo)
        .fold(None, |acc, lo| Some(acc.map_or(lo, |a: f64| a.min(lo))))
}

/// An SSB signal occupies ~2.8 kHz beside the carrier. THE passband width the whole app
/// judges phone by: the transmit gate measures the emission with it (`Engine::emission_allowed`)
/// and [`phone_home`] parks clear of a segment edge by it. ONE constant, because a home dial
/// computed against a narrower belief than the gate measures with is a dial the gate then
/// refuses — which is precisely the bug [`phone_home`] exists to end.
pub const SSB_BW_MHZ: f64 = 0.0028;

/// Where a PHONE band pick should park the dial for `class` on `band` — with the sideband
/// that band's convention uses. `None` = no phone privilege there (the band is omitted from
/// the dropdown), or no passband strictly inside the amateur allocation ([`inside_allocation`]).
///
/// ⚠️ NOT [`segment_start`], which is the EDGE. On the LSB bands (<10 MHz) the passband hangs
/// BELOW the dial, so parking ON the edge puts the lower 2.8 kHz outside the segment and the
/// transmit gate then refuses the very dial the band picker chose. Lifting the LSB home by one
/// passband is what makes the pick keyable. USB extends upward from `lo`, so a class's phone
/// FLOOR inside a band is clear there; a segment that starts at the BAND's edge (every US
/// segment from 1.25 m up, 3 cm's 10000.000 above all) is not, and parks on the band's
/// activity instead ([`crate::bandplan::activity_home_mhz`]).
///
/// WHY IT IS A FUNCTION AND NOT THREE COPIES (repair, 2026-08): the lift lived in
/// `Engine::mode_home` only. `Engine::band_pick_default` (the band dropdown's pick — the
/// `pick_band` path) and the `get_licensed_band_plan` command each recomputed the same home
/// from the bare `segment_start` and got an unkeyable dial: picking 40 m in Phone as an Extra
/// landed on 7.1250 LSB, whose passband starts at 7.1222 — below the 7.125 phone floor — so
/// the cockpit showed 🔒 TX LOCKED on a band the operator is fully licensed for. 160/80/40 m
/// are the LSB phone bands this bit. Every phone home in the app now comes from here.
pub fn phone_home(class: LicenseClass, band: &str) -> Option<(f64, &'static str)> {
    let lo = segment_start(class, band, OperatingMode::Phone)?;
    // WHERE THE PASSBAND STARTS: the band's phone ACTIVITY where it has one the class may key
    // (`bandplan::activity_home_mhz`: 10368.100 on 3 cm, not the 10000.000 band edge the QO-100
    // report was commanded to), else the segment start, as before.
    let floor = crate::bandplan::activity_home_mhz(class, band, OperatingMode::Phone)
        .filter(|&a| a >= lo && tx_span_allowed(class, a, a + SSB_BW_MHZ, OperatingMode::Phone))
        .unwrap_or(lo);
    // The band's sideband convention, 60 m's upper sideband included (`phone_is_lsb`).
    let (dial, sb) = if crate::bandplan::phone_is_lsb(floor) {
        (floor + SSB_BW_MHZ, "LSB")
    } else {
        (floor, "USB")
    };
    let (p_lo, p_hi) = if sb == "LSB" {
        (dial - SSB_BW_MHZ, dial)
    } else {
        (dial, dial + SSB_BW_MHZ)
    };
    // NEVER ON AN ALLOCATION EDGE, and never a passband the band cannot hold: 2200 m is 2.1 kHz
    // wide, so its lifted home was 138.5 kHz, off the band. No home is the honest answer there.
    inside_allocation(class, p_lo, p_hi).then_some((dial, sb))
}

/// Where a CW pick parks for `class` on `band`: the band's CW ACTIVITY (14.030, not the dead
/// 14.000 edge; `bandplan::activity_home_mhz` first, then `bandplan::cw_activity_mhz`), raised
/// to the licensed CW-segment start so it never drops below privileges. `None` = no CW
/// privilege there, or no home strictly inside the amateur allocation (the LF/MF bands, whose
/// only start is their edge). THE CW home: section entry and the band picker both read it.
pub fn cw_home(class: LicenseClass, band: &str) -> Option<f64> {
    let lo = segment_start(class, band, OperatingMode::Cw)?;
    let dial = crate::bandplan::activity_home_mhz(class, band, OperatingMode::Cw)
        .or_else(|| crate::bandplan::cw_activity_mhz(band))
        // Only an activity dial the class may key: 23 cm's 1296.100 is above a Novice's
        // 1270-1295, so a Novice parks on its own segment start instead.
        .filter(|&a| tx_allowed(class, a.max(lo), OperatingMode::Cw))
        .map_or(lo, |a| a.max(lo));
    inside_allocation(class, dial, dial).then_some(dial)
}

/// The amateur spectrum `class`'s regime holds, every mode, as merged `[lo, hi)` MHz spans:
/// [`OPEN_PARKING`] for `Open`, the union of every US class's rows otherwise.
fn regime_spans(class: LicenseClass) -> Vec<(f64, f64)> {
    let mut segs: Vec<(f64, f64)> = if matches!(class, LicenseClass::Open) {
        OPEN_PARKING.iter().map(|g| (g.lo, g.hi)).collect()
    } else {
        US_CLASSES
            .iter()
            .flat_map(|&c| segments(c))
            .map(|g| (g.lo, g.hi))
            .collect()
    };
    segs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in segs {
        match out.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// Does an emission from `lo_mhz` to `hi_mhz` lie STRICTLY inside the amateur spectrum of
/// `class`'s regime, touching neither end of it? The rule every home obeys: a home on an
/// allocation edge puts half a signal outside the band (and, through a station that maps its
/// dial to another frequency, as SDR Console maps a QO-100 downlink to its uplink, somewhere
/// that is not amateur spectrum at all). It judges WHERE A PICK PARKS, never keying, which
/// [`tx_allowed`] alone decides. A class's own phone floor inside the band (14.150 for an Extra)
/// is not an edge: the class's CW and data spectrum lies below it.
pub fn inside_allocation(class: LicenseClass, lo_mhz: f64, hi_mhz: f64) -> bool {
    const EPS: f64 = 1e-9;
    regime_spans(class)
        .iter()
        .any(|&(a, b)| lo_mhz > a + EPS && hi_mhz < b - EPS)
}

/// The PHONE (SSB/image) sub-band the operator may use on `band`, as an inclusive/exclusive
/// `[lo, hi)` MHz span for the band-strip's "where you may talk" shading. `None` when the class
/// has no phone privilege there, or for `Open` (non-US — no US sub-band model) and 60 m
/// (channelized).
///
/// LEGAL-HONESTY INVARIANT: this unions all of a class's phone segments on the band into ONE
/// span (lowest lo, highest hi). That is only honest while each band's phone privilege is a
/// SINGLE contiguous segment — true for every current US band, enforced by the test
/// `every_band_has_at_most_one_contiguous_phone_segment_per_class`. If a band is ever split into
/// disjoint phone sub-bands, this would shade the no-phone gap between them as legal; then this
/// must return a segment LIST and the strip must shade each separately.
pub fn phone_segment(class: LicenseClass, band: &str) -> Option<(f64, f64)> {
    // 13 cm is the one band whose phone privilege is genuinely DISJOINT (2300–2310 and
    // 2390–2450; the gap is not amateur spectrum). Unioning would shade 2310–2390 as
    // legal — the exact dishonesty the invariant above forbids — so like channelized
    // 60 m it gets no single-span shade. The TX lockout still enforces per segment.
    if band == "60m" || band == "13cm" || matches!(class, LicenseClass::Open) {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for seg in segments(class)
        .iter()
        .filter(|seg| seg.phone && crate::bandplan::band_for_dial(seg.lo) == Some(band))
    {
        lo = lo.min(seg.lo);
        hi = hi.max(seg.hi);
    }
    lo.is_finite().then_some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LicenseClass::*;
    use crate::settings::OperatingMode::{Cw, Digital, Keyboard, Phone, Rtty};

    /// The exact case that leaked onto the Needed board: LU6HL spotted on 7.140 SSB. 7.140 sits
    /// in the 40 m phone segment a GENERAL may not use (their phone starts at 7.175) but an EXTRA
    /// may (Extra phone from 7.125). The board's privilege gate keys off precisely this.
    #[test]
    fn general_may_not_work_7140_phone_but_extra_may() {
        assert!(
            !tx_allowed(General, 7.140, Phone),
            "7.140 is Extra-only 40m phone"
        );
        assert!(
            tx_allowed(Extra, 7.140, Phone),
            "Extra phone reaches down to 7.125"
        );
        // And the segment boundary the case turns on: 7.175 is where General phone begins.
        assert!(!tx_allowed(General, 7.174, Phone));
        assert!(tx_allowed(General, 7.175, Phone));
    }

    #[test]
    fn open_allows_everything() {
        assert!(tx_allowed(Open, 14.000, Phone)); // even the Extra-only bottom, even phone there
        assert!(tx_allowed(Open, 7.200, Cw));
        assert!(tx_allowed(Open, 5.000, Digital)); // off any US band — Open doesn't care
    }

    #[test]
    fn technician_hf_is_cw_only_on_80_40_15_and_data_only_on_10m() {
        assert!(tx_allowed(Technician, 3.550, Cw)); // 80 m CW ok
                                                    // 80/40/15 m are CW ONLY for a Technician — FT8/RTTY there is a Part 97 violation.
        assert!(!tx_allowed(Technician, 3.573, Digital)); // 80 m FT8 NOT legal for Tech
        assert!(!tx_allowed(Technician, 7.074, Digital)); // 40 m FT8 NOT legal for Tech
        assert!(!tx_allowed(Technician, 21.074, Digital)); // 15 m FT8 NOT legal for Tech
        assert!(!tx_allowed(Technician, 3.850, Phone)); // no 80 m phone for Tech
        assert!(!tx_allowed(Technician, 14.074, Digital)); // no 20 m at all for Tech
                                                           // 10 m is the only HF band where a Technician may run data.
        assert!(tx_allowed(Technician, 28.074, Digital)); // 10 m FT8 ok
        assert!(tx_allowed(Technician, 28.400, Phone)); // 10 m phone ok
        assert!(!tx_allowed(Technician, 28.600, Phone)); // Tech 10 m phone capped at 28.500
    }

    #[test]
    fn vhf_is_full_for_technician_incl_data_band_wide() {
        assert!(tx_allowed(Technician, 50.313, Digital)); // 6 m FT8 (in the all-mode segment)
        assert!(tx_allowed(Technician, 144.174, Digital)); // 2 m FT8
        assert!(tx_allowed(Technician, 1296.174, Digital)); // 23 cm FT8 (IC-9700)
        assert!(tx_allowed(Technician, 1296.100, Phone)); // 23 cm SSB
        assert!(tx_allowed(Technician, 146.520, Phone)); // 2 m phone
        assert!(!tx_allowed(Technician, 50.050, Digital)); // 6 m 50.0–50.1 is CW-only
        assert!(tx_allowed(Technician, 50.050, Cw)); // ...but CW is fine there
    }

    #[test]
    fn general_vs_extra_phone_floors_and_bottoms() {
        // 20 m phone floor: Extra 14.150, General 14.225.
        assert!(tx_allowed(Extra, 14.150, Phone));
        assert!(!tx_allowed(General, 14.150, Phone));
        assert!(tx_allowed(General, 14.225, Phone));
        // Extra-only CW bottoms (e.g. 14.000–14.025).
        assert!(tx_allowed(Extra, 14.010, Cw));
        assert!(!tx_allowed(General, 14.010, Cw)); // General CW floor 14.025
        assert!(tx_allowed(General, 14.030, Cw));
        // 40 m phone floor: Extra 7.125, General 7.175.
        assert!(tx_allowed(Extra, 7.130, Phone));
        assert!(!tx_allowed(General, 7.130, Phone));
    }

    #[test]
    fn rtty_rides_the_data_segments() {
        use crate::settings::OperatingMode::Rtty;
        assert!(tx_allowed(General, 14.083, Rtty)); // 20 m RTTY window: CW/data segment
        assert!(!tx_allowed(General, 14.300, Rtty)); // never in a phone segment
        assert!(!tx_allowed(Technician, 3.583, Rtty)); // Tech 80 m is CW-only — no RTTY
        assert!(tx_allowed(Technician, 28.083, Rtty)); // 10 m: the one Tech HF data grant
    }

    #[test]
    fn thirty_meters_is_data_cw_only_no_phone_any_class() {
        assert!(tx_allowed(General, 10.136, Digital)); // 30 m FT8 ok
        assert!(tx_allowed(Extra, 10.130, Cw));
        assert!(!tx_allowed(General, 10.130, Phone)); // never phone on 30 m
        assert!(!tx_allowed(Technician, 10.130, Cw)); // Tech not authorized on 30 m
    }

    #[test]
    fn emission_edge_is_half_open_inclusive_low() {
        // Exactly the floor is allowed; just below is not (the emission, not the dial).
        assert!(tx_allowed(General, 14.225, Phone));
        assert!(!tx_allowed(General, 14.2249, Phone));
        // Just below the upper edge is allowed; the upper edge itself is not.
        assert!(tx_allowed(General, 14.349, Phone));
        assert!(!tx_allowed(General, 14.350, Phone));
    }

    #[test]
    fn every_phone_home_is_a_dial_its_class_may_actually_key() {
        // THE contract that binds every phone band picker in the app. A picker parks the dial;
        // the transmit gate then judges the EMISSION — the whole passband, both edges, LSB
        // hanging below the dial. A home the gate refuses is a band the operator is licensed
        // for and cannot key, which is what shipped while two of the three pickers recomputed
        // the home from the bare `segment_start` (Extra picking 40 m → 7.1250, whose passband
        // opens at 7.1222, under the 7.125 phone floor → 🔒 TX LOCKED).
        const BANDS: &[&str] = &[
            "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "2m", "1.25m",
            "70cm", "23cm",
        ];
        let mut homes = 0;
        for class in US_CLASSES.into_iter().chain([Open]) {
            for band in BANDS {
                let Some((dial, sb)) = phone_home(class, band) else {
                    continue;
                };
                homes += 1;
                assert_eq!(
                    sb,
                    if dial < 10.0 { "LSB" } else { "USB" },
                    "{class:?} {band}: the home's sideband must follow the band's convention"
                );
                // The same span `Engine::emission_allowed` measures for Phone.
                let (lo, hi) = if dial < 10.0 {
                    (dial - SSB_BW_MHZ, dial)
                } else {
                    (dial, dial + SSB_BW_MHZ)
                };
                assert!(
                    tx_allowed(class, lo, Phone) && tx_allowed(class, hi, Phone),
                    "{class:?} {band}: the phone home {dial:.4} emits over [{lo:.4}, {hi:.4}], \
                     which leaves the phone segment"
                );
            }
        }
        assert!(
            homes > 20,
            "the sweep must actually have found homes to check"
        );
        // …and the LSB lift is a real move, not a rounding: 160/80/40 m are the bands where
        // the passband hangs below the dial.
        assert_eq!(phone_home(Extra, "40m"), Some((7.125 + SSB_BW_MHZ, "LSB")));
        assert_eq!(
            phone_home(General, "40m"),
            Some((7.175 + SSB_BW_MHZ, "LSB"))
        );
        assert_eq!(phone_home(Extra, "20m"), Some((14.150, "USB"))); // USB: already clear
        assert_eq!(phone_home(Technician, "20m"), None); // no privilege, no home
    }

    /// The `Open` bands that used to have no home, so a pick of them did nothing: 30 m in Phone
    /// (the table `Open` borrowed was the US one, which has no 30 m phone row) and 9 cm in Phone
    /// and CW (no US 9 cm row at all), plus 60 m, which no picker listed. Each now parks, and
    /// none of it reaches a US class, whose 60 m stays channelised and out of `segment_start`.
    #[test]
    fn open_parks_on_30m_phone_9cm_and_60m() {
        assert_eq!(phone_home(Open, "30m"), Some((10.120, "USB")));
        // 3400.100, the narrowband calling frequency, not the 3400.0 parking edge (no home
        // is an allocation edge: `inside_allocation`). The parking start itself is unchanged.
        assert_eq!(phone_home(Open, "9cm"), Some((3400.1, "USB")));
        assert_eq!(segment_start(Open, "9cm", Cw), Some(3400.0));
        // 60 m is upper sideband everywhere, so its phone home is USB though it sits below 10 MHz.
        assert_eq!(phone_home(Open, "60m"), Some((5.363, "USB")));
        assert_eq!(segment_start(Open, "60m", Cw), Some(5.3515));
        // Controls: no US class gains a 60 m segment start, a 30 m phone home or a 9 cm one.
        for class in US_CLASSES {
            for mode in [Phone, Cw, Digital] {
                assert_eq!(
                    segment_start(class, "60m", mode),
                    None,
                    "{class:?} {mode:?}"
                );
                assert_eq!(
                    segment_start(class, "9cm", mode),
                    None,
                    "{class:?} {mode:?}"
                );
            }
            assert_eq!(phone_home(class, "30m"), None, "{class:?}");
            assert!(!tx_allowed(class, 10.120, Phone), "{class:?} 30 m phone");
        }
    }

    /// Where `Open` parks must be amateur spectrum in EVERY IARU region, because Nexus has no
    /// region setting: a pick has to be right for a UK, a Japanese, an Australian and a US
    /// operator alike. The spans are the ITU Radio Regulations Article 5 amateur allocations
    /// common to Regions 1, 2 and 3 (160 m from 1.810 and 80 m to 3.800 are Region 1's edges,
    /// 40 m to 7.200 too, 6 m to 52 MHz, 2 m to 146 MHz, 70 cm 430–440 MHz). 60 m is the WRC-15
    /// secondary segment. 13 cm is narrowed to 2400–2450 MHz, the part the US (2390–2450), the
    /// UK and Japan (2400–2450) all hold. Bands that exist in one region only (4 m, 1.25 m,
    /// 33 cm, 9 cm) are checked against that region's allocation. Both ends of a phone passband
    /// are checked, not just the dial.
    #[test]
    fn open_parks_inside_the_amateur_allocation_of_every_region() {
        const COMMON: &[(&str, f64, f64)] = &[
            ("160m", 1.810, 1.850),
            ("80m", 3.500, 3.800),
            ("60m", 5.3515, 5.3665),
            ("40m", 7.000, 7.200),
            ("30m", 10.100, 10.150),
            ("20m", 14.000, 14.350),
            ("17m", 18.068, 18.168),
            ("15m", 21.000, 21.450),
            ("12m", 24.890, 24.990),
            ("10m", 28.000, 29.700),
            ("6m", 50.000, 52.000),
            ("4m", 70.000, 70.500),
            ("2m", 144.000, 146.000),
            ("1.25m", 222.000, 225.000),
            ("70cm", 430.000, 440.000),
            ("33cm", 902.000, 928.000),
            ("23cm", 1240.000, 1300.000),
            ("13cm", 2400.000, 2450.000),
            ("9cm", 3400.000, 3410.000),
            ("6cm", 5650.000, 5850.000),
            ("3cm", 10_000.000, 10_500.000),
            ("1.25cm", 24_000.000, 24_250.000),
        ];
        let mut checked = 0;
        for mode in [Phone, Cw] {
            for row in crate::bandplan::licensed_bands(Open, mode) {
                let &(_, lo, hi) = COMMON
                    .iter()
                    .find(|(b, _, _)| *b == row.band)
                    .unwrap_or_else(|| panic!("{} has no common span in this test", row.band));
                let (a, b) = match (mode, row.mode.as_str()) {
                    (Phone, "LSB") => (row.dial_mhz - SSB_BW_MHZ, row.dial_mhz),
                    (Phone, _) => (row.dial_mhz, row.dial_mhz + SSB_BW_MHZ),
                    _ => (row.dial_mhz, row.dial_mhz),
                };
                assert!(
                    a >= lo && b < hi,
                    "Open {mode:?} {} parks over [{a:.4}, {b:.4}], outside [{lo}, {hi})",
                    row.band
                );
                checked += 1;
            }
        }
        assert_eq!(
            checked,
            2 * COMMON.len(),
            "every band is listed in both modes"
        );
    }

    /// 2200 m (135.7-137.8 kHz) and 630 m (472-479 kHz) belong to a US General, Advanced and
    /// Extra, all emissions: 47 CFR 97.301(b)-(d) and 97.305(c). They read TX LOCKED on legal
    /// spectrum while the tables had no rows. A Technician and a Novice hold neither (97.301(e)),
    /// and both edges are half-open like every segment.
    #[test]
    fn us_general_advanced_and_extra_hold_2200m_and_630m() {
        for class in [General, Advanced, Extra] {
            for mode in [Cw, Digital, Rtty, Keyboard, Phone] {
                for f in [0.1357, 0.1360, 0.13779, 0.472, 0.4742, 0.47899] {
                    assert!(tx_allowed(class, f, mode), "{class:?} {mode:?} at {f}");
                }
                for f in [0.13569, 0.1378, 0.47199, 0.479, 0.502] {
                    assert!(!tx_allowed(class, f, mode), "{class:?} {mode:?} at {f}");
                }
            }
        }
        // CONTROLS: Technician and Novice stay locked on both bands, every mode.
        for class in [Technician, Novice] {
            for mode in [Cw, Digital, Rtty, Keyboard, Phone] {
                for f in [0.1357, 0.1360, 0.472, 0.4742] {
                    assert!(!tx_allowed(class, f, mode), "{class:?} {mode:?} at {f}");
                }
            }
        }
    }

    /// ⛔ NAMING MORE BANDS NEVER WIDENS A US PRIVILEGE. The band table now names 60 m from 5.06
    /// MHz, 8 m and 5 m; no US class holds any of that spectrum, and its gate, its spans and its
    /// sideband rules must say so at every point. On 60 m below the US segment, LSB and wide phone
    /// are refused too (the 60 m rule now sees it), which only ever adds a refusal.
    #[test]
    fn the_newly_named_spectrum_grants_no_us_class_anything() {
        let mut points = Vec::new();
        for (lo, hi) in [(5.06, 5.3305), (40.0, 45.0), (54.0, 69.9)] {
            let mut f = lo;
            while f < hi {
                points.push(f);
                f += (hi - lo) / 500.0;
            }
        }
        for class in US_CLASSES {
            for &f in &points {
                for mode in [Cw, Digital, Rtty, Keyboard, Phone] {
                    assert!(!tx_allowed(class, f, mode), "{class:?} {mode:?} at {f:.4}");
                    assert!(
                        !tx_span_allowed(class, f, f + SSB_BW_MHZ, mode),
                        "{class:?} {mode:?} span at {f:.4}"
                    );
                }
                if f < 5.5 {
                    assert!(!lsb_phone_allowed(class, f), "{class:?} LSB at {f:.4}");
                    assert!(!wide_phone_allowed(class, f), "{class:?} AM/FM at {f:.4}");
                }
            }
        }
        // Open is trusted there as everywhere.
        assert!(tx_allowed(Open, 5.2585, Phone) && lsb_phone_allowed(Open, 5.2585));
    }

    #[test]
    fn segment_start_for_the_band_dropdown() {
        assert_eq!(segment_start(Extra, "20m", Phone), Some(14.150));
        assert_eq!(segment_start(General, "20m", Phone), Some(14.225));
        assert_eq!(segment_start(Technician, "20m", Phone), None); // Tech has no 20 m
        assert_eq!(segment_start(Technician, "80m", Cw), Some(3.525));
        assert_eq!(segment_start(Extra, "80m", Cw), Some(3.500));
        assert_eq!(segment_start(Technician, "10m", Phone), Some(28.300));
        assert_eq!(segment_start(Technician, "10m", Cw), Some(28.000));
        // Open uses the conventional (Extra) starts.
        assert_eq!(segment_start(Open, "20m", Phone), Some(14.150));
        // 60 m is channelized — never in the dropdown.
        assert_eq!(segment_start(General, "60m", Phone), None);
    }

    #[test]
    fn phone_segment_span_for_the_band_strip_shade() {
        assert_eq!(phone_segment(Extra, "20m"), Some((14.150, 14.350)));
        assert_eq!(phone_segment(General, "20m"), Some((14.225, 14.350))); // General phone floor higher
        assert_eq!(phone_segment(Technician, "20m"), None); // Tech has no 20 m
        assert_eq!(phone_segment(Technician, "10m"), Some((28.300, 28.500))); // Tech 10 m phone cap
        assert_eq!(phone_segment(General, "30m"), None); // 30 m: no phone, any class
        assert_eq!(phone_segment(Open, "20m"), None); // non-US: the strip skips shading
        assert_eq!(phone_segment(General, "60m"), None); // channelized
    }

    #[test]
    fn every_band_has_at_most_one_contiguous_phone_segment_per_class() {
        // Guards `phone_segment`'s union-into-one-span: if the tables ever split a band's phone
        // privilege into DISJOINT segments, the strip would shade the no-phone gap as legal. This
        // fails loudly at that point so the shading stays legally honest.
        use std::collections::BTreeMap;
        for class in US_CLASSES {
            let mut by_band: BTreeMap<&'static str, Vec<(f64, f64)>> = BTreeMap::new();
            for seg in segments(class).iter().filter(|s| s.phone) {
                if let Some(b) = crate::bandplan::band_for_dial(seg.lo) {
                    by_band.entry(b).or_default().push((seg.lo, seg.hi));
                }
            }
            for (band, mut segs) in by_band {
                // 60 m is channelized (5 discrete phone channels) and 13 cm's privilege is
                // genuinely disjoint (2300–2310 / 2390–2450, the gap is not amateur) —
                // both legitimately multi-segment, and `phone_segment` early-returns None
                // for both, so the union never runs there.
                if band == "60m" || band == "13cm" {
                    continue;
                }
                segs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                for w in segs.windows(2) {
                    assert!(
                        w[0].1 >= w[1].0,
                        "{class:?} {band}: disjoint phone segments {:?} and {:?} — phone_segment() \
                         would bridge the gap; return a segment list and shade each instead",
                        w[0],
                        w[1]
                    );
                }
            }
        }
    }

    /// Every frequency the agreement test asks about: each segment edge of every class (on it and
    /// a hair either side), each segment's middle, and sweeps across every band — 1 kHz through
    /// HF and 6 m, 100 Hz across 60 m's 2.8 kHz channels, 50 kHz from 4 m up.
    fn privilege_samples() -> Vec<f64> {
        let mut f = Vec::new();
        for class in US_CLASSES.into_iter().chain([Open]) {
            for seg in segments(class) {
                for edge in [seg.lo, seg.hi] {
                    f.extend([edge, edge - 1e-6, edge + 1e-6]);
                }
                f.push((seg.lo + seg.hi) / 2.0);
            }
        }
        f.extend((1_700..=54_100).map(|k| k as f64 / 1_000.0));
        f.extend((53_000..=54_100).map(|k| k as f64 / 10_000.0));
        for (lo, hi) in [
            (69.0, 71.0),
            (143.0, 149.0),
            (221.0, 226.0),
            (419.0, 451.0),
            (901.0, 929.0),
            (1_239.0, 1_301.0),
            (2_299.0, 2_451.0),
            (3_299.0, 3_501.0),
            (5_649.0, 5_926.0),
            (9_999.0, 10_501.0),
            (23_999.0, 24_251.0),
        ] {
            let n = ((hi - lo) / 0.05_f64).round() as usize;
            f.extend((0..=n).map(|i| lo + i as f64 * 0.05));
        }
        f.extend([0.0, 0.5, 60.0, 100.0, 30_000.0]);
        f
    }

    /// THE BAND-EDGE OVERLAY SHOWS THE GATE, never a copy of it: for every class and every section,
    /// a frequency sits inside one of `allowed_spans` exactly where `tx_allowed` lets that class key
    /// that emission there. `Open` has no spans, and its gate allows everything.
    #[test]
    fn allowed_spans_agree_with_tx_allowed_for_every_class_and_mode() {
        let samples = privilege_samples();
        for class in US_CLASSES.into_iter().chain([Open]) {
            for mode in [Digital, Phone, Cw, Rtty, Keyboard] {
                let spans = allowed_spans(class, mode);
                if matches!(class, Open) {
                    assert_eq!(spans, None, "Open has no edges");
                    assert!(samples.iter().all(|&f| tx_allowed(class, f, mode)));
                    continue;
                }
                let spans = spans.expect("a US class has spans");
                for &f in &samples {
                    let inside = spans.iter().any(|&(lo, hi)| f >= lo && f < hi);
                    assert_eq!(
                        inside,
                        tx_allowed(class, f, mode),
                        "{class:?} {mode:?} at {f} MHz: the overlay and the gate disagree"
                    );
                }
            }
        }
    }

    #[test]
    fn allowed_spans_merge_touching_segments_and_keep_the_gaps() {
        let extra_cw = allowed_spans(Extra, Cw).unwrap();
        // 80 m: CW/data 3.500–3.600 and phone 3.600–4.000 both carry CW, so one span.
        assert!(extra_cw.contains(&(3.500, 4.000)), "{extra_cw:?}");
        assert!(extra_cw.contains(&(14.000, 14.350)));
        // 6 m: the CW-only slice and the all-mode segment touch at 50.1.
        assert!(extra_cw.contains(&(50.0, 54.0)));
        let general_phone = allowed_spans(General, Phone).unwrap();
        assert!(
            general_phone.contains(&(14.225, 14.350)),
            "{general_phone:?}"
        );
        assert!(general_phone.contains(&(7.175, 7.300)));
        // 60 m's channels stay apart: the gaps between them are no privilege.
        assert!(general_phone.contains(&(5.3305, 5.3334)));
        let tech_data = allowed_spans(Technician, Digital).unwrap();
        // Data on HF only on 10 m, and band-wide from 6 m up.
        assert!(tech_data.contains(&(28.000, 28.300)), "{tech_data:?}");
        assert!(tech_data.contains(&(50.1, 54.0)));
        assert!(tech_data.iter().all(|&(lo, _)| lo >= 28.0));
        for spans in [extra_cw, general_phone, tech_data] {
            assert!(
                spans.windows(2).all(|w| w[0].1 < w[1].0),
                "ascending, with a gap between each: {spans:?}"
            );
        }
        // RTTY and the keyboard modes ride the data segments, as the gate judges them.
        assert_eq!(
            allowed_spans(General, Rtty),
            allowed_spans(General, Digital)
        );
        assert_eq!(
            allowed_spans(General, Keyboard),
            allowed_spans(General, Digital)
        );
    }

    #[test]
    fn sixty_meters_is_enforced_even_though_not_in_the_dropdown() {
        assert!(tx_allowed(General, 5.3590, Phone)); // inside the new subband
        assert!(tx_allowed(General, 5.3320, Phone)); // a legacy channel center
        assert!(!tx_allowed(General, 5.3400, Phone)); // between channels → blocked
        assert!(!tx_allowed(Technician, 5.3590, Phone)); // Tech not authorized on 60 m
    }

    /// 47 CFR 97.303(h)(3): on a 60 m channel the phone carrier is set 1.5 kHz below the centre,
    /// so phone keys from that carrier to the channel's top, while data keeps the channel and CW
    /// its centre.
    #[test]
    fn sixty_meter_phone_keys_from_the_rules_carrier_and_data_keeps_the_channel() {
        // (carrier, centre, the channel's top) in MHz, as literals so no sum rounds across an edge.
        for (carrier, centre, top) in [
            (5.3305, 5.3320, 5.3334),
            (5.3465, 5.3480, 5.3494),
            (5.3715, 5.3730, 5.3744),
            (5.4035, 5.4050, 5.4064),
        ] {
            for class in [General, Extra] {
                assert!(
                    tx_allowed(class, carrier, Phone),
                    "{class:?} phone carrier {carrier}"
                );
                assert!(
                    tx_allowed(class, carrier + SSB_BW_MHZ, Phone),
                    "{class:?} {carrier}"
                );
                assert!(
                    !tx_allowed(class, carrier - 0.0001, Phone),
                    "{class:?} {carrier}"
                );
                assert!(
                    !tx_allowed(class, top, Phone),
                    "{class:?} phone at the top {top}"
                );
                assert!(!tx_allowed(class, carrier, Cw), "{class:?} CW at {carrier}");
                assert!(
                    !tx_allowed(class, carrier, Digital),
                    "{class:?} data at {carrier}"
                );
                assert!(tx_allowed(class, centre, Cw), "{class:?} CW at {centre}");
                assert!(
                    tx_allowed(class, centre, Digital),
                    "{class:?} data at {centre}"
                );
            }
            assert!(
                !tx_allowed(Technician, carrier, Phone),
                "Technician at {carrier}"
            );
        }
    }

    /// 47 CFR 97.303(h)(3): "For CW emissions (emission designator 150HA1A), the carrier frequency
    /// is set to the center frequency." So CW keys within 5 Hz of a channel's centre and nowhere
    /// else on the channel, while data keeps the whole channel and CW keeps the whole segment.
    #[test]
    fn sixty_meter_cw_keys_at_a_channel_centre_and_nowhere_else_on_the_channel() {
        // Whole hertz, the way a dial reads back.
        let mhz = |hz: i64| hz as f64 / 1e6;
        for centre in [5_332_000, 5_348_000, 5_373_000, 5_405_000] {
            for class in [General, Extra] {
                for hz in [centre, centre - 5, centre + 5] {
                    assert!(tx_allowed(class, mhz(hz), Cw), "{class:?} CW at {hz} Hz");
                }
                for hz in [
                    centre - 6,
                    centre + 6,
                    centre - 500,
                    centre + 500,
                    centre - 1_300,
                ] {
                    assert!(!tx_allowed(class, mhz(hz), Cw), "{class:?} CW at {hz} Hz");
                    assert!(
                        tx_allowed(class, mhz(hz), Digital),
                        "{class:?} data at {hz} Hz"
                    );
                }
            }
            assert!(
                !tx_allowed(Technician, mhz(centre), Cw),
                "Technician at {centre} Hz"
            );
        }
        for hz in [5_351_500, 5_360_000, 5_366_499] {
            assert!(
                tx_allowed(General, mhz(hz), Cw),
                "CW in the segment at {hz} Hz"
            );
        }
    }

    /// A signal is judged against ONE contiguous range. Its two ends can each be legal while its
    /// middle is not: a USB passband from the top of the 5348.0 kHz channel ends inside the
    /// segment, across the 2.1 kHz between them. Touching rows are one range.
    #[test]
    fn a_signal_must_lie_in_one_contiguous_range() {
        for class in [General, Extra] {
            for carrier in [5.3487, 5.3490, 5.3493] {
                let top = carrier + SSB_BW_MHZ;
                assert!(
                    tx_allowed(class, carrier, Phone) && tx_allowed(class, top, Phone),
                    "{class:?} {carrier}: both ends are legal on their own"
                );
                assert!(
                    !tx_span_allowed(class, carrier, top, Phone),
                    "{class:?} {carrier}: the passband crosses the gap"
                );
            }
            // Inside one channel, inside the segment: allowed.
            assert!(tx_span_allowed(class, 5.3465, 5.3493, Phone));
            assert!(tx_span_allowed(class, 5.3600, 5.3600 + SSB_BW_MHZ, Phone));
            // An RTTY pair across the same gap.
            assert!(!tx_span_allowed(class, 5.3491, 5.3516, Rtty));
            assert!(tx_span_allowed(class, 5.3575, 5.3600, Rtty));
        }
        // 80 m: Extra's CW/data row ends at 3.600 and its phone row, which also carries CW,
        // starts there. One range, so a span across the boundary is allowed, as both ends are.
        assert!(tx_span_allowed(Extra, 3.5995, 3.6005, Cw));
        assert!(tx_span_allowed(Extra, 14.1495, 14.1505, Cw));
        // ...and a span reaching past a range's top is not, whatever its bottom.
        assert!(!tx_span_allowed(General, 14.3480, 14.3508, Phone));
        assert!(!tx_span_allowed(Technician, 5.3600, 5.3628, Phone));
        // The order of the two ends does not matter, and a point is judged as `tx_allowed` judges it.
        assert!(!tx_span_allowed(General, 5.3515, 5.3487, Phone));
        for f in [5.3305, 5.3334, 5.3400, 14.2249, 14.225, 14.3499, 14.350] {
            assert_eq!(
                tx_span_allowed(General, f, f, Phone),
                tx_allowed(General, f, Phone),
                "{f}"
            );
        }
        assert!(tx_span_allowed(Open, 5.3487, 5.3515, Phone));
    }

    /// 60 m phone is upper sideband only, on the channels and in the segment, for every US class.
    /// Every other band keeps its lower sideband, and `Open` is trusted.
    #[test]
    fn lsb_phone_is_refused_anywhere_on_60_m() {
        for class in US_CLASSES {
            for carrier in [
                5.3305, 5.3333, 5.3493, 5.3543, 5.3600, 5.3636, 5.3743, 5.4063,
            ] {
                assert!(
                    !lsb_phone_allowed(class, carrier),
                    "{class:?} LSB at {carrier}"
                );
            }
            for carrier in [1.850, 3.900, 7.200, 14.300] {
                assert!(
                    lsb_phone_allowed(class, carrier),
                    "{class:?} LSB at {carrier}"
                );
            }
        }
        assert!(lsb_phone_allowed(Open, 5.3600));
    }

    /// 47 CFR 97.303(h)(3): "For all 60 m spectrum, Amateur operators shall ensure that their
    /// emissions do not occupy more than 2.8 kHz." AM and FM send the voice on both sides of the
    /// carrier, so neither keys anywhere on 60 m for a US class. Every other band keeps them, FM's
    /// own bands included, and `Open` is trusted.
    #[test]
    fn am_and_fm_phone_are_refused_anywhere_on_60_m() {
        for class in [Technician, General, Advanced, Extra] {
            for carrier in [
                5.3305, 5.3333, 5.3465, 5.3515, 5.3600, 5.3664, 5.3715, 5.4035, 5.4063,
            ] {
                assert!(
                    !wide_phone_allowed(class, carrier),
                    "{class:?} AM/FM at {carrier}"
                );
            }
            for carrier in [
                1.885, 3.885, 7.290, 14.286, 29.600, 52.525, 146.520, 446.000,
            ] {
                assert!(
                    wide_phone_allowed(class, carrier),
                    "{class:?} AM/FM at {carrier}"
                );
            }
        }
        assert!(wide_phone_allowed(Open, 5.3600));
    }

    /// One row of a golden table: a frequency (MHz, the emission), a section, and whether the
    /// class may key it there.
    type Golden = (f64, OperatingMode, bool);

    fn check_golden(class: LicenseClass, table: &[Golden]) {
        for &(f, mode, want) in table {
            assert_eq!(
                tx_allowed(class, f, mode),
                want,
                "{class:?} {mode:?} at {f} MHz: want {}",
                if want { "allowed" } else { "refused" }
            );
        }
    }

    /// ⛔ THE NOVICE CLASS, edge by edge — 47 CFR 97.301(e) for the segments, 97.305(c) and
    /// 97.307(f)(9) for the modes (CW only on 80/40/15 m; data on 10 m only below 28.3 MHz, phone
    /// from 28.3 to 28.5 MHz), and nothing from 6 m, 2 m or 70 cm, which 97.301(a) gives
    /// Technicians and up but not Novices. Each segment's floor keys, a hair under it does not,
    /// a hair under its top keys and its top does not (the gate is half-open, `[lo, hi)`).
    #[test]
    fn novice_golden_edges() {
        check_golden(
            Novice,
            &[
                // 80 m, 3.525–3.600, CW only (97.307(f)(9)).
                (3.524_999, Cw, false),
                (3.525, Cw, true),
                (3.599_999, Cw, true),
                (3.600, Cw, false),
                (3.573, Digital, false),
                (3.580, Rtty, false),
                (3.580, Keyboard, false),
                (3.550, Phone, false),
                // 40 m, 7.025–7.125, CW only.
                (7.024_999, Cw, false),
                (7.025, Cw, true),
                (7.124_999, Cw, true),
                (7.125, Cw, false),
                (7.074, Digital, false),
                (7.080, Rtty, false),
                (7.100, Phone, false),
                // 15 m, 21.025–21.200, CW only.
                (21.024_999, Cw, false),
                (21.025, Cw, true),
                (21.199_999, Cw, true),
                (21.200, Cw, false),
                (21.074, Digital, false),
                (21.150, Phone, false),
                // 10 m, 28.0–28.5: CW across it, RTTY/data to 28.3, phone from 28.3.
                (27.999_999, Cw, false),
                (28.000, Cw, true),
                (28.499_999, Cw, true),
                (28.500, Cw, false),
                (27.999_999, Digital, false),
                (28.000, Digital, true),
                (28.074, Digital, true),
                (28.080, Rtty, true),
                (28.120, Keyboard, true),
                (28.299_999, Digital, true),
                (28.300, Digital, false),
                (28.299_999, Phone, false),
                (28.300, Phone, true),
                (28.400, Phone, true),
                (28.499_999, Phone, true),
                (28.500, Phone, false),
                (28.600, Phone, false),
                // 1.25 m, 222–225, every section.
                (221.999_999, Cw, false),
                (222.000, Cw, true),
                (222.000, Digital, true),
                (222.000, Phone, true),
                (223.500, Phone, true),
                (224.999_999, Phone, true),
                (225.000, Phone, false),
                (225.000, Digital, false),
                // 23 cm, 1270–1295 only, every section (Technicians hold 1240–1300).
                (1_240.000, Cw, false),
                (1_269.999_999, Phone, false),
                (1_270.000, Cw, true),
                (1_270.000, Digital, true),
                (1_270.000, Phone, true),
                (1_294.999_999, Phone, true),
                (1_295.000, Phone, false),
                (1_296.100, Phone, false),
                (1_296.174, Digital, false),
                // Nothing a Novice was never granted.
                (1.840, Cw, false),
                (5.3320, Cw, false),
                (5.3600, Phone, false),
                (10.136, Digital, false),
                (14.025, Cw, false),
                (14.074, Digital, false),
                (18.100, Digital, false),
                (24.915, Digital, false),
                (29.600, Phone, false),
                (50.050, Cw, false),
                (50.313, Digital, false),
                (144.174, Digital, false),
                (146.520, Phone, false),
                (446.000, Phone, false),
                (902.100, Phone, false),
                (2_304.100, Phone, false),
                (10_368.100, Phone, false),
            ],
        );
    }

    /// ⛔ THE ADVANCED CLASS, edge by edge — 47 CFR 97.301(c) for the segments and 97.305(c)
    /// for the modes. Everything General has, plus 3.700–3.800 phone, 7.025–7.175 (data below
    /// 7.125, phone above it, so 7.125–7.175 is phone), 14.175–14.225 phone and 21.225–21.275
    /// phone. The Extra-only bottoms (3.500, 7.000, 14.000, 21.000) and the 3.600–3.700 and
    /// 14.150–14.175 / 21.200–21.225 gaps are refused.
    #[test]
    fn advanced_golden_edges() {
        check_golden(
            Advanced,
            &[
                // 160 m, all-mode.
                (1.799_999, Cw, false),
                (1.800, Cw, true),
                (1.800, Digital, true),
                (1.800, Phone, true),
                (1.999_999, Phone, true),
                (2.000, Phone, false),
                // 80/75 m: 3.525–3.600 CW/data, nothing 3.600–3.700, 3.700–4.000 phone (+CW).
                (3.500, Cw, false),
                (3.524_999, Cw, false),
                (3.525, Cw, true),
                (3.525, Digital, true),
                (3.599_999, Rtty, true),
                (3.600, Digital, false),
                (3.600, Cw, false),
                (3.650, Cw, false),
                (3.650, Phone, false),
                (3.699_999, Phone, false),
                (3.700, Phone, true),
                (3.700, Cw, true),
                (3.750, Phone, true),
                (3.750, Digital, false),
                (3.999_999, Phone, true),
                (4.000, Phone, false),
                // 60 m, as General: the segment and the four channels.
                (5.3305, Phone, true),
                (5.3400, Phone, false),
                (5.3600, Digital, true),
                (5.3480, Cw, true),
                // 40 m: 7.025–7.125 CW/data, 7.125–7.300 phone (+CW).
                (7.000, Cw, false),
                (7.024_999, Cw, false),
                (7.025, Cw, true),
                (7.074, Digital, true),
                (7.124_999, Digital, true),
                (7.125, Digital, false),
                (7.124_999, Phone, false),
                (7.125, Phone, true),
                (7.150, Phone, true),
                (7.299_999, Phone, true),
                (7.300, Phone, false),
                (7.300, Cw, false),
                // 30 m: CW/data, never phone.
                (10.100, Digital, true),
                (10.136, Digital, true),
                (10.149_999, Cw, true),
                (10.150, Cw, false),
                (10.130, Phone, false),
                // 20 m: 14.025–14.150 CW/data, nothing 14.150–14.175, 14.175–14.350 phone (+CW).
                (14.000, Cw, false),
                (14.024_999, Cw, false),
                (14.025, Cw, true),
                (14.074, Digital, true),
                (14.149_999, Digital, true),
                (14.150, Digital, false),
                (14.150, Cw, false),
                (14.160, Phone, false),
                (14.174_999, Phone, false),
                (14.175, Phone, true),
                (14.175, Cw, true),
                (14.200, Phone, true),
                (14.349_999, Phone, true),
                (14.350, Phone, false),
                // 17 m, as General.
                (18.068, Digital, true),
                (18.109_999, Digital, true),
                (18.110, Digital, false),
                (18.110, Phone, true),
                (18.167_999, Phone, true),
                (18.168, Phone, false),
                // 15 m: 21.025–21.200 CW/data, nothing 21.200–21.225, 21.225–21.450 phone (+CW).
                (21.000, Cw, false),
                (21.024_999, Cw, false),
                (21.025, Digital, true),
                (21.199_999, Digital, true),
                (21.200, Digital, false),
                (21.200, Cw, false),
                (21.224_999, Phone, false),
                (21.225, Phone, true),
                (21.250, Phone, true),
                (21.449_999, Phone, true),
                (21.450, Phone, false),
                // 12 m, as General.
                (24.890, Digital, true),
                (24.929_999, Digital, true),
                (24.929_999, Phone, false),
                (24.930, Phone, true),
                (24.989_999, Phone, true),
                (24.990, Phone, false),
                // 10 m, as General.
                (28.000, Digital, true),
                (28.300, Digital, false),
                (28.300, Phone, true),
                (29.600, Phone, true),
                (29.699_999, Phone, true),
                (29.700, Phone, false),
                // VHF and up, as Technician and up (97.301(a)).
                (50.050, Digital, false),
                (50.313, Digital, true),
                (144.174, Digital, true),
                (222.100, Phone, true),
                (1_296.100, Phone, true),
                (2_320.000, Phone, false),
            ],
        );
    }

    /// The grandfathered classes nest where the rule nests them: a Novice holds nothing a
    /// Technician does not (97.301(e) is one table for both, and 97.301(a) adds only to the
    /// Technician), an Advanced holds everything a General does and nothing an Extra does not.
    #[test]
    fn novice_and_advanced_nest_between_their_neighbours() {
        let samples = privilege_samples();
        for mode in [Digital, Phone, Cw, Rtty, Keyboard] {
            for &f in &samples {
                let (n, t) = (tx_allowed(Novice, f, mode), tx_allowed(Technician, f, mode));
                assert!(
                    !n || t,
                    "Novice {mode:?} at {f} MHz, which a Technician may not"
                );
                let (g, a, e) = (
                    tx_allowed(General, f, mode),
                    tx_allowed(Advanced, f, mode),
                    tx_allowed(Extra, f, mode),
                );
                assert!(
                    !g || a,
                    "General {mode:?} at {f} MHz, which an Advanced may not"
                );
                assert!(
                    !a || e,
                    "Advanced {mode:?} at {f} MHz, which an Extra may not"
                );
            }
        }
    }

    /// 47 CFR 97.307(f)(10): from 28.3 to 28.5 MHz a Novice may send only CW and single-sideband
    /// voice (J3E, R3E). AM and FM are refused there; a single sideband either way keys, and the
    /// 1.25 m and 23 cm phone stays all-mode.
    #[test]
    fn novice_ten_meter_phone_is_single_sideband_only() {
        for carrier in [28.300, 28.400, 28.497] {
            assert!(!wide_phone_allowed(Novice, carrier), "AM/FM at {carrier}");
            assert!(lsb_phone_allowed(Novice, carrier), "LSB at {carrier}");
            assert!(
                !usb_only_phone(Novice, carrier),
                "either sideband at {carrier}"
            );
        }
        for carrier in [222.100, 223.500, 1_270.100, 1_294.000] {
            assert!(wide_phone_allowed(Novice, carrier), "AM/FM at {carrier}");
        }
    }

    /// 47 CFR 97.307(f)(10) binds a Novice and a Technician alike from 28.3 to 28.5 MHz, edge by
    /// edge (half-open, as every segment is), and no other class: AM and FM are refused there, a
    /// single sideband either way is not, and General, Advanced, Extra and `Open` keep them.
    #[test]
    fn ten_meter_cw_and_ssb_voice_only_binds_novice_and_technician_alone() {
        for (f, held) in [
            (28.299_999, false),
            (28.300, true),
            (28.400, true),
            (28.499_999, true),
            (28.500, false),
            (28.500_001, false),
        ] {
            for class in [Novice, Technician] {
                assert_eq!(cw_and_ssb_voice_only(class, f), held, "{class:?} at {f}");
                if held {
                    assert!(!wide_phone_allowed(class, f), "{class:?} AM/FM at {f}");
                    assert!(lsb_phone_allowed(class, f), "{class:?} LSB at {f}");
                }
            }
            for class in [General, Advanced, Extra, Open] {
                assert!(!cw_and_ssb_voice_only(class, f), "{class:?} at {f}");
                assert!(wide_phone_allowed(class, f), "{class:?} AM/FM at {f}");
            }
        }
    }

    /// Every band a US class may be judged on, the band dropdown's and the gate's.
    const GOLDEN_BANDS: &[&str] = &[
        "160m", "80m", "60m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "4m", "2m",
        "1.25m", "70cm", "33cm", "23cm", "13cm", "9cm", "6cm", "3cm", "1.25cm",
    ];

    /// Everything the gate answers for `class`, as text: the ranges it lets each section key in
    /// (which `allowed_spans_agree_with_tx_allowed_for_every_class_and_mode` holds equal to
    /// `tx_allowed`), each band's dropdown starts, phone home and phone shade, and the 60 m
    /// sideband rules at carriers on and off the band.
    fn golden_text(class: LicenseClass) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        for mode in [Digital, Phone, Cw, Rtty, Keyboard] {
            writeln!(
                out,
                "{class:?} {mode:?} spans {:?}",
                allowed_spans(class, mode)
            )
            .unwrap();
        }
        for band in GOLDEN_BANDS {
            let starts: Vec<_> = [Digital, Phone, Cw, Rtty, Keyboard]
                .map(|m| segment_start(class, band, m))
                .into();
            writeln!(
                out,
                "{class:?} {band} starts {starts:?} home {:?} shade {:?}",
                phone_home(class, band),
                phone_segment(class, band)
            )
            .unwrap();
        }
        for carrier in [
            1.885, 3.885, 5.3305, 5.3600, 7.200, 14.300, 28.400, 29.600, 146.520,
        ] {
            writeln!(
                out,
                "{class:?} {carrier} lsb {} wide {} usb_only {}",
                lsb_phone_allowed(class, carrier),
                wide_phone_allowed(class, carrier),
                usb_only_phone(class, carrier)
            )
            .unwrap();
        }
        out
    }

    /// ⛔ **THE CLASSES THAT SHIPPED ANSWER EXACTLY AS THEY DID** — byte for byte. The file was
    /// written by THIS test against the gate as it stood before the Novice and Advanced classes
    /// existed, with `NEXUS_WRITE_PRIVILEGE_GOLDENS=1`; the default mode compares today's gate
    /// with it. Regenerating it against today's gate would make this test prove nothing: a
    /// deliberate change to a shipped class is a new golden AND a sentence in its commit saying
    /// why. One row has moved so: `Technician 28.4 … wide false`, AM and FM refused to a
    /// Technician from 28.3 to 28.5 MHz by 47 CFR 97.307(f)(10) ([`cw_and_ssb_voice_only`]).
    /// And ten rows grew, General's and Extra's `spans` in every section: each gained 2200 m
    /// (0.1357–0.1378) and 630 m (0.472–0.479), 47 CFR 97.301(b)/(d) ([`LF_MF`]); nothing else
    /// on those rows moved. And the `home` column moved where it sat on a band's own edge (the
    /// QO-100 report, 10000.000 on 3 cm): 160 m and 1.25 m up now park on the band's phone
    /// activity ([`crate::bandplan::activity_home_mhz`]). Every `spans`, `starts`, `shade` and
    /// sideband row is byte-identical: no privilege moved.
    #[test]
    fn the_shipped_classes_answer_byte_for_byte_as_before() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/privileges-us-classes.golden"
        );
        let got: String = [Technician, General, Extra].map(golden_text).concat();
        if std::env::var_os("NEXUS_WRITE_PRIVILEGE_GOLDENS").is_some() {
            std::fs::write(path, &got).expect("golden written");
            return;
        }
        let want = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("{path}: {e} (the golden is written by the gate before the new classes)")
        });
        assert!(got.lines().count() > 90, "the golden covers every band");
        for (n, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "line {}: a shipped class's privileges changed", n + 1);
        }
        assert_eq!(got, want, "a shipped class's privileges changed");
    }
}
