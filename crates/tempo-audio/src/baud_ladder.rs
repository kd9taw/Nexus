//! Test-CAT baud-ladder diagnosis — the "zero bytes" root-causer, for ANY serial rig.
//!
//! ⭐ **WHY THIS IS THE WHOLE FEATURE NOW** (operator decision, 2026-08-06). Nexus used to
//! impose a CAT rate when the operator picked a rig, from rate rows transcribed out of 61
//! hardware manuals. Four rounds of fixes each repaired the named rig and broke another, and the
//! fourth review found the IC-746 row simply WRONG (`[9600, 19200]`; the manual's Set-Mode item
//! 27 offers 300/1200/4800/9600/19200/AUTO), which silently clobbers an operator running one at
//! 4800 — the round-one bug, re-created by the fix for the round-one bug. The root cause was
//! never the logic: it is that hand transcription across 61 models has a failure rate, and every
//! wrong row overwrites a working setting. So the rows are gone except where Hamlib itself
//! states there is no choice (`serial_rate_min == serial_rate_max`), and **finding the rate
//! EMPIRICALLY is what replaces them**. Guessing from transcribed data is what kept breaking;
//! probing cannot.
//!
//! That makes this module load-bearing rather than a nicety, and it has to reach every rig whose
//! row was deleted — twenty-one of them, of which the ladder previously reached ZERO (see
//! [`ladder_applies`]).
//!
//! **Two ladders, because the rigs differ in what can PROVE a rate is right.**
//! - [`LadderKind::Civ`] — the rig speaks CI-V: probe the port raw, and a reply is proved by the
//!   rig's own bus address in the frame. Fast (~600 ms/rung) and cannot lock onto garbage.
//! - [`LadderKind::Hamlib`] — everything else: let Hamlib do the talking, one one-shot `rigctl`
//!   per rate, and prove the answer by [`classify_hamlib_probe`]. Slower, and the proof has to
//!   be built rather than read off a frame — see that function for why "it parsed as a number"
//!   is emphatically not enough.
//!
//! **Which ladder a rig gets is ASKED, never tabulated** ([`ladder_kind`]): Hamlib publishes a
//! `civaddr` conf parameter for exactly the rigs that speak CI-V. That is the same discipline
//! the rate rows now follow, applied to the ladder's own gate — and it is why this module no
//! longer consults `rigmodels::icom_scope_model`, which answers the unrelated question "does
//! this radio have a native panadapter?" and was being read as "does this radio speak CI-V?".
//!
//! FIELD FAILURE this exists for (IC-7610, 2026-07): the rig's USB CI-V port ships
//! factory-set to "Link to [REMOTE]", which caps it at the REMOTE-jack rate (≤ 19200,
//! usually Auto), while Nexus is configured for the 115200 the native scope needs. A
//! CI-V rig that can't clock our request never transmits ANYTHING — so the operator sees
//! "rig reply incomplete … (got \"\")" with zero bytes, in Hamlib mode and native mode
//! alike, and nothing says which side to fix. The same zero-byte signature also comes
//! from picking the WRONG of the rig's two COM ports (the dual-UART "Standard"/"Serial
//! Port B" side never speaks CI-V).
//!
//! So: when Test CAT fails on a serial Icom, walk the SAME port through the common CI-V
//! rates (configured rate first) with a direct, read-only CI-V frequency query — no
//! Hamlib in the loop — and tell the operator exactly which side to change, or that the
//! port itself is the wrong one. Read-only by construction: the only frame ever written
//! is `read_freq` (cmd `0x03`); nothing here can key or retune a radio.
//!
//! ⚠️ The real probe ([`run`]) must only be called while the radio loop has RELEASED the
//! CAT serial port (`Engine::hold_cat_port` → loop drops its daemon → ack): serial ports
//! are exclusive-open, and our own live daemon holds the port even when the rig is mute.
//! It runs in the `test_cat` command context, never inside the radio loop tick.
//!
//! Pure pieces (ladder order, reply classification, message composition) are unit-tested
//! here in the style of [`crate::control_line`]; only [`run`]/`probe_port_baud` touch a
//! real port, behind the `serial` feature.

use crate::civ::frame::{bcd_to_freq, FrameSplitter};

/// The common Icom CI-V rates tried after the configured one, most-likely first:
/// 19200 is the "Link to [REMOTE]" ceiling (and the Auto handshake rate), then the
/// older fixed rates, then the remaining "CI-V USB Baud Rate" menu picks (38400/57600 —
/// every selectable value must be here, or a rig set to one of them walks the whole
/// ladder silent and gets misdiagnosed as the wrong COM port), then 115200 for an
/// operator who configured something slower than the rig's USB default.
pub const LADDER_BAUDS: &[u32] = &[19200, 9600, 4800, 38400, 57600, 115200];

/// Which ladder a rig needs. Decided by [`ladder_kind`] from Hamlib's own caps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LadderKind {
    /// The rig speaks CI-V at this bus address: probe the port raw, no Hamlib in the loop.
    Civ { addr: u8 },
    /// Every other serial rig: Hamlib does the talking, one rate at a time, and these are the
    /// only two facts the ladder takes from its caps.
    Hamlib { caps: RigCaps },
}

/// The rig's CI-V bus address, **as Hamlib itself declares it** — `Some` for every rig whose
/// backend speaks CI-V, `None` for one that does not.
///
/// ⭐ **This replaced a five-model lookup, and the replacement is the point.** It used to be
/// `rigmodels::icom_scope_model(model).map(|m| m.default_civ_addr())`, and that function lists
/// the five radios with a native PANADAPTER — 3073/3078/3081/3085/3090. Reading it as "does this
/// speak CI-V?" silently excluded eleven CI-V rigs that simply have no scope: the IC-718, IC-746,
/// IC-746PRO, IC-756PROIII, IC-910, IC-7000 and IC-7100, and the four Xiegus. Every one of them
/// is a rig whose transcribed rate row was just deleted, so every one of them needs this.
///
/// Hamlib prints the answer itself: `rigctld -m <n> -L` lists a `civaddr` parameter ("Transceiver's
/// CI-V address") whose `Value:` is the backend's own default address, for CI-V backends and only
/// for them. **Its very presence is the CI-V test** — no model list to keep in step with anything.
///
/// Verified against the bundled rigctld 4.7.1 (decimal, as printed): 3073 IC-7300 → 148 (0x94)
/// and 3078 IC-7610 → 152 (0x98), which match the two addresses the old table had — independent
/// corroboration that this field is the same fact. The eleven newly covered: 3013 IC-718 → 94,
/// 3023 IC-746 → 86, 3046 IC-746PRO → 102, 3057 IC-756PROIII → 110, 3044 IC-910 → 96,
/// 3060 IC-7000 → 112, 3070 IC-7100 → 136, 3076 X108G → 112, 3087 X6100 → 164, 3089 X5105 → 112,
/// 3091 X6200 → 164. Absent, correctly, for 1001 FT-847, every Kenwood, 1042 FTDX-10, 2053 FX-4.
pub fn civ_addr_from_caps(rig_model: u32) -> Option<u8> {
    let model = rig_model.to_string();
    parse_civ_addr(&crate::rigctld_proc::daemon_dump(&[
        "-m",
        model.as_str(),
        "-L",
    ])?)
}

/// Read `civaddr` out of a `rigctld --show-conf` dump. Decimal, as Hamlib prints it.
///
/// Address 0 is refused: it is the frontend's "unset" default, never a rig's bus address, so a
/// backend that offered the parameter without seeding it would otherwise have us probing at an
/// address no radio answers to.
pub fn parse_civ_addr(show_conf: &str) -> Option<u8> {
    let (value, _) = crate::rigctld_proc::conf_param(show_conf, "civaddr")?;
    value.parse::<u8>().ok().filter(|addr| *addr != 0)
}

/// The two facts the ladder takes from Hamlib's `--dump-caps`, and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RigCaps {
    /// `Serial speed: 4800..57600 baud` — `serial_rate_min`/`_max`, the rates the BACKEND
    /// declares it can drive. Bounds the rungs: rates outside are ones Hamlib would not use, so
    /// probing them only spends the operator's time. It is also what makes a fixed-rate rig a
    /// ONE-rung ladder (2053 FX-4 prints `115200..115200`).
    pub serial_rates: Option<(u32, u32)>,
    /// The frequency ranges this backend declares the rig can RECEIVE, unioned over every
    /// region block. The plausibility test in [`classify_hamlib_probe`] — a rig cannot be tuned
    /// where it cannot receive, so a "frequency" outside every one of them did not come from
    /// this radio.
    pub rx_coverage: Vec<(u64, u64)>,
}

impl RigCaps {
    /// Is `hz` a frequency this rig could actually be sitting on?
    ///
    /// **An EMPTY coverage list answers `true`**, and that direction is deliberate: no coverage
    /// means we could not read caps at all, and an unreadable dump must not silently convert
    /// every rung into a rejection (which would report "your rig never answered" to an operator
    /// whose rig did). The mode veto still applies. In practice this cannot happen without the
    /// probe itself being impossible — the dump comes from the very binary that does the probing.
    pub fn covers(&self, hz: u64) -> bool {
        self.rx_coverage.is_empty()
            || self
                .rx_coverage
                .iter()
                .any(|(lo, hi)| (*lo..=*hi).contains(&hz))
    }
}

/// Ask Hamlib what this rig is: a CI-V bus, or something Hamlib must speak for us.
pub fn ladder_kind(rig_model: u32) -> LadderKind {
    if let Some(addr) = civ_addr_from_caps(rig_model) {
        return LadderKind::Civ { addr };
    }
    let model = rig_model.to_string();
    let caps = crate::rigctld_proc::daemon_dump(&["-m", model.as_str(), "-u"])
        .map(|d| parse_caps(&d))
        .unwrap_or_default();
    LadderKind::Hamlib { caps }
}

/// Read [`RigCaps`] out of a `rigctld --dump-caps` dump.
///
/// The shape is fixed by `dumpcaps.c`: a `Serial speed: <min>..<max> baud, 8N2, ctrl=…` line, and
/// `RX ranges #<n> for <region>:` blocks whose member lines are `\t<start> Hz - <end> Hz` (each
/// followed by indented VFO/Mode/Antenna lists, which is why a block ends at the next
/// UNINDENTED line and not at the next blank one). `RX ranges #n status for …` is a status line,
/// not a block, and opening a block on it would union in nothing and mask a real parse failure.
///
/// Anything we cannot read comes back empty, i.e. "claim nothing" — see [`RigCaps::covers`].
pub fn parse_caps(dump_caps: &str) -> RigCaps {
    let mut caps = RigCaps::default();
    let mut in_rx = false;
    for line in dump_caps.lines() {
        let line = line.trim_end_matches('\r');
        let indented = line.starts_with([' ', '\t']);
        if !indented {
            in_rx = line.starts_with("RX ranges #") && !line.contains(" status ");
        }
        if let Some(rest) = line.trim().strip_prefix("Serial speed:") {
            let mut halves = rest.trim().split("..");
            if let (Some(lo), Some(hi)) = (halves.next(), halves.next()) {
                let hi = hi.split_whitespace().next().unwrap_or("");
                if let (Ok(lo), Ok(hi)) = (lo.trim().parse(), hi.trim().parse()) {
                    caps.serial_rates = Some((lo, hi));
                }
            }
        }
        if in_rx && indented {
            if let Some((start, end)) = line.trim().split_once(" Hz - ") {
                let end = end.trim().trim_end_matches(" Hz");
                if let (Ok(start), Ok(end)) = (start.trim().parse(), end.parse()) {
                    caps.rx_coverage.push((start, end));
                }
            }
        }
    }
    caps
}

/// Does this Icom's built-in USB enumerate TWO virtual COM ports? Only the IC-7610 and
/// IC-9700 carry the dual-UART CP2105 ("Enhanced"/"Standard"); the IC-7300/705/905 show a
/// single port, so "try the other COM port" advice would send their owners hunting for a
/// port that does not exist. Must stay in step with the UI's dual-port hint
/// (SettingsPanel `[3078, 3081]`).
pub fn dual_com_ports(rig_model: u32) -> bool {
    use crate::civ::commands::IcomModel::{Ic7610, Ic9700};
    matches!(
        crate::rigmodels::icom_scope_model(rig_model),
        Some(Ic7610 | Ic9700)
    )
}

/// What a ladder run needs to know about the port before it touches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LadderGate {
    /// The control line the operator keys PTT with **on this very port**, when they do.
    ///
    /// ⚠️ Load-bearing for the Hamlib ladder, and it is a TX-safety matter. A serial driver
    /// RAISES RTS and DTR when the port is opened, and Hamlib puts a line back down only in the
    /// `RIG_PTT_SERIAL_RTS`/`_DTR` arms of `rig_open` — i.e. only when it has been TOLD that
    /// line is the keying line (see [`crate::rigctld_proc::LineState`], which documents the
    /// whole mechanism). An interface that keys on RTS is therefore keyed by the mere act of
    /// opening the port. Every rung hands Hamlib the SAME `-P <line> -p <port>` the live daemon
    /// gets, so a rung opens the port exactly the way the daemon that just failed did — parity,
    /// not new behaviour, and a sweep of five rungs is not five transmissions.
    pub keying: Option<crate::rig::SerialLine>,
}

