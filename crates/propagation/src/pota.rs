//! Parks/Summits On The Air activator-spot parsing (the "who's on the air now"
//! hunter feed). Pure JSON→[`OtaSpot`] mapping for the two live APIs; the HTTP
//! fetch lives in [`crate::live::pota`]. Reference validation is in
//! `tempo_core::pota`.

use serde::{Deserialize, Serialize};

/// One activator currently on the air (POTA or SOTA), normalized across both feeds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtaSpot {
    /// "POTA" | "SOTA".
    pub program: String,
    /// Park/summit id, e.g. "K-1234" / "W7A/MN-001".
    pub reference: String,
    /// Park/summit name (may be empty for RBN-sourced POTA spots).
    pub name: String,
    pub activator: String,
    /// Frequency in kHz (POTA reports kHz; SOTA reports MHz — normalized here).
    pub freq_khz: f64,
    pub mode: String,
    pub spotter: Option<String>,
    pub comment: Option<String>,
    pub grid: Option<String>,
    /// Exact park/summit position when the feed carries one. POTA reports
    /// `latitude`/`longitude` on every activator row, which places the park itself
    /// rather than the ~4 km square a grid gives — so the map plots where the
    /// operator actually is. `None` leaves placement to [`Self::grid`].
    ///
    /// ⚠️ SOTA carries NO position at all: its spot payload has only
    /// `associationCode`/`summitCode`, and resolving those to a summit needs a
    /// second lookup against a different endpoint. A SOTA spot is therefore
    /// unplottable from this feed alone — see [`parse_sota_spots`].
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    /// Spot time (unix seconds, UTC) from the feed's timestamp — POTA `spotTime`,
    /// SOTA `timeStamp`. `None` if absent/unparseable. Lets a consumer drop STALE
    /// activations, which matters for SOTA: its `spots/<n>/all` returns the last `n`
    /// spots by COUNT, not by recency, so an old summit can ride along on a quiet day.
    pub spot_time_unix: Option<i64>,
    /// The US states (and DC) and Canadian provinces the activation is in, as
    /// country-subdivision codes ("US-ND", "CA-ON"), the form POTA's own `locationDesc`
    /// takes. POTA: from that field, so a park on a state line carries each of its states
    /// ("US-MT", "US-ND"). SOTA: from the summit's association, or from its region where the
    /// association spans several states ([`sota_state`]). Empty anywhere else, and for a
    /// summit whose state SOTA's published data does not settle.
    ///
    /// ⚠️ NEVER SERIALIZED. Two consumers flatten this struct beside fields of their own: the
    /// desktop board's row, which writes these codes under a key of its own, and Remote's
    /// hunter-feed projection, whose page refuses any spot carrying a key it does not list. A
    /// serialized field here would empty the Remote board for every station that updated
    /// before the page did.
    #[serde(skip)]
    pub states: Vec<String>,
}

fn s(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .filter(|x| !x.is_empty())
}

