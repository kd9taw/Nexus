//! Nexus's needed spots for the SDR program's own panadapter, on a radio of the TCI connection
//! whose "Spots on the SDR panadapter" is on (per radio, off by default).
//!
//! The spots are the Needed board's own list (`read_need_alerts`, the one every reader of the board
//! shares, contest assistance rules included: an unassisted entry's board has no cluster or skimmer
//! spots, so its panadapter has none either). Every [`EVERY`], while the radio in use wants them
//! (`Engine::tci_spots_on`), a thread of its own ("nexus-tci-spots") reads that list with the engine
//! lock released for the fold, keeps the stations it has a frequency for, one spot per call, the
//! best first and at most `TCI_SPOTS_MAX`, each in the colour the board draws its need in, and hands
//! them to the engine (`Engine::set_tci_spots`). The radio loop gives them to the radio's TCI
//! client, which puts them on the panadapter (`tempo_audio::tci::spots`). When the switch goes off
//! the engine's list is emptied. Nothing here writes to a radio.

use std::time::Duration;

use propagation::{NeedAlert, NeedTag};
use tempo_app::engine::engine_lock;
use tempo_app::engine::tci_spots::PanadapterSpot;
use tempo_app::settings::TCI_SPOTS_MAX;

use crate::{
    now_unix, read_need_alerts, NeedsKept, SharedEngine, SharedLivePaths, SharedOtaSpots,
    SharedRegionPaths, SharedSpots,
};

/// How often the list is read again: spots move in minutes, and a cluster spot is read from the
/// Needed board's 15-minute window.
pub const EVERY: Duration = Duration::from_secs(10);

/// The colour a need is drawn in, as ARGB, opaque: the Needed board's own colours, the dark
/// theme's (`--need-*` in `ui/src/styles.css`), since an SDR program's panadapter is dark. The
/// first tag that is a reason to work the station colours it, as on the board: DXpedition, POTA and
/// SOTA only label a station (`NEED_VISUALS`' `iconOnly`), so they colour one only when nothing else
/// does.
pub fn argb(tags: &[NeedTag]) -> u32 {
    let labels = |t: &NeedTag| matches!(t, NeedTag::Dxped | NeedTag::Pota | NeedTag::Sota);
    let lead = tags
        .iter()
        .find(|t| !labels(t))
        .or_else(|| tags.first())
        .copied();
    let rgb = match lead {
        Some(NeedTag::Wanted) => 0xa3e635,
        Some(NeedTag::NewEntity) => 0xf23ec0,
        Some(NeedTag::NewIota) => 0xfb7185,
        Some(NeedTag::NewZone) => 0xc084fc,
        Some(NeedTag::NewState) => 0x2dd4bf,
        Some(NeedTag::NewGrid) => 0x34d399,
        Some(NeedTag::NewBand) => 0xf59e0b,
        Some(NeedTag::NewMode) => 0x22d3ee,
        Some(NeedTag::NewPark | NeedTag::Pota) => 0x4ade80,
        Some(NeedTag::Sota) => 0xa78bfa,
        Some(NeedTag::Dxped) => 0x38bdf8,
        Some(NeedTag::Confirm) | None => 0x9ca3af,
    };
    0xFF00_0000 | rgb
}

/// The mode a spot is labelled with: the mode it was heard in, or its class as a panadapter names
/// one ("SSB" for phone, "DIGI" for a data mode the spot did not name).
fn mode_label(a: &NeedAlert) -> String {
    match a.exact_mode.as_deref() {
        Some(mode) if !mode.trim().is_empty() => mode.trim().to_ascii_uppercase(),
        _ => match a.mode.as_str() {
            "CW" => "CW".into(),
            "Phone" => "SSB".into(),
            _ => "DIGI".into(),
        },
    }
}

