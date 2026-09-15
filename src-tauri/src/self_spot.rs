//! Self-spot: the operator's own activation, posted to pota.app AND the DX cluster, once per press.
//!
//! One policy for the desktop's "Spot me" and the Remote page's, so they cannot drift:
//!
//! - **Both targets, always.** pota.app is where POTA hunters look; the cluster is where DX
//!   chasers do. Each press tries both and reports each on its own, so one failing never hides
//!   the other.
//! - **Own call, current park, dial and mode only.** [`Context::confirmed`] reads all four from the
//!   station, and refuses when the park or dial is no longer the one the operator confirmed. A
//!   caller cannot name a different call: there is no field for one.
//! - **Validated before anything leaves:** the dial inside an amateur band (or neither target is
//!   tried); the callsign by each target's own rule (pota.app's website form, the cluster's
//!   `is_real_call`); the park reference by the website form's rule; a mode pota.app lists.
//! - **Gentle on repeats:** a target that took a spot for this park and frequency in the last
//!   [`REPEAT`] is not sent it again, and says so. Nothing ever retries.
//! - **A login refusal turns pota.app off for the session** ([`Gate`]); the cluster spot carries on.
//!   Building login support would be credential handling, which needs its own approval.
//!
//! ⛔ A test build has no path to either network target: the real doors are compiled only outside
//! tests, and the test build's doors refuse. Tests hand [`send_with`] their own posters.
use propagation::live::pota::{spot_excerpt, SpotAnswer, SpotPost};
use serde::Serialize;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use tempo_app::{dto::Tier, engine::Engine, settings::OperatingMode};

/// The `source` pota.app shows for the spot. The website sends "Web", PoLo "Ham2K Portable
/// Logger", hunterlog "hunterlog"; each names the app that posted it.
pub const SOURCE: &str = "Nexus";

/// pota.app's form requires a comment ("Comment is required"). A plain "on the air": no mode word
/// (the website reads one out of a comment to change the spot's mode) and never "QRT", which ends
/// the spot.
const COMMENTS: &str = "QRV";

/// A target that took a spot for a park and frequency is not sent the same one again for this long.
pub const REPEAT: Duration = Duration::from_secs(60);

/// The modes Nexus can be on that pota.app's own mode list names. That list is the website's ADIF
/// table (app bundle, 2026-09-14: `{mode:"SSB",submodes:["LSB","USB"],…,pota:"PHONE"}`,
/// `{mode:"MFSK",submodes:[…,"FT4","JS8",…,"FST4","Q65"],…}`, `{mode:"PSK",submodes:[…,"PSK31",
/// …,"QPSK31",…]}` and so on). FT2, the Tempo modes and FST4W are not in it.
const POTA_APP_MODES: [&str; 14] = [
    "SSB", "FM", "AM", "CW", "RTTY", "PSK31", "QPSK31", "FT8", "FT4", "FST4", "Q65", "MSK144",
    "JT65", "JS8",
];

/// What the operator confirmed, read back from the station.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub call: String,
    pub program: String,
    pub reference: String,
    pub dial_hz: u64,
    /// The pota.app name for the mode the station is on, if pota.app lists one.
    pub mode: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoActivation,
    /// The park or the dial is not the one the operator confirmed.
    Moved,
}

impl Context {
    /// Everything comes from the station; `reference` and `dial_hz` are only what the operator's
    /// confirm showed, checked against it.
    pub fn confirmed(engine: &Engine, reference: &str, dial_hz: u64) -> Result<Self, Refusal> {
        let (program, active) = engine.activation().ok_or(Refusal::NoActivation)?;
        if active != reference || engine.settings().dial_hz() != dial_hz {
            return Err(Refusal::Moved);
        }
        Ok(Self {
            call: engine.settings().mycall.clone(),
            program,
            reference: active,
            dial_hz,
            mode: pota_mode(engine),
        })
    }
}

