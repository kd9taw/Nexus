//! Each contest's own FT8/FT4 messages, by value.
//!
//! Every contest the picker offers is run through the Contest screen's two roles — Running
//! (`fieldday-run`) and S&P (`fieldday-sp`) — against a scripted partner that answers the way
//! WSJT-X 3.0.2 does in the matching Special Operating Activity (`widgets/mainwindow.cpp`:
//! `genCQMsg` 9466-9531, `genStdMsgs` 9564-9722, `processMessage` 8918-9453, and the Tx1
//! switch-off at 8238-8254). What this station puts on the air is read off the planned over —
//! the same text the modulator encodes — and every over is pushed through the FT8 encoder to
//! name its 77-bit type and the text a receiving station decodes.
//!
//! The partner is a script, not a decoder: it reacts to the message it was addressed and
//! repeats its last over when it hears anything else, which is what WSJT-X's auto-sequencer
//! does with a message it has no arm for.
//!
//! The Operate cockpit's own gestures are driven through the verbs its commands call: a
//! double-click through `work_station` (the `call_station` command), the strip's Call CQ through
//! `start_running`.

use super::*;
use crate::dto::Tier;
use crate::test_util::StoredLog;

/// A station already in the contest's log, worked again on FT8: logged a dupe, worth nothing.
mod picked_dupe;

const ME: &str = "K1ABC";
const MY_GRID: &str = "FN42";
const DX: &str = "W9XYZ";
const DX_GRID: &str = "EN37";
/// The SNR every one of the partner's overs is decoded at, so the report this station sends
/// it is `-12`.
const DX_SNR: i32 = -12;
/// The report the partner sends this station.
const DX_RPT: &str = "-07";
/// Slots per scene: eight overs each way, enough for the longest exchange and a re-arm.
const SLOTS: u64 = 16;

/// Every contest the picker offers (`ui/src/fdEvent.ts` `CONTESTS`, in its order), after the
/// blank default a fresh install carries.
const CONTESTS: [&str; 19] = [
    "",
    "arrlfd",
    "wfd",
    "arrlss_cw",
    "arrlss_ssb",
    "arrlvhf_jan",
    "arrlvhf_jun",
    "arrlvhf_sep",
    "cqww_cw",
    "cqww_ssb",
    "cqww_rtty",
    "cqwpx_cw",
    "cqwpx_ssb",
    "cqp",
    "ilqp",
    "nyqp",
    "ohqp",
    "tnqp",
    "txqp",
];

/// The WSJT-X 3.0.2 Special Operating Activity a station in this contest runs
/// (`Configuration::SpecialOperatingActivity`). ARRL Field Day has FIELD_DAY and the three
/// ARRL VHF contests have NA_VHF; no other contest on the list has an activity of its own,
/// so the partner there sends the standard messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    FieldDay,
    NaVhf,
    None,
}

fn act_of(contest: &str) -> Act {
    match contest {
        "" | "arrlfd" | "wfd" => Act::FieldDay,
        "arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep" => Act::NaVhf,
        _ => Act::None,
    }
}

/// The partner, as WSJT-X 3.0.2 with auto-sequencing on.
struct Partner {
    act: Act,
    /// What it sends on its next over.
    next: Option<String>,
    /// Its next over is its last: a 73, or a runner's RR73.
    last: bool,
}

impl Partner {
    /// A runner calls CQ from its first over; an S&P station waits for a CQ to answer.
    fn new(act: Act, running: bool) -> Self {
        let next = running.then(|| match act {
            Act::FieldDay => format!("CQ FD {DX} {DX_GRID}"),
            Act::NaVhf => format!("CQ TEST {DX} {DX_GRID}"),
            Act::None => format!("CQ {DX} {DX_GRID}"),
        });
        Self {
            act,
            next,
            last: false,
        }
    }

    /// What WSJT-X does on decoding `mine`.
    fn hear(&mut self, mine: &str) {
        let w: Vec<&str> = mine.split_whitespace().collect();
        let to_me = w.len() >= 3 && w[0] == DX && w[1] == ME;
        let a_cq = w.first() == Some(&"CQ") && w.contains(&ME);
        let is_grid = |s: &str| {
            let b = s.as_bytes();
            s.len() == 4
                && s != "RR73"
                && (b'A'..=b'R').contains(&b[0])
                && (b'A'..=b'R').contains(&b[1])
                && b[2].is_ascii_digit()
                && b[3].is_ascii_digit()
        };
        let is_report =
            |s: &str| (s.starts_with('-') || s.starts_with('+')) && s[1..].parse::<i32>().is_ok();
        let reply = |payload: &str| format!("{ME} {DX} {payload}");
        // Double-clicking a CQ: Tx2 in the contest activities (Tx1 is switched off,
        // mainwindow.cpp 8238-8248), Tx1 with no activity.
        if a_cq && self.next.is_none() && !self.last {
            self.next = Some(match self.act {
                Act::FieldDay => reply("1D WI"),
                Act::NaVhf | Act::None => reply(DX_GRID),
            });
            return;
        }
        if !to_me {
            return;
        }
        let closing = w[2] == "RR73" || w[2] == "RRR";
        let (next, last) = match self.act {
            Act::FieldDay if w.len() == 4 => (reply("R 1D WI"), false),
            Act::FieldDay if w.len() == 5 && w[2] == "R" => (reply("RR73"), true),
            Act::NaVhf if w.len() == 3 && is_grid(w[2]) => (reply(&format!("R {DX_GRID}")), false),
            Act::NaVhf if w.len() == 4 && w[2] == "R" && is_grid(w[3]) => (reply("RR73"), true),
            Act::FieldDay | Act::NaVhf if closing => (reply("73"), true),
            Act::None if w.len() == 3 && is_grid(w[2]) => (reply(DX_RPT), false),
            Act::None if w.len() == 3 && w[2].starts_with('R') && is_report(&w[2][1..]) => {
                (reply("RR73"), true)
            }
            Act::None if w.len() == 3 && is_report(w[2]) => (reply(&format!("R{DX_RPT}")), false),
            Act::None if closing => (reply("73"), true),
            // A 73, or anything this script has no arm for: nothing new.
            _ => return,
        };
        self.next = Some(next);
        self.last = last;
    }

    /// Its over, if it has one.
    fn transmit(&mut self) -> Option<String> {
        let out = self.next.clone();
        if self.last {
            self.next = None;
        }
        out
    }
}

/// A decode of the partner's over, as the engine's decoder would deliver it.
fn heard(text: &str) -> modes::Decode {
    modes::Decode {
        message: text.into(),
        sync: 1.0,
        snr: DX_SNR,
        dt: 0.1,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

/// A station in `contest`, with the master switch on and every Settings field a contest on
/// the list sends filled for an out-of-state entrant in Massachusetts.
fn engine_for(contest: &str, grid: &str) -> Engine {
    let mut e = Engine::new(ME, grid, 0);
    let mut s = e.settings().clone();
    s.fd_event = contest.into();
    // Winter Field Day's categories are H, I, O and M; ARRL Field Day's are A to F.
    s.fd_class = if contest == "wfd" { "1H" } else { "2B" }.into();
    s.fd_section = "EMA".into();
    s.contest_qth_state = "MA".into();
    s.contest_cq_zone = 5;
    s.contest_itu_zone = 8;
    s.contest_check = "74".into();
    s.contest_category_operator = "SINGLE-OP".into();
    s.contest_category_power = "LOW".into();
    s.contest_category_assisted = "NON-ASSISTED".into();
    s.tx_even = true;
    s.fd_active = true;
    e.apply_settings(s);
    e
}

/// The text of the over `plan_tx` planned, if it keyed one — through `commit_tx`, as the
/// radio loop does.
pub(super) fn keyed(e: &mut Engine, slot: u64) -> Option<String> {
    let plan = e.plan_tx(slot)?;
    let text = match &plan.waveform {
        TxWaveform::Native { text, .. }
        | TxWaveform::Fast { text, .. }
        | TxWaveform::Deep { text, .. } => text.clone(),
        TxWaveform::Js8 { .. } => return None,
    };
    let wave = plan.waveform.build();
    let overs = e.commit_tx(&plan, wave, slot);
    (!overs.is_empty()).then_some(text)
}

/// FT8 tones back to the 77 message bits (genft8.f90: Costas at 0-6, 36-42 and 72-78; three
/// Gray-coded bits a data symbol), `None` for the all-zero frame a message the packer refused
/// leaves behind.
pub(super) fn bits77(tones: &[i32]) -> Option<Vec<i8>> {
    const COSTAS: [i32; 7] = [3, 1, 4, 0, 6, 5, 2];
    const INV_GRAY: [i8; 8] = [0, 1, 3, 2, 6, 4, 5, 7];
    if tones.len() != 79 || tones[..7] != COSTAS {
        return None;
    }
    let mut bits = Vec::with_capacity(174);
    for (k, &t) in tones.iter().enumerate() {
        if (7..36).contains(&k) || (43..72).contains(&k) {
            let v = INV_GRAY[t as usize];
            bits.extend([(v >> 2) & 1, (v >> 1) & 1, v & 1]);
        }
    }
    bits.truncate(77);
    Some(bits)
}

/// The 77-bit type `pack77` chose (`i3`, with `n3` when `i3` is 0) and the text a receiving
/// station decodes, for `text` as the FT8 encoder packs it.
pub(super) fn air_form(text: &str) -> (String, String) {
    let tones = modes::make_mode(modes::ModeKind::Ft8).encode(text);
    let Some(bits) = bits77(&tones) else {
        return ("refused".into(), String::new());
    };
    let n = |r: std::ops::Range<usize>| bits[r].iter().fold(0u8, |a, &b| a * 2 + b as u8);
    let (i3, n3) = (n(74..77), n(71..74));
    let kind = if i3 == 0 {
        format!("i3=0 n3={n3}")
    } else {
        format!("i3={i3}")
    };
    let decoded = tempo_fast::unpack77(&bits).unwrap_or_default();
    (kind, decoded)
}

/// The tones, as digits, for the byte-for-byte comparison.
pub(super) fn tones_of(kind: modes::ModeKind, text: &str) -> String {
    modes::make_mode(kind)
        .encode(text)
        .iter()
        .map(|t| char::from_digit(*t as u32, 10).unwrap_or('?'))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Running,
    SearchAndPounce,
    /// S&P the way the Operate cockpit does it: a double-click on the partner's CQ.
    DoubleClick,
    /// A double-click on the partner's CQ (the cockpit's `call_station`) from the contest's
    /// S&P as the contest switch leaves it, TX off.
    ClickFromSp,
    /// The same double-click from the contest's Running.
    ClickFromRun,
    /// The same double-click from FT8 QSO mode, the contest switch on.
    ClickFromQso,
    /// The Operate strip's Call CQ, from the contest's S&P as the switch leaves it, TX off.
    CallCq,
    /// The Operate strip's Call CQ from FT8 QSO mode, the contest switch on.
    CallCqFromQso,
    /// The Operate strip's S&P pressed during the contest's Running, the partner calling CQ.
    StripSpFromRun,
    /// The Operate strip's S&P from FT8 QSO mode, the contest switch on.
    StripSpFromQso,
}

impl Role {
    /// The partner calls CQ, and this station answers it.
    fn partner_runs(self) -> bool {
        !matches!(self, Role::Running | Role::CallCq | Role::CallCqFromQso)
    }

    /// The operator double-clicks the partner's first CQ.
    fn clicks(self) -> bool {
        matches!(
            self,
            Role::DoubleClick | Role::ClickFromSp | Role::ClickFromRun | Role::ClickFromQso
        )
    }

    /// The dump's original scenes: past the TX switch at the end, they print nothing they did
    /// not print then.
    fn first_dump(self) -> bool {
        matches!(
            self,
            Role::Running | Role::SearchAndPounce | Role::DoubleClick
        )
    }
}

/// One scene's record: every over each side sent, by slot, and what the contest log kept.
struct Scene {
    entered: Result<(), String>,
    lines: Vec<String>,
    /// This station's overs, in order.
    sent: Vec<String>,
    /// The contest log's rows: call, received fields, sent fields, dupe mark.
    logged: Vec<String>,
    /// Where the station ended: the mode, its role and its sequencer's state.
    end: String,
    /// Whether TX was still on at the end.
    tx: bool,
    /// The slots at which the radio loop would key a CW ID ("CW ID after 73").
    cw_ids: Vec<u64>,
}

fn fields(v: &[tempo_core::contest::FieldValue]) -> String {
    v.iter()
        .map(|f| format!("{}={}", f.key, f.raw))
        .collect::<Vec<_>>()
        .join(",")
}

/// Run `role` in `contest` on `tier` against a WSJT-X partner in `act`.
fn scene(contest: &str, tier: Tier, role: Role, act: Act, grid: &str) -> Scene {
    play(engine_for(contest, grid), tier, role, act)
}

/// [`scene`] on the station `e`, set up for its contest.
fn play(mut e: Engine, tier: Tier, role: Role, act: Act) -> Scene {
    e.set_tier(tier);
    let entered = match role {
        Role::Running | Role::ClickFromRun | Role::StripSpFromRun => e.set_mode("fieldday-run"),
        Role::SearchAndPounce | Role::DoubleClick => e.set_mode("fieldday-sp"),
        // The contest switch has already put the station in S&P.
        Role::ClickFromSp | Role::CallCq => Ok(()),
        Role::ClickFromQso | Role::CallCqFromQso | Role::StripSpFromQso => {
            e.set_mode("qso-monitor")
        }
    };
    // S&P is passive and does not arm TX by itself; the operator's Enable TX does.
    if matches!(role, Role::SearchAndPounce | Role::DoubleClick) {
        e.set_tx_enabled(true);
    }
    let mut partner = Partner::new(act, role.partner_runs());
    let (mut lines, mut sent) = (Vec::new(), Vec::new());
    let mut cw_ids = Vec::new();
    let mut clicked = false;
    for slot in 0..SLOTS {
        // The radio loop's deferred "Disable Tx after sending 73", once the last over has played.
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        // …and its deferred CW ID, which it keys at the same moment.
        if e.take_pending_cw_id() {
            cw_ids.push(slot);
        }
        if slot % 2 == 0 {
            if slot == 0 && matches!(role, Role::CallCq | Role::CallCqFromQso) {
                let r = e.start_running();
                lines.push(format!("  s{slot:02} Call CQ: {r:?}"));
            }
            if matches!(
                (slot, role),
                (2, Role::StripSpFromRun) | (0, Role::StripSpFromQso)
            ) {
                let r = e.start_search_and_pounce();
                lines.push(format!("  s{slot:02} S&P: {r:?}"));
            }
            if let Some(text) = keyed(&mut e, slot) {
                let (kind, decoded) = air_form(&text);
                let tones = match tier {
                    Tier::Ft4 => tones_of(modes::ModeKind::Ft4, &text),
                    _ => tones_of(modes::ModeKind::Ft8, &text),
                };
                lines.push(format!(
                    "  s{slot:02} TX {text:?} {kind} decodes {decoded:?} tones {tones}"
                ));
                partner.hear(&text);
                sent.push(text);
            }
        } else {
            // The decoder runs on the partner's period whether or not it sent anything.
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            lines.push(format!("  s{slot:02} RX {text:?}"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
            if role.clicks() && !clicked && text.starts_with("CQ ") {
                clicked = true;
                let r = e.work_station(DX, Some(DX_GRID), Some(&text), Some(DX_SNR), None);
                lines.push(format!("  s{slot:02} double-click the CQ: {r:?}"));
            }
        }
    }
    let logged = match &e.mode {
        Mode::FieldDay { station, .. } => station
            .log
            .qsos()
            .iter()
            .map(|q| {
                format!(
                    "{} rx[{}] tx[{}] mode {} {} dupe {}",
                    q.call,
                    fields(&q.rx),
                    fields(&q.tx),
                    q.mode,
                    q.submode,
                    q.dupe
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    if e.take_pending_cw_id() {
        cw_ids.push(SLOTS);
    }
    let mode = mode_line(&e);
    lines.push(format!("  end: {mode} tx={}", on_off(e.tx_enabled())));
    for l in &logged {
        lines.push(format!("  LOG {l}"));
    }
    // Where a gesture leaves the contest, the contact is in the station's own log.
    if !role.first_dump() {
        for r in e.stored_log() {
            lines.push(format!(
                "  GENERAL {} {} grid {:?} sent {:?} rcvd {:?}",
                r.call, r.mode, r.grid, r.rst_sent, r.rst_rcvd
            ));
        }
    }
    Scene {
        entered,
        lines,
        sent,
        logged,
        end: mode,
        tx: e.tx_enabled(),
        cw_ids,
    }
}

fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::Ft8 => "FT8",
        Tier::Ft4 => "FT4",
        Tier::TempoFast => "FT1",
        Tier::TempoDeep => "DX1",
        Tier::Ft2 => "FT2",
        _ => "other",
    }
}

/// A stub country file for the contests that score by where the other station is (CQ WW,
/// CQ WPX), which refuse to start without one: it places this file's two calls and nothing
/// else.
fn place(call: &str) -> Option<tempo_core::contest::CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    (up.starts_with('K') || up.starts_with('W')).then_some(tempo_core::contest::CallLocation {
        entity: "United States",
        continent: "NA",
        cq_zone: None,
    })
}

/// One scene's block in the dump: its title and what `scene` recorded.
fn block(out: &mut String, title: String, s: Scene) {
    out.push_str(&format!("[{title}] entered={:?}\n", s.entered));
    for l in s.lines {
        out.push_str(&l);
        out.push('\n');
    }
}

/// The whole map, one block per contest, tier and role, in a fixed order — written to the
/// file `CONTEST_FT_DUMP` names (or printed), so a run on the base and a run on this branch can be
/// compared byte for byte.
///
/// ⚠️ Run it alone (`-- --ignored dump_contest_ft_messages`): it installs [`place`] as the
/// country file, which is process-wide and once-only, and every other test in this binary
/// runs without one.
#[test]
#[ignore = "writes the before/after dump; run by hand"]
fn dump_contest_ft_messages() {
    let _ = tempo_core::contest::install_call_resolver(place);
    let mut out = String::new();
    for contest in CONTESTS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::Running, Role::SearchAndPounce] {
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                block(
                    &mut out,
                    format!("{contest:?} {} {role:?}", tier_name(tier)),
                    s,
                );
            }
        }
    }
    // Controls: Field Day with a six-character locator in Settings; the grid contests on
    // FT1, which keeps today's messages; a double-click S&P in Field Day and in a VHF contest.
    for role in [Role::Running, Role::SearchAndPounce] {
        let s = scene("arrlfd", Tier::Ft8, role, Act::FieldDay, "FN42ab");
        block(&mut out, format!("\"arrlfd\" FT8 {role:?} grid FN42ab"), s);
        let s = scene("arrlvhf_jan", Tier::TempoFast, role, Act::NaVhf, MY_GRID);
        block(&mut out, format!("\"arrlvhf_jan\" FT1 {role:?}"), s);
    }
    for contest in ["arrlfd", "arrlvhf_jan", "cqww_cw"] {
        let s = scene(
            contest,
            Tier::Ft8,
            Role::DoubleClick,
            act_of(contest),
            MY_GRID,
        );
        block(&mut out, format!("{contest:?} FT8 DoubleClick"), s);
    }
    // Outside any contest: Call CQ and a double-click, on FT8 and FT4.
    for tier in [Tier::Ft8, Tier::Ft4] {
        out.push_str(&no_contest(tier, true));
        out.push_str(&no_contest(tier, false));
    }
    // The cockpit's own gestures while a contest runs: a double-click on the partner's CQ, and
    // the Operate strip's Call CQ, from the contest's S&P as the switch leaves it, in every
    // contest on FT8 and FT4.
    for contest in CONTESTS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::ClickFromSp, Role::CallCq] {
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                block(
                    &mut out,
                    format!("{contest:?} {} {role:?}", tier_name(tier)),
                    s,
                );
            }
        }
    }
    // …from the contest's Running, and from FT8 QSO mode with the switch on.
    for contest in ["arrlfd", "arrlvhf_jan", "txqp", "wfd", "ilqp"] {
        for role in [Role::ClickFromRun, Role::ClickFromQso, Role::CallCqFromQso] {
            let s = scene(contest, Tier::Ft8, role, act_of(contest), MY_GRID);
            block(&mut out, format!("{contest:?} FT8 {role:?}"), s);
        }
    }
    // FT1, DX1 and FT2 have no WSJT-X contest messages.
    for tier in [Tier::TempoFast, Tier::TempoDeep, Tier::Ft2] {
        for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
            for role in [Role::Running, Role::ClickFromSp, Role::CallCq] {
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                block(
                    &mut out,
                    format!("{contest:?} {} {role:?}", tier_name(tier)),
                    s,
                );
            }
        }
    }
    // The strip's S&P during the contest's Running and from FT8 QSO mode.
    for contest in ["arrlfd", "arrlvhf_jan", "txqp", "wfd"] {
        for role in [Role::StripSpFromRun, Role::StripSpFromQso] {
            let s = scene(contest, Tier::Ft8, role, act_of(contest), MY_GRID);
            block(&mut out, format!("{contest:?} FT8 {role:?}"), s);
        }
    }
    // The contest's S&P with Tx 1st on the CQ station's period.
    for (contest, tier) in [
        ("arrlfd", Tier::Ft8),
        ("arrlvhf_jan", Tier::Ft8),
        ("txqp", Tier::Ft8),
        ("txqp", Tier::Ft4),
        ("arrlfd", Tier::TempoFast),
    ] {
        out.push_str(&scene_period(contest, tier));
    }
    // A plain gesture with a contest picked and not filled in.
    for on in [false, true] {
        out.push_str(&unfilled_contest(on));
    }
    match std::env::var("CONTEST_FT_DUMP") {
        Ok(path) => std::fs::write(&path, out).expect("the dump writes"),
        Err(_) => print!("{out}"),
    }
}

