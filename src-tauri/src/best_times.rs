//! `get_best_times`: when and on what band to work one station.
//!
//! Two halves. [`place`] decides where the station is from what Nexus already holds, through the
//! same resolver the rotator's point-at-call uses ([`crate::labelled_station_fixes`] and
//! [`propagation::geo::best_fix`], then the station's DXCC entity centre), so a bearing shown
//! beside an answer can never differ from the one the rotator would turn to. It never asks the
//! network: a callbook lookup is the UI's to make, after an explicit gesture, and the answer is
//! asked for again once it lands. Then the path is predicted on a prediction thread of its own,
//! below normal priority and with no Engine lock held ([`crate::predict_path`], shared with
//! `get_path_outlook` and its kept pairs), the "now" overlay laid over it, and the whole folded
//! into an answer ([`propagation::besttimes::best_times`]).
//!
//! ⚠️ **It never calls `path_grades::keep_outlook`.** That keeps a card for grading a contact
//! logged with the station; an answer worked out from a country centre or a grid the operator
//! typed must never grade one.
//!
//! Display only: nothing here transmits, tunes or turns anything.

use propagation::besttimes::{best_times, BestAsk, BestMode, BestTimes};
use propagation::geo::{
    bearing_deg, haversine_km, is_logged_grid, maidenhead_to_latlon, StationFix,
};
use tauri::State;

use crate::{
    attach_now, below_normal, best_labelled_fix, engine_lock, labelled_station_fixes, predict_path,
    AuroraCache, FixSource, KpForecastCache, PathInputs, PathStation, PropCache, ProtonCache,
    SharedEngine,
};

/// What an answer's location rests on, for the basis line.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Basis {
    /// `grid`, `position` (a callbook's coordinates) or `entity` (the DXCC entity's centre).
    kind: &'static str,
    /// For a grid or a position: `typed`, `logForm`, `heard`, `callbook` or `logged`.
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    grid: Option<String>,
}

/// Where [`place`] put the station.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    /// The call (or grid) as asked, normalised.
    call: String,
    at: (f64, f64),
    basis: Basis,
    /// The far end in a kept prediction's key: the grid, or the position to two decimals.
    key: String,
}

/// The cases with no answer, as codes the UI owns the words for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoPlace {
    /// The operator's own grid is not set.
    NoGrid,
    /// Blank, or nothing shaped like a callsign.
    EnterCall,
    /// The country file cannot place the call.
    Unplaceable,
    /// Only the entity centre is known, and it is the operator's own entity: the centre says
    /// nothing about a station in the same country.
    SameEntity,
    /// Maritime or aeronautical mobile, with no grid heard or logged for it.
    Maritime,
    /// The operator's own call or square.
    You,
}

impl NoPlace {
    fn code(self) -> &'static str {
        match self {
            NoPlace::NoGrid => "noGrid",
            NoPlace::EnterCall => "enterCall",
            NoPlace::Unplaceable => "unplaceable",
            NoPlace::SameEntity => "sameEntity",
            NoPlace::Maritime => "maritime",
            NoPlace::You => "you",
        }
    }
}

/// `s` upper-cased with every space removed.
fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Shaped like a callsign: 3 to 15 of letters, digits and strokes, with a letter and a digit.
fn call_shaped(s: &str) -> bool {
    (3..=15).contains(&s.len())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '/')
        && s.chars().any(|c| c.is_ascii_alphabetic())
        && s.chars().any(|c| c.is_ascii_digit())
}

fn grid4(g: &str) -> String {
    g.trim()
        .chars()
        .take(4)
        .collect::<String>()
        .to_ascii_uppercase()
}

fn source_word(s: FixSource) -> &'static str {
    match s {
        FixSource::LogForm => "logForm",
        FixSource::Heard => "heard",
        FixSource::Callbook => "callbook",
        FixSource::Logged => "logged",
    }
}

fn at_grid(call: String, grid: &str, from: &'static str) -> Option<Placed> {
    let g = grid.trim().to_ascii_uppercase();
    let at = maidenhead_to_latlon(&g)?;
    Some(Placed {
        call,
        at,
        key: g.clone(),
        basis: Basis {
            kind: "grid",
            from: Some(from),
            grid: Some(g),
        },
    })
}