fn pota_mode(engine: &Engine) -> Option<&'static str> {
    match engine.settings().operating_mode {
        OperatingMode::Digital => match engine.tier() {
            Tier::Ft8 => Some("FT8"),
            Tier::Ft4 => Some("FT4"),
            Tier::Fst4 => Some("FST4"),
            Tier::Q65 => Some("Q65"),
            Tier::Msk144 => Some("MSK144"),
            Tier::Jt65 => Some("JT65"),
            Tier::Js8 => Some("JS8"),
            // Not in pota.app's list, or a beacon mode with no contact to spot.
            Tier::TempoFast | Tier::TempoDeep | Tier::Ft2 | Tier::Fst4w | Tier::Wspr => None,
        },
        OperatingMode::Phone => {
            let word = engine.rig_mode_effective();
            Some(if word.contains("FM") {
                "FM"
            } else if word == "AM" {
                "AM"
            } else {
                "SSB"
            })
        }
        OperatingMode::Cw => Some("CW"),
        OperatingMode::Rtty => Some("RTTY"),
        OperatingMode::Keyboard => Some(match engine.psk_mode().0 {
            tempo_core::psk::PskModeKind::Bpsk31 => "PSK31",
            tempo_core::psk::PskModeKind::Qpsk31 => "QPSK31",
        }),
    }
}

/// What pota.app did with one press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PotaResult {
    Posted,
    /// pota.app asked for a login, now or earlier this session.
    LoginRequired,
    /// Unreachable, or it refused for another reason.
    Failed,
    Throttled,
    /// A SOTA activation: pota.app takes parks only.
    NotPark,
    /// The station is on a mode pota.app does not list.
    UnknownMode,
    Invalid,
}

/// What the DX cluster did with one press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClusterResult {
    /// Queued for the connected node(s), which send it.
    Queued,
    /// No cluster node connected.
    Unavailable,
    Failed,
    Throttled,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub pota: PotaResult,
    pub cluster: ClusterResult,
}

