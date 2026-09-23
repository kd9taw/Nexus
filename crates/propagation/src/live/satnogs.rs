//! SatNOGS DB adapter (the `live` feature) — which amateur birds are still
//! alive and what transmitters they carry, keyed by NORAD catalog number.
//!
//! The pure orbital geometry in [`crate::sat`] answers "where is the bird and
//! when does it pass"; this answers "is it worth chasing and on what frequency".
//! Two endpoints of the SatNOGS DB:
//!   - `/api/satellites/?format=json` — per-bird `norad_cat_id`, `name`, and a
//!     `status` string (`alive` | `dead` | `re-entered` | `future`).
//!   - `/api/transmitters/?format=json` — per-transmitter `description`, `alive`
//!     flag, `mode`, `uplink_low`/`downlink_low` Hz, and the owning
//!     `norad_cat_id`.
//!
//! The lists are large and change slowly, so the app fetches the FULL list once
//! (weekly is plenty) and filters to the operator's tracked birds client-side —
//! one request, kinder to the API than N per-satellite queries. The parse halves
//! are pure and unit-tested; the fetch returns `Err` on trouble so the caller
//! keeps its cache rather than fabricating a transmitter plan.
//!
//! Data from the SatNOGS DB (<https://db.satnogs.org>), licensed CC-BY-SA 4.0.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

const SATELLITES_URL: &str = "https://db.satnogs.org/api/satellites/?format=json";
const TRANSMITTERS_URL: &str = "https://db.satnogs.org/api/transmitters/?format=json";
const UA: &str = "nexus-propagation/0.1 (+ham radio satellite operating)";

/// A bird's operational status: its catalog number, name, and the SatNOGS
/// `status` verbatim (`alive` | `dead` | `re-entered` | `future`) — kept as the
/// source string rather than an enum so an unseen value degrades to a plain
/// label instead of being dropped.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SatStatus {
    pub norad: u32,
    pub name: String,
    pub status: String,
}

/// One transmitter/transponder on a bird: what it is, whether it is currently
/// operational, its mode, and its uplink/downlink centre frequencies in Hz. The
/// frequencies and mode are legitimately absent on some records (a receive-only
/// beacon has no uplink; an uncharacterised one no mode) and stay `None` — never
/// a fabricated 0.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transmitter {
    pub norad: u32,
    pub description: String,
    pub alive: bool,
    pub mode: Option<String>,
    pub uplink_low_hz: Option<u64>,
    pub downlink_low_hz: Option<u64>,
    /// ⭐ LOAD-BEARING FOR DOPPLER. On an INVERTING linear transponder the
    /// uplink runs BACKWARDS relative to the downlink and the sidebands swap
    /// (LSB up / USB down). Tuning up the band moves your uplink DOWN. A
    /// station that gets this wrong lands on someone else entirely, which is
    /// why it must be per-transponder DATA and never a global setting.
    /// `false` when SatNOGS doesn't say — non-inverting is the safe reading
    /// for the FM/beacon majority.
    pub invert: bool,
    /// Passband edges. A linear transponder is a BAND, not a frequency: the
    /// centre-only model cannot express "the operator is 12 kHz up the
    /// passband". `None` for single-frequency transmitters.
    pub uplink_high_hz: Option<u64>,
    pub downlink_high_hz: Option<u64>,
    /// Per-leg modes. The single `mode` field above cannot express the
    /// USB-down/LSB-up an inverting transponder needs.
    pub uplink_mode: Option<String>,
    pub downlink_mode: Option<String>,
    /// SatNOGS `type`: "Transmitter" (beacon, downlink only), "Transponder"
    /// (linear, a passband), "Transceiver" (FM repeater). The three drive
    /// genuinely different operating behaviour, so the distinction is kept
    /// rather than inferred from which frequencies happen to be present.
    pub kind: Option<String>,
    /// SatNOGS `baud`: the symbol rate of the DOWNLINK — it qualifies `mode`,
    /// never `uplink_mode` (one live bird sends AFSK down at 1200 with a CW
    /// uplink). `f64` because SatNOGS sends a float and a few rates are
    /// genuinely fractional (an RTTY beacon's 45.45). `None` when it is not
    /// known — null, absent, junk, and the 0.0 some records carry (mostly CW
    /// beacons) — never a fabricated rate of 0.
    pub baud: Option<f64>,
}

impl Transmitter {
    /// True when this is a LINEAR transponder — a passband to tune inside,
    /// not a fixed channel. Derived from the declared type first, falling back
    /// to "it has an uplink passband wider than a channel".
    pub fn is_linear(&self) -> bool {
        if let Some(k) = &self.kind {
            return k.eq_ignore_ascii_case("Transponder");
        }
        matches!(
            (self.downlink_low_hz, self.downlink_high_hz),
            (Some(lo), Some(hi)) if hi > lo + 10_000
        )
    }