/// Call CQ (`qso-run`) or a double-click on a CQ (`qso-monitor`, then `call_station_ctx`)
/// with the master switch off, against a WSJT-X partner with no activity.
fn no_contest(tier: Tier, running: bool) -> String {
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(tier);
    let entered = if running {
        e.start_running()
    } else {
        e.set_mode("qso-monitor")
    };
    let mut partner = Partner::new(Act::None, !running);
    let mut out = format!(
        "[no contest {} {}] entered={entered:?}\n",
        tier_name(tier),
        if running { "CallCq" } else { "DoubleClick" }
    );
    let mut clicked = false;
    for slot in 0..SLOTS {
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        if slot % 2 == 0 {
            if let Some(text) = keyed(&mut e, slot) {
                let (kind, decoded) = air_form(&text);
                let kind_tones = match tier {
                    Tier::Ft4 => tones_of(modes::ModeKind::Ft4, &text),
                    _ => tones_of(modes::ModeKind::Ft8, &text),
                };
                out.push_str(&format!(
                    "  s{slot:02} TX {text:?} {kind} decodes {decoded:?} tones {kind_tones}\n"
                ));
                partner.hear(&text);
            }
        } else {
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            out.push_str(&format!("  s{slot:02} RX {text:?}\n"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
            if !running && !clicked && text.starts_with("CQ ") {
                clicked = true;
                e.set_tx_enabled(true);
                let r = e.work_station(DX, Some(DX_GRID), Some(&text), Some(DX_SNR), None);
                out.push_str(&format!("  s{slot:02} double-click the CQ: {r:?}\n"));
            }
        }
    }
    out.push_str(&format!("  end: tx={}\n", on_off(e.tx_enabled())));
    out
}

/// Where the station is: the mode, its role and its sequencer's state.
fn mode_line(e: &Engine) -> String {
    match &e.mode {
        Mode::FieldDay { station, running } => {
            format!("FieldDay running={running} state={:?}", station.state)
        }
        Mode::Qso { station, running } => {
            format!("Qso running={running} state={:?}", station.state)
        }
        Mode::Chat => "Chat".into(),
    }
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// The contest's S&P with this station's Tx 1st odd, the period the partner, a WSJT-X runner in
/// the contest's activity, calls CQ in: the partner hears only what goes out in the even periods,
/// and nothing is decoded in a period this station transmits in. Every slot is polled for this
/// station's over.
fn scene_period(contest: &str, tier: Tier) -> String {
    let mut e = engine_for(contest, MY_GRID);
    let mut s = e.settings().clone();
    s.tx_even = false;
    e.apply_settings(s);
    e.set_tier(tier);
    let entered = e.set_mode("fieldday-sp");
    e.set_tx_enabled(true);
    let mut partner = Partner::new(act_of(contest), true);
    let mut out = format!(
        "[{contest:?} {} S&P, Tx 1st odd] entered={entered:?}\n",
        tier_name(tier)
    );
    for slot in 0..SLOTS {
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        let mine = keyed(&mut e, slot);
        if let Some(text) = &mine {
            out.push_str(&format!("  s{slot:02} TX {text:?}\n"));
            if slot % 2 == 0 {
                partner.hear(text);
            }
        }
        if slot % 2 == 1 {
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            if mine.is_some() {
                out.push_str(&format!("  s{slot:02} RX missed, transmitting: {text:?}\n"));
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            }
            out.push_str(&format!("  s{slot:02} RX {text:?}\n"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
        }
    }
    out.push_str(&format!(
        "  end: {} tx={}\n",
        mode_line(&e),
        on_off(e.tx_enabled())
    ));
    if let Mode::FieldDay { station, .. } = &e.mode {
        for q in station.log.qsos() {
            out.push_str(&format!(
                "  LOG {} rx[{}] tx[{}]\n",
                q.call,
                fields(&q.rx),
                fields(&q.tx)
            ));
        }
    }
    out
}

/// A gesture with a contest picked whose exchange is not filled in (the Texas QSO Party with no
/// county or state): with Contest mode off the strip's Call CQ, a plain run; with it on the Tx6
/// row, whose own CQ the contest's Running calls there, refused for the exchange it lacks as Call
/// CQ is; against a WSJT-X partner with no activity answering it.
fn unfilled_contest(on: bool) -> String {
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.fd_event = "txqp".into();
    s.fd_active = on;
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    // The Tx6 row on this computer: the contest's Running while it shows the contest's CQ or the
    // Running calls the row's own (here a plain one).
    let entered = if on && (e.contest_running_cq().is_some() || e.contest_cq_typed()) {
        e.start_running()
    } else if on {
        e.start_cq(None)
    } else {
        e.start_running()
    };
    e.set_tx_enabled(true);
    let mut partner = Partner::new(Act::None, false);
    let mut out = format!(
        "[txqp not filled in, Contest mode {}, {}] entered={entered:?}\n",
        on_off(on),
        if on { "Tx6 CQ" } else { "Call CQ" }
    );
    for slot in 0..SLOTS {
        if e.take_pending_tx_disable() {
            e.set_tx_enabled(false);
        }
        if slot % 2 == 0 {
            if let Some(text) = keyed(&mut e, slot) {
                out.push_str(&format!("  s{slot:02} TX {text:?}\n"));
                partner.hear(&text);
            }
        } else {
            let Some(text) = partner.transmit() else {
                e.ingest_decodes_for_test(&[], slot + 1);
                continue;
            };
            out.push_str(&format!("  s{slot:02} RX {text:?}\n"));
            e.ingest_decodes_for_test(&[heard(&text)], slot + 1);
        }
    }
    out.push_str(&format!("  end: tx={}\n", on_off(e.tx_enabled())));
    for r in e.stored_log() {
        out.push_str(&format!("  GENERAL {} {}\n", r.call, r.mode));
    }
    out
}

/// What this station sends in `contest` as `role`, against a WSJT-X partner, up to and
/// including its first CQ after the contact (Running) or its last over (a double-click from
/// S&P); and the row the contest log keeps, `None` where the contact never completes.
fn expected(contest: &str, role: Role, tier: &str) -> (Vec<String>, Option<String>) {
    let row = |rx: &str, tx: &str| {
        Some(format!(
            "W9XYZ rx[{rx}] tx[{tx}] mode DIG {tier} dupe false"
        ))
    };
    let v = |s: &[&str]| s.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    match (contest, role) {
        // Field Day: what it has always sent, and the 73 WSJT-X's runner answers the caller's
        // RR73 with (Tx5).
        ("" | "arrlfd", Role::Running) => (
            v(&[
                "CQ FD K1ABC FN42",
                "W9XYZ K1ABC R 2B EMA",
                "W9XYZ K1ABC 73",
                "CQ FD K1ABC FN42",
            ]),
            row("CLASS=1D,SECTION=WI", "CLASS=2B,SECTION=EMA"),
        ),
        ("" | "arrlfd", _) => (
            v(&["W9XYZ K1ABC 2B EMA", "W9XYZ K1ABC RR73"]),
            row("CLASS=1D,SECTION=WI", "CLASS=2B,SECTION=EMA"),
        ),
        // Banned FT8/FT4: nothing new. Winter Field Day runs Field Day's exchange…
        ("wfd", Role::Running) => (
            v(&[
                "CQ FD K1ABC FN42",
                "W9XYZ K1ABC R 1H EMA",
                "CQ FD K1ABC FN42",
            ]),
            row("CLASS=1D,SECTION=WI", "CLASS=1H,SECTION=EMA"),
        ),
        ("wfd", _) => (
            v(&["W9XYZ K1ABC 1H EMA", "W9XYZ K1ABC RR73"]),
            row("CLASS=1D,SECTION=WI", "CLASS=1H,SECTION=EMA"),
        ),
        // …and the two parties' Running keeps sending what it sent, which completes nothing. A
        // double-click leaves the contest and works the station in QSO mode, as it always has:
        // Field Day's messages cannot carry the parties' exchange.
        ("ilqp" | "nyqp", Role::Running) => (v(&["CQ FD K1ABC FN42"; 8]), None),
        ("ilqp" | "nyqp", _) => (
            v(&["W9XYZ K1ABC FN42", "W9XYZ K1ABC R-12", "W9XYZ K1ABC 73"]),
            None,
        ),
        // The ARRL VHF contests: WSJT-X's NA VHF Contest messages.
        ("arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep", Role::Running) => (
            v(&[
                "CQ TEST K1ABC FN42",
                "W9XYZ K1ABC R FN42",
                "W9XYZ K1ABC 73",
                "CQ TEST K1ABC FN42",
            ]),
            row("GRID=EN37", "GRID=FN42"),
        ),
        ("arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep", _) => (
            v(&["W9XYZ K1ABC FN42", "W9XYZ K1ABC RR73"]),
            row("GRID=EN37", "GRID=FN42"),
        ),
        // Every other contest: WSJT-X has no activity for it, so the standard messages.
        (_, Role::Running) => (
            v(&[
                "CQ K1ABC FN42",
                "W9XYZ K1ABC -12",
                "W9XYZ K1ABC RR73",
                "CQ K1ABC FN42",
            ]),
            row("", ""),
        ),
        (_, _) => (
            v(&["W9XYZ K1ABC FN42", "W9XYZ K1ABC R-12", "W9XYZ K1ABC 73"]),
            row("", ""),
        ),
    }
}

/// ⭐ **Every contest on the list, both roles, FT8 and FT4, by value**: the text of each over,
/// its 77-bit type and the text a receiving station decodes, and the row the contest log
/// keeps. S&P, TX on, answers none of the partner's CQs by itself and sends nothing; the same CQ
/// double-clicked is worked to the log. CQ WW and CQ WPX need a country file this binary does
/// not install; they are in the dump, and the table that picks their messages is pinned in
/// tempo-core.
#[test]
fn every_contest_sends_its_own_messages_on_ft8_and_ft4() {
    let resolver_free = CONTESTS
        .iter()
        .filter(|c| !c.starts_with("cqww") && !c.starts_with("cqwpx"));
    for contest in resolver_free {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::Running, Role::SearchAndPounce, Role::DoubleClick] {
                let what = format!("{contest:?} {} {role:?}", tier_name(tier));
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                assert_eq!(s.entered, Ok(()), "{what}");
                if role == Role::SearchAndPounce {
                    assert_eq!(s.sent, Vec::<String>::new(), "{what}: S&P calls nobody");
                    assert_eq!(s.logged, Vec::<String>::new(), "{what}: nothing logged");
                    continue;
                }
                let (want, row) = expected(contest, role, tier_name(tier));
                assert!(
                    s.sent.len() >= want.len() && s.sent[..want.len()] == want[..],
                    "{what}: sent {:?}",
                    s.sent
                );
                if role == Role::DoubleClick {
                    assert_eq!(s.sent.len(), want.len(), "{what}: S&P sends nothing more");
                }
                assert_eq!(s.logged, row.into_iter().collect::<Vec<_>>(), "{what}");
                // Each over packs as the standard or Field Day message it reads as, and
                // decodes back to exactly its own text.
                for text in &s.sent {
                    let (kind, decoded) = air_form(text);
                    let want_kind = if text.ends_with(" EMA") {
                        "i3=0 n3=3"
                    } else {
                        "i3=1"
                    };
                    assert_eq!(kind, want_kind, "{what}: {text:?}");
                    assert_eq!(decoded, text.trim(), "{what}: {text:?}");
                }
            }
        }
    }
}

/// FT1, DX1 and FT2 have no WSJT-X contest messages: in a grid contest they keep Field
/// Day's, unchanged. Moving to FT8 changes the messages and re-arms the run onto `CQ TEST`;
/// FT8↔FT4 keeps the contact in flight.
#[test]
fn the_tier_decides_whether_the_contest_messages_apply() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::TempoFast);
    e.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ FD K1ABC FN42"));
    e.set_tier(Tier::Ft8);
    e.set_tx_enabled(true); // a mode switch that moves the dial turns TX off, as WSJT-X does
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("CQ TEST K1ABC FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 4);
    assert_eq!(keyed(&mut e, 4).as_deref(), Some("W9XYZ K1ABC R FN42"));
    e.set_tier(Tier::Ft4);
    e.set_tx_enabled(true);
    assert_eq!(
        keyed(&mut e, 6).as_deref(),
        Some("W9XYZ K1ABC R FN42"),
        "FT8 to FT4 keeps the contact"
    );
}

/// ⛔ The keying gate: a contest whose FT8/FT4 messages carry the grid does not key without a
/// valid one in Settings. Field Day's exchange carries none and still keys (the control), and
/// the same contest keys once the grid is there.
#[test]
fn a_contest_whose_messages_carry_the_grid_does_not_key_without_one() {
    let mut e = engine_for("txqp", "");
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-run").expect("the run starts");
    for slot in [0, 2, 4] {
        assert_eq!(keyed(&mut e, slot), None, "no grid, no over (slot {slot})");
    }
    let mut fd = engine_for("arrlfd", "");
    fd.set_tier(Tier::Ft8);
    fd.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut fd, 0).as_deref(), Some("CQ FD K1ABC"));
    let mut ok = engine_for("txqp", MY_GRID);
    ok.set_tier(Tier::Ft8);
    ok.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut ok, 0).as_deref(), Some("CQ K1ABC FN42"));
}

/// The passive-first rule: turning a contest on enters S&P, which calls nobody — with TX
/// enabled and a quiet band, no contest keys anything.
#[test]
fn entering_any_contest_keys_nothing() {
    for contest in CONTESTS
        .iter()
        .filter(|c| !c.starts_with("cqww") && !c.starts_with("cqwpx"))
    {
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(Tier::Ft8);
        assert!(
            matches!(e.mode, Mode::FieldDay { running: false, .. }),
            "{contest:?}: the switch enters S&P"
        );
        e.set_tx_enabled(true);
        for slot in (0..8).step_by(2) {
            assert_eq!(keyed(&mut e, slot), None, "{contest:?} slot {slot}");
            e.ingest_decodes_for_test(&[], slot + 1);
        }
    }
}