fn position_key((lat, lon): (f64, f64)) -> String {
    format!("{lat:.2},{lon:.2}")
}

/// Where `asked`'s station is, from the operator's call and grid, an optional grid the operator
/// typed for it, and the labelled fixes the engine holds for it ([`labelled_station_fixes`]).
pub(crate) fn place(
    asked: &str,
    typed_grid: Option<&str>,
    mycall: &str,
    mygrid: &str,
    fixes: &[(FixSource, StationFix)],
) -> Result<Placed, NoPlace> {
    if maidenhead_to_latlon(mygrid.trim()).is_none() {
        return Err(NoPlace::NoGrid);
    }
    let call = normalise(asked);
    if call.is_empty() {
        return Err(NoPlace::EnterCall);
    }
    // A bare grid in the box: the answer for that square.
    if is_logged_grid(&call) {
        if grid4(&call) == grid4(mygrid) {
            return Err(NoPlace::You);
        }
        return at_grid(call.clone(), &call, "typed").ok_or(NoPlace::EnterCall);
    }
    if !call_shaped(&call) {
        return Err(NoPlace::EnterCall);
    }
    if !mycall.trim().is_empty() && call == normalise(mycall) {
        return Err(NoPlace::You);
    }
    if let Some(g) = typed_grid.map(str::trim).filter(|g| is_logged_grid(g)) {
        return at_grid(call, g, "typed").ok_or(NoPlace::EnterCall);
    }
    // cty strips /MM and /AM and would name the home entity, which is wrong for a ship or a plane:
    // only a grid the station sent, or the log form's, places one.
    let mobile = call.ends_with("/MM") || call.ends_with("/AM");
    let usable: Vec<(FixSource, StationFix)> = fixes
        .iter()
        .filter(|(s, _)| !mobile || matches!(s, FixSource::LogForm | FixSource::Heard))
        .cloned()
        .collect();
    if let Some((src, fix)) = best_labelled_fix(&usable) {
        if let Some(at) = fix.latlon() {
            return Ok(match fix {
                StationFix::Grid(g) => {
                    at_grid(call, &g, source_word(src)).ok_or(NoPlace::EnterCall)?
                }
                StationFix::Position(..) => Placed {
                    call,
                    at,
                    key: position_key(at),
                    basis: Basis {
                        kind: "position",
                        from: Some(source_word(src)),
                        grid: None,
                    },
                },
            });
        }
    }
    if mobile {
        return Err(NoPlace::Maritime);
    }
    let info = propagation::dxcc::resolve(&call).ok_or(NoPlace::Unplaceable)?;
    if propagation::dxcc::resolve(mycall).is_some_and(|mine| mine.entity == info.entity) {
        return Err(NoPlace::SameEntity);
    }
    let at = (info.lat, info.lon);
    Ok(Placed {
        call,
        at,
        key: position_key(at),
        basis: Basis {
            kind: "entity",
            from: None,
            grid: None,
        },
    })
}

/// What `get_best_times` answers.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BestTimesView {
    /// `answer`, or the code of a case with no answer ([`NoPlace`]).
    state: &'static str,
    /// The call (or grid) as asked, normalised.
    call: String,
    /// The DXCC entity the call belongs to, by the country file (absent for a bare grid).
    #[serde(skip_serializing_if = "Option::is_none")]
    entity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    basis: Option<Basis>,
    /// Short-path distance (km) and bearing (degrees true) to the point the answer rests on.
    #[serde(skip_serializing_if = "Option::is_none")]
    km: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bearing: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<BestTimes>,
    /// The engine the setting names, and whether it could not run (the heuristic answered).
    engine: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    engine_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    power: Option<propagation::AssumedPower>,
    #[serde(skip_serializing_if = "Option::is_none")]
    storm: Option<propagation::storm::StormNote>,
    /// The "now" line: a flare, a proton event, a watch.
    #[serde(skip_serializing_if = "Option::is_none")]
    space_now: Option<propagation::pathnow::SpaceNow>,
}