    /// What the RADIO must be put in to work this transmitter — FM, or the
    /// linear path on the sideband the record declares.
    ///
    /// Per-leg mode FIRST for the same reason [`Transmitter::uplink_mode`]
    /// exists: the single `mode` field cannot describe an inverting
    /// transponder's two legs, and it is the DOWNLINK the rig demodulates. The
    /// classification itself lives in [`tempo_core::doppler::downlink_class`] —
    /// one map, shared with the routing class and the commanded rig mode, so
    /// the three cannot disagree about the same bird.
    ///
    /// ⭐ A PUBLISHED BAND PLAN OUTRANKS AN FM TAG. SatNOGS tags every live QO-100
    /// narrowband segment FM, where the band plan allows no FM, so a downlink inside a
    /// range [`tempo_core::doppler::band_plan_linear_downlink`] names is read as the
    /// linear class — USB, the side that transponder is worked on — never FM. Asked HERE,
    /// where the class is read, and not when the catalogue is parsed: the on-disk snapshot
    /// can be a week old, and it must not carry a stale answer to this.
    pub fn downlink_class(&self) -> tempo_core::doppler::DownlinkClass {
        let class = tempo_core::doppler::downlink_class(
            self.downlink_mode.as_deref().or(self.mode.as_deref()),
        );
        if class.is_fm()
            && self
                .downlink_centre_hz()
                .is_some_and(tempo_core::doppler::band_plan_linear_downlink)
        {
            return tempo_core::doppler::DownlinkClass::Usb;
        }
        class
    }

    /// True when the RADIO must be in FM to work this transmitter — the FM/AFSK
    /// repeater and packet birds (SO-50, AO-91, the ISS APRS digipeater), as
    /// opposed to the linear/SSB majority. The FM half of
    /// [`Self::downlink_class`], derived from it so the two cannot drift.
    pub fn is_fm(&self) -> bool {
        self.downlink_class().is_fm()
    }

    /// ⭐ True when the uplink takes **CW over an FM-class downlink** — the one shape whose
    /// TX mode cannot come from the downlink. KOSEN-1's onboard SDR (NORAD 49402) takes CW on
    /// 21.125–21.150 MHz and relays AFSK on 435.525 MHz, so deriving the uplink from the
    /// downlink put the TX VFO in FM on a CW-only 15 m segment. It is the only such record in
    /// the live catalogue (2026-09-23: ten records list a CW uplink, the other nine over a CW
    /// downlink, which is linear-class).
    ///
    /// Both halves are needed. A CW uplink over a LINEAR downlink (AO-7's "Lin CW", QO-100's
    /// "CW Only" segments) is CW through a passband, worked like any linear bird. An FM-class
    /// downlink with any other uplink is an FM bird. An absent or empty `uplink_mode` is
    /// `false`, so a record that says nothing changes nothing.
    ///
    /// This names the MODE only, never a SIDE: some inverting records list USB up, so the
    /// engine derives any sideband itself rather than reading one here.
    pub fn uplink_is_cw_only(&self) -> bool {
        self.is_fm()
            && self
                .uplink_mode
                .as_deref()
                .is_some_and(tempo_core::doppler::mode_is_cw)
    }

    /// Centre of the downlink passband (or the single downlink frequency).
    pub fn downlink_centre_hz(&self) -> Option<u64> {
        centre(self.downlink_low_hz, self.downlink_high_hz)
    }

    /// Centre of the uplink passband (or the single uplink frequency).
    pub fn uplink_centre_hz(&self) -> Option<u64> {
        centre(self.uplink_low_hz, self.uplink_high_hz)
    }
}

fn centre(lo: Option<u64>, hi: Option<u64>) -> Option<u64> {
    match (lo, hi) {
        (Some(l), Some(h)) if h > l => Some(l + (h - l) / 2),
        (Some(l), _) => Some(l),
        _ => None,
    }
}

/// Parse the `/api/satellites` array into [`SatStatus`]. Pure — unit-testable
/// without the network. `norad_cat_id`, `name`, and `status` are all required; an
/// entry missing any of them isn't a usable status record and is skipped (never
/// invented). Non-array/garbage input yields an empty vec.
pub fn parse_satellites(json: &str) -> Vec<SatStatus> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let (Some(norad), Some(name), Some(status)) = (
            item.get("norad_cat_id").and_then(Value::as_u64),
            item.get("name").and_then(Value::as_str),
            item.get("status").and_then(Value::as_str),
        ) else {
            continue;
        };
        out.push(SatStatus {
            norad: norad as u32,
            name: name.to_string(),
            status: status.to_string(),
        });
    }
    out
}