/// Should a failed Test CAT run a ladder at all?
///
/// Only for a real serial port with a rig model set — there is no rate to find on a network rig
/// or an empty port field — and only when the failed probe exercised the CAT channel itself.
/// Mirrors the radio loop's `probed_cat` attribution predicate (service.rs `reprobe`):
/// "cat"/"vox" probe CAT; "rts"/"dtr" probe CAT only when keying shares the CAT port
/// (`ptt_serial_port` empty or equal to `serial_port`, case-insensitively, like
/// `Transport::ptt_port`). A dedicated-PTT-port failure is a PTT problem — probing the (healthy)
/// CAT port would find the rig answering at the configured rate and REPLACE the real "Could not
/// open serial port" error with a verdict blaming a backend that never failed.
///
/// ⭐ **What changed, and it is the whole of B.** This used to end in `icom_civ_addr(rig_model)`,
/// so it returned `None` for every rig that is not one of the five native-scope Icoms — and
/// that is ALL TWENTY-ONE rigs whose transcribed rate rows were just deleted: the FT-847, nine
/// Kenwoods, seven older Icoms and four Xiegus. The rigs the deletions strand were exactly the
/// rigs the ladder could not reach. It is now a question about the PORT, not about the model;
/// which ladder the rig then gets is [`ladder_kind`]'s job, asked of Hamlib.
pub fn ladder_applies(
    is_network: bool,
    rig_model: u32,
    serial_port: &str,
    ptt_method: &str,
    ptt_serial_port: &str,
) -> Option<LadderGate> {
    if is_network || serial_port.trim().is_empty() || rig_model == 0 {
        return None;
    }
    let keying = match ptt_method {
        "cat" | "vox" => None,
        "rts" | "dtr" => {
            let ptt = ptt_serial_port.trim();
            if !(ptt.is_empty() || ptt.eq_ignore_ascii_case(serial_port.trim())) {
                return None; // a dedicated PTT port: its failure says nothing about CAT
            }
            Some(if ptt_method == "rts" {
                crate::rig::SerialLine::Rts
            } else {
                crate::rig::SerialLine::Dtr
            })
        }
        _ => return None,
    };
    Some(LadderGate { keying })
}

/// The rates to try, in order: the configured rate first (re-checked directly, without
/// Hamlib in the loop), then [`LADDER_BAUDS`] minus the configured one.
pub fn ladder_bauds(configured: u32) -> Vec<u32> {
    let mut out = vec![configured];
    out.extend(LADDER_BAUDS.iter().copied().filter(|&b| b != configured));
    out
}

/// What one (port, baud) probe observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaudProbe {
    /// A well-formed CI-V frame came back from the rig — proof of life at this rate.
    /// `freq_hz` when the frame carried a readable frequency (cmd `03` reply or a
    /// `00` transceive broadcast).
    Reply { freq_hz: Option<u64> },
    /// Bytes arrived but no valid rig frame (line noise, a non-CI-V device, or only
    /// our own echo).
    Noise,
    /// The port opened cleanly and returned nothing at all.
    Silence,
    /// The port could not be opened (OS error text verbatim).
    OpenFailed(String),
    /// The PROBE TOOL itself could not be spawned (`rigctl` — `ErrorKind::NotFound`), so
    /// this is not a verdict about the port: no rung ever touched it. Carries the
    /// ready-to-show install cure ([`crate::rigctld_proc::hamlib_missing`]).
    ///
    /// ⭐ Why this is its own variant and not an [`BaudProbe::OpenFailed`]: on a Mac without
    /// Homebrew Hamlib, the CAT open already produced the correct "brew install hamlib"
    /// diagnosis — and then Test CAT's ladder mapped its own rigctl ENOENT into `OpenFailed`,
    /// whose verdict blames the serial port ("close WSJT-X … and test again") and OVERWRITES
    /// the good diagnosis last-writer-wins (mac QA audit, 2026-08-17). A missing prober must
    /// out-rank every guess about hardware it never probed.
    ProberMissing(String),
}

/// Classify the raw bytes one probe read back. Echoes of our own query (`from ==
/// CONTROLLER`) are NOT proof the rig answered — the splitter drops them, so an
/// echo-only read classifies as [`BaudProbe::Noise`].
pub fn classify_probe_bytes(raw: &[u8]) -> BaudProbe {
    if raw.is_empty() {
        return BaudProbe::Silence;
    }
    let frames = FrameSplitter::new().push(raw);
    if frames.is_empty() {
        return BaudProbe::Noise;
    }
    // A frequency, when one of the frames carries it: a `03` read-freq reply or a `00`
    // transceive broadcast — both hold 5-byte little-endian BCD.
    let freq_hz = frames
        .iter()
        .find(|f| matches!(f.cmd, 0x00 | 0x03) && f.data.len() >= 5)
        .map(|f| bcd_to_freq(&f.data[..5]));
    BaudProbe::Reply { freq_hz }
}

/// Everything the ladder observed, ready for [`compose_ladder_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderReport {
    pub port: String,
    pub configured_baud: u32,
    /// `(baud, outcome)` in probe order; stops early after the first [`BaudProbe::Reply`]
    /// (the diagnosis is complete) or [`BaudProbe::ProberMissing`] (every further rung
    /// would fail identically — the tool is gone, not the rate).
    pub outcomes: Vec<(u32, BaudProbe)>,
    /// Rungs the sweep never got to because it ran out of its time budget. Normally empty —
    /// the budget is a guard against a pathologically slow backend, not a routine truncation
    /// (see [`run_hamlib`]) — but when it is not, the verdict must SAY so rather than report
    /// "no rate answered", which would be advice about a port that was never tested.
    pub not_tried: Vec<u32>,
}

/// Walk [`ladder_bauds`] with `probe`, stopping at the first rate that gets a
/// [`BaudProbe::Reply`] (the diagnosis is complete at that point). Pure orchestration —
/// `probe` is the only thing that touches hardware, exactly like
/// [`crate::control_line::open_first_working_baud`].
pub fn run_ladder(
    port: &str,
    configured_baud: u32,
    probe: impl FnMut(u32) -> BaudProbe,
) -> LadderReport {
    run_ladder_over(
        port,
        configured_baud,
        ladder_bauds(configured_baud),
        || false,
        probe,
    )
}

/// [`run_ladder`] over an explicit rung list, with a way to run out of time.
///
/// `out_of_time` is consulted BEFORE each rung and never interrupts one in flight, so the budget
/// can only ever decline to start a probe — a half-probed rate would be a rate we could say
/// nothing honest about. Rungs it declines are recorded in [`LadderReport::not_tried`] so the
/// verdict can say they were never tested rather than imply they were silent.
pub fn run_ladder_over(
    port: &str,
    configured_baud: u32,
    bauds: Vec<u32>,
    mut out_of_time: impl FnMut() -> bool,
    mut probe: impl FnMut(u32) -> BaudProbe,
) -> LadderReport {
    let mut outcomes = Vec::new();
    let mut not_tried = Vec::new();
    for baud in bauds {
        if !outcomes.is_empty() && out_of_time() {
            not_tried.push(baud);
            continue;
        }
        let outcome = probe(baud);
        // Reply: the diagnosis is complete — a rate answered. ProberMissing: the probe TOOL
        // is gone (deterministic — every further rung would spawn-fail identically), so
        // walking on would only decorate a wrong "no rate answered" story with more rungs.
        let done = matches!(
            outcome,
            BaudProbe::Reply { .. } | BaudProbe::ProberMissing(_)
        );
        outcomes.push((baud, outcome));
        if done {
            break;
        }
    }
    LadderReport {
        port: port.to_string(),
        configured_baud,
        outcomes,
        not_tried,
    }
}

// ---------------------------------------------------------------------------------------
// The Hamlib ladder: for every serial rig that does not speak CI-V.
// ---------------------------------------------------------------------------------------

/// The rates the Hamlib ladder walks, most-likely-first, before [`RigCaps::serial_rates`]
/// narrows them.
///
/// This order is a HEURISTIC and is allowed to be one: being wrong about it costs seconds, never
/// a wrong verdict, because every rung is proved on its own evidence. (Contrast the deleted rate
/// rows, where being wrong overwrote a working setting.) 9600 and 4800 lead because the ten rigs
/// on this path are the FT-847 and nine Kenwoods — an IF-232C-era set whose factory rates are
/// down there — then the fast rates a modern operator would have chosen, then 1200 last, which
/// only the vintage Kenwoods' caps admit at all.
pub const HAMLIB_LADDER_BAUDS: &[u32] = &[9600, 4800, 57600, 19200, 38400, 115200, 1200];

/// The rungs to walk: the configured rate first (re-checked directly, which is what separates
/// "the rate is wrong" from "the rate is fine and the daemon fell over"), then
/// [`HAMLIB_LADDER_BAUDS`] narrowed to what the backend says it can drive.
///
/// The configured rate is probed even when it lies OUTSIDE those bounds — that is a real
/// configuration to diagnose, not one to skip.
pub fn hamlib_ladder_bauds(configured: u32, serial_rates: Option<(u32, u32)>) -> Vec<u32> {
    let mut out = vec![configured];
    out.extend(
        HAMLIB_LADDER_BAUDS.iter().copied().filter(|b| {
            *b != configured && serial_rates.is_none_or(|(lo, hi)| (lo..=hi).contains(b))
        }),
    );
    out
}

/// What Hamlib made of the rig's MODE — the protocol-agnostic twin of the CI-V address check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeRead {
    /// Hamlib named it (`USB`, `CW`, `PKTUSB`…): the bytes decoded to a real rig state.
    Named(String),
    /// The `m` command SUCCEEDED and Hamlib printed no name at all. It decoded a reply whose
    /// mode is not one this backend knows — `RIG_MODE_NONE`, which `rig_strrmode` prints as the
    /// empty string, with a 0 passband under it. **This is the garbage signature**, and it is
    /// Hamlib's own vocabulary check on the rig's own bytes, not a table of ours.
    Unnamed,
    /// No mode to read: the command failed, or was never asked. Says NOTHING either way.
    Absent,
}

/// One `rigctl … f [m]` run, as it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigctlRead {
    pub freq_hz: Option<u64>,
    pub mode: ModeRead,
}

/// Read what `rigctl` printed on stdout.
///
/// ⚠️ **The frequency is taken from the first line, or the second when a banner took the first,
/// and never from further in.** rigctl prints the value it was asked for and nothing else before
/// it; when a command FAILS it prints an `error = …` blob instead — and that blob is many lines
/// of Hamlib's own trace, in which a bare number can appear (`m`'s passband prints as one).
/// Scanning for "the first line that parses as a number" would read a passband, or a fragment of
/// a failure, as the rig's frequency.
///
/// ⭐ **The banner, and why the window is two lines rather than one.** [`probe_args`] passes
/// `-vvv` (it has to — see that function), and at that verbosity rigctl opens stdout with
/// `Opened rig model 1001, 'FT-847'` before printing anything it was asked for. Measured against
/// the bundled rigctl 4.7.1, `-vvv … f m` against a stand-in rig:
///
/// ```text
/// Opened rig model 1001, 'FT-847'
/// 14074000
/// USB
/// 2200
/// ```
///
/// So a one-line window would have read `Opened rig model …` as the answer and turned **every
/// healthy rung into [`BaudProbe::Silence`]** — which is why `-vvv` and this skip are one change
/// and cannot be split.
///
/// The skip is "one leading line that is not a positive integer", not a match on the banner's
/// wording, and that choice is deliberate. Wording-matching fails CLOSED if rigctl ever reworded
/// it: every rung silent, an invented "your rig never answered", and nothing downstream that can
/// notice. Skipping one non-numeric line fails OPEN in the one case it could be wrong — an error
/// blob with no banner, whose second line happened to be a bare number — and there the
/// plausibility conjunction in [`classify_hamlib_probe`] is exactly the backstop that rejects it
/// (a stray passband like `2200` is inside no rig's RX coverage). Fail toward the check that
/// exists.
pub fn parse_rigctl_read(stdout: &str) -> RigctlRead {
    let mut lines = stdout.lines().map(|l| l.trim_end_matches('\r').trim());
    let first = lines.next().unwrap_or("");
    let value = if first.parse::<u64>().is_ok() {
        first
    } else {
        lines.next().unwrap_or("")
    };
    let freq_hz = value.parse::<u64>().ok().filter(|hz| *hz > 0);
    if freq_hz.is_none() {
        return RigctlRead {
            freq_hz,
            mode: ModeRead::Absent,
        };
    }
    // `m` prints the mode then the passband. An EMPTY mode with a passband under it is the
    // decoded-but-unnameable case; an error blob is neither.
    let (second, third) = (lines.next().unwrap_or(""), lines.next().unwrap_or(""));
    let mode = if second.is_empty() {
        if !third.is_empty() && third.bytes().all(|b| b.is_ascii_digit()) {
            ModeRead::Unnamed
        } else {
            ModeRead::Absent
        }
    } else if is_mode_token(second) {
        ModeRead::Named(second.to_string())
    } else {
        ModeRead::Absent
    };
    RigctlRead { freq_hz, mode }
}

/// Does this look like a Hamlib mode token rather than a sentence? Shape, deliberately, not a
/// vocabulary: the check that matters is whether Hamlib could NAME the mode at all, and pinning
/// its mode list here would be one more transcription to go stale.
fn is_mode_token(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/'))
}