impl BestTimesView {
    fn none(state: &'static str, call: String, engine: String) -> BestTimesView {
        BestTimesView {
            state,
            entity: propagation::dxcc::resolve(&call).map(|i| i.entity.to_string()),
            call,
            basis: None,
            km: None,
            bearing: None,
            answer: None,
            engine,
            engine_fallback: false,
            power: None,
            storm: None,
            space_now: None,
        }
    }
}

/// The bands the operator's licence lets them transmit on in `mode`; `None` for the `Open` class,
/// which filters nothing. Display only: the transmit gate stays the authority.
fn tx_bands(class: tempo_app::settings::LicenseClass, mode: BestMode) -> Option<Vec<String>> {
    use tempo_app::settings::{LicenseClass, OperatingMode};
    if class == LicenseClass::Open {
        return None;
    }
    let section = match mode {
        BestMode::Ft8 | BestMode::Ft4 => OperatingMode::Digital,
        BestMode::Cw => OperatingMode::Cw,
        BestMode::Ssb => OperatingMode::Phone,
    };
    Some(
        tempo_app::bandplan::licensed_bands(class, section)
            .into_iter()
            .filter(|b| b.tx)
            .map(|b| b.band)
            .collect(),
    )
}

/// Best times to work `call` (or the square `call` names): `grid` is a grid the operator typed
/// for it this session, `mode` one of FT8, FT4, CW, SSB (FT8 when absent or unknown).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn get_best_times(
    call: String,
    grid: Option<String>,
    mode: Option<String>,
    state: State<'_, SharedEngine>,
    cache: State<'_, PropCache>,
    kp_cache: State<'_, KpForecastCache>,
    aurora: State<'_, AuroraCache>,
    protons: State<'_, ProtonCache>,
) -> Result<BestTimesView, String> {
    let mode = mode
        .as_deref()
        .and_then(BestMode::parse)
        .unwrap_or(BestMode::Ft8);
    let asked = normalise(&call);
    let (station, placed, tx) = {
        let eng = engine_lock(&state);
        let station = PathStation::read(&eng);
        let fixes = labelled_station_fixes(&eng, &asked);
        let st = eng.settings();
        let placed = place(&asked, grid.as_deref(), &st.mycall, &st.mygrid, &fixes);
        (station, placed, tx_bands(st.license_class, mode))
    };
    let placed = match placed {
        Ok(p) => p,
        Err(why) => return Ok(BestTimesView::none(why.code(), asked, station.prop_engine)),
    };
    let inputs = PathInputs::read(&cache, &kp_cache, &aurora, &protons)?;
    below_normal(move || answer(&station, &inputs, placed, mode, tx.as_deref())).await
}