/// Parse the `/api/transmitters` array into [`Transmitter`]. Pure — unit-testable
/// without the network. `norad_cat_id`, `description`, and `alive` are required;
/// `mode`, `uplink_low`, and `downlink_low` degrade to `None` when null/absent.
/// Non-array/garbage input yields an empty vec.
pub fn parse_transmitters(json: &str) -> Vec<Transmitter> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let (Some(norad), Some(description), Some(alive)) = (
            item.get("norad_cat_id").and_then(Value::as_u64),
            item.get("description").and_then(Value::as_str),
            item.get("alive").and_then(Value::as_bool),
        ) else {
            continue;
        };
        out.push(Transmitter {
            norad: norad as u32,
            description: description.to_string(),
            alive,
            mode: item.get("mode").and_then(Value::as_str).map(str::to_string),
            uplink_low_hz: item.get("uplink_low").and_then(Value::as_u64),
            downlink_low_hz: item.get("downlink_low").and_then(Value::as_u64),
            // Absent/garbage `invert` reads as NON-inverting: that is the safe
            // default (the FM/beacon majority), and a wrongly-inverted uplink
            // transmits somewhere the operator never intended.
            invert: item.get("invert").and_then(Value::as_bool).unwrap_or(false),
            uplink_high_hz: item.get("uplink_high").and_then(Value::as_u64),
            downlink_high_hz: item.get("downlink_high").and_then(Value::as_u64),
            uplink_mode: item
                .get("uplink_mode")
                .and_then(Value::as_str)
                .map(str::to_string),
            downlink_mode: item
                .get("downlink_mode")
                .and_then(Value::as_str)
                .map(str::to_string),
            kind: item.get("type").and_then(Value::as_str).map(str::to_string),
            // Only a positive, finite number is a rate; anything else is unknown.
            baud: item
                .get("baud")
                .and_then(Value::as_f64)
                .filter(|b| b.is_finite() && *b > 0.0),
        });
    }
    out
}

/// GET `url` as text with the SatNOGS-etiquette client. `Err` on network/HTTP
/// trouble so callers keep their cache.
fn fetch_text(url: &str) -> Result<String, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60)) // full lists over slow shack DSL
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())?;
    c.get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

/// A 200 whose body isn't a JSON array is an API-shape surprise, not data —
/// surface it as `Err` so the caller keeps its cache instead of writing an
/// empty-but-"fresh" snapshot over good data. (The pure `parse_*` fns stay
/// forgiving; this gate is a FETCH-path contract.)
fn ensure_json_array(json: &str) -> Result<(), String> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Array(_)) => Ok(()),
        Ok(_) => Err("SatNOGS response was not a JSON array (API shape change?)".to_string()),
        Err(e) => Err(format!("SatNOGS response was not valid JSON: {e}")),
    }
}

/// Fetch every satellite's status, then keep only those whose NORAD id is in
/// `norad`. One request; filtered client-side. An empty `norad` yields an empty
/// vec (pass the birds you track). `Err` on network/HTTP trouble.
pub fn fetch_satellites(norad: &[u32]) -> Result<Vec<SatStatus>, String> {
    let json = fetch_text(SATELLITES_URL)?;
    ensure_json_array(&json)?;
    Ok(parse_satellites(&json)
        .into_iter()
        .filter(|s| norad.contains(&s.norad))
        .collect())
}