/// The Needed board's list as panadapter spots: the stations with a frequency, one spot per call
/// (its first, best row), at most `TCI_SPOTS_MAX`, in the board's order. `now` stands in for when a
/// row with no time of its own was heard.
pub fn panadapter_spots(alerts: &[NeedAlert], now: i64) -> Vec<PanadapterSpot> {
    let mut spots: Vec<PanadapterSpot> = Vec::new();
    for a in alerts {
        if spots.len() >= TCI_SPOTS_MAX {
            break;
        }
        let Some(mhz) = a.freq_mhz.filter(|f| f.is_finite() && *f > 0.0) else {
            continue;
        };
        if spots.iter().any(|s| s.call.eq_ignore_ascii_case(&a.call)) {
            continue;
        }
        spots.push(PanadapterSpot {
            call: a.call.clone(),
            hz: (mhz * 1e6).round() as i64,
            mode: mode_label(a),
            argb: argb(&a.tags),
            text: a
                .tags
                .iter()
                .map(|t| t.label())
                .collect::<Vec<_>>()
                .join(" / "),
            heard_unix: a.admitted_at.unwrap_or(now),
        });
    }
    spots
}

/// Start the thread that keeps the engine's panadapter spots ([`EVERY`]).
pub fn spawn(
    engine: SharedEngine,
    needs: NeedsKept,
    live: SharedLivePaths,
    region: SharedRegionPaths,
    spots: SharedSpots,
    ota: SharedOtaSpots,
) {
    let started = std::thread::Builder::new()
        .name("nexus-tci-spots".into())
        .spawn(move || run(&engine, &needs, &live, &region, &spots, &ota));
    if let Err(e) = started {
        tempo_core::applog::warn(
            "startup",
            &format!("spots for the SDR panadapter not started ({e})"),
        );
    }
}