/// The answer for a placed station: predict (kept pairs reused), lay the "now" overlay, fold.
/// Never keeps a card for contact grading (see the module header).
fn answer(
    station: &PathStation,
    inputs: &PathInputs,
    placed: Placed,
    mode: BestMode,
    tx: Option<&[String]>,
) -> BestTimesView {
    let me = maidenhead_to_latlon(&station.mygrid);
    let mut pred = predict_path(station, inputs, placed.at, &placed.key);
    if let Some(me) = me {
        attach_now(&mut pred, me, placed.at, &inputs.now_inputs, inputs.t);
    }
    let answer = best_times(&BestAsk {
        sp: &pred,
        lp: pred.long_path.as_deref(),
        mode,
        tx_bands: tx,
        forecast: inputs.forecast.as_ref(),
        now: inputs.t,
    });
    let entity = (placed.basis.from != Some("typed") || !is_logged_grid(&placed.call))
        .then(|| propagation::dxcc::resolve(&placed.call))
        .flatten()
        .map(|i| i.entity.to_string());
    BestTimesView {
        state: "answer",
        entity,
        km: me.map(|me| haversine_km(me, placed.at)),
        bearing: me.map(|me| bearing_deg(me, placed.at)),
        call: placed.call,
        basis: Some(placed.basis),
        answer: Some(answer),
        engine: station.prop_engine.clone(),
        engine_fallback: pred.engine_fallback,
        power: Some(station.power),
        storm: pred.storm,
        space_now: pred.space_now.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use propagation::geo::StationFix as F;

    const MYGRID: &str = "FN42";
    const MYCALL: &str = "W1AW";

    fn g(s: &str) -> F {
        F::Grid(s.into())
    }

    #[test]
    fn the_answer_rests_on_the_same_fix_the_rotator_aims_at_for_every_basis() {
        let cases: Vec<Vec<(FixSource, F)>> = vec![
            vec![
                (FixSource::LogForm, g("JO21ev")),
                (FixSource::Heard, g("JO21")),
            ],
            vec![
                (FixSource::Heard, g("IN53")),
                (FixSource::Logged, g("IN53UI")),
            ],
            vec![
                (FixSource::Callbook, g("JO22")),
                (FixSource::Callbook, F::Position(52.1, 4.6)),
            ],
            vec![(FixSource::Logged, g("PM95"))],
            vec![],
        ];
        for fixes in cases {
            let plain: Vec<F> = fixes.iter().map(|(_, f)| f.clone()).collect();
            let aim = crate::aim_at_call(MYGRID, "EA8ABC", plain).unwrap();
            let p = place("EA8ABC", None, MYCALL, MYGRID, &fixes).unwrap();
            let me = maidenhead_to_latlon(MYGRID).unwrap();
            assert!(
                (bearing_deg(me, p.at) - aim.bearing).abs() < 1e-9,
                "{fixes:?}: {} vs {}",
                bearing_deg(me, p.at),
                aim.bearing
            );
        }
    }

    #[test]
    fn each_basis_says_where_it_came_from() {
        let p = place(
            "ea8abc",
            None,
            MYCALL,
            MYGRID,
            &[(FixSource::Heard, g("IL18"))],
        )
        .unwrap();
        assert_eq!((p.basis.kind, p.basis.from), ("grid", Some("heard")));
        assert_eq!(p.basis.grid.as_deref(), Some("IL18"));
        assert_eq!(p.call, "EA8ABC");
        let p = place(
            "EA8ABC",
            None,
            MYCALL,
            MYGRID,
            &[(FixSource::Callbook, F::Position(28.1, -15.4))],
        )
        .unwrap();
        assert_eq!((p.basis.kind, p.basis.from), ("position", Some("callbook")));
        assert_eq!(p.key, "28.10,-15.40");
        let p = place("EA8ABC", None, MYCALL, MYGRID, &[]).unwrap();
        assert_eq!((p.basis.kind, p.basis.from), ("entity", None));
        // A grid typed for the call wins over the centre, labelled as typed.
        let p = place("EA8ABC", Some("il28"), MYCALL, MYGRID, &[]).unwrap();
        assert_eq!((p.basis.kind, p.basis.from), ("grid", Some("typed")));
        // A bare grid in the box answers for that square.
        let p = place(" jo21 ", None, MYCALL, MYGRID, &[]).unwrap();
        assert_eq!((p.call.as_str(), p.basis.from), ("JO21", Some("typed")));
    }

    #[test]
    fn the_cases_with_no_answer() {
        assert_eq!(place("EA8ABC", None, MYCALL, "", &[]), Err(NoPlace::NoGrid));
        assert_eq!(
            place("   ", None, MYCALL, MYGRID, &[]),
            Err(NoPlace::EnterCall)
        );
        assert_eq!(
            place("HELLO", None, MYCALL, MYGRID, &[]),
            Err(NoPlace::EnterCall)
        );
        assert_eq!(place("w1aw", None, MYCALL, MYGRID, &[]), Err(NoPlace::You));
        assert_eq!(place("FN42", None, MYCALL, MYGRID, &[]), Err(NoPlace::You));
        // Negative control: someone else's square answers.
        assert!(place("FN31", None, MYCALL, MYGRID, &[]).is_ok());
    }

    #[test]
    fn a_maritime_mobile_is_placed_only_by_a_grid_heard_or_logged_on_the_form() {
        assert_eq!(
            place("G4ABC/MM", None, MYCALL, MYGRID, &[]),
            Err(NoPlace::Maritime)
        );
        // A callbook answer is the home address: still unknown.
        let book = [(FixSource::Callbook, g("IO91"))];
        assert_eq!(
            place("G4ABC/MM", None, MYCALL, MYGRID, &book),
            Err(NoPlace::Maritime)
        );
        let heard = [(FixSource::Heard, g("HM55"))];
        let p = place("G4ABC/MM", None, MYCALL, MYGRID, &heard).unwrap();
        assert_eq!(p.basis.grid.as_deref(), Some("HM55"));
        assert_eq!(
            place("N1ABC/AM", None, MYCALL, MYGRID, &[]),
            Err(NoPlace::Maritime)
        );
    }

    #[test]
    fn a_centre_in_the_operators_own_entity_gives_no_answer_and_a_neighbours_does() {
        // W1AW is in the United States: a US call known only by its centre has no answer …
        assert_eq!(
            place("K6XYZ", None, MYCALL, MYGRID, &[]),
            Err(NoPlace::SameEntity)
        );
        // … but with a grid it does, and Canada's centre does.
        assert!(place(
            "K6XYZ",
            None,
            MYCALL,
            MYGRID,
            &[(FixSource::Heard, g("CM87"))]
        )
        .is_ok());
        let p = place("VE3ABC", None, MYCALL, MYGRID, &[]).unwrap();
        assert_eq!(p.basis.kind, "entity");
    }

    #[test]
    fn the_kept_pair_key_tells_engines_and_positions_apart() {
        let wx = propagation::SpaceWx::default();
        let k = |engine: &str, dx: &str| {
            crate::path_outlook::key(0, MYGRID, dx, engine, Some(100.0), 0.0, &wx)
        };
        assert_ne!(k("p533", "JO21"), k("heuristic", "JO21"));
        let a = position_key((52.104, 4.6));
        let b = position_key((52.3, 4.6));
        assert_ne!(k("p533", &a), k("p533", &b));
        // Two decimals: a jitter in the fourth place is the same position.
        assert_eq!(position_key((52.1004, 4.6)), position_key((52.1, 4.6)));
    }

    #[test]
    fn an_answer_never_keeps_a_card_for_contact_grading() {
        let station = PathStation {
            mygrid: MYGRID.into(),
            // P.533 answers carry the long path, which a kept grading card needs.
            prop_engine: "p533".into(),
            power: propagation::AssumedPower {
                watts: 100.0,
                source: propagation::PowerSource::Default,
                amplifier: false,
            },
            ant_gain_dbi: 0.0,
        };
        let t = crate::now_unix();
        // Space weather stamped as measured now: the stamps `get_path_outlook` would keep a card on.
        let stamps = propagation::model::SpaceWxStamps {
            sfi_at: Some(t),
            kp_at: Some(t),
            ..Default::default()
        };
        let inputs = PathInputs {
            t,
            wx: propagation::SpaceWx::default(),
            stamps: Some(stamps),
            forecast: None,
            oval: None,
            kp_now: Some(2.0),
            now_inputs: Default::default(),
        };
        let placed = place("JO21", None, MYCALL, MYGRID, &[]).unwrap();
        let v = answer(&station, &inputs, placed, BestMode::Ft8, None);
        assert_eq!(v.state, "answer");
        assert!(v.answer.as_ref().is_some_and(|a| a.per_mode));
        assert!(!crate::path_grades::holds_outlook_for(MYGRID, "JO21"));
        // Positive control: the same prediction handed to the grading keep, as `get_path_outlook`
        // does, is kept — so the assertion above can fail.
        let at = maidenhead_to_latlon("JO22").unwrap();
        let pred = predict_path(&station, &inputs, at, "JO22");
        crate::path_grades::keep_outlook(MYGRID, "JO22", t, &pred, inputs.stamps.as_ref());
        assert!(crate::path_grades::holds_outlook_for(MYGRID, "JO22"));
    }

    #[test]
    fn the_licence_filter_follows_the_mode_and_open_filters_nothing() {
        use tempo_app::settings::LicenseClass;
        assert_eq!(tx_bands(LicenseClass::Open, BestMode::Ft8), None);
        let tech = tx_bands(LicenseClass::Technician, BestMode::Ft8).unwrap();
        assert!(!tech.iter().any(|b| b == "20m"));
        assert!(tech.iter().any(|b| b == "10m"));
    }
}