/// ⛔ The contest's S&P never starts a contact by itself, in each contest's messages, with TX on
/// or off: a CQ it hears keys nothing and queues nothing, and turning TX on afterwards keys
/// nothing either. A double-click on that CQ starts the contact, which runs to the log as before;
/// with "Disable Tx after sending 73" off, TX stays on after it and the next CQ heard is passed
/// by, as WSJT-X calls nobody the operator did not pick.
#[test]
fn the_contests_sp_calls_nobody_until_a_double_click() {
    for (contest, cq, overs, next_cq) in [
        (
            "arrlfd",
            "CQ FD W9XYZ EN37",
            &[
                ("W9XYZ K1ABC 2B EMA", "K1ABC W9XYZ R 1D WI"),
                ("W9XYZ K1ABC RR73", ""),
            ][..],
            "CQ FD K2DEF FN31",
        ),
        (
            "arrlvhf_jan",
            "CQ TEST W9XYZ EN37",
            &[
                ("W9XYZ K1ABC FN42", "K1ABC W9XYZ R EN37"),
                ("W9XYZ K1ABC RR73", ""),
            ][..],
            "CQ TEST K2DEF FN31",
        ),
        (
            "txqp",
            "CQ W9XYZ EN37",
            &[
                ("W9XYZ K1ABC FN42", "K1ABC W9XYZ -07"),
                ("W9XYZ K1ABC R-12", "K1ABC W9XYZ RR73"),
                ("W9XYZ K1ABC 73", ""),
            ][..],
            "CQ K2DEF FN31",
        ),
    ] {
        for tx_on in [true, false] {
            let what = format!("{contest} TX {}", on_off(tx_on));
            let mut e = engine_for(contest, MY_GRID);
            let mut s = e.settings().clone();
            s.disable_tx_after_73 = false;
            e.apply_settings(s);
            e.set_tier(Tier::Ft8);
            e.set_mode("fieldday-sp").expect("S&P starts");
            e.set_tx_enabled(tx_on);
            e.ingest_decodes_for_test(&[heard(cq)], 2);
            assert_eq!(queued(&e), None, "{what}: nothing queued for the CQ heard");
            assert!(listening(&e), "{what}: still listening");
            assert_eq!(
                keyed(&mut e, 2),
                None,
                "{what}: the CQ heard is not answered"
            );
            e.set_tx_enabled(true);
            assert_eq!(keyed(&mut e, 4), None, "{what}: nor once TX is on");
            // The operator double-clicks the station's next CQ.
            e.ingest_decodes_for_test(&[heard(cq)], 6);
            e.work_station(DX, Some(DX_GRID), Some(cq), Some(DX_SNR), None)
                .expect("the double-click is taken");
            let mut slot = 6;
            let mut sent = Vec::new();
            for (_, reply) in overs {
                sent.push(keyed(&mut e, slot).unwrap_or_default());
                let rx: Vec<_> = (!reply.is_empty())
                    .then(|| heard(reply))
                    .into_iter()
                    .collect();
                e.ingest_decodes_for_test(&rx, slot + 2);
                slot += 2;
            }
            let want: Vec<String> = overs.iter().map(|(o, _)| o.to_string()).collect();
            assert_eq!(sent, want, "{what}: the double-clicked contact");
            assert_eq!(rows_of(&e).len(), 1, "{what}: logged");
            assert!(e.tx_enabled, "{what}: TX stays on");
            assert!(listening(&e), "{what}: S&P listens again");
            e.ingest_decodes_for_test(&[heard(next_cq)], slot + 2);
            assert_eq!(queued(&e), None, "{what}: the next CQ is not answered");
            assert_eq!(keyed(&mut e, slot + 2), None, "{what}: and nothing keys");
            assert!(listening(&e), "{what}: still listening");
        }
    }
}

/// ⭐ Where WSJT-X has no contest messages for Field Day's exchange (ARRL Field Day on FT1, DX1
/// and FT2; Winter Field Day, which bans FT8 and FT4), the contest's S&P still answers no CQ by
/// itself, with TX on or off, and a double-click on a CQ answers it in Field Day's messages, by
/// value: the contact is logged in the contest, and the next CQ heard starts nothing.
#[test]
fn a_double_click_in_field_days_sp_answers_in_its_messages_on_every_ft_tier() {
    // (contest, tier, the answer, the class W9XYZ sends)
    for (contest, tier, answer, class) in [
        ("arrlfd", Tier::TempoFast, "W9XYZ K1ABC 2B EMA", "1D"),
        ("arrlfd", Tier::TempoDeep, "W9XYZ K1ABC 2B EMA", "1D"),
        ("arrlfd", Tier::Ft2, "W9XYZ K1ABC 2B EMA", "1D"),
        ("", Tier::Ft2, "W9XYZ K1ABC 2B EMA", "1D"),
        ("wfd", Tier::Ft8, "W9XYZ K1ABC 1H EMA", "1H"),
        ("wfd", Tier::Ft4, "W9XYZ K1ABC 1H EMA", "1H"),
        ("wfd", Tier::TempoFast, "W9XYZ K1ABC 1H EMA", "1H"),
        ("wfd", Tier::Ft2, "W9XYZ K1ABC 1H EMA", "1H"),
    ] {
        for tx_on in [true, false] {
            let what = format!("{contest:?} {} TX {}", tier_name(tier), on_off(tx_on));
            let mut e = engine_for(contest, MY_GRID);
            let mut s = e.settings().clone();
            s.disable_tx_after_73 = false;
            e.apply_settings(s);
            e.set_tier(tier);
            e.set_mode("fieldday-sp").expect("S&P starts");
            e.set_tx_enabled(tx_on);
            e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
            assert_eq!(queued(&e), None, "{what}: the CQ heard queues nothing");
            assert_eq!(keyed(&mut e, 2), None, "{what}: and keys nothing");
            e.set_tx_enabled(true);
            assert_eq!(keyed(&mut e, 4), None, "{what}: nor once TX is on");
            click(&mut e, "CQ FD W9XYZ EN37", 6);
            assert_eq!(
                keyed(&mut e, 6).as_deref(),
                Some(answer),
                "{what}: the answer"
            );
            assert!(
                matches!(e.mode, Mode::FieldDay { running: false, .. }),
                "{what}: still the contest's S&P"
            );
            e.ingest_decodes_for_test(&[heard(&format!("K1ABC W9XYZ R {class} WI"))], 8);
            assert_eq!(
                keyed(&mut e, 8).as_deref(),
                Some("W9XYZ K1ABC RR73"),
                "{what}: RR73"
            );
            assert_eq!(
                rows_of(&e),
                [format!("W9XYZ CLASS={class},SECTION=WI")],
                "{what}: logged in the contest"
            );
            e.ingest_decodes_for_test(&[], 10);
            assert!(listening(&e), "{what}: S&P listens again");
            e.ingest_decodes_for_test(&[heard("CQ FD K2DEF FN31")], 12);
            assert_eq!(queued(&e), None, "{what}: the next CQ is not answered");
            assert_eq!(keyed(&mut e, 12), None, "{what}: and nothing keys");
        }
    }
}

/// ⛔ Where Field Day's messages cannot carry the contest's exchange, a double-click on a CQ from
/// the contest's S&P works the station outside the contest, as it always has: the ARRL VHF
/// contests on FT2, MSK144 and Q65, a state QSO party on FT1, the Illinois and New York
/// parties, which ban FT8 and FT4, and both Field Days on Q65 and MSK144. The contact runs to its 73 in QSO mode, by value, and is in
/// the station's own log; nothing goes out as Field Day's `W9XYZ K1ABC` with no exchange.
#[test]
fn a_double_click_from_sp_where_field_days_messages_do_not_fit_works_the_station_as_before() {
    for (contest, tier, cq) in [
        ("arrlvhf_jan", Tier::Ft2, "CQ TEST W9XYZ EN37"),
        ("arrlvhf_jan", Tier::Msk144, "CQ TEST W9XYZ EN37"),
        ("arrlvhf_jan", Tier::Q65, "CQ TEST W9XYZ EN37"),
        ("txqp", Tier::TempoFast, "CQ W9XYZ EN37"),
        ("ilqp", Tier::Ft8, "CQ W9XYZ EN37"),
        ("nyqp", Tier::Ft4, "CQ W9XYZ EN37"),
        ("arrlfd", Tier::Q65, "CQ FD W9XYZ EN37"),
        ("arrlfd", Tier::Msk144, "CQ FD W9XYZ EN37"),
        ("wfd", Tier::Q65, "CQ FD W9XYZ EN37"),
        ("wfd", Tier::Msk144, "CQ FD W9XYZ EN37"),
    ] {
        let what = format!("{contest:?} {}", tier_name(tier));
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-sp").expect("S&P starts");
        e.set_tx_enabled(true);
        click(&mut e, cq, 2);
        let mut sent = vec![keyed(&mut e, 2)];
        e.ingest_decodes_for_test(&[heard(&format!("K1ABC W9XYZ {DX_RPT}"))], 4);
        sent.push(keyed(&mut e, 4));
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 6);
        sent.push(keyed(&mut e, 6));
        assert_eq!(
            sent,
            [
                Some("W9XYZ K1ABC FN42".to_string()),
                Some("W9XYZ K1ABC R-12".to_string()),
                Some("W9XYZ K1ABC 73".to_string()),
            ],
            "{what}: the contact"
        );
        assert!(matches!(e.mode, Mode::Qso { .. }), "{what}: QSO mode");
        assert_eq!(
            e.stored_log()
                .iter()
                .map(|r| r.call.as_str())
                .collect::<Vec<_>>(),
            [DX],
            "{what}: in the station's own log"
        );
    }
}

/// ⛔ Field Day's answer never goes out without a class and a section. With either blanked in
/// the running session, a double-click from the contest's S&P on FT1 works the station outside
/// the contest instead, and on FT8, where the contest's own messages apply, it is refused and
/// nothing is keyed.
#[test]
fn field_days_sp_sends_no_answer_without_a_class_and_section() {
    for blank in ["CLASS", "SECTION"] {
        for tier in [Tier::TempoFast, Tier::Ft8] {
            let what = format!("{blank} blank, {}", tier_name(tier));
            let mut e = engine_for("arrlfd", MY_GRID);
            e.set_tier(tier);
            e.set_mode("fieldday-sp").expect("S&P starts");
            e.set_tx_enabled(true);
            if let Mode::FieldDay { station, .. } = &mut e.mode {
                station.log.session.my_exchange.retain(|v| v.key != blank);
            }
            e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
            let r = e.work_station(DX, None, Some("CQ FD W9XYZ EN37"), Some(DX_SNR), None);
            let first = keyed(&mut e, 2);
            if tier == Tier::Ft8 {
                assert!(r.is_err(), "{what}: refused, {r:?}");
                assert_eq!(first, None, "{what}: nothing keyed");
                assert!(listening(&e), "{what}: still listening");
            } else {
                assert_eq!(r, Ok(()), "{what}");
                assert_eq!(
                    first.as_deref(),
                    Some("W9XYZ K1ABC FN42"),
                    "{what}: worked outside the contest"
                );
                assert!(matches!(e.mode, Mode::Qso { .. }), "{what}: QSO mode");
            }
        }
    }
}

/// ⛔ The wall-clock watchdog still ends a contest run, and Halt still stops a contact in
/// flight — both checked on the grid contest's messages, with the same run keying first as
/// the control.
#[test]
fn the_watchdog_and_halt_stop_a_contest_run() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    let mut s = e.settings().clone();
    s.tx_watchdog_min = 6;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ TEST K1ABC FN42"));
    let limit = 6 * 60;
    e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(limit + 1));
    assert_eq!(keyed(&mut e, 2), None, "the watchdog trips");
    assert!(!e.tx_enabled, "and disarms TX");

    let mut e = engine_for("txqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-run").expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ K1ABC FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 2);
    e.halt_tx();
    for slot in [2, 4, 6] {
        assert_eq!(
            keyed(&mut e, slot),
            None,
            "nothing after Halt (slot {slot})"
        );
    }
}

