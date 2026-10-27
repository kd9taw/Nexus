//! Self-spot: the operator's own activation, posted to pota.app AND the DX cluster, once per press.
//!
//! One policy for the desktop's "Spot me" and the Remote page's, so they cannot drift:
//!
//! - **Both targets, always.** pota.app is where POTA hunters look; the cluster is where DX
//!   chasers do. Each press tries both and reports each on its own, so one failing never hides
//!   the other.
//! - **The comment is the operator's,** "QRV" when left blank and "QRT" when Stop offers it;
//!   it goes to both targets, so a QRT never reads on the cluster as a fresh spot.
//! - **Each park its own pota.app spot.** A two-fer's parks are posted one by one, each under
//!   the repeat limit and the login latch on its own, as a hunter's list shows one park a row;
//!   the cluster takes one line naming them all ("POTA US-1234 US-5678 QRV").
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

/// pota.app's form requires a comment ("Comment is required"), so a blank one is this: a plain
/// "on the air", with no mode word (the website reads one out of a comment to change the spot's
/// mode) and never "QRT", which ends the spot.
const DEFAULT_COMMENT: &str = "QRV";

/// The longest comment taken: kept short, because a cluster node shows only the start of a spot's
/// comment, and the cluster's carries the program and park ahead of it.
const COMMENT_MAX: usize = 30;

/// How much of a spot's comment a cluster node shows (a DXSpider-style node's comment column). The
/// cluster line's program, parks and comment are fitted to it ([`cluster_comment`]).
const CLUSTER_COLUMN: usize = 30;

/// The comment that ends a spot.
const QRT: &str = "QRT";

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
    /// The activation's reference as the contacts carry it: one park, or a two-fer's as a comma
    /// list.
    pub reference: String,
    pub dial_hz: u64,
    /// The pota.app name for the mode the station is on, if pota.app lists one.
    pub mode: Option<&'static str>,
    /// The spot's comment, already through [`spot_comment`].
    pub comment: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoActivation,
    /// The park or the dial is not the one the operator confirmed.
    Moved,
    /// A contest that allows spotting only over amateur RF is running (Winter Field Day:
    /// *"You may spot yourself and others only via amateur RF."*), so nothing is posted.
    RfOnly,
    /// The comment holds something other than printable ASCII, or is longer than [`COMMENT_MAX`].
    Comment,
}

impl Context {
    /// Each park of the activation, in the order the operator gave them.
    fn parks(&self) -> Vec<&str> {
        self.reference
            .split(',')
            .map(str::trim)
            .filter(|park| !park.is_empty())
            .collect()
    }

    /// Everything comes from the station; `reference` and `dial_hz` are only what the operator's
    /// confirm showed, checked against it. `comment` is the operator's own words.
    ///
    /// One frequency other than the dial is taken: a QRT on the frequency this activation was
    /// last spotted on, which is where the spot it ends stands — after a QSY, a QRT on the new
    /// dial would read on the cluster as a fresh spot there.
    pub fn confirmed(
        engine: &Engine,
        reference: &str,
        dial_hz: u64,
        comment: &str,
    ) -> Result<Self, Refusal> {
        Self::confirmed_in(&GATE, engine, reference, dial_hz, comment)
    }

    fn confirmed_in(
        gate: &Gate,
        engine: &Engine,
        reference: &str,
        dial_hz: u64,
        comment: &str,
    ) -> Result<Self, Refusal> {
        // Before anything else: during such a contest no target is tried at all.
        if engine.internet_spot_block().is_some() {
            return Err(Refusal::RfOnly);
        }
        let (program, active) = engine.activation().ok_or(Refusal::NoActivation)?;
        if active != reference {
            return Err(Refusal::Moved);
        }
        let comment = spot_comment(comment);
        if engine.settings().dial_hz() != dial_hz {
            let spotted = gate
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .spotted
                .clone();
            let ends_the_spot = comment.as_deref() == Some(QRT)
                && spotted.is_some_and(|(at, hz)| at == active && hz == dial_hz);
            if !ends_the_spot {
                return Err(Refusal::Moved);
            }
        }
        let comment = comment.ok_or(Refusal::Comment)?;
        Ok(Self {
            call: engine.settings().mycall.clone(),
            program,
            reference: active,
            dial_hz,
            mode: pota_mode(engine),
            comment,
        })
    }
}