impl Report {
    pub fn any_posted(self) -> bool {
        self.pota == PotaResult::Posted || self.cluster == ClusterResult::Queued
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Pota,
    Cluster,
}

/// Session state: pota.app's login latch and the recent spots the repeat limit reads. Held for a
/// whole press, so two presses (the desktop's and the Remote page's) can never both slip past the
/// limit.
pub struct Gate(Mutex<GateState>);

struct GateState {
    pota_off: bool,
    recent: Vec<(Target, String, String, Instant)>,
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    pub const fn new() -> Self {
        Self(Mutex::new(GateState {
            pota_off: false,
            recent: Vec::new(),
        }))
    }
}

impl GateState {
    fn repeat(&mut self, target: Target, reference: &str, freq: &str, now: Instant) -> bool {
        self.recent
            .retain(|(.., at)| now.saturating_duration_since(*at) < REPEAT);
        self.recent
            .iter()
            .any(|(t, r, f, _)| *t == target && r == reference && f == freq)
    }
    fn took(&mut self, target: Target, reference: &str, freq: &str, now: Instant) {
        self.recent
            .push((target, reference.into(), freq.into(), now));
    }
}

/// The process's one gate: the desktop and the Remote station share a session.
static GATE: Gate = Gate::new();

/// Post one press through the real doors (the refusing ones in a test build).
pub fn send(ctx: &Context) -> Report {
    send_with(&GATE, Instant::now(), ctx, post_pota, post_cluster)
}

/// One press. The cluster goes first (it only queues), then pota.app; neither result changes
/// whether the other is tried.
pub fn send_with(
    gate: &Gate,
    now: Instant,
    ctx: &Context,
    post_pota: impl FnOnce(&SpotPost) -> Result<SpotAnswer, String>,
    post_cluster: impl FnOnce(f64, &str, &str) -> Result<(), String>,
) -> Report {
    let mut gate = gate.0.lock().unwrap_or_else(PoisonError::into_inner);
    let call = ctx.call.trim().to_ascii_uppercase();
    let freq = khz_text(ctx.dial_hz);
    let freq_mhz = ctx.dial_hz as f64 / 1e6;
    if tempo_app::bandplan::band_for_dial(freq_mhz).is_none() {
        return Report {
            pota: PotaResult::Invalid,
            cluster: ClusterResult::Invalid,
        };
    }
    // Each target by its own call rule: a long portable call pota.app takes may be too long for
    // the cluster's, and that must not cost the pota.app spot.
    let cluster = if !crate::is_real_call(&call) {
        ClusterResult::Invalid
    } else if gate.repeat(Target::Cluster, &ctx.reference, &freq, now) {
        ClusterResult::Throttled
    } else {
        let comment = format!("{} {}", ctx.program, ctx.reference);
        match post_cluster(freq_mhz, &call, &comment) {
            Ok(()) => {
                gate.took(Target::Cluster, &ctx.reference, &freq, now);
                ClusterResult::Queued
            }
            // `post_spot` names the cluster only when no node is connected.
            Err(e) if e.contains("cluster") => ClusterResult::Unavailable,
            Err(e) => {
                eprintln!(
                    "self-spot: the DX cluster spot failed: {}",
                    spot_excerpt(&e)
                );
                ClusterResult::Failed
            }
        }
    };
    let pota = match ctx.mode.filter(|m| POTA_APP_MODES.contains(m)) {
        _ if ctx.program != "POTA" => PotaResult::NotPark,
        _ if !website_call(&call) || !website_reference(&ctx.reference) => PotaResult::Invalid,
        None => PotaResult::UnknownMode,
        Some(_) if gate.pota_off => PotaResult::LoginRequired,
        Some(_) if gate.repeat(Target::Pota, &ctx.reference, &freq, now) => PotaResult::Throttled,
        Some(mode) => {
            let spot = SpotPost {
                activator: call.clone(),
                spotter: call,
                frequency: freq.clone(),
                reference: ctx.reference.clone(),
                mode: mode.into(),
                source: SOURCE.into(),
                comments: COMMENTS.into(),
            };
            match post_pota(&spot) {
                Ok(SpotAnswer::Posted) => {
                    gate.took(Target::Pota, &ctx.reference, &freq, now);
                    PotaResult::Posted
                }
                Ok(SpotAnswer::LoginRequired) => {
                    gate.pota_off = true;
                    eprintln!(
                        "self-spot: pota.app now asks for a login; its spot path is off until Nexus restarts"
                    );
                    PotaResult::LoginRequired
                }
                Ok(SpotAnswer::Refused { status, excerpt }) => {
                    eprintln!("self-spot: pota.app refused the spot (HTTP {status}): {excerpt}");
                    PotaResult::Failed
                }
                Err(e) => {
                    eprintln!("self-spot: pota.app was not reached: {}", spot_excerpt(&e));
                    PotaResult::Failed
                }
            }
        }
    };
    Report { pota, cluster }
}

/// kHz as text at 100 Hz, the way pota.app's feed carries it ("14222", "10120.5").
fn khz_text(dial_hz: u64) -> String {
    let tenths = (dial_hz + 50) / 100;
    match tenths % 10 {
        0 => format!("{}", tenths / 10),
        d => format!("{}.{d}", tenths / 10),
    }
}

/// The website form's `validCallsignRegex`:
/// `/^(?:[A-Z\d]{1,4}\/)?[A-Z\d]{1,3}\d[A-Z\d]*(?:\/[A-Z\d]{1,4})?$/i`.
fn website_call(call: &str) -> bool {
    let affix =
        |s: &str| (1..=4).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric());
    // 1–3 characters, a digit, then any: a digit at index 1, 2 or 3.
    let base = |s: &str| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_alphanumeric())
            && s.bytes().skip(1).take(3).any(|b| b.is_ascii_digit())
    };
    match call.split('/').collect::<Vec<_>>()[..] {
        [b] => base(b),
        [a, b] => (affix(a) && base(b)) || (base(a) && affix(b)),
        [a, b, c] => affix(a) && base(b) && affix(c),
        _ => false,
    }
}

/// The website form's `validReferenceRegex`: `/^[A-Z0-9]{1,2}-[0-9]{4,5}$/`.
fn website_reference(reference: &str) -> bool {
    reference.split_once('-').is_some_and(|(prefix, number)| {
        (1..=2).contains(&prefix.len())
            && prefix
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            && (4..=5).contains(&number.len())
            && number.bytes().all(|b| b.is_ascii_digit())
    })
}

#[cfg(not(test))]
fn post_pota(spot: &SpotPost) -> Result<SpotAnswer, String> {
    propagation::live::pota::post_spot(env!("CARGO_PKG_VERSION"), spot)
}

#[cfg(not(test))]
fn post_cluster(freq_mhz: f64, call: &str, comment: &str) -> Result<(), String> {
    crate::post_spot(freq_mhz, call.into(), comment.into())
}

/// The test build's doors. Neither names the cluster, so a refusal reads as `failed`, never as
/// the `unavailable` the real cluster door would answer with no node connected.
#[cfg(test)]
const TEST_BUILD: &str = "test build: nothing is posted";