/// ⭐ A grid contest's FT8 contact is the row a typed one is: the same received and sent
/// GRID, so it dupes against a typed entry and a typed entry against it, and a rover in a
/// new grid is a new contact. S&P answers no CQ it hears, the worked station's included; a
/// duplicate the run logs is journaled at once, like a typed one.
#[test]
fn an_ft8_grid_contact_dupes_and_journals_like_a_typed_one() {
    let path = std::env::temp_dir().join(format!("contest-ft-vhf-{}.adi", std::process::id()));
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_fd_log_path(path.clone());
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-sp").expect("S&P starts");
    e.set_tx_enabled(true);
    let contact = |e: &mut Engine, from: u64| {
        click(e, "CQ TEST W9XYZ EN37", from);
        assert_eq!(keyed(e, from).as_deref(), Some("W9XYZ K1ABC FN42"));
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ R EN37")], from + 2);
        assert_eq!(keyed(e, from + 2).as_deref(), Some("W9XYZ K1ABC RR73"));
        e.ingest_decodes_for_test(&[], from + 4);
    };
    contact(&mut e, 0);
    let rows = |e: &Engine| match &e.mode {
        Mode::FieldDay { station, .. } => station
            .log
            .qsos()
            .iter()
            .map(|q| (fields(&q.rx), fields(&q.tx), q.dupe))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let ft8 = rows(&e);
    assert_eq!(ft8, vec![("GRID=EN37".into(), "GRID=FN42".into(), false)]);
    // A typed entry for the same station and grid is the duplicate…
    let typed = vec![("GRID".to_string(), "EN37".to_string())];
    assert_eq!(
        e.contest_log_manual("W9XYZ", &typed, "DIG", Some("FT8")),
        Ok(true)
    );
    assert_eq!(rows(&e)[1], ("GRID=EN37".into(), "GRID=FN42".into(), true));
    // …a rover in a new grid is not…
    let moved = vec![("GRID".to_string(), "EN38".to_string())];
    assert_eq!(
        e.contest_log_manual("W9XYZ", &moved, "DIG", Some("FT8")),
        Ok(true)
    );
    assert!(!rows(&e)[2].2);
    // …and S&P, TX on, does not answer that station's CQ again by itself.
    e.set_tx_enabled(true);
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 6);
    assert_eq!(
        keyed(&mut e, 6),
        None,
        "the same contact is not answered twice"
    );
    // A runner works whoever calls, so the same station calling again is worked; that
    // duplicate is logged, marked, and journaled before anything else is.
    e.set_mode("fieldday-run").expect("Running starts");
    assert_eq!(keyed(&mut e, 8).as_deref(), Some("CQ TEST K1ABC FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 10);
    assert_eq!(keyed(&mut e, 10).as_deref(), Some("W9XYZ K1ABC R FN42"));
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 12);
    assert_eq!(keyed(&mut e, 12).as_deref(), Some("W9XYZ K1ABC 73"));
    assert_eq!(rows(&e).len(), 4);
    assert_eq!(rows(&e)[3], ("GRID=EN37".into(), "GRID=FN42".into(), true));
    e.station.journals.settle();
    let journal = std::fs::read_to_string(&path).expect("the journal is written");
    let records = journal.to_ascii_lowercase().matches("<eor>").count();
    let _ = std::fs::remove_file(&path);
    assert_eq!(records, 4, "every row is in the journal");
}

/// Field Day's runner answers the caller's RR73 or RRR with 73 and then calls CQ, in ARRL Field
/// Day on FT8 and FT4, as WSJT-X's Field Day activity does (Tx5, mainwindow.cpp 9210-9215). A
/// bare 73 from the caller is answered with nothing, as before, and FT1 and Winter Field Day
/// close as they always have: one row each time. A confirmation from anyone else closes nothing
/// ([`only_the_callers_own_confirmation_closes_a_contest_contact`]).
#[test]
fn a_field_day_runner_answers_the_callers_rr73_and_rrr_with_73() {
    let rows = |e: &Engine| match &e.mode {
        Mode::FieldDay { station, .. } => station.log.qsos().len(),
        _ => 0,
    };
    let run = |contest: &str, tier: Tier, closing: &str| {
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("the run starts");
        let cq = keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ 1D WI")], 2);
        let exch = keyed(&mut e, 2).expect("the rogered exchange");
        assert!(exch.starts_with("W9XYZ K1ABC R "), "{exch}");
        e.ingest_decodes_for_test(&[heard(closing)], 4);
        let after = keyed(&mut e, 4);
        e.ingest_decodes_for_test(&[], 6);
        let next = keyed(&mut e, 6);
        (cq, after, next, rows(&e))
    };
    for tier in [Tier::Ft8, Tier::Ft4] {
        for closing in ["K1ABC W9XYZ RR73", "K1ABC W9XYZ RRR"] {
            let (cq, after, next, logged) = run("arrlfd", tier, closing);
            let what = format!("{} {closing:?}", tier_name(tier));
            assert_eq!(after.as_deref(), Some("W9XYZ K1ABC 73"), "{what}");
            assert_eq!(next.as_deref(), Some(cq.as_str()), "{what}: then CQ");
            assert_eq!(logged, 1, "{what}");
        }
    }
    // Nothing goes back where it never did.
    for (contest, tier, closing) in [
        ("arrlfd", Tier::Ft8, "K1ABC W9XYZ 73"),
        ("arrlfd", Tier::TempoFast, "K1ABC W9XYZ RR73"),
        ("wfd", Tier::Ft8, "K1ABC W9XYZ RR73"),
    ] {
        let (cq, after, _, logged) = run(contest, tier, closing);
        let what = format!("{contest:?} {} {closing:?}", tier_name(tier));
        assert_eq!(
            after.as_deref(),
            Some(cq.as_str()),
            "{what}: straight back to CQ"
        );
        assert_eq!(logged, 1, "{what}");
    }
    // Another station's RR73 closes nothing: the caller still gets the rogered exchange.
    let (_, after, next, logged) = run("arrlfd", Tier::Ft8, "K1ABC K2DEF RR73");
    assert_eq!(
        after.as_deref(),
        Some("W9XYZ K1ABC R 2B EMA"),
        "K2DEF's RR73"
    );
    assert_eq!(after, next, "K2DEF's RR73: and again");
    assert_eq!(logged, 0, "K2DEF's RR73: nothing logged");
}

/// The contest's rows, each as its call and the exchange it sent.
fn rows_of(e: &Engine) -> Vec<String> {
    match &e.mode {
        Mode::FieldDay { station, .. } => station
            .log
            .qsos()
            .iter()
            .map(|q| format!("{} {}", q.call, fields(&q.rx)))
            .collect(),
        _ => Vec::new(),
    }
}

/// ⭐ ONLY THE CALLER'S OWN CONFIRMATION CLOSES A CONTEST CONTACT, as WSJT-X acts only on its QSO
/// partner's message: `processMessage` (mainwindow.cpp 9107-9110) needs the sender to be the DX
/// call, and answers another station's RR73 with nothing (9315, 9336-9338). With W9XYZ's
/// exchange in hand and our roger on the air, K2DEF's RR73, RRR or 73 to us logs nothing and
/// sends K2DEF nothing: the next over is W9XYZ's again. W9XYZ's own RR73 then logs W9XYZ and
/// closes as before: with 73 in ARRL Field Day on FT8 and FT4, where W9XYZ runs WSJT-X's Field
/// Day activity, and with nothing on FT1 and in Winter Field Day; then the run calls CQ. Heard in
/// the same period as W9XYZ's, K2DEF's costs W9XYZ nothing. The ARRL VHF contests' runner and
/// the standard messages' S&P, which took only their partner's already, are the control.
#[test]
fn only_the_callers_own_confirmation_closes_a_contest_contact() {
    let strangers = ["K1ABC K2DEF RR73", "K1ABC K2DEF RRR", "K1ABC K2DEF 73"];
    let fd = "W9XYZ CLASS=1D,SECTION=WI";
    // (contest, tier, W9XYZ's answer to our CQ, our roger, its row, our answer to its RR73)
    let runs = [
        (
            "arrlfd",
            Tier::Ft8,
            "K1ABC W9XYZ 1D WI",
            "W9XYZ K1ABC R 2B EMA",
            fd,
            Some("W9XYZ K1ABC 73"),
        ),
        (
            "arrlfd",
            Tier::Ft4,
            "K1ABC W9XYZ 1D WI",
            "W9XYZ K1ABC R 2B EMA",
            fd,
            Some("W9XYZ K1ABC 73"),
        ),
        (
            "arrlfd",
            Tier::TempoFast,
            "K1ABC W9XYZ 1D WI",
            "W9XYZ K1ABC R 2B EMA",
            fd,
            None,
        ),
        (
            "wfd",
            Tier::Ft8,
            "K1ABC W9XYZ 1H WI",
            "W9XYZ K1ABC R 1H EMA",
            "W9XYZ CLASS=1H,SECTION=WI",
            None,
        ),
        (
            "arrlvhf_jan",
            Tier::Ft8,
            "K1ABC W9XYZ EN37",
            "W9XYZ K1ABC R FN42",
            "W9XYZ GRID=EN37",
            Some("W9XYZ K1ABC 73"),
        ),
    ];
    for (contest, tier, call, roger, row, answer) in runs {
        for stranger in strangers {
            let what = format!("{contest:?} {} {stranger:?}", tier_name(tier));
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(tier);
            e.set_mode("fieldday-run").expect("the run starts");
            let cq = keyed(&mut e, 0).expect("the CQ");
            e.ingest_decodes_for_test(&[heard(call)], 2);
            assert_eq!(keyed(&mut e, 2).as_deref(), Some(roger), "{what}");
            e.ingest_decodes_for_test(&[heard(stranger)], 4);
            assert_eq!(rows_of(&e), Vec::<String>::new(), "{what}: nothing logged");
            assert_eq!(
                keyed(&mut e, 4).as_deref(),
                Some(roger),
                "{what}: W9XYZ's again, nothing to K2DEF"
            );
            e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 6);
            assert_eq!(rows_of(&e), [row], "{what}: W9XYZ's own RR73 logs W9XYZ");
            assert_eq!(
                keyed(&mut e, 6).as_deref(),
                Some(answer.unwrap_or(&cq)),
                "{what}: and closes as before"
            );
            e.ingest_decodes_for_test(&[], 8);
            assert_eq!(keyed(&mut e, 8), Some(cq), "{what}: then CQ");
            assert_eq!(rows_of(&e), [row], "{what}: once");
        }
    }
    // K2DEF's RR73 in the same period as W9XYZ's, before it or after it.
    let stranger = modes::Decode {
        freq: 1720.0,
        ..heard("K1ABC K2DEF RR73")
    };
    let own = heard("K1ABC W9XYZ RR73");
    for (order, both) in [
        ("K2DEF's first", [stranger.clone(), own.clone()]),
        ("W9XYZ's first", [own, stranger]),
    ] {
        let mut e = engine_for("arrlfd", MY_GRID);
        e.set_tier(Tier::Ft8);
        e.set_mode("fieldday-run").expect("the run starts");
        keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ 1D WI")], 2);
        keyed(&mut e, 2).expect("the roger");
        e.ingest_decodes_for_test(&both, 4);
        assert_eq!(rows_of(&e), [fd], "{order}");
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("W9XYZ K1ABC 73"),
            "{order}: W9XYZ still gets its 73"
        );
    }
    // The standard messages' S&P, after its R and report.
    for stranger in strangers {
        let what = format!("\"txqp\" S&P {stranger:?}");
        let mut e = engine_for("txqp", MY_GRID);
        e.set_tier(Tier::Ft8);
        e.set_mode("fieldday-sp").expect("S&P starts");
        e.set_tx_enabled(true);
        click(&mut e, "CQ W9XYZ EN37", 2);
        assert_eq!(
            keyed(&mut e, 2).as_deref(),
            Some("W9XYZ K1ABC FN42"),
            "{what}"
        );
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ -07")], 4);
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("W9XYZ K1ABC R-12"),
            "{what}"
        );
        e.ingest_decodes_for_test(&[heard(stranger)], 6);
        assert_eq!(rows_of(&e), Vec::<String>::new(), "{what}: nothing logged");
        assert_eq!(
            keyed(&mut e, 6).as_deref(),
            Some("W9XYZ K1ABC R-12"),
            "{what}: W9XYZ's again"
        );
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 8);
        assert_eq!(rows_of(&e), ["W9XYZ "], "{what}: W9XYZ logged");
        assert_eq!(
            keyed(&mut e, 8).as_deref(),
            Some("W9XYZ K1ABC 73"),
            "{what}"
        );
    }
}

/// ⭐ THE CONTEST'S S&P COMPLETES A FIELD DAY CONTACT ONLY WITH THE STATION IT CALLED, as WSJT-X
/// acts only on its QSO partner's message: `auto_sequence` passes a message to us on to
/// `processMessage` only when its sender holds the DX call (mainwindow.cpp 7546-7549), and
/// `processMessage` acts only on its partner's (9107-9110). With W9XYZ's CQ FD double-clicked,
/// N0ABC's rogered exchange to us logs nothing and sends N0ABC nothing: our exchange goes to
/// W9XYZ again. W9XYZ's own then logs W9XYZ and is closed with RR73, as before: in ARRL Field Day
/// on FT8, FT4 and FT1, and in Winter Field Day. Heard in the same period as W9XYZ's, N0ABC's
/// costs W9XYZ nothing.
#[test]
fn the_contests_sp_completes_a_field_day_contact_only_with_the_station_it_called() {
    // The contest's S&P on `tier`, W9XYZ's CQ FD double-clicked and answered with our exchange,
    // `exch`.
    let answered = |contest: &str, tier: Tier, exch: &str| {
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-sp").expect("S&P starts");
        e.set_tx_enabled(true);
        click(&mut e, "CQ FD W9XYZ EN37", 2);
        let what = format!("{contest:?} {}", tier_name(tier));
        assert_eq!(
            keyed(&mut e, 2).as_deref(),
            Some(exch),
            "{what}: the answer"
        );
        e
    };
    // (contest, tier, our exchange to W9XYZ, the class W9XYZ and N0ABC send)
    for (contest, tier, exch, class) in [
        ("arrlfd", Tier::Ft8, "W9XYZ K1ABC 2B EMA", "1D"),
        ("arrlfd", Tier::Ft4, "W9XYZ K1ABC 2B EMA", "1D"),
        ("arrlfd", Tier::TempoFast, "W9XYZ K1ABC 2B EMA", "1D"),
        ("wfd", Tier::Ft8, "W9XYZ K1ABC 1H EMA", "1H"),
    ] {
        let what = format!("{contest:?} {}", tier_name(tier));
        let mut e = answered(contest, tier, exch);
        e.ingest_decodes_for_test(&[heard(&format!("K1ABC N0ABC R {class} WI"))], 4);
        assert_eq!(
            rows_of(&e),
            Vec::<String>::new(),
            "{what}: N0ABC's logs nothing"
        );
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some(exch),
            "{what}: W9XYZ's again, nothing to N0ABC"
        );
        e.ingest_decodes_for_test(&[heard(&format!("K1ABC W9XYZ R {class} WI"))], 6);
        let row = format!("W9XYZ CLASS={class},SECTION=WI");
        assert_eq!(rows_of(&e), [row], "{what}: W9XYZ's own logs W9XYZ");
        assert_eq!(
            keyed(&mut e, 6).as_deref(),
            Some("W9XYZ K1ABC RR73"),
            "{what}: and gets RR73"
        );
    }
    // N0ABC's rogered exchange in the same period as W9XYZ's, before it or after it.
    let stranger = modes::Decode {
        freq: 1720.0,
        ..heard("K1ABC N0ABC R 1D WI")
    };
    let own = heard("K1ABC W9XYZ R 1D WI");
    for (order, both) in [
        ("N0ABC's first", [stranger.clone(), own.clone()]),
        ("W9XYZ's first", [own, stranger]),
    ] {
        let mut e = answered("arrlfd", Tier::Ft8, "W9XYZ K1ABC 2B EMA");
        e.ingest_decodes_for_test(&both, 4);
        assert_eq!(rows_of(&e), ["W9XYZ CLASS=1D,SECTION=WI"], "{order}");
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("W9XYZ K1ABC RR73"),
            "{order}: W9XYZ gets its RR73"
        );
    }
}

/// ⭐ THE CONTEST LOGS A HASHED CALL WITHOUT ITS BRACKETS in Field Day's messages, as the ARRL VHF
/// and standard messages do and as WSJT-X logs the call in its DX call box, which a decoded
/// message fills with the brackets dropped (decodedtext.cpp 66). W9XYZ/M's call is nonstandard, so
/// its Field Day exchange names it hashed: `K1ABC <W9XYZ/M> R 1D WI` to S&P, `K1ABC <W9XYZ/M> 1D
/// WI` to the run. In ARRL Field Day on FT8 and FT4 and in Winter Field Day, S&P and Running, the
/// row and the exported ADIF's CALL are `W9XYZ/M`, and every over is what it was, but RR73 and 73,
/// which go as WSJT-X writes them (`rr73_and_73_to_a_nonstandard_call_are_wsjtxs_bit_for_bit`).
#[test]
fn the_contest_logs_a_hashed_call_without_its_brackets() {
    // The rows, and every CALL the exported ADIF writes.
    let logged = |e: &Engine| {
        let adif = e.export_log("adif").expect("the contest exports");
        let calls = adif
            .split("<CALL:")
            .skip(1)
            .filter_map(|r| {
                let (len, rest) = r.split_once('>')?;
                rest.get(..len.parse().ok()?).map(str::to_string)
            })
            .collect::<Vec<_>>();
        (rows_of(e), calls)
    };
    // (contest, tier, our exchange's class, W9XYZ/M's)
    for (contest, tier, mine, theirs) in [
        ("arrlfd", Tier::Ft8, "2B", "1D"),
        ("arrlfd", Tier::Ft4, "2B", "1D"),
        ("wfd", Tier::Ft8, "1H", "1H"),
    ] {
        let row = || {
            (
                vec![format!("W9XYZ/M CLASS={theirs},SECTION=WI")],
                vec!["W9XYZ/M".to_string()],
            )
        };
        let what = format!("{contest:?} {} S&P", tier_name(tier));
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-sp").expect("S&P starts");
        e.set_tx_enabled(true);
        click(&mut e, "CQ W9XYZ/M", 2);
        assert_eq!(
            keyed(&mut e, 2),
            Some(format!("W9XYZ/M K1ABC {mine} EMA")),
            "{what}: the answer"
        );
        e.ingest_decodes_for_test(&[heard(&format!("K1ABC <W9XYZ/M> R {theirs} WI"))], 4);
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("W9XYZ/M <K1ABC> RR73"),
            "{what}: RR73"
        );
        assert_eq!(logged(&e), row(), "{what}: logged");

        let what = format!("{contest:?} {} Running", tier_name(tier));
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("the run starts");
        let cq = keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard(&format!("K1ABC <W9XYZ/M> {theirs} WI"))], 2);
        assert_eq!(
            keyed(&mut e, 2),
            Some(format!("<W9XYZ/M> K1ABC R {mine} EMA")),
            "{what}: R and ours"
        );
        e.ingest_decodes_for_test(&[heard("K1ABC <W9XYZ/M> RR73")], 4);
        // ARRL Field Day's runner answers RR73 with 73; Winter Field Day's goes back to CQ.
        let close = match contest {
            "arrlfd" => "W9XYZ/M <K1ABC> 73".to_string(),
            _ => cq,
        };
        assert_eq!(keyed(&mut e, 4), Some(close), "{what}: the close");
        assert_eq!(logged(&e), row(), "{what}: logged");
    }
}

/// ⭐ RR73 AND 73 TO A NONSTANDARD CALL ARE WSJT-X 3.0.2'S, BIT FOR BIT. In ARRL Field Day on FT8
/// and FT4, S&P working a runner signing W9XYZ/M and the run working such a caller: each over
/// this station keys packs to the 77 bits of the over WSJT-X writes in its place, and on FT4 to
/// the same tones (`genStdMsgs`, mainwindow.cpp: Tx2 and Tx3 `t0a` and the exchange, 9596,
/// 9629-9631, 9655-9657; Tx4 and Tx5 `hisCall <my_callsign>`, 9701 and 9709). RR73 and 73 are
/// type 4 messages. The FT8 encoder is the one the radio loop keys with: WSJT-X's packer,
/// ported, held to the vendored one by `crates/modes/tests/wsjt77_golden.rs`.
#[test]
fn rr73_and_73_to_a_nonstandard_call_are_wsjtxs_bit_for_bit() {
    let bits = |t: &str| bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(t));
    for tier in [Tier::Ft8, Tier::Ft4] {
        let mut e = engine_for("arrlfd", MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-sp").expect("S&P starts");
        e.set_tx_enabled(true);
        click(&mut e, "CQ W9XYZ/M", 2);
        let tx2 = keyed(&mut e, 2).expect("S&P's answer");
        e.ingest_decodes_for_test(&[heard("K1ABC <W9XYZ/M> R 1D WI")], 4);
        let tx4 = keyed(&mut e, 4).expect("S&P's RR73");

        let mut e = engine_for("arrlfd", MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("the run starts");
        keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard("K1ABC <W9XYZ/M> 1D WI")], 2);
        let tx3 = keyed(&mut e, 2).expect("the run's R and exchange");
        e.ingest_decodes_for_test(&[heard("K1ABC <W9XYZ/M> RR73")], 4);
        let tx5 = keyed(&mut e, 4).expect("the run's 73");

        // (the over, this station's, WSJT-X's, its 77-bit type)
        for (over, ours, wsjtx, kind) in [
            ("Tx2", tx2, "<W9XYZ/M> K1ABC 2B EMA", "i3=0 n3=3"),
            ("Tx3", tx3, "<W9XYZ/M> K1ABC R 2B EMA", "i3=0 n3=3"),
            ("Tx4", tx4, "W9XYZ/M <K1ABC> RR73", "i3=4"),
            ("Tx5", tx5, "W9XYZ/M <K1ABC> 73", "i3=4"),
        ] {
            let what = format!("{} {over} {ours:?}", tier_name(tier));
            assert_eq!(air_form(wsjtx).0, kind, "{what}: WSJT-X's packs");
            assert_eq!(bits(&ours), bits(wsjtx), "{what}: WSJT-X's 77 bits");
            assert_eq!(
                tones_of(modes::ModeKind::Ft4, &ours),
                tones_of(modes::ModeKind::Ft4, wsjtx),
                "{what}: WSJT-X's FT4 tones"
            );
        }
    }
}