fn run(
    engine: &SharedEngine,
    needs: &NeedsKept,
    live: &SharedLivePaths,
    region: &SharedRegionPaths,
    spots: &SharedSpots,
    ota: &SharedOtaSpots,
) {
    let mut handed = false;
    loop {
        let on = engine_lock(engine).tci_spots_on();
        if on {
            // A read the log refuses for now (a contact still being saved) is tried again next
            // time round; the spots already handed stay until then.
            if let Ok(alerts) =
                read_need_alerts(engine_lock(engine), needs, live, region, spots, ota)
            {
                let chosen = panadapter_spots(&alerts, now_unix());
                engine_lock(engine).set_tci_spots(chosen);
                handed = true;
            }
        } else if std::mem::take(&mut handed) {
            engine_lock(engine).set_tci_spots(Vec::new());
        }
        std::thread::sleep(EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(call: &str, freq_mhz: Option<f64>, tags: Vec<NeedTag>) -> NeedAlert {
        NeedAlert {
            call: call.into(),
            entity: String::new(),
            band: "20m".into(),
            zone: 0,
            priority: tags.first().map_or(0, |t| t.tier()),
            tags,
            headline: String::new(),
            mode: "CW".into(),
            exact_mode: None,
            freq_mhz,
            admitted_at: Some(1_000),
            evidence: None,
            grid_rarity: None,
            grid: None,
            park: None,
            state_from: None,
            wpx_need: None,
            wae_need: None,
            iota: None,
        }
    }

    /// The board's rows become spots in its order, one per call, only those with a frequency, at
    /// most `TCI_SPOTS_MAX`; each with the colour and the words of its needs and the time it was
    /// heard. CONTROL: a row with no time of its own is heard now.
    #[test]
    fn the_boards_rows_with_a_frequency_become_spots_one_per_call_at_most_the_cap() {
        let mut rows = vec![
            alert(
                "K1ABC",
                Some(14.025),
                vec![NeedTag::NewEntity, NeedTag::Dxped],
            ),
            alert("K1ABC", Some(7.025), vec![NeedTag::NewBand]),
            alert("W1AW", None, vec![NeedTag::NewZone]),
            alert("N0CALL", Some(3.5305), vec![NeedTag::Confirm]),
        ];
        rows[3].admitted_at = None;
        rows[3].exact_mode = Some("ft8".into());
        let spots = panadapter_spots(&rows, 2_000);
        assert_eq!(
            spots,
            [
                PanadapterSpot {
                    call: "K1ABC".into(),
                    hz: 14_025_000,
                    mode: "CW".into(),
                    argb: 0xFFF2_3EC0,
                    text: "New one / DXpedition".into(),
                    heard_unix: 1_000,
                },
                PanadapterSpot {
                    call: "N0CALL".into(),
                    hz: 3_530_500,
                    mode: "FT8".into(),
                    argb: 0xFF9C_A3AF,
                    text: "Confirm".into(),
                    heard_unix: 2_000,
                },
            ]
        );
        let many: Vec<NeedAlert> = (0..150)
            .map(|n| alert(&format!("W{n}X"), Some(14.0 + f64::from(n) / 1e3), vec![]))
            .collect();
        let spots = panadapter_spots(&many, 2_000);
        assert_eq!(spots.len(), TCI_SPOTS_MAX);
        assert_eq!(spots[TCI_SPOTS_MAX - 1].call, "W99X");
    }

    /// The class names a panadapter knows: CW, SSB for phone, DIGI for a data mode not named.
    #[test]
    fn a_spot_is_labelled_with_its_mode_or_its_class() {
        let mut a = alert("K1ABC", Some(14.2), vec![]);
        let mut labels = Vec::new();
        for (class, exact) in [
            ("CW", None),
            ("Phone", None),
            ("Digital", None),
            ("Digital", Some("FT4")),
            ("Phone", Some(" ")),
        ] {
            a.mode = class.into();
            a.exact_mode = exact.map(str::to_string);
            labels.push(mode_label(&a));
        }
        assert_eq!(labels, ["CW", "SSB", "DIGI", "FT4", "SSB"]);
    }

    /// Each need's colour is the Needed board's, as the dark theme draws it, read from the board's
    /// own stylesheet: the panadapter and the board can never disagree about what a colour means.
    /// The tag that colours a row is the first reason to work the station; DXpedition, POTA and
    /// SOTA only colour a row that has nothing else.
    #[test]
    fn each_needs_colour_is_the_boards() {
        let css = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/styles.css"),
        )
        .expect("the board's stylesheet");
        // The first definition of each is the dark theme's (the light theme's block follows it).
        let colour = |var: &str| -> u32 {
            let at = css
                .find(&format!("{var}:"))
                .unwrap_or_else(|| panic!("{var} in styles.css"));
            let hex: String = css[at + var.len() + 1..]
                .trim_start()
                .trim_start_matches('#')
                .chars()
                .take(6)
                .collect();
            0xFF00_0000 | u32::from_str_radix(&hex, 16).expect("a #rrggbb colour")
        };
        let pairs = [
            (NeedTag::Wanted, "--need-watch"),
            (NeedTag::NewEntity, "--need-entity"),
            (NeedTag::NewZone, "--need-zone"),
            (NeedTag::NewIota, "--need-iota"),
            (NeedTag::NewState, "--need-state"),
            (NeedTag::NewGrid, "--need-grid"),
            (NeedTag::NewBand, "--need-band"),
            (NeedTag::NewMode, "--need-mode"),
            (NeedTag::NewPark, "--need-pota"),
            (NeedTag::Confirm, "--need-confirm"),
            (NeedTag::Dxped, "--need-dxped"),
            (NeedTag::Pota, "--need-pota"),
            (NeedTag::Sota, "--need-sota"),
        ];
        for (tag, var) in pairs {
            assert_eq!(argb(&[tag]), colour(var), "{tag:?} is {var}");
        }
        assert_eq!(
            argb(&[NeedTag::Dxped, NeedTag::NewBand]),
            colour("--need-band"),
            "a DXpedition label does not colour a new band"
        );
    }
}