/// ⭐ **WHAT PROVES A RATE IS RIGHT, when there is no CI-V address to check.**
///
/// The CI-V ladder has an easy job: a reply frame carries the rig's bus address, so garbage
/// cannot impersonate it. Here there is no frame — only whatever Hamlib made of the bytes — and
/// **"it parsed as a number" is not proof.** Measured, against the bundled rigctl 4.7.1 driving
/// the real `ft847` backend with a stand-in rig answering mis-clocked bytes (30 trials, fresh
/// random reply each time — the harness and every number below are in the commit message):
///
/// | test | garbage streams it ACCEPTED |
/// |---|---|
/// | `f` printed a number | **30 / 30** |
/// | …and inside the rig's declared RX coverage | 8 / 30 |
/// | …and Hamlib could name the mode | 2 / 30 |
/// | **both** (what this function requires) | **0 / 30** |
///
/// One real observed value: 101054360 Hz — 101.05 MHz, which is inside no FT-847 RX range (its
/// coverage skips 76–108 MHz), with the mode printed empty. The round-four report's
/// `1054365010 Hz` is the same failure.
///
/// So a rate is proved when **both** hold, and each is a different property of the byte stream:
/// 1. the frequency lies in a range THIS BACKEND declares the rig can receive ([`RigCaps`]) — a
///    rig cannot be tuned where it cannot listen, so a number outside them did not come from it;
/// 2. Hamlib did not decode a mode it could not name ([`ModeRead::Unnamed`]) — its own
///    vocabulary check on the rig's own bytes.
///
/// **Why not "read it twice and require the same answer".** It was the other candidate and it is
/// unsound: a mis-clocked stream is DETERMINISTIC — the rig sends the same reply bytes and a
/// fixed rate error mis-samples them the same way — so two reads agree on the same rubbish. The
/// stand-in reproduces exactly that. A second read is still taken (see
/// [`probe_rate_via_hamlib`]), but as a second INDEPENDENT decode that must pass both tests
/// again, never as an equality check.
///
/// **Why the mode is a veto and not a requirement.** [`ModeRead::Absent`] — the `m` command
/// failing outright — cannot count against a rate: it is normal for a backend that answers `f`
/// from one wire read and cannot answer `m` from another, and requiring a mode would report "your
/// rig never answered" to an operator whose rig did. Only a mode Hamlib DECODED AND COULD NOT
/// NAME is evidence, and it is evidence of garbage. That asymmetry is why this is safe in both
/// directions on the ASCII (Kenwood) and binary (FT-847) families alike — and the nine Kenwoods
/// never reach it anyway: measured, their `;`-terminated protocol rejects mis-framed bytes at the
/// framing layer, printing no frequency at all.
pub fn classify_hamlib_probe(read: &RigctlRead, caps: &RigCaps) -> BaudProbe {
    let Some(hz) = read.freq_hz else {
        return BaudProbe::Silence;
    };
    if !caps.covers(hz) || read.mode == ModeRead::Unnamed {
        return BaudProbe::Noise;
    }
    BaudProbe::Reply { freq_hz: Some(hz) }
}

/// The things Hamlib says when the PORT ITSELF could not be opened, as opposed to a rig that did
/// not answer through it. Same three fragments `service::CAUSE_FRAGMENTS` selects on, and all
/// three are in the captures under `tests/fixtures/rigctld/`.
const OPEN_FAILURE_FRAGMENTS: &[&str] = &["does not exist", "is already open", "Unable to open"];

/// The one line of a failed `rigctl` run that says the port could not be opened, if it said so.
/// A busy COM port is one of the commonest CAT faults and its verdict ("close WSJT-X/flrig") is
/// completely different from a baud verdict, so it must not be reported as silence.
///
/// ⚠️ **Give it STDERR, and only stderr.** Hamlib prints an open failure through `rig_debug`,
/// which writes to stderr; stdout carries only what rigctl was asked for (plus the `-vvv`
/// banner) and, when a command failed, an `error = …` blob that names a READ timeout, never the
/// open. Measured against the bundled rigctl 4.7.1 — `-vvv -m 1001 -s 9600 f`, and the captures
/// are the four `tests/fixtures/rigctld/probe_*` files:
///
/// | state | rc | stdout | stderr |
/// |---|---|---|---|
/// | port not there (`-r COM99`) | 2 | *empty* | `serial_open: serial port COM99 does not exist` |
/// | port held by another program | 2 | *empty* | `… error 231: All pipe instances are busy.` + `serial_open: … is already open` |
/// | rig mute (opens, never answers) | 0 | banner + `error = …` blob | 13 lines, **no** open-failure fragment |
/// | rig working | 0 | banner + `14074000` | 2 lines, **no** open-failure fragment |
///
/// Handing it stdout as well cannot help (the fragments are never there) and can only widen what
/// a rig's own chatter could impersonate.
pub fn open_failure_line(output: &str) -> Option<String> {
    output
        .lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .find(|l| OPEN_FAILURE_FRAGMENTS.iter().any(|f| l.contains(f)))
        .map(|l| l.trim_start_matches("error = ").to_string())
}

/// Which rig family's CI-V prose a verdict may speak. Derived from the catalog name by
/// [`civ_family`], and the reason the enum exists is [`compose_ladder_message`]'s three
/// Icom-only facts — the rig menu, the [REMOTE] jack and Icom's USB driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CivFamily {
    /// An Icom. The verdict may quote `MENU » SET » Connectors » CI-V` and the
    /// "Link to [REMOTE]" ceiling, because **every** Icom has both.
    Icom,
    /// A Xiegu whose built-in USB enumerates TWO serial interfaces (X6100 / X6200 — a WCH
    /// CH342 pair, `USB-Enhanced-SERIAL-A`/`-B`, CAT on **B**; see
    /// [`crate::usbrig::civ_port_side`]).
    XieguDualUart,
    /// A Xiegu behind a single serial port (G90 / X5105 / X108G).
    XieguSingleUart,
    /// Any other rig that publishes a `civaddr`. Name no rig menu at all — the same
    /// discipline [`compose_hamlib_ladder_message`] states for its ten families.
    Other,
}

/// Classify a rig family from its CATALOG NAME (`rigmodels::rig_model_name`, which is what
/// the composer is handed).
///
/// ⚠️ **An unrecognised name is [`CivFamily::Other`], and that direction is deliberate.**
/// Getting this wrong costs prose, not bytes on a wire, so the failure that matters is the
/// one that ASSERTS something — a rig menu the operator does not have. Falling back to
/// "name no menu" loses a cure the operator could have used; falling back to Icom's menu
/// sends them to the radio for twenty minutes. `civ_family_is_pinned_to_the_catalog_names`
/// re-derives every CI-V model's family from the live catalog, so a rename fails there
/// rather than silently reverting this to "everything is an Icom".
pub fn civ_family(model_name: &str) -> CivFamily {
    let upper = model_name.to_ascii_uppercase();
    match upper.split_whitespace().next().unwrap_or_default() {
        "ICOM" => CivFamily::Icom,
        // The X-series pair is the only Xiegu split that changes the advice: two USB
        // interfaces instead of one. Everything else about the family is shared.
        "XIEGU" if upper.contains("X6100") || upper.contains("X6200") => CivFamily::XieguDualUart,
        "XIEGU" => CivFamily::XieguSingleUart,
        _ => CivFamily::Other,
    }
}