/// ⭐ RR73 AND 73 TO A NONSTANDARD CALL ARE WSJT-X 3.0.2'S, BIT FOR BIT, IN THE ARRL VHF AND
/// STANDARD MESSAGES TOO: WSJT-X's Tx4 and Tx5 are the same lines in every activity
/// (`genStdMsgs`, mainwindow.cpp 9701 and 9709). Running an ARRL VHF contest and one with the
/// standard messages on FT8 and FT4, a caller signing W9XYZ/M sends its grid (NA VHF's Tx2) or
/// its report (the standard Tx2) with its call hashed (`t0b`, 9597, 9625-9626, 9655), then its
/// RR73 with ours hashed (9702), each heard as this station's decoder prints it. The run's roger
/// is WSJT-X's (`t0a`, 9596, 9629-9631, 9657) and its 73 is `W9XYZ/M <K1ABC> 73`, a type 4
/// message; a caller that sends its grid and rogers our report gets `W9XYZ/M <K1ABC> RR73`.
/// Each over keyed packs to WSJT-X's 77 bits and, on FT4, its tones. Before, the 73 was
/// `W9XYZ/M K1ABC 73`, which goes on the air as `W9XYZ K1ABC 73`: another station's call.
#[test]
fn rr73_and_73_to_a_nonstandard_call_are_wsjtxs_bit_for_bit_in_every_set() {
    // What goes on the air: the 77-bit type and the text a receiving station decodes, the 77
    // bits, and the FT4 tones.
    let air = |t: &str| {
        let bits: String = bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(t))
            .unwrap_or_default()
            .iter()
            .map(|b| char::from(b'0' + *b as u8))
            .collect();
        (air_form(t), bits, tones_of(modes::ModeKind::Ft4, t))
    };
    for tier in [Tier::Ft8, Tier::Ft4] {
        let mut overs = Vec::new();
        for (contest, call, roger) in [
            (
                "arrlvhf_jan",
                "K1ABC <W9XYZ/M> EN37",
                "<W9XYZ/M> K1ABC R FN42",
            ),
            ("txqp", "K1ABC <W9XYZ/M> -07", "<W9XYZ/M> K1ABC R-12"),
        ] {
            let what = format!("{contest:?} {}", tier_name(tier));
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(tier);
            e.set_mode("fieldday-run").expect("the run starts");
            keyed(&mut e, 0).expect("the CQ");
            e.ingest_decodes_for_test(&[heard(&air_form(call).1)], 2);
            let tx3 = keyed(&mut e, 2).expect("the run's roger");
            e.ingest_decodes_for_test(&[heard(&air_form("<K1ABC> W9XYZ/M RR73").1)], 4);
            let tx5 = keyed(&mut e, 4).expect("the run's 73");
            overs.push((format!("{what} Tx3"), tx3, roger, "i3=1"));
            overs.push((format!("{what} Tx5"), tx5, "W9XYZ/M <K1ABC> 73", "i3=4"));
        }
        let what = format!("\"txqp\" {}", tier_name(tier));
        let mut e = engine_for("txqp", MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("the run starts");
        keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard(&air_form("K1ABC <W9XYZ/M> EN37").1)], 2);
        let tx2 = keyed(&mut e, 2).expect("the run's report");
        e.ingest_decodes_for_test(&[heard(&air_form("K1ABC <W9XYZ/M> R-07").1)], 4);
        let tx4 = keyed(&mut e, 4).expect("the run's RR73");
        overs.push((format!("{what} Tx2"), tx2, "<W9XYZ/M> K1ABC -12", "i3=1"));
        overs.push((format!("{what} Tx4"), tx4, "W9XYZ/M <K1ABC> RR73", "i3=4"));

        for (what, ours, wsjtx, kind) in overs {
            assert_eq!(air_form(wsjtx).0, kind, "{what}: WSJT-X's packs");
            assert_eq!(
                air(&ours),
                air(wsjtx),
                "{what} {ours:?}: WSJT-X's on the air"
            );
        }
    }
}

/// ⭐ THE CONTEST'S RUN CLOSES WITH A CALLER WHOSE CALL IS NONSTANDARD, as WSJT-X 3.0.2 closes
/// with one. Running WSJT-X, a caller signing W9XYZ/M sends its exchange with its call hashed
/// (`t0b`, mainwindow.cpp 9597 and 9625-9626) and its RR73 with ours hashed,
/// `<K1ABC> W9XYZ/M RR73` (9702), each heard here as this station's decoder prints it from the
/// air. In ARRL Field Day on FT8 and FT4 the run logs W9XYZ/M, bare, answers with WSJT-X's 73,
/// `W9XYZ/M <K1ABC> 73` (9709), and calls CQ; in Winter Field Day it logs and calls CQ. Before,
/// the RR73 was not taken: the caller got the R and exchange again, and nothing was logged.
#[test]
fn the_contests_run_closes_with_a_caller_whose_call_is_nonstandard() {
    // (contest, tier, our class, W9XYZ/M's, the run answers RR73 with 73)
    for (contest, tier, mine, theirs, answers) in [
        ("arrlfd", Tier::Ft8, "2B", "1D", true),
        ("arrlfd", Tier::Ft4, "2B", "1D", true),
        ("wfd", Tier::Ft8, "1H", "1H", false),
    ] {
        let what = format!("{contest:?} {}", tier_name(tier));
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("the run starts");
        let cq = keyed(&mut e, 0).expect("the CQ");
        let exch = air_form(&format!("K1ABC <W9XYZ/M> {theirs} WI")).1;
        e.ingest_decodes_for_test(&[heard(&exch)], 2);
        assert_eq!(
            keyed(&mut e, 2),
            Some(format!("<W9XYZ/M> K1ABC R {mine} EMA")),
            "{what}: R and ours"
        );
        let rr73 = air_form("<K1ABC> W9XYZ/M RR73");
        assert_eq!(
            rr73,
            ("i3=4".to_string(), "<K1ABC> W9XYZ/M RR73".to_string()),
            "{what}: the caller's RR73, as decoded"
        );
        e.ingest_decodes_for_test(&[heard(&rr73.1)], 4);
        assert_eq!(
            rows_of(&e),
            [format!("W9XYZ/M CLASS={theirs},SECTION=WI")],
            "{what}: logged"
        );
        let close = if answers {
            "W9XYZ/M <K1ABC> 73".to_string()
        } else {
            cq.clone()
        };
        assert_eq!(keyed(&mut e, 4), Some(close), "{what}: the close");
        e.ingest_decodes_for_test(&[], 6);
        assert_eq!(keyed(&mut e, 6), Some(cq), "{what}: then CQ");
        assert_eq!(rows_of(&e).len(), 1, "{what}: once");
    }
}

/// ⭐ A STANDARD-MESSAGES RUN ANSWERS A WSJT-X CALLER'S FIRST CALL, WHICH CARRIES NO GRID, as
/// WSJT-X running with no activity answers it. A caller signing W9XYZ/M sends its first call as
/// `t0a` alone, `<K1ABC> W9XYZ/M` (mainwindow.cpp 9627), a type 4 message with no grid; WSJT-X
/// answers it with Tx2, the report (`processMessage` 9315-9325), `<W9XYZ/M> K1ABC -12` (`t0a` and
/// the report, 9629-9631, 9655). Its roger `K1ABC <W9XYZ/M> R-07` (`t0b`, 9625-9626, 9656) then
/// gets WSJT-X's RR73 (9701). In a contest with the standard messages on FT8 and FT4, each over
/// keyed packs to WSJT-X's 77 bits and FT4 tones, and the contact is logged as W9XYZ/M. Before,
/// the run kept calling CQ.
#[test]
fn a_standard_messages_run_answers_a_wsjtx_callers_first_call_with_no_grid() {
    let air = |t: &str| {
        let bits: String = bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(t))
            .unwrap_or_default()
            .iter()
            .map(|b| char::from(b'0' + *b as u8))
            .collect();
        (air_form(t), bits, tones_of(modes::ModeKind::Ft4, t))
    };
    for tier in [Tier::Ft8, Tier::Ft4] {
        let what = format!("\"txqp\" {}", tier_name(tier));
        let mut e = engine_for("txqp", MY_GRID);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("the run starts");
        keyed(&mut e, 0).expect("the CQ");
        let call = air_form("<K1ABC> W9XYZ/M");
        assert_eq!(
            call,
            ("i3=4".to_string(), "<K1ABC> W9XYZ/M".to_string()),
            "{what}: its first call, as decoded"
        );
        e.ingest_decodes_for_test(&[heard(&call.1)], 2);
        let tx2 = keyed(&mut e, 2);
        assert_eq!(
            tx2.as_deref(),
            Some("<W9XYZ/M> K1ABC -12"),
            "{what}: answered with the report"
        );
        e.ingest_decodes_for_test(&[heard(&air_form("K1ABC <W9XYZ/M> R-07").1)], 4);
        let tx4 = keyed(&mut e, 4);
        assert_eq!(
            tx4.as_deref(),
            Some("W9XYZ/M <K1ABC> RR73"),
            "{what}: its roger, RR73"
        );
        assert_eq!(
            rows_of(&e)
                .iter()
                .map(|r| r.split(' ').next())
                .collect::<Vec<_>>(),
            [Some("W9XYZ/M")],
            "{what}: logged"
        );
        for (over, ours, wsjtx, kind) in [
            ("Tx2", tx2, "<W9XYZ/M> K1ABC -12", "i3=1"),
            ("Tx4", tx4, "W9XYZ/M <K1ABC> RR73", "i3=4"),
        ] {
            let ours = ours.unwrap_or_default();
            assert_eq!(air_form(wsjtx).0, kind, "{what} {over}: WSJT-X's packs");
            assert_eq!(
                air(&ours),
                air(wsjtx),
                "{what} {over} {ours:?}: WSJT-X's on the air"
            );
        }
    }
}

/// ⭐ THE CONTEST'S RUN PASSES BY A CALLER THE DECODER COULD NOT RESOLVE, as WSJT-X's Field Day
/// activity does: `processMessage` returns at once for a message that carries `<...>`
/// (mainwindow.cpp 9097-9099). W9XYZ/M's exchange carrying a hash this station's table does not
/// hold decodes as `K1ABC <...> 1D WI`: in ARRL Field Day on FT8 and FT4 and in Winter Field
/// Day, it gets no answer and no row, plain or rogered first, and the run calls CQ. Its next,
/// the call resolved, is worked and logged as before.
#[test]
fn the_contests_run_passes_by_a_caller_the_decoder_could_not_resolve() {
    let bits = |t: &str| {
        bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(t)).expect("the text packs")
    };
    let print = |b: &[i8]| tempo_fast::unpack77(b).unwrap_or_default();
    for (contest, tier, mine, theirs) in [
        ("arrlfd", Tier::Ft8, "2B", "1D"),
        ("arrlfd", Tier::Ft4, "2B", "1D"),
        ("wfd", Tier::Ft8, "1H", "1H"),
    ] {
        for roger in ["", "R "] {
            let what = format!("{contest:?} {} {roger:?}", tier_name(tier));
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(tier);
            e.set_mode("fieldday-run").expect("the run starts");
            let cq = keyed(&mut e, 0).expect("the CQ");
            // The second call of a Field Day exchange is bits 28-55; its last bit flipped is a
            // hash the table does not hold.
            let mut unknown = bits(&format!("K1ABC <W9XYZ/M> {roger}{theirs} WI"));
            unknown[55] ^= 1;
            let heard_as = print(&unknown);
            assert_eq!(
                heard_as,
                format!("K1ABC <...> {roger}{theirs} WI"),
                "{what}: unresolved"
            );
            e.ingest_decodes_for_test(&[heard(&heard_as)], 2);
            assert_eq!(keyed(&mut e, 2), Some(cq.clone()), "{what}: no answer");
            assert_eq!(rows_of(&e), Vec::<String>::new(), "{what}: no row");
            e.ingest_decodes_for_test(&[heard(&format!("K1ABC <W9XYZ/M> {theirs} WI"))], 4);
            assert_eq!(
                keyed(&mut e, 4),
                Some(format!("<W9XYZ/M> K1ABC R {mine} EMA")),
                "{what}: its next, resolved, is answered"
            );
            e.ingest_decodes_for_test(&[heard("<K1ABC> W9XYZ/M RR73")], 6);
            assert_eq!(
                rows_of(&e),
                [format!("W9XYZ/M CLASS={theirs},SECTION=WI")],
                "{what}: and logged"
            );
        }
    }
}

/// ⛔ THE ARRL VHF AND STANDARD MESSAGES' RUN PASSES BY A CALLER THE DECODER COULD NOT RESOLVE, as
/// WSJT-X builds no message to `<...>`: its messages go to the call in its DX call box
/// (`genStdMsgs`, mainwindow.cpp 9567-9577), which takes only a callsign (`Radio::is_callsign`,
/// 9402). W9XYZ/M's grid or report carrying a hash this station's table does not hold decodes as
/// `K1ABC <...> EN37`: on FT8 and FT4 the run keys its CQ again, and the next caller in the same
/// period, its call resolved, is answered.
#[test]
fn the_vhf_and_standard_runs_pass_by_a_caller_the_decoder_could_not_resolve() {
    let bits = |t: &str| {
        bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(t)).expect("the text packs")
    };
    let print = |b: &[i8]| tempo_fast::unpack77(b).unwrap_or_default();
    for (contest, over, answer) in [
        (
            "arrlvhf_jan",
            "K1ABC <W9XYZ/M> EN37",
            "<W9XYZ/M> K1ABC R FN42",
        ),
        ("txqp", "K1ABC <W9XYZ/M> EN37", "<W9XYZ/M> K1ABC -12"),
        ("txqp", "K1ABC <W9XYZ/M> -07", "<W9XYZ/M> K1ABC R-12"),
    ] {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let what = format!("{contest:?} {} {over:?}", tier_name(tier));
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(tier);
            e.set_mode("fieldday-run").expect("the run starts");
            let cq = keyed(&mut e, 0).expect("the CQ");
            // The second call of a standard message is bits 29-56; its last bit flipped is a hash
            // the table does not hold.
            let mut unknown = bits(over);
            unknown[56] ^= 1;
            let heard_as = print(&unknown);
            assert_eq!(
                heard_as,
                over.replace("<W9XYZ/M>", "<...>"),
                "{what}: unresolved"
            );
            e.ingest_decodes_for_test(&[heard(&heard_as)], 2);
            assert_eq!(keyed(&mut e, 2), Some(cq.clone()), "{what}: no answer");
            e.ingest_decodes_for_test(&[heard(&heard_as), heard(over)], 4);
            assert_eq!(
                keyed(&mut e, 4).as_deref(),
                Some(answer),
                "{what}: the next caller, resolved, is answered"
            );
        }
    }
}

/// The operator's double-click on the CQ `cq`, heard at `slot` (the cockpit's `call_station`):
/// the contest's S&P answers no CQ by itself, so this is how each of its contacts starts.
fn click(e: &mut Engine, cq: &str, slot: u64) {
    e.ingest_decodes_for_test(&[heard(cq)], slot);
    let Msg::Cq { de, grid, .. } = Msg::parse(cq) else {
        panic!("a CQ: {cq}");
    };
    let grid = (!grid.is_empty()).then_some(grid.as_str());
    e.work_station(&de, grid, Some(cq), Some(DX_SNR), None)
        .expect("the double-click is taken");
}

/// Whether the station is in the contest's S&P, listening, with nothing to send.
fn listening(e: &Engine) -> bool {
    matches!(&e.mode, Mode::FieldDay { station, running: false }
        if station.state == tempo_core::fieldday::FdState::Listening && station.pending.is_none())
}

/// A station with the contest switch on and Field Day's class not filled in, in FT8 QSO mode:
/// the contest never started. The contest is a fresh install's (blank, ARRL Field Day), the
/// configuration most stations run.
fn contest_that_cannot_start() -> Engine {
    let mut e = engine_for("", MY_GRID);
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
    let mut s = e.settings().clone();
    s.fd_class = String::new();
    s.fd_active = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-monitor").expect("FT8 QSO mode");
    e
}

/// The double-click answers on the period opposite the CQ's, as WSJT-X's does (`m_txFirst`
/// from the clicked message's time, mainwindow.cpp 8963-8964), and moves RX — and TX, unless
/// Hold is on — onto the station; it enables TX and keys at once.
#[test]
fn the_contest_double_click_takes_the_period_and_the_frequency_of_the_cq() {
    for hold in [false, true] {
        let mut e = engine_for("arrlfd", MY_GRID);
        let mut s = e.settings().clone();
        s.tx_even = false;
        e.apply_settings(s);
        e.set_tier(Tier::Ft8);
        e.set_hold_tx_freq(hold);
        let before_tx = e.tx_offset_hz;
        // The CQ was heard in an odd period (decoded at the even boundary after it).
        e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
        e.work_station(
            DX,
            Some(DX_GRID),
            Some("CQ FD W9XYZ EN37"),
            Some(DX_SNR),
            Some(1234.0),
        )
        .expect("the double-click is taken");
        assert!(e.tx_enabled, "Double-click on call sets Tx enable");
        assert!(e.immediate_tx, "and the current period keys if it fits");
        assert_eq!(e.rx_offset_hz, 1234.0, "RX moves onto the station");
        if hold {
            assert_eq!(e.tx_offset_hz, before_tx, "Hold keeps TX where it was");
        } else {
            assert_eq!(e.tx_offset_hz, 1234.0, "TX follows RX without Hold");
        }
        assert_eq!(keyed(&mut e, 3), None, "never in the CQ's own period");
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("W9XYZ K1ABC 2B EMA"),
            "the period opposite the CQ's"
        );
    }
}

/// ⛔ A double-click on a CQ that carries no grid, in a contest that answers with the grid, is
/// refused and changes nothing: the contest's S&P does not answer such a CQ either.
#[test]
fn a_double_click_on_a_cq_with_no_grid_changes_nothing() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ")], 2);
    let r = e.work_station(DX, None, Some("CQ TEST W9XYZ"), Some(DX_SNR), None);
    assert!(r.is_err(), "a CQ with no grid: {r:?}");
    assert!(listening(&e), "the station is as it was");
    assert!(!e.tx_enabled, "TX stays off");
}

/// ⛔ With no decode to take the period from, the double-click is refused, as outside a contest.
#[test]
fn a_double_click_with_no_decode_changes_nothing() {
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    let r = e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None);
    assert!(r.is_err(), "no decode: {r:?}");
    assert!(listening(&e), "the station is as it was");
    assert!(!e.tx_enabled, "TX stays off");
}

/// ⛔ With the contest switch on and its exchange not filled in, the double-click is refused
/// with the reason the Contest screen gives, and changes nothing.
#[test]
fn a_double_click_in_a_contest_that_cannot_start_changes_nothing() {
    let mut e = contest_that_cannot_start();
    e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
    let r = e.work_station(DX, None, Some("CQ FD W9XYZ EN37"), Some(DX_SNR), None);
    assert!(r.is_err(), "no class: {r:?}");
    assert!(
        matches!(&e.mode, Mode::Qso { station, running: false } if station.dxcall.is_none()),
        "still in FT8 QSO mode, working nobody"
    );
    assert!(!e.tx_enabled, "TX stays off");
}

/// What the station has queued to send next, in whichever mode it is in.
fn queued(e: &Engine) -> Option<String> {
    match &e.mode {
        Mode::FieldDay { station, .. } => station.outgoing().map(|m| m.to_text()),
        Mode::Qso { station, .. } => station.outgoing().map(|m| m.to_text()),
        _ => None,
    }
}