#[cfg(test)]
fn post_pota(_: &SpotPost) -> Result<SpotAnswer, String> {
    Err(TEST_BUILD.into())
}

#[cfg(test)]
fn post_cluster(_: f64, _: &str, _: &str) -> Result<(), String> {
    Err(TEST_BUILD.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn ctx() -> Context {
        Context {
            call: "W9XYZ".into(),
            program: "POTA".into(),
            reference: "US-0001".into(),
            dial_hz: 14_285_000,
            mode: Some("SSB"),
        }
    }

    type Posts = RefCell<(Vec<SpotPost>, Vec<(f64, String, String)>)>;

    /// A press with recording posters that answer `pota` and `cluster`.
    fn press(
        gate: &Gate,
        now: Instant,
        ctx: &Context,
        posts: &Posts,
        pota: Result<SpotAnswer, String>,
        cluster: Result<(), String>,
    ) -> Report {
        send_with(
            gate,
            now,
            ctx,
            |spot| {
                posts.borrow_mut().0.push(spot.clone());
                pota
            },
            |freq, call, comment| {
                posts
                    .borrow_mut()
                    .1
                    .push((freq, call.into(), comment.into()));
                cluster
            },
        )
    }

    fn both() -> Report {
        Report {
            pota: PotaResult::Posted,
            cluster: ClusterResult::Queued,
        }
    }

    #[test]
    fn a_press_sends_both_targets_the_websites_exact_seven_fields() {
        let (gate, posts) = (Gate::new(), Posts::default());
        let report = press(
            &gate,
            Instant::now(),
            &ctx(),
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        assert_eq!(report, both());
        let (pota, cluster) = posts.into_inner();
        assert_eq!(
            serde_json::to_value(&pota).unwrap(),
            serde_json::json!([{
                "activator": "W9XYZ", "spotter": "W9XYZ", "frequency": "14285",
                "reference": "US-0001", "mode": "SSB", "source": "Nexus", "comments": "QRV"
            }])
        );
        assert_eq!(
            cluster,
            vec![(14.285, "W9XYZ".into(), "POTA US-0001".into())]
        );
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::json!({"pota": "posted", "cluster": "queued"})
        );
    }

    #[test]
    fn a_login_refusal_turns_pota_off_for_the_session_and_keeps_the_cluster() {
        let (gate, posts, now) = (Gate::new(), Posts::default(), Instant::now());
        let first = press(
            &gate,
            now,
            &ctx(),
            &posts,
            Ok(SpotAnswer::LoginRequired),
            Ok(()),
        );
        assert_eq!(first.pota, PotaResult::LoginRequired);
        assert_eq!(first.cluster, ClusterResult::Queued);
        // Later, elsewhere (no repeat limit in play): pota.app is not even asked, the cluster is.
        let elsewhere = Context {
            dial_hz: 7_285_000,
            ..ctx()
        };
        let later = now + REPEAT * 2;
        let second = press(
            &gate,
            later,
            &elsewhere,
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        assert_eq!(second.pota, PotaResult::LoginRequired);
        assert_eq!(second.cluster, ClusterResult::Queued);
        let (pota, cluster) = posts.into_inner();
        assert_eq!((pota.len(), cluster.len()), (1, 2));
        // Positive control: a fresh session posts to pota.app again.
        let fresh = Posts::default();
        assert_eq!(
            press(
                &Gate::new(),
                later,
                &elsewhere,
                &fresh,
                Ok(SpotAnswer::Posted),
                Ok(())
            ),
            both()
        );
    }

    #[test]
    fn a_failure_on_one_target_still_reports_the_other() {
        let now = Instant::now();
        let posts = Posts::default();
        let unreachable = press(
            &Gate::new(),
            now,
            &ctx(),
            &posts,
            Err("timed out".into()),
            Ok(()),
        );
        assert_eq!(
            unreachable,
            Report {
                pota: PotaResult::Failed,
                cluster: ClusterResult::Queued
            }
        );
        let refused = Ok(SpotAnswer::Refused {
            status: 500,
            excerpt: "oops".into(),
        });
        let no_cluster = Err("no DX cluster connected — set a cluster host in Settings".into());
        let report = press(&Gate::new(), now, &ctx(), &posts, refused, no_cluster);
        assert_eq!(
            report,
            Report {
                pota: PotaResult::Failed,
                cluster: ClusterResult::Unavailable
            }
        );
        let cluster_down = press(
            &Gate::new(),
            now,
            &ctx(),
            &posts,
            Ok(SpotAnswer::Posted),
            Err("queue".into()),
        );
        assert_eq!(
            cluster_down,
            Report {
                pota: PotaResult::Posted,
                cluster: ClusterResult::Failed
            }
        );
        assert!(!report.any_posted() && cluster_down.any_posted() && unreachable.any_posted());
    }

    #[test]
    fn nothing_leaves_for_a_call_or_dial_that_fails_validation() {
        let now = Instant::now();
        for bad in [
            Context {
                call: "".into(),
                ..ctx()
            },
            Context {
                call: "NOCALL".into(),
                ..ctx()
            },
            Context {
                call: "W9XYZ/P/QRP/X".into(),
                ..ctx()
            },
            // Between the bands, and nowhere at all.
            Context {
                dial_hz: 15_000_000,
                ..ctx()
            },
            Context {
                dial_hz: 0,
                ..ctx()
            },
        ] {
            let posts = Posts::default();
            let report = press(
                &Gate::new(),
                now,
                &bad,
                &posts,
                Ok(SpotAnswer::Posted),
                Ok(()),
            );
            assert_eq!(
                report,
                Report {
                    pota: PotaResult::Invalid,
                    cluster: ClusterResult::Invalid
                },
                "{bad:?}"
            );
            assert_eq!(posts.into_inner(), (vec![], vec![]));
        }
        // Positive control: the website's own call shapes pass.
        for good in ["W9XYZ", "VE3/W9XYZ", "W9XYZ/P", "2E0ABC"] {
            let ok = Context {
                call: good.into(),
                ..ctx()
            };
            assert_eq!(
                press(
                    &Gate::new(),
                    now,
                    &ok,
                    &Posts::default(),
                    Ok(SpotAnswer::Posted),
                    Ok(())
                ),
                both(),
                "{good}"
            );
        }
        // Each target judges the call by its own rule: pota.app's form takes a 13-character
        // portable call the cluster's 10-character rule refuses, and the pota.app spot still goes.
        let posts = Posts::default();
        let long = Context {
            call: "KH6/W9XYZ/QRP".into(),
            ..ctx()
        };
        let report = press(
            &Gate::new(),
            now,
            &long,
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        assert_eq!(
            report,
            Report {
                pota: PotaResult::Posted,
                cluster: ClusterResult::Invalid
            }
        );
        assert_eq!(posts.borrow().1.len(), 0);
    }

    #[test]
    fn pota_app_is_skipped_for_a_summit_a_bad_park_or_an_unlisted_mode_and_the_cluster_still_goes()
    {
        let now = Instant::now();
        for (bad, expected) in [
            (
                Context {
                    program: "SOTA".into(),
                    reference: "W7A/MN-001".into(),
                    ..ctx()
                },
                PotaResult::NotPark,
            ),
            // The station's normalizer takes a 3-character prefix; pota.app's form does not.
            (
                Context {
                    reference: "VE3-1234".into(),
                    ..ctx()
                },
                PotaResult::Invalid,
            ),
            (
                Context {
                    reference: "us-0001".into(),
                    ..ctx()
                },
                PotaResult::Invalid,
            ),
            (
                Context {
                    mode: None,
                    ..ctx()
                },
                PotaResult::UnknownMode,
            ),
            (
                Context {
                    mode: Some("FT2"),
                    ..ctx()
                },
                PotaResult::UnknownMode,
            ),
        ] {
            let posts = Posts::default();
            let report = press(
                &Gate::new(),
                now,
                &bad,
                &posts,
                Ok(SpotAnswer::Posted),
                Ok(()),
            );
            assert_eq!(report.pota, expected, "{bad:?}");
            assert_eq!(report.cluster, ClusterResult::Queued, "{bad:?}");
            let (pota, cluster) = posts.into_inner();
            assert!(pota.is_empty(), "{bad:?}");
            assert_eq!(cluster.len(), 1);
        }
    }

    #[test]
    fn a_repeat_press_within_a_minute_is_held_per_park_and_frequency() {
        let (gate, posts, now) = (Gate::new(), Posts::default(), Instant::now());
        assert_eq!(
            press(&gate, now, &ctx(), &posts, Ok(SpotAnswer::Posted), Ok(())),
            both()
        );
        let again = press(
            &gate,
            now + Duration::from_secs(30),
            &ctx(),
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        assert_eq!(
            again,
            Report {
                pota: PotaResult::Throttled,
                cluster: ClusterResult::Throttled
            }
        );
        assert_eq!(posts.borrow().0.len() + posts.borrow().1.len(), 2);
        // A QSY is a new spot, and so is the same one a minute later.
        let qsy = Context {
            dial_hz: 14_290_000,
            ..ctx()
        };
        assert_eq!(
            press(
                &gate,
                now + Duration::from_secs(30),
                &qsy,
                &posts,
                Ok(SpotAnswer::Posted),
                Ok(())
            ),
            both()
        );
        assert_eq!(
            press(
                &gate,
                now + REPEAT,
                &ctx(),
                &posts,
                Ok(SpotAnswer::Posted),
                Ok(())
            ),
            both()
        );
        // A target that failed holds nothing back: pressing again retries just that press.
        let (gate, posts) = (Gate::new(), Posts::default());
        press(&gate, now, &ctx(), &posts, Err("timed out".into()), Ok(()));
        let retry = press(&gate, now, &ctx(), &posts, Ok(SpotAnswer::Posted), Ok(()));
        assert_eq!(
            retry,
            Report {
                pota: PotaResult::Posted,
                cluster: ClusterResult::Throttled
            }
        );
    }

    #[test]
    fn a_test_build_reaches_neither_network_target() {
        // The process gate and the build's own doors, exactly what the desktop command uses.
        let report = send(&Context {
            dial_hz: 21_285_000,
            ..ctx()
        });
        assert_eq!(
            report,
            Report {
                pota: PotaResult::Failed,
                cluster: ClusterResult::Failed
            }
        );
        assert!(!report.any_posted());
    }

    #[test]
    fn frequency_text_is_khz_at_100_hz() {
        assert_eq!(khz_text(14_285_000), "14285");
        assert_eq!(khz_text(10_120_500), "10120.5");
        assert_eq!(khz_text(7_074_049), "7074");
        assert_eq!(khz_text(7_074_051), "7074.1");
    }

    #[test]
    fn the_context_is_the_stations_own_and_refuses_a_moved_park_or_dial() {
        let mut engine = Engine::new("W9XYZ", "EN52", 0);
        let dial = engine.settings().dial_hz();
        assert_eq!(
            Context::confirmed(&engine, "US-0001", dial),
            Err(Refusal::NoActivation)
        );
        engine.set_activation("POTA", "US-0001").unwrap();
        assert_eq!(
            Context::confirmed(&engine, "US-0002", dial),
            Err(Refusal::Moved)
        );
        assert_eq!(
            Context::confirmed(&engine, "US-0001", dial + 1000),
            Err(Refusal::Moved)
        );
        let confirmed = Context::confirmed(&engine, "US-0001", dial).unwrap();
        assert_eq!(
            confirmed,
            Context {
                call: "W9XYZ".into(),
                program: "POTA".into(),
                reference: "US-0001".into(),
                dial_hz: dial,
                mode: Some("FT8"),
            }
        );
    }

    #[test]
    fn the_mode_is_the_one_the_station_is_on_and_only_one_pota_app_lists() {
        let mut engine = Engine::new("W9XYZ", "EN52", 0);
        let mode = |e: &Engine| pota_mode(e);
        assert_eq!(mode(&engine), Some("FT8"));
        engine.set_tier(Tier::Ft4);
        assert_eq!(mode(&engine), Some("FT4"));
        engine.set_tier(Tier::TempoFast);
        assert_eq!(mode(&engine), None);
        for (section, expected) in [
            ("phone", "SSB"),
            ("cw", "CW"),
            ("rtty", "RTTY"),
            ("keyboard", "PSK31"),
        ] {
            engine.set_operating_mode(section, false);
            assert_eq!(mode(&engine), Some(expected), "{section}");
        }
        // Every name Nexus can send is one pota.app lists.
        for tier in Tier::ALL {
            let mut e = Engine::new("W9XYZ", "EN52", 0);
            e.set_tier(tier);
            if let Some(m) = pota_mode(&e) {
                assert!(POTA_APP_MODES.contains(&m), "{m}");
            }
        }
    }
}