/// Turn a [`LadderReport`] into the operator-facing Test-CAT verdict: what answered
/// (if anything), and exactly what to change on which side. `native_selected` = the
/// radio is opted into the native CI-V backend (adds the diagnostic-log pointer);
/// `dual_ports` = the model enumerates two COM ports ([`dual_com_ports`]) — gates the
/// "try the other COM port" advice out of single-port rigs' verdicts.
///
/// ⚠️ **It is no longer only Icoms, and the prose has to follow.** [`ladder_kind`] was
/// broadened from a five-Icom lookup to "any rig that publishes a `civaddr`", which brought
/// the X-series Xiegus (3076/3087/3089/3091) onto this verdict. Three of its facts are
/// Icom-only — the `MENU » SET » Connectors » CI-V` cure, the "Link to [REMOTE]" ceiling
/// that explains a silent rate above 19200, and the closing "install Icom's USB driver" —
/// and every one of them was being told to Xiegu owners, whose radios have no such menu, no
/// [REMOTE] jack, and a Silicon Labs / WCH bridge Icom publishes no driver for. So each is
/// gated on [`CivFamily`], and where a Xiegu-specific fact cannot be grounded in this tree
/// the replacement text asserts nothing rather than inventing a menu path.
///
/// `mac` is the platform, as data, so both prose sets are testable on any box (the
/// `rigctld_launch_failed_for` discipline). The no-answer arms walk the operator through
/// the OS's own port UI, and until the mac QA audit (2026-08-17) every one of them talked
/// Windows at a Mac: "Windows Device Manager (Ports)", the CP210x "Enhanced" driver label,
/// and "install Icom's USB driver" — macOS ships the CP210x driver in-kernel and Icom
/// publishes no Mac driver, so that advice dead-ends. On a Mac the same walkthrough is done
/// in `/dev/cu.*` vocabulary; the dual-UART tie-break there is port ORDER (the Silicon Labs
/// VCP driver names the CI-V "Enhanced" side plain `cu.SLAB_USBtoUART`, the dead "Standard"
/// side gets a suffix), because the Windows driver labels this text leans on do not exist in
/// the mac port names. **Family and platform are independent dimensions** — the platform
/// split is not collapsed or duplicated by the family split, it is nested inside it.
///
/// `mac` is the platform, as data, so both prose sets are testable on any box (the
/// `rigctld_launch_failed_for` discipline). The no-answer arms walk the operator through
/// the OS's own port UI, and until the mac QA audit (2026-08-17) every one of them talked
/// Windows at a Mac: "Windows Device Manager (Ports)", the CP210x "Enhanced" driver label,
/// and "install Icom's USB driver" — macOS ships the CP210x driver in-kernel and Icom
/// publishes no Mac driver, so that advice dead-ends. On a Mac the same walkthrough is done
/// in `/dev/cu.*` vocabulary; the dual-UART tie-break there is port ORDER (the Silicon Labs
/// VCP driver names the CI-V "Enhanced" side plain `cu.SLAB_USBtoUART`, the dead "Standard"
/// side gets a suffix), because the Windows driver labels this text leans on do not exist in
/// the mac port names.
pub fn compose_ladder_message(
    r: &LadderReport,
    model_name: &str,
    civ_addr: u8,
    native_selected: bool,
    dual_ports: bool,
    mac: bool,
) -> String {
    let port = &r.port;
    let configured = r.configured_baud;
    let family = civ_family(model_name);
    let mhz = |freq_hz: Option<u64>| {
        freq_hz
            .filter(|&hz| hz > 0)
            .map(|hz| format!(" (reads {:.3} MHz)", hz as f64 / 1e6))
            .unwrap_or_default()
    };
    // The rig answered at the CONFIGURED rate when probed directly → the serial side is
    // fine and the CAT backend between us and the port is what fell over.
    if let Some((_, BaudProbe::Reply { freq_hz })) = r
        .outcomes
        .first()
        .filter(|(b, o)| *b == configured && matches!(o, BaudProbe::Reply { .. }))
    {
        let backend = if native_selected {
            "the native CI-V daemon"
        } else {
            // Not "the bundled rigctld": nothing is bundled on macOS or in the AppImage.
            "the CAT daemon (Hamlib rigctld)"
        };
        let diag = if native_selected {
            " If it keeps failing, turn on the CI-V diagnostic log (Settings » Radio) and send \
             the capture with a bug report."
        } else {
            ""
        };
        return format!(
            "The rig answers CI-V directly on {port} @ {configured} baud{f} — port, cable and \
             baud are all fine, so {backend} is what failed. Save the settings again to relaunch \
             it.{diag}",
            f = mhz(*freq_hz)
        );
    }
    // Another rate answered → say exactly which side to change. The APP side is the same
    // sentence for every family; the RIG side is where the Icom-only facts live.
    if let Some((baud, freq_hz)) = r.outcomes.iter().find_map(|(b, o)| match o {
        BaudProbe::Reply { freq_hz } => Some((*b, *freq_hz)),
        _ => None,
    }) {
        let rig_side = match family {
            CivFamily::Icom => {
                // The [REMOTE] ceiling only explains a CONFIGURED rate above 19200, and only
                // on a rig that has the jack — i.e. this arm and this arm only. (It is the
                // Xiegu case by default, since the app ships 38400, which is exactly why it
                // could not stay ungated.)
                let why = if configured > 19_200 {
                    format!(
                        " (At the factory default \"Link to [REMOTE]\" the USB CI-V port follows \
                         the slower [REMOTE]-jack rate, which tops out at 19200 — that is why \
                         {configured} got silence.)"
                    )
                } else {
                    String::new()
                };
                format!(
                    "Fix either side — set Baud to {baud} here in Settings, or set the rig to \
                     {configured}: MENU » SET » Connectors » CI-V » \"CI-V USB Baud Rate\" = \
                     {configured} and \"CI-V USB Port\" = \"Unlink from [REMOTE]\".{why}"
                )
            }
            // No menu is named, because none can be: nothing in this tree records where (or
            // whether) a Xiegu's CI-V rate is settable, and the app side is a cure that
            // always exists. Same discipline as `compose_hamlib_ladder_message`.
            _ => format!(
                "Set Baud to {baud} here in Settings — that is the side to change. If you would \
                 rather change the radio, look up its CAT / CI-V rate setting in the rig's own \
                 manual."
            ),
        };
        return format!(
            "Found it: the rig answers CI-V on {port} at {baud} baud{f}, not the configured \
             {configured}. {rig_side}",
            f = mhz(freq_hz)
        );
    }
    // No rate answered. Say why that usually is, in the order it actually happens.
    let tried = r
        .outcomes
        .iter()
        .map(|(b, _)| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if r.outcomes
        .iter()
        .all(|(_, o)| matches!(o, BaudProbe::OpenFailed(_)))
    {
        let os_err = r
            .outcomes
            .iter()
            .find_map(|(_, o)| match o {
                BaudProbe::OpenFailed(e) => Some(e.as_str()),
                _ => None,
            })
            .unwrap_or("unknown error");
        return format!(
            "Test CAT could not open {port} at any rate (tried {tried}) — the system said: \
             {os_err}. Usually another program is holding the port — close other CAT/logging \
             software (WSJT-X, flrig, RS-BA1) and test again."
        );
    }
    let noise = if r
        .outcomes
        .iter()
        .any(|(_, o)| matches!(o, BaudProbe::Noise))
    {
        if mac {
            " The port did carry bytes at one rate, but not valid CI-V — that usually means a \
             different device is on this port."
        } else {
            " The port did carry bytes at one rate, but not valid CI-V — that usually means a \
             different device is on this COM port."
        }
    } else {
        ""
    };
    // Two ports or one? For an Icom that is the caller's `dual_ports` ([`dual_com_ports`] —
    // the CP2105 models). For a Xiegu it is the family itself, and the answer differs from
    // the Icom one in BOTH directions: the X6100/X6200 do show a pair (a WCH CH342, which
    // `dual_com_ports` does not know about), and the G90/X5105/X108G show one port.
    let two_ports = match family {
        CivFamily::XieguDualUart => true,
        CivFamily::XieguSingleUart => false,
        _ => dual_ports,
    };
    // The port-identity walkthrough leans on the OS's own port UI, so it is per-platform
    // prose: Device Manager and the CP210x driver's "Enhanced" label exist only on Windows;
    // on a Mac the twins are told apart by /dev/cu.* NAME ORDER (see the fn doc). The pair
    // arms are additionally per-FAMILY, because the two pairs are told apart by different
    // labels and their CAT sides are on OPPOSITE letters.
    let port_identity = match (two_ports, mac) {
        (true, true) => match family {
            CivFamily::XieguDualUart => format!(
                "This usually means {port} is not the rig's CAT port: the {model_name}'s \
                 built-in USB shows up as TWO /dev/cu.* ports and only one speaks CI-V. The \
                 port picker labels each one with its USB name — pick the one whose name ends \
                 SERIAL-B, and try the rig's other cu.* port."
            ),
            _ => format!(
                "This usually means {port} is not the rig's CI-V port: this Icom's USB shows up \
                 as TWO /dev/cu.* ports and only one speaks CI-V. With the Silicon Labs VCP \
                 driver the CI-V one is usually the FIRST of the pair (plain cu.SLAB_USBtoUART; \
                 the dead twin gets a numeric suffix) — try the rig's other cu.* port."
            ),
        },
        (true, false) => match family {
            CivFamily::XieguDualUart => format!(
                "This usually means {port} is not the rig's CAT port: the {model_name}'s \
                 built-in USB shows up as TWO COM ports and only one speaks CI-V. Windows names \
                 them \"USB-Enhanced-SERIAL-A\" and \"USB-Enhanced-SERIAL-B\" in Device Manager \
                 (Ports); CAT answers on the SERIAL-B one — try the other COM port."
            ),
            _ => format!(
                "This usually means {port} is not the rig's CI-V port: this Icom's USB shows up \
                 as TWO COM ports and only one speaks CI-V. In Windows Device Manager (Ports), \
                 the CI-V one is the CP210x port marked \"Enhanced\" (Icom's driver labels it \
                 \"Serial Port A (CI-V)\"); the \"Standard\" / \"Serial Port B\" one never \
                 answers — try the other COM port."
            ),
        },
        (false, true) => format!(
            "This usually means {port} is not the rig: {model_name} shows a single port — \
             unplug the rig's USB cable and confirm {port} is the one that disappears from \
             the port list, then reconnect."
        ),
        (false, false) => format!(
            "This usually means {port} is not the rig: {model_name} shows a single COM port — \
             unplug the rig's USB cable and confirm {port} is the one that disappears from \
             Device Manager (Ports), then reconnect."
        ),
    };
    // The closing checks. Two of them are Icom-only: `MENU » … » CI-V Address` names a menu,
    // and "install Icom's USB driver" names a download that exists for Windows and for Icoms.
    // Everything else here rides on the same per-platform split as above — macOS ships the
    // CP210x/FTDI drivers in-kernel, so a rig with no port at all is a hardware /
    // System-Information question there, not a download.
    let also_check = match family {
        CivFamily::Icom if mac => format!(
            "Also check: the radio is on; the rig menu CI-V Address is at its default \
             ({civ_addr:02X}h); and if no cu.* port appears at all, confirm the rig shows up in \
             System Information » USB (the driver is built into macOS — there is nothing to \
             install)."
        ),
        CivFamily::Icom => format!(
            "Also check: the radio is on; the rig menu CI-V Address is at its default \
             ({civ_addr:02X}h); and if no COM ports appear at all, install Icom's USB driver."
        ),
        // The address is still worth naming — a bus set to a different address is silent in
        // exactly this way — but WHERE it is set is the radio's manual, not ours to guess.
        _ if mac => format!(
            "Also check that the radio is on. Nexus asked at CI-V address {civ_addr:02X}h, the \
             default Hamlib publishes for this model — a radio answering at a different address \
             stays silent. If no cu.* port appears at all, confirm the rig shows up in System \
             Information » USB and check Detect for a driver hint: macOS ships the Silicon Labs \
             and FTDI drivers in-kernel, but a WCH bridge can still want one on older macOS."
        ),
        _ => format!(
            "Also check that the radio is on. Nexus asked at CI-V address {civ_addr:02X}h, the \
             default Hamlib publishes for this model — a radio answering at a different address \
             stays silent. If no COM ports appear at all, install the USB-serial driver for the \
             rig's bridge chip — Detect names the chip and links its driver."
        ),
    };
    format!(
        "{model_name} on {port} never answered CI-V at any rate (tried {tried}).{noise} \
         {port_identity} {also_check}"
    )
}

/// The Hamlib ladder's verdict. Same job as [`compose_ladder_message`], different evidence and
/// different cures: there is no CI-V address to check, no rig menu whose path we know, and no
/// second COM port to send anyone hunting for.
///
/// ⚠️ **It never names a rig menu.** The CI-V verdict can quote `MENU » SET » Connectors » CI-V`
/// because every Icom has it; the ten rigs here have ten different menus, and printing a guessed
/// path is the transcription mistake this whole feature exists to stop. It says *what* to change
/// and leaves *where* to the rig's manual.
pub fn compose_hamlib_ladder_message(r: &LadderReport, model_name: &str) -> String {
    let (port, configured) = (&r.port, r.configured_baud);
    let mhz = |hz: Option<u64>| {
        hz.filter(|&hz| hz > 0)
            .map(|hz| format!(" (reads {:.3} MHz)", hz as f64 / 1e6))
            .unwrap_or_default()
    };
    let tried = r
        .outcomes
        .iter()
        .map(|(b, _)| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    // The probe TOOL is missing — FIRST, above every verdict about the port, because no rung
    // ever touched the port and every other arm would be a guess about hardware that was
    // never probed. The carried message is the per-platform install cure, already composed
    // where the spawn failed; it must reach the operator VERBATIM — this is the arm that
    // stops the "close WSJT-X" guess from overwriting the correct "brew install hamlib"
    // diagnosis (mac QA audit, 2026-08-17).
    if let Some((_, BaudProbe::ProberMissing(msg))) = r
        .outcomes
        .iter()
        .find(|(_, o)| matches!(o, BaudProbe::ProberMissing(_)))
    {
        return msg.clone();
    }
    // The rig answered at the CONFIGURED rate when asked directly → the link is fine and the
    // CAT daemon between us and the port is what fell over.
    if let Some((_, BaudProbe::Reply { freq_hz })) = r
        .outcomes
        .first()
        .filter(|(b, o)| *b == configured && matches!(o, BaudProbe::Reply { .. }))
    {
        return format!(
            "{model_name} answers on {port} @ {configured} baud{f} when asked directly — port, \
             cable and baud are all fine, so the CAT daemon (Hamlib rigctld) is what failed. \
             Save the settings again to relaunch it.",
            f = mhz(*freq_hz)
        );
    }
    // Another rate answered → name it, and both ways to fix it.
    if let Some((baud, freq_hz)) = r.outcomes.iter().find_map(|(b, o)| match o {
        BaudProbe::Reply { freq_hz } => Some((*b, *freq_hz)),
        _ => None,
    }) {
        return format!(
            "Found it: {model_name} answers on {port} at {baud} baud{f}, not the configured \
             {configured}. Fix either side — set Baud to {baud} here in Settings, or set the \
             radio's own CAT/serial menu to {configured}.",
            f = mhz(freq_hz)
        );
    }
    // Nothing answered. The port itself, first: it is a different fault with a different cure.
    if let Some((_, BaudProbe::OpenFailed(e))) = r
        .outcomes
        .iter()
        .find(|(_, o)| matches!(o, BaudProbe::OpenFailed(_)))
    {
        return format!(
            "Test CAT could not open {port} (tried {tried}) — the system said: {e}. Usually \
             another program is holding the port: close other CAT/logging software (WSJT-X, \
             flrig, N1MM) and test again."
        );
    }
    let noise = if r
        .outcomes
        .iter()
        .any(|(_, o)| matches!(o, BaudProbe::Noise))
    {
        format!(
            " Bytes DID come back at one rate, but they did not decode as anything {model_name} \
             could be doing — so something is on {port}, but either it is not this radio or the \
             rate is close enough to frame and still wrong."
        )
    } else {
        String::new()
    };
    // A truncated sweep must never read as "your rig is silent" — it did not get that far.
    if !r.not_tried.is_empty() {
        let left = r
            .not_tried
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return format!(
            "{model_name} did not answer on {port} at {tried}.{noise} The check ran out of time \
             before trying {left} — run Test CAT again to carry on, or set Baud to one of those \
             and test that directly."
        );
    }
    format!(
        "{model_name} never answered on {port} at any rate its Hamlib backend supports (tried \
         {tried}).{noise} That usually means {port} is not the radio: unplug the rig's USB or \
         serial cable and confirm {port} is the one that disappears from the port list, then \
         reconnect. Also check the radio is on, that its CAT/serial port is enabled in the rig \
         menu, and that the USB driver is installed."
    )
}

/// The argument vector for one rung, built where it can be read and tested — the same discipline
/// as [`crate::rigctld_proc::rigctld_args`], and for the same reason: what is on this line is the
/// whole difference between a diagnosis and a guess.
///
/// **Read-only by construction.** The only `commands` any caller passes are `f` (get frequency)
/// and `m` (get mode) — Hamlib GET verbs. Nothing here can key or retune a radio.
///
/// ⭐ **`-vvv`, and it is not optional.** Hamlib prints through `rig_debug`, and rigctl leaves
/// the debug level at `RIG_DEBUG_NONE` unless it is asked. Measured, bundled rigctl 4.7.1: a port
/// that does not exist and a port another program is holding **both exit 2 having printed nothing
/// at all — zero bytes on stdout AND stderr** — so without this flag [`open_failure_line`] has
/// nothing to find, every rung of a busy port comes back [`BaudProbe::Silence`], and an operator
/// whose COM port is held by WSJT-X walks the whole sweep to be told to check the cable. The exit
/// code cannot stand in for it either: a Kenwood rejecting mis-framed bytes fails `rig_open` and
/// exits 2 as well, with the same empty streams. `rc=2` means "no value", not "no port".
///
/// It is also parity: the live daemon is launched with `-vvv` for this very reason
/// ([`crate::rigctld_proc::rigctld_args`]). Measured cost of the flag: none (3.56 s vs 3.56 s on
/// a rig that answers, 4.15 s vs 4.14 s on one that does not).
///
/// ⚠️ It cannot ship without the [`parse_rigctl_read`] change: `-vvv` also puts an `Opened rig
/// model …` banner on stdout ahead of the value.
///
/// ⭐ **No `--set-conf`, and that is the second fix.** This used to pass
/// `--set-conf=timeout=300,retry=0`, justified by wire time ("a 5-byte reply at 4800 baud is
/// ~10 ms"). Wire time is not what those numbers are for — the rig's turnaround is — and every
/// backend the Hamlib ladder can reach declares a longer budget than 300 ms, most of them with
/// retries this was deleting (`rigctl -m N -u`, bundled 4.7.1, measured):
///
/// | model | declares |
/// |---|---|
/// | 1001 FT-847 | 1000 ms, 0 retry |
/// | 2004 TS-570D · 2016 TS-570S · 2010 TS-870S | 500 ms, **10 retry** |
/// | 2011 TS-940S | 600 ms, 10 retry |
/// | 3073 IC-7300 · 2053 FX-4 · 2028 TS-480 | 500–1000 ms, 3 retry |
/// | 1042 FTDX-10 · 1031 FT-980 | 2000 ms, 3 retry |
/// | 1020 FT-817 · 1041 FT-818 | 3000 ms, 5 retry |
///
/// **The probe is issuing a verdict about the daemon, so it has to hold the daemon's
/// conversation.** [`crate::rigctld_proc::rigctld_args`] passes no `--set-conf` at all — the
/// daemon runs at the backend's declared timeout and retries — so any narrowing here makes the
/// probe answer a question nobody asked, and a rung it calls silent is then a rate the daemon may
/// well have read. That lands hardest on the three Kenwoods whose transcribed rate rows were
/// deleted (TS-570D/S, TS-870S: 500 ms × 10 retries), for whom this ladder is now the only thing
/// there is.
///
/// The alternative — read the declared timeout out of the caps dump this module already parses
/// and pass it back — was rejected as *the same numbers with a parser in front of them*: passing
/// the declared timeout while keeping `retry=0` would still be stricter than the daemon on eight
/// of the backends above, and passing declared timeout AND declared retries is byte-for-byte what
/// Hamlib does when handed nothing. See [`run_hamlib`] for what dropping it costs.
///
/// Pure, and separated out so it can be read and asserted on without a rig — but gated with its
/// caller, like [`crate::rigctld_proc::resolve_rigctl`], so the headless workspace clippy job
/// does not see it as dead code.
#[cfg(feature = "serial")]
fn probe_args(
    port: &str,
    baud: u32,
    rig_model: u32,
    keying: Option<crate::rig::SerialLine>,
    commands: &[&str],
) -> Vec<String> {
    let mut args = vec![
        "-vvv".to_string(),
        "-m".to_string(),
        rig_model.to_string(),
        "-r".to_string(),
        port.to_string(),
        "-s".to_string(),
        baud.to_string(),
    ];
    // Hand Hamlib the same keying override the live daemon gets, so `rig_open` lowers the line
    // it keys with instead of leaving the driver's power-on HIGH on a keyed interface. See
    // [`LadderGate::keying`] — this is parity with the daemon that just failed, not new
    // behaviour, and it is what keeps a five-rung sweep from being five transmissions.
    if let Some(line) = keying {
        args.push("-P".to_string());
        args.push(crate::rigctld_proc::ptt_type_token(line).to_string());
        args.push("-p".to_string());
        args.push(port.to_string());
    }
    args.extend(commands.iter().map(|c| c.to_string()));
    args
}

/// What one `rigctl` run printed, kept on the stream it printed it on.
///
/// They used to be concatenated. They cannot be: [`open_failure_line`] must see stderr and only
/// stderr, and [`parse_rigctl_read`] must see stdout from its first line — gluing them made the
/// first impossible (the flag that produces the text was missing) and would now corrupt the
/// second.
#[cfg(feature = "serial")]
struct RigctlOutput {
    /// What rigctl was asked for, behind the `-vvv` banner.
    stdout: String,
    /// What Hamlib had to say about it, including an open failure.
    stderr: String,
}

/// One `rigctl` run against the rig: [`probe_args`] executed in a single open.
#[cfg(feature = "serial")]
fn rigctl_read(
    port: &str,
    baud: u32,
    rig_model: u32,
    keying: Option<crate::rig::SerialLine>,
    commands: &[&str],
) -> Result<RigctlOutput, std::io::Error> {
    let mut cmd = std::process::Command::new(crate::rigctld_proc::resolve_rigctl());
    cmd.args(probe_args(port, baud, rig_model, keying, commands));
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.output() {
        // The exit status is not consulted: rigctl exits 0 when a command failed but the port
        // opened, and 2 both for a port it could not open AND for a backend whose `rig_open`
        // handshake was answered with garbage. What separates those is what it PRINTED, and
        // where.
        Ok(out) => Ok(RigctlOutput {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }),
        // TYPED, not stringified: the caller must be able to tell "rigctl itself is not
        // installed" (NotFound → [`BaudProbe::ProberMissing`]) from a port fault — flattening
        // this to a String is exactly how the ENOENT got dressed up as a busy COM port.
        Err(e) => Err(e),
    }
}

/// One (port, baud) rung of the Hamlib ladder.
///
/// Two stages, and the split is what keeps the sweep affordable: the rung itself asks only `f`
/// (a silent rung is the common case and costs one read), and only a rung that produces a
/// coverage-plausible frequency pays for the confirm — a SECOND, independent open asking `f m`,
/// which must satisfy [`classify_hamlib_probe`] all over again. The confirm is where the mode
/// veto can bite, and it is a fresh decode rather than a re-read of the same bytes.
#[cfg(feature = "serial")]
fn probe_rate_via_hamlib(
    port: &str,
    baud: u32,
    rig_model: u32,
    keying: Option<crate::rig::SerialLine>,
    caps: &RigCaps,
) -> BaudProbe {
    let out = match rigctl_read(port, baud, rig_model, keying, &["f"]) {
        Ok(out) => out,
        // rigctl itself would not spawn. NotFound = Hamlib's tools are not installed — a
        // different fault from any port fault, with a different (per-platform) cure; see
        // [`BaudProbe::ProberMissing`] for the mac field failure this arm exists for.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return BaudProbe::ProberMissing(crate::rigctld_proc::hamlib_missing("rigctl", &e))
        }
        Err(e) => return BaudProbe::OpenFailed(e.to_string()),
    };
    // The port first, off STDERR, where Hamlib says so — see [`open_failure_line`]. A port
    // another program is holding is a different fault with a different cure, and reporting it as
    // silence sends the operator after a cable.
    if let Some(line) = open_failure_line(&out.stderr) {
        return BaudProbe::OpenFailed(line);
    }
    match classify_hamlib_probe(&parse_rigctl_read(&out.stdout), caps) {
        BaudProbe::Reply { .. } => match rigctl_read(port, baud, rig_model, keying, &["f", "m"]) {
            Ok(confirm) => classify_hamlib_probe(&parse_rigctl_read(&confirm.stdout), caps),
            // A rung whose FIRST read spawned cannot lose the binary before the confirm in
            // any real install, but the arm must still not invent a port fault out of it.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                BaudProbe::ProberMissing(crate::rigctld_proc::hamlib_missing("rigctl", &e))
            }
            Err(e) => BaudProbe::OpenFailed(e.to_string()),
        },
        other => other,
    }
}