/// A CQ from `<K9QQQ/M>` in `contest`'s form, as the FT8 encoder packs it, and that CQ with the
/// hash of its call changed to one this station's table does not hold: the second call of a
/// standard message is bits 29-56, and its last bit flipped is another hash.
fn hashed_cq(cq: &str) -> (Vec<i8>, Vec<i8>) {
    let sent =
        bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(&format!("{cq} <K9QQQ/M> EN37")))
            .expect("the CQ packs");
    let mut unknown = sent.clone();
    unknown[56] ^= 1;
    (sent, unknown)
}

/// The contests with a CQ that carries a hashed call, one for each set of messages, the CQ's
/// form there, and the answer to it once the call is resolved.
const HASHED_CQS: [(&str, &str, &str); 3] = [
    ("arrlfd", "CQ FD", "<K9QQQ/M> K1ABC 2B EMA"),
    ("arrlvhf_jan", "CQ TEST", "<K9QQQ/M> K1ABC FN42"),
    ("txqp", "CQ TEST", "<K9QQQ/M> K1ABC FN42"),
];

/// ⛔ A DOUBLE-CLICK ON A CQ FROM A CALL THE DECODER COULD NOT RESOLVE ANSWERS NOTHING, as
/// WSJT-X's does. WSJT-X takes a CQ's call from the word after it (`deCallAndGrid`,
/// decodedtext.cpp 212-224), which `...` cannot be, and puts only a callsign into its DX call box
/// (`Radio::is_callsign`, mainwindow.cpp 9402), so it builds no message to `<...>`. A CQ whose call
/// this station's hash table does not hold decodes as `CQ FD <...> EN37`. In ARRL Field Day, an
/// ARRL VHF contest and a contest with the standard messages, on FT8 and FT4, from QSO mode and
/// from the contest's S&P as the contest switch leaves it, a double-click on that CQ and a
/// logger's Reply to it are refused and change nothing: the mode, what is queued, TX (off), and
/// the RX and TX offsets are as they were, and nothing keys. Its next CQ, heard with the call
/// resolved, is worked as before.
#[test]
fn a_double_click_on_a_cq_from_an_unresolved_call_answers_nothing() {
    let print = |b: &[i8]| tempo_fast::unpack77(b).unwrap_or_default();
    // Where the station is, what it has queued, TX, and the RX and TX offsets.
    let state = |e: &Engine| {
        (
            mode_line(e),
            queued(e),
            e.tx_enabled,
            (e.rx_offset_hz, e.tx_offset_hz),
        )
    };
    for (contest, cq, answer) in HASHED_CQS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for from_qso in [true, false] {
                let from = if from_qso {
                    "QSO mode"
                } else {
                    "the contest's S&P"
                };
                let what = format!("{contest:?} {} from {from}", tier_name(tier));
                let mut e = engine_for(contest, MY_GRID);
                e.set_tier(tier);
                if from_qso {
                    e.set_mode("qso-monitor").expect("QSO mode");
                }
                let (sent, unknown) = hashed_cq(cq);
                let unresolved = print(&unknown);
                assert_eq!(unresolved, format!("{cq} <...> EN37"), "{what}: unresolved");
                e.ingest_decodes_for_test(&[heard(&unresolved)], 2);
                let before = state(&e);
                let click =
                    e.work_station("<...>", None, Some(&unresolved), Some(DX_SNR), Some(1234.0));
                let reply = e.answer_udp_reply("<...>", &unresolved, DX_SNR, Some(1234.0));
                assert_eq!(
                    (click.is_ok(), reply.is_ok(), state(&e), keyed(&mut e, 4)),
                    (false, false, before.clone(), None),
                    "{what}: no answer"
                );
                if from_qso {
                    assert_eq!(before.1, None, "{what}: nothing queued");
                }
                let resolved = print(&sent);
                assert_eq!(resolved, format!("{cq} <K9QQQ/M> EN37"), "{what}: resolved");
                e.ingest_decodes_for_test(&[heard(&resolved)], 6);
                e.work_station(
                    "<K9QQQ/M>",
                    None,
                    Some(&resolved),
                    Some(DX_SNR),
                    Some(1234.0),
                )
                .expect("its next CQ is taken");
                assert_eq!(
                    keyed(&mut e, 8).as_deref(),
                    Some(answer),
                    "{what}: its next CQ, resolved, is answered"
                );
            }
        }
    }
}

/// The control: a double-click on a CQ whose hashed call the decoder resolved works the station
/// as it always has, in each set of messages, on FT8 and FT4, from QSO mode and from the
/// contest's S&P.
#[test]
fn a_double_click_on_a_cq_from_a_resolved_hashed_call_works_as_before() {
    for (contest, cq, answer) in HASHED_CQS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for from_qso in [true, false] {
                let what = format!("{contest:?} {} from_qso={from_qso}", tier_name(tier));
                let mut e = engine_for(contest, MY_GRID);
                e.set_tier(tier);
                if from_qso {
                    e.set_mode("qso-monitor").expect("QSO mode");
                }
                let resolved = tempo_fast::unpack77(&hashed_cq(cq).0).unwrap_or_default();
                assert_eq!(resolved, format!("{cq} <K9QQQ/M> EN37"), "{what}: resolved");
                e.ingest_decodes_for_test(&[heard(&resolved)], 2);
                e.work_station(
                    "<K9QQQ/M>",
                    None,
                    Some(&resolved),
                    Some(DX_SNR),
                    Some(1234.0),
                )
                .expect("the CQ is taken");
                assert!(e.tx_enabled, "{what}: TX on");
                assert_eq!(
                    keyed(&mut e, 4).as_deref(),
                    Some(answer),
                    "{what}: answered"
                );
            }
        }
    }
}

/// ⛔ THE CONTEST'S S&P PASSES BY A CQ FROM A CALL THE DECODER COULD NOT RESOLVE, with TX on, in
/// every set of messages, as WSJT-X builds no message to `<...>`: it takes a CQ's call from the
/// word after it (`deCallAndGrid`, decodedtext.cpp 212-224), which `...` cannot be, and puts only
/// a callsign into its DX call box (`Radio::is_callsign`, mainwindow.cpp 9402). In ARRL Field Day
/// (which passed it by already), an ARRL VHF contest and a contest with the standard messages, on
/// FT8 and FT4, `CQ … <...> EN37` queues nothing and keys nothing; the station's next CQ, its
/// call resolved, is answered as before once double-clicked.
#[test]
fn the_contests_sp_passes_by_a_cq_from_an_unresolved_call() {
    let print = |b: &[i8]| tempo_fast::unpack77(b).unwrap_or_default();
    for (contest, cq, answer) in HASHED_CQS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let what = format!("{contest:?} {}", tier_name(tier));
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(tier);
            e.set_mode("fieldday-sp").expect("S&P starts");
            e.set_tx_enabled(true);
            let (sent, unknown) = hashed_cq(cq);
            let unresolved = print(&unknown);
            assert_eq!(unresolved, format!("{cq} <...> EN37"), "{what}: unresolved");
            e.ingest_decodes_for_test(&[heard(&unresolved)], 2);
            assert_eq!(
                (queued(&e), keyed(&mut e, 2)),
                (None, None),
                "{what}: passed by"
            );
            assert!(listening(&e), "{what}: still listening");
            click(&mut e, &print(&sent), 4);
            assert_eq!(
                keyed(&mut e, 4).as_deref(),
                Some(answer),
                "{what}: its next CQ, resolved, is answered once double-clicked"
            );
        }
    }
}

/// The cockpit's other clicks, and every click where the contest's own FT8/FT4 messages do not
/// apply, work the station as they always have: a double-click on a station calling you, a
/// roster click, a double-click with the contest switch off, on FT2, and on your own CQ.
#[test]
fn every_other_click_works_the_station_as_before() {
    let qso_with = |e: &Engine| match &e.mode {
        Mode::Qso { station, .. } => station.dxcall.clone(),
        _ => None,
    };
    // A double-click on a station calling you, in a contest.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("K1ABC W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert_eq!(
        qso_with(&e).as_deref(),
        Some(DX),
        "a call to you: FT8 QSO mode"
    );
    // A roster click (no message), in a contest.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, Some(DX_GRID), None, None, None)
        .expect("worked");
    assert_eq!(
        qso_with(&e).as_deref(),
        Some(DX),
        "a roster click: FT8 QSO mode"
    );
    // A double-click on a CQ on FT2, in a contest.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft2);
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert_eq!(qso_with(&e).as_deref(), Some(DX), "FT2: FT2 QSO mode");
    // A double-click on a CQ with the contest switch off.
    let mut e = Engine::new(ME, MY_GRID, 0);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-monitor").expect("FT8 QSO mode");
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert_eq!(
        qso_with(&e).as_deref(),
        Some(DX),
        "no contest: FT8 QSO mode"
    );
    // A double-click on your own CQ (its echo in the band activity) starts nothing.
    let mut e = engine_for("arrlfd", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.work_station(ME, None, Some("CQ FD K1ABC FN42"), Some(DX_SNR), None)
        .expect("ignored");
    assert!(listening(&e), "your own CQ: nothing");
    assert!(!e.tx_enabled, "and TX stays off");
}

/// The contests whose FT8/FT4 messages these tests can run: every one on the list but CQ WW and
/// CQ WPX, which need a country file this binary does not install.
fn resolver_free() -> impl Iterator<Item = &'static str> {
    CONTESTS
        .into_iter()
        .filter(|c| !c.starts_with("cqww") && !c.starts_with("cqwpx"))
}

/// The contests whose rules ban FT8 and FT4.
fn bans_ft8(contest: &str) -> bool {
    matches!(contest, "wfd" | "ilqp" | "nyqp")
}

/// ⭐ **The Operate cockpit's own gestures while a contest runs, every contest, FT8 and FT4, by
/// value.** A double-click on the partner's CQ hands that station to the contest's S&P, which
/// sends exactly what a double-click from S&P sends and logs the same row; the strip's Call CQ
/// is the contest's Running and sends what Running sends. In the three contests that ban FT8 and
/// FT4 both leave the contest as they always have, and nothing reaches the contest log, but for
/// a double-click from Winter Field Day's S&P: Field Day's messages carry its exchange, and that
/// S&P starts a contact no other way.
#[test]
fn the_cockpits_gestures_work_the_running_contest() {
    for contest in resolver_free() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for role in [Role::ClickFromSp, Role::CallCq] {
                let what = format!("{contest:?} {} {role:?}", tier_name(tier));
                let s = scene(contest, tier, role, act_of(contest), MY_GRID);
                assert_eq!(s.entered, Ok(()), "{what}");
                if bans_ft8(contest) && (role == Role::CallCq || contest != "wfd") {
                    assert!(
                        s.end.starts_with("Qso"),
                        "{what}: left as before, {}",
                        s.end
                    );
                    assert!(s.logged.is_empty(), "{what}: {:?}", s.logged);
                    continue;
                }
                let like = if role == Role::CallCq {
                    Role::Running
                } else {
                    Role::DoubleClick
                };
                let (want, row) = expected(contest, like, tier_name(tier));
                assert!(
                    s.sent.len() >= want.len() && s.sent[..want.len()] == want[..],
                    "{what}: sent {:?}",
                    s.sent
                );
                if like == Role::DoubleClick {
                    assert_eq!(s.sent.len(), want.len(), "{what}: nothing more");
                }
                assert_eq!(s.logged, row.into_iter().collect::<Vec<_>>(), "{what}");
                let running = role == Role::CallCq;
                assert!(
                    s.end.starts_with(&format!("FieldDay running={running}")),
                    "{what}: {}",
                    s.end
                );
            }
        }
    }
    // The same double-click from the contest's Running and from FT8 QSO mode, and Call CQ from
    // FT8 QSO mode.
    for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
        let cq = expected(contest, Role::Running, "FT8").0[0].clone();
        for role in [Role::ClickFromRun, Role::ClickFromQso, Role::CallCqFromQso] {
            let what = format!("{contest:?} FT8 {role:?}");
            let s = scene(contest, Tier::Ft8, role, act_of(contest), MY_GRID);
            let like = if role == Role::CallCqFromQso {
                Role::Running
            } else {
                Role::SearchAndPounce
            };
            let (mut want, row) = expected(contest, like, "FT8");
            if role == Role::ClickFromRun {
                want.insert(0, cq.clone());
            }
            assert!(
                s.sent.len() >= want.len() && s.sent[..want.len()] == want[..],
                "{what}: sent {:?}",
                s.sent
            );
            assert_eq!(s.logged, row.into_iter().collect::<Vec<_>>(), "{what}");
        }
    }
}

/// ⛔ Call CQ while the contest cannot start (its exchange not filled in) is refused as the
/// Contest screen's Running is, and changes nothing.
#[test]
fn call_cq_in_a_contest_that_cannot_start_changes_nothing() {
    let mut e = contest_that_cannot_start();
    let r = e.start_running();
    assert!(r.is_err(), "no class: {r:?}");
    assert!(
        matches!(&e.mode, Mode::Qso { running: false, .. }),
        "still in FT8 QSO mode"
    );
    assert!(!e.tx_enabled, "TX stays off");
    assert_eq!(keyed(&mut e, 0), None, "nothing keys");
}

/// The Remote page's Tx6 row (`start_cq`) still calls the CQ it shows, plain or directed, as an
/// FT8 QSO-mode run, in a contest too, as the Remote page's Call CQ does. On this computer the row
/// is the contest's CQ while a contest runs ([`the_tx6_row_shows_the_cq_the_contests_running_sends`]).
#[test]
fn the_remote_pages_tx6_still_sends_the_cq_it_shows() {
    for (dir, want) in [(None, "CQ K1ABC FN42"), (Some("DX"), "CQ DX K1ABC FN42")] {
        let mut e = engine_for("arrlfd", MY_GRID);
        e.set_tier(Tier::Ft8);
        e.start_cq(dir).expect("the run starts");
        assert!(
            matches!(&e.mode, Mode::Qso { running: true, .. }),
            "Tx6 {dir:?}: FT8 QSO mode"
        );
        assert_eq!(keyed(&mut e, 0).as_deref(), Some(want), "Tx6 {dir:?}");
    }
}

/// ⭐ WHILE A CONTEST RUNS, THE TX6 ROW SHOWS THE CQ ITS RUNNING SENDS, as WSJT-X's Tx6 is its
/// activity's CQ (`genCQMsg`, 9502-9524), and on this computer Tx 6 calls it through the contest's
/// Running, as the strip's Call CQ does. By value, in every contest on FT8 and FT4, from the
/// contest's S&P (where the switch leaves the station) and from FT8 QSO mode with the switch on:
/// `CQ FD` in Field Day and the blank default, `CQ TEST` in the ARRL VHF contests, the standard CQ
/// where WSJT-X has none; and what the row shows is what a receiving station decodes from the
/// Running's first over (a contest that cannot start here refuses, and nothing keys). None where the
/// contest's messages do not apply (a contest that bans the mode, FT1, DX1, FT2, Contest mode off),
/// where WSJT-X has no activity (its Tx6 is the operator's own there:
/// [`where_wsjtx_has_no_activity_the_running_calls_the_cq_typed_in_tx6`]), and for a nonstandard
/// call, whose Tx6 WSJT-X never gives an activity's CQ (`genCQMsg`: `stdCall(my_callsign)`): the
/// row keeps its own CQ there.
#[test]
fn the_tx6_row_shows_the_cq_the_contests_running_sends() {
    let want = |contest: &str| match contest {
        "" | "arrlfd" => Some("CQ FD K1ABC FN42"),
        "arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep" => Some("CQ TEST K1ABC FN42"),
        _ => None,
    };
    // What a receiver decodes from the over Tx 6 starts, and that it matches what the row showed.
    let pressed = |e: &mut Engine, shown: &str, what: &str| match e.start_running() {
        Ok(()) => {
            let text = keyed(e, 0).unwrap_or_else(|| panic!("{what}: the Running keys"));
            assert_eq!(
                air_form(&text).1,
                shown,
                "{what}: the air carries what the row shows"
            );
        }
        // CQ WW and CQ WPX cannot start without a country file, as the Contest screen's Running.
        Err(why) => assert_eq!(keyed(e, 0), None, "{what}: refused ({why}), nothing keys"),
    };
    let mut started = 0;
    for contest in CONTESTS {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for from_qso in [false, true] {
                let what = format!("{contest:?} {} from_qso={from_qso}", tier_name(tier));
                let mut e = engine_for(contest, MY_GRID);
                e.set_tier(tier);
                if from_qso {
                    e.set_mode("qso-monitor").expect("FT8 QSO mode");
                }
                let shown = e.contest_running_cq();
                assert_eq!(shown.as_deref(), want(contest), "{what}");
                assert_eq!(
                    e.snapshot().contest_cq,
                    shown,
                    "{what}: the snapshot carries it"
                );
                if let Some(shown) = shown {
                    pressed(&mut e, &shown, &what);
                    started += 1;
                }
            }
        }
    }
    assert_eq!(
        started, 20,
        "Tx 6 pressed in the contests WSJT-X has an activity for"
    );
    // Field Day with a six-character locator: the Running hands the encoder the locator as typed,
    // and the air carries four characters — as the row shows, and as WSJT-X's Tx6 does.
    let mut e = engine_for("arrlfd", "FN42ab");
    e.set_tier(Tier::Ft8);
    assert_eq!(e.contest_running_cq().as_deref(), Some("CQ FD K1ABC FN42"));
    e.start_running().expect("the run starts");
    assert_eq!(
        keyed(&mut e, 0).as_deref(),
        Some("CQ FD K1ABC FN42ab"),
        "the encoder's text"
    );
    // A locator changed in Settings during the contest: what the row shows is still what goes out.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    let mut s = e.settings().clone();
    s.mygrid = "EN52".into();
    e.apply_settings(s);
    let shown = e.contest_running_cq().expect("a VHF contest CQ");
    pressed(&mut e, &shown, "a locator changed mid-contest");
    // No contest CQ where the contest's messages do not apply.
    for tier in [Tier::TempoFast, Tier::TempoDeep, Tier::Ft2] {
        for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(tier);
            assert_eq!(
                e.contest_running_cq(),
                None,
                "{contest:?} {}",
                tier_name(tier)
            );
            assert_eq!(e.snapshot().contest_cq, None);
        }
    }
    let mut e = engine_for("arrlfd", MY_GRID);
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    assert_eq!(e.contest_running_cq(), None, "Contest mode off");
    for contest in ["arrlfd", "arrlvhf_jan"] {
        let mut e = engine_for(contest, MY_GRID);
        let mut s = e.settings().clone();
        s.mycall = "PJ4/K1ABC".into();
        e.apply_settings(s);
        e.set_tier(Tier::Ft8);
        assert_eq!(
            e.contest_running_cq(),
            None,
            "{contest}: a nonstandard call"
        );
    }
}