/// The comment as it is sent: upper case, one space between words, [`DEFAULT_COMMENT`] when
/// blank. `None` for anything but printable ASCII (a line break would end the cluster's line and
/// start a command) or for more than [`COMMENT_MAX`] characters.
fn spot_comment(raw: &str) -> Option<String> {
    if !raw.bytes().all(|b| b == b' ' || b.is_ascii_graphic()) {
        return None;
    }
    let words = raw.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>();
    if words.is_empty() {
        return Some(DEFAULT_COMMENT.into());
    }
    let comment = words.join(" ").to_ascii_uppercase();
    (comment.len() <= COMMENT_MAX).then_some(comment)
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
        // PSK31, QPSK31 or JTTY: the Keyboard section is all three, and pota.app lists no JTTY.
        OperatingMode::Keyboard => {
            Some(engine.keyboard_mode_name()).filter(|m| POTA_APP_MODES.contains(m))
        }
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

/// What pota.app did with one park's spot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParkReport {
    pub reference: String,
    pub pota: PotaResult,
}

/// One press: pota.app's answer for each park, in the activation's order, and the cluster's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub parks: Vec<ParkReport>,
    pub cluster: ClusterResult,
}

impl Report {
    #[cfg(test)]
    pub fn any_posted(&self) -> bool {
        self.parks.iter().any(|p| p.pota == PotaResult::Posted)
            || self.cluster == ClusterResult::Queued
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
    /// Target, park (the cluster's: every park of the line), frequency and comment: a QRT is
    /// not a repeat of the spot it ends.
    recent: Vec<(Target, String, String, String, Instant)>,
    /// The activation (as [`Context::reference`]) and dial a target last took a spot for, other
    /// than a QRT: where a QRT is sent ([`Context::confirmed`]).
    spotted: Option<(String, u64)>,
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
            spotted: None,
        }))
    }

    fn forget_spotted(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .spotted = None;
    }
}

impl GateState {
    fn repeat(
        &mut self,
        target: Target,
        park: &str,
        ctx: &Context,
        freq: &str,
        now: Instant,
    ) -> bool {
        self.recent
            .retain(|(.., at)| now.saturating_duration_since(*at) < REPEAT);
        self.recent
            .iter()
            .any(|(t, r, f, c, _)| *t == target && r == park && f == freq && *c == ctx.comment)
    }
    fn took(&mut self, target: Target, park: &str, ctx: &Context, freq: &str, now: Instant) {
        self.recent
            .push((target, park.into(), freq.into(), ctx.comment.clone(), now));
        if ctx.comment != QRT {
            self.spotted = Some((ctx.reference.clone(), ctx.dial_hz));
        }
    }
}

/// The process's one gate: the desktop and the Remote station share a session.
static GATE: Gate = Gate::new();

/// Stop: the activation's spot is over, so no QRT may later go on the frequency it was spotted
/// on — the same park started again in this session is a new activation, spotted afresh.
pub fn activation_ended() {
    GATE.forget_spotted();
}

/// Post one press through the real doors (the refusing ones in a test build).
pub fn send(ctx: &Context) -> Report {
    send_with(&GATE, Instant::now(), ctx, post_pota, post_cluster)
}