/// **The Hamlib ladder.** Walks `port` through the rates this rig's backend can drive until one
/// answers provably ([`classify_hamlib_probe`]).
///
/// ⏱ **What it costs, and when the operator pays it.** It runs ONLY after a Test CAT that has
/// already failed — never when a rig is picked, which is the whole point of deleting the rate
/// rows: choosing a radio now imposes nothing and waits for nothing. A rung that ANSWERS ends the
/// sweep there, so the full length is only ever paid when nothing answers at all.
///
/// **A silent rung costs whatever the backend's own read budget costs**, and that is the price of
/// [`probe_args`] no longer narrowing it. Measured per silent rung against a stand-in rig that
/// accepts and stays mute, bundled rigctl 4.7.1 (same harness, `--set-conf=timeout=300,retry=0`
/// vs nothing; these absolutes carry ~3.5 s of harness overhead a serial port does not pay, so
/// read the columns against each other):
///
/// | model | declared | with the old override | at the backend's own budget |
/// |---|---|---|---|
/// | 2004 TS-570D | 500 ms × 10 | 6.75 s | **6.37 s** (cheaper: `rig_open` fails fast) |
/// | 1001 FT-847 | 1000 ms × 0 | 11.02 s | 12.25 s |
/// | 1042 FTDX-10 | 2000 ms × 3 | 11.22 s | **18.15 s** |
/// | 1020 FT-817 / 1041 FT-818 | 3000 ms × 5 | 12.75 s | **39.93 s** |
///
/// So the Kenwoods are unchanged, the FT-847's five-rung sweep goes from ~31 s to ~40 s (the
/// figure the round-five review measured on real serial), and the Yaesu newcat rigs are the
/// outlier the old override was hiding: an FT-817 that answers nothing cannot be swept inside any
/// budget an operator would sit through, and never could — at 12.75 s a rung it already
/// overran 45 s.
///
/// `BUDGET` is a ceiling on the wait, not a routine truncation, and it is sized off that table:
/// the slowest backend a complete sweep is promised for is the FTDX-10, whose widest ladder is
/// five rungs, so the last rung must be allowed to START at 4 × 18.15 s ≈ 73 s. **90 s** clears
/// that; the FT-847 (≈ 61 s for five) and every Kenwood (≈ 45 s for seven) clear it comfortably.
/// It was 45 s, which the FTDX-10 would now overrun — a budget left there would have converted
/// this fix into a truncated sweep for a current, common radio. When it does fire, the rungs it
/// declined are reported as untested rather than as silence.
#[cfg(feature = "serial")]
pub fn run_hamlib(
    port: &str,
    configured_baud: u32,
    rig_model: u32,
    gate: LadderGate,
    caps: &RigCaps,
) -> LadderReport {
    const BUDGET: std::time::Duration = std::time::Duration::from_secs(90);
    let deadline = std::time::Instant::now() + BUDGET;
    run_ladder_over(
        port,
        configured_baud,
        hamlib_ladder_bauds(configured_baud, caps.serial_rates),
        || std::time::Instant::now() >= deadline,
        |baud| probe_rate_via_hamlib(port, baud, rig_model, gate.keying, caps),
    )
}

/// One real (port, baud) probe: open, send a single read-only CI-V `read_freq`, gather
/// whatever comes back for ~600 ms, classify.
#[cfg(feature = "serial")]
fn probe_port_baud(port: &str, baud: u32, civ_addr: u8) -> BaudProbe {
    use std::io::Read;
    use std::time::{Duration, Instant};
    let mut sp = match serialport::new(port, baud)
        .timeout(Duration::from_millis(50))
        .open()
    {
        Ok(sp) => sp,
        Err(e) => return BaudProbe::OpenFailed(e.to_string()),
    };
    let query = crate::civ::commands::read_freq(civ_addr).to_bytes();
    if let Err(e) = std::io::Write::write_all(&mut sp, &query).and_then(|()| sp.flush()) {
        return BaudProbe::OpenFailed(e.to_string());
    }
    let mut raw = Vec::new();
    let mut buf = [0u8; 256];
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        match sp.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                // A frame terminator is in hand — classify now rather than sitting
                // out the rest of the window.
                if raw.contains(&crate::civ::frame::END)
                    && matches!(classify_probe_bytes(&raw), BaudProbe::Reply { .. })
                {
                    break;
                }
            }
            // Timeout = no bytes this tick; anything else = the port died mid-read.
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                if raw.is_empty() {
                    return BaudProbe::OpenFailed(e.to_string());
                }
                break;
            }
        }
    }
    classify_probe_bytes(&raw)
}