/// ⭐ WHERE WSJT-X HAS NO ACTIVITY FOR THE CONTEST, ITS RUNNING CALLS THE CQ TYPED IN TX6, as WSJT-X's
/// normal mode sends its Tx6 (`on_tx6_editingFinished`, mainwindow.cpp 10075-10086; `genCQMsg`): a
/// direction of one to four letters or three digits, `CQ QP K1ABC FN42`, on the Running's first CQ
/// and on every CQ after a contact, the contact in the contest's log. On this computer Tx 6 and Call
/// CQ hand the direction to the Running, and the Contest screen's Running keeps it. The contests
/// WSJT-X has an activity for keep their own CQ; outside a contest Call CQ is the plain run it always
/// was; a refused start changes nothing; and a nonstandard call's CQ never carries a direction, which
/// the packer has no form for.
#[test]
fn where_wsjtx_has_no_activity_the_running_calls_the_cq_typed_in_tx6() {
    let typed = "CQ QP K1ABC FN42";
    // Every CQ the Running keys over a contact with a WSJT-X partner with no activity, and how many
    // contacts the contest's log holds afterwards.
    let run = |e: &mut Engine| -> (Vec<String>, usize) {
        let mut partner = Partner::new(Act::None, false);
        let mut cqs = Vec::new();
        for slot in 0..SLOTS {
            if slot % 2 == 0 {
                if let Some(text) = keyed(e, slot) {
                    if text.starts_with("CQ ") {
                        cqs.push(text.clone());
                    }
                    partner.hear(&text);
                }
            } else {
                match partner.transmit() {
                    Some(text) => e.ingest_decodes_for_test(&[heard(&text)], slot + 1),
                    None => e.ingest_decodes_for_test(&[], slot + 1),
                }
            }
        }
        let logged = match &e.mode {
            Mode::FieldDay { station, .. } => station.log.qsos().len(),
            _ => 0,
        };
        (cqs, logged)
    };
    let mut ran = 0;
    let no_activity = CONTESTS
        .iter()
        .filter(|c| act_of(c) == Act::None && !matches!(**c, "ilqp" | "nyqp"));
    for contest in no_activity {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let what = format!("{contest:?} {}", tier_name(tier));
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(tier);
            match e.start_running_with_cq(Some("QP")) {
                Ok(()) => {
                    let (cqs, logged) = run(&mut e);
                    assert_eq!(
                        cqs.first().map(String::as_str),
                        Some(typed),
                        "{what}: the first CQ"
                    );
                    assert!(
                        cqs.len() >= 2 && cqs.iter().all(|c| c == typed),
                        "{what}: every CQ, after the contact too: {cqs:?}"
                    );
                    assert_eq!(logged, 1, "{what}: the contact is the contest's");
                    ran += 1;
                }
                // CQ WW and CQ WPX cannot start without a country file, as the Contest screen's Running.
                Err(why) => assert_eq!(keyed(&mut e, 0), None, "{what}: refused ({why})"),
            }
            assert!(e.contest_cq_typed(), "{what}: the row stays the operator's");
            assert_eq!(
                e.contest_running_cq(),
                None,
                "{what}: no contest CQ replaces it"
            );
        }
    }
    assert!(
        ran >= 12,
        "the sweep ran the contests that can start here: {ran}"
    );
    // The contests WSJT-X has an activity for keep their own CQ.
    for (contest, want) in [
        ("arrlfd", "CQ FD K1ABC FN42"),
        ("arrlvhf_jan", "CQ TEST K1ABC FN42"),
    ] {
        let mut e = engine_for(contest, MY_GRID);
        e.set_tier(Tier::Ft8);
        assert!(!e.contest_cq_typed(), "{contest}");
        e.start_running_with_cq(Some("QP")).expect("the run starts");
        assert_eq!(
            keyed(&mut e, 0).as_deref(),
            Some(want),
            "{contest}: its own CQ"
        );
        assert_eq!(
            e.cq_dir, None,
            "{contest}: set aside, so no later Running inherits it"
        );
    }
    // Outside a contest, Call CQ is the plain run it always was.
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    assert!(!e.contest_cq_typed());
    e.start_running_with_cq(Some("QP")).expect("a plain run");
    assert_eq!(
        keyed(&mut e, 0).as_deref(),
        Some("CQ K1ABC FN42"),
        "no contest: plain"
    );
    // The Contest screen's Running keeps the direction the cockpit handed over; a plain Call CQ
    // takes it away again.
    let mut e = engine_for("txqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.start_running_with_cq(Some("QP")).expect("the run starts");
    e.set_mode("fieldday-sp").expect("the screen's S&P");
    e.set_mode("fieldday-run").expect("the screen's Running");
    assert_eq!(
        keyed(&mut e, 0).as_deref(),
        Some(typed),
        "the Contest screen's Running"
    );
    e.start_running_with_cq(None).expect("a plain Call CQ");
    assert_eq!(
        keyed(&mut e, 2).as_deref(),
        Some("CQ K1ABC FN42"),
        "plain again"
    );
    // A refused start changes nothing: the direction a Tx6 press left is still there.
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.fd_event = "txqp".into();
    s.fd_active = true;
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.start_cq(Some("DX")).expect("a directed run");
    assert!(
        e.start_running_with_cq(Some("QP")).is_err(),
        "txqp not filled in"
    );
    assert_eq!(e.cq_dir.as_deref(), Some("DX"), "untouched by the refusal");
    // A nonstandard call's CQ never carries a direction.
    let mut e = engine_for("txqp", MY_GRID);
    let mut s = e.settings().clone();
    s.mycall = "PJ4/K1ABC".into();
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    assert!(!e.contest_cq_typed(), "a nonstandard call");
    e.start_running_with_cq(Some("QP")).expect("the run starts");
    let cq = keyed(&mut e, 0).expect("the Running keys");
    assert!(
        !cq.contains(" QP "),
        "no direction for a nonstandard call: {cq:?}"
    );
}

/// ⭐ A NONSTANDARD CALL CALLS WSJT-X'S PLAIN CQ ON EVERY CONTEST PATH, never another station's
/// call. WSJT-X's `stdCall` (mainwindow.cpp 9541-9552) decides, and for a call that fails it
/// `genCQMsg` sends `"%1 %2".arg(m_CQtype).arg(my_callsign)` — no grid (9488-9494) — and no
/// activity token (9506-9510). FT8 cannot carry such a call beside a token or a grid: `CQ FD
/// PJ4/K1ABC FN42` packs as `CQ FD K1ABC FN42`. By value, in Field Day, an ARRL VHF contest and a
/// QSO party on FT8 and FT4: Call CQ, the Contest screen's Running and the Tx6 row each key `CQ
/// <call>`, and every over a receiver decodes names the operator's own call. The control: a `/P`
/// call is standard, and keeps the contest's CQ.
#[test]
fn a_nonstandard_call_calls_the_plain_cq_on_every_contest_path() {
    for me in ["PJ4/K1ABC", "K1ABC/QRP"] {
        let plain = format!("CQ {me}");
        for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
            for tier in [Tier::Ft8, Tier::Ft4] {
                let what = format!("{me} {contest:?} {}", tier_name(tier));
                let station = || {
                    let mut e = engine_for(contest, MY_GRID);
                    let mut s = e.settings().clone();
                    s.mycall = me.into();
                    e.apply_settings(s);
                    e.set_tier(tier);
                    e
                };
                // Each path keys the CQ it calls, on two of its overs: what goes out, and what a
                // receiver decodes.
                let cqs = |e: &mut Engine| -> Vec<(String, String)> {
                    [0, 2]
                        .into_iter()
                        .filter_map(|slot| {
                            let text = keyed(e, slot)?;
                            e.ingest_decodes_for_test(&[], slot + 1);
                            let air = air_form(&text).1;
                            Some((text, air))
                        })
                        .collect()
                };
                let want = vec![(plain.clone(), plain.clone()); 2];
                let mut e = station();
                e.start_running().expect("Call CQ starts");
                assert!(
                    matches!(&e.mode, Mode::FieldDay { running: true, .. }),
                    "{what}"
                );
                assert_eq!(cqs(&mut e), want, "{what}: Call CQ");
                let mut e = station();
                e.set_mode("fieldday-run")
                    .expect("the Contest screen's Running");
                assert_eq!(cqs(&mut e), want, "{what}: the Contest screen's Running");
                // The Tx6 row: no contest CQ on it for this call, so it calls its own.
                let mut e = station();
                assert_eq!(e.contest_running_cq(), None, "{what}");
                e.start_cq(None).expect("Tx 6");
                assert_eq!(cqs(&mut e), want, "{what}: Tx 6");
            }
        }
    }
    // The control: `/P` is standard (`stdCall`'s `(/R | /P)?`), and keeps the contest's CQ.
    let mut e = engine_for("arrlfd", MY_GRID);
    let mut s = e.settings().clone();
    s.mycall = "K1ABC/P".into();
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.start_running().expect("Call CQ starts");
    let text = keyed(&mut e, 0).expect("the Running keys");
    assert_eq!(text, "CQ FD K1ABC/P FN42");
    assert_eq!(
        air_form(&text).1,
        "CQ FD K1ABC/P FN42",
        "a /P call is carried whole"
    );
}

/// One contest contact by `me`, the Contest screen's S&P with TX on or its Running, against a
/// partner whose overs come in the order given, each heard as this station's decoder prints it
/// from the air. Ten slots, this station's the even ones. Returns the station and what it keyed.
fn contact_as(
    me: &str,
    contest: &str,
    tier: Tier,
    running: bool,
    partner: &[String],
) -> (Engine, Vec<String>) {
    let mut e = engine_for(contest, MY_GRID);
    let mut s = e.settings().clone();
    s.mycall = me.into();
    e.apply_settings(s);
    e.set_tier(tier);
    if running {
        e.set_mode("fieldday-run")
            .expect("the Contest screen's Running");
    } else {
        e.set_mode("fieldday-sp").expect("the Contest screen's S&P");
        e.set_tx_enabled(true);
    }
    let mut theirs = partner.iter();
    let mut ours = Vec::new();
    for slot in 0..10u64 {
        if slot % 2 == 0 {
            ours.extend(keyed(&mut e, slot));
        } else {
            let over = theirs.next().map(|t| air_form(t).1);
            match over {
                // The operator double-clicks the partner's CQ: S&P answers none by itself.
                Some(cq) if !running && ours.is_empty() && cq.starts_with("CQ ") => {
                    click(&mut e, &cq, slot + 1)
                }
                over => {
                    let over: Vec<_> = over.iter().map(|t| heard(t)).collect();
                    e.ingest_decodes_for_test(&over, slot + 1);
                }
            }
        }
    }
    (e, ours)
}

/// ⭐ A NONSTANDARD CALL GOES OUT IN EVERY OVER OF A CONTEST CONTACT AS WSJT-X WRITES IT, and
/// never as another station's call. With `PJ4/K1ABC` and `K1ABC/QRP`, as S&P and as Running, in
/// Field Day, an ARRL VHF contest and a QSO party, on FT8 and FT4, against a partner sending what
/// WSJT-X 3.0.2 sends to such a call (`genStdMsgs`, mainwindow.cpp: `t0a`, the call hashed beside
/// its grid, report or exchange, 9596 and 9629-9631; its own hashed in RR73 and 73, 9701 and 9709).
/// By value: every over this station keys (its call hashed beside a grid or a report, whole
/// beside the other's hashed call in RR73, 73 and the standard messages' answer to a CQ, whole in
/// Field Day's exchange), what a receiver decodes from each, and the call the contest logs.
#[test]
fn a_nonstandard_call_goes_out_as_wsjtx_writes_it_in_every_contest_over() {
    for me in ["PJ4/K1ABC", "K1ABC/QRP"] {
        let h = format!("<{me}>");
        let cq = format!("CQ {me}");
        // (contest, running, the partner's overs, this station's)
        let scenes = [
            (
                "arrlfd",
                false,
                vec![
                    "CQ FD W9XYZ EN37".to_string(),
                    format!("{me} W9XYZ R 1D WI"),
                    format!("{me} <W9XYZ> 73"),
                ],
                vec![format!("W9XYZ {me} 2B EMA"), format!("<W9XYZ> {me} RR73")],
            ),
            (
                "arrlfd",
                true,
                vec![format!("{me} W9XYZ 1D WI"), format!("{me} <W9XYZ> RR73")],
                vec![
                    cq.clone(),
                    format!("W9XYZ {me} R 2B EMA"),
                    format!("<W9XYZ> {me} 73"),
                    cq.clone(),
                    cq.clone(),
                ],
            ),
            (
                "arrlvhf_jan",
                false,
                vec![
                    "CQ TEST W9XYZ EN37".to_string(),
                    format!("{h} W9XYZ R EN37"),
                    format!("{me} <W9XYZ> 73"),
                ],
                vec![format!("W9XYZ {h} FN42"), format!("<W9XYZ> {me} RR73")],
            ),
            (
                "arrlvhf_jan",
                true,
                vec![format!("{h} W9XYZ EN37"), format!("{me} <W9XYZ> RR73")],
                vec![
                    cq.clone(),
                    format!("W9XYZ {h} R FN42"),
                    format!("<W9XYZ> {me} 73"),
                    cq.clone(),
                    cq.clone(),
                ],
            ),
            (
                "txqp",
                false,
                vec![
                    "CQ W9XYZ EN37".to_string(),
                    format!("{h} W9XYZ {DX_RPT}"),
                    format!("{me} <W9XYZ> RR73"),
                ],
                vec![
                    format!("<W9XYZ> {me}"),
                    format!("W9XYZ {h} R{DX_SNR:+03}"),
                    format!("<W9XYZ> {me} 73"),
                ],
            ),
            (
                "txqp",
                true,
                vec![
                    format!("{h} W9XYZ EN37"),
                    format!("{h} W9XYZ R{DX_RPT}"),
                    format!("{me} <W9XYZ> 73"),
                ],
                vec![
                    cq.clone(),
                    format!("W9XYZ {h} {DX_SNR:+03}"),
                    format!("<W9XYZ> {me} RR73"),
                    cq.clone(),
                    cq.clone(),
                ],
            ),
        ];
        for (contest, running, partner, want) in scenes {
            for tier in [Tier::Ft8, Tier::Ft4] {
                let what = format!("{me} {contest:?} {} running={running}", tier_name(tier));
                let (e, ours) = contact_as(me, contest, tier, running, &partner);
                assert_eq!(ours, want, "{what}");
                for over in &ours {
                    let air = air_form(over).1;
                    let words: Vec<&str> = air.split_whitespace().collect();
                    assert!(
                        words.iter().any(|w| *w == me || *w == h),
                        "{what}: {over:?} carries {me}: {air:?}"
                    );
                    assert!(
                        !words
                            .iter()
                            .any(|w| w.contains("K1ABC") && *w != me && *w != h),
                        "{what}: {over:?} names no other station: {air:?}"
                    );
                }
                let Mode::FieldDay { station, .. } = &e.mode else {
                    panic!("{what}: the contest runs");
                };
                assert_eq!(
                    station.log.qsos().last().map(|q| q.call.as_str()),
                    Some("W9XYZ"),
                    "{what}: logged"
                );
            }
        }
    }
    // The control: a `/P` call is standard, and keeps the standard forms.
    let partner = [
        "CQ TEST W9XYZ EN37".to_string(),
        "K1ABC/P W9XYZ R EN37".to_string(),
    ];
    let (_, ours) = contact_as("K1ABC/P", "arrlvhf_jan", Tier::Ft8, false, &partner);
    assert_eq!(ours, ["W9XYZ K1ABC/P FN42", "W9XYZ K1ABC/P RR73"]);
    // Field Day's exchange goes with the call whole, and the packer hashes it. WSJT-X 3.0.2 writes
    // it bracketed (`t0b`, 9597), which the packer takes from a suffixed call alone: for a
    // prefixed one `chkcall` reads `K1ABC>` as the base call, and WSJT-X's own exchange, and the
    // reply to it, go out as free text.
    let ft8 = |t: &str| tones_of(modes::ModeKind::Ft8, t);
    assert_eq!(
        ft8("W9XYZ K1ABC/QRP 2B EMA"),
        ft8("W9XYZ <K1ABC/QRP> 2B EMA"),
        "a suffixed call: WSJT-X's exchange, bit for bit"
    );
    assert_eq!(
        air_form("W9XYZ PJ4/K1ABC 2B EMA"),
        (
            "i3=0 n3=3".to_string(),
            "W9XYZ <PJ4/K1ABC> 2B EMA".to_string()
        )
    );
    for wsjtx in ["W9XYZ <PJ4/K1ABC> 2B EMA", "<PJ4/K1ABC> W9XYZ R 1D WI"] {
        assert_eq!(air_form(wsjtx).0, "i3=0 n3=0", "{wsjtx}: free text");
    }
}