/// Fetch every transmitter, then keep only those whose owning NORAD id is in
/// `norad`. One request; filtered client-side. An empty `norad` yields an empty
/// vec (pass the birds you track). `Err` on network/HTTP trouble.
pub fn fetch_transmitters(norad: &[u32]) -> Result<Vec<Transmitter>, String> {
    let json = fetch_text(TRANSMITTERS_URL)?;
    ensure_json_array(&json)?;
    Ok(parse_transmitters(&json)
        .into_iter()
        .filter(|t| norad.contains(&t.norad))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real SatNOGS `/api/satellites` entries (trimmed to the fields we read; the
    // extra keys prove the parser ignores what it doesn't use), plus two crafted
    // rows — one missing `norad_cat_id`, one missing `name` — to exercise the
    // skip path.
    const SATS_FIXTURE: &str = r#"[
      {"sat_id":"SCHX","norad_cat_id":965,"name":"TRANSIT 5B-5","status":"alive","countries":"US"},
      {"sat_id":"AMOM","norad_cat_id":1002,"name":"LES-1","status":"alive"},
      {"sat_id":"HUET","norad_cat_id":2012,"name":"Unknown Satellite","status":"re-entered"},
      {"sat_id":"NONE","name":"NO NORAD","status":"alive"},
      {"sat_id":"NAME","norad_cat_id":9999,"status":"future"}
    ]"#;

    // Real SatNOGS `/api/transmitters` entries (965 downlink-only USB; the ISS
    // 25544 Mode-V APRS transceiver with both up and downlink), plus two crafted
    // rows — one with `mode: null`/`alive: false`, one missing `norad_cat_id`.
    const XMIT_FIXTURE: &str = r#"[
      {"uuid":"UzPz","description":"Upper side band (drifting)","alive":true,"type":"Transmitter","uplink_low":null,"downlink_low":136658500,"mode":"USB","norad_cat_id":965},
      {"uuid":"ZJxC","description":"Mode V APRS","alive":true,"type":"Transceiver","uplink_low":145825000,"downlink_low":145825000,"mode":"AFSK","norad_cat_id":25544},
      {"uuid":"CRFT","description":"beacon, mode uncharacterised","alive":false,"uplink_low":null,"downlink_low":437000000,"mode":null,"norad_cat_id":25544},
      {"uuid":"CRF2","description":"orphan, no norad","alive":true,"downlink_low":100000000}
    ]"#;

    #[test]
    fn parses_real_satellites_and_skips_incomplete() {
        let sats = parse_satellites(SATS_FIXTURE);
        assert_eq!(sats.len(), 3, "three complete rows; two malformed skipped");
        assert_eq!(sats[0].norad, 965);
        assert_eq!(sats[0].name, "TRANSIT 5B-5");
        assert_eq!(sats[0].status, "alive");
        assert_eq!(sats[2].status, "re-entered"); // status kept verbatim
    }

    #[test]
    fn parses_real_transmitters_with_optional_fields() {
        let x = parse_transmitters(XMIT_FIXTURE);
        assert_eq!(
            x.len(),
            3,
            "three complete rows; the norad-less one skipped"
        );
        // Real 965: downlink-only USB, uplink_low was null → None.
        assert_eq!(x[0].norad, 965);
        assert_eq!(x[0].description, "Upper side band (drifting)");
        assert!(x[0].alive);
        assert_eq!(x[0].mode.as_deref(), Some("USB"));
        assert_eq!(x[0].uplink_low_hz, None);
        assert_eq!(x[0].downlink_low_hz, Some(136_658_500));
        // Real ISS APRS transceiver: both up and downlink present.
        assert_eq!(x[1].norad, 25544);
        assert_eq!(x[1].uplink_low_hz, Some(145_825_000));
        assert_eq!(x[1].downlink_low_hz, Some(145_825_000));
        // Crafted: mode null → None, alive false preserved (not fabricated true).
        assert_eq!(x[2].mode, None);
        assert!(!x[2].alive);
    }

    #[test]
    fn linear_transponder_fields_drive_doppler() {
        // The shape SatNOGS publishes for a linear INVERTING transponder — the
        // RS-44 / AO-7 class every satellite operator actually uses. Without
        // these fields the Doppler engine cannot be correct: the uplink runs
        // backwards and the sidebands swap.
        let json = r#"[
          {"norad_cat_id":44909,"description":"Linear Transponder","alive":true,
           "type":"Transponder","invert":true,
           "uplink_low":145935000,"uplink_high":145995000,"uplink_mode":"LSB",
           "downlink_low":435610000,"downlink_high":435670000,"downlink_mode":"USB"},
          {"norad_cat_id":25544,"description":"FM Repeater","alive":true,
           "type":"Transceiver","invert":false,
           "uplink_low":145990000,"downlink_low":437800000,"mode":"FM"}
        ]"#;
        let x = parse_transmitters(json);
        assert_eq!(x.len(), 2);

        let lin = &x[0];
        assert!(lin.invert, "an inverting transponder MUST report it");
        assert!(lin.is_linear());
        assert_eq!(lin.uplink_high_hz, Some(145_995_000));
        assert_eq!(lin.downlink_high_hz, Some(435_670_000));
        assert_eq!(lin.uplink_mode.as_deref(), Some("LSB"));
        assert_eq!(lin.downlink_mode.as_deref(), Some("USB"));
        // Centres, not edges — what the radio parks on before the operator tunes.
        assert_eq!(lin.uplink_centre_hz(), Some(145_965_000));
        assert_eq!(lin.downlink_centre_hz(), Some(435_640_000));

        let fm = &x[1];
        assert!(!fm.invert);
        assert!(
            !fm.is_linear(),
            "an FM repeater is a channel, not a passband"
        );
        // A single-frequency channel: centre IS the frequency.
        assert_eq!(fm.downlink_centre_hz(), Some(437_800_000));
        assert_eq!(fm.uplink_centre_hz(), Some(145_990_000));
    }

    #[test]
    fn the_iss_aprs_transceiver_is_an_fm_bird_on_one_channel() {
        // THE field-report record, straight out of the fixture above:
        //   {"description":"Mode V APRS","type":"Transceiver",
        //    "uplink_low":145825000,"downlink_low":145825000,"mode":"AFSK"}
        // Two facts decide how it must be worked, and the pick path got both
        // wrong: it is FM (a 2 m packet signal demodulated as SSB is garbled
        // audio — the same correctness gate the terrestrial APRS tune already
        // carries), and it is SIMPLEX (one dial, no split, no satellite mode).
        //
        // Note there is no "APRS" MODE in the SatNOGS vocabulary — APRS lives
        // in the description, and the mode is AFSK. A fix keying on the string
        // "APRS" would miss every packet bird.
        let x = parse_transmitters(XMIT_FIXTURE);
        let iss = &x[1];
        assert_eq!(iss.description, "Mode V APRS");
        assert_eq!(iss.mode.as_deref(), Some("AFSK"));
        assert!(iss.is_fm(), "AFSK is FM to a radio");
        assert_eq!(iss.uplink_centre_hz(), iss.downlink_centre_hz());
        assert!(!iss.is_linear(), "a channel, not a passband");

        // The linear pin, byte-identical: RS-44 stays SSB-class, and its two
        // legs stay a cross-band pair.
        let lin = &parse_transmitters(
            r#"[{"norad_cat_id":44909,"description":"Linear Transponder","alive":true,
                 "type":"Transponder","invert":true,
                 "uplink_low":145935000,"uplink_high":145995000,"uplink_mode":"LSB",
                 "downlink_low":435610000,"downlink_high":435670000,"downlink_mode":"USB"}]"#,
        )[0];
        assert!(!lin.is_fm(), "a linear transponder is worked on SSB");
        assert_ne!(lin.uplink_centre_hz(), lin.downlink_centre_hz());

        // Per-LEG mode wins over the single `mode` field: an inverting bird
        // reports USB down / LSB up, and the downlink is what the rig hears.
        let mixed = &parse_transmitters(
            r#"[{"norad_cat_id":1,"description":"x","alive":true,"mode":"FM",
                 "downlink_mode":"USB","downlink_low":435000000}]"#,
        )[0];
        assert!(
            !mixed.is_fm(),
            "the downlink leg decides what the rig hears"
        );

        // An uncharacterised transmitter (mode null) is not claimed as FM.
        assert!(!x[2].is_fm());
    }

    #[test]
    fn only_a_cw_uplink_over_an_fm_downlink_is_cw_only() {
        // KOSEN-1's onboard-SDR transponder, verbatim as db.satnogs.org served it on
        // 2026-09-23. `mode` is the DOWNLINK's (SatNOGS sends no `downlink_mode`), and it is
        // AFSK — FM-class — under a CW `uplink_mode`.
        let kosen1 = &parse_transmitters(
            r#"[{"uuid":"YcezEz4dXU5uTwzYEFFrtk","description":"Mode HF/U - Onboard SDR","alive":true,"type":"Transponder","uplink_low":21125000,"uplink_high":21150000,"uplink_drift":null,"uplink_drifted":21125000,"downlink_low":435525000,"downlink_high":435525000,"downlink_drift":null,"downlink_drifted":435525000,"mode":"AFSK","mode_id":49,"uplink_mode":"CW","invert":false,"baud":1200.0,"sat_id":"NJBJ-3838-9654-0508-8247","norad_cat_id":49402,"norad_follow_id":null,"status":"active","updated":"2021-09-30T18:19:40.605018Z","citation":"http://space.kochi-ct.jp/kosen-1/ and https://iaru.amsat-uk.org/finished_detail.php?serialnum=687","service":"Amateur","iaru_coordination":"IARU Coordinated","iaru_coordination_url":"https://iaru.amsat-uk.org/finished_detail.php?serialnum=687","itu_notification":{"urls":[]},"frequency_violation":false,"unconfirmed":false,"params":null}]"#,
        )[0];
        assert!(kosen1.is_fm(), "scene: AFSK down is FM-class");
        assert!(
            kosen1.uplink_is_cw_only(),
            "KOSEN-1 takes CW up over its AFSK downlink"
        );

        // Everything else keeps today's answer. The same catalogue, the same day — the fields
        // the parser reads, values verbatim.
        let others = parse_transmitters(
            r#"[
              {"norad_cat_id":44909,"description":"Mode V/U - Transponder","alive":true,"type":"Transponder","invert":true,"uplink_low":145935000,"uplink_high":145995000,"uplink_mode":"LSB","downlink_low":435610000,"downlink_high":435670000,"mode":"USB"},
              {"norad_cat_id":27607,"description":"Mode V/U FM Voice CTCSS 67.0 Hz","alive":true,"type":"Transceiver","invert":false,"uplink_low":145850000,"uplink_high":null,"uplink_mode":"FM","downlink_low":436795000,"downlink_high":null,"mode":"FM"},
              {"norad_cat_id":25544,"description":"Mode V APRS","alive":true,"type":"Transceiver","invert":false,"uplink_low":145825000,"uplink_high":null,"uplink_mode":"AFSK","downlink_low":145825000,"downlink_high":null,"mode":"AFSK"},
              {"norad_cat_id":7530,"description":"Mode V/A (A) Lin CW","alive":true,"type":"Transponder","invert":false,"uplink_low":145850000,"uplink_high":145950000,"uplink_mode":"CW","downlink_low":29400000,"downlink_high":29500000,"mode":"CW"},
              {"norad_cat_id":43700,"description":"CW Only Transpoder","alive":true,"type":"Transponder","invert":false,"uplink_low":2400005000,"uplink_high":2400040000,"uplink_mode":"CW","downlink_low":10489505000,"downlink_high":10489540000,"mode":"CW"},
              {"norad_cat_id":99054,"description":"Mode U/V Transponder","alive":true,"type":"Transponder","invert":true,"uplink_low":435012500,"uplink_high":435037500,"uplink_mode":"USB","downlink_low":145962500,"downlink_high":145987500,"mode":"USB"}
            ]"#,
        );
        assert_eq!(others.len(), 6, "scene: six records parsed");
        for t in &others {
            assert!(
                !t.uplink_is_cw_only(),
                "{} {:?}: not a CW uplink over an FM downlink",
                t.norad,
                t.description
            );
        }
        // The two CW-uplink controls are CW-over-LINEAR: it is the FM half that rules them out.
        assert!(others[3].uplink_mode.as_deref() == Some("CW") && !others[3].is_fm());
        assert!(others[4].uplink_mode.as_deref() == Some("CW") && !others[4].is_fm());

        // No uplink mode, or an empty one, says nothing.
        let mut quiet = kosen1.clone();
        quiet.uplink_mode = None;
        assert!(!quiet.uplink_is_cw_only(), "absent uplink_mode");
        quiet.uplink_mode = Some(String::new());
        assert!(!quiet.uplink_is_cw_only(), "empty uplink_mode");
    }

    #[test]
    fn qo100_narrowband_is_linear_by_band_plan_whatever_satnogs_tags_it() {
        use tempo_core::doppler::DownlinkClass;
        // Every live QO-100 row SatNOGS tags FM, verbatim as db.satnogs.org served them on
        // 2026-09-23: the five narrowband segments (FM up AND down) and a receive-only beacon.
        // The band plan allows no FM on that transponder.
        let nb = parse_transmitters(
            r#"[
{"uuid":"URSfwDDuDJUh6MTN7UPrAj","description":"Narrowband digimodes(500Hz max BW)","alive":true,"type":"Transponder","uplink_low":2400040000,"uplink_high":2400080000,"uplink_drift":null,"uplink_drifted":2400040000,"downlink_low":10489540000,"downlink_high":10489580000,"downlink_drift":null,"downlink_drifted":10489540000,"mode":"FM","mode_id":1,"uplink_mode":"FM","invert":false,"baud":null,"sat_id":"LBQI-9987-6865-3928-8294","norad_cat_id":43700,"norad_follow_id":null,"status":"active","updated":"2020-03-18T21:13:52.221618Z","citation":"https://amsat-dl.org/en/neuer-qo-100-bandplan/","service":"Unknown","iaru_coordination":"N/A","iaru_coordination_url":"","itu_notification":{"urls":[]},"frequency_violation":false,"unconfirmed":false,"params":null},
{"uuid":"dX48avS4dR5KSA22RyX6rb","description":"digimodes(2700Hz max BW)","alive":true,"type":"Transponder","uplink_low":2400080000,"uplink_high":2400150000,"uplink_drift":null,"uplink_drifted":2400080000,"downlink_low":10489580000,"downlink_high":10489650000,"downlink_drift":null,"downlink_drifted":10489580000,"mode":"FM","mode_id":1,"uplink_mode":"FM","invert":false,"baud":null,"sat_id":"LBQI-9987-6865-3928-8294","norad_cat_id":43700,"norad_follow_id":null,"status":"active","updated":"2020-03-18T21:13:52.213029Z","citation":"https://amsat-dl.org/en/neuer-qo-100-bandplan/","service":"Unknown","iaru_coordination":"N/A","iaru_coordination_url":"","itu_notification":{"urls":[]},"frequency_violation":false,"unconfirmed":false,"params":null},
{"uuid":"yubm5DLaq5jrab2p9eTrt7","description":"SSB Only Transpoder","alive":true,"type":"Transponder","uplink_low":2400150000,"uplink_high":2400245000,"uplink_drift":null,"uplink_drifted":2400150000,"downlink_low":10489650000,"downlink_high":10489745000,"downlink_drift":null,"downlink_drifted":10489650000,"mode":"FM","mode_id":1,"uplink_mode":"FM","invert":false,"baud":null,"sat_id":"LBQI-9987-6865-3928-8294","norad_cat_id":43700,"norad_follow_id":null,"status":"active","updated":"2020-03-18T21:13:52.224099Z","citation":"https://amsat-dl.org/en/neuer-qo-100-bandplan/","service":"Unknown","iaru_coordination":"N/A","iaru_coordination_url":"","itu_notification":{"urls":[]},"frequency_violation":false,"unconfirmed":false,"params":null},
{"uuid":"VJqQe6wpik5Lb9itkrP3cf","description":"SSB Only Transpoder","alive":true,"type":"Transponder","uplink_low":2400255000,"uplink_high":2400350000,"uplink_drift":null,"uplink_drifted":2400255000,"downlink_low":10489755000,"downlink_high":10489850000,"downlink_drift":null,"downlink_drifted":10489755000,"mode":"FM","mode_id":1,"uplink_mode":"FM","invert":false,"baud":null,"sat_id":"LBQI-9987-6865-3928-8294","norad_cat_id":43700,"norad_follow_id":null,"status":"active","updated":"2020-03-18T21:13:52.222837Z","citation":"https://amsat-dl.org/en/neuer-qo-100-bandplan/","service":"Unknown","iaru_coordination":"N/A","iaru_coordination_url":"","itu_notification":{"urls":[]},"frequency_violation":false,"unconfirmed":false,"params":null},
{"uuid":"M7jS8zxq8x6P4Jy8mM4KHA","description":"Mixed modes(2700Hz max BW)","alive":true,"type":"Transponder","uplink_low":2400350000,"uplink_high":2400495000,"uplink_drift":null,"uplink_drifted":2400350000,"downlink_low":10489850000,"downlink_high":10489995000,"downlink_drift":null,"downlink_drifted":10489850000,"mode":"FM","mode_id":1,"uplink_mode":"FM","invert":false,"baud":null,"sat_id":"LBQI-9987-6865-3928-8294","norad_cat_id":43700,"norad_follow_id":null,"status":"active","updated":"2020-03-18T21:13:52.215801Z","citation":"https://amsat-dl.org/en/neuer-qo-100-bandplan/","service":"Unknown","iaru_coordination":"N/A","iaru_coordination_url":"","itu_notification":{"urls":[]},"frequency_violation":false,"unconfirmed":false,"params":null},
{"uuid":"RNNoHdKN73KSQWTim9J8Co","description":"Experimental Beacon","alive":true,"type":"Transponder","uplink_low":null,"uplink_high":null,"uplink_drift":null,"uplink_drifted":null,"downlink_low":10489995000,"downlink_high":10490000000,"downlink_drift":null,"downlink_drifted":10489995000,"mode":"FM","mode_id":1,"uplink_mode":null,"invert":false,"baud":null,"sat_id":"LBQI-9987-6865-3928-8294","norad_cat_id":43700,"norad_follow_id":null,"status":"active","updated":"2020-03-18T21:13:52.218988Z","citation":"https://amsat-dl.org/en/neuer-qo-100-bandplan/","service":"Unknown","iaru_coordination":"N/A","iaru_coordination_url":"","itu_notification":{"urls":[]},"frequency_violation":false,"unconfirmed":false,"params":null}
            ]"#,
        );
        assert_eq!(nb.len(), 6, "scene: six rows parsed");
        for t in &nb {
            assert_eq!(
                t.mode.as_deref(),
                Some("FM"),
                "scene: SatNOGS tags {:?} FM",
                t.description
            );
            assert_eq!(
                t.downlink_class(),
                DownlinkClass::Usb,
                "{:?}: linear by band plan, not FM",
                t.description
            );
            assert!(!t.is_fm(), "{:?}", t.description);
        }

        // The FM birds stay FM, and the linear ones stay as they were. Same catalogue, same
        // day — the fields the parser reads, values verbatim.
        let guards = parse_transmitters(
            r#"[
              {"norad_cat_id":27607,"description":"Mode V/U FM Voice CTCSS 67.0 Hz","alive":true,"type":"Transceiver","invert":false,"uplink_low":145850000,"uplink_high":null,"uplink_mode":"FM","downlink_low":436795000,"downlink_high":null,"mode":"FM"},
              {"norad_cat_id":43017,"description":"Mode U/V FM Voice (no CTCSS any longer)","alive":true,"type":"Transceiver","invert":false,"uplink_low":435250000,"uplink_high":null,"uplink_mode":"FM","downlink_low":145960000,"downlink_high":null,"mode":"FM"},
              {"norad_cat_id":25544,"description":"Mode V/U FM - Voice Repeater CTCSS 67.0 Hz","alive":true,"type":"Transceiver","invert":false,"uplink_low":145990000,"uplink_high":null,"uplink_mode":"FM","downlink_low":437800000,"downlink_high":null,"mode":"FM"},
              {"norad_cat_id":25544,"description":"Mode V/V FM (crew R2+3)","alive":true,"type":"Transceiver","invert":false,"uplink_low":144490000,"uplink_high":null,"uplink_mode":"FM","downlink_low":145800000,"downlink_high":null,"mode":"FM"},
              {"norad_cat_id":25544,"description":"Mode V APRS","alive":true,"type":"Transceiver","invert":false,"uplink_low":145825000,"uplink_high":null,"uplink_mode":"AFSK","downlink_low":145825000,"downlink_high":null,"mode":"AFSK"},
              {"norad_cat_id":43678,"description":"FM VOICE","alive":true,"type":"Transceiver","invert":false,"uplink_low":437500000,"uplink_high":null,"uplink_mode":"FM","downlink_low":145900000,"downlink_high":null,"mode":"FM"},
              {"norad_cat_id":49402,"description":"Mode HF/U - Onboard SDR","alive":true,"type":"Transponder","invert":false,"uplink_low":21125000,"uplink_high":21150000,"uplink_mode":"CW","downlink_low":435525000,"downlink_high":435525000,"mode":"AFSK"},
              {"norad_cat_id":44909,"description":"Mode V/U - Transponder","alive":true,"type":"Transponder","invert":true,"uplink_low":145935000,"uplink_high":145995000,"uplink_mode":"LSB","downlink_low":435610000,"downlink_high":435670000,"mode":"USB"},
              {"norad_cat_id":43700,"description":"CW Only Transpoder","alive":true,"type":"Transponder","invert":false,"uplink_low":2400005000,"uplink_high":2400040000,"uplink_mode":"CW","downlink_low":10489505000,"downlink_high":10489540000,"mode":"CW"}
            ]"#,
        );
        let class: Vec<DownlinkClass> = guards.iter().map(Transmitter::downlink_class).collect();
        use DownlinkClass::{Fm, Usb};
        assert_eq!(
            class,
            [Fm, Fm, Fm, Fm, Fm, Fm, Fm, Usb, Usb],
            "SO-50, AO-91, the ISS voice and APRS channels, PO-101 and KOSEN-1 stay FM; \
             RS-44 and QO-100's CW-only segment stay linear"
        );
        assert!(
            guards[6].uplink_is_cw_only(),
            "KOSEN-1's CW uplink is untouched"
        );
    }

    #[test]
    fn absent_invert_reads_as_non_inverting() {
        // Fail SAFE: a missing/garbage `invert` must never produce a reversed
        // uplink, which would transmit somewhere the operator never intended.
        let json = r#"[{"norad_cat_id":1,"description":"x","alive":true,
                        "downlink_low":435000000},
                       {"norad_cat_id":2,"description":"y","alive":true,
                        "invert":"yes","downlink_low":435000000}]"#;
        let x = parse_transmitters(json);
        assert!(!x[0].invert, "absent invert → non-inverting");
        assert!(!x[1].invert, "non-boolean invert → non-inverting");
    }

    #[test]
    fn baud_is_a_known_rate_or_nothing() {
        // SatNOGS sends `baud` on every record, as a float literal or null: 2,721 and 2,298 of
        // them in the live list on 2026-09-23, the ISS APRS digipeater's being the 1200.0 here.
        // Only a positive number is a rate. Null, a missing key, junk, and the 0.0 that 31 records
        // carry (18 of them CW beacons) all mean "not known" — "0 bd" is a claim nobody made.
        // A fractional rate is real data (an RTTY beacon's 45.45), so it keeps its precision.
        let x = parse_transmitters(
            r#"[
              {"norad_cat_id":25544,"description":"Mode V APRS","alive":true,"mode":"AFSK","baud":1200.0},
              {"norad_cat_id":25544,"description":"FM voice","alive":true,"mode":"FM","baud":null},
              {"norad_cat_id":25544,"description":"no key","alive":true,"mode":"FM"},
              {"norad_cat_id":25544,"description":"junk","alive":true,"mode":"FSK","baud":"9k6"},
              {"norad_cat_id":25544,"description":"junk","alive":true,"mode":"FSK","baud":true},
              {"norad_cat_id":25544,"description":"Mode V Imaging","alive":true,"mode":"SSTV","baud":0.0},
              {"norad_cat_id":25544,"description":"negative","alive":true,"mode":"FSK","baud":-9600.0},
              {"norad_cat_id":47721,"description":"FSK RTTY","alive":true,"mode":"FSK","baud":45.45},
              {"norad_cat_id":25544,"description":"integer literal","alive":true,"mode":"GMSK","baud":9600}
            ]"#,
        );
        let baud: Vec<Option<f64>> = x.iter().map(|t| t.baud).collect();
        assert_eq!(
            baud,
            [
                Some(1200.0),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(45.45),
                Some(9600.0)
            ]
        );
    }

    #[test]
    fn baud_round_trips_through_the_snapshot_and_the_detail_dto() {
        // `Transmitter` is written as-is into the on-disk SatNOGS snapshot and into the detail
        // DTO the Satellites view reads, so the rate must come back out of serde intact, under
        // the key the TypeScript mirror names.
        let x = parse_transmitters(
            r#"[{"norad_cat_id":25544,"description":"Mode V APRS","alive":true,"mode":"AFSK","baud":1200.0},
                {"norad_cat_id":47721,"description":"FSK RTTY","alive":true,"mode":"FSK","baud":45.45},
                {"norad_cat_id":25544,"description":"FM voice","alive":true,"mode":"FM","baud":null}]"#,
        );
        let wire = serde_json::to_value(&x).unwrap();
        assert_eq!(wire[0]["baud"], serde_json::json!(1200.0));
        assert_eq!(wire[1]["baud"], serde_json::json!(45.45));
        assert_eq!(wire[2]["baud"], Value::Null);
        let back: Vec<Transmitter> = serde_json::from_value(wire).unwrap();
        let baud: Vec<Option<f64>> = back.iter().map(|t| t.baud).collect();
        assert_eq!(baud, [Some(1200.0), Some(45.45), None]);

        // A snapshot written before the field existed carries no `baud` key. It must still load,
        // as unknown: a snapshot that fails to parse is read as "never fetched", and an offline
        // station would lose its whole transponder list until the next successful fetch.
        let legacy: Transmitter = serde_json::from_str(
            r#"{"norad":25544,"description":"Mode V APRS","alive":true,"mode":"AFSK",
                "uplinkLowHz":145825000,"downlinkLowHz":145825000,"invert":false,
                "uplinkHighHz":null,"downlinkHighHz":null,"uplinkMode":null,
                "downlinkMode":null,"kind":"Transceiver"}"#,
        )
        .unwrap();
        assert_eq!(legacy.baud, None);
    }

    #[test]
    fn empty_or_garbage_yields_no_entries() {
        assert!(parse_satellites("").is_empty());
        assert!(parse_satellites("not json").is_empty());
        assert!(parse_satellites("{}").is_empty()); // object, not the expected array
        assert!(parse_transmitters("[]").is_empty());
        assert!(parse_transmitters("null").is_empty());
    }
}