/// The real ladder: probe `port` at [`ladder_bauds`] until a rate answers. Blocking for
/// up to ~3 s — call from the `test_cat` command (off the radio loop), with the loop's
/// CAT port hold acknowledged.
#[cfg(feature = "serial")]
pub fn run(port: &str, configured_baud: u32, civ_addr: u8) -> LadderReport {
    run_ladder(port, configured_baud, |baud| {
        probe_port_baud(port, baud, civ_addr)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ::frame::Frame;

    // ---- ladder order ----

    #[test]
    fn ladder_tries_the_configured_rate_first_then_the_common_civ_rates() {
        // The native-scope config (the field failure): 115200 configured → re-check it
        // directly, then the "Link to [REMOTE]" ceiling first among the alternatives.
        assert_eq!(
            ladder_bauds(115200),
            vec![115200, 19200, 9600, 4800, 38400, 57600]
        );
        // A rig-menu rate already configured → 115200 still gets tried (last).
        assert_eq!(
            ladder_bauds(19200),
            vec![19200, 9600, 4800, 38400, 57600, 115200]
        );
        // A configured rate from the middle of the set is never probed twice.
        assert_eq!(
            ladder_bauds(38400),
            vec![38400, 19200, 9600, 4800, 57600, 115200]
        );
    }

    #[test]
    fn every_civ_usb_baud_rate_menu_value_is_on_the_ladder() {
        // The rig menu the verdicts name ("CI-V USB Baud Rate") offers exactly these
        // fixed rates; any one missing here walks the ladder silent and gets the
        // wrong-COM-port verdict instead of the baud-mismatch one.
        for menu_rate in [4800u32, 9600, 19200, 38400, 57600, 115200] {
            assert!(
                LADDER_BAUDS.contains(&menu_rate),
                "{menu_rate} is a CI-V USB Baud Rate menu value but not on the ladder"
            );
        }
    }

    /// ⭐ FAILING-FIRST for B: the ladder has to reach the rigs whose baud entries were
    /// deleted, and today it reaches NONE of them.
    ///
    /// `icom_civ_addr` is `rigmodels::icom_scope_model(..).map(..)`, and that function answers
    /// a DIFFERENT question — "does this radio have a native panadapter?" — for exactly five
    /// models. Every one of the twenty-one rigs whose hand-transcribed rate rows are being
    /// deleted falls outside it, so `ladder_applies` returns `None` and the operator gets
    /// nothing at all where the entry used to (wrongly) guess for them.
    #[test]
    fn the_ladder_reaches_every_delisted_rig_not_just_the_five_native_scope_icoms() {
        // The eleven that DO speak CI-V — seven older Icoms and four Xiegus. They are absent
        // from `icom_scope_model` only because none of them has a native scope.
        for model in [
            3013u32, 3023, 3046, 3057, 3044, 3060, 3070, 3076, 3087, 3089, 3091,
        ] {
            assert!(
                ladder_applies(false, model, "COM4", "cat", "").is_some(),
                "model {model} speaks CI-V and lost its baud row — it needs the ladder"
            );
        }
        // The ten that do not speak CI-V at all: the FT-847 and nine Kenwoods.
        for model in [
            1001u32, 2001, 2002, 2003, 2004, 2005, 2010, 2011, 2016, 2025,
        ] {
            assert!(
                ladder_applies(false, model, "COM4", "cat", "").is_some(),
                "model {model} lost its baud row and has no CI-V — it needs the ladder"
            );
        }
    }

    /// The gate is about the PORT, not the model — the model only has to be set at all.
    #[test]
    fn ladder_applies_to_any_rig_on_a_real_serial_port() {
        let cat = |m| ladder_applies(false, m, "COM4", "cat", "");
        assert_eq!(cat(3078), Some(LadderGate { keying: None })); // IC-7610
        assert_eq!(cat(1042), Some(LadderGate { keying: None })); // FTDX-10 — no CI-V, still laddered
        assert_eq!(ladder_applies(true, 3078, "COM4", "cat", ""), None); // network rig
        assert_eq!(cat(0), None); // no model picked
        assert_eq!(ladder_applies(false, 3078, "  ", "cat", ""), None); // no port
    }

    #[test]
    fn the_ladder_only_runs_when_the_failed_probe_was_a_cat_probe() {
        let none = Some(LadderGate { keying: None });
        // "cat" and "vox" probe the CAT channel → ladder applies, nothing keys the port.
        assert_eq!(ladder_applies(false, 3078, "COM4", "cat", ""), none);
        assert_eq!(ladder_applies(false, 3078, "COM4", "vox", ""), none);
        // Dedicated-port RTS/DTR keying: reprobe tested only the PTT line — its failure
        // says nothing about the CAT port, and a ladder run would bury the real
        // "Could not open serial port COM5" error under a bogus backend verdict.
        assert_eq!(ladder_applies(false, 3078, "COM4", "rts", "COM5"), None);
        assert_eq!(ladder_applies(false, 3078, "COM4", "dtr", "COM5"), None);
        // Shared-port keying (dedicated port empty, or equal ignoring case): rigctld owns
        // the CAT port and the reprobe DID probe it → ladder applies — and the gate must
        // carry WHICH line keys it, or a rung would open the port and leave the rig keyed.
        let rts = Some(LadderGate {
            keying: Some(crate::rig::SerialLine::Rts),
        });
        let dtr = Some(LadderGate {
            keying: Some(crate::rig::SerialLine::Dtr),
        });
        assert_eq!(ladder_applies(false, 3078, "COM4", "rts", ""), rts);
        assert_eq!(ladder_applies(false, 3078, "COM4", "dtr", "com4"), dtr);
        assert_eq!(ladder_applies(false, 3078, "COM4", "rts", " COM4 "), rts);
    }

    // ---- B1: the CI-V address, asked of Hamlib instead of tabulated ----

    /// The eleven CI-V rigs the five-model lookup excluded are covered by ASKING, and the
    /// captures are real `rigctld -m <n> -L` output from the bundled 4.7.1.
    #[test]
    fn hamlib_hands_us_the_civ_address_and_says_nothing_for_a_rig_without_one() {
        // IC-746 (3023) — one of the seven older Icoms whose transcribed rate row was deleted,
        // and the row that was factually WRONG. Hamlib prints 86 = 0x56.
        let ic746 = include_str!("../tests/fixtures/rigctld/showconf_ic746.log");
        assert_eq!(parse_civ_addr(ic746), Some(0x56));
        // FT-847 — no CI-V at all, so no `civaddr` parameter: it must fall to the Hamlib ladder
        // rather than be probed at some invented address.
        let ft847 = include_str!("../tests/fixtures/rigctld/showconf_ft847.log");
        assert_eq!(parse_civ_addr(ft847), None);
        // A dump we cannot read claims nothing.
        assert_eq!(parse_civ_addr(""), None);
        // Address 0 is the frontend's "unset", never a rig.
        assert_eq!(
            parse_civ_addr("civaddr: \"Transceiver's CI-V address\"\n\tDefault: 0, Value: 0\n"),
            None
        );
    }

    // ---- B2: what Hamlib's caps tell the ladder ----

    #[test]
    fn caps_give_the_rate_bounds_and_the_coverage_the_ladder_judges_by() {
        let ft847 = parse_caps(include_str!("../tests/fixtures/rigctld/caps_ft847.log"));
        assert_eq!(ft847.serial_rates, Some((4800, 57600)));
        // The FT-847 hears HF, 6 m/4 m, air+2 m and 70 cm — and NOT 76–108 MHz.
        assert!(ft847.covers(14_074_000), "20 m");
        assert!(ft847.covers(145_000_000), "2 m");
        assert!(
            !ft847.covers(101_054_360),
            "the measured garbage frequency is in its RX gap"
        );
        assert!(
            !ft847.covers(1_054_365_010),
            "round four's garbage frequency, far above it"
        );
        let ts570 = parse_caps(include_str!("../tests/fixtures/rigctld/caps_ts570d.log"));
        assert_eq!(ts570.serial_rates, Some((1200, 57600)));
        assert!(ts570.covers(7_074_000));
        assert!(
            !ts570.covers(145_000_000),
            "an HF-only rig cannot be on 2 m"
        );
        // An unreadable dump must not turn every rung into a rejection.
        let unknown = parse_caps("");
        assert_eq!(unknown.serial_rates, None);
        assert!(
            unknown.covers(101_054_360),
            "no coverage = no opinion, not 'no'"
        );
    }

    #[test]
    fn the_rungs_are_bounded_by_what_the_backend_says_it_can_drive() {
        // FT-847: 4800..57600 — 115200 and 1200 are not rates Hamlib would use, so probing
        // them only spends the operator's time.
        assert_eq!(
            hamlib_ladder_bauds(38400, Some((4800, 57600))),
            vec![38400, 9600, 4800, 57600, 19200]
        );
        // A fixed-rate rig is a ONE-rung ladder (2053 FX-4 prints 115200..115200).
        assert_eq!(
            hamlib_ladder_bauds(115200, Some((115200, 115200))),
            vec![115200]
        );
        // The configured rate is probed even when it is outside the bounds — that is a real
        // misconfiguration to diagnose, not one to skip.
        assert_eq!(hamlib_ladder_bauds(115200, Some((4800, 57600)))[0], 115200);
        // No caps → the full most-likely-first order, configured first, never twice.
        assert_eq!(
            hamlib_ladder_bauds(19200, None),
            vec![19200, 9600, 4800, 57600, 38400, 115200, 1200]
        );
    }

    // ---- B2: what proves a rate, and what must not ----

    /// ⭐ THE GARBAGE CASE, from the real bundled rigctl driving the real `ft847` backend.
    ///
    /// These three stdout shapes are what it printed, verbatim (a stand-in rig on the network
    /// transport, which is how the existing `never_answers_*` captures were made too).
    #[test]
    fn a_number_is_not_proof_and_the_two_tests_together_reject_what_it_accepts() {
        let caps = parse_caps(include_str!("../tests/fixtures/rigctld/caps_ft847.log"));
        // A HEALTHY rig: frequency in coverage, mode named.
        let healthy = parse_rigctl_read("14074000\nUSB\n2200\n");
        assert_eq!(healthy.freq_hz, Some(14_074_000));
        assert_eq!(healthy.mode, ModeRead::Named("USB".into()));
        assert_eq!(
            classify_hamlib_probe(&healthy, &caps),
            BaudProbe::Reply {
                freq_hz: Some(14_074_000)
            }
        );
        // MIS-CLOCKED BYTES, as measured: a number with no error, and an empty mode over a
        // zero passband. `f` alone would have called this a working rate.
        let garbage = parse_rigctl_read("101054360\n\n0\n");
        assert_eq!(
            garbage.freq_hz,
            Some(101_054_360),
            "it DID parse — that is the trap"
        );
        assert_eq!(garbage.mode, ModeRead::Unnamed);
        assert_eq!(
            classify_hamlib_probe(&garbage, &caps),
            BaudProbe::Noise,
            "a rig cannot be tuned where it cannot receive, and Hamlib could not name the mode"
        );
        // Either test alone rejects it, and each on its own grounds — measured leak rates
        // over 30 random garbage streams were 8/30 (coverage) and 2/30 (mode), 0/30 together.
        assert_eq!(
            classify_hamlib_probe(
                &RigctlRead {
                    freq_hz: Some(101_054_360),
                    mode: ModeRead::Named("USB".into())
                },
                &caps
            ),
            BaudProbe::Noise,
            "coverage alone must still reject it"
        );
        assert_eq!(
            classify_hamlib_probe(
                &RigctlRead {
                    freq_hz: Some(14_074_000),
                    mode: ModeRead::Unnamed
                },
                &caps
            ),
            BaudProbe::Noise,
            "the mode veto alone must still reject it"
        );
        // Nothing rigctl would call a frequency = the rate got no answer.
        assert_eq!(
            classify_hamlib_probe(&parse_rigctl_read(""), &caps),
            BaudProbe::Silence
        );
    }

    /// A mode that could not be READ is not evidence of anything — only a mode Hamlib decoded
    /// and could not NAME is. Requiring one would report "your rig never answered" to an
    /// operator whose rig answered.
    #[test]
    fn a_mode_that_could_not_be_read_never_counts_against_a_rate() {
        let caps = parse_caps(include_str!("../tests/fixtures/rigctld/caps_ts570d.log"));
        // Measured: a Kenwood answering `f` while `m` fails prints the value, then an error
        // blob. That is ModeRead::Absent, and the rate is still proved by coverage.
        let read = parse_rigctl_read(
            "14074000\nerror = rig_get_mode(3132): freqMainB=0, modeMainB=, widthMainB=0\n",
        );
        assert_eq!(read.mode, ModeRead::Absent);
        assert_eq!(
            classify_hamlib_probe(&read, &caps),
            BaudProbe::Reply {
                freq_hz: Some(14_074_000)
            }
        );
        // The rung probe asks `f` alone, so its reads carry no mode at all.
        assert_eq!(parse_rigctl_read("14074000\n").mode, ModeRead::Absent);
    }

    /// rigctl prints the value FIRST and an `error =` blob instead when a command fails — and
    /// that blob contains bare numbers (a passband is one). Scanning for "the first line that
    /// parses" would read one of them as the rig's frequency.
    #[test]
    fn a_failure_blob_is_never_mistaken_for_a_frequency() {
        let mute = "error = rig_get_freq: cache miss age=10006ms, cached_vfo=Main\n\
                    read_block_generic(): Timed out 1.27710 seconds after 0 chars, direct=1\n\
                    ft847: read_block returned -5\n\
                    rig_get_freq(2674): freqMainA=0, modeMainA=, widthMainA=0\n\
                    2200\n\
                    Communication timed out\n";
        assert_eq!(parse_rigctl_read(mute).freq_hz, None);
        // Nor is a zero.
        assert_eq!(parse_rigctl_read("0\n\n0\n").freq_hz, None);
    }

    #[test]
    fn a_port_that_will_not_open_is_reported_as_such_not_as_silence() {
        assert_eq!(
            open_failure_line("error = serial_open: serial port COM7 is already open\n").as_deref(),
            Some("serial_open: serial port COM7 is already open")
        );
        assert!(open_failure_line("14074000\nUSB\n2200\n").is_none());
    }

    /// ⭐ FAILING-FIRST for D1, half one: the `-vvv` banner.
    ///
    /// `probe_args` has to pass `-vvv` or a port another program is holding prints NOTHING and
    /// comes back as silence (the other half, below). At `-vvv` rigctl opens stdout with
    /// `Opened rig model 1001, 'FT-847'` — so a one-line window reads the banner as the answer
    /// and **every healthy rung becomes [`BaudProbe::Silence`]**. The two halves are one change;
    /// this is the test that fails if only the flag lands.
    ///
    /// The capture is verbatim `rigctl.exe -vvv -m 1001 -r <stand-in> -s 9600 f m`, bundled
    /// 4.7.1.
    #[test]
    fn the_vvv_banner_never_becomes_the_answer_and_never_hides_it() {
        let caps = parse_caps(include_str!("../tests/fixtures/rigctld/caps_ft847.log"));
        let banner = include_str!("../tests/fixtures/rigctld/probe_working_ft847.stdout.log");
        assert!(
            banner
                .lines()
                .next()
                .unwrap()
                .starts_with("Opened rig model"),
            "the capture must actually carry the banner or this proves nothing: {banner:?}"
        );
        let read = parse_rigctl_read(banner);
        assert_eq!(
            read.freq_hz,
            Some(14_074_000),
            "behind the banner: {banner:?}"
        );
        assert_eq!(read.mode, ModeRead::Named("USB".into()));
        assert_eq!(
            classify_hamlib_probe(&read, &caps),
            BaudProbe::Reply {
                freq_hz: Some(14_074_000)
            },
            "a healthy rung must stay a hit once the flag is on"
        );
        // A rig that opens and says nothing still reads as silence THROUGH the banner: the line
        // under it is the `error = …` blob, not a number.
        let mute = include_str!("../tests/fixtures/rigctld/probe_mute_ft847.stdout.log");
        assert_eq!(parse_rigctl_read(mute).freq_hz, None, "{mute}");
        assert_eq!(
            classify_hamlib_probe(&parse_rigctl_read(mute), &caps),
            BaudProbe::Silence
        );
        // The window is TWO lines, never three: the skip may step over a banner, never into a
        // blob. `2200` here is `m`'s passband three lines down — the shape that made the
        // one-line rule right in the first place.
        assert_eq!(
            parse_rigctl_read("Opened rig model 1001, 'FT-847'\nnot a number\n2200\n").freq_hz,
            None
        );
        // And the un-bannered shape still parses, so this is not a rule about position 2.
        assert_eq!(
            parse_rigctl_read("14074000\nUSB\n2200\n").freq_hz,
            Some(14_074_000)
        );
    }

    /// ⭐ FAILING-FIRST for D1, half two: WHICH STREAM the open failure is on.
    ///
    /// All four captures are verbatim from the bundled rigctl 4.7.1 at `-vvv`; the port-busy one
    /// is a Windows named pipe whose single instance was held by another process, which is the
    /// same `ERROR_*` path a COM port held by WSJT-X takes.
    ///
    /// The point of the test is the **separation**: an open failure appears on stderr and only
    /// stderr, and a rig that merely stayed mute puts nothing there that could be mistaken for
    /// one — including a line that says "failed" for an unrelated reason.
    #[test]
    fn the_four_states_a_rung_can_be_in_separate_on_the_streams_rigctl_prints_them_on() {
        let missing = include_str!("../tests/fixtures/rigctld/probe_missing_port.stderr.log");
        let busy = include_str!("../tests/fixtures/rigctld/probe_port_busy.stderr.log");
        let mute_err = include_str!("../tests/fixtures/rigctld/probe_mute_ft847.stderr.log");
        let mute_out = include_str!("../tests/fixtures/rigctld/probe_mute_ft847.stdout.log");
        let work_out = include_str!("../tests/fixtures/rigctld/probe_working_ft847.stdout.log");
        // 1. The port is not there.
        assert_eq!(
            open_failure_line(missing).as_deref(),
            Some("serial_open: serial port COM99 does not exist")
        );
        // 2. Another program is holding it — a different fault with a different cure.
        assert_eq!(
            open_failure_line(busy).as_deref(),
            Some("serial_open: serial port \\\\.\\pipe\\nexuscom is already open")
        );
        // 3. The port opened and the rig never answered. Nothing on stderr may read as an open
        //    failure — and this capture DOES contain the word "failed", on a line about an
        //    unrelated connect retry, which is exactly the impersonation the fragments must not
        //    fall for.
        assert!(mute_err.contains("failed"), "precondition: {mute_err}");
        assert_eq!(open_failure_line(mute_err), None, "{mute_err}");
        // …and stdout could never have carried it, which is why folding the streams together
        // could not have worked even with the flag on.
        assert_eq!(open_failure_line(mute_out), None, "{mute_out}");
        // 4. The rig answered.
        assert_eq!(open_failure_line(work_out), None, "{work_out}");
        assert_eq!(
            parse_rigctl_read(work_out).freq_hz,
            Some(14_074_000),
            "{work_out}"
        );
    }

    /// ⭐ FAILING-FIRST for D1 + D2 on the one line that produces all of it: the argument vector.
    ///
    /// Two properties, and each is a measured defect:
    /// - `-vvv`, or Hamlib prints nothing at all for a port it cannot open (rc 2, zero bytes on
    ///   both streams) and a busy port is reported as silence;
    /// - no `--set-conf`, because the probe is issuing a verdict about the daemon and
    ///   [`crate::rigctld_proc::rigctld_args`] gives the daemon none — every narrowing here is a
    ///   rung the daemon might have read being called silent.
    #[test]
    #[cfg(feature = "serial")]
    fn the_probe_speaks_and_never_narrows_the_budget_the_daemon_itself_runs_at() {
        let args = probe_args("COM4", 9600, 1001, None, &["f"]);
        assert!(
            args.iter().any(|a| a == "-vvv"),
            "without it a port that cannot be opened prints nothing at all: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a.starts_with("--set-conf")),
            "a probe stricter than the daemon it is judging invents silence: {args:?}"
        );
        // The daemon's own line carries no --set-conf either; that is the whole standard.
        let daemon = crate::rigctld_proc::rigctld_args(
            1001,
            "COM4",
            9600,
            4532,
            false,
            None,
            crate::rigctld_proc::ControlLines::default(),
        );
        assert!(
            !daemon.iter().any(|a| a.starts_with("--set-conf")),
            "if the daemon ever narrows its budget, the probe must follow it there: {daemon:?}"
        );
        // The rest of the line is unchanged, and the keying override is still on EVERY rung —
        // without it a serial driver's power-on-HIGH RTS makes a sweep five transmissions.
        assert_eq!(
            args,
            ["-vvv", "-m", "1001", "-r", "COM4", "-s", "9600", "f"]
        );
        assert_eq!(
            probe_args(
                "COM4",
                4800,
                2004,
                Some(crate::rig::SerialLine::Rts),
                &["f", "m"]
            ),
            [
                "-vvv", "-m", "2004", "-r", "COM4", "-s", "4800", "-P", "RTS", "-p", "COM4", "f",
                "m"
            ]
        );
    }

    // ---- B2: the sweep ----

    #[test]
    fn the_sweep_records_what_it_never_got_to_rather_than_calling_it_silence() {
        // A budget that expires after the first rung: the rest were never tested, and the
        // verdict must not describe them as having been silent.
        let mut probes = 0;
        let r = run_ladder_over(
            "COM4",
            38400,
            vec![38400, 9600, 4800, 57600],
            || true,
            |_| {
                probes += 1;
                BaudProbe::Silence
            },
        );
        assert_eq!(
            probes, 1,
            "the budget may decline a rung, never interrupt one"
        );
        assert_eq!(r.outcomes.len(), 1);
        assert_eq!(r.not_tried, vec![9600, 4800, 57600]);
        let m = compose_hamlib_ladder_message(&r, "Yaesu FT-847");
        assert!(m.contains("ran out of time"), "{m}");
        assert!(m.contains("9600, 4800, 57600"), "name what is left: {m}");
        assert!(
            !m.contains("never answered"),
            "an untested rate is not a silent one: {m}"
        );
        // A sweep that finishes leaves nothing untried.
        let r = run_ladder_over(
            "COM4",
            38400,
            vec![38400, 9600],
            || false,
            |_| BaudProbe::Silence,
        );
        assert!(r.not_tried.is_empty());
    }

    #[test]
    fn the_hamlib_verdict_names_the_rate_and_both_cures_without_inventing_a_rig_menu() {
        // THE R1 CASE: an FT-847 actually running at 57600, configured to 4800.
        let r = LadderReport {
            port: "COM4".into(),
            configured_baud: 4800,
            not_tried: Vec::new(),
            outcomes: vec![
                (4800, BaudProbe::Silence),
                (9600, BaudProbe::Silence),
                (
                    57600,
                    BaudProbe::Reply {
                        freq_hz: Some(14_074_000),
                    },
                ),
            ],
        };
        let m = compose_hamlib_ladder_message(&r, "Yaesu FT-847");
        assert!(m.contains("57600"), "name the answering rate: {m}");
        assert!(m.contains("14.074"), "show what it read: {m}");
        assert!(m.contains("Baud"), "cure 1 — change it here: {m}");
        assert!(m.contains("radio's own CAT/serial menu"), "cure 2: {m}");
        assert!(
            !m.contains("MENU »") && !m.contains("CI-V"),
            "these ten rigs have ten different menus — quoting one is the transcription \
             mistake this feature exists to stop: {m}"
        );
        // Answering at the CONFIGURED rate means the link is fine and the daemon fell over.
        let r = LadderReport {
            port: "COM4".into(),
            configured_baud: 9600,
            not_tried: Vec::new(),
            outcomes: vec![(
                9600,
                BaudProbe::Reply {
                    freq_hz: Some(7_074_000),
                },
            )],
        };
        let m = compose_hamlib_ladder_message(&r, "Kenwood TS-570D");
        assert!(m.contains("rigctld"), "{m}");
        assert!(m.contains("Save the settings again"), "{m}");
        // A busy port is its own fault with its own cure, never a baud verdict.
        let r = LadderReport {
            port: "COM4".into(),
            configured_baud: 9600,
            not_tried: Vec::new(),
            outcomes: vec![(
                9600,
                BaudProbe::OpenFailed("serial_open: serial port COM4 is already open".into()),
            )],
        };
        let m = compose_hamlib_ladder_message(&r, "Kenwood TS-570D");
        assert!(m.contains("already open") && m.contains("WSJT-X"), "{m}");
        // Bytes that decode as nothing the rig could be doing.
        let r = LadderReport {
            port: "COM4".into(),
            configured_baud: 9600,
            not_tried: Vec::new(),
            outcomes: vec![(9600, BaudProbe::Noise), (4800, BaudProbe::Silence)],
        };
        let m = compose_hamlib_ladder_message(&r, "Yaesu FT-847");
        assert!(m.contains("Bytes DID come back"), "{m}");
        // Total silence: the port identity check, and no Icom-only advice.
        let r = LadderReport {
            port: "COM4".into(),
            configured_baud: 9600,
            not_tried: Vec::new(),
            outcomes: vec![(9600, BaudProbe::Silence), (4800, BaudProbe::Silence)],
        };
        let m = compose_hamlib_ladder_message(&r, "Yaesu FT-847");
        assert!(m.contains("unplug"), "{m}");
        assert!(
            !m.contains("TWO COM ports"),
            "that is Icom-only advice: {m}"
        );
    }

    #[test]
    fn only_the_dual_uart_icoms_get_two_port_advice() {
        // Must stay in step with the UI's [3078, 3081] dual-port hint (SettingsPanel).
        assert!(dual_com_ports(3078)); // IC-7610 — CP2105 dual UART
        assert!(dual_com_ports(3081)); // IC-9700 — CP2105 dual UART
        assert!(!dual_com_ports(3073)); // IC-7300 — single port
        assert!(!dual_com_ports(3085)); // IC-705 — single port
        assert!(!dual_com_ports(3090)); // IC-905 — single port
        assert!(!dual_com_ports(1042)); // non-Icom
        assert!(!dual_com_ports(0));
    }

    // ---- reply classification ----

    #[test]
    fn a_freq_reply_frame_classifies_as_reply_with_the_frequency() {
        // IC-7610 answering read_freq: FE FE E0 98 03 <BCD 14.074.000> FD
        let mut f = Frame::command(0x98, 0x03, &crate::civ::frame::freq_to_bcd(14_074_000));
        (f.to, f.from) = (crate::civ::frame::CONTROLLER, 0x98);
        assert_eq!(
            classify_probe_bytes(&f.to_bytes()),
            BaudProbe::Reply {
                freq_hz: Some(14_074_000)
            }
        );
    }

    #[test]
    fn a_transceive_broadcast_also_proves_life_and_carries_the_freq() {
        // CI-V Transceive ON: the rig broadcasts `00` frames to address 00 as the dial
        // moves — proof of life at this rate even if our own query got no direct answer.
        let mut f = Frame::command(0x00, 0x00, &crate::civ::frame::freq_to_bcd(7_074_000));
        (f.to, f.from) = (0x00, 0x98);
        assert_eq!(
            classify_probe_bytes(&f.to_bytes()),
            BaudProbe::Reply {
                freq_hz: Some(7_074_000)
            }
        );
    }

    #[test]
    fn our_own_echo_is_not_a_rig_reply() {
        // USB Echo Back ON echoes the query verbatim (from == CONTROLLER). Bytes arrived
        // but the rig said nothing — that must NOT read as a working link.
        let echo = crate::civ::commands::read_freq(0x98).to_bytes();
        assert_eq!(classify_probe_bytes(&echo), BaudProbe::Noise);
    }

    #[test]
    fn garbage_is_noise_and_nothing_is_silence() {
        assert_eq!(classify_probe_bytes(&[0x55, 0xAA, 0x00]), BaudProbe::Noise);
        assert_eq!(classify_probe_bytes(&[]), BaudProbe::Silence);
    }

    #[test]
    fn an_ack_frame_with_no_freq_still_proves_life() {
        let mut f = Frame::command(0x98, crate::civ::frame::OK, &[]);
        (f.to, f.from) = (crate::civ::frame::CONTROLLER, 0x98);
        assert_eq!(
            classify_probe_bytes(&f.to_bytes()),
            BaudProbe::Reply { freq_hz: None }
        );
    }

    // ---- ladder orchestration ----

    #[test]
    fn the_ladder_stops_at_the_first_rate_that_replies() {
        let mut tried = Vec::new();
        let r = run_ladder("COM4", 115200, |baud| {
            tried.push(baud);
            if baud == 19200 {
                BaudProbe::Reply {
                    freq_hz: Some(14_074_000),
                }
            } else {
                BaudProbe::Silence
            }
        });
        assert_eq!(tried, vec![115200, 19200], "9600/4800 must not be probed");
        assert_eq!(r.outcomes.len(), 2);
        assert_eq!(r.outcomes[0], (115200, BaudProbe::Silence));
        assert_eq!(
            r.outcomes[1],
            (
                19200,
                BaudProbe::Reply {
                    freq_hz: Some(14_074_000)
                }
            )
        );
    }

    #[test]
    fn a_totally_silent_port_walks_every_rate() {
        let r = run_ladder("COM4", 115200, |_| BaudProbe::Silence);
        assert_eq!(
            r.outcomes.iter().map(|(b, _)| *b).collect::<Vec<_>>(),
            vec![115200, 19200, 9600, 4800, 38400, 57600]
        );
    }

    // ---- message composition ----

    fn report(configured: u32, outcomes: Vec<(u32, BaudProbe)>) -> LadderReport {
        LadderReport {
            port: "COM4".into(),
            configured_baud: configured,
            outcomes,
            not_tried: Vec::new(),
        }
    }

    #[test]
    fn a_hit_at_another_rate_names_both_fixes_and_the_exact_rig_menu() {
        // THE field case: configured 115200, rig still linked to [REMOTE] → answers at 19200.
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::Silence),
                (
                    19200,
                    BaudProbe::Reply {
                        freq_hz: Some(14_074_000),
                    },
                ),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, true, true, false);
        assert!(m.contains("COM4"), "{m}");
        assert!(m.contains("19200"), "must name the answering rate: {m}");
        assert!(m.contains("14.074"), "must show the read frequency: {m}");
        // Option 1: fix the app side.
        assert!(m.contains("Baud"), "{m}");
        // Option 2: fix the rig side, with the exact menu path.
        assert!(m.contains("MENU"), "{m}");
        assert!(m.contains("CI-V USB Baud Rate"), "{m}");
        assert!(m.contains("Unlink from [REMOTE]"), "{m}");
        // And WHY 115200 was silent (the linked-port ceiling) — only relevant > 19200.
        assert!(m.contains("Link to [REMOTE]"), "{m}");
    }

    #[test]
    fn a_hit_with_a_slow_configured_rate_skips_the_remote_link_explanation() {
        let r = report(
            9600,
            vec![
                (9600, BaudProbe::Silence),
                (19200, BaudProbe::Reply { freq_hz: None }),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true, false);
        assert!(m.contains("19200"), "{m}");
        assert!(
            !m.contains("Link to [REMOTE]"),
            "the linked-port ceiling can't explain a silent 9600: {m}"
        );
    }

    #[test]
    fn total_silence_gives_the_two_port_identity_walkthrough() {
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::Silence),
                (19200, BaudProbe::Silence),
                (9600, BaudProbe::Silence),
                (4800, BaudProbe::Silence),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true, false);
        assert!(m.contains("Icom IC-7610"), "{m}");
        assert!(
            m.contains("115200") && m.contains("4800"),
            "list rates: {m}"
        );
        assert!(m.contains("TWO COM ports"), "{m}");
        // How to tell them apart on Windows (Enhanced = CI-V side, Standard = never).
        assert!(m.contains("Enhanced"), "{m}");
        assert!(m.contains("Standard"), "{m}");
        // The remaining silent killers: power, a changed CI-V address, a missing driver.
        assert!(m.contains("CI-V Address"), "{m}");
        assert!(m.contains("98h"), "must name the expected address: {m}");
        assert!(m.contains("driver"), "{m}");
    }

    #[test]
    fn total_silence_on_a_single_port_icom_never_sends_the_operator_port_hunting() {
        // An IC-7300 (single CP2102 UART) with a dead cable/driver: there IS no second
        // COM port, so the dual-port walkthrough would be a wild-goose chase. The verdict
        // must instead verify the port's identity (unplug test) and keep the real silent
        // killers: power, CI-V address, driver.
        let r = LadderReport {
            port: "COM4".into(),
            configured_baud: 115200,
            not_tried: Vec::new(),
            outcomes: vec![
                (115200, BaudProbe::Silence),
                (19200, BaudProbe::Silence),
                (9600, BaudProbe::Silence),
                (4800, BaudProbe::Silence),
                (38400, BaudProbe::Silence),
                (57600, BaudProbe::Silence),
            ],
        };
        let m = compose_ladder_message(&r, "Icom IC-7300", 0x94, false, false, false);
        assert!(!m.contains("TWO COM ports"), "{m}");
        assert!(!m.contains("Enhanced"), "{m}");
        assert!(!m.contains("other COM port"), "{m}");
        assert!(m.contains("single COM port"), "{m}");
        assert!(
            m.contains("unplug"),
            "identity check via the unplug test: {m}"
        );
        assert!(m.contains("CI-V Address"), "{m}");
        assert!(m.contains("94h"), "must name the 7300's address: {m}");
        assert!(m.contains("driver"), "{m}");
    }

    /// ⭐ FAILING-FIRST for the mac prose split. Every no-answer arm walked a Mac operator
    /// through WINDOWS: "Windows Device Manager (Ports)", the CP210x "Enhanced" driver label,
    /// and "install Icom's USB driver" — a driver macOS ships in-kernel and Icom does not
    /// publish for the platform (mac QA audit, 2026-08-17). Platform as data, both branches
    /// runnable on any box, exactly like `rigctld_launch_failed_for`.
    #[test]
    fn the_mac_verdicts_speak_dev_cu_never_device_manager() {
        let silent = |bauds: &[u32]| LadderReport {
            port: "/dev/cu.SLAB_USBtoUART".into(),
            configured_baud: 115200,
            not_tried: Vec::new(),
            outcomes: bauds.iter().map(|b| (*b, BaudProbe::Silence)).collect(),
        };
        // Dual-UART Icom (IC-7610): the tie-break is /dev/cu.* name order, not a driver label.
        let m = compose_ladder_message(
            &silent(&[115200, 19200]),
            "Icom IC-7610",
            0x98,
            false,
            true,
            true,
        );
        assert!(m.contains("TWO /dev/cu.* ports"), "{m}");
        assert!(m.contains("cu.SLAB_USBtoUART"), "{m}");
        assert!(!m.contains("Device Manager"), "{m}");
        assert!(
            !m.contains("Enhanced"),
            "that is a Windows driver label: {m}"
        );
        assert!(
            !m.contains("install Icom's USB driver"),
            "macOS ships the driver in-kernel and Icom publishes no Mac driver: {m}"
        );
        assert!(m.contains("System Information"), "{m}");
        assert!(m.contains("CI-V Address") && m.contains("98h"), "{m}");
        // Single-port Icom (IC-7300): the unplug test names "the port list", no Device Manager.
        let m = compose_ladder_message(
            &silent(&[115200, 19200, 9600]),
            "Icom IC-7300",
            0x94,
            false,
            false,
            true,
        );
        assert!(m.contains("the port list"), "{m}");
        assert!(m.contains("unplug"), "{m}");
        assert!(!m.contains("Device Manager"), "{m}");
        assert!(!m.contains("COM"), "COM is Windows vocabulary: {m}");
        // The noise sentence drops the COM vocabulary on a Mac too.
        let mut r = silent(&[115200, 19200]);
        r.outcomes[0].1 = BaudProbe::Noise;
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true, true);
        assert!(m.contains("not valid CI-V"), "{m}");
        assert!(!m.contains("COM port"), "{m}");
        // And the Windows branch is UNCHANGED by the split — same walkthrough as ever.
        let m =
            compose_ladder_message(&silent(&[115200]), "Icom IC-7610", 0x98, false, true, false);
        assert!(m.contains("Windows Device Manager (Ports)"), "{m}");
        assert!(m.contains("Enhanced"), "{m}");
        assert!(m.contains("install Icom's USB driver"), "{m}");
    }

    /// ⭐ FAILING-FIRST for the missing-prober overwrite (mac QA audit, 2026-08-17). On a Mac
    /// without Homebrew Hamlib the CAT open already says "brew install hamlib" — and then Test
    /// CAT's ladder spawn-failed rigctl (ENOENT), mapped it into `OpenFailed`, and the verdict
    /// blamed the serial port ("close other CAT/logging software … and test again"), replacing
    /// the correct diagnosis last-writer-wins. A missing prober must out-rank every guess
    /// about hardware it never probed, and the sweep must stop at the first spawn failure
    /// rather than decorate the wrong story with more rungs.
    #[test]
    fn a_missing_rigctl_is_named_and_never_dressed_up_as_a_busy_port() {
        // The sweep stops at the first ProberMissing — the tool is gone, not the rate.
        let mut probes = 0;
        let cure = "Hamlib's rigctl isn't installed. In Terminal: brew install hamlib, then \
                    restart Nexus. (No such file or directory (os error 2))";
        let r = run_ladder_over(
            "/dev/cu.usbserial-1420",
            38400,
            vec![38400, 9600, 4800],
            || false,
            |_| {
                probes += 1;
                BaudProbe::ProberMissing(cure.to_string())
            },
        );
        assert_eq!(probes, 1, "every further rung fails identically");
        assert_eq!(r.outcomes.len(), 1);
        // The verdict is the carried install cure, verbatim — never a port guess.
        let m = compose_hamlib_ladder_message(&r, "Yaesu FT-847");
        assert_eq!(m, cure);
        assert!(!m.contains("WSJT-X"), "{m}");
        assert!(!m.contains("close other"), "{m}");
        assert!(
            !m.contains("never answered"),
            "no port was ever probed: {m}"
        );
    }

    /// ⭐ FAILING-FIRST for the rig-family split. [`ladder_kind`] was broadened from a
    /// five-Icom lookup to "any rig that publishes a `civaddr`", which brought the four
    /// X-series Xiegus onto this verdict — and the verdict was not broadened with it. It
    /// quoted `MENU » SET » Connectors » CI-V` (a menu no Xiegu has), told a REMOTE-jack
    /// story about a jack no Xiegu has, and closed by asking the owner of a CP210x/CH34x
    /// bridge to install Icom's USB driver. Half the verdict was right and the other half
    /// sent the operator to the radio for twenty minutes.
    ///
    /// Both platform arms are asserted for both families: the mac/Windows split (mac QA
    /// audit, 2026-08-17) is the pattern this rides on, not a thing it replaces.
    #[test]
    fn a_xiegu_verdict_never_speaks_icom() {
        let hit = report(
            38400,
            vec![
                (38400, BaudProbe::Silence),
                (
                    19200,
                    BaudProbe::Reply {
                        freq_hz: Some(14_074_000),
                    },
                ),
            ],
        );
        // The rate hit: name the fix that IS actionable (Settings), and nothing that is not.
        // 38400 is the app default, so this is exactly the fresh-install Xiegu case.
        for mac in [false, true] {
            let m = compose_ladder_message(&hit, "Xiegu X6100", 0xA4, false, false, mac);
            assert!(m.contains("19200"), "{m}");
            assert!(m.contains("Baud"), "the Settings-side cure survives: {m}");
            assert!(!m.contains("MENU"), "no Xiegu has that menu: {m}");
            assert!(!m.contains("CI-V USB Baud Rate"), "{m}");
            assert!(!m.contains("REMOTE"), "no Xiegu has that jack: {m}");
        }
        // Total silence: the port-identity walkthrough and the closing checks.
        let silent = |port: &str| LadderReport {
            port: port.into(),
            configured_baud: 19200,
            not_tried: Vec::new(),
            outcomes: LADDER_BAUDS
                .iter()
                .map(|b| (*b, BaudProbe::Silence))
                .collect(),
        };
        // X6100/X6200 — the CH342 pair, and CAT is on the SERIAL-B one (the INVERSE of the
        // Icom pair, so the Icom walkthrough is not merely off-brand here, it is backwards).
        let m = compose_ladder_message(&silent("COM6"), "Xiegu X6100", 0xA4, false, false, false);
        assert!(m.contains("TWO"), "the X6100 does show a pair: {m}");
        assert!(m.contains("SERIAL-B"), "and CAT is on the B one: {m}");
        // The CP2105 tie-break labels belong to the Icom pair, not this one.
        assert!(!m.contains("Standard"), "{m}");
        assert!(!m.contains("CP210x"), "{m}");
        assert!(!m.contains("Icom"), "{m}");
        assert!(!m.contains("MENU"), "{m}");
        assert!(
            m.contains("A4h"),
            "the probed CI-V address is still worth naming: {m}"
        );
        // The same rig on a Mac: /dev/cu.* vocabulary, no Device Manager, no COM.
        let m = compose_ladder_message(
            &silent("/dev/cu.wchusbserial1420"),
            "Xiegu X6200",
            0xA4,
            false,
            false,
            true,
        );
        assert!(m.contains("SERIAL-B"), "{m}");
        assert!(m.contains("/dev/cu."), "{m}");
        assert!(!m.contains("Device Manager"), "{m}");
        assert!(!m.contains("COM"), "COM is Windows vocabulary: {m}");
        // G90 / X5105 / X108G — one port. No pair hunt, and still no Icom menu or driver.
        for (name, mac) in [("Xiegu G90", false), ("Xiegu X5105", true)] {
            let m = compose_ladder_message(&silent("COM4"), name, 0x70, false, false, mac);
            assert!(m.contains("single"), "{m}");
            assert!(!m.contains("SERIAL-B"), "one port, no pair to hunt: {m}");
            assert!(!m.contains("install Icom's USB driver"), "{m}");
            assert!(!m.contains("rig menu"), "no rig menu is named: {m}");
            assert!(!m.contains("MENU"), "{m}");
            // The address itself is still reported — it is a real silent-bus cause.
            assert!(m.contains("70h"), "{m}");
        }
        // POSITIVE CONTROL — the Icom verdicts are untouched, on BOTH platforms.
        let m = compose_ladder_message(&hit, "Icom IC-7610", 0x98, false, true, false);
        assert!(m.contains("MENU » SET » Connectors » CI-V"), "{m}");
        assert!(m.contains("Link to [REMOTE]"), "{m}");
        let m = compose_ladder_message(&silent("COM4"), "Icom IC-7610", 0x98, false, true, false);
        assert!(m.contains("Windows Device Manager (Ports)"), "{m}");
        assert!(m.contains("install Icom's USB driver"), "{m}");
        let m = compose_ladder_message(
            &silent("/dev/cu.SLAB_USBtoUART"),
            "Icom IC-7610",
            0x98,
            false,
            true,
            true,
        );
        assert!(m.contains("TWO /dev/cu.* ports"), "{m}");
        assert!(m.contains("System Information"), "{m}");
    }

    /// The classifier reads the CATALOG NAME, which is what the composer is handed. This
    /// re-derives it from `rigmodels::rig_model_name` for every model that reaches the CI-V
    /// verdict, so a catalog rename fails HERE instead of silently reverting the fix to
    /// "everything is an Icom".
    #[test]
    fn civ_family_is_pinned_to_the_catalog_names() {
        use crate::rigmodels::rig_model_name;
        let family = |model: u32| civ_family(rig_model_name(model).expect("in the catalog"));
        // The four X-series Xiegus `ladder_kind` verified as publishing a civaddr, plus the
        // G90 (which may route to the Hamlib ladder instead — classifying it costs nothing).
        assert_eq!(family(3087), CivFamily::XieguDualUart); // X6100 — CH342 pair
        assert_eq!(family(3091), CivFamily::XieguDualUart); // X6200 — CH342 pair
        assert_eq!(family(3088), CivFamily::XieguSingleUart); // G90 — CP210x
        assert_eq!(family(3089), CivFamily::XieguSingleUart); // X5105 — CP210x
        assert_eq!(family(3076), CivFamily::XieguSingleUart); // X108G

        // The Icoms the verdict's menu/REMOTE/driver prose was written for.
        for m in [3073, 3078, 3081, 3085, 3090, 3013, 3023, 3060, 3070] {
            assert_eq!(family(m), CivFamily::Icom, "model {m}");
        }
        // A rig whose family we have no prose for names no menu at all.
        assert_eq!(civ_family("Yaesu FT-847"), CivFamily::Other);
        assert_eq!(civ_family("The rig"), CivFamily::Other);
    }

    #[test]
    fn a_direct_answer_at_the_configured_rate_points_at_the_backend_not_the_rig() {
        let r = report(
            115200,
            vec![(
                115200,
                BaudProbe::Reply {
                    freq_hz: Some(14_074_000),
                },
            )],
        );
        // Native backend selected → name it, and point at the CI-V diagnostic log.
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, true, true, false);
        assert!(m.contains("answers CI-V directly"), "{m}");
        assert!(m.contains("native CI-V"), "{m}");
        assert!(m.contains("CI-V diagnostic log"), "{m}");
        // Hamlib backend → name rigctld, and no native-only log pointer.
        let m2 = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true, false);
        assert!(m2.contains("rigctld"), "{m2}");
        assert!(!m2.contains("CI-V diagnostic log"), "{m2}");
    }

    #[test]
    fn an_unopenable_port_reports_the_os_error_not_a_guess() {
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::OpenFailed("Access is denied.".into())),
                (19200, BaudProbe::OpenFailed("Access is denied.".into())),
                (9600, BaudProbe::OpenFailed("Access is denied.".into())),
                (4800, BaudProbe::OpenFailed("Access is denied.".into())),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true, false);
        assert!(m.contains("Access is denied."), "{m}");
        assert!(m.contains("another program"), "{m}");
    }

    #[test]
    fn noise_without_a_valid_frame_says_the_port_is_talking_but_not_civ() {
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::Noise),
                (19200, BaudProbe::Silence),
                (9600, BaudProbe::Silence),
                (4800, BaudProbe::Silence),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true, false);
        assert!(m.contains("not valid CI-V"), "{m}");
    }
}