/// ⭐ A REPLY TO THIS STATION'S HASHED CALL IS DECODED AS ITS OWN, AND THE CONTACT GOES ON. A
/// nonstandard call goes out hashed beside a grid, and comes back hashed: `<ZK3/K1ABC> W9XYZ R
/// EN37`. The decoder prints a hash as a call only while its table holds that call, and the
/// packer fills the table on every message it encodes (`pack28` → `save_hash_call`), this
/// station's own over included, in the one table the decoder reads. By value, with a call no
/// other test sends: once S&P has keyed its answer, the runner's reply carrying that answer's
/// own hash decodes as `<ZK3/K1ABC> …`, is taken, and closes with RR73; the same reply with a
/// hash the table does not hold decodes as `<...> …` and is not.
#[test]
fn a_reply_to_our_hashed_call_is_decoded_as_ours_and_goes_on() {
    let me = "ZK3/K1ABC";
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    let mut s = e.settings().clone();
    s.mycall = me.into();
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-sp").expect("the Contest screen's S&P");
    e.set_tx_enabled(true);
    click(&mut e, "CQ TEST W9XYZ EN37", 2);
    let answer = keyed(&mut e, 2).expect("S&P answers the CQ double-clicked");
    assert_eq!(answer, "W9XYZ <ZK3/K1ABC> FN42");
    // The reply: the call fields of a standard message are bits 0-27 and 29-56. Its first holds
    // the hash from this station's own answer, where that hash is the second.
    let bits = |t: &str| {
        bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(t)).expect("the text packs")
    };
    let mut reply = bits("K2DEF W9XYZ R EN37");
    reply[..28].copy_from_slice(&bits(&answer)[29..57]);
    let mut unknown = reply.clone();
    unknown[27] ^= 1;
    let print = |b: &[i8]| tempo_fast::unpack77(b).unwrap_or_default();
    assert_eq!(
        print(&unknown),
        "<...> W9XYZ R EN37",
        "a hash the table does not hold"
    );
    e.ingest_decodes_for_test(&[heard(&print(&unknown))], 4);
    assert_eq!(
        keyed(&mut e, 4).as_deref(),
        Some("W9XYZ <ZK3/K1ABC> FN42"),
        "a reply to an unknown hash is not to this station"
    );
    assert_eq!(print(&reply), "<ZK3/K1ABC> W9XYZ R EN37");
    e.ingest_decodes_for_test(&[heard(&print(&reply))], 6);
    assert_eq!(
        keyed(&mut e, 6).as_deref(),
        Some("<W9XYZ> ZK3/K1ABC RR73"),
        "the reply to its hashed call is this station's, and the contact goes on"
    );
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs");
    };
    assert_eq!(
        station.log.qsos().last().map(|q| q.call.as_str()),
        Some("W9XYZ")
    );
}

/// ⛔ The transmit guards hold for the cockpit's gestures exactly as for the Contest screen's
/// buttons: a contest whose messages carry the grid keys nothing on Call CQ without one; the
/// wall-clock watchdog ends a run Call CQ started; Halt stops a contact a double-click started;
/// and with "Double-click on call sets Tx enable" off, a double-click leaves TX off.
#[test]
fn the_transmit_guards_hold_for_the_cockpits_gestures() {
    // No grid in Settings: Call CQ keys nothing.
    let mut e = engine_for("txqp", "");
    e.set_tier(Tier::Ft8);
    e.start_running().expect("the run starts");
    for slot in [0, 2, 4] {
        assert_eq!(keyed(&mut e, slot), None, "no grid, no over (slot {slot})");
    }
    // The watchdog ends a run Call CQ started.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    let mut s = e.settings().clone();
    s.tx_watchdog_min = 6;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.start_running().expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ TEST K1ABC FN42"));
    e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(6 * 60 + 1));
    assert_eq!(keyed(&mut e, 2), None, "the watchdog trips");
    assert!(!e.tx_enabled, "and disarms TX");
    // Halt stops a contact a double-click started.
    let mut e = engine_for("txqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("W9XYZ K1ABC FN42"));
    e.halt_tx();
    for slot in [4, 6, 8] {
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ -07")], slot);
        assert_eq!(
            keyed(&mut e, slot),
            None,
            "nothing after Halt (slot {slot})"
        );
    }
    // The double-click arms TX only where the operator lets it.
    let mut e = engine_for("txqp", MY_GRID);
    let mut s = e.settings().clone();
    s.double_click_sets_tx = false;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert!(!e.tx_enabled, "TX stays off");
    assert_eq!(keyed(&mut e, 2), None, "and nothing keys");
}

/// Where a running contest's own FT8/FT4 messages do not apply (the contest switch off, FT2, a
/// contest whose rules ban FT8), Call CQ is the plain CQ run it always was. And like any Call CQ
/// it clears a directed-CQ token the Tx6 row left, so no later run inherits "CQ DX".
#[test]
fn call_cq_outside_the_contests_messages_is_a_plain_run() {
    let plain = |e: &mut Engine, what: &str| {
        e.start_running().expect(what);
        assert!(
            matches!(&e.mode, Mode::Qso { running: true, .. }),
            "{what}: QSO mode"
        );
        assert_eq!(keyed(e, 0).as_deref(), Some("CQ K1ABC FN42"), "{what}");
    };
    let mut e = Engine::new(ME, MY_GRID, 0);
    let mut s = e.settings().clone();
    s.tx_even = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    plain(&mut e, "the contest switch off");
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft2);
    plain(&mut e, "FT2");
    let mut e = engine_for("ilqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    plain(&mut e, "a contest that bans FT8");
    // The token: Tx6's "CQ DX", then Call CQ in the contest, then the contest switched off.
    let mut e = engine_for("arrlvhf_jan", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.start_cq(Some("DX")).expect("Tx6");
    e.start_running().expect("Call CQ");
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
    e.set_mode("qso-run").expect("a run");
    assert_eq!(
        keyed(&mut e, 2).as_deref(),
        Some("CQ K1ABC FN42"),
        "no CQ DX left behind"
    );
}

/// The double-click follows the contest that is running: a Save that moves the pick off the
/// contest running switches to the new one, and a double-click on a CQ then works the station in
/// that contest's S&P, in its messages.
#[test]
fn a_double_click_follows_the_contest_a_save_switched_to() {
    // From a contest whose rules ban FT8, where the click would work the station outside it.
    let mut e = engine_for("ilqp", MY_GRID);
    e.set_tier(Tier::Ft8);
    let mut s = e.settings().clone();
    s.fd_event = "arrlvhf_jan".into();
    e.apply_settings(s);
    assert!(
        matches!(&e.mode, Mode::FieldDay { station, .. }
            if station.log.session.event_id == "arrlvhf_jan"),
        "the Save switched the running contest"
    );
    e.ingest_decodes_for_test(&[heard("CQ TEST W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ TEST W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert!(
        matches!(e.mode, Mode::FieldDay { running: false, .. }),
        "in the running contest's S&P"
    );
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("W9XYZ K1ABC FN42"));
}

/// A double-click carries the decode's own sender; a click naming one station on another's CQ
/// is not that station's CQ, and works the named station outside the contest, as before. And a
/// deferred "disable TX after 73" left from an earlier contact never disarms the station the
/// double-click is working.
#[test]
fn the_double_click_works_only_the_cqs_own_station_and_keeps_tx() {
    let mut e = engine_for("arrlfd", MY_GRID);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
    e.work_station("K2DEF", None, Some("CQ FD W9XYZ EN37"), Some(DX_SNR), None)
        .expect("worked");
    assert!(
        matches!(&e.mode, Mode::Qso { station, .. } if station.dxcall.as_deref() == Some("K2DEF")),
        "the named station, outside the contest"
    );
    // With "Double-click on call sets Tx enable" off, the click arms nothing itself (arming TX
    // would cancel the disable on its own): the operator armed TX, and the earlier contact's
    // disable is still to be carried out.
    let mut e = engine_for("arrlfd", MY_GRID);
    let mut s = e.settings().clone();
    s.double_click_sets_tx = false;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_tx_enabled(true);
    e.pending_tx_disable = true;
    e.ingest_decodes_for_test(&[heard("CQ FD W9XYZ EN37")], 2);
    e.work_station(DX, None, Some("CQ FD W9XYZ EN37"), Some(DX_SNR), None)
        .expect("the double-click is taken");
    assert!(
        !e.pending_tx_disable,
        "the earlier contact's disable is cancelled"
    );
    assert!(e.tx_enabled, "TX stays armed for the station being worked");
}

/// Whether the runner sends the 73 follows the tier it is on now: a run started on FT1 and
/// moved to FT8 answers the caller's RR73 with 73; a run moved from FT8 to FT1 does not.
#[test]
fn the_runners_73_follows_the_tier_it_is_on() {
    for (from, to, want_73) in [
        (Tier::TempoFast, Tier::Ft8, true),
        (Tier::Ft8, Tier::TempoFast, false),
    ] {
        let what = format!("{} to {}", tier_name(from), tier_name(to));
        let mut e = engine_for("arrlfd", MY_GRID);
        e.set_tier(from);
        e.set_mode("fieldday-run").expect("the run starts");
        e.set_tier(to);
        e.set_tx_enabled(true); // a mode switch that moves the dial turns TX off, as WSJT-X does
        let cq = keyed(&mut e, 0).expect("the CQ");
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ 1D WI")], 2);
        keyed(&mut e, 2).expect("the rogered exchange");
        e.ingest_decodes_for_test(&[heard("K1ABC W9XYZ RR73")], 4);
        let after = keyed(&mut e, 4);
        if want_73 {
            assert_eq!(after.as_deref(), Some("W9XYZ K1ABC 73"), "{what}");
        } else {
            assert_eq!(after, Some(cq), "{what}: straight back to CQ");
        }
    }
}

/// ⛔ Call CQ into a contest's Running needs a valid callsign, as the Contest screen's Running
/// does (the `set_mode` command's identity check): refused with the reason, and nothing changes.
#[test]
fn call_cq_without_a_callsign_is_refused_as_the_screens_running_is() {
    let mut e = engine_for("arrlfd", MY_GRID);
    e.set_tier(Tier::Ft8);
    let mut s = e.settings().clone();
    s.mycall = String::new();
    e.apply_settings(s);
    let r = e.start_running();
    assert!(r.is_err(), "no callsign: {r:?}");
    assert!(listening(&e), "still the contest's S&P, listening");
    assert!(!e.tx_enabled, "TX stays off");
}

/// ⭐ The contest's S&P answers each CQ the operator double-clicks on the period opposite the
/// CQ's, as WSJT-X takes Tx 1st from the CQ it answers: with Tx 1st on the runner's own period,
/// the answer goes out while the runner listens, never over its next CQ. On each tier a contest
/// runs on, for a CQ heard in either period.
#[test]
fn the_contests_sp_answers_each_cq_on_the_period_opposite_it() {
    for (contest, tier, answer) in [
        ("arrlfd", Tier::Ft8, "W9XYZ K1ABC 2B EMA"),
        ("arrlvhf_jan", Tier::Ft8, "W9XYZ K1ABC FN42"),
        ("txqp", Tier::Ft4, "W9XYZ K1ABC FN42"),
        ("arrlfd", Tier::TempoFast, "W9XYZ K1ABC 2B EMA"),
    ] {
        for cq_slot in [1, 2] {
            let mut e = engine_for(contest, MY_GRID);
            let mut s = e.settings().clone();
            s.tx_even = cq_slot % 2 == 0;
            e.apply_settings(s);
            e.set_tier(tier);
            e.set_mode("fieldday-sp").expect("the contest's S&P");
            e.set_tx_enabled(true);
            let cq = Partner::new(act_of(contest), true)
                .transmit()
                .expect("the runner's CQ");
            // Heard in period `cq_slot`, decoded at the boundary after it, and double-clicked.
            click(&mut e, &cq, cq_slot + 1);
            let first = keyed(&mut e, cq_slot + 1);
            let next = keyed(&mut e, cq_slot + 2);
            assert_eq!(
                (first.as_deref(), next.as_deref()),
                (Some(answer), None),
                "{contest} {}: the answer to a CQ heard in period {cq_slot}",
                tier_name(tier)
            );
        }
    }
}

/// ⭐ "Disable Tx after sending 73" turns TX off after a contest contact the operator picked with a
/// double-click on its CQ, once its last over has gone out, as WSJT-X turns it off after a
/// double-clicked contact's RR73 or 73; the station stays in the contest's S&P, listening. With
/// the setting off TX stays on. S&P with TX on and no double-click calls nobody, logs nothing and
/// leaves TX as it was.
#[test]
fn tx_goes_off_after_a_double_clicked_contest_contact() {
    let listening = "FieldDay running=false state=Listening";
    for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
        let s = scene(
            contest,
            Tier::Ft8,
            Role::DoubleClick,
            act_of(contest),
            MY_GRID,
        );
        assert_eq!(
            (s.logged.len(), s.tx, s.end.as_str()),
            (1, false, listening),
            "{contest}: TX off after the double-clicked contact, still in S&P"
        );
        let mut e = engine_for(contest, MY_GRID);
        let mut st = e.settings().clone();
        st.disable_tx_after_73 = false;
        e.apply_settings(st);
        let s = play(e, Tier::Ft8, Role::DoubleClick, act_of(contest));
        assert_eq!(
            (s.logged.len(), s.tx, s.end.as_str()),
            (1, true, listening),
            "{contest}: with the setting off TX stays on"
        );
        let s = scene(
            contest,
            Tier::Ft8,
            Role::SearchAndPounce,
            act_of(contest),
            MY_GRID,
        );
        assert_eq!(
            (s.sent.len(), s.logged.len(), s.tx, s.end.as_str()),
            (0, 0, true, listening),
            "{contest}: S&P alone calls nobody"
        );
    }
}

/// ⭐ NO FT MODE KEYS A CW ID, ON ANY PATH. With "CW ID after 73" on, no scene keys one: every
/// contest the picker offers on FT8 and FT4, and two grid contests and a state QSO party on FT1,
/// DX1 and FT2, through the Contest screen's Running and S&P and every gesture of the Operate
/// cockpit's. WSJT-X 3.0.2 clears the ID on every GUI tick in FT8 and FT4
/// (`widgets/mainwindow.cpp:7673`), whatever the activity; FT2, FT1 and DX1 do as FT8 does. The
/// gestures that leave the contest close an S&P contact with this station's 73 in QSO mode, the
/// over the ID followed. The control: the same gesture on Q65, which keeps the ID, keys one.
#[test]
fn no_ft_mode_keys_a_cw_id_on_any_path() {
    let roles = [
        Role::Running,
        Role::SearchAndPounce,
        Role::DoubleClick,
        Role::ClickFromSp,
        Role::ClickFromRun,
        Role::ClickFromQso,
        Role::CallCq,
        Role::CallCqFromQso,
        Role::StripSpFromRun,
        Role::StripSpFromQso,
    ];
    let cw_ids = |contest: &str, tier: Tier, role: Role| {
        let mut e = engine_for(contest, MY_GRID);
        let mut st = e.settings().clone();
        st.cw_id_after_73 = true;
        e.apply_settings(st);
        play(e, tier, role, act_of(contest)).cw_ids
    };
    let mut with_id = Vec::new();
    let tiers = [
        (&CONTESTS[..], Tier::Ft8),
        (&CONTESTS[..], Tier::Ft4),
        (&["arrlfd", "arrlvhf_jan", "txqp"][..], Tier::TempoFast),
        (&["arrlfd", "arrlvhf_jan", "txqp"][..], Tier::TempoDeep),
        (&["arrlfd", "arrlvhf_jan", "txqp"][..], Tier::Ft2),
    ];
    for (contests, tier) in tiers {
        for &contest in contests {
            for role in roles {
                let ids = cw_ids(contest, tier, role);
                if !ids.is_empty() {
                    with_id.push(format!("{contest:?} {} {role:?}: {ids:?}", tier_name(tier)));
                }
            }
        }
    }
    assert_eq!(with_id, Vec::<String>::new(), "CW IDs keyed in an FT mode");
    assert_eq!(
        cw_ids("txqp", Tier::Q65, Role::ClickFromSp),
        [7],
        "control: Q65 keys its CW ID after this station's 73"
    );
}

/// ⭐ The Operate strip's S&P hands the station to the running contest's S&P, its messages and
/// its log, from the contest's Running and from FT8 QSO mode, as the Contest screen's S&P does,
/// opening the contest's session when it enters the contest from outside it. Outside the
/// contest's own messages (Contest mode off, or FT1) it is FT8 QSO mode's S&P as before, and a
/// contest that cannot start refuses it and nothing changes.
#[test]
fn the_strips_sp_is_the_running_contests_sp() {
    let contest_sp = "FieldDay running=false state=Listening";
    for contest in ["arrlfd", "arrlvhf_jan", "txqp"] {
        for (from, opens) in [("fieldday-run", false), ("qso-monitor", true)] {
            let mut e = engine_for(contest, MY_GRID);
            e.set_tier(Tier::Ft8);
            e.set_mode(from).expect(from);
            let opened = e.start_search_and_pounce_opens_session();
            let r = e.start_search_and_pounce();
            assert_eq!(
                (r, opened, mode_line(&e)),
                (Ok(()), opens, contest_sp.to_string()),
                "{contest} from {from}"
            );
        }
    }
    let monitor = "Qso running=false state=Listening";
    for (on, tier) in [(false, Tier::Ft8), (true, Tier::TempoFast)] {
        let mut e = engine_for("arrlfd", MY_GRID);
        let mut s = e.settings().clone();
        s.fd_active = on;
        e.apply_settings(s);
        e.set_tier(tier);
        e.set_mode("qso-run").expect("a plain run");
        let opened = e.start_search_and_pounce_opens_session();
        let r = e.start_search_and_pounce();
        assert_eq!(
            (r, opened, mode_line(&e)),
            (Ok(()), false, monitor.to_string()),
            "Contest mode {on} on {}",
            tier_name(tier)
        );
    }
    let mut e = contest_that_cannot_start();
    let r = e.start_search_and_pounce();
    assert!(r.is_err(), "no class: {r:?}");
    assert_eq!(mode_line(&e), monitor, "still FT8 QSO mode");
}