/// One press. The cluster goes first (it only queues), then pota.app, park by park; no result
/// changes whether another is tried.
pub fn send_with(
    gate: &Gate,
    now: Instant,
    ctx: &Context,
    mut post_pota: impl FnMut(&SpotPost) -> Result<SpotAnswer, String>,
    post_cluster: impl FnOnce(f64, &str, &str) -> Result<(), String>,
) -> Report {
    let mut gate = gate.0.lock().unwrap_or_else(PoisonError::into_inner);
    let call = ctx.call.trim().to_ascii_uppercase();
    let freq = khz_text(ctx.dial_hz);
    let freq_mhz = ctx.dial_hz as f64 / 1e6;
    let parks = ctx.parks();
    let every_park = |pota: PotaResult| {
        parks
            .iter()
            .map(|park| ParkReport {
                reference: (*park).into(),
                pota,
            })
            .collect()
    };
    if tempo_app::bandplan::band_for_dial(freq_mhz).is_none() {
        return Report {
            parks: every_park(PotaResult::Invalid),
            cluster: ClusterResult::Invalid,
        };
    }
    // The cluster's line names every park, space-separated as a hunter reads them.
    let line_parks = parks.join(" ");
    // Each target by its own call rule: a long portable call pota.app takes may be too long for
    // the cluster's, and that must not cost the pota.app spot.
    let cluster = if !crate::is_real_call(&call) {
        ClusterResult::Invalid
    } else if gate.repeat(Target::Cluster, &line_parks, ctx, &freq, now) {
        ClusterResult::Throttled
    } else {
        let comment = cluster_comment(&ctx.program, &parks, &ctx.comment);
        match post_cluster(freq_mhz, &call, &comment) {
            Ok(()) => {
                gate.took(Target::Cluster, &line_parks, ctx, &freq, now);
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
    let mode = ctx.mode.filter(|m| POTA_APP_MODES.contains(m));
    let mut pota_park = |park: &str| match mode {
        _ if ctx.program != "POTA" => PotaResult::NotPark,
        _ if !website_call(&call) || !website_reference(park) => PotaResult::Invalid,
        None => PotaResult::UnknownMode,
        Some(_) if gate.pota_off => PotaResult::LoginRequired,
        Some(_) if gate.repeat(Target::Pota, park, ctx, &freq, now) => PotaResult::Throttled,
        Some(mode) => {
            let spot = SpotPost {
                activator: call.clone(),
                spotter: call.clone(),
                frequency: freq.clone(),
                reference: park.into(),
                mode: mode.into(),
                source: SOURCE.into(),
                comments: ctx.comment.clone(),
            };
            match post_pota(&spot) {
                Ok(SpotAnswer::Posted) => {
                    gate.took(Target::Pota, park, ctx, &freq, now);
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
    let parks = parks
        .iter()
        .map(|park| ParkReport {
            reference: (*park).into(),
            pota: pota_park(park),
        })
        .collect();
    Report { parks, cluster }
}

/// The cluster line's comment: the program, the parks and the operator's comment, fitted to the
/// node's comment column ([`CLUSTER_COLUMN`]) by naming the later parks as a count ("POTA US-1234
/// +2 QRT") — never by cutting the comment, so a QRT always reads. The first park is always named;
/// a comment too long for the column after it is the operator's, and goes as typed.
fn cluster_comment(program: &str, parks: &[&str], comment: &str) -> String {
    let line = |named: usize| {
        let mut words = vec![program.to_string()];
        words.extend(parks[..named].iter().map(|park| park.to_string()));
        if named < parks.len() {
            words.push(format!("+{}", parks.len() - named));
        }
        words.push(comment.to_string());
        words.join(" ")
    };
    (1..=parks.len())
        .rev()
        .map(line)
        .find(|text| text.len() <= CLUSTER_COLUMN)
        .unwrap_or_else(|| line(parks.len().min(1)))
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

/// The cluster half, after [`Context::confirmed`] — which refuses first while a contest that
/// allows spotting only over amateur RF runs — so it queues without asking again.
#[cfg(not(test))]
fn post_cluster(freq_mhz: f64, call: &str, comment: &str) -> Result<(), String> {
    crate::queue_spot(freq_mhz, call.into(), comment.into())
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
            comment: DEFAULT_COMMENT.into(),
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
            // Every park of the press is answered alike.
            |spot| {
                posts.borrow_mut().0.push(spot.clone());
                pota.clone()
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

    /// A one-park press's report, the park [`ctx`]'s.
    fn one(pota: PotaResult, cluster: ClusterResult) -> Report {
        Report {
            parks: vec![ParkReport {
                reference: "US-0001".into(),
                pota,
            }],
            cluster,
        }
    }

    fn both() -> Report {
        one(PotaResult::Posted, ClusterResult::Queued)
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
            vec![(14.285, "W9XYZ".into(), "POTA US-0001 QRV".into())]
        );
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::json!({
                "parks": [{"reference": "US-0001", "pota": "posted"}],
                "cluster": "queued"
            })
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
        assert_eq!(first.parks[0].pota, PotaResult::LoginRequired);
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
        assert_eq!(second.parks[0].pota, PotaResult::LoginRequired);
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
        assert_eq!(unreachable, one(PotaResult::Failed, ClusterResult::Queued));
        let refused = Ok(SpotAnswer::Refused {
            status: 500,
            excerpt: "oops".into(),
        });
        let no_cluster = Err("no DX cluster connected — set a cluster host in Settings".into());
        let report = press(&Gate::new(), now, &ctx(), &posts, refused, no_cluster);
        assert_eq!(report, one(PotaResult::Failed, ClusterResult::Unavailable));
        let cluster_down = press(
            &Gate::new(),
            now,
            &ctx(),
            &posts,
            Ok(SpotAnswer::Posted),
            Err("queue".into()),
        );
        assert_eq!(cluster_down, one(PotaResult::Posted, ClusterResult::Failed));
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
                one(PotaResult::Invalid, ClusterResult::Invalid),
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
        assert_eq!(report, one(PotaResult::Posted, ClusterResult::Invalid));
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
            assert_eq!(report.parks[0].pota, expected, "{bad:?}");
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
        assert_eq!(again, one(PotaResult::Throttled, ClusterResult::Throttled));
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
        assert_eq!(retry, one(PotaResult::Posted, ClusterResult::Throttled));
    }

    #[test]
    fn a_two_fer_posts_each_park_to_pota_app_on_its_own_and_both_on_one_cluster_line() {
        let (gate, posts, now) = (Gate::new(), Posts::default(), Instant::now());
        let two_fer = Context {
            reference: "US-1234,US-5678".into(),
            ..ctx()
        };
        let parks = |pota: [PotaResult; 2], cluster| Report {
            parks: vec![
                ParkReport {
                    reference: "US-1234".into(),
                    pota: pota[0],
                },
                ParkReport {
                    reference: "US-5678".into(),
                    pota: pota[1],
                },
            ],
            cluster,
        };
        let posted = PotaResult::Posted;
        assert_eq!(
            press(&gate, now, &two_fer, &posts, Ok(SpotAnswer::Posted), Ok(())),
            parks([posted, posted], ClusterResult::Queued)
        );
        let (pota, cluster) = posts.take();
        assert_eq!(
            pota.iter()
                .map(|p| p.reference.as_str())
                .collect::<Vec<_>>(),
            ["US-1234", "US-5678"]
        );
        assert!(pota
            .iter()
            .all(|p| p.comments == "QRV" && p.frequency == "14285"));
        assert_eq!(
            cluster,
            vec![(14.285, "W9XYZ".into(), "POTA US-1234 US-5678 QRV".into())]
        );
        // The repeat limit holds each park, and the cluster's line, on its own.
        let held = PotaResult::Throttled;
        let later = now + Duration::from_secs(30);
        assert_eq!(
            press(
                &gate,
                later,
                &two_fer,
                &posts,
                Ok(SpotAnswer::Posted),
                Ok(())
            ),
            parks([held, held], ClusterResult::Throttled)
        );
        let only_second = Context {
            reference: "US-5678".into(),
            ..ctx()
        };
        let report = press(
            &gate,
            later,
            &only_second,
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        assert_eq!(report.parks[0].pota, held);
        assert_eq!(report.cluster, ClusterResult::Queued);
        // A QRT ends each park's spot.
        let qrt = Context {
            comment: "QRT".into(),
            ..two_fer.clone()
        };
        assert_eq!(
            press(&gate, later, &qrt, &posts, Ok(SpotAnswer::Posted), Ok(())),
            parks([posted, posted], ClusterResult::Queued)
        );
        let (pota, cluster) = posts.take();
        assert_eq!(
            pota.iter()
                .map(|p| (p.reference.as_str(), p.comments.as_str()))
                .collect::<Vec<_>>(),
            [("US-1234", "QRT"), ("US-5678", "QRT")]
        );
        assert_eq!(cluster.last().unwrap().2, "POTA US-1234 US-5678 QRT");
        // A login refusal at the first park leaves the second unasked.
        let (gate, posts) = (Gate::new(), Posts::default());
        assert_eq!(
            press(
                &gate,
                now,
                &two_fer,
                &posts,
                Ok(SpotAnswer::LoginRequired),
                Ok(())
            ),
            parks(
                [PotaResult::LoginRequired, PotaResult::LoginRequired],
                ClusterResult::Queued
            )
        );
        assert_eq!(posts.borrow().0.len(), 1);
    }

    #[test]
    fn a_test_build_reaches_neither_network_target() {
        // The process gate and the build's own doors, exactly what the desktop command uses.
        let report = send(&Context {
            dial_hz: 21_285_000,
            ..ctx()
        });
        assert_eq!(report, one(PotaResult::Failed, ClusterResult::Failed));
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
            Context::confirmed(&engine, "US-0001", dial, "QRV"),
            Err(Refusal::NoActivation)
        );
        engine
            .set_activation("POTA", "US-0001", Vec::new())
            .unwrap();
        assert_eq!(
            Context::confirmed(&engine, "US-0002", dial, "QRV"),
            Err(Refusal::Moved)
        );
        assert_eq!(
            Context::confirmed(&engine, "US-0001", dial + 1000, "QRV"),
            Err(Refusal::Moved)
        );
        // A two-fer is confirmed as the contacts carry it, and spotted park by park.
        let mut two_fer = Engine::new("W9XYZ", "EN52", 0);
        two_fer
            .set_activation("POTA", "us-1234, US-5678", Vec::new())
            .unwrap();
        let both = Context::confirmed(&two_fer, "US-1234,US-5678", dial, "").unwrap();
        assert_eq!(both.parks(), ["US-1234", "US-5678"]);
        assert_eq!(
            Context::confirmed(&two_fer, "US-1234", dial, ""),
            Err(Refusal::Moved),
            "a press naming one park of a two-fer is not the activation the operator confirmed"
        );
        let confirmed = Context::confirmed(&engine, "US-0001", dial, " qrv  ").unwrap();
        assert_eq!(
            confirmed,
            Context {
                call: "W9XYZ".into(),
                program: "POTA".into(),
                reference: "US-0001".into(),
                dial_hz: dial,
                mode: Some("FT8"),
                comment: "QRV".into(),
            }
        );
    }

    /// The cluster line fits the node's comment column by naming the later parks as a count,
    /// never by cutting the comment: a QRT always reads.
    #[test]
    fn the_cluster_line_fits_the_comment_column_by_counting_parks_never_cutting_the_comment() {
        assert_eq!(
            cluster_comment("POTA", &["US-1234", "US-5678"], "QRT"),
            "POTA US-1234 US-5678 QRT"
        );
        let three = ["US-1234", "US-5678", "US-9012"];
        assert_eq!(
            cluster_comment("POTA", &three, "QRT"),
            "POTA US-1234 US-5678 +1 QRT"
        );
        assert!(cluster_comment("POTA", &three, "QRT").len() <= CLUSTER_COLUMN);
        assert_eq!(
            cluster_comment("POTA", &three, "CW 20 WPM"),
            "POTA US-1234 +2 CW 20 WPM"
        );
        let long = "A".repeat(COMMENT_MAX);
        assert_eq!(
            cluster_comment("POTA", &three, &long),
            format!("POTA US-1234 +2 {long}"),
            "the operator's words go whole"
        );
        // Through a press: the queued line is the fitted one.
        let (gate, posts) = (Gate::new(), Posts::default());
        let qrt = Context {
            reference: "US-1234,US-5678,US-9012".into(),
            comment: "QRT".into(),
            ..ctx()
        };
        press(
            &gate,
            Instant::now(),
            &qrt,
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        assert_eq!(posts.borrow().1[0].2, "POTA US-1234 US-5678 +1 QRT");
    }

    /// ⭐ The QRT goes where the spot it ends stands: the frequency a target last took this
    /// activation's spot on, though the dial has moved since. Only a QRT, only that frequency.
    #[test]
    fn a_qrt_goes_on_the_frequency_last_spotted_and_nothing_else_leaves_the_dial() {
        let mut engine = Engine::new("W9XYZ", "EN52", 0);
        engine
            .set_activation("POTA", "US-0001", Vec::new())
            .unwrap();
        let dial = engine.settings().dial_hz();
        let spotted = dial + 10_000;
        let gate = Gate::new();
        let confirm = |hz: u64, comment: &str| {
            Context::confirmed_in(&gate, &engine, "US-0001", hz, comment).map(|c| c.dial_hz)
        };
        assert_eq!(
            confirm(spotted, "QRT"),
            Err(Refusal::Moved),
            "never spotted"
        );
        let posts = Posts::default();
        let at = Context {
            dial_hz: spotted,
            ..ctx()
        };
        press(
            &gate,
            Instant::now(),
            &at,
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        assert_eq!(confirm(spotted, "qrt"), Ok(spotted));
        assert_eq!(confirm(spotted, "QRV"), Err(Refusal::Moved), "only a QRT");
        assert_eq!(confirm(spotted + 1_000, "QRT"), Err(Refusal::Moved));
        assert_eq!(confirm(dial, "QRT"), Ok(dial), "the dial, as always");
        // Stop, and the same park started again later: its old spot is not this activation's.
        gate.forget_spotted();
        assert_eq!(
            confirm(spotted, "QRT"),
            Err(Refusal::Moved),
            "a stopped activation's spot"
        );
        assert_eq!(confirm(dial, "QRT"), Ok(dial));
        press(
            &gate,
            Instant::now(),
            &at,
            &posts,
            Ok(SpotAnswer::Posted),
            Ok(()),
        );
        engine
            .set_activation("POTA", "US-0002", Vec::new())
            .unwrap();
        assert_eq!(
            Context::confirmed_in(&gate, &engine, "US-0002", spotted, "QRT"),
            Err(Refusal::Moved),
            "another activation's spot"
        );
    }

    #[test]
    fn the_comment_is_the_operators_own_and_blank_is_qrv() {
        let mut engine = Engine::new("W9XYZ", "EN52", 0);
        let dial = engine.settings().dial_hz();
        engine
            .set_activation("POTA", "US-0001", Vec::new())
            .unwrap();
        let comment = |c: &str| Context::confirmed(&engine, "US-0001", dial, c).map(|x| x.comment);
        assert_eq!(comment(""), Ok("QRV".into()));
        assert_eq!(comment("   "), Ok("QRV".into()));
        assert_eq!(comment("qrt"), Ok("QRT".into()));
        assert_eq!(comment(" CW  20 wpm "), Ok("CW 20 WPM".into()));
        assert_eq!(
            comment(&"A".repeat(COMMENT_MAX)),
            Ok("A".repeat(COMMENT_MAX))
        );
        // Nothing that could end a cluster line or that either form would not take.
        for bad in [
            "QRV\r\nSET/NAME X",
            "QRV\n",
            "tab\there",
            "caf\u{e9}",
            &"A".repeat(COMMENT_MAX + 1),
        ] {
            assert_eq!(comment(bad), Err(Refusal::Comment), "{bad:?}");
        }
        // A moved park is still reported as moved, whatever the comment.
        assert_eq!(
            Context::confirmed(&engine, "US-0002", dial, "\n"),
            Err(Refusal::Moved)
        );
    }

    #[test]
    fn the_comment_goes_to_both_targets_and_a_qrt_is_not_a_repeat_of_the_spot() {
        let (gate, posts, now) = (Gate::new(), Posts::default(), Instant::now());
        assert_eq!(
            press(&gate, now, &ctx(), &posts, Ok(SpotAnswer::Posted), Ok(())),
            both()
        );
        let qrt = Context {
            comment: "QRT".into(),
            ..ctx()
        };
        let later = now + Duration::from_secs(20);
        assert_eq!(
            press(&gate, later, &qrt, &posts, Ok(SpotAnswer::Posted), Ok(())),
            both()
        );
        // The same QRT again inside the minute is a repeat, held as before.
        assert_eq!(
            press(&gate, later, &qrt, &posts, Ok(SpotAnswer::Posted), Ok(())),
            one(PotaResult::Throttled, ClusterResult::Throttled)
        );
        let (pota, cluster) = posts.into_inner();
        assert_eq!(
            pota.iter().map(|p| p.comments.as_str()).collect::<Vec<_>>(),
            ["QRV", "QRT"]
        );
        assert_eq!(
            cluster.iter().map(|c| c.2.as_str()).collect::<Vec<_>>(),
            ["POTA US-0001 QRV", "POTA US-0001 QRT"]
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
        // JTTY is the Keyboard section too, and pota.app lists no JTTY: no mode, never PSK31.
        engine.set_operating_mode("jtty", false);
        assert_eq!(mode(&engine), None, "JTTY");
        engine.set_operating_mode("keyboard", false);
        engine
            .set_psk_mode(tempo_core::psk::PskModeKind::Qpsk31, false)
            .unwrap();
        assert_eq!(mode(&engine), Some("QPSK31"), "control: PSK's own sub-mode");
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