/// Parse the POTA activator-spots JSON (`https://api.pota.app/spot/activator`): an
/// array of objects with `activator`, `reference`, `frequency` (kHz string), `mode`,
/// `name`, `spotTime`, `spotter`, `comments`, `grid6`/`grid4`. Rows missing the
/// required call/reference/frequency are skipped. Malformed JSON → empty.
pub fn parse_pota_spots(json: &str) -> Vec<OtaSpot> {
    let arr: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    arr.iter()
        .filter_map(|v| {
            let activator = s(v, "activator")?;
            let reference = s(v, "reference")?;
            let freq_khz = freq_field(v, "frequency")?;
            Some(OtaSpot {
                program: "POTA".into(),
                reference,
                name: s(v, "name").unwrap_or_default(),
                activator,
                freq_khz,
                mode: s(v, "mode").unwrap_or_default(),
                spotter: s(v, "spotter"),
                comment: s(v, "comments"),
                grid: s(v, "grid6").or_else(|| s(v, "grid4")),
                lat: num_field(v, "latitude").filter(|x| (-90.0..=90.0).contains(x)),
                lon: num_field(v, "longitude").filter(|x| (-180.0..=180.0).contains(x)),
                spot_time_unix: time_field(v, "spotTime"),
                states: s(v, "locationDesc")
                    .map(|l| location_states(&l))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// Parse the SOTAwatch v2 spots JSON
/// (`https://api-db2.sota.org.uk/api/spots/<n>/all`): an array with
/// `activatorCallsign`, `associationCode` + `summitCode` (joined as `ASSOC/SUMMIT`),
/// `frequency` (**MHz** string → kHz here), `mode`, `summitDetails`, `callsign`
/// (spotter), `comments`. Rows missing call/summit/frequency are skipped.
pub fn parse_sota_spots(json: &str) -> Vec<OtaSpot> {
    let arr: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    arr.iter()
        .filter_map(|v| {
            let activator = s(v, "activatorCallsign")?;
            let assoc = s(v, "associationCode")?;
            let summit = s(v, "summitCode")?;
            let mhz = freq_field(v, "frequency")?;
            let reference = format!("{assoc}/{summit}");
            Some(OtaSpot {
                program: "SOTA".into(),
                states: sota_state(&reference)
                    .map(str::to_string)
                    .into_iter()
                    .collect(),
                reference,
                name: s(v, "summitDetails").unwrap_or_default(),
                activator,
                freq_khz: mhz * 1000.0,
                mode: s(v, "mode").unwrap_or_default(),
                spotter: s(v, "callsign"),
                comment: s(v, "comments"),
                grid: None,
                // The feed carries no position — see the field docs on `OtaSpot::lat`.
                lat: None,
                lon: None,
                spot_time_unix: time_field(v, "timeStamp"),
            })
        })
        .collect()
}

/// The US states and Canadian provinces in a POTA `locationDesc`, in the feed's order:
/// "US-ND", or "US-MT,US-ND" for a park on a state line. Kept are the subdivisions ADIF gives a
/// STATE for: the 50 states and DC (Alaska and Hawaii are POTA's US-AK and US-HI) and the 13
/// provinces and territories. Everything else is left out rather than shown as what it is not:
/// another country's ("FR-ARA"), and codes POTA files under the US or Canada that are no state
/// or province ("US-KI", "CA-SP").
///
/// Public because the park list POTA publishes carries the same field: a park read from the list
/// is placed by exactly the rule a spot is.
pub fn location_states(location: &str) -> Vec<String> {
    location.split(',').filter_map(subdivision).collect()
}

/// One `locationDesc` code, canonical ("US-ND"), when it is a state, DC or a province.
fn subdivision(code: &str) -> Option<String> {
    let (country, sub) = code.trim().split_once('-')?;
    let country = country.to_ascii_uppercase();
    let sub = match country.as_str() {
        "US" if sub.trim().eq_ignore_ascii_case("DC") => "DC",
        "US" => crate::awards::valid_state(sub)?,
        "CA" => crate::awards::valid_province(sub)?,
        _ => return None,
    };
    Some(format!("{country}-{sub}"))
}

/// The US state or Canadian province of a SOTA summit, from its reference ("W7M/MT-001"):
/// its association's, or, for an association that spans several states, its region's. `None`
/// for anything SOTA's published data does not place in one state or province.
pub fn sota_state(reference: &str) -> Option<&'static str> {
    let (association, summit) = reference.trim().split_once('/')?;
    let association = association.to_ascii_uppercase();
    if let Some((_, st)) = SOTA_ASSOCIATION_STATES
        .iter()
        .find(|(a, _)| *a == association)
    {
        return Some(st);
    }
    let region = format!(
        "{association}/{}",
        summit.split('-').next()?.to_ascii_uppercase()
    );
    SOTA_REGION_STATES
        .iter()
        .find(|(r, _)| *r == region)
        .map(|(_, st)| *st)
}

// THE TWO SOTA TABLES are SOTA's own published data, not a reading of callsign prefixes: the
// association list (https://api-db2.sota.org.uk/api/associations) and, for each association
// that spans several states, its regions (https://api-db2.sota.org.uk/api/associations/<code>),
// both read 2026-10-06. An association is listed here only when its published name is one state
// or province ("USA - Montana"). The six named for several (W0D "Dakotas", W1, W2, W3, W4C
// "Carolinas" and W9) are placed per region, each by its own name or the notes SOTA publishes
// with it: W1/CR "Connecticut River" is "all of Massachusetts east of the Connecticut River",
// so a name alone would have guessed wrong. Three W3 regions (ER, PH, PO) name only counties,
// all Pennsylvania's, and SOTA's own bounds for each lie north of Maryland and Delaware. KH0,
// KH2, KH8 and KP4 are territories, not states, and are left out. An association or region
// added after that date carries nothing until it is listed here.

/// Associations whose published name is one US state or Canadian province.
const SOTA_ASSOCIATION_STATES: [(&str, &str); 43] = [
    ("K0M", "US-MN"),
    ("KH6", "US-HI"),
    ("KLA", "US-AK"),
    ("KLF", "US-AK"),
    ("KLS", "US-AK"),
    ("W0C", "US-CO"),
    ("W0I", "US-IA"),
    ("W0M", "US-MO"),
    ("W0N", "US-NE"),
    ("W4A", "US-AL"),
    ("W4G", "US-GA"),
    ("W4K", "US-KY"),
    ("W4T", "US-TN"),
    ("W4V", "US-VA"),
    ("W5A", "US-AR"),
    ("W5M", "US-MS"),
    ("W5N", "US-NM"),
    ("W5O", "US-OK"),
    ("W5T", "US-TX"),
    ("W6", "US-CA"),
    ("W7A", "US-AZ"),
    ("W7I", "US-ID"),
    ("W7M", "US-MT"),
    ("W7N", "US-NV"),
    ("W7O", "US-OR"),
    ("W7U", "US-UT"),
    ("W7W", "US-WA"),
    ("W7Y", "US-WY"),
    ("W8M", "US-MI"),
    ("W8O", "US-OH"),
    ("W8V", "US-WV"),
    ("VE1", "CA-NS"),
    ("VE2", "CA-QC"),
    ("VE3", "CA-ON"),
    ("VE4", "CA-MB"),
    ("VE5", "CA-SK"),
    ("VE6", "CA-AB"),
    ("VE7", "CA-BC"),
    ("VE9", "CA-NB"),
    ("VO1", "CA-NL"),
    ("VO2", "CA-NL"),
    ("VY1", "CA-YT"),
    ("VY2", "CA-PE"),
];

/// Regions of the associations that span several states, each in the one state SOTA's
/// published name or notes for it place it in.
const SOTA_REGION_STATES: [(&str, &str); 42] = [
    ("W0D/BB", "US-SD"),
    ("W0D/ES", "US-SD"),
    ("W0D/MR", "US-SD"),
    ("W0D/ND", "US-ND"),
    ("W0D/NW", "US-SD"),
    ("W1/AM", "US-ME"),
    ("W1/CB", "US-CT"),
    ("W1/CR", "US-MA"),
    ("W1/DI", "US-ME"),
    ("W1/EM", "US-ME"),
    ("W1/GM", "US-VT"),
    ("W1/HA", "US-NH"),
    ("W1/HH", "US-CT"),
    ("W1/MB", "US-MA"),
    ("W1/MR", "US-CT"),
    ("W1/MV", "US-NH"),
    ("W1/NK", "US-VT"),
    ("W1/NL", "US-NH"),
    ("W2/EH", "US-NY"),
    ("W2/GA", "US-NY"),
    ("W2/GC", "US-NY"),
    ("W2/NJ", "US-NJ"),
    ("W2/WE", "US-NY"),
    ("W3/CR", "US-MD"),
    ("W3/CT", "US-MD"),
    ("W3/ER", "US-PA"),
    ("W3/PD", "US-PA"),
    ("W3/PH", "US-PA"),
    ("W3/PO", "US-PA"),
    ("W3/PT", "US-PA"),
    ("W3/PW", "US-PA"),
    ("W3/SV", "US-PA"),
    ("W3/WE", "US-MD"),
    ("W4C/CM", "US-NC"),
    ("W4C/EM", "US-NC"),
    ("W4C/EP", "US-NC"),
    ("W4C/US", "US-SC"),
    ("W4C/WM", "US-NC"),
    ("W4C/WP", "US-NC"),
    ("W9/IL", "US-IL"),
    ("W9/IN", "US-IN"),
    ("W9/WI", "US-WI"),
];

/// Parse a naive-UTC ISO timestamp field (POTA `spotTime` / SOTA `timeStamp`, both
/// UTC) to unix seconds. Reuses the crate's tolerant parser (handles the `Z` suffix +
/// fractional seconds SOTA sometimes emits). `None` if absent/unparseable.
fn time_field(v: &serde_json::Value, k: &str) -> Option<i64> {
    v.get(k)
        .and_then(|x| x.as_str())
        .and_then(crate::kc2g::parse_naive_utc_unix)
}

/// Read a frequency that may be a JSON string or number.
/// A plain number field, accepting the string form the POTA feed sometimes uses.
/// Unlike [`freq_field`] this does NOT require a positive value: a longitude in the
/// western hemisphere is negative and the equator/prime meridian are 0.
fn num_field(v: &serde_json::Value, k: &str) -> Option<f64> {
    let f = v.get(k)?;
    f.as_f64()
        .or_else(|| f.as_str().and_then(|x| x.trim().parse::<f64>().ok()))
        .filter(|x| x.is_finite())
}

fn freq_field(v: &serde_json::Value, k: &str) -> Option<f64> {
    let f = v.get(k)?;
    f.as_str()
        .and_then(|x| x.trim().parse::<f64>().ok())
        .or_else(|| f.as_f64())
        .filter(|x| *x > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map places a park by the coordinates the feed gives, not by its grid
    /// square: POTA sends `latitude`/`longitude` on every activator row, which is the
    /// park itself rather than the ~4 km square `grid6` rounds it into. A western
    /// longitude is negative and the equator/meridian are 0, so the parser must not
    /// treat "positive" as "valid" the way the frequency field does.
    #[test]
    fn pota_spots_carry_exact_coordinates() {
        let json = r#"[
          {"activator":"N3ES","reference":"US-1352","name":"Fort Washington State Park",
           "frequency":"14049","mode":"CW","spotTime":"2026-08-31T20:47:50",
           "grid4":"FN20","grid6":"FN20jc","latitude":40.1209,"longitude":-75.2237},
          {"activator":"9M2X","reference":"XZ-0001","name":"Equator Park",
           "frequency":"7032","mode":"CW","latitude":0,"longitude":0},
          {"activator":"K0NO","reference":"US-0002","name":"No Coords",
           "frequency":"7040","mode":"SSB","grid4":"EN52"}
        ]"#;
        let spots = parse_pota_spots(json);
        assert_eq!(spots.len(), 3);
        assert_eq!(spots[0].lat, Some(40.1209));
        assert_eq!(
            spots[0].lon,
            Some(-75.2237),
            "a western longitude was dropped"
        );
        // 0/0 is a real place, not a missing value.
        assert_eq!(spots[1].lat, Some(0.0));
        assert_eq!(
            spots[1].lon,
            Some(0.0),
            "the prime meridian was treated as absent"
        );
        // No coordinates in the row: placement falls back to the grid.
        assert_eq!(spots[2].lat, None);
        assert_eq!(spots[2].grid.as_deref(), Some("EN52"));
    }

    /// Out-of-range junk is refused rather than plotted somewhere impossible.
    #[test]
    fn pota_rejects_impossible_coordinates() {
        let json = r#"[{"activator":"K0X","reference":"US-1","name":"Bad",
           "frequency":"7040","mode":"SSB","latitude":991.0,"longitude":-75.0}]"#;
        let spots = parse_pota_spots(json);
        assert_eq!(spots[0].lat, None, "a latitude of 991 was accepted");
        assert_eq!(spots[0].lon, Some(-75.0));
    }

    /// ⚠️ SOTA SPOTS CANNOT BE PLACED FROM THIS FEED. The payload carries
    /// `associationCode`/`summitCode` and no coordinates, so anything drawing summits
    /// on a map needs a second lookup. Pinned so nobody assumes symmetry with POTA.
    #[test]
    fn sota_spots_carry_no_position() {
        let json = r#"[{"activatorCallsign":"G0ABC","associationCode":"G","summitCode":"LD-001",
           "frequency":"14.285","mode":"ssb","summitDetails":"Scafell Pike","timeStamp":"2026-08-31T12:00:00"}]"#;
        let spots = parse_sota_spots(json);
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].lat, None);
        assert_eq!(spots[0].lon, None);
        assert_eq!(spots[0].grid, None);
    }

    #[test]
    fn parses_pota_activator_spots() {
        // Trimmed real-shape payload (api.pota.app/spot/activator).
        let json = r#"[
          {"spotId":51518793,"activator":"F4MOJ/P","frequency":"3573.0","mode":"FT8",
           "reference":"FR-11086","parkName":null,"spotTime":"2026-06-07T05:34:30",
           "spotter":"OE9GHV-#","comments":"RBN 1 dB via OE9GHV-#","source":"RBN",
           "name":"Belledonne Reserve","locationDesc":"FR-ARA","grid4":"JN35","grid6":"JN35bh"},
          {"activator":"K1ABC","frequency":"14074.0","mode":"FT8","reference":"K-1234",
           "name":"Acadia NP","spotter":"W9XYZ","comments":""}
        ]"#;
        let spots = parse_pota_spots(json);
        assert_eq!(spots.len(), 2);
        assert_eq!(spots[0].program, "POTA");
        assert_eq!(spots[0].reference, "FR-11086");
        assert_eq!(spots[0].activator, "F4MOJ/P");
        assert_eq!(spots[0].freq_khz, 3573.0);
        assert_eq!(spots[0].grid.as_deref(), Some("JN35bh")); // grid6 preferred
        assert_eq!(spots[0].spotter.as_deref(), Some("OE9GHV-#"));
        // Empty comments → None.
        assert_eq!(spots[1].comment, None);
        assert_eq!(spots[1].reference, "K-1234");
        // spotTime → unix (2026-06-07T05:34:30 UTC).
        assert!(spots[0]
            .spot_time_unix
            .is_some_and(|t| (1_767_000_000..1_800_000_000).contains(&t)));
    }

    #[test]
    fn parses_sota_spots_and_converts_mhz() {
        let json = r#"[
          {"id":324998,"userID":8546,"timeStamp":"2026-06-07T05:32:14",
           "comments":"[SOTA Activator] last calls","callsign":"VK3HN",
           "associationCode":"VK3","summitCode":"VN-012","activatorCallsign":"VK3KR",
           "activatorName":"David","frequency":"7.033","mode":"CW",
           "summitDetails":"Mt Mitchell, 935m, 6 points","highlightColor":null}
        ]"#;
        let spots = parse_sota_spots(json);
        assert_eq!(spots.len(), 1);
        let s = &spots[0];
        assert_eq!(s.program, "SOTA");
        assert_eq!(s.reference, "VK3/VN-012");
        assert_eq!(s.activator, "VK3KR");
        assert_eq!(s.freq_khz, 7033.0); // 7.033 MHz → kHz
        assert_eq!(s.mode, "CW");
        assert_eq!(s.spotter.as_deref(), Some("VK3HN"));
        assert!(s.name.contains("Mt Mitchell"));
        // timeStamp → unix (recency filtering for sparse SOTA).
        assert!(s
            .spot_time_unix
            .is_some_and(|t| (1_767_000_000..1_800_000_000).contains(&t)));
    }

    /// The state an activator is in, for the board and for WAS. POTA says it on every row
    /// (`locationDesc`), and a park on a state line says each of its states, comma-joined,
    /// the form all 141 multi-state US parks of pota.app's park list take
    /// (`all_parks_ext.csv`, 2026-10-05). Only a US state, DC or a Canadian province is kept:
    /// another country's subdivision, or a code POTA files under the US or Canada that is no
    /// state at all, would be shown as something it is not.
    #[test]
    fn pota_spots_carry_the_states_of_their_park() {
        let json = r#"[
          {"activator":"K0ND","reference":"US-0700","frequency":"14285","mode":"SSB","locationDesc":"US-ND"},
          {"activator":"N7XX","reference":"US-4567","frequency":"7185","mode":"SSB","locationDesc":"US-MT,US-ND"},
          {"activator":"VE3XX","reference":"CA-0001","frequency":"14062","mode":"CW","locationDesc":"CA-ON"},
          {"activator":"K3DC","reference":"US-0015","frequency":"14062","mode":"CW","locationDesc":"US-DC,US-MD,US-WV"},
          {"activator":"F4MOJ/P","reference":"FR-11086","frequency":"3573","mode":"FT8","locationDesc":"FR-ARA"},
          {"activator":"VE1XX","reference":"CA-9999","frequency":"14062","mode":"CW","locationDesc":"CA-SP"},
          {"activator":"KH6XX","reference":"US-9999","frequency":"14062","mode":"CW","locationDesc":"US-KI"},
          {"activator":"K1ABC","reference":"K-1234","frequency":"14074","mode":"FT8"}
        ]"#;
        let states: Vec<Vec<String>> = parse_pota_spots(json)
            .into_iter()
            .map(|s| s.states)
            .collect();
        let want: Vec<Vec<&str>> = vec![
            vec!["US-ND"],
            vec!["US-MT", "US-ND"],
            vec!["CA-ON"],
            vec!["US-DC", "US-MD", "US-WV"],
            vec![],
            vec![],
            vec![],
            vec![],
        ];
        assert_eq!(states, want);
    }

    /// A SOTA spot carries no position, so its state comes from the summit's association, as
    /// SOTA publishes it. An association that spans several states settles a state only
    /// through its region; a region its data does not place, a territory and another country
    /// carry nothing rather than a guess.
    #[test]
    fn sota_spots_carry_their_associations_state_or_nothing() {
        let json = r#"[
          {"activatorCallsign":"K7MT","associationCode":"W7M","summitCode":"MT-001","frequency":"14.062","mode":"CW"},
          {"activatorCallsign":"K0ND","associationCode":"W0D","summitCode":"ND-001","frequency":"14.062","mode":"CW"},
          {"activatorCallsign":"VE3XX","associationCode":"VE3","summitCode":"ON-001","frequency":"14.062","mode":"CW"},
          {"activatorCallsign":"G0ABC","associationCode":"G","summitCode":"LD-001","frequency":"14.285","mode":"SSB"}
        ]"#;
        let states: Vec<Vec<String>> = parse_sota_spots(json)
            .into_iter()
            .map(|s| s.states)
            .collect();
        let want: Vec<Vec<&str>> = vec![vec!["US-MT"], vec!["US-ND"], vec!["CA-ON"], vec![]];
        assert_eq!(states, want);

        // W1 spans six states. Its Connecticut River region is defined as Massachusetts east of
        // the river, so its name would have guessed the wrong state.
        assert_eq!(sota_state("W1/CR-001"), Some("US-MA"));
        assert_eq!(sota_state("W1/HA-001"), Some("US-NH"));
        assert_eq!(sota_state("W0D/MR-001"), Some("US-SD"));
        // A region W1's published data does not list: nothing.
        assert_eq!(sota_state("W1/ZZ-001"), None);
        // Alaska is three associations; Labrador is Newfoundland and Labrador.
        assert_eq!(sota_state("KLF/FN-001"), Some("US-AK"));
        assert_eq!(sota_state("VO2/LB-001"), Some("CA-NL"));
        // Guam is no state, and a malformed reference names nothing.
        assert_eq!(sota_state("KH2/GU-001"), None);
        assert_eq!(sota_state("W7M"), None);
    }

    /// The two SOTA tables can only name what the board shows: every value is a state, DC or
    /// a province in the same codes POTA uses, no association is in both, and every region
    /// belongs to an association the first table does not answer for.
    #[test]
    fn the_sota_tables_name_only_states_the_board_shows() {
        for (_, st) in SOTA_ASSOCIATION_STATES
            .iter()
            .chain(SOTA_REGION_STATES.iter())
        {
            assert_eq!(
                location_states(st),
                vec![st.to_string()],
                "{st} is not a state"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for (key, _) in SOTA_ASSOCIATION_STATES
            .iter()
            .chain(SOTA_REGION_STATES.iter())
        {
            assert!(seen.insert(*key), "{key} is in the tables twice");
        }
        for (region, _) in SOTA_REGION_STATES {
            let (assoc, _) = region.split_once('/').expect("a region is ASSOC/REGION");
            assert!(
                SOTA_ASSOCIATION_STATES.iter().all(|(a, _)| *a != assoc),
                "{region}: its association already has one state"
            );
        }
        // Positive control: the tables are not empty, so the loops above checked something.
        assert!(SOTA_ASSOCIATION_STATES.len() >= 40 && SOTA_REGION_STATES.len() >= 40);
    }

    /// The states never reach JSON from here: Remote's page refuses a spot carrying any key it
    /// does not list, and two consumers flatten this struct. The desktop row writes them under
    /// a key of its own.
    #[test]
    fn states_are_never_serialized() {
        let json = r#"[{"activator":"K0ND","reference":"US-0700","frequency":"14285","mode":"SSB","locationDesc":"US-ND"}]"#;
        let spot = parse_pota_spots(json).remove(0);
        assert_eq!(
            spot.states,
            vec!["US-ND"],
            "the control: a spot that HAS a state"
        );
        let value = serde_json::to_value(&spot).expect("serializes");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "activator",
                "comment",
                "freqKhz",
                "grid",
                "lat",
                "lon",
                "mode",
                "name",
                "program",
                "reference",
                "spotTimeUnix",
                "spotter"
            ]
        );
    }

    #[test]
    fn malformed_or_incomplete_rows_are_skipped() {
        assert!(parse_pota_spots("not json").is_empty());
        assert!(parse_sota_spots("{}").is_empty());
        // A row missing the activator is dropped, the valid one kept.
        let json = r#"[{"reference":"K-1","frequency":"14074"},
                       {"activator":"K1ABC","reference":"K-1234","frequency":"14074"}]"#;
        assert_eq!(parse_pota_spots(json).len(), 1);
    }
}
