//! The real-radio service loop (feature `device`).
//!
//! Drives a shared [`Engine`] against the sound card + rig on the FT1 slot clock.
//! Designed to run on a dedicated thread: the cpal backend (whose streams are
//! not `Send`) is created here and never leaves this thread; only the
//! `Arc<Mutex<Engine>>` is shared with the UI command handlers.
//!
//! Typical use from the desktop shell:
//! ```ignore
//! let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
//! let radio = engine.clone();
//! std::thread::spawn(move || {
//!     if let Err(e) = tempo_audio::service::run_radio(radio, RadioConfig::default()) {
//!         eprintln!("radio loop stopped: {e}");
//!     }
//! });
//! ```

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::engine::{engine_lock, DecodeApplied, DecodeJob, DecodePass, DecodeResult, Engine};
use tempo_core::tempo_fast;
use tempo_core::timing::{now_unix_ms, SlotClock};

use crate::backend::AudioBackend;
use crate::device::CpalBackend;
use crate::frames::RxRing;
use crate::rig::{PttMode, Rig, SerialLine};
use crate::rigctld_proc::{spawn_rigctld, RigctldProc};

/// The daemon serving the rigctld protocol on a radio's TCP port: Hamlib's spawned
/// `rigctld` (classic), or Nexus's own native CI-V daemon (`icom_native_cat` — same
/// protocol on the same port, plus the scope waveform + transceive the Hamlib path
/// can't deliver). Everything downstream (Rig, probe, handoff, monitors) is agnostic.
enum CatDaemon {
    Spawned(RigctldProc),
    // Only constructed with the `serial` feature (the native daemon owns a COM port).
    #[cfg_attr(not(feature = "serial"), allow(dead_code))]
    Native(crate::civ::broker::CivDaemon),
}

impl CatDaemon {
    fn is_alive(&mut self) -> bool {
        match self {
            CatDaemon::Spawned(p) => p.is_alive(),
            CatDaemon::Native(d) => d.is_alive(),
        }
    }
    /// The native daemon, when that's what this is (scope drain / enable).
    fn native(&self) -> Option<&crate::civ::broker::CivDaemon> {
        match self {
            CatDaemon::Native(d) => Some(d),
            CatDaemon::Spawned(_) => None,
        }
    }
}

/// A rigctld TCP port is never allowed to be 0. Nexus spawns rigctld on this port and connects to
/// `127.0.0.1:<port>`; connecting to port 0 fails on Windows with WSAEADDRNOTAVAIL ("the requested
/// address is not valid in its context", os error 10049). Settings repair (`ensure_distinct_radio_ports`)
/// keeps a persisted 0 from surviving a load, but this is the runtime backstop for a just-detected,
/// not-yet-saved profile. 4532 is Hamlib's default rigctld port.
fn safe_rigctld_port(port: u16) -> u16 {
    if port == 0 {
        4532
    } else {
        port
    }
}

/// The CI-V address to natively drive `t` at — `Some` only when the operator opted this
/// radio into `icom_native_cat` AND it's a scope-capable Icom on a serial connection.
fn native_civ_addr(t: &Transport) -> Option<u8> {
    if !t.icom_native_cat || t.is_network() || t.rig_model == 0 {
        return None;
    }
    crate::rigmodels::icom_scope_model(t.rig_model).map(|m| m.default_civ_addr())
}

/// Does this transport key RTS/DTR on the SAME serial port rigctld uses for CAT?
///
/// This is the single-cable interface (Digirig Mobile and friends): one USB port carries both
/// the CI-V/CAT bytes and the RTS keying line. Nexus used to detect only the OPPOSITE case (a
/// dedicated keying port, e.g. an SO2R controller) and fell back to "serial keying, no CAT" for
/// everything else — so the commonest single-cable interface in the hobby silently ran with NO
/// CAT AT ALL, while `probe_serial` reported success. The band never followed and nothing said why.
///
/// When true, rigctld owns the port and does BOTH (Hamlib shares the fd — see
/// [`crate::rigctld_proc::rigctld_args`]), so keying MUST go through the daemon
/// (`PttMode::Cat`). Our own `PttMode::Serial` could not open a port rigctld already holds.
///
/// ⚠️ THE SINGLE SOURCE OF TRUTH for this decision. [`ptt_mode_for`] and [`open_rig`] must both
/// consult it: they are two separate matches over `ptt_method`, and the last time they disagreed
/// the adopted rig kept `PttMode::Vox` and TX was silently dead after a radio switch. Excludes
/// network rigs — a TCP transport has no RTS line to key.
fn keys_on_the_cat_port(t: &Transport) -> bool {
    matches!(t.ptt_method.as_str(), "rts" | "dtr")
        && t.rig_model != 0
        && !t.is_network()
        && !t.serial_port.trim().is_empty()
        && t.ptt_port().eq_ignore_ascii_case(t.serial_port.trim())
}

/// Start the CAT daemon for `t` on its rigctld port: the native CI-V daemon when opted
/// in (falling back to rigctld if the port/serial open fails), else Hamlib's rigctld.
/// The second field of the `Ok` tuple is the native daemon's start error when it fell
/// back to rigctld — surfaced to the operator, so a "native selected" radio can never be
/// silently tested through Hamlib without saying so.
///
/// `ptt_line` is `Some` only for the shared-port keying case ([`keys_on_the_cat_port`]); it makes
/// the spawned rigctld key the transmitter on the same port it opened for CAT.
fn spawn_cat_daemon(
    t: &Transport,
    target: &str,
    network: bool,
    ptt_line: Option<SerialLine>,
) -> std::io::Result<(CatDaemon, Option<String>)> {
    // ⚠️ The native CI-V daemon speaks Icom CI-V on the serial port itself and has NO keying
    // path — it cannot assert RTS. Taking it here would open the port, leave PTT unkeyed, and
    // present as a rig that tunes but never transmits. When keying rides the CAT port, Hamlib's
    // rigctld is the ONLY backend that can do both, so skip native entirely (the operator keeps
    // CAT and keying; they lose only the native panadapter, which is the correct trade and is
    // surfaced by the scope falling back rather than failing silently).
    #[cfg_attr(not(feature = "serial"), allow(unused_mut))] // only mutated on the serial path
    let mut native_fallback: Option<String> = None;
    #[cfg(feature = "serial")]
    if let Some(addr) = native_civ_addr(t).filter(|_| ptt_line.is_none()) {
        match crate::civ::broker::CivDaemon::start(&t.serial_port, t.baud, addr, t.rigctld_port) {
            Ok(d) => return Ok((CatDaemon::Native(d), None)),
            Err(e) => {
                // Fall through to rigctld — CAT keeps working, just without the scope.
                // Recorded, not just printed: the probe detail must SAY the tested
                // backend was the fallback, or the operator debugs the wrong daemon.
                eprintln!("tempo-audio: native CI-V daemon failed ({e}); falling back to rigctld");
                native_fallback = Some(e.to_string());
            }
        }
    }
    #[cfg(not(feature = "serial"))]
    let _ = native_civ_addr(t); // native CI-V needs the serial feature; classic path below
    spawn_rigctld(
        t.rig_model,
        target,
        t.baud,
        t.rigctld_port,
        network,
        ptt_line,
    )
    .map(|p| (CatDaemon::Spawned(p), native_fallback))
}

/// Which CAT backend is actually serving, for probe/status attribution — the operator
/// must never have to guess whether "isn't answering" came from the native CI-V daemon
/// or from Hamlib. `native_wanted` = the transport opted into native CI-V (and keying
/// doesn't force rigctld); `daemon` = `Some(is_native)` for a daemon we own, `None` when
/// we attached to a rigctld someone else launched.
fn cat_backend_label(native_wanted: bool, daemon: Option<bool>) -> &'static str {
    match daemon {
        Some(true) => "native CI-V",
        Some(false) if native_wanted => "Hamlib rigctld — the native CI-V daemon didn't start",
        Some(false) => "Hamlib rigctld",
        None => "a shared external rigctld",
    }
}

/// Append the backend attribution to a probe/status detail line, success and failure
/// alike (WSJT-X-style Test CAT says what it tested, not just how it went).
fn with_backend(detail: String, label: &str) -> String {
    format!("{detail} (via {label})")
}

/// A clear, model-aware "CAT is down" message for when the rig stops answering — the field-report
/// fix for a tester who ran hours not knowing CAT was dead and a silent "reply incomplete" loop.
/// It NAMES the config (model / port / baud) so the operator can see a baud/port mismatch at a
/// glance, and for an Icom adds the dual-USB-port gotcha (the IC-7610/9700 expose two serial ports,
/// only one of which carries CI-V — picking the wrong one looks exactly like this).
fn cat_down_message(t: &Transport, err: &std::io::Error) -> String {
    if t.is_network() {
        return format!(
            "CAT can't reach the rig — no reply from {} ({err}). Check the radio is powered on \
             and the network CAT address/port is correct.",
            t.rig_addr
        );
    }
    let name = crate::rigmodels::rig_model_name(t.rig_model).unwrap_or("the rig");
    let hint = if crate::rigmodels::rig_model_name(t.rig_model)
        .is_some_and(|n| n.starts_with("Icom"))
    {
        " Icom rigs expose TWO USB serial ports — make sure the CAT port is the CI-V one, and that \
         the rig's CI-V baud matches this setting."
    } else {
        ""
    };
    format!(
        "CAT can't reach the rig — {name} on {} @ {} baud isn't answering ({err}). Check the COM \
         port, that the CAT baud matches the rig, and that the radio is on.{hint}",
        t.serial_port, t.baud
    )
}

use tempo_app::dto::{SourceKind, Tier};
use tempo_app::settings::{RadioProfile, Settings};
use tempo_core::message::Msg;
// Band label → club-log meter string. Lives in `tempo_net` beside the two
// protocols that consume it (N1MM `<band>`, N3FJP `fldBand`), because the
// shell's per-QSO N1MM forwarder needs the identical conversion.
use tempo_net::band_for_interop;
use tempo_net::pskreporter::{PskReporter, Spot};
use tempo_net::server::WsjtxServer;
use tempo_net::wsjtx::{
    Decode as WsjtxDecode, Inbound as WsjtxInbound, QsoLogged as WsjtxQso, Status as WsjtxStatus,
};

/// Flush PSK Reporter spots at most this often (seconds) — its service rate-limits.
const PSK_FLUSH_SECS: f64 = 300.0;

/// Coarse heartbeat (ms) for the no-CAT N3FJP band report, so the club board
/// stays fresh without a TCP connect every slot boundary. A band/mode change
/// reports immediately regardless of this interval.
const N3FJP_BAND_REPORT_MS: f64 = 60_000.0;

/// Tune-carrier audio tone (Hz), the same f0 the FT1 modem centers on.
const TUNE_FREQ_HZ: f32 = 1500.0;
/// How many ms of tune carrier to queue per loop iteration (keeps the output
/// ring fed across the loop's sleep without building a large backlog).
const TUNE_CHUNK_MS: f32 = 40.0;
/// HARD CEILING on the tune auto-release: never hold PTT + a steady carrier
/// longer than this, whatever `settings.tune_timeout_secs` says — the setting
/// is a bare numeric field AND settings.json is hand-editable, so one mistyped
/// digit (120 for 12) must not buy a two-minute unattended carrier into the
/// finals or a dead load. Clamped at the point of use, not only in the UI.
const MAX_TUNE_MS: f64 = 60_000.0;
/// Safety auto-stop for a forgotten QSO recording: cap a single recording at 2 hours so a
/// recording the operator forgot to stop can't fill the disk unbounded (~86 MB/hour).
const MAX_QSO_REC_MS: f64 = 2.0 * 60.0 * 60.0 * 1000.0;
/// How often to run the FULL rig read-back over CAT — RF power, S-meter, mode mirror, DSP funcs.
/// Each is a blocking TCP round-trip, so the heavy set is throttled well below the loop rate.
const RIG_POLL_MS: f64 = 750.0;
/// How often to read the NEXT transmit meter while keyed — the mirror image of the RX health
/// poll. One meter is read per interval (round-robin over SWR/ALC/Po/COMP), so at 150 ms each
/// meter refreshes ~1.7×/s: live enough to set mic gain against the moving ALC bar, while never
/// more than one blocking CAT read lands per loop tick. RX health polling is suspended while
/// keyed, so this reuses that bus headroom.
const TX_METER_POLL_MS: f64 = 150.0;
/// How often to run the FAST dial-only read-back. The dial is the one value that must track a
/// manual VFO knob in real time, so it's polled ~4× faster than the heavy set — matching HRD's
/// Yaesu responsiveness (which is pure fast polling; the earlier 1–2 s lag was self-inflicted by
/// reading the dial only on the 750 ms health cadence). A single `F`-read is cheap on a healthy
/// serial link, and the transport-aware read deadline bounds a stalled one.
const FREQ_POLL_MS: f64 = 180.0;
/// Consecutive heavy-poll dial-read failures before the CAT breaker trips. >1 so a single slow
/// reply (the short serial deadline can cut off a legitimately-slow band-stack switch / USB spike)
/// doesn't permanently kill read-back; small enough that a truly dead link still stops the loop
/// blocking within ~2 s.
const FREQ_MISS_LIMIT: u32 = 3;
/// First re-probe delay after the CAT breaker trips (ms). Short enough that a transient stall —
/// a band-stack switch, a USB-serial spike, the reconnect churn a refused command causes — costs
/// a couple of seconds of read-back, not the whole session.
const CAT_RETRY_BASE_MS: f64 = 2_000.0;
/// Re-probe ceiling after repeated failures (ms). A genuinely unplugged rig settles at one cheap
/// timeout per ~30 s: enough to notice a cable going back in, cheap enough to ignore.
const CAT_RETRY_MAX_MS: f64 = 30_000.0;
/// How many times a REFUSED dial is re-sent before we stop asking. Much smaller than
/// [`MODE_SET_MAX_TRIES`]: a rejected mode is often a settling rig that will accept it shortly,
/// whereas a rejected FREQUENCY is nearly always a hard fact about the radio's range — and each
/// retry costs a full CAT round-trip on a link that is already unhappy.
const DIAL_SET_MAX_TRIES: u32 = 3;
/// Hamlib func tokens for the Expert DSP toggles, in the engine's `[nb, nr, notch, comp, vox]`
/// order. `ANF` (auto-notch) is the notch we expose — it works as a bare on/off toggle, unlike
/// `MN` (manual notch) which needs a separate NOTCHF frequency level.
const RIG_FUNCS: [&str; 5] = ["NB", "NR", "ANF", "COMP", "VOX"];
/// First re-probe delay for a DSP func that latched unsupported, in heavy polls (40 × 750 ms
/// ≈ 30 s — the old fixed cadence, now only the FIRST retry).
const FUNC_RETRY_BACKOFF_BASE: u32 = 40;
/// Backoff ceiling, in heavy polls (2560 × 750 ms ≈ 32 min). A func the rig genuinely lacks
/// settles here instead of costing a CAT timeout every 30 s for the whole session.
const FUNC_RETRY_BACKOFF_MAX: u32 = 2560;

/// Indices into the `RadioLoop` `level_supported` / `level_misses` arrays — the optional extended
/// per-poll level reads (RF power, mic gain, NR level, AGC). They mirror the rig's real knob
/// positions into the UI every RX poll. A rig that's slow or silent on any of them (the Elecraft
/// K4 via QK4 Remote is the report) makes each read eat the full per-command timeout and then
/// drop+reconnect the CAT socket — the ~5 s "Nexus hangs up every few seconds" churn. Capability-
/// caching them (3 consecutive misses → stop issuing that read) ends it, the same way
/// `smeter_supported` and `func_supported` already gate their own reads.
const LVL_RFPOWER: usize = 0;
const LVL_MICGAIN: usize = 1;
const LVL_NR: usize = 2;
const LVL_AGC: usize = 3;

/// Record one extended-level read outcome into its `supported`/`misses` slot, with the same
/// miss-tolerance as the S-meter: a hit resets the counter and confirms support; three consecutive
/// misses mark the read unsupported so the poll loop stops issuing it (and stops the socket churn).
fn note_ext_read(supported: &mut Option<bool>, misses: &mut u8, ok: bool) {
    if ok {
        *supported = Some(true);
        *misses = 0;
    } else {
        *misses = misses.saturating_add(1);
        if *misses >= 3 {
            *supported = Some(false);
        }
    }
}

/// AGC speed <-> Hamlib enum int (FAST=2, MEDIUM=5, SLOW=3). The UI/engine speak
/// "fast"/"mid"/"slow"; the rigctld `AGC` level carries the enum int.
fn agc_to_hamlib(speed: &str) -> u8 {
    match speed {
        "fast" => 2,
        "slow" => 3,
        _ => 5, // mid
    }
}
fn agc_from_hamlib(v: u8) -> &'static str {
    match v {
        2 => "fast",
        3 => "slow",
        _ => "mid", // 5 medium (and off/superfast fold to mid for display)
    }
}
/// Max consecutive `set_mode` retries for one target mode before giving up (so a rig
/// that rejects a submode doesn't get an `M` command every loop). Sized to ride out a
/// rig/rigctld that's still settling (a failing CAT round-trip can block up to the
/// 500 ms read timeout, so even a couple dozen tries spans seconds), then we stop
/// retrying THAT mode until the target changes.
const MODE_SET_MAX_TRIES: u32 = 30;
/// After this many consecutive failures, DATA-mode retries drop their explicit 3 kHz
/// passband and go filter-agnostic (`M PKTUSB 0`) — the middle rung of the mode-set
/// resilience ladder (see [`retry_passband`]). A backend that chokes on the width→DATA-
/// filter mapping (not the mode itself) then gets accepted instead of riding the whole
/// budget into a bogus "no such mode" give-up.
const MODE_SET_PASSBAND0_AFTER: u32 = 10;

/// Station configuration for the radio loop.
///
/// Maps directly from `tempo_app::settings::Settings`: `ptt_method` selects how
/// PTT is keyed, and for CAT the `rig_model` / `serial_port` / `baud` /
/// `rigctld_port` describe the `rigctld` daemon Tempo launches itself.
pub struct RadioConfig {
    /// Where every waterfall source publishes, shared with the UI reader and the rx-dsp thread.
    /// Defaulted so existing constructions (tests, tools) need no change.
    pub spectrum_feed: tempo_app::engine::SpectrumFeed,
    /// The wait-free capture tee the rx-dsp thread drains (rxtap.rs).
    pub rx_tap: Arc<crate::rxtap::RxTap>,
    /// PTT method: `"cat"` (launch + use rigctld), `"rts"`, `"dtr"`, or `"vox"`.
    pub ptt_method: String,
    /// Hamlib rig model number for `rigctld -m` (0 = none / VOX).
    pub rig_model: u32,
    /// Serial port for CAT / serial PTT, e.g. `"COM5"` or `"/dev/ttyUSB0"`.
    pub serial_port: String,
    /// Serial baud for CAT.
    pub baud: u32,
    /// "network" → rigctld connects to `rig_addr` over TCP (Flex/SmartSDR); else serial.
    pub rig_conn: String,
    /// host:port for a network rig (when `rig_conn == "network"`).
    pub rig_addr: String,
    /// Local TCP port Tempo runs rigctld on (and connects to).
    pub rigctld_port: u16,
    /// Native Icom CI-V opt-in (Nexus owns the CI-V serial port + serves the rigctld
    /// protocol itself — unlocks the rig's real scope waveform). Off = classic rigctld.
    pub icom_native_cat: bool,
    /// The port our OWN CAT broker serves on (if enabled), so auto-coexist never
    /// connects Nexus to itself. `None` = broker off.
    pub broker_self_port: Option<u16>,
    /// Dial frequency to set on the rig (Hz).
    pub dial_hz: u64,
    /// Operating mode to set on the rig (e.g. "USB", "FM"). FM repeater shift / offset /
    /// CTCSS are read LIVE from the engine settings in the loop (not carried here).
    pub mode: String,
    /// Emit the WSJT-X-compatible UDP protocol (loggers / JTAlert / GridTracker).
    pub wsjtx_udp: bool,
    /// UDP target for WSJT-X messages (WSJT-X default 127.0.0.1:2237).
    pub wsjtx_addr: String,
    /// Upload heard stations to PSK Reporter.
    pub pskreporter: bool,
    /// Input (capture) device name. Empty = system default input.
    pub audio_in: String,
    /// Output (playback) device name. Empty = system default output.
    pub audio_out: String,
    /// Tx audio level (0.0–1.0) applied to outgoing samples.
    pub tx_level: f32,
    /// RX capture gain (≥1.0) applied to received audio before decode.
    pub rx_gain: f32,
}

impl Default for RadioConfig {
    fn default() -> Self {
        Self {
            spectrum_feed: tempo_app::engine::SpectrumFeed::default(),
            rx_tap: Arc::new(crate::rxtap::RxTap::new()),
            ptt_method: "vox".to_string(),
            rig_model: 0,
            serial_port: String::new(),
            baud: 38400,
            rig_conn: "serial".to_string(),
            rig_addr: String::new(),
            rigctld_port: 4532,
            icom_native_cat: false,
            broker_self_port: None,
            dial_hz: 14_090_500,
            mode: "USB".to_string(),
            wsjtx_udp: false,
            wsjtx_addr: "127.0.0.1:2237".to_string(),
            pskreporter: false,
            audio_in: String::new(),
            audio_out: String::new(),
            tx_level: 0.9,
            rx_gain: 1.0,
        }
    }
}

/// Set on app shutdown so the radio loop unkeys the transmitter and exits
/// (see the check at the top of the loop in [`run_radio`]). A stuck carrier on
/// quit is a TX-safety hazard, so the exit path sets this and waits briefly.
pub static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by the radio loop AFTER it has unkeyed the transmitter and is exiting.
/// The shutdown path polls this so it returns the instant the un-key is flushed
/// (~tens of ms in the common case) but still waits out a worst-case in-flight
/// CAT command (a blocking read can hold the loop for up to 2.5 s) instead of a
/// fixed sleep that could exit before the un-key ever runs.
pub static SHUTDOWN_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Bind the WSJT-X UDP emitter for `addr` (None when disabled or the address is
/// unparseable). Loopback logger → bind loopback so the TX-arming inbound control
/// socket isn't reachable off-host; a logger on another machine → all-interfaces.
/// On success sends the opening Heartbeat so a listener (GridTracker, JTAlert)
/// registers the client immediately — the same Heartbeat is what makes a live
/// rebind (toggle flipped after launch) connect without an app restart.
fn build_wsjtx_server(enabled: bool, addr: &str) -> Option<WsjtxServer> {
    if !enabled {
        return None;
    }
    match addr.parse::<std::net::SocketAddr>() {
        Ok(target) => {
            let bind = if target.ip().is_loopback() {
                "127.0.0.1:0"
            } else {
                "0.0.0.0:0"
            };
            match WsjtxServer::new(bind.parse().unwrap(), target) {
                Ok(s) => {
                    let _ = s.send_heartbeat(3, env!("CARGO_PKG_VERSION"), "Nexus");
                    Some(s)
                }
                Err(e) => {
                    eprintln!("tempo: WSJT-X UDP disabled: {e}");
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("tempo: invalid wsjtxAddr {:?}: {e}", addr);
            None
        }
    }
}

/// Run the radio slot loop until an unrecoverable error. Blocks — call on a
/// dedicated thread. Opens the default sound devices, sets the rig, then each
/// slot transmits the engine's `poll_tx` audio (holding PTT for the over) or
/// decodes the captured frame into the engine.
pub fn run_radio(engine: Arc<Mutex<Engine>>, cfg: RadioConfig) -> Result<(), String> {
    let in_name = (!cfg.audio_in.is_empty()).then_some(cfg.audio_in.as_str());
    let out_name = (!cfg.audio_out.is_empty()).then_some(cfg.audio_out.as_str());
    let mut backend = match CpalBackend::open(in_name, out_name) {
        Ok(b) => b,
        Err(e) => {
            // Surface a sound-card open failure to the UI (which would otherwise
            // see only a silent, blank waterfall) before the loop bails out.
            {
                let mut eng = engine_lock(&engine);
                eng.set_audio_error(Some(format!("Sound card failed to open: {e}")));
            }
            return Err(e);
        }
    };
    backend.set_tx_level(cfg.tx_level);
    backend.set_rx_gain(cfg.rx_gain);
    // Hand the capture tee to the waterfall producer and start it. From here the row is made on
    // ITS thread, so this loop's blocking CAT can no longer starve the waterfall (rxtap.rs).
    if let Some((ring, rate)) = backend.spectrum_tap() {
        cfg.rx_tap.publish_card(ring, rate);
    }
    crate::rxdsp::spawn(cfg.rx_tap.clone(), cfg.spectrum_feed.clone());

    // Resolve the PTT method into a Rig and probe it. `open_rig` launches rigctld
    // for CAT (its kill-on-drop handle lives as long as the rig) and reports the
    // connection status so the UI shows green/red right away. The transport is
    // rebuilt **live** below when the operator changes rig/PTT/audio settings, so
    // CAT connects on Save without an app restart.
    let applied = Transport::from_cfg(&cfg);
    // Initial open: allow coexisting onto a pre-existing EXTERNAL rigctld (e.g. WSJT-X already sharing
    // the rig). Mid-session rig SWITCHES pass `allow_coexist=false` when they reuse their own port.
    let (mut rig, rigctld_proc, init_probe) = open_rig(&applied, true);
    let init_freq = init_probe.freq_hz;
    {
        let mut eng = engine_lock(&engine);
        eng.set_cat_status(init_probe.ok, init_probe.detail);
        // Read-only-launch seed: the rig's OWN dial/mode become the app's belief, under
        // the same lock and BEFORE the loop starts — so the UI's first snapshot poll
        // already shows the rig's reality, and the band-edge chime's first-value
        // suppression sees one coherent value instead of a persisted→read flip.
        // `freq_hz`/`mode` are Some only when a real read succeeded over a real control
        // channel, which is precisely the `rig_confirmed` condition (a serial-PTT rig
        // sharing the CAT port has ok==true but no read — stays unconfirmed).
        if let Some(hz) = init_probe.freq_hz {
            eng.seed_rig_dial(hz);
            eng.set_rig_confirmed(true);
        }
        if let Some(m) = init_probe.mode {
            eng.observe_rig_mode(m); // display-only; never adopted into operating_mode
        }
    }

    // Background clock-offset probe (SNTP), on its own thread so a slow/failed
    // network query never stalls the audio loop. Honors the `clock_check`
    // setting and fails silently off-grid (publishes None → UI shows DT health).
    {
        let clk_engine = engine.clone();
        std::thread::spawn(move || clock_probe_loop(clk_engine));
    }

    // Optional network outputs (WSJT-X UDP API + PSK Reporter). Built here from the
    // startup config AND rebuilt live in the loop when the operator flips a toggle or
    // retargets the WSJT-X address — otherwise a GridTracker/PSK setup done AFTER launch
    // never connects (the reported "needs a Nexus restart" bug). `*_applied` tracks what
    // the current emitters were built for so the loop rebuilds only on a real change.
    let mut wsjtx = build_wsjtx_server(cfg.wsjtx_udp, &cfg.wsjtx_addr);
    let mut wsjtx_applied = (cfg.wsjtx_udp, cfg.wsjtx_addr.clone());
    let mut psk = cfg.pskreporter.then(PskReporter::new);
    let mut psk_applied = cfg.pskreporter;

    // The loop's persistent state lives in RadioLoop; one iteration is
    // RadioLoop::step (generic over the AudioBackend, so a MockBackend can drive
    // it in tests). The wrapper owns only the device edges (sound card + rigctld)
    // and injects their re-open side-effects.
    let mut state = RadioLoop::new(applied, rigctld_proc, &cfg);
    // Station-wide sinks live OUTSIDE the per-radio loop (multi-radio Phase 1 boundary):
    // one PSK buffer and one Field Day / club-board cursor for the whole station.
    let mut station = StationSinks::new();
    // Seed last_dial from the READ dial when present: seed_rig_dial moved
    // settings.dial_hz() to the same value, so `dial != last_dial` must not fire a
    // command on tick 1. last_mode deliberately stays cfg.mode (the app's BELIEF) —
    // seeding it from the read would make the steady-state retune command the mode
    // ~20 ms after boot, defeating read-only launch (the documented trap).
    if let Some(hz) = init_freq {
        state.last_dial = hz;
    }

    // --- Dual-radio: persistent per-radio CAT (true "both live"). The ACTIVE radio is `rig`/`state`
    // above (unchanged path). Every OTHER enabled radio gets its own persistent rigctld+Rig in the
    // monitor pool, polled READ-ONLY on a dedicated thread → the switcher pills show both rigs live.
    // Switching = a HANDOFF (swap the active Rig with a pool one) — no teardown, so no read-back race.
    let pool: MonitorPool = Arc::new(Mutex::new(Vec::new()));
    // The active radio at startup (so the monitor thread doesn't also open it).
    let mut last_active = engine_lock(&engine).settings().active_radio;
    // Raised the moment a switch intent is seen, dropped when the handoff completes: the
    // monitor thread pauses its pool work while set, so a switch never queues behind slow
    // monitor CAT reads (the pool lock is otherwise held for whole read bursts).
    let switch_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mon_engine = engine.clone();
        let mon_pool = pool.clone();
        let mon_pending = switch_pending.clone();
        std::thread::spawn(move || monitor_loop(mon_engine, mon_pool, mon_pending));
    }
    loop {
        // Dual-radio: if the operator switched the active radio, hand off between the active Rig and
        // the monitor pool BEFORE the normal tick — so `state.applied` already matches the new active
        // and the `rig_differs` teardown never fires (the new rig is already connected + on-frequency).
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &switch_pending,
        );
        // App shutdown: unkey the transmitter through the still-alive rig before
        // the process exits. Without this, quitting while keyed (a TX slot or a
        // tune carrier) leaves the radio transmitting until its own timeout.
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            backend.flush_output();
            let _ = rig.ptt(false);
            // Drop any in-flight SSTV image feed too (the flush_output above already
            // dumped its queued audio and ptt(false) unkeyed the carrier — this is
            // symmetry with the CW/RTTY cuts below).
            state.sstv_feed = None;
            // Cut any in-progress CW too: stop a CAT `send_morse` and flush a
            // WinKeyer's hardware buffer NOW, deterministically, rather than
            // relying on Drop running before the process is killed (a half-sent
            // WinKeyer message would otherwise keep keying on the air).
            let _ = rig.stop_morse();
            #[cfg(feature = "serial")]
            if let Some((_, wk)) = state.winkeyer.as_mut() {
                let _ = wk.clear();
            }
            // Cut any in-progress RTTY FSK keying the same way: abort the keying
            // thread NOW (line parked at mark) rather than relying on Drop order.
            // (The AFSK path is already covered — flush_output above dumps its
            // queued audio and the ptt(false) unkeyed the carrier.)
            #[cfg(feature = "serial")]
            if let Some((_, _, k)) = state.rtty_keyer.as_ref() {
                k.clear();
            }
            SHUTDOWN_DONE.store(true, std::sync::atomic::Ordering::Relaxed);
            return Ok(());
        }
        // Hot-apply the WSJT-X UDP + PSK Reporter settings (enable/disable, and the
        // WSJT-X target address) without a restart. A brief settings read per tick;
        // an actual rebuild only when the setting changed — the rebind re-sends the
        // WSJT-X Heartbeat so GridTracker/JTAlert register the client immediately.
        // The lock is released before state.step (which takes its own).
        {
            let e = engine_lock(&engine);
            let s = e.settings();
            if (s.wsjtx_udp, s.wsjtx_udp_addr.as_str())
                != (wsjtx_applied.0, wsjtx_applied.1.as_str())
            {
                wsjtx = build_wsjtx_server(s.wsjtx_udp, &s.wsjtx_udp_addr);
                wsjtx_applied = (s.wsjtx_udp, s.wsjtx_udp_addr.clone());
            }
            if s.pskreporter != psk_applied {
                psk = s.pskreporter.then(PskReporter::new);
                psk_applied = s.pskreporter;
            }
        }
        let sinks = Sinks {
            wsjtx: wsjtx.as_ref(),
            psk: psk.as_ref(),
            cfg_dial_hz: cfg.dial_hz,
        };
        let now = now_unix_ms();
        let stepped = state.step(
            &engine,
            &mut backend,
            &mut rig,
            &sinks,
            now,
            &mut |t: &Transport| {
                let inn = (!t.audio_in.is_empty()).then_some(t.audio_in.as_str());
                let outn = (!t.audio_out.is_empty()).then_some(t.audio_out.as_str());
                CpalBackend::open(inn, outn).map(|mut b| {
                    b.set_tx_level(t.tx_level);
                    b.set_rx_gain(t.rx_gain);
                    b
                })
            },
            &mut |t: &Transport, allow_coexist: bool| open_rig(t, allow_coexist),
            &mut station,
        );
        if let Err(e) = stepped {
            // This thread is the ONLY thing that ever drops PTT — the tx_until_ms
            // deadline, the hard stop and the idle self-heal all die with it, and
            // RigctldProc's Drop kills the daemon without unkeying. An exit
            // mid-over would leave the carrier up until the operator notices, so
            // best-effort unkey on EVERY error exit, not only the SHUTDOWN path.
            let _ = rig.ptt(false);
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ======================= Dual-radio: persistent per-radio CAT (monitor pool) =======================

/// The shared pool of persistent, read-only CAT connections to the NON-active radios ("both live").
type MonitorPool = Arc<Mutex<Vec<MonitorConn>>>;

/// Per-radio dial-read cadence for a monitor (unhurried — the active radio has the fast poll).
const MONITOR_POLL_MS: f64 = 600.0;

/// One persistent CAT connection to a NON-active radio. Holds its own live rigctld + Rig; a switch
/// HANDS this Rig to/from the active slot (never a teardown). CAT-only: no audio, and this struct is
/// only ever READ from (no `ptt`/`set_*` call site touches a `MonitorConn` — single-TX-authority).
struct MonitorConn {
    id: u32,
    transport: Transport,
    rig: Rig,
    rigctld_proc: Option<CatDaemon>,
    last_poll: f64,
    ticks: u32,
    smeter_supported: Option<bool>,
    /// Consecutive failed freq reads — the pill only goes red after ≥3 (mirrors the
    /// active loop's FREQ_MISS_LIMIT; a single slow poll must not flash the pill).
    freq_misses: u32,
    /// Consecutive FAILED OPENS for this radio — see `retry_after_ms`.
    open_failures: u32,
    /// Monotonic ms before which this conn must NOT be recycled, even though it has
    /// no control channel.
    ///
    /// ⭐ WITHOUT THIS THE POOL IS A PROCESS-SPAWN LOOP. `open_monitor` parks a
    /// radio whose rigctld cannot stay up as a control-less `Rig::vox()`; the keep
    /// test below then sees `!has_control()` and recycles it; the reconcile runs
    /// every 150 ms and each reopen costs a 700 ms daemon-liveness wait. A second
    /// radio that is ENABLED but unreachable — powered off, unplugged, COM port
    /// absent, port clash — therefore spawns and kills a rigctld.exe roughly every
    /// 850 ms, forever. On Windows that is expensive process creation plus a 12 MB
    /// libhamlib DLL re-scanned by Defender on every launch, which is why it shows
    /// up as antimalware CPU rather than as ours.
    retry_after_ms: f64,
}

impl Transport {
    /// Build a transport from a SPECIFIC radio profile (not the flat active mirror) — to open a
    /// monitor connection to a non-active radio. Audio/monitor fields are zeroed (monitors are
    /// CAT-only) and the broker port dropped (only the active radio talks to the broker).
    fn from_profile(p: &RadioProfile) -> Self {
        Self {
            ptt_method: p.ptt_method.clone(),
            rig_model: p.rig_model,
            serial_port: p.serial_port.clone(),
            // Global keying-line setting (not per-radio) — the live `from_settings` rebuild
            // of the ACTIVE radio supplies it; a monitor radio is read-only (never keys).
            ptt_serial_port: String::new(),
            baud: p.baud,
            rig_conn: p.rig_conn.clone(),
            rig_addr: p.rig_addr.clone(),
            rigctld_port: safe_rigctld_port(p.rigctld_port),
            icom_native_cat: p.icom_native_cat,
            broker_self_port: None,
            audio_in: String::new(),
            audio_out: String::new(),
            voice_mic_device: String::new(),
            tx_level: p.tx_level,
            rx_gain: p.rx_gain,
            monitor_enabled: false,
            monitor_device: String::new(),
            monitor_level: 0.5,
        }
    }
}

/// Open a READ-ONLY CAT connection for a monitor radio: launch its rigctld (or share an EXTERNAL one
/// already on the port) and probe by reading the dial — but NEVER set freq/mode/PTT (a monitor must
/// not disturb the radio the operator isn't focused on). Returns the Rig + daemon handle + cat_ok.
fn open_monitor(t: &Transport) -> (Rig, Option<CatDaemon>, Option<bool>) {
    if t.rig_model == 0 {
        return (Rig::vox(), None, None);
    }
    // A monitor ALWAYS spawns its OWN rigctld — it must NEVER coexist onto a daemon already on the
    // port, because `probe_rigctld` can only tell that SOMETHING is listening, not WHICH radio it
    // serves; coexisting onto another radio's daemon is the dual-radio crossed-CAT bug (a monitor
    // reading + commanding the wrong rig). If the port is already taken, our spawned rigctld can't
    // bind and exits immediately → `is_alive()` is false → we report DISCONNECTED (fail safe) instead
    // of connecting to the foreign daemon. Distinct ports (validated on every save) make this the
    // normal, clean path.
    let addr = format!("127.0.0.1:{}", t.rigctld_port);
    let (target, network) = if t.is_network() {
        (t.rig_addr.as_str(), true)
    } else {
        (t.serial_port.as_str(), false)
    };
    // `None`: a monitor is READ-ONLY and must never be able to key. Even for a shared-port
    // keying transport, the background rig's daemon comes up WITHOUT --ptt-type, so a stray
    // keying command cannot reach a radio the operator is not focused on.
    match spawn_cat_daemon(t, target, network, None) {
        Ok((mut proc, _native_fallback)) => {
            std::thread::sleep(Duration::from_millis(700));
            if !proc.is_alive() {
                // Our daemon exited — it couldn't bind the port (a clash). Do NOT connect: whatever's
                // on the port isn't ours. Report disconnected; the pill shows the radio down.
                return (Rig::vox(), None, Some(false));
            }
            let mut rig = Rig::with_control(Some(addr), PttMode::Vox);
            // Native-daemon transports are LOCAL TCP but their serve path can take up to
            // ~1.3 s (engine queue) — the client deadline must outlast it or every busy
            // moment reads as CAT-dead (the flapping pill).
            rig.set_slow_transport(
                network
                    || native_civ_addr(t).is_some()
                    || crate::rigmodels::is_slow_serial_link(t.rig_model, t.baud),
            );
            let ok = probe_cat(&mut rig, t.rigctld_port).ok;
            (rig, Some(proc), ok)
        }
        Err(_) => (Rig::vox(), None, Some(false)),
    }
}

/// The monitor thread: keeps a persistent read-only CAT connection to every ENABLED, NON-active radio,
/// reconciling the pool against live settings and polling each radio's dial/mode/S-meter into the
/// engine's per-radio live cache. NEVER commands or keys a rig.
fn monitor_loop(
    engine: Arc<Mutex<Engine>>,
    pool: MonitorPool,
    pending: Arc<std::sync::atomic::AtomicBool>,
) {
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        // Desired monitor set (enabled, non-active, has a rig model), snapshot under a brief lock.
        let (active, want): (u32, Vec<(u32, Transport)>) = {
            let e = engine_lock(&engine);
            let s = e.settings();
            let active = s.active_radio;
            let want = s
                .radios
                .iter()
                .filter(|p| p.enabled && p.id != active && p.rig_model != 0)
                .map(|p| (p.id, Transport::from_profile(p)))
                .collect();
            (active, want)
        };
        // A switch is mid-flight: stay off the pool entirely so the handoff's try_lock wins
        // on its next 20 ms tick (a monitor poll can hold the lock for whole read bursts).
        if pending.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        reconcile_pool(&pool, &want, active, &engine, now_unix_ms());
        poll_monitors(&pool, active, &engine, &pending);
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Bring the monitor pool in line with the desired `(id, transport)` set: open newly-wanted radios,
/// close removed ones, rebuild a radio whose CAT config changed. Opens happen WITHOUT the pool lock
/// held (spawning rigctld is slow) so a concurrent handoff never waits on a daemon launch.
fn reconcile_pool(
    pool: &MonitorPool,
    want: &[(u32, Transport)],
    active: u32,
    engine: &Arc<Mutex<Engine>>,
    now_ms: f64,
) {
    let (to_open, to_close): (Vec<(u32, Transport)>, Vec<u32>) = {
        let mut p = pool.lock().unwrap_or_else(|e| e.into_inner());
        let mut to_open = Vec::new();
        for (id, t) in want {
            // Keep only a CAT-identical AND LIVE conn — live Rig control channel AND a live
            // daemon. A conn parked as `Rig::vox()` (rigctld couldn't bind / CAT probe failed)
            // has no control channel; a dead DAEMON behind a cached TCP answer is a zombie.
            // Either way: recycle so it self-heals (and a switch-to never adopts a dead conn).
            let keep = p.iter_mut().find(|c| c.id == *id).is_some_and(|c| {
                if c.transport.rig_differs(t) {
                    return false; // CAT settings changed — always reopen, no backoff
                }
                // ⭐ BACKOFF: a conn that failed to open is KEPT (not recycled) until
                // its retry window opens. Without this the 150 ms reconcile respawns
                // an unreachable radio's rigctld forever — see `retry_after_ms`.
                // A CAT change above bypasses it, because that is the operator
                // fixing the very thing that was broken and they should not wait.
                if !c.rig.has_control() && now_ms < c.retry_after_ms {
                    return true;
                }
                c.rig.has_control() && c.rigctld_proc.as_mut().is_none_or(CatDaemon::is_alive)
            });
            if !keep {
                to_open.push((*id, t.clone())); // new / CAT changed / DEAD → (re)open
            }
        }
        let mut to_close: Vec<u32> = Vec::new();
        for c in p.iter_mut() {
            // NEVER close the new ACTIVE radio's conn: right after a switch it leaves the want
            // list, but the handoff wants to ADOPT it (the instant switch). Closing it here
            // wins the race by design (back-to-back locks vs a 20 ms-cadence try_lock) and
            // downgrades every switch to a fresh daemon spawn. If the handoff instead takes
            // its fallback, IT drops this conn — nothing leaks.
            if c.id == active {
                continue;
            }
            let keep = match want.iter().find(|(wid, _)| *wid == c.id) {
                None => false, // no longer wanted
                Some((_, t)) => {
                    !c.transport.rig_differs(t)
                        && c.rig.has_control()
                        // A dead DAEMON behind a live TCP cache is a zombie: the pill
                        // would show a frozen dial forever. Recycle it.
                        && c.rigctld_proc.as_mut().is_none_or(CatDaemon::is_alive)
                }
            };
            if !keep {
                to_close.push(c.id);
            }
        }
        (to_open, to_close)
    };
    if !to_close.is_empty() {
        crate::civ::diag::note("monitor pool: closing daemon(s) — a recycle drops+unkeys them");
        let mut p = pool.lock().unwrap_or_else(|e| e.into_inner());
        p.retain(|c| !to_close.contains(&c.id)); // drop kills each daemon
        {
            let mut e = engine_lock(engine);
            for id in &to_close {
                e.forget_radio_live(*id);
            }
        }
    }
    for (id, t) in to_open {
        let (rig, proc, ok) = open_monitor(&t); // slow (spawn) — pool lock NOT held
        {
            let mut e = engine_lock(engine);
            e.observe_radio_cat(id, ok);
        }
        // Exponential backoff on a failed open: 1 s, 2 s, 4 s … capped at 60 s, so
        // an unreachable radio settles to one probe a minute instead of one every
        // 850 ms. A SUCCESSFUL open clears it, so a radio that comes back on line
        // is adopted at the next reconcile.
        let prior_failures = {
            let p = pool.lock().unwrap_or_else(|e| e.into_inner());
            p.iter().find(|c| c.id == id).map_or(0, |c| c.open_failures)
        };
        let (open_failures, retry_after_ms) = if rig.has_control() {
            (0, 0.0)
        } else {
            let n = prior_failures.saturating_add(1);
            let wait = (1000.0_f64 * 2.0_f64.powi(n.min(6) as i32 - 1)).min(60_000.0);
            if n == 1 || n % 8 == 0 {
                crate::civ::diag::note(&format!(
                    "monitor radio {id}: CAT open failed ({n}x) — retrying in {:.0}s",
                    wait / 1000.0
                ));
            }
            (n, now_ms + wait)
        };
        let mut p = pool.lock().unwrap_or_else(|e| e.into_inner());
        // A handoff may have inserted this id meanwhile (old active → pool); don't double-open.
        if !p.iter().any(|c| c.id == id) {
            p.push(MonitorConn {
                id,
                transport: t,
                rig,
                rigctld_proc: proc,
                last_poll: 0.0,
                ticks: 0,
                smeter_supported: None,
                freq_misses: 0,
                open_failures,
                retry_after_ms,
            });
        }
    }
}

/// Poll each monitor connection read-only into the engine's per-radio live cache. Dial every poll;
/// mode + S-meter every 3rd. Holds the pool lock during the (short-timeout) reads — a concurrent
/// handoff uses `try_lock` and simply retries next tick, so the active audio/TX loop never blocks.
fn poll_monitors(
    pool: &MonitorPool,
    active: u32,
    engine: &Arc<Mutex<Engine>>,
    pending: &std::sync::atomic::AtomicBool,
) {
    let now = now_unix_ms();
    let mut p = pool.lock().unwrap_or_else(|e| e.into_inner());
    // Poll only the SINGLE most-overdue monitor per call, so the pool lock is held for one read
    // burst rather than all of them (each read is bounded by the rig deadline — up to the SLOW
    // 2.5 s one for daemon-backed rigs). A concurrent handoff try_locks AND raises `pending`,
    // which pauses these polls entirely, so a switch waits out at most one in-flight read.
    let conn = match p
        .iter_mut()
        .filter(|c| c.id != active && now - c.last_poll >= MONITOR_POLL_MS)
        .min_by(|a, b| {
            a.last_poll
                .partial_cmp(&b.last_poll)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
        Some(c) => c,
        None => return,
    };
    {
        conn.last_poll = now;
        conn.ticks = conn.ticks.wrapping_add(1);
        match conn.rig.read_freq() {
            Ok(hz) => {
                conn.freq_misses = 0;
                {
                    let mut e = engine_lock(engine);
                    e.observe_radio_freq(conn.id, hz);
                    e.observe_radio_cat(conn.id, Some(true));
                }
                if pending.load(std::sync::atomic::Ordering::Relaxed) {
                    return; // a switch just started — release the pool after the one read
                }
                if conn.ticks % 3 == 0 {
                    if let Some(mm) = conn.rig.read_mode() {
                        {
                            let mut e = engine_lock(engine);
                            e.observe_radio_mode(conn.id, mm);
                        }
                    }
                    if conn.smeter_supported != Some(false) {
                        match conn.rig.read_smeter_db() {
                            Some(db) => {
                                conn.smeter_supported = Some(true);
                                {
                                    let mut e = engine_lock(engine);
                                    e.observe_radio_smeter(conn.id, db);
                                }
                            }
                            None if conn.smeter_supported.is_none() => {
                                conn.smeter_supported = Some(false);
                            }
                            None => {}
                        }
                    }
                }
            }
            Err(_) => {
                // Debounced: one slow/failed poll is routine on a busy CI-V link; only a
                // STREAK means the radio is really unreachable (the flashing-pill fix).
                conn.freq_misses = conn.freq_misses.saturating_add(1);
                if conn.freq_misses >= 3 {
                    {
                        let mut e = engine_lock(engine);
                        e.observe_radio_cat(conn.id, Some(false));
                    }
                }
            }
        }
    }
}

/// If the operator switched the active radio, HAND OFF between the active Rig and the monitor pool:
/// take the (already-connected) new active out of the pool into the active slot, and push the old
/// active back into the pool. No teardown, no reconnect — so the dial can't race back to the old rig.
/// Non-blocking: if the monitor thread holds the pool (mid-poll), retry next 20 ms tick.
fn handoff_if_switched(
    engine: &Arc<Mutex<Engine>>,
    pool: &MonitorPool,
    rig: &mut Rig,
    state: &mut RadioLoop,
    last_active: &mut u32,
    pending: &std::sync::atomic::AtomicBool,
) {
    use std::sync::atomic::Ordering;
    let (active, want_active) = {
        let e = engine_lock(engine);
        let s = e.settings();
        (s.active_radio, Transport::from_settings(s))
    };
    if active == *last_active {
        // No switch in flight (or the intent vanished before the handoff won the pool —
        // operator flipped back / band-routing bounced): the deferral guard protects only
        // the switch currently in flight, so it must vanish with the intent.
        state.handoff_deferred = false;
        pending.store(false, Ordering::Relaxed);
        return;
    }
    // Switch in flight: pause the monitor thread's pool work so this handoff isn't
    // queued behind a multi-second monitor read burst (cleared on every exit below).
    pending.store(true, Ordering::Relaxed);
    // FIX #1 (TX-safety): unkey the OUTGOING rig if it's keyed BEFORE it leaves the active slot into
    // the READ-ONLY monitor pool — otherwise it would sit there with PTT still asserted (a stuck
    // carrier that nothing ever drops). `set_active_radio` cleared the ENGINE's TX intent (halt_tx);
    // this drops the PHYSICAL PTT, which only the loop thread can command. Mirrors step()'s
    // unkey-before-teardown guard.
    // UNCONDITIONAL (root-cause fix): the client-side flags can desync from the radio
    // (a failed unkey used to clear them), and a keyed radio demoted into the read-only
    // pool is unrecoverable there. One idempotent key-up per switch is cheap insurance.
    // Once per SWITCH INTENT, not per deferred retry tick (each retry is a 20 ms-cadence
    // try_lock; re-unkeying every retry adds CAT round-trips that stretch the retry past the
    // monitor's lock-free gaps). Still re-runs if anything keyed the rig mid-deferral.
    if !state.handoff_deferred || rig.keyed || state.tx_until_ms.is_some() {
        crate::civ::diag::note(
            "dual-radio handoff: unkeying the outgoing rig before it leaves the active slot",
        );
        let _ = rig.ptt(false);
        let _ = rig.stop_morse();
        state.tx_until_ms = None;
        state.tuning_keyed = false;
        state.manual_ptt_applied = false;
        state.tune_started_ms = None; // a stale tune clock would auto-cancel the NEXT tune
                                      // Own the switch's TX cut COMPLETELY. `set_active_radio`→`halt_tx` armed the engine's
                                      // one-shot `cw_abort`/`rtty_abort` for the audio loop to act on; the physical unkey above
                                      // IS that action for the outgoing rig. Drain them here so step() — which runs AFTER this
                                      // handoff every tick and is otherwise blind to the deferral — doesn't re-issue a SECOND
                                      // `stop_morse`/`ptt(false)` to the outgoing rig on the same switch. Without this, a
                                      // contended switch (the pool held by the monitor's read burst — the steady state with two
                                      // same-model Icoms) double-commands the old rig's CAT link: exactly the "commands the old
                                      // rig … once per retry tick" isolation failure. Also stop the hardware keyers now, like the
                                      // shutdown unkey, so a mid-CW/RTTY switch doesn't keep keying after the abort is consumed.
        {
            let mut e = engine_lock(engine);
            let _ = e.take_cw_abort();
            let _ = e.take_rtty_abort();
            // Same for SSTV: this handoff already unkeyed (above), so consume the abort a
            // switch-time halt raised — else step()'s SSTV block issues a SECOND ptt(false)
            // to the outgoing rig (the "once per retry tick" double-command regression).
            let _ = e.take_sstv_abort();
        }
        #[cfg(feature = "serial")]
        if let Some((_, wk)) = state.winkeyer.as_mut() {
            let _ = wk.clear();
        }
        #[cfg(feature = "serial")]
        if let Some((_, _, k)) = state.rtty_keyer.as_ref() {
            k.clear();
        }
    }
    let mut p = match pool.try_lock() {
        Ok(p) => p,
        // FIX #4: recover a poisoned pool (like poll/reconcile do) — else every future switch would be
        // silently lost. WouldBlock = monitor mid-poll → retry next tick (never stall the audio loop).
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            // Monitor mid-poll: retry next tick — and tell step() to SKIP its rig_differs
            // rebuild until the handoff has had its chance, else it tears down/reopens the
            // new radio while its monitor conn still owns the serial port (a bind race).
            state.handoff_deferred = true;
            return;
        }
    };
    state.handoff_deferred = false;
    // The monitor's `from_profile` conn transport zeroes the broker port; compare CAT fields against a
    // broker-stripped `want` so the broker being on doesn't spuriously fail the match (FIX #3: adopt
    // ONLY a conn whose CAT config matches what we now want — a stale conn is dropped + reopened).
    let mut want_cat = want_active.clone();
    want_cat.broker_self_port = None;
    // Adopt ONLY a LIVE conn: a monitor whose rigctld failed to bind / whose CAT probe never connected
    // is parked in the pool as a `Rig::vox()` (no control channel — see `open_monitor`). Adopting that
    // dead conn would install a control-less rig as the active radio, and because `state.applied` is
    // then set to its transport, step()'s `rig_differs` stays false and NEVER rebuilds it → the radio's
    // CAT is permanently dead after the switch. Requiring `has_control()` makes a dead conn fall through
    // to the fallback branch, which drops it and lets step()'s `rig_differs` reopen the radio FRESH via
    // `open_cat` (no is_alive gate, self-healing) — exactly how the startup radio stays healthy.
    if let Some(idx) = p.iter_mut().position(|c| {
        c.id == active
            && c.rig.has_control()
            // Mirror reconcile's keep-gate: a live TCP cache over a DEAD daemon is a zombie —
            // adopting it installs dead CAT as the active radio with `applied` matching, so
            // rig_differs would never rebuild it. Refuse → the fallback drops it + reopens fresh.
            && c.rigctld_proc.as_mut().is_none_or(CatDaemon::is_alive)
            && !c.transport.rig_differs(&want_cat)
    }) {
        let conn = p.remove(idx);
        let mut old_rig = std::mem::replace(rig, conn.rig);
        // The adopted rig was opened READ-ONLY by the monitor (`PttMode::Vox`); give it the active
        // radio's REAL PTT mode so it can key (else `ptt()` no-ops → "TX dead after switching to the
        // FTDX10"). The demoted radio goes back to Vox — a monitor must never key.
        rig.set_ptt_mode(ptt_mode_for(&want_active));
        // Unkey-on-adopt: the radio may be PHYSICALLY keyed from a previous wedge (the
        // fresh Rig starts keyed=false and would never know). Now that this rig has
        // control + a real PTT mode, one idempotent key-up puts the newly active radio
        // in a known-unkeyed state — Session 2's "light stays lit after switching".
        let _ = rig.ptt(false);
        old_rig.set_ptt_mode(PttMode::Vox);
        let old_proc = state.rigctld_proc.take();
        // The demoted radio becomes a monitor: stop its scope stream (the waveform would
        // crowd the monitor's slow poll off the serial link). The adopted radio's stream
        // is enabled by the active loop's per-tick drain.
        if let Some(d) = old_proc.as_ref().and_then(CatDaemon::native) {
            d.set_scope_enabled(false);
        }
        let mut old_transport = std::mem::replace(&mut state.applied, conn.transport);
        // Monitor conns always carry `broker_self_port = None` (`from_profile`); strip it off the
        // demoted radio's transport too, so the monitor `reconcile` doesn't see `rig_differs` (which
        // compares broker port) and needlessly tear down + reopen the radio we just demoted.
        old_transport.broker_self_port = None;
        state.rigctld_proc = conn.rigctld_proc;
        // The ACTIVE radio DOES interact with the CAT broker — set its broker port to the live value so
        // `rig_differs` won't see a diff and tear the just-handed-off rig back down. (Audio fields stay
        // zeroed → `audio_differs` fires → the RX codec rebuilds to the new radio, the one device swap.)
        state.applied.broker_self_port = want_active.broker_self_port;
        {
            let mut e = engine_lock(engine);
            e.forget_radio_live(active);
        }
        // The new active rig is ALREADY connected + on its own frequency; reset the per-rig caches so
        // step()'s retune re-asserts the restored dial/mode and the health/capability re-probe runs.
        state.reset_for_handoff();
        // The old active radio joins the monitor pool (stays live); the new active leaves it.
        p.push(MonitorConn {
            id: *last_active,
            transport: old_transport,
            rig: old_rig,
            rigctld_proc: old_proc,
            last_poll: 0.0,
            ticks: 0,
            smeter_supported: None,
            freq_misses: 0,
            open_failures: 0,
            retry_after_ms: 0.0,
        });
        *last_active = active;
    } else {
        // Fallback: no MATCHING live conn for the new active (never opened / model 0 / a stale conn from
        // a config change). Drop any stale conn for this id so its daemon is reaped + its port freed,
        // then let step()'s `rig_differs` path open the new active fresh (it also unkeys + tears down
        // the OLD active safely). The old active is not kept monitored in this edge — steady state
        // (both radios configured) always ADOPTS above. A switch during a radio's very first monitor
        // open can transiently coexist onto the monitor daemon; it self-heals on the next reconcile.
        p.retain(|c| c.id != active);
        {
            let mut e = engine_lock(engine);
            e.forget_radio_live(active);
        }
        // The active radio changed — force the RX audio to rebuild to the new radio's device even if
        // step()'s rig_differs path handles the CAT (audio_differs alone can miss an empty-vs-empty).
        state.force_audio_rebuild = true;
        *last_active = active;
    }
    pending.store(false, Ordering::Relaxed);
}

/// The network outputs the loop emits to, borrowed for the loop's lifetime.
struct Sinks<'a> {
    wsjtx: Option<&'a WsjtxServer>,
    psk: Option<&'a PskReporter>,
    /// Startup dial (Hz) reported as the QSO-logged TX frequency.
    cfg_dial_hz: u64,
}

/// SSTV TX working rate (12 kHz = `tempo_fast::SAMPLE_RATE`): the image is synthesized
/// directly at the modem rate, so no resample is needed on the way to the backend.
const SSTV_TX_RATE_HZ: f64 = 12_000.0;
/// Chunk size for the SSTV look-ahead feed: ~2 s at 12 kHz.
const SSTV_CHUNK_SAMPLES: usize = 24_000;
/// How far ahead of playback we keep the SSTV output ring filled (ms). Bounds the
/// unbounded `out_ring`: a 10 s look-ahead caps a PD290 at ~2 MB queued instead of the
/// ~55 MB a one-shot `play` of the whole image would peak, and survives multi-second
/// loop stalls (shared CAT reads) without underrunning.
const SSTV_FEED_AHEAD_MS: f64 = 10_000.0;

/// The SSTV image currently streaming to the rig: the whole pre-encoded 12 kHz buffer,
/// a feed cursor (how many samples have been handed to the backend), and timing.
struct SstvFeed {
    /// Full over-the-air waveform (12 kHz `f32` PCM).
    samples: Vec<f32>,
    /// Next sample index to feed to the backend.
    cursor: usize,
    /// Loop-clock ms when the image started keying.
    started_ms: f64,
    /// Exact total duration of `samples` (ms) — the PTT hold and progress denominator.
    total_ms: f64,
}

/// All persistent state of the radio loop. One iteration is [`RadioLoop::step`],
/// generic over [`AudioBackend`] so a `MockBackend` (+ a `Rig::vox()` / mock
/// rigctld) can drive the whole heartbeat in a test with no sound card.
/// Owner of the single audio-error status line (see `err_owner`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ErrOwner {
    None,
    Device,
    Monitor,
    VoiceMic,
    /// The rig rejected a TX key command (PTT NAK/timeout) — otherwise we'd play modem
    /// audio into a receiving rig with no warning ("silent dead air").
    Ptt,
    /// Native Flex DAX RX audio was selected but no audio is arriving — otherwise the
    /// operator is simply deaf, with silence indistinguishable from a dead band.
    Dax,
}

/// How long native DAX RX may deliver NOTHING before we call it broken, fall back to the
/// sound card and say so.
///
/// Why a fallback and not just an error: when `dax_src` is `Some`, the loop takes DAX audio
/// INSTEAD of the sound card. If the Flex never streams — wrong IP, firewall, the slice never
/// bound, DAX disabled on the radio — `take_audio()` returns empty forever and the operator
/// hears NOTHING, with no error anywhere. Deafness is a worse failure than losing the native
/// path, so DAX starvation degrades to the sound card exactly like a CAT failure degrades to
/// direct keying (`open_serial_ptt`'s TX floor).
///
/// 6 s is comfortably longer than a stream create + slice bind round-trip (which the control
/// thread does in well under a second) but short enough that the operator is not left guessing
/// through a whole QSO.
const DAX_STARVE_AFTER: Duration = Duration::from_secs(6);

/// The persistent decode worker: one background thread that runs the heavy per-slot
/// decode ([`tempo_app::engine::run_decode_job`]) OFF the radio-loop thread and OFF
/// the engine mutex. The loop builds an owned job under the engine lock, sends it
/// here, keeps ticking (feeding the waterfall), and drains the result on a later
/// tick — so the ~1–2 s decode never freezes the UI or the waterfall.
///
/// The worker touches NO engine state: everything it needs (including an `Arc` clone
/// of the decoder) travels in the job. Created once per loop; the [`Drop`] closes the
/// job channel (ending the worker's `for` loop) and joins the thread for a clean exit.
struct DecodeWorker {
    /// `Option` only so [`Drop`] can drop the sender first, then join.
    job_tx: Option<Sender<DecodeJob>>,
    result_rx: Receiver<DecodeResult>,
    handle: Option<JoinHandle<()>>,
}

impl DecodeWorker {
    fn spawn() -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<DecodeJob>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<DecodeResult>();
        let handle = std::thread::Builder::new()
            .name("nexus-decode".into())
            .spawn(move || {
                // Ends when the job sender drops (loop shutdown / RadioLoop drop).
                for job in job_rx {
                    let result = tempo_app::engine::run_decode_job(job);
                    if result_tx.send(result).is_err() {
                        break; // loop went away
                    }
                }
            })
            .expect("spawn decode worker");
        Self {
            job_tx: Some(job_tx),
            result_rx,
            handle: Some(handle),
        }
    }

    /// Hand a job to the worker. Silently drops if the worker is gone (shutdown).
    fn dispatch(&self, job: DecodeJob) {
        if let Some(tx) = &self.job_tx {
            let _ = tx.send(job);
        }
    }

    /// Non-blocking: take the next completed result, if one is ready.
    fn try_recv(&self) -> Option<DecodeResult> {
        self.result_rx.try_recv().ok()
    }
}

impl Drop for DecodeWorker {
    fn drop(&mut self) {
        // Close the job channel so the worker's `for job in job_rx` ends, then join.
        self.job_tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Record of a boundary whose TX decision already ran at t=0 (see
/// `RadioLoop::boundary_keyed`).
#[derive(Clone, Copy)]
struct KeyedBoundary {
    slot: u64,
    /// Whether the boundary decision actually transmitted (feeds the deferred
    /// WSJT-X status emission's `decoding`/`transmitting` phases).
    tx_this_slot: bool,
    /// Dial (Hz) captured BEFORE keying — Split Operation may move the TX dial, and
    /// the status emission must report the pre-shift RX dial exactly as the
    /// deferred path always has.
    dial_hz: u64,
}

/// Station-wide loop state — the fields that must exist **once per station**, never
/// once per radio.
///
/// Extracted from `RadioLoop` ahead of multi-radio (Phase 1): everything else in that
/// struct is genuinely per-radio and becomes a `RadioChain`, but duplicating THESE
/// across chains would double-report. Concretely:
/// * `psk_spots`/`last_psk_flush` — one PSK Reporter identity and one flush cadence for
///   the station; every chain fans its spots into this one buffer (each spot already
///   carries its own dial via `emit_rx_decodes`).
/// * the Field Day cursor + N3FJP band-board state — a per-chain copy would push every
///   FD QSO to the club network (and the WSJT-X sink) once per radio.
///
/// Threaded into `step` by reference so the boundary is enforced by the compiler rather
/// than by discipline.
struct StationSinks {
    /// PSK Reporter spot buffer, flushed on `PSK_FLUSH_SECS`.
    psk_spots: Vec<Spot>,
    last_psk_flush: f64,
    /// Field Day log cursor — how many FD QSOs have already been pushed to the club
    /// network / WSJT-X sinks.
    last_fd_qsos: usize,
    /// Last time (loop ms) we reported our band to the N3FJP club board, so the
    /// no-CAT band report fires on a coarse heartbeat, not every slot boundary.
    last_reported_band: f64,
    /// The last "band|mode" reported to N3FJP, so a band/mode change reports
    /// immediately (between heartbeats). Empty until the first report.
    last_reported_bm: String,
    /// Whether the previous boundary saw a live FD session — the None→Some
    /// edge seeds `last_fd_qsos` past the restored journal rows so they are
    /// never re-pushed to the club network / WSJT-X sinks as newly logged.
    fd_was_active: bool,
}

impl StationSinks {
    fn new() -> Self {
        Self {
            psk_spots: Vec::new(),
            last_psk_flush: now_unix_ms(),
            last_fd_qsos: 0,
            last_reported_band: now_unix_ms(),
            last_reported_bm: String::new(),
            fd_was_active: false,
        }
    }
}

struct RadioLoop {
    cur_tier: Tier,
    /// The slot period the clock + capture ring were BUILT for. Tracked alongside
    /// the tier because tier alone no longer determines it: Q65, FST4/FST4W and
    /// MSK144 take their T/R period from Settings, so a save can change the period
    /// with the tier unchanged. Keying the rebuild on the tier only left the clock
    /// running the OLD period — 30 s slots against a 60 s Q65 decode.
    cur_slot_secs: f64,
    clock: SlotClock,
    rx: RxRing,
    last_slot: Option<u64>,
    /// Whether the slot we just finished was one we TRANSMITTED in. Gates the RX
    /// decode: we decode the slot that just ended UNLESS we transmitted in it (the
    /// capture ring then holds our own carrier). Tying the decode to the *previous*
    /// slot — not whether we're about to TX in the new one — is what lets stations
    /// in the RX slots BETWEEN our transmissions get decoded while calling CQ.
    prev_slot_was_tx: bool,
    tx_until_ms: Option<f64>,
    tuning_keyed: bool,
    /// Was the operator in a DATA mode (FT8/PKTUSB → DATA-U) when this tune started? The Icom
    /// tune keys in DATA mode regardless; on release we restore THIS state, not a hardcoded OFF —
    /// else an FT8 operator gets dropped from DATA-U to plain USB.
    tune_was_data: bool,
    tune_phase: f32,
    tune_started_ms: Option<f64>,
    applied: Transport,
    /// Set when a handoff bailed on the pool lock: step() skips ONE rig_differs rebuild
    /// tick so the handoff (not a fresh spawn racing the monitor's port) wins.
    handoff_deferred: bool,
    rigctld_proc: Option<CatDaemon>,
    /// Test CAT's baud-ladder probe is holding the CAT serial port (`Engine::cat_port_hold`):
    /// our daemon + control channel are dropped for the duration; the falling edge forces a
    /// rebuild through the rig_differs branch.
    cat_hold_active: bool,
    last_dial: u64,
    last_mode: String,
    /// The mode last commanded onto the SPLIT (TX) VFO, mirroring `last_mode` for
    /// the RX side. Only a satellite pass sets one today: an inverting linear
    /// transponder transmits in the opposite sideband to the one it is heard on
    /// (`Engine::sat_tx_mode_for_split` — answered only when the split being
    /// applied is the sat's own corrected uplink, never for a terrestrial
    /// pile-up split). Tracked so the mode is written when the ANSWER
    /// changes and never re-asserted every cycle — the split VFO's mode cannot
    /// be read back, so re-asserting would silently fight an operator using the
    /// rig's own mode knob. `None` = we are not holding the TX VFO's mode.
    last_split_mode: Option<String>,
    /// The applied split rides the SUB BAND (the engine answered "Sub" and the
    /// rig took it — the IC-9700's satellite mode, engaged by the native
    /// backend). Remembered so the TEARDOWN releases the same thing it
    /// engaged: `set_split(false, "Sub")` leaves satellite mode, where the old
    /// unconditional `"VFOA"` fired `0F 00` at a rig that was never in an A/B
    /// split and left it in satellite mode forever.
    split_on_sub: bool,
    /// Consecutive failed `set_mode` attempts for the current target mode. Bounds the
    /// retune retry so a rig that flatly rejects a mode (e.g. no DATA/PKT submode)
    /// gets a budget of tries (covers a rig/rigctld still settling) then we give up
    /// instead of spamming the CAT link every loop. Reset to 0 once a mode-set sticks.
    mode_fail_count: u32,
    /// The target mode we GAVE UP retrying (rig kept rejecting it). Suppresses further
    /// `set_mode` of exactly this mode WITHOUT corrupting `last_mode` (which tracks the
    /// last mode actually applied). Cleared on any successful set_mode, so a later
    /// section change that re-selects this mode (after a different mode succeeded) tries
    /// again. `None` = nothing suppressed.
    mode_giveup: Option<String>,
    /// Whether any failure in the CURRENT mode-retry run was an active rig REJECTION
    /// (`RPRT -1` → `ErrorKind::Other`) rather than a link fault (timeout/refused).
    /// Decides the give-up outcome: a rejection means the rig really refused the mode
    /// (→ try the plain-sideband fallback, tell the operator to press DATA); all link
    /// faults mean the CAT link is too slow or mute — claiming "rig has no mode" there
    /// sent an IC-7610 @ 19200 baud operator chasing a mode the rig has always had.
    mode_saw_reject: bool,
    /// Last CW keyer speed (WPM) pushed to the rig, so we only `set_keyspd` on change.
    last_cw_wpm: u32,
    /// Unix-ms until which the current CW word is still keying — the next queued word is
    /// held until then, so at most one word sits in the rig's keyer buffer (Stop TX drops
    /// the rest). 0.0 = idle / ready to send now.
    cw_busy_until: f64,
    /// Last FM repeater config (shift, offset Hz, CTCSS Hz) applied — so the shift/offset/
    /// CTCSS commands only fire on change, not every loop. `None` when not in FM.
    last_fm: Option<(String, i64, f32)>,
    /// The open WinKeyer keyer (port + handle) when the CW backend is WinKeyer — opened
    /// on demand, reopened if the configured port changes.
    #[cfg(feature = "serial")]
    winkeyer: Option<(String, crate::winkeyer::WinKeyer)>,
    /// The open serial DTR/RTS keyline keyer (port + line + handle) when the CW backend is
    /// Serial — opened on demand, reopened if the configured port or line changes.
    #[cfg(feature = "serial")]
    serial_keyer: Option<(String, String, crate::serial_keyer::SerialKeyer)>,
    /// Unix-ms until which the current RTTY message is still keying — the next queued
    /// message is held until then (poll pacing), and the PTT drop rides `tx_until_ms`.
    /// 0.0 = idle / ready to send now.
    rtty_busy_until: f64,
    /// The open true-FSK keyline keyer (port + line + handle) when the RTTY backend is
    /// FSK — opened on demand, reopened if the configured port or line changes, dropped
    /// (line back to mark) when the operator switches to AFSK.
    #[cfg(feature = "serial")]
    rtty_keyer: Option<(String, String, crate::rtty_fsk::FskKeyer)>,
    /// The SSTV image currently streaming to the rig (pre-encoded 12 kHz PCM + a feed
    /// cursor + timing), fed to the output ring in chunked look-ahead slices so a
    /// multi-minute image never dumps into the unbounded ring at once. `None` = no
    /// image in flight. PTT is held for the whole image via `tx_until_ms`.
    sstv_feed: Option<SstvFeed>,
    /// Last manual-PTT (live phone) state we applied to the rig — only key on change.
    manual_ptt_applied: bool,
    /// Last RF power fraction we pushed to the rig — only set on change.
    last_rf_power: Option<f32>,
    /// Last mic-gain fraction we pushed to the rig — only set on change.
    last_mic_gain: Option<f32>,
    /// Last NR level / AGC speed we pushed to the rig — only set on change.
    last_nr_level: Option<f32>,
    last_agc: Option<String>,
    /// Open WAV sink while a QSO recording is streaming live RX capture to disk (audio
    /// bridge). The loop owns the file handle so the audio never has to live in RAM.
    qso_sink: Option<crate::voice::WavSink>,
    /// When the in-progress QSO recording started (loop ms), for the max-duration auto-stop.
    qso_started_ms: Option<f64>,
    /// A transient voice-mic input stream is live and feeding the recorder (see
    /// `voice_mic_device`). Toggled on the recording session's rising/falling edge.
    voice_mic_open: bool,
    /// Retry suppression for a failed mic open — cleared when the recording
    /// ends so the NEXT recording tries the device again (not per-loop spam).
    voice_mic_failed: bool,
    /// Nudge: re-evaluate the monitor block next loop even without a settings
    /// change (used when the voice-mic notice cleared a line the monitor may
    /// still be entitled to — its guard/failure state gets re-surfaced).
    monitor_reapply: bool,
    /// One-shot: force the RX-audio backend to rebuild on the next tick even if `audio_differs` is
    /// false. Set by a dual-radio handoff — the new radio's audio device MUST be (re)opened, and a
    /// radio whose audio is "system default" (empty) would otherwise compare equal to another empty
    /// and skip the rebuild, leaving the OLD radio's sound-card stream running (the "audio never
    /// leaves the FTDX10" bug). Consumed (taken) in the step() audio-rebuild guard.
    force_audio_rebuild: bool,
    /// The NATIVE RF panadapter worker (Flex SmartSDR VITA / Icom CI-V) for the ACTIVE radio, if
    /// it has one. Reconciled each step from `native_spectrum_kind(want)`: started when the active
    /// radio gains a native scope, dropped (threads stopped + pan removed) when it loses it or the
    /// operator switches to a non-native rig. `None` = the universal audio-FFT scope. Inert unless
    /// a Flex is the active radio with `flex_radio_ip` set.
    spectrum_src: Option<crate::flexspectrum::FlexSpectrum>,
    /// The (radio-model, network?) key the current `spectrum_src` was started for, so a switch to a
    /// different native-scope rig tears down + restarts it, and same-radio ticks are a no-op.
    spectrum_src_key: Option<(u32, bool)>,
    /// Native FlexRadio DAX RX audio worker (Phase 2). `Some` only while `flex_native_audio` is on
    /// and a network Flex is active; its 12 kHz audio then replaces the soundcard as the RX source.
    /// Opt-in + unverified-on-hardware, exactly like `spectrum_src`.
    dax_src: Option<crate::flexdax::FlexDax>,
    /// The key the current `dax_src` was started for (same tear-down/no-op discipline as spectrum).
    dax_src_key: Option<(u32, bool)>,
    /// Whether the DAX TX-audio tee is currently installed in the backend — installed when `dax_src`
    /// starts, cleared when it stops, so TX audio routes over DAX exactly while native audio is on.
    dax_tee_set: bool,
    /// When the current `dax_src` started, for the starvation check. `None` once starvation has
    /// been reported (the check is one-shot per source — it must not re-fire every tick).
    dax_started: Option<Instant>,
    /// Has the current `dax_src` EVER delivered a sample? Once true the source is proven and the
    /// starvation check is done for good; a later quiet band is just a quiet band.
    dax_saw_audio: bool,
    /// We wrote the current audio-error line with a voice-mic open failure, so we clear
    /// Slot index whose WSJT-X-style EARLY decode pass already ran (once per
    /// RX slot; the boundary decode then ingests only the stragglers).
    early_done_slot: Option<u64>,
    /// A slot whose boundary TX decision already ran AT the boundary (the WSJT-X
    /// key-at-boundary ordering, taken when the just-ended slot's early decode had
    /// folded): the slot, whether it actually keyed, and the pre-key dial for the
    /// deferred status emission. `finish_boundary` consults this so the straggler
    /// decode's drain runs housekeeping ONLY — keying again would double-transmit
    /// the slot. Never cleared per-slot: slots are monotonic, so a stale entry can
    /// never match a future boundary; reset on a tier switch (new slot numbering).
    boundary_keyed: Option<KeyedBoundary>,
    /// Read-only launch latch: has the rig's mode been COMMANDED (asserted) this
    /// session? While `false`, `ensure_commanded` pushes dial/mode immediately before
    /// any key-up — closing the silent-no-op-transmit hole that removing the launch
    /// commands would otherwise open (FT8 tones into a rig left in LSB). `true` from
    /// construction until the final flip lands, which makes every call a no-op — the
    /// latch machinery ships inert first, per the plan.
    rig_asserted: bool,
    /// This tick's effective dial/mode policy (stashed where step derives them, read by
    /// `ensure_commanded` at the key sites, which sit in narrower scopes).
    cur_dial: u64,
    cur_md: String,
    /// Fake-It split moved the VFO for the playing over — restore THIS dial
    /// (Hz) when the over ends (PTT drop / hard stop).
    fake_it_restore: Option<u64>,
    /// An audio Rig-mode split engaged VFO B for an over — tear the rig split
    /// down once no over is pending (unless the cluster split owns VFO B).
    audio_rig_split: bool,
    /// Last time we ran the FULL rig read-back (dial + RF power + S-meter + mode + funcs), ms.
    last_rig_poll: f64,
    /// Last time we read the TRANSMIT meters (ms). 0.0 when the bars are blanked (not keyed), so
    /// the first keyed tick reads immediately and unkey clears them exactly once.
    last_tx_meter_poll: f64,
    /// Round-robin index over the four TX meters (SWR/ALC/Po/COMP) — one read per throttled
    /// cycle, so a slow rig can never block the loop with four back-to-back reads.
    tx_meter_idx: usize,
    /// Last time we ran the FAST dial-only read-back (ms). The dial is mirrored on a much shorter
    /// cadence than the heavy reads so a manual VFO-knob turn tracks like HRD (~⅕ s), not the
    /// 750 ms health poll — the heavy reads (S-meter/mode/funcs) stay slow to bound CAT traffic.
    last_freq_poll: f64,
    /// Consecutive HEAVY-poll dial-read failures. The CAT breaker only trips after a few in a row
    /// (not a single miss) so one legitimately-slow reply — a band-stack switch, a USB-serial
    /// latency spike — doesn't permanently disable read-back. Reset to 0 on any successful read.
    freq_misses: u32,
    /// Last known CAT health (from connect/Test-CAT): `Some(false)` = configured but failing,
    /// so we skip the read-back poll to avoid blocking the loop on a dead read every cycle.
    cat_ok: Option<bool>,
    /// When (ms, loop clock) a tripped CAT breaker may try ONE probe read again.
    ///
    /// ⚠️ THE BUG THIS EXISTS FOR: `cat_ok = Some(false)` used to be a permanent latch. It gates
    /// both read-back paths, and the only thing that cleared it was a successful `set_freq`/
    /// `set_mode` from the retune block — which does not fire while the commanded dial and mode
    /// already equal `last_dial`/`last_mode`. So a link that came back stayed dead for the rest of
    /// the session: proven by driving 40 loop ticks against a perfectly healthy rigctld after a
    /// trip and observing ZERO commands on the wire. The breaker's job is to stop the loop
    /// blocking on a dead read EVERY cycle — that is rate-limiting, not a one-way door.
    cat_retry_at: f64,
    /// Current breaker re-probe interval (ms), doubling on each failed retry to
    /// [`CAT_RETRY_MAX_MS`]. A genuinely dead link settles at one cheap timeout per ~30 s
    /// instead of one per tick; a link that recovers is picked up within seconds.
    cat_retry_ms: f64,
    /// A dial frequency the rig REFUSED (`RPRT <negative>`) — do not keep re-sending it. Mirrors
    /// `mode_giveup`: the operator's HF-only radio cannot be talked into covering 2 m by asking
    /// 8 times a second. Cleared by an explicit operator retune (the force branch) or any
    /// successful dial set.
    dial_giveup: Option<u64>,
    /// Consecutive refusals of the currently-commanded dial, against [`DIAL_SET_MAX_TRIES`].
    dial_fail_count: u32,
    /// RX frequency ranges (Hz) read from the rig's Hamlib capability table once per CAT
    /// confirmation. `None` = not probed yet or unknown (must fail OPEN — see
    /// [`crate::rig::Rig::read_rx_ranges`]).
    rx_ranges: Option<Vec<(u64, u64)>>,
    /// Whether the range probe has been attempted for the current CAT confirmation, so an
    /// unsupported `\dump_state` costs one round-trip per rig — not one per poll.
    rx_ranges_probed: bool,
    /// Lazy S-meter capability: `None` = not yet probed, `Some(true)` = rig reports
    /// STRENGTH (keep polling it), `Some(false)` = rig answered the dial but not
    /// STRENGTH (no CAT S-meter — stop polling it so we don't burn a round-trip every
    /// cycle). Reset to `None` when CAT re-confirms so a rig swap re-probes.
    smeter_supported: Option<bool>,
    /// Consecutive STRENGTH read misses while the dial poll is succeeding, so a single
    /// transient timeout doesn't wrongly declare a capable rig's S-meter unsupported.
    smeter_misses: u8,
    /// Monotonic RX-poll counter, used to sub-cadence the slower CAT reads (mode) and to
    /// periodically re-probe a rig whose S-meter was found unsupported.
    rig_poll_ticks: u32,
    /// Per-func DSP capability ([nb, nr, notch, comp, vox], same as [`RIG_FUNCS`]), mirroring
    /// `smeter_supported`: `None` = unprobed, `Some(true)` = rig reports the func, `Some(false)`
    /// = confirmed absent (stop polling → toggle hidden). Reset on CAT re-confirm / breaker trip.
    func_supported: [Option<bool>; 5],
    /// Consecutive get-miss counters per func — the same miss-tolerance as `smeter_misses`.
    func_misses: [u8; 5],
    /// Last-known func states, mirrored to the engine each sub-cadence poll; a read miss on a
    /// supported func keeps the last value so the toggle never flickers.
    func_state: [Option<bool>; 5],
    /// Earliest `rig_poll_ticks` at which a func latched `Some(false)` may be re-probed, and the
    /// backoff (in heavy polls) applied when it fails again.
    ///
    /// WHY THIS EXISTS (operator report, 2026-07-25 — the waterfall "hangs and stops moving"
    /// for ~1 s every 10-20 s, in Phone/CW/FT, from the first minute). A func GET on a rig that
    /// does not cleanly reject an unsupported func blocks to the CAT deadline (700 ms, 2500 ms
    /// on slow serial) — and it runs on the RADIO LOOP, the sole producer of waterfall rows via
    /// `feed_rx_audio`. Block that thread and no new row is produced, so the UI re-draws
    /// the cached row: the waterfall does not blank, it STREAKS vertically, which is exactly
    /// what the operator's screenshot shows.
    ///
    /// The old recovery re-armed EVERY latched-off func unconditionally every 40 heavy polls
    /// (~30 s), forever. So a func the rig never answers cost a full CAT timeout every 30 s for
    /// the life of the session: three stalls at 15 s spacing → latch off → quiet → re-arm →
    /// repeat. That is the operator's "then it might be fine again, then we get a small lag".
    /// Transient-hiccup recovery is still worth having, so the retry is kept but BACKED OFF
    /// (40 → 80 → 160 … heavy polls, capped), and reset on a successful read.
    func_retry_at: [u32; 5],
    func_retry_backoff: [u32; 5],
    /// Where every spectrum source publishes. Held here so the CI-V native row can be published
    /// WITHOUT the engine mutex — that mutex is held across this loop's own blocking CAT at the
    /// slot boundary, which is what starved the panadapter along with the audio row.
    spectrum_feed: tempo_app::engine::SpectrumFeed,
    /// The wait-free tee the rx-dsp thread drains. Republished on every audio (re)open.
    rx_tap: Arc<crate::rxtap::RxTap>,
    /// Per-extended-level capability ([RFPOWER, MICGAIN, NR, AGC], see the `LVL_*` indices), the
    /// same miss-tolerant caching as `func_supported`: `Some(false)` after 3 get-misses → stop
    /// issuing that read, so a rig slow/silent on it doesn't churn the CAT socket every poll
    /// (the K4/QK4 "hangs up every 5 s" bug). Reset on CAT re-confirm / rig rebuild.
    level_supported: [Option<bool>; 4],
    /// Consecutive get-miss counters per extended level — same tolerance as `smeter_misses`.
    level_misses: [u8; 4],
    /// Whether we last surfaced the "monitor refused — would transmit into the TX
    /// device" note on the audio-error line, so we clear only our OWN message.
    /// The monitor block currently OWNS the audio-error line (it wrote either
    /// the guard refusal or an open failure there). A real device error takes
    /// ownership back; only an owning monitor may clear the line on success.
    /// WHO wrote the shared audio-error line. Three writers (real device
    /// failures, the headphone monitor, the voice mic) previously juggled two
    /// booleans and could stomp/erase each other's notices (review ×3). Rules:
    /// Device is set only by the audio-reopen path and outranks everything;
    /// Monitor/VoiceMic may write only over None or themselves, and clear only
    /// what they own.
    err_owner: ErrOwner,
    /// Latest measured PC-clock-vs-UTC offset (ms, `local − UTC`), read from the
    /// engine each loop and SUBTRACTED from the system clock so TX/RX slots land
    /// on the true UTC grid even when the OS clock is skewed. 0 until measured.
    clock_offset_ms: i64,
    /// The persistent decode worker (heavy decode off this thread + the engine mutex).
    decode: DecodeWorker,
    /// A decode job (early OR boundary) is out on the worker. Guards against a second
    /// dispatch while one is in flight — the boundary defers a tick if the early pass
    /// is still running, so the early result is always folded (setting `early_seen`)
    /// before the boundary decode filters against it. Cleared when a result drains.
    decode_in_flight: bool,
    /// Periods whose decode was dropped because the worker was still busy. Counted
    /// and logged rather than silently swallowed: a rising number is the signal that
    /// the decoder cannot keep up with the T/R period on this hardware.
    dropped_decodes: u64,
}

impl RadioLoop {
    fn new(applied: Transport, rigctld_proc: Option<CatDaemon>, cfg: &RadioConfig) -> Self {
        Self {
            cur_tier: Tier::TempoFast,
            // Rebuilt on the first tick that disagrees; the clock below is
            // constructed from the same source of truth.
            cur_slot_secs: 0.0,
            clock: SlotClock::ft1(),
            rx: RxRing::new(),
            last_slot: None,
            prev_slot_was_tx: false,
            tx_until_ms: None,
            tuning_keyed: false,
            tune_was_data: false,
            tune_phase: 0.0,
            tune_started_ms: None,
            applied,
            rigctld_proc,
            cat_hold_active: false,
            last_dial: cfg.dial_hz,
            last_mode: cfg.mode.clone(),
            last_split_mode: None,
            split_on_sub: false,
            mode_fail_count: 0,
            mode_giveup: None,
            mode_saw_reject: false,
            last_cw_wpm: 0, // 0 = unset → first send pushes the speed
            cw_busy_until: 0.0,
            last_fm: None,
            #[cfg(feature = "serial")]
            winkeyer: None,
            #[cfg(feature = "serial")]
            serial_keyer: None,
            rtty_busy_until: 0.0,
            #[cfg(feature = "serial")]
            rtty_keyer: None,
            sstv_feed: None,
            manual_ptt_applied: false,
            last_rf_power: None,
            last_mic_gain: None,
            last_nr_level: None,
            last_agc: None,
            qso_sink: None,
            qso_started_ms: None,
            voice_mic_open: false,
            voice_mic_failed: false,
            monitor_reapply: false,
            force_audio_rebuild: false,
            spectrum_src: None,
            spectrum_src_key: None,
            dax_src: None,
            dax_src_key: None,
            dax_started: None,
            dax_saw_audio: false,
            dax_tee_set: false,
            err_owner: ErrOwner::None,
            early_done_slot: None,
            boundary_keyed: None,
            rig_asserted: false, // read-only launch: nothing asserted until a real command
            cur_dial: 0,
            cur_md: String::new(),
            fake_it_restore: None,
            audio_rig_split: false,
            last_rig_poll: now_unix_ms(),
            last_tx_meter_poll: 0.0,
            tx_meter_idx: 0,
            last_freq_poll: now_unix_ms(),
            freq_misses: 0,
            cat_ok: None,
            cat_retry_at: 0.0,
            cat_retry_ms: CAT_RETRY_BASE_MS,
            dial_giveup: None,
            dial_fail_count: 0,
            rx_ranges: None,
            rx_ranges_probed: false,
            handoff_deferred: false,
            smeter_supported: None,
            smeter_misses: 0,
            rig_poll_ticks: 0,
            func_supported: [None; 5],
            func_misses: [0; 5],
            func_state: [None; 5],
            func_retry_at: [0; 5],
            func_retry_backoff: [FUNC_RETRY_BACKOFF_BASE; 5],
            spectrum_feed: cfg.spectrum_feed.clone(),
            rx_tap: cfg.rx_tap.clone(),
            level_supported: [None; 4],
            level_misses: [0; 4],

            clock_offset_ms: 0,
            decode: DecodeWorker::spawn(),
            decode_in_flight: false,
            dropped_decodes: 0,
        }
    }

    /// The backend attribution for the CURRENTLY-owned CAT channel, appended to probe and
    /// health messages (see [`cat_backend_label`]). `t` is the transport the channel was
    /// built for (`applied`, or `want` when they compare equal).
    fn live_backend_label(&self, t: &Transport) -> &'static str {
        let native_wanted = native_civ_addr(t).is_some() && !keys_on_the_cat_port(t);
        cat_backend_label(
            native_wanted,
            self.rigctld_proc
                .as_ref()
                .map(|d| matches!(d, CatDaemon::Native(_))),
        )
    }

    /// Start/stop the native RF panadapter worker to match the ACTIVE radio's capability
    /// ([`native_spectrum_kind`]). Cheap when nothing changed (a key compare, no lock); only a
    /// scope-rig transition touches threads. Flex runs as a worker here; the Icom CI-V scope
    /// streams through the radio's own `CatDaemon::Native` (drained right after this call), so
    /// `IcomCiv` needs no worker — an Icom without the native daemon keeps the audio-FFT scope.
    fn reconcile_spectrum_source(
        &mut self,
        engine: &Arc<Mutex<Engine>>,
        rig_model: u32,
        is_network: bool,
    ) {
        use crate::rigmodels::{native_spectrum_kind, SpectrumKind};
        let conn = if is_network { "network" } else { "serial" };
        let kind = native_spectrum_kind(rig_model, conn);
        // Flex's native panadapter is OPT-IN (`flex_native_pan`) — unverified on real hardware
        // until a tester enables it. Read the toggle ONLY when the active radio is actually a
        // scope-capable Flex, so non-Flex users keep the lock-free fast path (the `&&`
        // short-circuits before any lock). Folding it into the key makes toggling take effect
        // on the next tick (key flips Some↔None → the worker starts/stops).
        let flex_enabled = matches!(kind, Some(SpectrumKind::FlexVita))
            && engine_lock(engine).settings().flex_native_pan;
        let key = match kind {
            None => None,
            Some(SpectrumKind::FlexVita) if !flex_enabled => None, // opt-in off → no worker
            Some(_) => Some((rig_model, is_network)),
        };
        // Native DAX RX audio is its OWN opt-in (`flex_native_audio`), independent of the pan — a
        // Flex user can want native audio without the native pan, or vice versa. Same short-circuit
        // discipline: only read the toggle when a scope-capable Flex is active.
        let dax_enabled = matches!(kind, Some(SpectrumKind::FlexVita))
            && engine_lock(engine).settings().flex_native_audio;
        let dax_key = if dax_enabled {
            Some((rig_model, is_network))
        } else {
            None
        };
        if key == self.spectrum_src_key && dax_key == self.dax_src_key {
            return; // both unchanged — no-op (the common case, every tick)
        }
        // Read the Flex API IP once for whichever worker (re)starts (a later IP edit takes effect on
        // the next radio re-select). Lock only on this rare transition, never per tick.
        let (ip, dial_hz) = {
            let e = engine_lock(engine);
            (
                e.settings().flex_radio_ip.trim().to_string(),
                (e.settings().dial_mhz * 1_000_000.0) as u64,
            )
        };
        // Panadapter worker: tear down the old (its Drop stops threads + removes the pan) before
        // starting the new one.
        if key != self.spectrum_src_key {
            self.spectrum_src = None;
            self.spectrum_src_key = key;
            if flex_enabled && !ip.is_empty() {
                self.spectrum_src = crate::flexspectrum::FlexSpectrum::start(
                    engine.clone(),
                    self.spectrum_feed.clone(),
                    ip.clone(),
                    dial_hz,
                )
                .ok();
            }
        }
        // DAX RX audio worker: same tear-down/restart (Drop removes the DAX stream).
        if dax_key != self.dax_src_key {
            self.dax_src = None;
            self.dax_src_key = dax_key;
            // Reset the starvation bookkeeping with the source it belongs to.
            self.dax_started = None;
            self.dax_saw_audio = false;
            if dax_enabled && !ip.is_empty() {
                match crate::flexdax::FlexDax::start(engine.clone(), ip) {
                    Ok(d) => {
                        self.dax_src = Some(d);
                        self.dax_started = Some(Instant::now());
                    }
                    // Was `.ok()`, which threw the reason away: native audio silently did
                    // nothing and the operator had a toggle that appeared to be on. The sound
                    // card still works (dax_src stays None), so this is a warning, not a fault.
                    Err(e) => {
                        {
                            let mut eng = engine_lock(engine);
                            eng.set_audio_error(Some(format!(
                                "Native Flex audio couldn't start ({e}). Using the sound card \
                                 instead — check the Flex API address in Settings."
                            )));
                        }
                        self.err_owner = ErrOwner::Dax;
                    }
                }
            }
        }
    }

    /// Native DAX RX was selected but nothing is arriving — drop back to the sound card and SAY
    /// so. Returns true when it fired (the caller clears `dax_src`).
    ///
    /// Pure decision, split out so it is testable without a Flex on the bench: the whole feature
    /// is unverifiable locally, so at minimum its FAILURE handling must not be.
    fn dax_starved(started: Option<Instant>, saw_audio: bool, now: Instant) -> bool {
        match started {
            // Proven sources and already-reported ones are done: a quiet band must never trip this.
            Some(_) if saw_audio => false,
            Some(t) => now.duration_since(t) >= DAX_STARVE_AFTER,
            None => false,
        }
    }

    /// Publish "Nexus is transmitting" to the native broker RIGHT NOW. Called at each keying
    /// site, because the per-tick publish (the scope-gate block) can lag a fresh key-up by a
    /// whole tick (~20 ms) — and a capture showed the broker's disconnect fail-safe racing
    /// that gap: it fired 5 ms after PTT-ON with tx_intent still false and unkeyed the tune.
    /// Idempotent atomic store; a no-op on the Hamlib path (no native daemon).
    fn publish_tx_intent_now(&self) {
        if let Some(d) = self.rigctld_proc.as_ref().and_then(CatDaemon::native) {
            d.set_tx_intent(true);
        }
    }

    /// Surface (or clear) a "the rig didn't accept PTT" status on the shared audio-error
    /// banner. A keyed-but-NAK'd rig plays modem audio into a receiver = silent dead air
    /// with no warning, so `keying` calls that swallowed the `ptt()` result now route it
    /// here. Uses the err-owner arbitration so a PTT status never clobbers a device/mic
    /// error, and clears only its OWN status when keying succeeds again.
    fn report_ptt(&mut self, engine: &Arc<Mutex<Engine>>, failed: bool) {
        if failed {
            if matches!(self.err_owner, ErrOwner::None | ErrOwner::Ptt) {
                {
                    let mut eng = engine_lock(engine);
                    eng.set_audio_error(Some(
                        "The rig didn't accept PTT — check your PTT method and CAT/port. \
                         Modem audio may be going out while the radio is still receiving."
                            .to_string(),
                    ));
                }
                self.err_owner = ErrOwner::Ptt;
            }
        } else if self.err_owner == ErrOwner::Ptt {
            {
                let mut eng = engine_lock(engine);
                eng.set_audio_error(None);
            }
            self.err_owner = ErrOwner::None;
        }
    }

    /// Reset the per-rig caches after a dual-radio HANDOFF adopted an already-connected Rig for a new
    /// active radio. Forces the retune block to re-assert the restored dial/mode (sentinel
    /// `last_dial`/`last_mode`) and the health / S-meter / DSP-func capabilities to re-probe for the
    /// new rig. Does NOT touch `applied`/`rigctld_proc` (the handoff set those) or the slot/TX clock.
    /// One-shot lazy assert before a key-up (read-only launch): if the rig's mode has
    /// never been commanded this session, push dial (only when it differs from
    /// last_dial — never re-slam a hand-tuned freq inside the read-back window) and
    /// mode NOW, immediately before the key. On mode success, credit last_mode and
    /// latch; on failure leave the latch open and last_mode untouched so the
    /// steady-state retune ladder retries with its full give-up/fallback machinery.
    /// Failing OPEN (keying anyway) is the status quo — today's open-time commands are
    /// also `let _ =` best-effort.
    fn ensure_commanded(&mut self, rig: &mut Rig) {
        if self.rig_asserted {
            return;
        }
        // NEVER command during a deferred dual-radio switch: `rig` is still the OLD
        // radio and cur_dial/cur_md are the NEW radio's settings — asserting here is
        // exactly the cross-radio contamination the contended-switch test pins.
        if self.handoff_deferred {
            return;
        }
        let dial = self.cur_dial;
        let md = self.cur_md.clone();
        if dial != 0 && dial != self.last_dial && rig.set_freq(dial).is_ok() {
            self.last_dial = dial;
        }
        if !md.trim().is_empty() {
            match rig.set_mode(&md, passband_for(&md)) {
                Ok(()) => {
                    self.last_mode = md;
                    self.rig_asserted = true;
                }
                Err(_) => { /* ladder retries; latch stays open */ }
            }
        } else {
            // No mode policy yet (first tick hasn't derived one) — nothing to assert.
        }
    }

    /// Push a dial frequency to the rig, honouring a REFUSAL.
    ///
    /// Returns `None` when the rig accepted it (the caller counts that as a retune), or
    /// `Some(note)` describing the refusal for the CAT status detail.
    ///
    /// THE BUG THIS EXISTS FOR (FTdx10 field report): an out-of-range frequency — 144.390 sent to
    /// an HF-only radio when the APRS cockpit opened — used to be indistinguishable from success,
    /// because `Rig::set_freq` threw its reply away. The loop advanced `last_dial` to a frequency
    /// the radio was never on, reported "CAT confirmed", suppressed the read-back that would have
    /// corrected it, and left the operator's dial reading 144.390 with a dead link.
    ///
    /// Three things a refusal must do, and all three matter:
    ///  1. NOT advance `last_dial` — we did not move the radio, so nothing may claim we did;
    ///  2. stop asking, past a small budget — a radio's frequency range is a hard fact, and every
    ///     retry is a round-trip on a link that is already unhappy;
    ///  3. HEAL the app's belief from the rig itself. `set_frequency` writes the dial optimistically
    ///     the moment the operator asks, so on a refusal the UI is showing a frequency that exists
    ///     nowhere but in our own state. Read the rig and adopt what it says.
    fn push_dial(
        &mut self,
        rig: &mut Rig,
        dial: u64,
        engine: &Arc<Mutex<Engine>>,
    ) -> Option<String> {
        match rig.set_freq(dial) {
            Ok(()) => {
                self.last_dial = dial;
                self.dial_fail_count = 0;
                self.dial_giveup = None;
                // The rig ACKNOWLEDGED the dial — report it DONE so a pending
                // satellite-binding leg can confirm (a no-op for every other
                // QSY). Gated on a real control channel: a control-less Rig
                // returns Ok without a byte on the wire, and "confirming" off
                // that would be the computed-not-done lie all over again.
                if rig.has_control() {
                    engine_lock(engine).rig_dial_applied(dial);
                }
                None
            }
            Err(e) => {
                self.dial_fail_count += 1;
                let mhz = dial as f64 / 1_000_000.0;
                if self.dial_fail_count < DIAL_SET_MAX_TRIES {
                    return Some(format!(
                        "{mhz:.4} MHz {} ({}/{DIAL_SET_MAX_TRIES})",
                        dial_failure_brief(&e),
                        self.dial_fail_count
                    ));
                }
                // Budget spent: this radio will not go there. Stop asking, and stop showing a dial
                // the radio refused — the rig's own frequency is the only true answer.
                self.dial_giveup = Some(dial);
                self.dial_fail_count = 0;
                eprintln!(
                    "tempo-audio: set_freq({dial}) refused {DIAL_SET_MAX_TRIES} times — giving up \
                     (the radio does not appear to cover {mhz:.4} MHz)."
                );
                let healed = rig.read_freq().ok();
                {
                    let mut eng = engine_lock(engine);
                    if let Some(hz) = healed {
                        self.last_dial = hz;
                        eng.observe_rig_freq(hz);
                    }
                    eng.set_rig_refused_dial(Some(mhz));
                }
                Some(match healed {
                    Some(hz) => format!(
                        "the radio refused {mhz:.4} MHz — it does not cover that frequency; \
                         still on {:.4} MHz",
                        hz as f64 / 1_000_000.0
                    ),
                    None => {
                        format!("the radio refused {mhz:.4} MHz — it does not cover that frequency")
                    }
                })
            }
        }
    }

    fn reset_for_handoff(&mut self) {
        self.last_dial = 0; // != any real dial → force the retune to command the restored freq
        self.last_mode = String::new(); // force the mode re-assert
        self.rig_asserted = false; // belt-and-braces: the retune re-asserts + re-latches same tick
        self.mode_fail_count = 0;
        self.mode_giveup = None;
        self.mode_saw_reject = false;
        self.last_cw_wpm = 0;
        self.cw_busy_until = 0.0;
        self.rtty_busy_until = 0.0;
        self.last_fm = None;
        self.manual_ptt_applied = false;
        self.last_rf_power = None;
        self.last_mic_gain = None;
        self.last_nr_level = None;
        self.last_agc = None;
        self.fake_it_restore = None;
        self.audio_rig_split = false;
        self.last_rig_poll = 0.0; // poll the new rig's health/mode/S-meter immediately
        self.last_freq_poll = 0.0;
        self.freq_misses = 0;
        self.cat_ok = None; // re-establish CAT health from the new rig
        self.cat_retry_at = 0.0;
        self.cat_retry_ms = CAT_RETRY_BASE_MS;
        // ⚠️ MUST reset with the radio. Carrying the OLD radio's frequency ranges over a handoff
        // would be a fail-CLOSED bug — the exact inverse of the capability gate's safety property:
        // an HF-only rig's range list inherited by the IC-9700 would block APRS on the one radio
        // that can actually do it. Same for a dial the old radio refused; the new one may accept it.
        self.rx_ranges = None;
        self.rx_ranges_probed = false;
        self.dial_giveup = None;
        self.dial_fail_count = 0;
        // The new radio is not in OUR satellite-mode session; if the old one
        // was, the operator gets it back as-is (hand back, never restore).
        self.split_on_sub = false;
        self.smeter_supported = None;
        self.smeter_misses = 0;
        self.func_supported = [None; 5];
        self.func_misses = [0; 5];
        self.func_state = [None; 5];
        self.level_supported = [None; 4];
        self.level_misses = [0; 4];
        // The audio device must be (re)opened for the new radio even if its device name matches
        // (e.g. both "system default") — force it, since `audio_differs` alone would skip an
        // empty-vs-empty compare and leave the OLD radio's sound-card stream running.
        self.force_audio_rebuild = true;
    }

    /// One radio-loop iteration: fold captured audio in, apply live reconfig
    /// (re-open the rig/sound card via the injected closures on a Settings
    /// change), drop the TX tail, run the slot (TX keying / RX decode), emit
    /// WSJT-X/PSK, and flush spots. Behavior-identical to the original
    /// `run_radio` loop body; the device side-effects are injected.
    #[allow(clippy::too_many_arguments)]
    fn step<B: AudioBackend>(
        &mut self,
        engine: &Arc<Mutex<Engine>>,
        backend: &mut B,
        rig: &mut Rig,
        sinks: &Sinks,
        now: f64,
        reopen_audio: &mut dyn FnMut(&Transport) -> Result<B, String>,
        // `allow_coexist`: may reuse a rigctld already on the port (external share) vs must spawn fresh.
        reopen_rig: &mut dyn FnMut(&Transport, bool) -> RigOpen,
        // Station-wide sinks (PSK buffer + Field Day/club cursor) — shared by every
        // radio chain, so they live outside this per-radio loop state.
        station: &mut StationSinks,
    ) -> Result<(), String> {
        // Steer the slot clock to TRUE UTC: subtract the measured PC-clock-vs-UTC
        // offset (local − UTC) from the system clock, so TX keys and RX decode
        // windows land on the real UTC grid (:00/:15/:30/:45 for FT8) even when the
        // OS clock is skewed — the difference between "decodes only on a
        // well-synced PC" and "decodes anywhere". Applied to ALL downstream `now`
        // uses (slot index, next-slot countdown, TX-hold deadlines) consistently.
        let now = now - self.clock_offset_ms as f64;

        // Continuously fold captured audio into the rolling RX window. Always drain the soundcard
        // ring (so it can't overflow), but when native Flex DAX RX audio is the active source, use
        // its 12 kHz stream as the RX audio instead of the soundcard.
        let soundcard = backend.capture();
        let captured = match self.dax_src.as_ref() {
            Some(dax) => {
                let dax_audio = dax.take_audio();
                if !dax_audio.is_empty() {
                    self.dax_saw_audio = true;
                }
                dax_audio
            }
            None => soundcard,
        };
        // ⚠️ RX FLOOR. Taking DAX audio means IGNORING the sound card, so a DAX source that never
        // streams (wrong IP, firewall, DAX off on the radio, slice never bound) leaves the
        // operator completely deaf — and silence is indistinguishable from a dead band, so there
        // is nothing to notice. Give up on it, fall back, and say why. Same principle as the TX
        // floor in `open_serial_ptt`: a feature that fails must never cost the operator the radio.
        if Self::dax_starved(self.dax_started, self.dax_saw_audio, Instant::now()) {
            self.dax_src = None;
            self.dax_started = None;
            if self.dax_tee_set {
                backend.set_tx_tee(None);
                self.dax_tee_set = false;
            }
            if matches!(self.err_owner, ErrOwner::None | ErrOwner::Dax) {
                {
                    let mut eng = engine_lock(engine);
                    eng.set_audio_error(Some(
                        "Native Flex audio is selected but no audio is arriving — switched back \
                         to the sound card. Check the Flex API address, that DAX is enabled on \
                         the radio, and that a firewall isn't blocking its UDP audio."
                            .to_string(),
                    ));
                }
                self.err_owner = ErrOwner::Dax;
            }
        }
        if !captured.is_empty() {
            self.rx.push(&captured);
        }
        // Keep the DAX TX tee in sync with the DAX source: install it when native audio starts (so
        // backend.play also sends TX over DAX), clear it when it stops. TX schedule is unchanged.
        match (self.dax_src.as_ref(), self.dax_tee_set) {
            (Some(dax), false) => {
                backend.set_tx_tee(Some(dax.tx_tee()));
                self.dax_tee_set = true;
            }
            (None, true) => {
                backend.set_tx_tee(None);
                self.dax_tee_set = false;
            }
            _ => {}
        }

        // --- Live rig/PTT/audio reconfiguration (operator hit Save) + Test-CAT
        // re-probe. Read settings under a short lock, do the slow rig/audio
        // re-open WITHOUT the lock, then publish status. Makes CAT connect on
        // Save with no restart. ---
        {
            // Retune (set freq/mode) only while not actively transmitting a slot or tuning —
            // rigs reject VFO/mode changes mid-TX. We deliberately DON'T gate on manual PTT:
            // a section/mode change must always reach the rig (the proven behavior), and the
            // read-back is gated separately, so gating retune on manual PTT here is what made
            // "the VFO mirrors but modes won't switch" regress. Consume the one-shot "apply
            // now" flag only when we can act, so a click during a slot-TX is honored after it.
            // …and never while a radio switch is mid-flight (handoff deferred): the loop's rig
            // is still the OLD radio, and the want-side dial/mode are already the NEW radio's —
            // retuning here drives the old rig with the new radio's settings (the 2026-07-11
            // "pill says Icom, CAT still controls the Yaesu" regression). The one-shot flags
            // stay queued (consume-only-when-acting) and apply after the handoff lands.
            let can_retune =
                self.tx_until_ms.is_none() && !self.tuning_keyed && !self.handoff_deferred;
            let (want, dial, md, reprobe_req, force_retune, split_req, fm, cat_hold) = {
                let mut eng = engine_lock(engine);
                // FM repeater config (shift, band-offset magnitude, CTCSS) — applied below
                // only when the mode policy resolves to FM. Computed first (owned) so the
                // mutable take_* calls that follow don't fight the settings borrow. APRS forces
                // simplex here (see `fm_repeater_config`) so a beacon never keys through a shift.
                let fm = eng.fm_repeater_config();
                (
                    Transport::from_settings(eng.settings()),
                    eng.settings().dial_hz(),
                    eng.rig_mode_effective(), // operator Phone mode override, else band-derived policy
                    eng.take_cat_reprobe(),
                    if can_retune {
                        eng.take_immediate_retune()
                    } else {
                        false
                    },
                    // Split is a retune-class command — same mid-TX guard, same
                    // leave-it-pending semantics when keyed.
                    if can_retune {
                        eng.take_split_request()
                    } else {
                        None
                    },
                    fm,
                    eng.cat_port_hold(),
                )
            };
            // Stash for the key-site latch (ensure_commanded) — the bindings above live in
            // this block's scope; the key-ups happen in narrower ones.
            self.cur_dial = dial;
            self.cur_md = md.clone();
            // Falling edge of Test CAT's port hold → rebuild the CAT channel we dropped,
            // through the rig_differs branch below (same teardown-then-reopen path).
            let resume_after_hold = !cat_hold && self.cat_hold_active;
            if self.handoff_deferred {
                // A radio switch is mid-flight but the handoff couldn't take the pool
                // lock this tick — do NOT rebuild toward the new transport here, or we
                // spawn a fresh daemon racing the monitor conn that still owns the port.
                // The handoff retries next tick and clears this flag.
            } else if cat_hold {
                // Test CAT's baud-ladder probe needs to open the CAT serial port ITSELF
                // (serial ports are exclusive-open, and our daemon holds the port even when
                // the rig is mute), so drop the daemon + control channel and ack. One-shot
                // on entry; while held, no rebuild/reprobe runs. The unkey-first order
                // mirrors the rig_differs teardown below — never drop a daemon under a
                // possibly-keyed rig. The hold self-expires engine-side, so a crashed
                // prober can't leave CAT down.
                if !self.cat_hold_active {
                    crate::civ::diag::note(
                        "test-cat hold: releasing the CAT port for the baud-ladder probe",
                    );
                    backend.flush_output();
                    let _ = rig.ptt(false);
                    self.tx_until_ms = None;
                    self.tuning_keyed = false;
                    self.manual_ptt_applied = false;
                    self.tune_started_ms = None;
                    {
                        let mut eng = engine_lock(engine);
                        eng.halt_tx();
                    }
                    self.rigctld_proc = None; // drop kills + reaps the daemon (frees the port)
                    *rig = Rig::vox();
                    self.rig_asserted = false;
                    self.cat_hold_active = true;
                    let mut eng = engine_lock(engine);
                    eng.ack_cat_port_released();
                }
            } else if want.rig_differs(&self.applied) || resume_after_hold {
                self.cat_hold_active = false;
                // Unkey through the STILL-ALIVE old rig/daemon before tearing it
                // down. Dropping rigctld_proc and swapping *rig first would strand
                // a keyed transmitter (or a tune carrier): the un-key command
                // would go to a dead daemon. Order matters — flush, unkey, clear
                // TX state, THEN drop the daemon.
                {
                    // UNCONDITIONAL: the flags can desync from a keyed radio (failed
                    // unkey); this teardown is the last chance to key-up through a
                    // LIVE channel before the daemon dies. Idempotent when idle.
                    crate::civ::diag::note(
                        "rig_differs: transport changed → teardown+rebuild daemon (unkey first)",
                    );
                    backend.flush_output();
                    let _ = rig.ptt(false);
                    self.tx_until_ms = None;
                    self.tuning_keyed = false;
                    self.manual_ptt_applied = false;
                    self.tune_started_ms = None;
                    {
                        let mut eng = engine_lock(engine);
                        eng.halt_tx();
                    }
                }
                // Whether `reopen_rig` may auto-coexist onto a rigctld ALREADY listening on the new
                // port (see `allow_coexist_on_swap`). We must NOT coexist onto our OWN daemon that
                // we're about to kill — its corpse would keep commanding the OLD radio (the dual-radio
                // "switch back to HF still drives the 2 m Icom" bug).
                let allow_coexist = allow_coexist_on_swap(
                    self.rigctld_proc.is_some(),
                    self.applied.rigctld_port,
                    want.rigctld_port,
                );
                self.rigctld_proc = None; // drop kills + reaps the old daemon (frees its port)
                let (new_rig, proc, probe) = reopen_rig(&want, allow_coexist);
                let (ok, detail) = (probe.ok, probe.detail);
                self.rig_asserted = false; // fresh rig: unclaimed caches make the retune re-assert this tick
                *rig = new_rig;
                self.rigctld_proc = proc;
                // Do NOT claim last_dial/last_mode here: open_cat's set_freq/set_mode are best-effort
                // (`let _ =`), so a failed open-time tune must be retried. Leaving these at the OLD
                // radio's values makes the retune block below (same tick) see `dial != last_dial` and
                // re-apply until it sticks, instead of silently stranding the new rig off-frequency.
                self.mode_fail_count = 0; // fresh rig — the retune retry budget resets
                self.mode_giveup = None; // and a fresh rig may well accept what the old rejected
                self.mode_saw_reject = false;
                self.cat_ok = ok;
                {
                    let mut eng = engine_lock(engine);
                    eng.set_cat_status(ok, detail);
                }
            } else if reprobe_req {
                let (ok, mut detail) = reprobe(rig, &want);
                // Attribution only when the CAT channel itself was probed (the branches of
                // `reprobe` that call probe_cat_or_explain) — a serial-PTT line test
                // ("Serial RTS PTT on COM5") or a VOX result has no backend to name.
                let probed_cat = (matches!(want.ptt_method.as_str(), "cat" | "vox")
                    && want.rig_model != 0)
                    || keys_on_the_cat_port(&want);
                if ok.is_some() && probed_cat && rig.has_control() {
                    detail = with_backend(detail, self.live_backend_label(&want));
                }
                self.cat_ok = ok;
                {
                    let mut eng = engine_lock(engine);
                    eng.set_cat_status(ok, detail);
                }
            }
            let mut audio_rebuilt = false;
            // A dual-radio switch forces the rebuild (a new radio's device must be opened even if the
            // name compares equal — e.g. two "system default"s); else rebuild only on a real change.
            if !self.handoff_deferred
                && (std::mem::take(&mut self.force_audio_rebuild)
                    || want.audio_differs(&self.applied))
            {
                // The queued TX audio for a live over lives ENTIRELY in the old
                // backend's output ring — replacing the backend discards it. If
                // we're mid-transmission (a slot over, a tune carrier, or manual
                // PTT), end the over cleanly FIRST: flush, unkey, drop the hold,
                // halt the engine's TX. Otherwise the rig would sit KEYED on a
                // dead, unmodulated carrier for the rest of the slot while the
                // modem samples are already gone — and the sequencer would count
                // that silent over as sent and wait for a reply that never comes.
                // Mirrors the rig-rebuild path above.
                {
                    // UNCONDITIONAL — same desync rationale as the rig-rebuild guard.
                    crate::civ::diag::note("audio rebuild: ending the over (flush+unkey) before reopening the sound card");
                    backend.flush_output();
                    let _ = rig.ptt(false);
                    self.tx_until_ms = None;
                    self.tuning_keyed = false;
                    self.manual_ptt_applied = false;
                    self.tune_started_ms = None;
                    {
                        let mut eng = engine_lock(engine);
                        eng.halt_tx();
                    }
                }
                match reopen_audio(&want) {
                    Ok(b) => {
                        *backend = b;
                        audio_rebuilt = true;
                        // New stream, new ring: republish so the producer rebuilds its resampler
                        // and clears its window rather than smearing two sample rates together.
                        if let Some((ring, rate)) = backend.spectrum_tap() {
                            self.rx_tap.publish_card(ring, rate);
                        }
                        {
                            let mut eng = engine_lock(engine);
                            eng.set_audio_error(None);
                        }
                        self.err_owner = ErrOwner::None;
                        // The fresh backend has NO mic stream — a stale-true flag
                        // here fed the recorder empty audio for the rest of a
                        // live recording, silently (review MAJOR). The rising
                        // edge reopens the mic on the new backend next loop.
                        self.voice_mic_open = false;
                    }
                    Err(e) => {
                        {
                            let mut eng = engine_lock(engine);
                            eng.set_audio_error(Some(format!("Audio device failed to open: {e}")));
                        }
                        // A REAL device error owns the line — monitor/voice-mic
                        // notices may neither overwrite nor clear it.
                        self.err_owner = ErrOwner::Device;
                    }
                }
            } else {
                // No backend rebuild — apply the live gains in place. Independent checks (not an
                // else-if chain) so a same-tick change to both TX level and RX gain both land.
                if (want.tx_level - self.applied.tx_level).abs() > f32::EPSILON {
                    backend.set_tx_level(want.tx_level);
                }
                if (want.rx_gain - self.applied.rx_gain).abs() > f32::EPSILON {
                    backend.set_rx_gain(want.rx_gain);
                }
            }

            // Headphone monitor (DARK, off by default): reconfigure it IN PLACE on a
            // monitor-setting change — or re-apply it to a freshly rebuilt backend,
            // whose monitor starts off. This never rebuilds the capture/TX streams, so
            // the decode path never restarts. Guard: refuse to open the monitor on the
            // rig's TX output device, which would transmit the received band back out.
            if audio_rebuilt
                || want.monitor_differs(&self.applied)
                || std::mem::take(&mut self.monitor_reapply)
            {
                // Resolve "system default" to its REAL device name first — an
                // empty monitor_device against a named audio_out that happens to
                // BE the OS default was a hole in the name-based guard (review
                // catch: the monitor would mix the received band into the rig's
                // TX stream). Resolution only runs when the monitor is on.
                let (mon_dev, out_dev) = if want.monitor_enabled {
                    (
                        crate::monitor::resolve_output_name(&want.monitor_device),
                        crate::monitor::resolve_output_name(&want.audio_out),
                    )
                } else {
                    (want.monitor_device.clone(), want.audio_out.clone())
                };
                let guarded = crate::monitor::monitor_would_transmit(&mon_dev, &out_dev);
                let effective = want.monitor_enabled && !guarded;
                let outcome =
                    backend.set_monitor(effective, &want.monitor_device, want.monitor_level);
                {
                    let mut eng = engine_lock(engine);
                    match outcome {
                        Err(e) => {
                            // Write only over None or our own prior notice — a
                            // Device error outranks us; a VoiceMic notice is the
                            // operator's more recent concern.
                            if matches!(self.err_owner, ErrOwner::None | ErrOwner::Monitor) {
                                eng.set_audio_error(Some(format!(
                                    "Headphone monitor could not open: {e}"
                                )));
                                self.err_owner = ErrOwner::Monitor;
                            }
                        }
                        Ok(()) if want.monitor_enabled && guarded => {
                            if matches!(self.err_owner, ErrOwner::None | ErrOwner::Monitor) {
                                eng.set_audio_error(Some(
                                    "Headphone monitor is off: the chosen output is the rig's TX \
                                     device — monitoring it would transmit the received band. Pick a \
                                     separate headphone or speaker device."
                                        .to_string(),
                                ));
                                self.err_owner = ErrOwner::Monitor;
                            }
                        }
                        Ok(()) => {
                            // Clear only a line the MONITOR wrote — never a real
                            // device error, never the voice-mic's notice.
                            if self.err_owner == ErrOwner::Monitor {
                                eng.set_audio_error(None);
                                self.err_owner = ErrOwner::None;
                            }
                        }
                    }
                }
            }
            if !self.handoff_deferred && want != self.applied {
                // NEVER on a deferred tick: `rig` is still the OLD radio's connection, and
                // claiming the NEW transport here poisons `rig_differs` — the handoff's
                // fallback branch relies on it to open the new radio fresh.
                self.applied = want;
            }
            // Reconcile the native RF panadapter (Flex VITA / Icom CI-V) to the ACTIVE radio's
            // capability — cheap (a key compare) unless it just gained/lost/changed a native scope.
            let (scope_model, scope_net) = (self.applied.rig_model, self.applied.is_network());
            self.reconcile_spectrum_source(engine, scope_model, scope_net);
            // Native CI-V scope: THE ACTIVE radio's daemon streams the rig's real panadapter.
            // Enable is per-tick idempotent (an atomic store); monitors never enable it, so a
            // backgrounded radio's serial link stays free for its slow poll. Rows land in the
            // same engine slot as the Flex path, tagged "civ" (auto-fallback keeps working).
            if let Some(d) = self.rigctld_proc.as_ref().and_then(CatDaemon::native) {
                // The waveform stream requires CI-V USB baud 115200 — not just for headroom
                // (~7.5 KB/s of scope frames + CAT), but because the RIG enforces it: per the
                // official Icom CI-V reference (IC-9700 guide, 27 11 footnote), wave output
                // over USB needs "Unlink from [REMOTE]" + 115200, and the rig NAKs `27 11 01`
                // at lower baud (verified on an IC-9700 at 57600). Below that: CAT-only.
                //
                // AND pause it while TRANSMITTING: on the shared half-duplex CI-V bus a continuous
                // 0x27 flood during TX makes the IC-9700's PTT chatter (rapid key/unkey → no RF, no
                // CAT error). Gate the stream OFF for any keyed state — an FT8 over (tx_until_ms),
                // the tune carrier (tuning_keyed), or manual phone PTT — and it resumes on unkey.
                // RX scope is meaningless during TX anyway. (Native path only; Hamlib has no stream.)
                // `rig.keyed` flips true the instant ANY keying path (slot, tune, voice, CW) calls
                // ptt(true), so it leads the per-path flags by up to a tick — include it so there's
                // no window right after keying where we'd wrongly report "not transmitting".
                let keyed_now = rig.keyed
                    || self.tx_until_ms.is_some()
                    || self.tuning_keyed
                    || self.manual_ptt_applied;
                // In FT8/FT4 (a DATA mode) the Operate waterfall shows the AUDIO FFT (0–4000 Hz),
                // not the RF panadapter — so keep the native scope OFF here and never feed its
                // absolute-RF row into the shared spectrum. Otherwise spectrum_row() prefers the
                // fresh "civ" MHz-span row and the source-unaware FT8 waterfall maps it onto a
                // 0–4000 Hz view → every bin clamps to the floor → a flat "purple" field (while FT8
                // still decodes, since the decoder reads raw audio). Phone/CW keep the scope — their
                // PhoneScope is source-aware and renders the civ row correctly.
                let data_mode = mode_is_data(&self.last_mode);
                d.set_scope_enabled(self.applied.baud >= 115_200 && !keyed_now && !data_mode);
                // Tell the broker we're on the air, so its disconnect fail-safe unkey stands down
                // while WE'RE transmitting — a transient reconnect of Nexus's own Rig must never
                // steal the over (the native-CI-V PTT flicker). Cleared the moment TX ends.
                d.set_tx_intent(keyed_now);
                // Native-scope CONTROL one-shots from the UI (span/ref/mode). These are short 27
                // CAT frames (NOT the waveform stream), so no 115200 requirement — but they share
                // the half-duplex bus, so hold them until unkey (same reason the stream pauses).
                if !keyed_now {
                    let (span, refl, fixed) = {
                        let mut e = engine_lock(engine);
                        (
                            e.take_scope_span_request(),
                            e.take_scope_ref_request(),
                            e.take_scope_fixed_request(),
                        )
                    };
                    if let Some(hz) = span {
                        d.set_scope_span(hz);
                    }
                    if let Some(t) = refl {
                        d.set_scope_ref(t);
                    }
                    if let Some(f) = fixed {
                        d.set_scope_center_mode(f);
                    }
                }
                // Publish straight to the spectrum feed. This used to go through the engine
                // mutex, so the Icom panadapter was starved by the very hold that starved the
                // audio row (the boundary CAT block downstream of this loop's engine.lock()).
                if !data_mode {
                    if let Some(sweep) = d.take_scope_row() {
                        self.spectrum_feed.publish_rf(tempo_app::dto::Spectrum {
                            row: sweep.row,
                            lo_hz: sweep.lo_hz,
                            hi_hz: sweep.hi_hz,
                            source: "civ".into(),
                        });
                    }
                } else {
                    // DATA mode (FT8/FT4): drop any stale native row so the audio FFT takes over
                    // immediately (no ~1 s window where the last civ row still wins).
                    self.spectrum_feed.clear_rf();
                }
            }

            // Live dial / mode retune — only while not keyed (rigs reject VFO
            // changes mid-TX); retried every loop until it sticks.
            let mut retuned = false;
            // A human-readable note about what we just commanded the rig to do, surfaced into
            // the CAT status so the operator (and we) can SEE the mode the rig was told to use
            // and whether it accepted it — turning "modes won't switch" from a guess into data.
            let mut retune_note: Option<String> = None;
            // A DIAL refusal, held separately so the mode note below cannot bury it.
            let mut dial_note: Option<String> = None;
            if can_retune {
                if force_retune {
                    // The operator just clicked a section / worked a Needed spot / QSY'd.
                    // Apply the dial + mode RIGHT NOW, clearing any give-up so a single
                    // click is never ignored — even on a mode a prior attempt abandoned
                    // (the whole reason a re-click of e.g. CW used to do nothing). The MODE
                    // is re-asserted unconditionally (picking CW while already on a CW freq
                    // must still command the rig to CW). The DIAL is only pushed when it
                    // actually changed: a mode-only click (CW preserves the dial) must NOT
                    // re-slam a freq the operator may have just hand-tuned inside the up-to-
                    // 750 ms read-back window — that would fight the VFO-knob mirroring.
                    self.mode_giveup = None;
                    self.mode_fail_count = 0;
                    self.mode_saw_reject = false;
                    // An explicit operator retune also clears a dial give-up: they may have just
                    // switched to a radio that CAN reach it, so a re-click must always try again.
                    self.dial_giveup = None;
                    self.dial_fail_count = 0;
                    if dial != self.last_dial {
                        match self.push_dial(rig, dial, engine) {
                            // A refused DIAL outranks any mode note produced below: "the radio
                            // refused 144.390 MHz" is the answer to the operator's question, and a
                            // cheerful "rig set to FM" beside a dial that never moved is how this
                            // bug stayed invisible in the first place.
                            Some(note) => dial_note = Some(note),
                            None => retuned = true,
                        }
                    }
                    if !md.trim().is_empty() {
                        // A dial-only QSY (wheel/nudge) re-enters this force path with the SAME mode;
                        // skip the diagnostic mode read-back then, so continuous wheel-tuning doesn't
                        // fire an extra `w MD0;` round-trip per ~120 ms flush. The mode is still
                        // re-asserted (an explicit same-mode re-click must still command the rig).
                        let mode_changed = md != self.last_mode;
                        match rig.set_mode(&md, passband_for(&md)) {
                            Ok(()) => {
                                self.last_mode = md.clone();
                                self.rig_asserted = true; // a real assert — credit the latch
                                retuned = true;
                                if mode_changed {
                                    // Read the mode straight back FROM the rig to confirm it
                                    // actually applied — rigctld can answer RPRT 0 without the rig
                                    // changing, which is the only way to tell those apart.
                                    retune_note = Some(mode_set_note(rig, &md));
                                }
                            }
                            // `last_mode` is unchanged, so the steady-state path below re-tries
                            // on later loops and re-gives-up past the budget — a non-supporting
                            // rig is still never spammed forever.
                            Err(e) => {
                                self.mode_saw_reject |= e.kind() == std::io::ErrorKind::Other;
                                retune_note = Some(mode_command_failed(&md, &e));
                            }
                        }
                    }
                } else {
                    // `dial_giveup` stops a frequency the radio has REFUSED from being re-sent on
                    // every tick — the HF-only-rig-on-2 m storm, and the same shape as
                    // `mode_giveup` below.
                    if dial != self.last_dial && self.dial_giveup != Some(dial) {
                        match self.push_dial(rig, dial, engine) {
                            // A refused DIAL outranks any mode note produced below: "the radio
                            // refused 144.390 MHz" is the answer to the operator's question, and a
                            // cheerful "rig set to FM" beside a dial that never moved is how this
                            // bug stayed invisible in the first place.
                            Some(note) => dial_note = Some(note),
                            None => retuned = true,
                        }
                    }
                    // Apply the section's mode — unless it's the one we already gave up on
                    // (rig kept rejecting it). `last_mode` only ever holds a mode actually
                    // applied, so a give-up never masquerades as success.
                    if md != self.last_mode && self.mode_giveup.as_deref() != Some(md.as_str()) {
                        match rig.set_mode(&md, retry_passband(&md, self.mode_fail_count)) {
                            Ok(()) => {
                                self.last_mode = md.clone();
                                self.rig_asserted = true; // a real assert — credit the latch
                                self.mode_fail_count = 0;
                                self.mode_giveup = None; // a success clears any prior give-up
                                self.mode_saw_reject = false;
                                retuned = true;
                                retune_note = Some(mode_set_note(rig, &md));
                            }
                            Err(e) => {
                                // Retries cover a rig/rigctld still settling; past the budget the
                                // rig is rejecting this mode (e.g. no DATA/PKT submode) — stop
                                // retrying THIS mode so we don't spam the CAT link every loop. A
                                // later section change to a different mode still tries (md flips),
                                // and once any mode sticks the give-up is cleared.
                                self.mode_fail_count += 1;
                                self.mode_saw_reject |= e.kind() == std::io::ErrorKind::Other;
                                retune_note = Some(format!(
                                    "{} ({}/{MODE_SET_MAX_TRIES})",
                                    mode_command_failed(&md, &e),
                                    self.mode_fail_count
                                ));
                                if self.mode_fail_count >= MODE_SET_MAX_TRIES {
                                    eprintln!(
                                        "tempo-audio: set_mode({md:?}) failed {} times — giving up \
                                         (rejected by rig: {}).",
                                        self.mode_fail_count, self.mode_saw_reject
                                    );
                                    self.mode_giveup = Some(md.clone());
                                    self.mode_fail_count = 0;
                                    let saw_reject = std::mem::take(&mut self.mode_saw_reject);
                                    // Last rung of the ladder: a rig that actively REFUSED a
                                    // DATA submode still speaks the plain sideband — put it
                                    // there (filter untouched) so the operator only has to
                                    // press the rig's DATA key, instead of a dead-end note.
                                    // Sent ONCE; link-fault give-ups skip it (the link, not
                                    // the mode, is the problem — don't add more traffic).
                                    let fallback = if saw_reject {
                                        fallback_sideband(&md)
                                            .filter(|base| rig.set_mode(base, -1).is_ok())
                                    } else {
                                        None
                                    };
                                    if let Some(base) = fallback {
                                        self.last_mode = base.to_string();
                                    }
                                    retune_note = Some(mode_giveup_note(&md, saw_reject, fallback));
                                }
                            }
                        }
                    }
                }
            }

            // FM repeater: once the mode policy is FM, push the shift / offset / CTCSS —
            // ON CHANGE only, so the CAT link isn't spammed every loop. Leaving FM clears
            // the tracker so the next FM entry re-applies. Best-effort (a rig without
            // repeater or CTCSS support no-ops the unsupported command). Same mid-TX guard
            // as the retune above.
            // Read-only launch: the FM repeater config (shift/offset/CTCSS) must not be
            // pushed before the first genuine assert — with last_fm starting None it
            // would otherwise fire on the first FM tick with no operator action, i.e. a
            // launch-time command surviving the flip.
            if can_retune && md == "FM" && self.rig_asserted {
                if self.last_fm.as_ref() != Some(&fm) {
                    let _ = rig.set_fm_repeater(&fm.0, fm.1, fm.2);
                    self.last_fm = Some(fm);
                    retuned = true;
                }
            } else if md != "FM" {
                self.last_fm = None;
            }

            // Live READ-BACK of the rig's actual dial, so a manual VFO knob turn (or another
            // app on the CAT broker) is mirrored in the UI. CAT-only — read_freq no-ops
            // (cheap) on VOX/serial. We adopt a reported change AND advance last_dial so the
            // retune block above doesn't push it back. Guards:
            //  - skip on any tick we just pushed an app change (the rig is still settling) and
            //    defer the next poll a full interval, so a stale read can't revert the QSY;
            //  - skip while transmitting/tuning;
            //  - skip when CAT is known-failing, so a connected-but-mute rig doesn't block the
            //    slot loop on the read timeout every cycle.
            //  (Mode read-back is DISPLAY-ONLY — mirrored into a separate snapshot field for
            //   the mismatch tag; it never overwrites the canonical commanded sideband.)
            if retuned {
                self.last_rig_poll = now;
                // Defer the fast dial mirror a FULL heavy interval after an app QSY: a read only
                // ~180 ms after the F-ack could return the pre-QSY dial (Hamlib's get-cache, or a
                // slow network chain) and observe_rig_freq would adopt it as a knob QSY and revert.
                self.last_freq_poll = now + (RIG_POLL_MS - FREQ_POLL_MS);
                self.freq_misses = 0; // a successful set_freq/set_mode proves the link is alive
                                      // The app just commanded a new dial/mode — drop the stale read-back mode + passband
                                      // width so a band/mode change can't flash a false "rig: X" mismatch or show the
                                      // prior mode's filter width before the next poll reads the rig's true state.
                {
                    let mut eng = engine_lock(engine);
                    eng.clear_rig_mode();
                    eng.clear_rig_passband();
                }
                // A CAT command (set_freq/set_mode) just SUCCEEDED, so CAT is alive — clear
                // a stale `cat_ok=Some(false)` (e.g. a transient read_freq failure at the
                // initial probe). Otherwise the dial read-back stays disabled even though
                // mode-switching works, and the VFO knob never mirrors into the UI. Also
                // clear the matching "no rig control" UI warning, once, on the flip.
                if self.cat_ok != Some(true) {
                    self.cat_ok = Some(true);
                    self.cat_retry_ms = CAT_RETRY_BASE_MS;
                    self.cat_retry_at = 0.0;
                    // Re-probe rig capabilities (S-meter + DSP funcs) on a fresh CAT confirmation,
                    // so swapping to a different rig doesn't inherit the old one's verdict.
                    self.rx_ranges_probed = false;
                    self.smeter_supported = None;
                    self.smeter_misses = 0;
                    self.func_supported = [None; 5];
                    self.func_misses = [0; 5];
                    self.func_state = [None; 5];
                    self.level_supported = [None; 4];
                    self.level_misses = [0; 4];
                    {
                        let mut eng = engine_lock(engine);
                        eng.set_cat_status(
                            Some(true),
                            "CAT confirmed — rig accepted a command".to_string(),
                        );
                    }
                }
            } else if self.tx_until_ms.is_none()
                && !self.tuning_keyed
                && !self.manual_ptt_applied
                // A TRIPPED breaker skips the poll — but only until its re-probe is due. It exists
                // to stop the loop blocking on a dead read every cycle, which is rate-limiting;
                // implemented as a permanent latch it left a recovered link dead for the session.
                && (self.cat_ok != Some(false) || now >= self.cat_retry_at)
                && now - self.last_rig_poll >= RIG_POLL_MS
            {
                let breaker_probe = self.cat_ok == Some(false);
                if breaker_probe {
                    // Schedule the NEXT attempt before trying this one, doubling the wait, so a
                    // link that stays dead costs one timeout per ~30 s rather than one per tick.
                    self.cat_retry_ms = (self.cat_retry_ms * 2.0).min(CAT_RETRY_MAX_MS);
                    self.cat_retry_at = now + self.cat_retry_ms;
                }
                self.last_rig_poll = now;
                self.last_freq_poll = now; // heavy tick reads the dial too — don't double-read below
                self.rig_poll_ticks = self.rig_poll_ticks.wrapping_add(1);
                // Periodically re-probe a rig whose S-meter was found unsupported — a few
                // STRENGTH misses can be a transient hiccup, not a real lack of support — so it
                // recovers without needing a full CAT drop + reconfirm.
                if self.smeter_supported == Some(false) && self.rig_poll_ticks.is_multiple_of(40) {
                    self.smeter_supported = None;
                    self.smeter_misses = 0;
                }
                // Re-probe a given-up func only once its BACKOFF has elapsed. A rig that never
                // answers a func used to be retried every 40 heavy polls (~30 s) forever, and
                // every retry costs a full CAT timeout on this thread — which starves the
                // waterfall (see `func_retry_at`). Backing off keeps transient-hiccup recovery
                // while making a permanently-absent func cost progressively nothing.
                for i in 0..RIG_FUNCS.len() {
                    if self.func_supported[i] == Some(false)
                        && self.rig_poll_ticks >= self.func_retry_at[i]
                    {
                        self.func_supported[i] = None; // give a given-up func one retry
                        self.func_misses[i] = 0;
                    }
                }
                match rig.read_freq() {
                    Ok(hz) => {
                        self.freq_misses = 0; // a good read clears the breaker's miss run
                                              // A tripped breaker's re-probe answered: the link is BACK. Reset the health
                                              // verdict + the backoff and re-probe the rig's capabilities, exactly like
                                              // the successful-command path above — otherwise read-back stays disabled
                                              // for the session even though the radio is answering perfectly.
                        if breaker_probe {
                            self.cat_ok = Some(true);
                            self.cat_retry_ms = CAT_RETRY_BASE_MS;
                            self.cat_retry_at = 0.0;
                            self.smeter_supported = None;
                            self.smeter_misses = 0;
                            self.func_supported = [None; 5];
                            self.func_misses = [0; 5];
                            self.func_state = [None; 5];
                            self.level_supported = [None; 4];
                            self.level_misses = [0; 4];
                            self.rx_ranges_probed = false;
                            {
                                let mut eng = engine_lock(engine);
                                eng.set_cat_status(
                                    Some(true),
                                    "CAT recovered — the radio is answering again".to_string(),
                                );
                            }
                        }
                        if hz != self.last_dial {
                            self.last_dial = hz;
                            {
                                let mut eng = engine_lock(engine);
                                eng.observe_rig_freq(hz);
                            }
                        }
                        // Read the radio's frequency-range table ONCE per CAT confirmation, so the
                        // app can know a radio cannot reach 2 m BEFORE commanding it there (the
                        // HF-only-rig report). Cheap: one round-trip per rig, never per poll, and
                        // an unsupported `\dump_state` is remembered as unknown → callers fail open.
                        if !self.rx_ranges_probed {
                            self.rx_ranges_probed = true;
                            self.rx_ranges = rig.read_rx_ranges();
                            {
                                let mut eng = engine_lock(engine);
                                eng.observe_rig_rx_ranges(self.rx_ranges.clone());
                            }
                        }
                        // RF power / mic gain / NR / AGC read-backs mirror the rig's real knob
                        // positions into the UI slider (kept separate from the commanded value —
                        // observe never fights a pending set; see observe_rig_power). Each is
                        // capability-cached (3 misses → stop issuing it) so a rig slow or silent on
                        // one — the K4 via QK4 Remote — doesn't time out and drop+reconnect the CAT
                        // socket every poll. Only AFTER the dial probe answered, so a half-open link
                        // can't eat a SECOND 2.5 s timeout on the same dead poll.
                        if self.level_supported[LVL_RFPOWER] != Some(false) {
                            let ok = match rig.read_level("RFPOWER") {
                                Ok(frac) => {
                                    {
                                        let mut eng = engine_lock(engine);
                                        eng.observe_rig_power(frac);
                                    }
                                    true
                                }
                                Err(_) => false,
                            };
                            note_ext_read(
                                &mut self.level_supported[LVL_RFPOWER],
                                &mut self.level_misses[LVL_RFPOWER],
                                ok,
                            );
                        }
                        if self.level_supported[LVL_MICGAIN] != Some(false) {
                            let ok = match rig.read_level("MICGAIN") {
                                Ok(frac) => {
                                    {
                                        let mut eng = engine_lock(engine);
                                        eng.observe_rig_mic_gain(frac);
                                    }
                                    true
                                }
                                Err(_) => false,
                            };
                            note_ext_read(
                                &mut self.level_supported[LVL_MICGAIN],
                                &mut self.level_misses[LVL_MICGAIN],
                                ok,
                            );
                        }
                        if self.level_supported[LVL_NR] != Some(false) {
                            let ok = match rig.read_level("NR") {
                                Ok(frac) => {
                                    {
                                        let mut eng = engine_lock(engine);
                                        eng.observe_rig_nr_level(frac);
                                    }
                                    true
                                }
                                Err(_) => false,
                            };
                            note_ext_read(
                                &mut self.level_supported[LVL_NR],
                                &mut self.level_misses[LVL_NR],
                                ok,
                            );
                        }
                        if self.level_supported[LVL_AGC] != Some(false) {
                            let ok = match rig.read_agc() {
                                Some(v) => {
                                    {
                                        let mut eng = engine_lock(engine);
                                        eng.observe_rig_agc(agc_from_hamlib(v).to_string());
                                    }
                                    true
                                }
                                None => false,
                            };
                            note_ext_read(
                                &mut self.level_supported[LVL_AGC],
                                &mut self.level_misses[LVL_AGC],
                                ok,
                            );
                        }
                        // Real CAT S-meter (STRENGTH, dB rel S9), mirrored to the UI as a
                        // calibrated S-unit bar. RX-only (this whole block is gated on
                        // `tx_until_ms.is_none()`), so it never reads a meaningless TX value.
                        // Lazy capability: the dial read above just succeeded, so the link is
                        // alive — if STRENGTH still returns nothing the rig has no CAT S-meter,
                        // so stop polling it (don't burn a round-trip every cycle) and leave the
                        // UI meter empty rather than faking one.
                        if self.smeter_supported != Some(false) {
                            match rig.read_smeter_db() {
                                Some(db) => {
                                    self.smeter_supported = Some(true);
                                    self.smeter_misses = 0;
                                    {
                                        let mut eng = engine_lock(engine);
                                        eng.observe_rig_smeter(db);
                                    }
                                }
                                // Only give up after several consecutive misses — one
                                // transient timeout on a capable rig must not permanently
                                // kill its S-meter.
                                None => {
                                    self.smeter_misses = self.smeter_misses.saturating_add(1);
                                    if self.smeter_misses >= 3 {
                                        self.smeter_supported = Some(false);
                                        // Don't leave the last good reading frozen on the UI.
                                        {
                                            let mut eng = engine_lock(engine);
                                            eng.clear_rig_smeter();
                                        }
                                    }
                                }
                            }
                        }
                        // Display-only mode read-back: mirror the rig's actual mode into a
                        // SEPARATE snapshot field so the cockpit can flag when the operator's
                        // mode knob disagrees with the app's commanded mode. Never overwrites
                        // the canonical commanded sideband (App-side invariant). `m` can be a
                        // touch stale on some backends — fine for a display-only hint.
                        // Mode changes rarely — read it on a slower sub-cadence (every 4th
                        // poll) to keep the fast dial/health check tight on slow serial links.
                        if self.rig_poll_ticks.is_multiple_of(4) {
                            // One `m` read gives BOTH the mode (mirror) and the RX passband width.
                            let (m, pb) = rig.read_mode_passband();
                            {
                                let mut eng = engine_lock(engine);
                                if let Some(ref mm) = m {
                                    eng.observe_rig_mode(mm.clone());
                                }
                                eng.observe_rig_passband(pb); // None (a split read) keeps the last width
                            }
                            // Apply a pending RX filter-width change (Hamlib carries width as the
                            // 2nd arg of set_mode). Only drain the request when we KNOW the mode to
                            // set it against, and re-queue on a failed/rejected set — so a CAT
                            // hiccup or a split `m` read never silently swallows the operator's click.
                            if let Some(ref mode) = m {
                                let width_req = engine_lock(engine).take_passband_request();
                                if let Some(hz) = width_req {
                                    if rig.set_passband(mode, hz).is_ok() {
                                        {
                                            let mut eng = engine_lock(engine);
                                            eng.observe_rig_passband(Some(hz)); // optimistic; next read confirms
                                        }
                                    } else {
                                        let mut eng = engine_lock(engine);
                                        eng.request_filter_width(hz); // re-queue for the next cycle
                                    }
                                }
                            }
                        }
                        // Apply any pending DSP-func toggle from the UI promptly — the dial read
                        // proved the link is alive. Drain under the lock, RELEASE it, then do the
                        // set_func TCP round-trip so the UI thread never blocks on the socket.
                        let func_reqs =
                            Some(engine_lock(engine)).map(|mut e| e.take_func_requests());
                        if let Some(reqs) = func_reqs {
                            let mut changed = false;
                            for i in 0..RIG_FUNCS.len() {
                                if let Some(on) = reqs[i] {
                                    if rig.set_func(RIG_FUNCS[i], on).is_ok() {
                                        self.func_state[i] = Some(on); // optimistic; a GET confirms
                                        changed = true;
                                    }
                                }
                            }
                            if changed {
                                {
                                    let mut eng = engine_lock(engine);
                                    eng.observe_rig_funcs(self.func_state);
                                }
                            }
                        }
                        // Apply pending RIT/XIT/VFO clarifier requests (CAT-panel controls). Drain
                        // under the lock, RELEASE it, then do the CAT round-trip. Write-only +
                        // optimistic — the snapshot already mirrors the commanded value.
                        if let Some(hz) =
                            Some(engine_lock(engine)).and_then(|mut e| e.take_rit_apply())
                        {
                            let _ = rig.set_rit(hz);
                        }
                        if let Some(hz) =
                            Some(engine_lock(engine)).and_then(|mut e| e.take_xit_apply())
                        {
                            let _ = rig.set_xit(hz);
                        }
                        if let Some(vfo_b) =
                            Some(engine_lock(engine)).and_then(|mut e| e.take_vfo_apply())
                        {
                            let _ = rig.set_vfo(if vfo_b { "VFOB" } else { "VFOA" });
                        }
                        // DSP funcs (NB/NR/notch=ANF/COMP/VOX): one GET per still-supported func on
                        // the slow sub-cadence, mirroring the S-meter's lazy-capability + miss-
                        // tolerance. A GET miss on this proven-alive link means the rig lacks the
                        // func (hide it); a read failure on a supported func keeps the last state.
                        // Read ONE DSP func per cycle, round-robin — NOT all five at once, and on a
                        // different sub-tick than the mode read above. A func GET on a rig that
                        // doesn't cleanly reject an unsupported func blocks to the ~2.5 s CAT
                        // deadline; reading all five on one tick could stall the poll loop (and the
                        // S-meter / scope it feeds) for many seconds every fourth poll — the
                        // "runs 4 s, hangs a few, repeats" symptom. One-at-a-time bounds a tick's
                        // worst case to a single timeout. SET (immediate, optimistic) is unchanged,
                        // so slower GET confirmation costs no responsiveness.
                        if self.rig_poll_ticks % 4 == 2 {
                            let i = ((self.rig_poll_ticks / 4) as usize) % RIG_FUNCS.len();
                            if self.func_supported[i] != Some(false) {
                                match rig.read_func(RIG_FUNCS[i]) {
                                    Some(on) => {
                                        self.func_supported[i] = Some(true);
                                        self.func_misses[i] = 0;
                                        self.func_state[i] = Some(on);
                                        // A real answer clears the backoff: a func that works
                                        // now must recover full responsiveness if it ever drops.
                                        self.func_retry_backoff[i] = FUNC_RETRY_BACKOFF_BASE;
                                    }
                                    None => {
                                        self.func_misses[i] = self.func_misses[i].saturating_add(1);
                                        if self.func_misses[i] >= 3 {
                                            self.func_supported[i] = Some(false);
                                            self.func_state[i] = None; // hide the toggle
                                                                       // Schedule the next retry, then double the wait for
                                                                       // the one after (capped) — a func that keeps failing
                                                                       // must stop costing a CAT timeout on a fixed cycle.
                                            self.func_retry_at[i] = self
                                                .rig_poll_ticks
                                                .saturating_add(self.func_retry_backoff[i]);
                                            self.func_retry_backoff[i] = self.func_retry_backoff[i]
                                                .saturating_mul(2)
                                                .min(FUNC_RETRY_BACKOFF_MAX);
                                        }
                                    }
                                }
                                {
                                    let mut eng = engine_lock(engine);
                                    eng.observe_rig_funcs(self.func_state);
                                }
                            }
                        }
                    }
                    // The dial probe is the CAT health check. On a REAL CAT rig a
                    // failure/timeout here means the link went half-open (writes
                    // succeed, replies never arrive) — trip the circuit breaker so
                    // the `cat_ok != Some(false)` guard above stops polling and the
                    // slot loop no longer blocks ~2.5 s every cycle, keying overs
                    // seconds late. Recovers on the next successful retune
                    // (set_freq/set_mode) or a Test-CAT reprobe. A VOX/serial rig
                    // has no control channel — its read_freq errors instantly and
                    // means nothing, so it must NOT trip the breaker.
                    Err(e) => {
                        // A real CAT rig tolerates a few consecutive misses before tripping — a slow
                        // reply cut off by the short serial deadline must not permanently kill
                        // read-back. A VOX/serial rig errors instantly + meaninglessly: never counts.
                        if rig.has_control() {
                            self.freq_misses = self.freq_misses.saturating_add(1);
                        }
                        if rig.has_control() && self.freq_misses >= FREQ_MISS_LIMIT {
                            self.cat_ok = Some(false);
                            // Arm the re-probe. Without this the breaker is a one-way door: it
                            // gates both read-back paths, and the only other clearer is a
                            // successful set_freq/set_mode, which the retune block does not send
                            // while the commanded dial/mode already match `last_dial`/`last_mode`.
                            if !breaker_probe {
                                self.cat_retry_ms = CAT_RETRY_BASE_MS;
                            }
                            self.cat_retry_at = now + self.cat_retry_ms;
                            // Re-probe funcs on recovery; don't leave stale toggle states shown.
                            self.func_supported = [None; 5];
                            self.func_misses = [0; 5];
                            self.func_state = [None; 5];
                            // Name the rig config in the diagnostic too, so a capture taken while
                            // the fault is ongoing records model/port/baud (the spawn note may
                            // predate logging being armed).
                            crate::civ::diag::note(&format!(
                                "CAT down: model={} port={:?} baud={} conn={}",
                                self.applied.rig_model,
                                self.applied.serial_port,
                                self.applied.baud,
                                self.applied.rig_conn
                            ));
                            let msg = with_backend(
                                cat_down_message(&self.applied, &e),
                                self.live_backend_label(&self.applied),
                            );
                            {
                                let mut eng = engine_lock(engine);
                                // Clear the read-backs so a dead link doesn't freeze the
                                // S-meter needle or flash a stale mode-mismatch tag.
                                eng.clear_rig_smeter();
                                eng.clear_rig_mode();
                                eng.clear_rig_funcs();
                                eng.clear_rig_passband();
                                eng.set_cat_status(Some(false), msg);
                            }
                        }
                    }
                }
            }

            // Fast dial-only mirror: the dial is the one value that must track a manual VFO knob in
            // real time (a 1–2 s lag made live tuning feel unusable — HRD tracks Yaesu in ~⅕ s with
            // pure fast polling). Runs on the fast cadence when the heavy read-back above did NOT (it
            // stamps last_freq_poll, so never a double read), never right after an app retune (that
            // branch defers it), under the same TX-safety + CAT-health gates. A read miss here is
            // ignored — the 750 ms heavy poll stays the authoritative CAT health probe / breaker.
            if !retuned
                && self.tx_until_ms.is_none()
                && !self.tuning_keyed
                && !self.manual_ptt_applied
                && self.cat_ok != Some(false)
                && self.freq_misses == 0 // a heavy-poll miss pauses fast reads until it recovers
                && now - self.last_freq_poll >= FREQ_POLL_MS
            {
                self.last_freq_poll = now;
                if let Ok(hz) = rig.read_freq() {
                    if hz != self.last_dial {
                        self.last_dial = hz;
                        {
                            let mut eng = engine_lock(engine);
                            eng.observe_rig_freq(hz);
                        }
                    }
                }
            }

            // Apply a pending SPLIT request (after the dial/mode retune so the TX
            // VFO programs against the fresh dial). Pile-up spots ("UP 2") set it;
            // any plain QSY clears it back to simplex.
            if can_retune {
                if let Some(req) = split_req {
                    match req {
                        Some(tx_mhz) => {
                            let tx_hz = (tx_mhz * 1_000_000.0).round() as u64;
                            // Which VFO carries the TX dial — "VFOB" (the
                            // shipped A/B split, every terrestrial pile-up and
                            // every A/B-mapped rig, byte-identical) or "Sub"
                            // (Main = downlink / Sub = uplink: the native CI-V
                            // backend engages the rig's SATELLITE MODE and
                            // select-writes the Sub band — an IC-9700 cannot
                            // cross-band on A/B split). An Err is a mapping the
                            // rig cannot run (Main = uplink; satellite mode
                            // fixes TX on Sub): NOTHING is sent — silently
                            // accepting it would transmit on the operator's
                            // own downlink.
                            //
                            // A 9700 served by REAL Hamlib rigctld (not our
                            // native daemon) gets `S 1 Sub` + `I` + `X` here;
                            // Hamlib's satmode split handling is version-
                            // dependent, and the reliable recipe there is
                            // `U SATMODE 1`, then `V Main`/`F`/`M` and
                            // `V Sub`/`F`/`M`, verified with `v`/`f` — wire it
                            // if a Hamlib-driven 9700 station ever surfaces.
                            //
                            // ⚠️ A `let`, NEVER a `match` scrutinee: under
                            // edition 2021 a scrutinee temporary lives to the
                            // END of the match, so `match { engine_lock(…) … }`
                            // held the engine guard through both arms — and
                            // every arm re-locks it (`split_rejected`,
                            // `rig_split_applied`, `sat_tx_mode_for_split`).
                            // `std::sync::Mutex` is not reentrant: the loop
                            // thread deadlocked on itself HOLDING the engine
                            // mutex, every Tauri command queued behind it, and
                            // Windows killed the frozen window (the 0.24.3
                            // sat-pick hang). The `let` ends the guard at the
                            // `;`, which also keeps the engine lock off the CAT
                            // round-trips below — the loop's own
                            // lock-scoped-then-I/O discipline.
                            let tx_vfo = { engine_lock(engine).sat_split_tx_vfo(tx_hz) };
                            match tx_vfo {
                                Err(reason) => {
                                    // The mapping itself is undrivable: nothing
                                    // was (or will be) sent, and the desired
                                    // state must not outlive the refusal.
                                    {
                                        let mut eng = engine_lock(engine);
                                        eng.split_rejected(tx_mhz);
                                    }
                                    retune_note = Some(reason);
                                    self.last_split_mode = None;
                                }
                                Ok(tx_vfo) => {
                                    // The operator-facing TX-VFO name for notes.
                                    let vfo_name = if tx_vfo == "Sub" { "Sub" } else { "VFO B" };
                                    let ok = rig.set_split(true, tx_vfo).is_ok()
                                        && rig.set_split_freq(tx_hz).is_ok();
                                    retune_note = Some(if ok {
                                        self.split_on_sub = tx_vfo == "Sub";
                                        // The rig ACKNOWLEDGED the split TX dial
                                        // (the native Sub path additionally read
                                        // it back) — report it DONE for the
                                        // binding rail. Gated on a real control
                                        // channel like the dial acknowledgment.
                                        if rig.has_control() {
                                            engine_lock(engine).rig_split_applied(tx_hz);
                                        }
                                        format!("split ON — TX {tx_mhz:.4} MHz ({vfo_name})")
                                    } else {
                                        // The desired state must not outlive the rejection —
                                        // a SPLIT badge claiming a split the rig isn't
                                        // running would burn the operator mid-pile-up.
                                        {
                                            let mut eng = engine_lock(engine);
                                            eng.split_rejected(tx_mhz);
                                        }
                                        "rig rejected split — work the pile-up manually".to_string()
                                    });
                                    // The TX VFO's MODE, while a satellite pass holds it. On
                                    // a linear INVERTING transponder the sidebands swap —
                                    // listen USB, transmit LSB — and `M` only ever reaches
                                    // the RX VFO, so `X` here is the one place the uplink's
                                    // sideband can be commanded at all.
                                    //
                                    // Consulted per SPLIT, not per hold: this one-shot also
                                    // serves the terrestrial pile-up path ("UP 5"), and a
                                    // transponder hold legitimately outlives its pick (a
                                    // pre-AOS pick is the normal flow). The engine answers
                                    // only when `tx_hz` IS its own corrected uplink, so a
                                    // pile-up split worked while a bird is held can never
                                    // be put in the bird's swapped sideband.
                                    //
                                    // Written only when the ANSWER changes. The split VFO's
                                    // mode cannot be read back, so re-asserting it every
                                    // correction would silently overrule an operator who
                                    // reached for the rig's own mode knob — the same
                                    // don't-fight discipline the frequency side gets from
                                    // `sat_observe_operator_tune`. See `Engine::sat_tx_mode`.
                                    if ok {
                                        let want_md =
                                            { engine_lock(engine).sat_tx_mode_for_split(tx_hz) };
                                        if want_md != self.last_split_mode {
                                            match &want_md {
                                                Some(md) => {
                                                    // ONE attempt per distinct answer, whether it
                                                    // lands or not. `ok` above already proves CAT
                                                    // answered this instant, so a refusal here is
                                                    // a backend with no `X` verb rather than a
                                                    // hiccup — and retrying it on every correction
                                                    // would spam the bus and the status line for
                                                    // the whole pass. A CHANGED answer (new bird,
                                                    // new transponder, operator sideband change)
                                                    // re-arms it. Same give-up-and-say-so shape as
                                                    // the RX side's `mode_giveup`.
                                                    //
                                                    // The failure is REPORTED, never assumed away:
                                                    // an uplink left in the wrong sideband sounds
                                                    // exactly like nobody answering, and the
                                                    // operator can fix it from the front panel in
                                                    // seconds once they know.
                                                    let sent = rig
                                                        .set_split_mode(md, passband_for(md))
                                                        .is_ok();
                                                    retune_note = Some(if sent {
                                                        format!(
                                                            "split ON — TX {tx_mhz:.4} MHz {md} ({vfo_name})"
                                                        )
                                                    } else {
                                                        format!(
                                                            "rig would not set the TX mode — put {vfo_name} in {md} by hand"
                                                        )
                                                    });
                                                    self.last_split_mode = want_md.clone();
                                                }
                                                // Nothing holds the TX mode any more (transponder
                                                // released / LOS). We stop writing it and
                                                // deliberately do NOT rewind the rig — exactly as
                                                // releasing the transponder hands the dial back
                                                // rather than restoring where it used to be.
                                                None => self.last_split_mode = None,
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            // Back to simplex — TX returns to the main/RX VFO, which
                            // carries its own mode again. A split that rode the SUB
                            // BAND is released the same way it was engaged: the
                            // backend leaves satellite mode (firing `0F 00` at a rig
                            // in satellite mode would strand it there).
                            let cleared = rig
                                .set_split(false, if self.split_on_sub { "Sub" } else { "VFOA" })
                                .is_ok();
                            if self.split_on_sub && !cleared {
                                // The rig would NOT leave satellite mode — it is
                                // still in it, TX still exits the Sub band, and
                                // pretending simplex would hide that. Keep the
                                // session marked (so the next release goes through
                                // the satmode path, never `0F`) and tell the
                                // operator, who can clear it at the front panel.
                                retune_note = Some(
                                    "rig would not leave satellite mode — turn SATELLITE \
                                     off on the rig"
                                        .to_string(),
                                );
                            } else {
                                self.split_on_sub = false;
                            }
                            self.last_split_mode = None;
                        }
                    }
                }
            }

            // Surface the mode-set outcome to the CAT status so the operator can SEE the mode
            // the rig was commanded into (and any rejection) — emitted only on a real change
            // or failure, so it never spams. A success implies CAT is alive (Some(true)).
            if let Some(note) = dial_note.or(retune_note) {
                let ok = if note.starts_with("rig set to") {
                    Some(true)
                } else {
                    self.cat_ok
                };
                {
                    let mut eng = engine_lock(engine);
                    eng.set_cat_status(ok, note);
                }
            }
        }

        // CW keying: feed the rig ONE WORD AT A TIME, paced so at most one word is ever in
        // the rig's keyer buffer. That is what lets Stop TX actually interrupt a long macro:
        // the abort clears the engine's word queue, so every word not yet sent is dropped
        // (a whole-macro `send_morse` blob would keep keying out of the rig's buffer past the
        // one `\stop_morse`). Operator-initiated; the engine gates on tx_enabled + privileges.
        {
            let ready = now >= self.cw_busy_until;
            let (abort, wpm, word, soundcard, pitch, winkeyer_port, serial_key) = {
                let mut eng = engine_lock(engine);
                (
                    eng.take_cw_abort(),
                    eng.cw_wpm(),
                    if ready { eng.poll_cw_one() } else { None },
                    eng.cw_soundcard(),
                    eng.cw_pitch_hz(),
                    eng.cw_winkeyer_port(),
                    eng.cw_serial_key_port()
                        .map(|p| (p, eng.cw_serial_key_line())),
                )
            };
            #[cfg(not(feature = "serial"))]
            {
                let _ = (&winkeyer_port, &serial_key); // only the serial build keys these
            }
            // Switched away from a serial-port keyer → release its port.
            #[cfg(feature = "serial")]
            if winkeyer_port.is_none() {
                self.winkeyer = None;
            }
            #[cfg(feature = "serial")]
            if serial_key.is_none() {
                self.serial_keyer = None;
            }
            if abort {
                let _ = rig.stop_morse(); // CAT keyer abort (cut the one word in the rig buffer)
                                          // WinKeyer abort: one Clear Buffer byte stops keying + flushes its queue.
                #[cfg(feature = "serial")]
                if let Some((_, wk)) = self.winkeyer.as_mut() {
                    let _ = wk.clear();
                }
                // Serial keyline abort: key up NOW + drop the rest of the macro.
                #[cfg(feature = "serial")]
                if let Some((_, _, sk)) = self.serial_keyer.as_ref() {
                    sk.clear();
                }
                if soundcard {
                    // Soundcard abort: dump the queued tone audio + unkey now.
                    backend.flush_output();
                    let _ = rig.ptt(false);
                    self.tx_until_ms = None;
                }
                self.cw_busy_until = 0.0; // a fresh macro after Stop keys immediately
            }
            if let Some(text) = word {
                // Hold the next word until this one finishes keying + a word space (7 dits),
                // so only ONE word is buffered in the rig at a time.
                let unit_ms = 1200.0 / wpm.clamp(5, 60) as f64;
                self.cw_busy_until =
                    now + tempo_core::cw::morse_duration_ms(&text, wpm) + 7.0 * unit_ms;
                let mut handled = false;
                // WinKeyer hardware keyer: open the serial port on demand (reopen if the
                // configured port changed) and stream the word to it. On open failure,
                // fall through to the CAT keyer so CW still goes out.
                #[cfg(feature = "serial")]
                if let Some(port) = &winkeyer_port {
                    let reopen = self
                        .winkeyer
                        .as_ref()
                        .map(|(p, _)| p != port)
                        .unwrap_or(true);
                    if reopen {
                        self.winkeyer = crate::winkeyer::WinKeyer::open(port)
                            .ok()
                            .map(|(wk, _rev)| (port.clone(), wk));
                    }
                    if let Some((_, wk)) = self.winkeyer.as_mut() {
                        if wpm != self.last_cw_wpm && wk.set_wpm(wpm).is_ok() {
                            self.last_cw_wpm = wpm;
                        }
                        let _ = wk.send(&text);
                        handled = true;
                    }
                }
                // Serial DTR/RTS keyline keyer: open the port on demand (reopen if the port
                // OR the line changed) and hand it the word — its own thread times the
                // keying, rig in CW. On open failure, surface a serial-specific error rather
                // than falling through to the CAT keyer (whose send_morse error would mislead).
                #[cfg(feature = "serial")]
                if !handled {
                    if let Some((port, line)) = &serial_key {
                        let reopen = self
                            .serial_keyer
                            .as_ref()
                            .map(|(p, l, _)| p != port || l != line)
                            .unwrap_or(true);
                        let mut open_err = None;
                        if reopen {
                            match crate::serial_keyer::SerialKeyer::open(
                                port,
                                crate::serial_keyer::KeyLine::parse(line),
                            ) {
                                Ok(sk) => {
                                    self.serial_keyer = Some((port.clone(), line.clone(), sk))
                                }
                                // Report what the SYSTEM said, verbatim. Guessing at causes
                                // while hiding the OS error is what sent the FTX-1 reporter
                                // to PowerShell to diagnose a refused baud rate by hand.
                                Err(e) => {
                                    self.serial_keyer = None;
                                    open_err = Some(format!(
                                        "Serial keyline: {e}. If the port name is right, check \
                                         that nothing else (CAT, another app) has it open."
                                    ));
                                }
                            }
                        }
                        if let Some((_, _, sk)) = self.serial_keyer.as_ref() {
                            sk.send(&text, wpm);
                        }
                        {
                            let mut eng = engine_lock(engine);
                            eng.set_cw_keyer_error(open_err);
                        }
                        handled = true; // the Serial backend owns this word (sent or errored)
                    }
                }
                if !handled {
                    if soundcard {
                        // Key a generated tone (rig in USB): PTT + play. Hold PTT across the
                        // inter-word gap (until the next word extends it) so the carrier
                        // stays up for the whole macro, not toggling per word.
                        let buf = tempo_core::cw::morse_samples(
                            &text,
                            wpm,
                            pitch,
                            tempo_fast::SAMPLE_RATE as u32,
                        );
                        if !buf.is_empty() {
                            // Capture PTT: if the rig won't key, the tone still plays locally so
                            // it LOOKS like it sent while nothing reaches the air — surface that
                            // instead of the silent false-positive. (Audio-routing problems can't
                            // be detected here — see the Soundcard control's caveat.)
                            self.ensure_commanded(rig); // read-only launch: assert before key
                            self.publish_tx_intent_now(); // before keying
                            let ptt_err = rig.ptt(true).is_err();
                            backend.play(&buf);
                            let until = self.cw_busy_until + crate::slot::TX_TAIL_MS;
                            self.tx_until_ms =
                                Some(self.tx_until_ms.map_or(until, |t| t.max(until)));
                            {
                                let mut eng = engine_lock(engine);
                                eng.set_cw_keyer_error(ptt_err.then(|| {
                                    "Soundcard keyer: the rig didn't accept PTT. Check your PTT \
                                     method + that Nexus's audio output is routed to the rig \
                                     (like FT8). If in doubt, use the WinKeyer or CAT keyer."
                                        .to_string()
                                }));
                            }
                        }
                    } else {
                        // CAT keyer: the rig generates CW from the word via send_morse. Many
                        // Hamlib backends accept freq/mode/PTT but NOT send_morse (`b`), so
                        // capture the result and SURFACE a failure instead of keying into
                        // the void — point the operator at the Soundcard keyer.
                        if wpm != self.last_cw_wpm && rig.set_keyspd(wpm).is_ok() {
                            self.last_cw_wpm = wpm;
                        }
                        self.ensure_commanded(rig); // read-only launch: assert before key
                        let cw_err = rig.send_morse(&text).is_err();
                        {
                            let mut eng = engine_lock(engine);
                            eng.set_cw_keyer_error(cw_err.then(|| {
                                "Your rig didn't accept CAT CW keying (Hamlib send_morse). \
                                 Use the WinKeyer keyer, or the Soundcard keyer (which needs \
                                 Nexus's audio routed to the rig)."
                                    .to_string()
                            }));
                        }
                    }
                }
            }
        }

        // RTTY keying: feed ONE MESSAGE at a time, paced on the REAL bit-stream
        // duration (fsk_schedule computes it from the framed Baudot stream — never
        // guessed), so Stop TX between messages drops the rest of the queue before it
        // reaches the rig. Operator-initiated only: the engine's poll_rtty_one gates
        // on tx_enabled + privileges + the Rtty operating-mode ownership + not-tuning
        // (the FT8/FT1 slot sequencer is gated off for non-Digital the same way, so
        // the two can never key together). Send-and-done — no diddle idle in v1: PTT
        // drops when the stop bit ends (tx_until_ms expiry), on Stop/halt (the abort
        // below), on the watchdog trip (poll_rtty_one arms the abort), and at app
        // exit (the SHUTDOWN flush).
        {
            let ready = now >= self.rtty_busy_until;
            let (abort, msg, baud, shift, reverse, fsk_port_line) = {
                let mut eng = engine_lock(engine);
                // Keep the cockpit's sending indicator honest each tick: an over is
                // "sending" until its computed duration has fully played out.
                eng.set_rtty_sending(now < self.rtty_busy_until);
                // Service the RTTY auto-sequencer BEFORE poll_rtty_one, so any over it
                // produces this tick (on_tx_complete → the next reply, or a silence
                // timeout → AGN/CQ) is picked up by the poll below. on_tx_complete
                // fires exactly once per over, gated on the sending flag just stamped
                // above — which shares one Unix-millis clock epoch with the RX feed.
                eng.rtty_auto_service();
                (
                    eng.take_rtty_abort(),
                    if ready { eng.poll_rtty_one() } else { None },
                    eng.rtty_baud(),
                    eng.rtty_shift_hz(),
                    eng.rtty_reverse(),
                    eng.rtty_fsk_port().map(|p| (p, eng.rtty_fsk_line())),
                )
            };
            #[cfg(not(feature = "serial"))]
            {
                let _ = &fsk_port_line; // only the serial build keys the FSK line
            }
            // Switched away from the FSK keyer (AFSK now, or no port) → release the
            // port; the keyer's Drop aborts any keying and parks the line at mark.
            #[cfg(feature = "serial")]
            if fsk_port_line.is_none() {
                self.rtty_keyer = None;
            }
            if abort {
                // Stop TX mid-over (Stop button, halt_tx, watchdog trip, TX disarm):
                // stop the FSK keying thread NOW (line back to mark, queued bits
                // dropped), dump any queued AFSK audio, and unkey immediately.
                #[cfg(feature = "serial")]
                if let Some((_, _, k)) = self.rtty_keyer.as_ref() {
                    k.clear();
                }
                backend.flush_output();
                let _ = rig.ptt(false);
                self.tx_until_ms = None;
                self.rtty_busy_until = 0.0; // a fresh send after Stop keys immediately
                {
                    let mut eng = engine_lock(engine);
                    eng.set_rtty_sending(false);
                }
            }
            if let Some(text) = msg {
                // Text → ITA2 codes → the over-the-air bit stream (5 data bits per
                // char, LSB first). The SAME framed stream drives both backends, so
                // the schedule's total is the true on-air duration in either.
                let mut enc = tempo_core::rtty::BaudotEncoder::new(true);
                let bits = tempo_core::rtty::code_bits(&enc.encode(&text));
                let sched = crate::rtty_fsk::fsk_schedule(&bits, baud);
                if sched.total_ms > 0.0 {
                    // Hold the next queued message until this one has fully keyed
                    // out + one character of clear air (send-and-done, no diddle).
                    self.rtty_busy_until = now + sched.total_ms + 7.5 * (1000.0 / baud);
                    let mut handled = false;
                    // True FSK: the data bits ride the DTR/RTS keyline (the keyer
                    // thread times the edges against absolute deadlines; rig in RTTY
                    // mode). PTT rides its OWN path — CAT PTT or the separate PTT
                    // serial line — via rig.ptt, NEVER the keyed line (the engine
                    // refuses a send configured with both on one line).
                    #[cfg(feature = "serial")]
                    if let Some((port, line)) = &fsk_port_line {
                        let reopen = self
                            .rtty_keyer
                            .as_ref()
                            .map(|(p, l, _)| p != port || l != line)
                            .unwrap_or(true);
                        let mut open_err_msg = None;
                        if reopen {
                            match crate::rtty_fsk::FskKeyer::open(
                                port,
                                crate::rtty_fsk::KeyLine::parse(line),
                            ) {
                                Ok(k) => self.rtty_keyer = Some((port.clone(), line.clone(), k)),
                                // Same honesty rule as the CW keyline: the OS error IS the
                                // diagnosis (a refused baud rate reads nothing like a busy
                                // port), so pass it through instead of guessing.
                                Err(e) => {
                                    self.rtty_keyer = None;
                                    open_err_msg = Some(format!(
                                        "FSK keyline: {e}. If the port name is right, check that \
                                         nothing else (CAT, another app) has it open — or use \
                                         the AFSK backend."
                                    ));
                                }
                            }
                        }
                        let open_err = self.rtty_keyer.is_none();
                        let mut ptt_err = false;
                        if !open_err {
                            // Hoisted out of the keyer borrow below (read-only launch):
                            // assert dial/mode BEFORE the key, same as every other site.
                            self.ensure_commanded(rig);
                        }
                        if let Some((_, _, k)) = self.rtty_keyer.as_ref() {
                            // PTT immediately before the bits start; the computed
                            // duration rides tx_until_ms so the existing expiry
                            // unkeys the moment the final stop bit ends (+ tail).
                            self.publish_tx_intent_now(); // before keying
                            ptt_err = rig.ptt(true).is_err();
                            k.send(bits.clone(), baud);
                            let until = self.rtty_busy_until + crate::slot::TX_TAIL_MS;
                            self.tx_until_ms =
                                Some(self.tx_until_ms.map_or(until, |t| t.max(until)));
                        }
                        if open_err {
                            // Nothing keyed — don't sit "busy" for a send that never started.
                            self.rtty_busy_until = 0.0;
                        }
                        {
                            let mut eng = engine_lock(engine);
                            eng.set_rtty_sending(!open_err);
                            eng.set_rtty_keyer_error(if open_err {
                                open_err_msg
                            } else if ptt_err {
                                Some(
                                    "FSK keyer: the rig didn't accept PTT. Check your PTT \
                                     method (CAT, or the separate PTT line) — the FSK data \
                                     line never doubles as PTT."
                                        .to_string(),
                                )
                            } else {
                                None
                            });
                        }
                        handled = true; // the FSK backend owns this message (sent or errored)
                    }
                    if !handled {
                        // Soundcard AFSK (rig in LSB): render the SAME framed bit
                        // stream to the phase-continuous two-tone waveform and play
                        // it through the SAME TX audio output the FT8 modem uses —
                        // one route, so the operator's tx_level / drive / ALC
                        // discipline applies to RTTY exactly as to FT8. PTT around
                        // it like the soundcard CW keyer.
                        let cfg = crate::rtty_afsk::AfskConfig {
                            space_hz: crate::rtty_afsk::MARK_HZ + shift as f32,
                            baud,
                            reverse,
                            ..crate::rtty_afsk::AfskConfig::default()
                        };
                        let buf = crate::rtty_afsk::afsk_char_samples(&bits, &cfg);
                        if !buf.is_empty() {
                            self.ensure_commanded(rig); // read-only launch: assert before key
                            self.publish_tx_intent_now(); // before keying
                            let ptt_err = rig.ptt(true).is_err();
                            backend.play(&buf);
                            let until = self.rtty_busy_until + crate::slot::TX_TAIL_MS;
                            self.tx_until_ms =
                                Some(self.tx_until_ms.map_or(until, |t| t.max(until)));
                            {
                                let mut eng = engine_lock(engine);
                                eng.set_rtty_sending(true);
                                eng.set_rtty_keyer_error(ptt_err.then(|| {
                                    "AFSK keyer: the rig didn't accept PTT. Check your PTT \
                                     method + that Nexus's audio output is routed to the rig \
                                     (like FT8)."
                                        .to_string()
                                }));
                            }
                        }
                    }
                }
            }
        }

        // APRS beacon: an explicit one-shot position beacon. The engine already rendered the
        // AFSK-1200 audio (12 kHz mono); key PTT, play it, and drop PTT via `tx_until_ms` — the
        // same seam the voice/CW soundcard keyer uses. The `tx_until_ms` guard covers the
        // one-shot overs, `!manual_ptt_applied` covers a PHYSICALLY held mic (whose unkey path
        // deliberately refuses to drop PTT under a held key — an injected packet would ride the
        // live over), and poll_aprs_tx's tx_owner() gate covers everything the engine can see.
        if self.tx_until_ms.is_none() && !self.manual_ptt_applied {
            let beacon = Some(engine_lock(engine)).and_then(|mut e| e.poll_aprs_tx());
            if let Some(buf) = beacon.filter(|b| !b.is_empty()) {
                self.ensure_commanded(rig); // read-only launch: assert before key
                self.publish_tx_intent_now();
                let _ = rig.ptt(true);
                backend.play(&buf);
                let dur_ms = buf.len() as f64 / 12.0; // 12 kHz mono → milliseconds
                self.tx_until_ms = Some(now + dur_ms + crate::slot::TX_TAIL_MS);
            }
        }

        // Voice keyer (phone): play a recorded message to the rig (PTT + 12 kHz mono
        // samples, drop PTT when played out — same TX path as the soundcard CW keyer),
        // and, while recording, accumulate the captured frame into the engine's buffer.
        // One engine lock for both. Gated on `tx_enabled` (Monitor) inside the engine.
        {
            // Voice-mic recording source: while a VOICE-MESSAGE recording is in
            // progress AND the operator configured a dedicated voice-mic device, capture
            // the operator's voice from a SECOND transient input stream on that device —
            // instead of the shared tap, which on a digital setup is the rig's RX codec
            // (so recording a voice message would otherwise record the band). QSO
            // recording is deliberately NOT mic-routed: its documented job is capturing
            // the CONTACT (the received audio), which IS the shared tap. The mic
            // open/close takes the cpal host lock, so it runs OUTSIDE the engine lock, and
            // it never touches the main capture stream, so the decode path never restarts.
            let recording_active = {
                let eng = engine_lock(engine);
                eng.is_recording()
            };
            let want_mic =
                crate::backend::want_voice_mic(recording_active, &self.applied.voice_mic_device);
            if want_mic && !self.voice_mic_open && !self.voice_mic_failed {
                // Rising edge: open the mic once. A failed open surfaces why and falls back
                // to the shared tap; `voice_mic_failed` blocks a per-loop retry until the
                // recording ends (so we don't spam the device open every 20 ms).
                match backend.set_voice_mic(Some(&self.applied.voice_mic_device)) {
                    Ok(()) => self.voice_mic_open = true,
                    Err(e) => {
                        self.voice_mic_failed = true;
                        // Notice only over None or our own line — a real device
                        // error or a live monitor notice is not ours to stomp
                        // (review: the mic failure erased both kinds).
                        if matches!(self.err_owner, ErrOwner::None | ErrOwner::VoiceMic) {
                            {
                                let mut eng = engine_lock(engine);
                                eng.set_audio_error(Some(format!(
                                    "Voice mic could not open: {e} — recording from the shared \
                                     input instead"
                                )));
                            }
                            self.err_owner = ErrOwner::VoiceMic;
                        }
                    }
                }
            } else if !want_mic && (self.voice_mic_open || self.voice_mic_failed) {
                // Falling edge (recording ended / device cleared): close the mic stream,
                // clear retry suppression, and clear only a notice WE own — then nudge
                // the monitor block to re-surface its own guard/failure state if any
                // (its notice may have predated ours).
                if self.voice_mic_open {
                    backend.set_voice_mic(None).ok();
                    self.voice_mic_open = false;
                }
                self.voice_mic_failed = false;
                if self.err_owner == ErrOwner::VoiceMic {
                    {
                        let mut eng = engine_lock(engine);
                        eng.set_audio_error(None);
                    }
                    self.err_owner = ErrOwner::None;
                    self.monitor_reapply = true;
                }
            }
            // The audio the recorder ingests this iteration: the mic when its stream is
            // live, else the shared capture tap (today's behavior / the failed-open
            // fallback). Only the recorder switches source — the decoder always reads the
            // shared `captured` folded in at the top of the loop.
            let mic_samples: Vec<f32> = if self.voice_mic_open {
                backend.voice_capture()
            } else {
                Vec::new()
            };
            let rec_samples: &[f32] = if self.voice_mic_open {
                &mic_samples
            } else {
                &captured
            };

            let (abort, samples, qso_rec, qso_path) = {
                let mut eng = engine_lock(engine);
                if eng.is_recording() {
                    eng.push_record_samples(rec_samples);
                }
                (
                    eng.take_voice_abort(),
                    eng.poll_voice(),
                    eng.is_qso_recording(),
                    eng.qso_record_path(),
                )
            };
            if abort {
                backend.flush_output(); // dump queued message audio + unkey now
                let _ = rig.ptt(false);
                self.tx_until_ms = None;
            }
            if let Some(buf) = samples {
                if !buf.is_empty() {
                    let secs = buf.len() as f32 / tempo_fast::SAMPLE_RATE;
                    self.ensure_commanded(rig); // read-only launch: assert before key
                    self.publish_tx_intent_now(); // before keying — the fail-safe must already know
                    let ptt_err = rig.ptt(true).is_err();
                    backend.play(&buf);
                    let until = now + secs as f64 * 1000.0 + crate::slot::TX_TAIL_MS;
                    self.tx_until_ms = Some(self.tx_until_ms.map_or(until, |t| t.max(until)));
                    // A NAK here means the modem audio above went out while the rig stayed in
                    // RX — surface it instead of silent dead air.
                    self.report_ptt(engine, ptt_err);
                }
            }
            // QSO recording (audio bridge): stream the live RX capture straight to a WAV on
            // disk — open the sink on start, append each captured frame (the sink checkpoints
            // the header ~1×/s so an abnormal exit still leaves a readable file), finalize on
            // stop. No RAM buffer, so a multi-hour QSO stays bounded.
            match (qso_rec, self.qso_sink.is_some()) {
                (true, false) => {
                    if let Some(p) = qso_path {
                        match crate::voice::WavSink::create(&p) {
                            Ok(s) => {
                                self.qso_sink = Some(s);
                                self.qso_started_ms = Some(now);
                            }
                            // Don't spin re-trying every 20 ms: clear the engine flag (so the
                            // REC badge stops lying) and surface why via the audio-error chip.
                            Err(e) => {
                                let mut eng = engine_lock(engine);
                                eng.stop_qso_recording();
                                eng.set_audio_error(Some(format!(
                                    "Could not start QSO recording: {e}"
                                )));
                            }
                        }
                    }
                }
                (true, true) => {
                    if let Some(s) = self.qso_sink.as_mut() {
                        // Always the shared RX tap: the QSO recording is the
                        // CONTACT, never the operator's mic (which may be live
                        // for a simultaneous voice-message recording).
                        let _ = s.write(&captured);
                    }
                    // Safety auto-stop for a forgotten recording (mirrors the tune-carrier
                    // cap): the (false,true) arm next pass finalizes the file.
                    if let Some(start) = self.qso_started_ms {
                        if now - start > MAX_QSO_REC_MS {
                            {
                                let mut eng = engine_lock(engine);
                                eng.stop_qso_recording();
                            }
                        }
                    }
                }
                (false, true) => {
                    if let Some(s) = self.qso_sink.take() {
                        let _ = s.finish();
                    }
                    self.qso_started_ms = None;
                }
                (false, false) => {}
            }
        }

        // SSTV image transmit (phone): stream a pre-encoded 12 kHz image with PTT held for
        // its exact, precomputed duration. Generate-then-stream — the whole buffer is
        // encoded up front in the command layer; the loop feeds it to the output ring in
        // chunked look-ahead slices (never one giant `play`, so a 4.8-min PD290 can't peak
        // ~55 MB in the unbounded ring). Human-initiated only: the engine's `poll_sstv_tx`
        // gates on tx_enabled + privileges + Phone ownership + not-tuning, and the over's
        // length was bounded UP FRONT by `sstv_send`'s duration budget. PTT drops
        // unconditionally at the precomputed `tx_until_ms` (below), or earlier on
        // Stop/halt/disarm/exit via the abort. One image in flight, no queue.
        {
            let abort = {
                let mut eng = engine_lock(engine);
                eng.take_sstv_abort()
            };
            if abort {
                // Stop TX mid-image (Stop button, halt_tx, TX disarm): drop the feed, dump
                // any queued audio, and unkey immediately.
                self.sstv_feed = None;
                backend.flush_output();
                let _ = rig.ptt(false);
                self.tx_until_ms = None;
                {
                    let mut eng = engine_lock(engine);
                    eng.set_sstv_sending(false);
                }
            }
            // Start a new image ONLY when the transmitter is otherwise idle — SSTV shares
            // the Phone segment with the voice keyer + live mic PTT, so this backstop gives
            // the mutual exclusion RTTY gets for free from mode-exclusivity. Polling only
            // when idle also HOLDS the engine's job (poll_sstv_tx takes it) instead of
            // dropping it, so a busy tick can't lose the image.
            if self.sstv_feed.is_none()
                && self.tx_until_ms.is_none()
                && !self.tuning_keyed
                && !self.manual_ptt_applied
            {
                let job = {
                    let mut eng = engine_lock(engine);
                    eng.poll_sstv_tx()
                };
                if let Some(job) = job {
                    self.ensure_commanded(rig); // read-only launch: assert before key
                    self.publish_tx_intent_now(); // before keying — the fail-safe must already know
                    let ptt_err = rig.ptt(true).is_err();
                    // Hold PTT for the WHOLE image via the precomputed duration; the
                    // tx_until_ms expiry below drops it even if every other mechanism fails.
                    let until = now + job.duration_ms + crate::slot::TX_TAIL_MS;
                    self.tx_until_ms = Some(self.tx_until_ms.map_or(until, |t| t.max(until)));
                    {
                        let mut eng = engine_lock(engine);
                        eng.set_sstv_sending(true);
                    }
                    // A NAK here means modem audio would go out into a receiving rig — surface it.
                    self.report_ptt(engine, ptt_err);
                    self.sstv_feed = Some(SstvFeed {
                        samples: job.samples,
                        cursor: 0,
                        started_ms: now,
                        total_ms: job.duration_ms,
                    });
                }
            }
            // Chunked look-ahead feed + progress + completion.
            let done = if let Some(feed) = self.sstv_feed.as_mut() {
                let elapsed = now - feed.started_ms;
                // Keep ~SSTV_FEED_AHEAD_MS of audio queued ahead of playback, no more.
                while feed.cursor < feed.samples.len() {
                    let fed_ms = feed.cursor as f64 / SSTV_TX_RATE_HZ * 1000.0;
                    if fed_ms - elapsed >= SSTV_FEED_AHEAD_MS {
                        break;
                    }
                    let end = (feed.cursor + SSTV_CHUNK_SAMPLES).min(feed.samples.len());
                    backend.play(&feed.samples[feed.cursor..end]);
                    feed.cursor = end;
                }
                let played_ms = elapsed.clamp(0.0, feed.total_ms);
                let total_ms = feed.total_ms;
                {
                    let mut eng = engine_lock(engine);
                    eng.set_sstv_tx_progress(played_ms, total_ms);
                }
                // Done once every sample is fed AND the audio has played out; the PTT drop
                // rides the tx_until_ms expiry below (which never unkeys under a held mic).
                feed.cursor >= feed.samples.len() && elapsed >= feed.total_ms
            } else {
                false
            };
            if done {
                self.sstv_feed = None;
                {
                    let mut eng = engine_lock(engine);
                    eng.set_sstv_sending(false);
                }
            }
        }

        // Manual PTT (live phone) + RF power — applied via the rig on change. Only the
        // Phone section drives these (the FT8 TX path is idle there), so no PTT clash.
        {
            let (ptt, power) = {
                let mut eng = engine_lock(engine);
                let ptt = eng.manual_ptt();
                (ptt, eng.rf_power_to_command())
            };
            if ptt != self.manual_ptt_applied {
                if ptt {
                    self.ensure_commanded(rig); // read-only launch: assert before key
                    self.publish_tx_intent_now(); // before keying — the fail-safe must already know
                }
                // Report only a KEYING failure (a failed unkey is the watchdog's job); a clean
                // key or any unkey clears our own PTT status.
                let ptt_failed = rig.ptt(ptt).is_err();
                self.report_ptt(engine, ptt && ptt_failed);
                self.manual_ptt_applied = ptt;
            }
            if let Some((p, force)) = power {
                // Command on change OR when the cap must be re-asserted (`force`): a manual
                // knob-up past the ceiling is pulled back down even though our target is unchanged.
                if (force || Some(p) != self.last_rf_power) && rig.set_power(p).is_ok() {
                    self.last_rf_power = Some(p);
                }
            }
            let mic = Some(engine_lock(engine)).and_then(|e| e.mic_gain());
            if let Some(mg) = mic {
                if Some(mg) != self.last_mic_gain && rig.set_mic_gain(mg).is_ok() {
                    self.last_mic_gain = Some(mg);
                }
            }
            // RX DSP levels: NR level (0..1) + AGC speed — applied on change like mic gain.
            let (nr, agc) = {
                let e = engine_lock(engine);
                (e.nr_level(), e.agc())
            };
            if let Some(n) = nr {
                if Some(n) != self.last_nr_level && rig.set_rx_level("NR", n).is_ok() {
                    self.last_nr_level = Some(n);
                }
            }
            if let Some(a) = agc {
                if self.last_agc.as_deref() != Some(a.as_str())
                    && rig.set_agc(agc_to_hamlib(&a)).is_ok()
                {
                    self.last_agc = Some(a);
                }
            }
        }

        // Drop PTT once the transmitted audio has played out (+ a small tail). Do NOT
        // unkey while the operator is holding live PTT — they own the key then, so a
        // voice/CW message tail ending must not cut a live phone over (the manual-PTT
        // applier handles unkeying when the operator actually releases).
        if let Some(t) = self.tx_until_ms {
            if now >= t {
                if !self.manual_ptt_applied {
                    let _ = rig.ptt(false);
                }
                self.tx_until_ms = None;
                // Split restore happens in the catch-all below (single drain
                // point — per-site restores leaked through HaltTx/tune paths).
            }
        }

        // Transmit meters (SWR / ALC / Po / COMP) — the mirror image of the RX S-meter poll:
        // read ONLY while keyed (a tune carrier, a slot/CW/voice over, or live phone PTT), and
        // blanked on unkey so the bars never freeze on a stale reading. Read via the generic
        // `l NAME` level path, so it works on BOTH the native CI-V daemon (Icom 15 11/12/13/14)
        // and any Hamlib rig reporting these levels; an unsupported meter returns None and
        // simply doesn't render. Deliberately placed AFTER the PTT-drop above: a meter read is a
        // blocking CAT round-trip, so it must never sit upstream of the auto-unkey and hold the
        // transmitter keyed past the over on a slow rig. And only ONE meter is read per throttled
        // cycle (round-robin) so at most one blocking read lands per tick — four back-to-back
        // reads could stall a chunked tune/voice carrier if the rig answers slowly.
        {
            let keyed_now =
                self.tx_until_ms.is_some() || self.tuning_keyed || self.manual_ptt_applied;
            if keyed_now && self.cat_ok != Some(false) {
                if now - self.last_tx_meter_poll >= TX_METER_POLL_MS {
                    // RFPOWER_METER_WATTS (not RFPOWER_METER): Hamlib's plain RFPOWER_METER is a
                    // normalized 0..1, only the _WATTS variant is true watts — and the native
                    // daemon answers both with calibrated watts. So `tx_po_w` is watts on both.
                    let (swr, alc, po, comp) = match self.tx_meter_idx % 4 {
                        0 => (rig.read_meter_f32("SWR"), None, None, None),
                        1 => (None, rig.read_meter_f32("ALC"), None, None),
                        2 => (None, None, rig.read_meter_f32("RFPOWER_METER_WATTS"), None),
                        _ => (None, None, None, rig.read_meter_f32("COMP_METER")),
                    };
                    self.tx_meter_idx = self.tx_meter_idx.wrapping_add(1);
                    self.last_tx_meter_poll = now;
                    {
                        let mut eng = engine_lock(engine);
                        eng.observe_rig_tx_meters(swr, alc, po, comp);
                    }
                }
            } else if self.last_tx_meter_poll != 0.0 {
                // Just unkeyed (or CAT tripped): blank the bars once.
                self.last_tx_meter_poll = 0.0;
                self.tx_meter_idx = 0;
                {
                    let mut eng = engine_lock(engine);
                    eng.clear_rig_tx_meters();
                }
            }
        }

        // `mut`: a tier/period change below replaces the clock, and everything after
        // it must use the NEW numbering — see the rebuild.
        let mut slot = self.clock.slot_index(now);
        // ⚠ THIS GUARD IS HELD ACROSS BLOCKING CAT I/O for the rest of the tick —
        // set_freq, set_split, several ptt() paths and finish_boundary all run
        // under it, each up to the CAT deadline (700/2500 ms). The tune path
        // below drops it first and says why ("the hang convoy"); the TX, unkey
        // and self-heal paths never got the same treatment, so a wedged rig
        // still makes this the app's longest lock hold. What that can no longer
        // do is hang the WINDOW: every Tauri command that takes this mutex now
        // runs off the UI thread (`#[tauri::command(async)]`, guarded by
        // src-tauri's `no_engine_locking_command_runs_on_the_ui_thread`), so a
        // stall costs a late readout, not a dead message pump. That guarantee is
        // NOT free and it is what caps this hold: those commands wait on a tokio
        // WORKER thread, so a hold long enough to have several of them queued at
        // once eats the worker pool. The CAT deadline (2500 ms) is what keeps
        // that bounded — do not add an unbounded wait under this guard. Hoisting the CAT
        // calls out of the guard here is the real repair and it reorders the
        // transmit path — maintainer sign-off first (CLAUDE.md TX-sequencing rule).
        let mut eng = engine_lock(engine);
        // Split-Operation teardown catch-all: the moment NO over is pending,
        // restore a Fake-It-shifted VFO and drop an audio Rig-split. ONE drain
        // point, deliberately not per-exit-path: expiry, hard stop, UDP HaltTx
        // and a tune supersede all just clear tx_until_ms, and per-site
        // restores provably leaked (review: stranded shifted dial = every
        // subsequent decode/spot/log on a wrong frequency). Deferred while the
        // operator holds live phone PTT — never move the VFO under a live over.
        if self.tx_until_ms.is_none() && !self.manual_ptt_applied {
            if let Some(hz) = self.fake_it_restore.take() {
                let _ = rig.set_freq(hz);
                // Settle the poll guards so the knob-QSY detector can't adopt
                // a not-yet-restored read-back as an operator QSY (fast mirror deferred a full
                // heavy interval, matching the retune path).
                self.last_dial = hz;
                self.last_rig_poll = now;
                self.last_freq_poll = now + (RIG_POLL_MS - FREQ_POLL_MS);
            }
            if self.audio_rig_split {
                self.audio_rig_split = false;
                // The cluster SPLIT-on-Work owns VFO B when active — leave it.
                if !eng.cluster_split_active() {
                    let _ = rig.set_split(false, "VFOA");
                }
            }
        }

        // Operator hit Erase → mirror it to cooperating apps (UDP Clear).
        if let Some(window) = eng.take_pending_udp_clear() {
            if let Some(server) = sinks.wsjtx {
                let _ = server.send_clear(window);
            }
        }

        // Deferred "Disable Tx after sending 73": only once the final over has
        // fully played out (tx_until cleared) — disabling mid-over would trip
        // the hard-stop path above and cut the 73 itself.
        if self.tx_until_ms.is_none() && eng.take_pending_tx_disable() {
            eng.set_tx_enabled(false);
        }
        // Deferred WSJT-X-style CW ID: the final 73 has fully left the air —
        // key MYCALL through the normal CW path (PTT + tone), like the CW
        // cockpit does. Consumed only on TX-idle for the same reason as the
        // deferred disable above.
        if self.tx_until_ms.is_none() && eng.take_pending_cw_id() {
            let mycall = eng.settings().mycall.clone();
            eng.send_cw(&mycall);
        }
        // Pick up the latest measured clock offset for the NEXT iteration's UTC
        // steering (the NTP probe thread writes it onto the engine).
        self.clock_offset_ms = eng.clock_offset_ms().unwrap_or(0);
        // Keep the TopBar's next-slot countdown live every iteration.
        eng.set_slot_timing(self.clock.ms_to_next_slot(now) as u64);
        // RX input meter + live waterfall audio (decoupled from the slot decoder).
        eng.set_rx_level(backend.rx_level());
        // The WATERFALL row is NOT produced here any more — see rxtap.rs / rxdsp.rs. This loop
        // issues every blocking CAT call (up to 2500 ms on slow serial), and while it was also
        // the sole producer of spectrum rows, any CAT stall froze the waterfall. These mode
        // taps (CW/RTTY/APRS/SSTV/QSO) deliberately stay: the loop already holds this lock, so
        // they cost nothing here, and moving them would risk dropping audio under contention.
        eng.feed_rx_audio(&captured);

        // --- Tune carrier: hold PTT + a steady f0 sine while the operator holds
        // "tune", with a safety auto-release. Normal slot TX is suppressed. ---
        let mut is_tuning = eng.tuning();
        if is_tuning {
            if let Some(start) = self.tune_started_ms {
                // Operator-configurable auto-release (WSJT-X "Tune after t s"),
                // floored at 1 s and CLAMPED to the MAX_TUNE_MS hard ceiling.
                let max_ms =
                    ((eng.settings().tune_timeout_secs.max(1) as f64) * 1000.0).min(MAX_TUNE_MS);
                if now - start > max_ms {
                    eng.set_tune(false);
                    is_tuning = false;
                }
            }
        }
        if is_tuning {
            let keying = !self.tuning_keyed;
            // Drop the ENGINE lock before the CAT+audio work: a slow/wedged daemon must
            // freeze this tick, not every UI command sharing the mutex (the hang convoy).
            drop(eng);
            if keying {
                // Icom-native only: a plain-USB/LSB Icom takes TX audio from the MIC, so
                // a keyed tune tone via the USB codec radiates ZERO RF ("red light, no
                // signal"). Flip DATA mode on for the tune (this exact sequence — set DATA,
                // then PTT — is the known-good keying path; don't skip it or the CI-V PTT
                // won't hold). We remember the pre-tune data state so the release RESTORES it
                // instead of forcing DATA off: an FT8 (DATA-U) operator must stay in DATA-U.
                // Yaesu/hamlib paths untouched.
                self.tune_was_data = mode_is_data(&self.last_mode);
                if let Some(d) = self.rigctld_proc.as_ref().and_then(CatDaemon::native) {
                    // Clear the scope stream off the bus BEFORE keying (the retune gate at ~1401
                    // only catches it a tick later), so the tune carrier keys onto an idle bus.
                    d.set_scope_enabled(false);
                    d.set_data_mode(true);
                }
                self.ensure_commanded(rig); // read-only launch: assert before key
                self.publish_tx_intent_now(); // before keying — the fail-safe must already know
                let _ = rig.ptt(true);
                self.tuning_keyed = true;
                self.tune_started_ms = Some(now);
                self.tx_until_ms = None; // a tune supersedes any pending slot TX tail
            }
            let n = (tempo_fast::SAMPLE_RATE * (TUNE_CHUNK_MS / 1000.0)) as usize;
            let chunk = tune_carrier(
                TUNE_FREQ_HZ,
                n,
                tempo_fast::SAMPLE_RATE,
                &mut self.tune_phase,
            );
            backend.play(&chunk);
            self.rx.clear(); // don't decode our own carrier
            return Ok(());
        } else if self.tuning_keyed {
            // Tuning just released: drop PTT and re-anchor to the slot grid. The keyed
            // flag only clears on a SUCCESSFUL unkey (fail-safe Rig::ptt), so a miss
            // here is retried by the idle self-heal below.
            crate::civ::diag::note("tune released: unkey (tune ended or Tune toggled off)");
            let _ = rig.ptt(false);
            if let Some(d) = self.rigctld_proc.as_ref().and_then(CatDaemon::native) {
                // Restore the PRE-TUNE data state — NOT a hardcoded OFF. An FT8/DATA-U operator
                // (tune_was_data) stays in DATA-U; only a plain USB/LSB operator gets DATA off.
                d.set_data_mode(self.tune_was_data);
            }
            self.tuning_keyed = false;
            self.tune_started_ms = None;
            self.last_slot = None;
            self.prev_slot_was_tx = false;
        }

        // Hard Stop TX: if transmit was disabled mid-over (the UI "Stop TX" button
        // calls engine.halt_tx, or a logger sent HaltTx), cut the CURRENT
        // transmission immediately — drop PTT and discard the queued TX audio
        // rather than letting the slot's audio play out to its deadline.
        if self.tx_until_ms.is_some() && !eng.tx_enabled() {
            crate::civ::diag::note("hard-stop TX: tx_enabled went false mid-over → unkey");
            let _ = rig.ptt(false);
            backend.flush_output();
            self.tx_until_ms = None;
        }

        // IDLE SELF-HEAL (TX safety): the loop believes the radio should be receiving,
        // but the fail-safe keyed flag says a previous unkey never succeeded (wedged
        // CI-V, rigctld hiccup). Retry key-up every tick until the radio acknowledges —
        // this is what turns "stuck TX light until the radio reboots" into a self-
        // recovering blip. One idempotent CAT call per tick, only while desynced.
        if rig.keyed && self.tx_until_ms.is_none() && !self.tuning_keyed && !self.manual_ptt_applied
        {
            crate::civ::diag::note("idle self-heal: rig still keyed but loop thinks RX → unkey");
            let _ = rig.ptt(false);
        }

        // Inbound WSJT-X control (HaltTx / FreeText / Reply) from a logger / JTAlert.
        // BOUNDED per tick: this whole block runs with the engine lock held and a
        // HaltTx spends a blocking CAT round trip (up to 2500 ms on slow serial),
        // so the old unbounded drain let one stuck consumer own the engine mutex —
        // and every Tauri command queued behind it — for as long as it kept
        // sending. The overflow is not dropped, just deferred one 20 ms tick.
        if let Some(server) = sinks.wsjtx {
            for inb in server.drain(tempo_net::server::INBOUND_PER_TICK) {
                match inb {
                    WsjtxInbound::HaltTx { .. } => {
                        eng.halt_tx();
                        let _ = rig.ptt(false);
                        backend.flush_output();
                        self.tx_until_ms = None;
                    }
                    WsjtxInbound::Clear { .. } => {
                        // Visual clear only — the engine's decode context (answer
                        // parity / history) is not a window and stays intact.
                        eng.apply_udp_clear();
                    }
                    WsjtxInbound::Replay { .. } => {
                        // A consumer that just connected wants the WHOLE current
                        // period back — `last_decodes` alone holds only the most
                        // recent ingest (post-early-pass it's just the boundary
                        // stragglers). NO PSK spots here: replays must never
                        // double-spot.
                        if let Some(server) = sinks.wsjtx {
                            let tier = tier_mode(eng.tier());
                            let ms_mid = (now as u64 % 86_400_000) as u32;
                            for d in eng.current_period_decodes() {
                                let _ = server.send_decode(&build_decode(
                                    &d.message,
                                    d.snr,
                                    d.dt,
                                    d.freq,
                                    tier,
                                    ms_mid,
                                    d.qual < 0.17,
                                ));
                            }
                        }
                    }
                    WsjtxInbound::Location { location, .. } => {
                        eng.apply_udp_location(&location);
                    }
                    WsjtxInbound::HighlightCallsign { call, bg, fg, .. } => {
                        eng.set_highlight(&call, bg, fg);
                    }
                    WsjtxInbound::FreeText { text, send, .. } => {
                        let t = text.trim();
                        if send && !t.is_empty() {
                            eng.broadcast(t);
                        }
                    }
                    WsjtxInbound::Reply {
                        message,
                        snr,
                        delta_freq,
                        ..
                    } => {
                        // The Reply datagram (a logger/JTAlert/companion double-click)
                        // carries the exact clicked line, its SNR, and the DX's audio
                        // offset — pass all three so the sequencer resumes from that
                        // message (WSJT-X double-click semantics) AND moves our RX/TX
                        // onto the DX's frequency, not always from the grid at band-center.
                        let parsed = Msg::parse(&message);
                        if let Some(sender) = parsed.sender() {
                            eng.call_station_ctx(
                                sender,
                                None,
                                Some(&message),
                                Some(snr),
                                Some(delta_freq as f32),
                            );
                            // Stock parity: "double-click sets Tx enable" governs
                            // only OUR OWN UI clicks — an inbound UDP Reply
                            // (JTAlert/GridTracker) always arms TX in WSJT-X.
                            eng.set_tx_enabled(true);
                        }
                    }
                    // Companion mode: WSJT-X logged a QSO. It emits BOTH LoggedAdif
                    // (type 12, the full ADIF record) and QsoLogged (type 5, a
                    // structured summary) for the same contact — route ONLY the
                    // ADIF one through the dedup-safe import path, and ignore the
                    // structured summary, so the contact reaches the logbook /
                    // awards / Needed board exactly once (never double-logged).
                    WsjtxInbound::LoggedAdif { adif, .. } => {
                        eng.import_adif(&adif);
                    }
                    WsjtxInbound::QsoLogged { .. } => {} // handled via LoggedAdif above
                    _ => {}
                }
            }
        }

        // Immediate first over: a just-armed directed call (double-click) keys on
        // the CURRENT period if it's our TX parity AND the whole over still fits
        // before the next boundary — instead of waiting a full T/R cycle for the
        // next boundary (the "a few cycles go by" lag). If it doesn't fit / wrong
        // parity, the normal boundary path transmits at the next valid period.
        if self.tx_until_ms.is_none() && eng.peek_immediate_tx() {
            let slot_now = self.clock.slot_index(now);
            let on_our_parity = slot_now.is_multiple_of(2) == eng.tx_even();
            let room_ms = self.clock.ms_to_next_slot(now);
            // Fit on AUDIO length only — TX_TAIL is PTT hold after the audio ends
            // and may bleed into the next slot (it does at boundary starts too).
            // Counting it here inflated the deficit by up to 250 ms and trimmed
            // silence we didn't need to, starting the signal early (dt shift).
            let need_ms = eng.tx_over_secs() * 1000.0;
            // Late start, the WSJT-X way: the transmission stays TIME-ALIGNED to
            // the period grid — starting late just SKIPS the wave's leading
            // samples (the 0.5 s silence lead-in first, then leading symbols).
            // The remote decoder still syncs (dt ≈ 0, just fewer symbols), so
            // stock keys the CURRENT period rather than eating a full T/R cycle.
            //
            // Budget = how much leading audio a late over may drop and still decode.
            // FT8 carries three 7-symbol Costas sync arrays (start / middle ≈6.3 s in
            // / end); dropping only the head keeps the middle+end, so a click up to
            // ~7.9 s into the period still syncs. FT4's ~half-signal edge is ~3 s of
            // tones. The old shared 2 s cap deferred a click landing >~3.9 s in to the
            // NEXT same-parity boundary (a full cycle later) — the "clicked 1 s too
            // late, wait 30 s" complaint. Per-tier, Costas-preserving budgets mirror
            // WSJT-X keying a late over. (WSJT-X keys even later; we stop at the
            // decodable edge, which is the strictly safer product choice.)
            let allowed_deficit = match eng.tier() {
                tempo_app::dto::Tier::Ft8 => 6_000.0,
                tempo_app::dto::Tier::Ft4 => 3_000.0,
                _ => 0.0,
            };
            let deficit_ms = (need_ms - room_ms).max(0.0);
            if on_our_parity && deficit_ms <= allowed_deficit {
                // CONSUME the request only now that it actually fires — a click
                // outside the window used to be swallowed here and then wait an
                // EXTRA full cycle past the boundary it should have keyed at.
                let _ = eng.take_immediate_tx();
                let waves = eng.poll_tx(slot_now);
                if !waves.is_empty() {
                    let trim_samples =
                        ((deficit_ms / 1000.0) * tempo_fast::SAMPLE_RATE as f64) as usize;
                    // Must leave a transmittable remainder (always true within the
                    // per-tier budget — trimming ≤6 s of FT8's 12.6 s keeps ≥6.6 s).
                    let trimmable = waves
                        .first()
                        .map(|w| trim_samples < w.len())
                        .unwrap_or(false);
                    if trimmable {
                        // Split Operation: the engine reduced this over's audio —
                        // move the TX dial before the carrier keys (same as the
                        // boundary path).
                        let split = crate::slot::apply_tx_dial_shift(&mut eng, rig);
                        if split.fake_it_restore.is_some() {
                            self.fake_it_restore = split.fake_it_restore;
                        }
                        if split.rig_split_engaged {
                            self.audio_rig_split = true;
                        }
                        self.ensure_commanded(rig); // read-only launch: assert before key
                        self.publish_tx_intent_now(); // before keying
                        let _ = rig.ptt(true);
                        let mut secs = 0.0f32;
                        let last = waves.len() - 1;
                        for (i, w) in waves.iter().enumerate() {
                            let mut w2: &[f32] = if i == 0 && trim_samples > 0 {
                                &w[trim_samples..]
                            } else {
                                w
                            };
                            // The generated buffer can carry TRAILING silence
                            // (FT4: ~1.0 s of zero pad). On a LATE start the fit
                            // math is airtime-based — playing that pad would
                            // hold PTT past the boundary into the partner's
                            // period. Strip it; it carries nothing.
                            if i == last {
                                let end = w2.iter().rposition(|&x| x != 0.0).map_or(0, |p| p + 1);
                                w2 = &w2[..end];
                            }
                            secs += w2.len() as f32 / tempo_fast::SAMPLE_RATE;
                            backend.play(w2);
                        }
                        self.rx.clear(); // our just-started carrier must not be decoded
                        self.tx_until_ms =
                            Some(now + secs as f64 * 1000.0 + crate::slot::TX_TAIL_MS);
                        self.last_slot = Some(slot_now); // slot handled; skip the boundary
                        self.prev_slot_was_tx = true;
                    }
                }
            }
        }

        // Rebuild the slot clock + capture ring if the operator switched tier — or
        // changed the ACTIVE TIER'S PERIOD in Settings, which is the same event for
        // the clock's purposes and used not to be noticed at all.
        let tier_now = eng.tier();
        let slot_secs_now = eng.active_slot_secs();
        if tier_now != self.cur_tier || slot_secs_now != self.cur_slot_secs {
            self.cur_tier = tier_now;
            self.cur_slot_secs = slot_secs_now;
            self.clock = SlotClock::with_period_secs(slot_secs_now);
            self.rx = RxRing::with_capacity(eng.active_capture_samples());
            self.last_slot = None;
            self.prev_slot_was_tx = false;
            // Slot indices renumber with the new period — stale per-slot markers from
            // the old tier must not coincidentally match a new tier's slot.
            self.early_done_slot = None;
            self.boundary_keyed = None;
            // Including the index THIS tick already computed, above, from the clock we
            // just replaced. `last_slot = None` makes the boundary block below fire on
            // this very tick, so leaving it stale ran that boundary — and the TX
            // decision hanging off it — under the old period's numbering, at a moment
            // that is mid-period in the new one. Renumber before anyone reads it.
            slot = self.clock.slot_index(now);
        }

        // --- Decode-worker results: fold any completed decode, then act on it. The
        // heavy decode ran on the worker thread (off this thread + the engine mutex);
        // here we non-blockingly pick up finished results and run the DEFERRED back
        // half under the engine lock. A Boundary result runs the slot's TX decision
        // NOW that its decode is folded (preserving decode→TX ordering exactly); an
        // Early result just publishes spots; a Stale result (tier/source switch since
        // dispatch) is dropped. Draining BEFORE the new-boundary dispatch guarantees
        // an early result's `early_seen` is set before the same-slot boundary filters
        // against it. At most one decode is ever in flight (the in-flight guard).
        while let Some(result) = self.decode.try_recv() {
            self.decode_in_flight = false;
            match eng.apply_decode_result(result) {
                DecodeApplied::Boundary {
                    slot: bslot, frame, ..
                } => {
                    self.finish_boundary(
                        &mut eng,
                        rig,
                        backend,
                        sinks,
                        station,
                        now,
                        bslot,
                        true,
                        Some(frame),
                        // The worker has just finished, so the modem is free — no
                        // contention here, and no reason to release the engine.
                        None,
                    )?;
                }
                DecodeApplied::Early { n } => {
                    if n > 0 {
                        let cur_dial = eng.settings().dial_hz();
                        emit_rx_decodes(sinks, &eng, &mut station.psk_spots, now, cur_dial);
                    }
                }
                DecodeApplied::Stale => {}
            }
        }

        // --- WSJT-X-style early decode (FT8/FT4): a few seconds before the
        // boundary, decode the partial capture so callers appear while the
        // period is still running (stock decodes ~3×/period from ~11.8 s; our
        // single boundary pass made decodes land exactly as the operator's TX
        // window opened — zero decision time). RX slots only: our own carrier
        // (current TX or its boundary-crossing tail) must never reach the
        // decoder. The boundary pass below stays authoritative and ingests only
        // the stragglers this pass missed.
        if self.tx_until_ms.is_none()
            && !self.prev_slot_was_tx
            && self.early_done_slot != Some(slot)
            && !is_tuning
        {
            let early_at_ms = match tier_now {
                Tier::Ft8 => Some(11_800.0),
                Tier::Ft4 => Some(5_500.0),
                _ => None,
            };
            if let Some(at) = early_at_ms {
                let slot_ms = eng.active_slot_secs() * 1000.0;
                let elapsed_ms = slot_ms - self.clock.ms_to_next_slot(now);
                // `< slot_ms` guards the exact-boundary tick (ms_to_next_slot
                // returns 0 there, which would read as a FULL slot elapsed and
                // early-decode the PREVIOUS slot's audio under the wrong index).
                // Native FT8/FT4 only, and only when the worker is free — the early
                // result must fold in (setting `early_seen`) before the same-slot
                // boundary decode, so we never let two decodes race the one worker.
                if elapsed_ms >= at
                    && elapsed_ms < slot_ms
                    && !self.rx.is_empty()
                    && !self.decode_in_flight
                    && eng.source_kind() == SourceKind::Native
                {
                    self.early_done_slot = Some(slot);
                    // Only THIS slot's audio, at its true position from the slot
                    // start, tail-padded — a rolling tail of the previous slot
                    // (or front-padding) would wreck the decoder's dt alignment.
                    let n = ((elapsed_ms / 1000.0) * tempo_fast::SAMPLE_RATE as f64) as usize;
                    let frame = self.rx.frame_latest_padded(n);
                    // Dispatch the early partial decode (boundary-slot index = audio
                    // slot + 1, matching the boundary ingest's parity/history). The
                    // result folds in — and publishes its spots — via the drain block.
                    let job = eng.build_decode_job(frame, slot + 1, DecodePass::Early);
                    self.decode.dispatch(job);
                    self.decode_in_flight = true;
                }
            }
        }

        // New slot boundary: decode the just-ended RX slot (async) or, when there
        // is nothing to decode (own carrier / empty ring), run the TX decision now.
        // A boundary that needs a decode DEFERS its TX decision until the worker
        // result lands (drained above) — preserving decode->TX ordering while the
        // loop keeps ticking. If the worker is still busy (an early pass in flight),
        // retry next tick WITHOUT consuming the boundary, so no decode is ever lost.
        if Some(slot) != self.last_slot {
            let currently_tx = self.tx_until_ms.is_some();
            let prev_was_tx = self.prev_slot_was_tx;
            if crate::slot::slot_wants_decode(currently_tx, prev_was_tx, self.rx.is_empty()) {
                if !self.decode_in_flight {
                    self.last_slot = Some(slot);
                    // Capture the just-ended slot's audio BEFORE any keying — a TX
                    // start clears the ring (own-carrier guard) and the straggler
                    // decode needs the pure RX frame.
                    let frame = self.rx.frame();
                    // WSJT-X key-at-boundary (operator-approved 2026-07-21): when the
                    // just-ended RX slot's EARLY decode already folded (FT8/FT4 native —
                    // dispatched at 11.8 s / 5.5 s and drained above), the
                    // auto-sequencer's inputs are ready NOW. Run the TX decision AT the
                    // boundary — exactly WSJT-X's ordering (it keys at t=0 and decodes
                    // in parallel; stragglers can't change an in-flight over there
                    // either) — and let the boundary decode chase stragglers alongside.
                    // `finish_boundary`'s boundary_keyed guard turns that decode's
                    // drain into housekeeping-only, so the slot can never key twice.
                    // Without a folded early pass (FT1/DX1, companion sources, first
                    // slot, busy worker) the deferred decode→TX ordering below is
                    // UNCHANGED — this deliberately narrows the new behavior to the
                    // path that produced the ~1-2 s late TX.
                    //
                    // ⭐ AND FOR EVERY TIER THAT HAS NO EARLY PASS AT ALL.
                    // `early_done_slot` is set only by the early-pass block, whose
                    // trigger table is FT8 11.8 s / FT4 5.5 s / everything else None
                    // — so Q65, FST4, MSK144, WSPR and FST4W could NEVER satisfy the
                    // condition above and always fell through to the deferred path,
                    // keyed late by however long the previous period's decode took.
                    //
                    // On the air (operator report, 2026-07-28): MSK144 started ~8 s
                    // into a 15 s slot, so a 14.7 s over ran well past the boundary;
                    // FST4-60 never transmitted at all, because by the time its far
                    // slower decode landed there was no room left for a 52.8 s over.
                    // Q65-60A worked only because its decode happens to be quick
                    // enough — the same bug, under the threshold.
                    //
                    // WSJT-X keys at t=0 and decodes in PARALLEL; stragglers cannot
                    // change an over already in flight there either. This restores
                    // that ordering for the modes that had no way to reach it.
                    // FT8/FT4 keep their existing early-pass condition untouched.
                    let has_early_pass = matches!(
                        eng.tier(),
                        tempo_app::dto::Tier::Ft8 | tempo_app::dto::Tier::Ft4
                    );
                    if !has_early_pass || self.early_done_slot == Some(slot.wrapping_sub(1)) {
                        let _ = self
                            .key_boundary_tx(&mut eng, rig, backend, now, slot, false, None, None);
                    }
                    let job = eng.build_decode_job(frame, slot, DecodePass::Boundary);
                    self.decode.dispatch(job);
                    self.decode_in_flight = true;
                    // TX decision (when not already keyed above) deferred until this
                    // result is drained (next ticks).
                } else {
                    // ⭐ WORKER STILL BUSY: DROP THIS PERIOD'S DECODE — WSJT-X's own
                    // behaviour, `if(m_decoderBusy) return;` at mainwindow.cpp:5377
                    // ("Don't start decoder if it's already busy").
                    //
                    // This used to leave `last_slot` unset and retry on later ticks,
                    // which sounds harmless and is not: `rx.frame()` would then be
                    // captured at the RETRY tick, and RxRing keeps the newest `cap`
                    // samples. The frame was therefore a rolling window straddling the
                    // old slot's tail and the new slot's head — every dt in it shifted
                    // by the retry delay, the whole thing attributed to the wrong slot
                    // index, and the new slot decoded a second time when it truly
                    // ended. A dropped period is a period of missed decodes; a
                    // misaligned one is wrong data presented as fact.
                    //
                    // Reachable on FT8 today, not only on the long modes: the early
                    // pass dispatches at 11.8 s of a 15 s slot, so an early decode
                    // running over ~3.2 s lands here. The Pi builds are the concern.
                    //
                    // The TX decision still runs — WSJT-X keys at t=0 regardless of
                    // whether a decode completed, and a busy decoder must not cost the
                    // operator an over. The ring is deliberately NOT cleared: it holds
                    // the newest `cap` samples, so by the next boundary it contains
                    // exactly that slot.
                    self.last_slot = Some(slot);
                    self.dropped_decodes = self.dropped_decodes.saturating_add(1);
                    eprintln!(
                        "[decode] worker still busy at the slot {slot} boundary — period \
                         dropped (total {}). The decoder is not keeping up with the T/R \
                         period on this hardware.",
                        self.dropped_decodes
                    );
                    // ⭐ BUILD THIS OVER WITH THE ENGINE MUTEX RELEASED.
                    //
                    // This is the ONE branch where the modem is contended: we are
                    // here because a decode from an earlier slot is STILL RUNNING,
                    // and that decode holds `MODEM_LOCK` for its whole duration.
                    // The TX build needs the same lock, so it waits — and it used
                    // to wait while holding the engine mutex, which every Tauri
                    // snapshot poll and every UI command also needs. The window
                    // went "not responding" for as long as the decoder took
                    // (sub-second here, seconds on a Pi).
                    //
                    // Plan under the lock, release, build, re-acquire, commit. On
                    // the air nothing changes: the same wait happens at the same
                    // point, and `commit_tx` refuses the plan if the tier moved
                    // underneath us while the engine was unlocked.
                    //
                    // Skipped entirely when this slot was already keyed at its
                    // boundary — `finish_boundary` is then housekeeping-only and
                    // never runs the TX decision, so planning would advance the
                    // sequencer and write an ALL.TXT Tx line for an over that is
                    // not sent.
                    let already_keyed =
                        self.boundary_keyed.map(|k| k.slot == slot).unwrap_or(false);
                    let (prebuilt, now_tx) = if already_keyed {
                        (None, now)
                    } else {
                        match eng.plan_tx(slot) {
                            Some(plan) => {
                                drop(eng);
                                let wave = plan.waveform.build();
                                eng = engine_lock(engine);
                                // Re-read the clock AFTER the build (it waited out
                                // MODEM_LOCK): commit_tx refuses if the T/R slot
                                // rolled over meanwhile, and the PTT-hold deadline
                                // must be measured from when the audio actually
                                // starts — the boundary tick's `now` would leave
                                // the tail short by the whole build time.
                                let now = now_unix_ms() - self.clock_offset_ms as f64;
                                let waves = eng.commit_tx(&plan, wave, self.clock.slot_index(now));
                                (Some(waves), now)
                            }
                            // Planned to nothing: hand the empty result straight
                            // through so the TX phase is not re-run under the lock.
                            None => (Some(Vec::new()), now),
                        }
                    };
                    self.finish_boundary(
                        &mut eng, rig, backend, sinks, station, now_tx, slot, false, None, prebuilt,
                    )?;
                }
            } else {
                self.last_slot = Some(slot);
                // Own carrier: the ring holds our own transmission -> drop it so a
                // fragment can't contaminate the next decode.
                if currently_tx || prev_was_tx {
                    self.rx.clear();
                }
                // Nothing to decode -> run the TX decision + emission immediately.
                self.finish_boundary(
                    &mut eng, rig, backend, sinks, station, now, slot, false, None, None,
                )?;
            }
        }
        drop(eng); // release before the PSK flush re-locks the engine

        // PSK Reporter: flush accumulated spots periodically (outside the lock).
        if let Some(reporter) = sinks.psk {
            if !station.psk_spots.is_empty()
                && now - station.last_psk_flush >= PSK_FLUSH_SECS * 1000.0
            {
                let (rx_call, rx_grid) = {
                    let eng = engine_lock(engine);
                    let s = eng.snapshot();
                    (s.mycall.clone(), s.mygrid.clone())
                };
                let _ = reporter.send_spots(&rx_call, &rx_grid, "Tempo", &station.psk_spots);
                station.psk_spots.clear();
                station.last_psk_flush = now;
            }
        }

        Ok(())
    }

    /// Finish a slot boundary once its RX decode is folded in: run the deferred
    /// TX decision (`slot_tx_phase`) and then the WSJT-X/PSK/club-network emission
    /// for the period. `did_rx`/`rx_frame` describe the just-folded decode (both
    /// false/None when the boundary had nothing to decode — own carrier / empty
    /// ring). Shared by the no-decode boundary path and the worker-result drain.
    #[allow(clippy::too_many_arguments)]
    fn finish_boundary<B: AudioBackend>(
        &mut self,
        eng: &mut Engine,
        rig: &mut Rig,
        backend: &mut B,
        sinks: &Sinks,
        station: &mut StationSinks,
        now: f64,
        slot: u64,
        did_rx: bool,
        rx_frame: Option<Vec<f32>>,
        // See `slot_tx_phase`: a waveform built with the engine mutex RELEASED.
        prebuilt: Option<Vec<Vec<f32>>>,
    ) -> Result<(), String> {
        // Key-at-boundary (the WSJT-X ordering, operator-approved 2026-07-21): when
        // this slot's TX decision already ran AT the boundary, this call is the
        // straggler decode's housekeeping only — keying again would double-transmit
        // the slot.
        if let Some(k) = self.boundary_keyed {
            if k.slot == slot {
                return self.emit_boundary_housekeeping(
                    eng,
                    sinks,
                    station,
                    now,
                    k.dial_hz,
                    did_rx,
                    k.tx_this_slot,
                    rx_frame,
                );
            }
        }
        // Deferred path (unchanged behavior): TX decision with the just-ended slot's
        // decode ALREADY folded (inline when there was nothing to decode, or via the
        // worker result otherwise), then the housekeeping back-to-back.
        let cur_dial = eng.settings().dial_hz();
        let action = self.key_boundary_tx(eng, rig, backend, now, slot, did_rx, rx_frame, prebuilt);
        let did_rx = action.did_rx;
        let tx_this_slot = action.tx_this_slot;
        self.emit_boundary_housekeeping(
            eng,
            sinks,
            station,
            now,
            cur_dial,
            did_rx,
            tx_this_slot,
            action.rx_frame,
        )
    }

    /// The TX half of a slot boundary: run the auto-sequencer's transmit decision and
    /// key NOW. Everything transmit-critical lives here — and nothing else — so the
    /// key-at-boundary path can run it at t=0 while the straggler decode chases in
    /// parallel. Records `boundary_keyed` so `finish_boundary` never keys the same
    /// slot twice.
    #[allow(clippy::too_many_arguments)] // mirrors slot_tx_phase's boundary parameter set
    #[allow(clippy::too_many_arguments)]
    fn key_boundary_tx<B: AudioBackend>(
        &mut self,
        eng: &mut Engine,
        rig: &mut Rig,
        backend: &mut B,
        now: f64,
        slot: u64,
        did_rx: bool,
        rx_frame: Option<Vec<f32>>,
        // See `slot_tx_phase`: a waveform built with the engine mutex RELEASED.
        prebuilt: Option<Vec<Vec<f32>>>,
    ) -> crate::slot::SlotAction {
        // Dial BEFORE keying: Split Operation may shift the TX dial inside
        // slot_tx_phase, and the deferred status emission reports the pre-shift dial.
        let dial_hz = eng.settings().dial_hz();
        // Read-only launch: the slot sequencer's key-up must also assert first (slot_tx_phase
        // keys inside slot.rs, so the latch runs here). Gated on TX being ARMED: this fn runs
        // on EVERY boundary including pure-RX monitoring, and an unarmed boundary must stay
        // read-only — asserting only when armed keeps "command on key-up" true while never
        // commanding a rig the operator is merely listening to.
        if eng.tx_enabled() {
            self.ensure_commanded(rig);
        }
        let action = crate::slot::slot_tx_phase(
            eng,
            rig,
            backend,
            &mut self.rx,
            slot,
            now,
            did_rx,
            rx_frame,
            prebuilt,
        );
        if let Some(t) = action.tx_until_ms {
            self.tx_until_ms = Some(t);
            // The slot core just keyed (slot.rs) — publish TX intent immediately rather
            // than waiting for the next tick's scope-gate publish (~20 ms), so the broker's
            // disconnect fail-safe can't race the fresh key-up.
            self.publish_tx_intent_now();
        }
        if action.fake_it_restore.is_some() {
            self.fake_it_restore = action.fake_it_restore;
        }
        if action.rig_split_engaged {
            self.audio_rig_split = true;
        }
        // Remember whether THIS slot was a transmit slot so the next boundary
        // knows not to decode our own carrier (and to decode it otherwise).
        self.prev_slot_was_tx = action.tx_this_slot;
        // The boundary owns the slot now — drain any still-pending immediate-TX
        // request (it either just fired via the slot core's parity path, or its
        // moment passed; leaving it set would key mid-slot LATER, off-cycle).
        let _ = eng.take_immediate_tx();
        self.boundary_keyed = Some(KeyedBoundary {
            slot,
            tx_this_slot: action.tx_this_slot,
            dial_hz,
        });
        action
    }

    /// The non-transmit half of a slot boundary: period-WAV save, WSJT-X/PSK network
    /// emission, and the Field Day club push. Runs back-to-back with the TX half on
    /// the deferred path, or at the straggler decode's drain on the key-at-boundary
    /// path. Touches no TX state and never keys.
    #[allow(clippy::too_many_arguments)]
    fn emit_boundary_housekeeping(
        &mut self,
        eng: &mut Engine,
        sinks: &Sinks,
        station: &mut StationSinks,
        now: f64,
        cur_dial: u64,
        did_rx: bool,
        tx_this_slot: bool,
        rx_frame: Option<Vec<f32>>,
    ) -> Result<(), String> {
        // Save the received period as a WAV when asked (WSJT-X's Save menu:
        // "all" = every RX period, "decodes" = only periods that produced
        // one). Best-effort — a full disk must never stall the radio loop.
        if let Some(frame) = &rx_frame {
            let mode = eng.settings().save_wav.clone();
            let want = match mode.as_str() {
                "all" => true,
                // The WHOLE period's decode set (early pass + boundary
                // stragglers) — wire_decodes() alone is only the boundary
                // batch, which is empty when the early pass caught
                // everything (review catch: that skipped exactly the
                // cleanest, strongest-signal periods).
                "decodes" => !eng.current_period_decodes().is_empty(),
                _ => false,
            };
            if want {
                if let Some(dir) = eng.periods_dir() {
                    let secs = (now / 1000.0) as i64;
                    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
                    let (h, m, sec) = (
                        secs.rem_euclid(86_400) / 3600,
                        secs.rem_euclid(3600) / 60,
                        secs.rem_euclid(60),
                    );
                    // WSJT-X-style stamp + the band for at-a-glance sorting.
                    // Sanitize band first: settings.band is a free-form string
                    // from settings.json, and a value containing a path
                    // separator or ".." would make `join` escape periods_dir.
                    let band: String = eng
                        .settings()
                        .band
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .collect();
                    let name = format!("{y:04}{mo:02}{d:02}_{h:02}{m:02}{sec:02}_{band}.wav");
                    let path = std::path::Path::new(&dir).join(name);
                    if let Err(e) = crate::voice::write_wav_12k(&path, frame) {
                        eng.set_audio_error(Some(format!("period WAV save failed: {e}")));
                    }
                }
            }
        }
        // Snapshot once for BOTH the WSJT-X/PSK emission and the club-network
        // Field Day push below. The club push has to run on every slot boundary
        // an FD session is live — whether or not the WSJT-X/PSK sinks are on —
        // so `field_day.is_some()` joins the gate. It used to be trapped INSIDE
        // that gate, silently starving N3FJP/N1MM whenever both sinks were their
        // default-off (the club master log simply never received the QSOs).
        let snap = eng.snapshot();
        // An FD session just (re)started: the journal restore repopulates
        // qso_count from 0 in one jump — seed the cursor so restored rows are
        // never re-pushed to the club network / WSJT-X sinks as newly logged.
        if !station.fd_was_active {
            if let Some(fd) = snap.field_day.as_ref() {
                station.last_fd_qsos = fd.qso_count;
            }
        }
        station.fd_was_active = snap.field_day.is_some();
        // --- network emission (WSJT-X UDP API + PSK Reporter) ---
        if sinks.wsjtx.is_some() || sinks.psk.is_some() || snap.field_day.is_some() {
            let tier = tier_mode(snap.link.tier);
            let _ms_mid = (now as u64 % 86_400_000) as u32;
            let now_secs = (now / 1000.0) as i64;
            if did_rx {
                emit_rx_decodes(sinks, &*eng, &mut station.psk_spots, now, cur_dial);
            }
            if let Some(server) = sinks.wsjtx {
                let dx = snap
                    .qso
                    .as_ref()
                    .and_then(|q| q.dxcall.clone())
                    .unwrap_or_default();
                let _ = server.send_status(&WsjtxStatus {
                    dial_freq: cur_dial,
                    mode: tier,
                    dx_call: &dx,
                    report: "",
                    tx_mode: tier,
                    tx_enabled: false,
                    transmitting: snap.radio.transmitting,
                    // `decoding` and `transmitting` are disjoint phases in
                    // WSJT-X: when we decode the prior RX slot AND transmit in
                    // this one (calling CQ), report the transmit phase only.
                    decoding: did_rx && !tx_this_slot,
                    // REAL audio offsets (GridTracker/JTAlert show these) —
                    // hardcoded 1500s confused every cooperating logger.
                    rx_df: snap.radio.rx_offset_hz.max(0.0) as u32,
                    tx_df: snap.radio.tx_offset_hz.max(0.0) as u32,
                    de_call: &snap.mycall,
                    de_grid: &snap.mygrid,
                    dx_grid: "",
                    tx_watchdog: false,
                    sub_mode: "",
                    fast_mode: false,
                    // The LIVE mode wins: field_day is Some only while the
                    // Field Day mode is actually RUNNING, whereas special_op
                    // is a persistent setting an operator can forget to turn
                    // off — a stale Hound flag must not misadvertise an
                    // active FD session (review catch). 6=FOX stays unbuilt.
                    special_op: if snap.field_day.is_some() {
                        3
                    } else if matches!(
                        eng.settings().special_op,
                        tempo_app::settings::SpecialOp::Hound
                            | tempo_app::settings::SpecialOp::SuperHound
                    ) {
                        7
                    } else {
                        0
                    },
                    freq_tol: 0,
                    // T/R period (s), mode-driven: FT1 = 4, FT4 ≈ 8, FT8/DX1 = 15.
                    tr_period: eng.active_slot_secs().round() as u32,
                    config_name: "Default",
                    tx_message: "",
                });
                if let Some(fd) = snap.field_day.as_ref() {
                    if fd.qso_count > station.last_fd_qsos {
                        let sent = format!("{} {}", fd.my_class, fd.my_section);
                        for q in &fd.log[station.last_fd_qsos.min(fd.log.len())..] {
                            let recvd = format!("{} {}", q.class, q.section);
                            let _ = server.send_qso_logged(&WsjtxQso {
                                time_off: now_secs,
                                dx_call: &q.call,
                                dx_grid: "",
                                tx_freq: sinks.cfg_dial_hz,
                                mode: tier,
                                report_sent: "",
                                report_recvd: "",
                                tx_power: "",
                                comments: "",
                                name: "",
                                time_on: now_secs,
                                op_call: &snap.mycall,
                                my_call: &snap.mycall,
                                my_grid: &snap.mygrid,
                                exchange_sent: &sent,
                                exchange_recvd: &recvd,
                                adif_propmode: "",
                            });
                        }
                    }
                }
            }
            // Club-network push (independent of the WSJT-X sink): every NEW
            // Field Day QSO goes to N3FJP (the club master log, TCP) and/or
            // an N1MM-network dashboard (UDP <contactinfo>) when configured.
            // Spawned: a parked N3FJP box must never stall the slot loop.
            if let Some(fd) = snap.field_day.as_ref() {
                if fd.qso_count > station.last_fd_qsos {
                    let st = eng.settings();
                    let n3_host = st.n3fjp_host.trim().to_string();
                    let n3_port = st.n3fjp_port;
                    // Field Day contacts use the ENTER sequence (which scores
                    // the contest log) unless the operator opts back to ADDDIRECT.
                    let n3_use_enter = st.n3fjp_use_enter;
                    let n1_addr = st.n1mm_addr.trim().to_string();
                    if !n3_host.is_empty() || !n1_addr.is_empty() {
                        let new_qsos: Vec<_> =
                            fd.log[station.last_fd_qsos.min(fd.log.len())..].to_vec();
                        let mycall = snap.mycall.clone();
                        // The operator at the key (FD rotates ops) — the settable
                        // fd_operator when set, else the station call.
                        let operator = {
                            let op = st.fd_operator.trim();
                            if op.is_empty() {
                                mycall.clone()
                            } else {
                                op.to_string()
                            }
                        };
                        let myexch = format!("{} {}", fd.my_class, fd.my_section);
                        let contest = if fd.event == "wfd" {
                            "WFD"
                        } else {
                            "ARRL-FIELD-DAY"
                        };
                        let dial_mhz = cur_dial as f64 / 1e6;
                        let fallback_unix = (now / 1000.0) as u64;
                        std::thread::spawn(move || {
                            for (i, q) in new_qsos.iter().enumerate() {
                                let mode_str = match q.mode.as_str() {
                                    "CW" => "CW",
                                    "PH" => "SSB",
                                    _ => "FT8",
                                };
                                // Per-QSO log time (a multi-contact batch must not
                                // collapse onto one wall-clock second).
                                let when = if q.when_unix > 0 {
                                    q.when_unix
                                } else {
                                    fallback_unix
                                };
                                if !n3_host.is_empty() {
                                    let push = tempo_net::n3fjp::N3fjpQso {
                                        call: q.call.clone(),
                                        class: q.class.clone(),
                                        section: q.section.clone(),
                                        band_meters: band_for_interop(&q.band),
                                        mode: mode_str.to_string(),
                                        freq_mhz: dial_mhz,
                                        when_unix: when,
                                        operator: operator.clone(),
                                    };
                                    let res = if n3_use_enter {
                                        tempo_net::n3fjp::push_qso_enter(&n3_host, n3_port, &push)
                                            .map(|_| ())
                                    } else {
                                        tempo_net::n3fjp::push_qso(&n3_host, n3_port, &push)
                                    };
                                    if let Err(e) = res {
                                        eprintln!("tempo: N3FJP push failed: {e}");
                                    }
                                }
                                if !n1_addr.is_empty() {
                                    let c = tempo_net::n1mm::N1mmContact {
                                        mycall: mycall.clone(),
                                        call: q.call.clone(),
                                        band: band_for_interop(&q.band),
                                        mode: mode_str.to_string(),
                                        timestamp: tempo_net::n1mm::utc_timestamp(when),
                                        section: q.section.clone(),
                                        // A contest exchange carries no grid.
                                        gridsquare: String::new(),
                                        points: tempo_core::fieldday::qso_points_for_mode(&q.mode),
                                        contestname: contest.to_string(),
                                        freq_10hz: (dial_mhz * 1e5) as u64,
                                        sent_exchange: myexch.clone(),
                                        operator: operator.clone(),
                                        // 32-hex dedup id: time + batch index + call hash.
                                        id: tempo_net::n1mm::dedup_id(when, &q.call, i as u64),
                                    };
                                    if let Err(e) = tempo_net::n1mm::send_contact(&n1_addr, &c) {
                                        eprintln!("tempo: N1MM broadcast failed: {e}");
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
        // Advance the FD cursor on EVERY boundary (independent of the sinks
        // above) — so it also RESETS to 0 when a session ends, and a stale
        // count can never later flood the club log after FD is re-armed.
        station.last_fd_qsos = snap.field_day.as_ref().map(|f| f.qso_count).unwrap_or(0);

        // Club band board (N3FJP Network Status Display): report THIS
        // position's band without CAT so the club sees where we are. Fires
        // on a band/mode change or a coarse heartbeat; spawned so a parked
        // N3FJP box never stalls the slot loop. Opt-in (default off).
        if eng.settings().n3fjp_report_band {
            let host = eng.settings().n3fjp_host.trim().to_string();
            if !host.is_empty() {
                let band_meters = band_for_interop(&snap.radio.band);
                let mode = snap.radio.sideband.clone();
                let bm_key = format!("{band_meters}|{mode}");
                if bm_key != station.last_reported_bm
                    || now - station.last_reported_band >= N3FJP_BAND_REPORT_MS
                {
                    station.last_reported_band = now;
                    station.last_reported_bm = bm_key;
                    let port = eng.settings().n3fjp_port;
                    let freq_mhz = snap.radio.dial_mhz;
                    std::thread::spawn(move || {
                        // Nexus owns the rig, so N3FJP's own rig interface is
                        // off → CHANGEBM (rig_iface_on = false), the no-CAT
                        // local-bridge default.
                        if let Err(e) = tempo_net::n3fjp::report_band(
                            &host,
                            port,
                            &band_meters,
                            &mode,
                            freq_mhz,
                            false,
                        ) {
                            eprintln!("tempo: N3FJP band report failed: {e}");
                        }
                    });
                }
            }
        }
        Ok(())
    }
}

// ---- network-emission builders (pure; unit-tested) -----------------------
//
// Extracted from the loop so the WSJT-X / PSK Reporter emission content is
// provable without a sound card, rig, or live socket. The loop calls these and
// sends the result; the math (audio-offset → RF frequency) and the
// callsign-gating live here where they can be tested.

/// The WSJT-X mode string for a link [`Tier`].
fn tier_mode(tier: Tier) -> &'static str {
    match tier {
        Tier::TempoFast => "TempoFast",
        Tier::TempoDeep => "TempoDeep",
        Tier::Ft8 => "FT8",
        Tier::Ft4 => "FT4",
        // These feed the WSJT-X UDP Decode message and the PSK Reporter spot
        // queue, so they must be the names cooperating loggers and the reporter
        // expect — "Q65" without the submode, as in ADIF, not the "Q65-30A" the
        // tier displays.
        Tier::Fst4 => "FST4",
        Tier::Fst4w => "FST4W",
        Tier::Q65 => "Q65",
        Tier::Msk144 => "MSK144",
        Tier::Jt65 => "JT65",
        Tier::Wspr => "WSPR",
    }
}

/// Build the WSJT-X **Decode (type 2)** message for one decoded signal.
/// Borrows `message`/`mode` for the lifetime of the returned struct.
/// Forward the engine's `last_decodes` (the rows the ingest that just ran
/// produced — boundary OR early pass) to the WSJT-X UDP server and the PSK
/// Reporter spot queue. Shared so early decodes reach cooperating loggers and
/// PSKR at the same moment they reach our own UI.
/// Hinnant's civil-from-days (UTC): days since the epoch → (year, month, day).
/// For the period-WAV filename stamp only.
fn civil_from_days(z0: i64) -> (i64, u32, u32) {
    let z = z0 + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn emit_rx_decodes(
    sinks: &Sinks,
    eng: &Engine,
    psk_spots: &mut Vec<Spot>,
    now: f64,
    cur_dial: u64,
) {
    if sinks.wsjtx.is_none() && sinks.psk.is_none() {
        return;
    }
    let tier = tier_mode(eng.tier());
    let ms_mid = (now as u64 % 86_400_000) as u32;
    let now_secs = (now / 1000.0) as u32;
    // ON-AIR text only — never the hound-rewritten internal form.
    for d in eng.wire_decodes() {
        if let Some(server) = sinks.wsjtx {
            let _ = server.send_decode(&build_decode(
                &d.message,
                d.snr,
                d.dt,
                d.freq,
                tier,
                ms_mid,
                d.qual < 0.17, // the stock low-confidence line
            ));
        }
        if sinks.psk.is_some() {
            if let Some(spot) = build_spot(&d.message, d.snr, d.freq, tier, cur_dial, now_secs) {
                psk_spots.push(spot);
            }
        }
    }
}

fn build_decode<'a>(
    message: &'a str,
    snr: i32,
    dt: f32,
    freq: f32,
    mode: &'a str,
    time_ms: u32,
    low_confidence: bool,
) -> WsjtxDecode<'a> {
    WsjtxDecode {
        new: true,
        time_ms,
        snr,
        delta_time: dt as f64,
        delta_freq: freq as u32,
        mode,
        message,
        low_confidence,
        off_air: false,
    }
}

/// Build a PSK Reporter [`Spot`] from a decode, or `None` if no sender callsign
/// can be parsed (only stations we actually copied get reported). The spot
/// frequency is the dial frequency plus the decode's audio offset.
fn build_spot(
    message: &str,
    snr: i32,
    freq: f32,
    mode: &str,
    cur_dial: u64,
    now_secs: u32,
) -> Option<Spot> {
    Msg::parse(message).sender().map(|call| Spot {
        call: call.to_string(),
        freq_hz: cur_dial + freq as u64,
        snr,
        mode: mode.to_string(),
        time_secs: now_secs,
    })
}

/// Generate `n` samples of a unit-amplitude sine at `freq` Hz, continuing from
/// `phase` (radians, advanced in place) so successive chunks join seamlessly.
/// Tx-level scaling is applied later by the backend's `play`.
fn tune_carrier(freq: f32, n: usize, sample_rate: f32, phase: &mut f32) -> Vec<f32> {
    use std::f32::consts::TAU;
    let step = TAU * freq / sample_rate;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(phase.sin());
        *phase += step;
        if *phase >= TAU {
            *phase -= TAU;
        }
    }
    out
}

/// Periodically probe an NTP server to estimate the PC-clock-vs-UTC offset and
/// publish it to the engine (for the UI clock chip). Runs on its own thread so a
/// slow or failed query never stalls the audio loop; honors the `clock_check`
/// setting and fails silently when off-grid (publishes `None`, so the UI falls
/// back to the DT-derived sync health).
fn clock_probe_loop(engine: Arc<Mutex<Engine>>) {
    const SERVERS: [&str; 3] = [
        "pool.ntp.org:123",
        "time.nist.gov:123",
        "time.google.com:123",
    ];
    loop {
        let enabled = engine_lock(&engine).settings().clock_check;
        let offset = if enabled {
            tempo_net::sntp::query_any(&SERVERS, Duration::from_secs(3)).ok()
        } else {
            None
        };
        {
            let mut e = engine_lock(&engine);
            e.set_clock_offset_ms(offset);
        }
        std::thread::sleep(Duration::from_secs(600)); // ~10 min
    }
}

/// The transport-affecting subset of the operator's settings: which rig/PTT and
/// audio devices the radio loop is driving. The loop compares the live value
/// (from the engine's settings) against the one it has `applied` and rebuilds
/// the rig / re-opens the sound card when these change — so a Settings "Save"
/// reconnects CAT without an app restart.
#[derive(Clone, PartialEq)]
struct Transport {
    ptt_method: String,
    rig_model: u32,
    serial_port: String,
    /// Serial port for RTS/DTR PTT when it differs from the CAT port (SO2R controller
    /// routing keying on its own COM port). Empty = key on `serial_port` (prior behavior).
    ptt_serial_port: String,
    baud: u32,
    /// "network" → rigctld talks to `rig_addr` over TCP (Flex/SmartSDR); else serial.
    rig_conn: String,
    /// host:port for a network rig (when `rig_conn == "network"`).
    rig_addr: String,
    rigctld_port: u16,
    /// Native Icom CI-V opt-in for this radio (see `RadioProfile::icom_native_cat`) —
    /// selects Nexus's own CI-V daemon instead of rigctld at the spawn sites.
    icom_native_cat: bool,
    /// The port our OWN CAT broker is serving on (if enabled), so auto-coexist never
    /// connects Nexus to itself. `None` = broker off.
    broker_self_port: Option<u16>,
    audio_in: String,
    audio_out: String,
    /// Dedicated voice-mic device for recordings ("" = record from the shared input).
    /// Carried here so the recording block reads the live value; changing it never
    /// rebuilds the capture/TX streams (it only affects the transient mic stream).
    voice_mic_device: String,
    tx_level: f32,
    rx_gain: f32,
    /// Dark headphone-monitor settings (off by default). Carried here so a change is
    /// applied to the running backend IN PLACE — never as a capture-stream rebuild.
    monitor_enabled: bool,
    monitor_device: String,
    monitor_level: f32,
}

impl Transport {
    fn from_cfg(c: &RadioConfig) -> Self {
        Self {
            ptt_method: c.ptt_method.clone(),
            rig_model: c.rig_model,
            serial_port: c.serial_port.clone(),
            // Not part of the per-radio startup seed (it's a GLOBAL keying-line setting):
            // the live per-tick `from_settings` rebuild supplies the real value, and empty
            // here just falls back to `serial_port` for the brief pre-first-tick window.
            ptt_serial_port: String::new(),
            baud: c.baud,
            rig_conn: c.rig_conn.clone(),
            rig_addr: c.rig_addr.clone(),
            rigctld_port: safe_rigctld_port(c.rigctld_port),
            icom_native_cat: c.icom_native_cat,
            broker_self_port: c.broker_self_port,
            audio_in: c.audio_in.clone(),
            audio_out: c.audio_out.clone(),
            // The voice mic is not part of the startup seed — the initial applied state
            // is "none", so the first recording reads it from the live engine settings.
            voice_mic_device: String::new(),
            tx_level: c.tx_level,
            rx_gain: c.rx_gain,
            // The monitor is not part of the startup seed — the initial applied state
            // is "off", so the first loop turns it on from the live engine settings.
            monitor_enabled: false,
            monitor_device: String::new(),
            monitor_level: 0.5,
        }
    }

    fn from_settings(s: &Settings) -> Self {
        Self {
            ptt_method: s.ptt_method.clone(),
            rig_model: s.rig_model,
            serial_port: s.serial_port.clone(),
            ptt_serial_port: s.ptt_serial_port.clone(),
            baud: s.baud,
            icom_native_cat: s.icom_native_cat,
            rig_conn: s.rig_conn.clone(),
            rig_addr: s.rig_addr.clone(),
            rigctld_port: safe_rigctld_port(s.rigctld_port),
            broker_self_port: if s.cat_broker {
                Some(s.cat_broker_port)
            } else {
                None
            },
            audio_in: s.audio_in.clone(),
            audio_out: s.audio_out.clone(),
            voice_mic_device: s.voice_mic_device.clone(),
            tx_level: s.tx_level,
            rx_gain: s.rx_gain,
            monitor_enabled: s.monitor_enabled,
            monitor_device: s.monitor_device.clone(),
            monitor_level: s.monitor_level,
        }
    }

    /// The serial port the RTS/DTR keying line lives on: the dedicated `ptt_serial_port`
    /// when set (an SO2R controller's own COM port), else the CAT `serial_port` (the prior
    /// single-port behavior). Only meaningful when `ptt_method` is "rts"/"dtr".
    fn ptt_port(&self) -> &str {
        if self.ptt_serial_port.trim().is_empty() {
            &self.serial_port
        } else {
            self.ptt_serial_port.trim()
        }
    }

    /// True if a field that requires (re)launching rigctld / rebuilding the Rig
    /// changed (PTT method, rig model, serial port, baud, rigctld TCP port).
    fn rig_differs(&self, o: &Transport) -> bool {
        self.ptt_method != o.ptt_method
            || self.rig_model != o.rig_model
            || self.serial_port != o.serial_port
            || self.ptt_serial_port != o.ptt_serial_port
            || self.baud != o.baud
            || self.rig_conn != o.rig_conn
            || self.rig_addr != o.rig_addr
            || self.rigctld_port != o.rigctld_port
            || self.icom_native_cat != o.icom_native_cat
            || self.broker_self_port != o.broker_self_port
    }

    /// A networked rig (FlexRadio/SmartSDR or a remote rigctld): rigctld connects to
    /// `rig_addr` over TCP instead of a serial port. Requires a non-empty address.
    fn is_network(&self) -> bool {
        self.rig_conn == "network" && !self.rig_addr.is_empty()
    }

    /// True if the selected sound-card input/output device changed.
    fn audio_differs(&self, o: &Transport) -> bool {
        self.audio_in != o.audio_in || self.audio_out != o.audio_out
    }

    /// True if a headphone-monitor setting changed (enable, device, or level). Drives
    /// an in-place monitor reconfigure — NOT a capture-stream rebuild.
    fn monitor_differs(&self, o: &Transport) -> bool {
        self.monitor_enabled != o.monitor_enabled
            || self.monitor_device != o.monitor_device
            || (self.monitor_level - o.monitor_level).abs() > f32::EPSILON
    }
}

/// The passband (Hz) to command alongside a rig mode. FT8/FT4 (the DATA submodes) need the
/// FULL ~3 kHz audio passband — decodes span the whole band, and a narrow recalled DATA filter
/// (e.g. 600 Hz on the FTDX10) clips signals — so we force 3000 Hz there.
/// For SSB / CW / FM we pass `-1` (`RIG_PASSBAND_NOCHANGE`) so the rig keeps EXACTLY its current
/// filter — the operator's chosen CW width / SSB filter is left untouched. (Passband `0` is
/// Hamlib's `RIG_PASSBAND_NORMAL`, which actively commands the rig's *default* width and pops the
/// rig's Width display on every mode change — the bug this avoids.)
/// Is `md` a DATA/PKT mode (PKTUSB/PKTLSB, DATA-U/DATA-L)? The Icom tune path skips its
/// temporary DATA-mode flip for these — an FT8 operator is already in DATA-U and must stay
/// there through tune (else the release turns DATA off and strands the rig in plain USB).
fn mode_is_data(md: &str) -> bool {
    let m = md.trim().to_ascii_uppercase();
    m.starts_with("PKT") || m.starts_with("DATA")
}

fn passband_for(md: &str) -> i32 {
    match md.trim().to_ascii_uppercase().as_str() {
        "PKTUSB" | "PKTLSB" => 3000,
        _ => -1,
    }
}

/// The passband for attempt `prior_fails + 1` of the bounded mode-set retry — the middle
/// rung of the resilience ladder. DATA modes start with the full 3 kHz passband
/// ([`passband_for`]); once a run keeps failing past [`MODE_SET_PASSBAND0_AFTER`], later
/// attempts send passband `0` (`M PKTUSB 0` — Hamlib's `RIG_PASSBAND_NORMAL`, the rig's
/// own default width) so a backend that rejects the width→DATA-filter mapping, not the
/// mode itself, still gets the mode set. Non-DATA modes keep `-1` (NOCHANGE) always —
/// `0` would actively re-command the default width and pop the rig's Width display.
fn retry_passband(md: &str, prior_fails: u32) -> i32 {
    let pb = passband_for(md);
    if pb > 0 && prior_fails >= MODE_SET_PASSBAND0_AFTER {
        0
    } else {
        pb
    }
}

/// The plain sideband underneath a DATA/PKT submode — the LAST rung of the mode-set
/// ladder. A rig whose CAT refuses the DATA submode (or a Hamlib backend that garbles
/// it) still takes plain USB/LSB; landing there leaves the operator one rig-front-panel
/// DATA press from working, instead of stranded on whatever mode was active before.
/// `None` for non-DATA modes — there is nothing sensible to fall back to.
fn fallback_sideband(md: &str) -> Option<&'static str> {
    match md.trim().to_ascii_uppercase().as_str() {
        "PKTUSB" | "DATA-U" | "PKT-U" => Some("USB"),
        "PKTLSB" | "DATA-L" | "PKT-L" => Some("LSB"),
        _ => None,
    }
}

/// The give-up note after [`MODE_SET_MAX_TRIES`] failures. The old note said
/// "rig has no {md} mode" for EVERY exhausted budget — but a run of link faults
/// (timeouts on a slow CI-V baud, a mute rig) proves nothing about the rig's modes,
/// and that wording sent an IC-7610 operator chasing a missing PKTUSB the rig has
/// always had (as USB-D). Only a run containing an active rejection (`RPRT -1`)
/// may blame the rig, and even then the note says what to DO, not just what failed.
fn mode_giveup_note(md: &str, saw_reject: bool, fallback: Option<&str>) -> String {
    if !saw_reject {
        return format!(
            "couldn't set {md}: no reply over CAT — link too slow or rig mute; try raising \
             the rig's CI-V baud (115200) and turning CI-V Transceive off — gave up"
        );
    }
    match fallback {
        Some(base) => format!(
            "rig refused {md} — set {base} instead; press the rig's DATA key ({base}-D) to work digital"
        ),
        None if mode_is_data(md) => format!(
            "rig refused {md} — couldn't set DATA mode; select USB-D/DATA on the rig by hand — gave up"
        ),
        None => format!("rig refused {md} — set the mode on the rig by hand — gave up"),
    }
}

/// After commanding a mode, read it straight back from the rig and describe the outcome —
/// the ONLY way to distinguish "rigctld answered RPRT 0 AND the rig actually changed" from
/// "rigctld answered RPRT 0 but the rig is still in the old mode" (a Hamlib/rig no-op). The
/// note is surfaced into the CAT status so the operator can see it on the rig.
fn mode_set_note(rig: &mut Rig, md: &str) -> String {
    // Read the rig's TRUE mode straight off the wire (raw Yaesu `MD0;` via rigctld send_cmd),
    // bypassing Hamlib's mode cache — `read_mode` (`m`) can report the commanded mode even
    // when the rig never moved (which fooled us once). The raw reply (e.g. "MD02;" = USB,
    // "MD0C;" = DATA-U on Yaesu) is the ground truth of what the radio is actually in.
    if let Some(raw) = rig.send_raw("MD0;") {
        return format!("sent {md} → rig raw mode {raw}");
    }
    match rig.read_mode() {
        Some(m) if m.eq_ignore_ascii_case(md) => format!("rig confirmed in {md}"),
        Some(m) => format!("set {md} but rig reports {m}"),
        None => format!("rig set to {md} (mode read-back unavailable)"),
    }
}

/// Describe a failed `set_mode` WITHOUT misdiagnosing the fault. The old note said
/// "rig rejected {mode}" for every failure, which sent operators of a broken CAT
/// link chasing a mode-support problem that doesn't exist. There are three distinct
/// faults, and the operator's fix differs for each:
///
/// - **Rig rejection** — `set_mode` reached the radio and it answered `RPRT -1`
///   (e.g. no DATA/PKT submode). This is the ONE case `set_mode` reports as
///   `ErrorKind::Other`, and the only one where "rig rejected" is accurate.
/// - **No reply** — the CAT bridge (rigctld) was reached and accepted the command,
///   but no complete reply came back before the deadline, or the link dropped
///   mid-reply (`TimedOut`/`UnexpectedEof`/`ConnectionReset`/`ConnectionAborted`/
///   `BrokenPipe`). The bridge is up but the RADIO behind it is mute — rig off/
///   asleep, wrong CAT port or model, serial baud mismatch, or (Flex) SmartSDR not
///   actually connected to the radio. This is the `rig reply incomplete after N ms`
///   case.
/// - **Unreachable** — the CAT endpoint refused the connection or isn't listening
///   (`ConnectionRefused` etc.): rigctld or SmartSDR not running, or the wrong
///   address/port. This is the Windows `os error 10061` case.
///
/// The raw `{e}` is kept in every message because its OS detail helps support.
fn mode_command_failed(md: &str, e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        Other => format!("rig rejected {md}: {e}"),
        TimedOut | UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe => {
            format!("no reply from the rig over CAT — couldn't set {md}: {e}")
        }
        _ => format!("can't reach the radio's CAT link — couldn't set {md}: {e}"),
    }
}

/// One short clause naming WHY a dial set failed, for the retry notes. Distinguishes the rig
/// actively refusing (`ErrorKind::Other` — a `RPRT <negative>` reply) from the link not answering,
/// because the two have completely different fixes: a different radio vs a cable/daemon.
fn dial_failure_brief(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match e.kind() {
        Other => "refused by the rig",
        TimedOut | UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe => {
            "no reply from the rig"
        }
        _ => "CAT link unreachable",
    }
}

/// The result of opening/probing a rig: `(rig, rigctld handle, cat_ok, detail)`.
/// `cat_ok` is `Some(true/false)` for CAT/serial, `None` for VOX; the handle
/// keeps the launched `rigctld` daemon alive (kill-on-drop).
/// Result of opening/probing a CAT channel: health + detail for the status pill, plus
/// the rig's OWN freq/mode read at open — the read-only-launch seed. `freq_hz`/`mode`
/// are `Some` only when a real read succeeded over a real control channel, which is
/// exactly the condition for `rig_confirmed` (NEVER derive that from `ok`: a serial-PTT
/// rig sharing the CAT port reports `ok == Some(true)` while being structurally
/// unreadable).
struct CatProbe {
    ok: Option<bool>,
    detail: String,
    freq_hz: Option<u64>,
    mode: Option<String>,
}

impl CatProbe {
    /// A status-only probe (VOX / serial PTT / error arms): no read happened.
    fn status(ok: Option<bool>, detail: impl Into<String>) -> Self {
        Self {
            ok,
            detail: detail.into(),
            freq_hz: None,
            mode: None,
        }
    }
}

type RigOpen = (Rig, Option<CatDaemon>, CatProbe);

/// The [`PttMode`] a transport keys with — mirrors `open_rig`'s ptt_method dispatch. A monitor
/// opens each background rig read-only (`PttMode::Vox`); when the handoff ADOPTS that rig as the
/// active radio, it must be switched to this real mode or `ptt()` silently no-ops (the "TX dead on
/// the FTDX10 after switching to it, but freq/mode still work" bug — Vox keying is a no-op while
/// set_freq/set_mode ignore the PTT mode).
fn ptt_mode_for(t: &Transport) -> PttMode {
    // Shared-port keying: rigctld holds the port and asserts the line on our behalf, so the
    // keying command goes to the DAEMON. Must be checked before the rts/dtr arms below, and
    // must stay in step with `open_rig` — both consult `keys_on_the_cat_port` for exactly that
    // reason. Handing back PttMode::Serial here would try to open a port rigctld owns: on
    // Windows that fails outright, and the operator sees a rig that tunes but never keys.
    if keys_on_the_cat_port(t) {
        return PttMode::Cat;
    }
    match t.ptt_method.as_str() {
        "cat" if t.rig_model != 0 => PttMode::Cat,
        "rts" => PttMode::Serial {
            port: t.ptt_port().to_string(),
            line: SerialLine::Rts,
        },
        "dtr" => PttMode::Serial {
            port: t.ptt_port().to_string(),
            line: SerialLine::Dtr,
        },
        _ => PttMode::Vox,
    }
}

/// Build the [`Rig`] for a transport and report its connection status. For CAT,
/// launches the bundled `rigctld`, sets the dial/mode, and probes by reading the
/// frequency back; for serial PTT it opens the control line; for VOX `cat_ok` is
/// `None` (not applicable). Mirrors WSJT-X's Test CAT.
fn open_rig(t: &Transport, allow_coexist: bool) -> RigOpen {
    match t.ptt_method.as_str() {
        // CAT PTT: control + keying both over rigctld.
        "cat" if t.rig_model != 0 => open_cat(t, PttMode::Cat, allow_coexist, None),
        "cat" => (
            Rig::vox(),
            None,
            CatProbe::status(
                Some(false),
                "CAT selected but no rig model is set — pick your rig in Settings.",
            ),
        ),
        // Serial-line PTT (RTS/DTR) — see `open_serial_ptt`. When keying is on a SEPARATE
        // port from CAT (an SO2R controller), we open CAT control too so freq/mode still
        // track; when it shares the CAT port, keying owns the port and there's no CAT
        // (launching rigctld there would fight for it).
        "rts" => open_serial_ptt(t, SerialLine::Rts, allow_coexist),
        "dtr" => open_serial_ptt(t, SerialLine::Dtr, allow_coexist),
        // VOX: the rig is keyed by its own VOX. But if a CAT rig is configured we STILL
        // open the control channel so freq/mode track the section — control is
        // INDEPENDENT of keying (the WSJT-X model). THIS is the fix for "the rig doesn't
        // change mode when I move between sections": before, a CAT rig keyed by VOX got
        // no `M`/`F` command at all because CAT was fused to the PTT method. (Matched
        // explicitly, not via the catch-all, so a typo'd/legacy ptt_method string
        // degrades safely to pure VOX below rather than silently grabbing the port.)
        "vox" if t.rig_model != 0 => open_cat(t, PttMode::Vox, allow_coexist, None),
        _ => (
            Rig::vox(),
            None,
            CatProbe::status(None, "VOX — no CAT; the rig is keyed by transmit audio."),
        ),
    }
}

/// Open serial-line (RTS/DTR) PTT, asserting the keying line on [`Transport::ptt_port`].
/// When that port is a DIFFERENT port from the CAT `serial_port` and a rig model is set —
/// the SO2R case, where a controller (u2R/MK2R) routes keying on its own COM port — we ALSO
/// open CAT control (rigctld on the CAT port) so frequency/mode still track the section,
/// exactly like the VOX+CAT path. When keying shares the CAT port (no dedicated PTT port),
/// we can't also run rigctld there (it would fight for the port), so it stays pure serial
/// keying with no CAT — the prior behavior.
fn open_serial_ptt(t: &Transport, line: SerialLine, allow_coexist: bool) -> RigOpen {
    let ptt_port = t.ptt_port().to_string();
    // Single-cable interface (Digirig Mobile): keying and CAT are the SAME port, so let rigctld
    // own it and do both. Hamlib shares the fd, so this is one open, not a fight for the port.
    if keys_on_the_cat_port(t) {
        // allow_coexist is deliberately FORCED OFF here. Coexisting means attaching to a rigctld
        // that is ALREADY listening — one we did not launch and whose --ptt-type we cannot know.
        // If it came up without keying flags (the default), every `T 1` we send is accepted and
        // does nothing: a rig that tunes, reports healthy, and never transmits. We must own a
        // daemon we know was told to key. If the port is genuinely held by someone else our
        // spawn fails and reports it, which is the honest outcome.
        let (rig, daemon, probe) = open_cat(t, PttMode::Cat, false, Some(line));
        // ⚠️ TX FLOOR. Before this change a shared-port operator keyed the line DIRECTLY and had
        // no CAT, so a wrong rig model cost them nothing they had. Now keying rides the daemon,
        // and if that daemon never came up they would lose TX as well — a strictly worse radio
        // for a CAT-only misconfiguration. When no daemon is running, nothing holds the port, so
        // we can still key it ourselves: fall back to exactly the old behaviour. A CAT problem
        // must never take the operator's transmitter away.
        if daemon.is_none() && probe.ok == Some(false) {
            let mut fallback = probe_serial(&ptt_port, line);
            fallback.2.detail = format!(
                "{} Keying {} directly instead — CAT is off until that is fixed.",
                probe.detail, ptt_port
            );
            return fallback;
        }
        return (rig, daemon, probe);
    }
    let separate = t.rig_model != 0 && !ptt_port.eq_ignore_ascii_case(t.serial_port.trim());
    if separate {
        open_cat(
            t,
            PttMode::Serial {
                port: ptt_port,
                line,
            },
            allow_coexist,
            None,
        )
    } else {
        // Pure serial keying, no CAT. After the shared-port branch above this is reached only
        // when NO rig model is set, so the honest report names the missing half: keying works,
        // but nothing will follow the band. This used to report a bare success and the operator
        // was left to work out why the radio ignored every band change.
        let mut open = probe_serial(&ptt_port, line);
        if t.rig_model == 0 && open.2.ok == Some(true) {
            open.2.detail = format!(
                "{} — no CAT (no rig model set), so the radio will not follow the band.",
                open.2.detail
            );
        }
        open
    }
}

/// Decide whether a rig SWITCH may auto-coexist onto a rigctld already listening on the new radio's
/// port. When we currently own a daemon (`owns_daemon`) and the new radio reuses its port
/// (`old_port == new_port`), the daemon "already here" after we kill ours is our own dying corpse —
/// coexisting onto it would keep commanding the OLD radio. Force a fresh daemon in that case; else a
/// genuinely external rigctld (WSJT-X, a different port, or one we never owned) may be shared. Pure.
fn allow_coexist_on_swap(owns_daemon: bool, old_port: u16, new_port: u16) -> bool {
    !(owns_daemon && old_port == new_port)
}

/// Open a CAT control channel via the bundled `rigctld` (launching it, or sharing one
/// already running) and PROBE it — read-only: the open commands nothing (read-only
/// launch); the probe's read seeds the app. `ptt_mode` layers on top so keying (CAT vs
/// VOX) stays independent of control. Used for BOTH a CAT-PTT rig and a VOX-keyed rig
/// that still has CAT freq/mode control.
///
/// `ptt_line` is `Some` ONLY for the shared-port keying case ([`keys_on_the_cat_port`]), where the
/// daemon we spawn must also be told to assert RTS/DTR on the port it opens. Callers passing
/// `Some` must also pass `allow_coexist == false`: an already-running daemon we did not launch
/// cannot be assumed to have keying enabled, and attaching to one that doesn't yields a rig that
/// tunes but never transmits.
fn open_cat(
    t: &Transport,
    ptt_mode: PttMode,
    allow_coexist: bool,
    ptt_line: Option<SerialLine>,
) -> RigOpen {
    debug_assert!(
        ptt_line.is_none() || !allow_coexist,
        "shared-port keying must own its daemon — coexisting risks silent no-key"
    );
    let addr = format!("127.0.0.1:{}", t.rigctld_port);
    if t.broker_self_port == Some(t.rigctld_port) {
        // Misconfig: our own CAT broker and the launched rigctld want the same port.
        // Don't connect to ourselves, and don't try to spawn (it can't bind) — tell the
        // operator to fix the ports.
        return (
            Rig::vox(),
            None,
            CatProbe::status(
                Some(false),
                format!(
                    "CAT broker and rigctld are both on :{} — give them different ports, or turn the broker off.",
                    t.rigctld_port
                ),
            ),
        );
    }
    if allow_coexist && crate::rigctld_server::probe_rigctld(&addr, Duration::from_millis(400)) {
        // Auto-coexist: a rigctld is ALREADY here (e.g. WSJT-X launched one). Connect
        // THROUGH it instead of fighting for the serial port. Skipped on a dual-radio SWITCH that
        // reuses the port of the daemon we just killed (`allow_coexist == false`), so we never
        // reconnect through our own dying daemon and keep commanding the OLD radio.
        let mut rig = Rig::with_control(Some(addr.clone()), ptt_mode);
        rig.set_slow_transport(
            t.is_network() || crate::rigmodels::is_slow_serial_link(t.rig_model, t.baud),
        ); // network chains + slow serial links (Xiegu / vintage Kenwood / any rig ≤ 19200 baud) get the long command deadline
        let mut probe = finish_cat_open(&mut rig, t);
        probe.detail = format!(
            "Sharing the rigctld already on :{} — {}",
            t.rigctld_port, probe.detail
        );
        return (
            rig, None, // we didn't spawn it — leave the existing daemon alone
            probe,
        );
    }
    // A network rig (Flex/SmartSDR or a remote rig) → point rigctld at host:port over TCP
    // (no serial device, no baud); else the serial port + baud as before.
    let (rig_target, network) = if t.is_network() {
        (t.rig_addr.as_str(), true)
    } else {
        (t.serial_port.as_str(), false)
    };
    match spawn_cat_daemon(t, rig_target, network, ptt_line) {
        Ok((proc, native_fallback)) => {
            // Give the daemon a moment to bind its TCP port before connecting.
            std::thread::sleep(Duration::from_millis(700));
            let mut rig = Rig::with_control(Some(addr), ptt_mode);
            rig.set_slow_transport(
                network
                    || native_civ_addr(t).is_some()
                    || crate::rigmodels::is_slow_serial_link(t.rig_model, t.baud),
            ); // network chains + the native daemon + slow serial links (Xiegu / vintage Kenwood / any rig ≤ 19200 baud) get the long deadline
            let mut probe = finish_cat_open(&mut rig, t);
            // Say WHICH backend this result came from — a native-CI-V radio silently
            // falling back to rigctld otherwise reads as "native was tested and failed".
            // Shared-port keying skips native BY DESIGN (rigctld must own keying), so it
            // is plain "Hamlib rigctld" there, not a fallback.
            let native_wanted = native_civ_addr(t).is_some() && ptt_line.is_none();
            probe.detail = with_backend(
                probe.detail,
                cat_backend_label(native_wanted, Some(matches!(proc, CatDaemon::Native(_)))),
            );
            if let Some(e) = native_fallback {
                probe.detail = format!("{} Native CI-V start error: {e}.", probe.detail);
            }
            (rig, Some(proc), probe)
        }
        Err(e) => (
            Rig::vox(),
            None,
            CatProbe::status(
                Some(false),
                format!("Could not launch the bundled rigctld (Hamlib): {e}"),
            ),
        ),
    }
}

/// The single shared tail of both `open_cat` branches (coexist + spawn): the open-time
/// dial/mode commands and the health probe. ONE copy on purpose — the read-only-launch
/// flip deletes the two commands here, and a duplicated tail is how a future edit
/// silently resurrects one of them (the tests exercise the coexist branch; this shared
/// seam is what makes them cover the spawn branch by construction).
fn finish_cat_open(rig: &mut Rig, t: &Transport) -> CatProbe {
    // READ-ONLY LAUNCH (operator-approved): the open no longer commands the rig.
    // The set_freq/set_mode that lived here for every session before 2026-07-21 are
    // deleted — the probe below READS the rig's own dial+mode and that read seeds the
    // app. The first genuine command happens when the operator enters a cockpit,
    // clicks a spot, or keys up (ensure_commanded / the retune paths). Do NOT re-add
    // a command here: launch_never_commands_the_rig pins this.
    probe_cat(rig, t.rigctld_port)
}

/// Probe a CAT rig by reading its frequency, mapping failures to a concrete,
/// operator-actionable message (rigctld unreachable vs. rig not answering).
fn probe_cat(rig: &mut Rig, port: u16) -> CatProbe {
    match rig.read_freq() {
        Ok(hz) => CatProbe {
            ok: Some(true),
            detail: format!("Connected — {:.3} MHz", hz as f64 / 1e6),
            freq_hz: Some(hz),
            // One mode read after a successful freq read — load-bearing now (the
            // read-only-launch seed), not cosmetic. Display-only downstream; Hamlib's
            // cached-mode caveat (rig.rs read_mode docs) is acceptable for display.
            mode: rig.read_mode(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => CatProbe::status(
            Some(false),
            format!("rigctld is not reachable on 127.0.0.1:{port}."),
        ),
        Err(e) => CatProbe::status(Some(false), format!("CAT error: {e}")),
    }
}

/// Build a serial-PTT rig and verify the control line opens (unkeyed = safe).
fn probe_serial(port: &str, line: SerialLine) -> RigOpen {
    let mut rig = Rig::serial(port, line);
    let shown = if port.is_empty() {
        "(no port set)"
    } else {
        port
    };
    let (ok, detail) = match rig.ptt(false) {
        Ok(()) => (Some(true), format!("Serial {line:?} PTT on {shown}")),
        Err(e) => (
            Some(false),
            format!("Could not open serial port {shown}: {e}"),
        ),
    };
    (rig, None, CatProbe::status(ok, detail))
}

/// Re-probe the *current* rig (the Test-CAT button) without rebuilding it, so it
/// doesn't fight the running rigctld for the serial port.
fn reprobe(rig: &mut Rig, t: &Transport) -> (Option<bool>, String) {
    match t.ptt_method.as_str() {
        "cat" if t.rig_model != 0 => probe_cat_or_explain(rig, t.rigctld_port),
        "cat" => (
            Some(false),
            "CAT selected but no rig model is set — pick your rig in Settings.".to_string(),
        ),
        // Shared CAT+keying port: rigctld owns the port and there IS a live control channel, so
        // Test CAT must probe it. Reporting "Serial PTT on COM5" here would contradict what the
        // app is actually doing and hide a genuinely broken CAT link behind a green pill.
        _ if keys_on_the_cat_port(t) => probe_cat_or_explain(rig, t.rigctld_port),
        "rts" | "dtr" => {
            let shown = if t.ptt_port().is_empty() {
                "(no port set)"
            } else {
                t.ptt_port()
            };
            match rig.ptt(false) {
                Ok(()) => {
                    // Say what is NOT happening. Keying works and the pill goes green, but with
                    // no rig model there is no CAT at all — the band will not follow, and the
                    // operator otherwise has to infer that from a control that looks healthy.
                    let detail = if t.rig_model == 0 {
                        format!("Serial PTT on {shown} — no CAT (no rig model set), so the radio will not follow the band.")
                    } else {
                        format!("Serial PTT on {shown}")
                    };
                    (Some(true), detail)
                }
                Err(e) => (
                    Some(false),
                    format!("Could not open serial port {shown}: {e}"),
                ),
            }
        }
        // VOX with a CAT rig configured: keying is VOX, but CAT control is live, so the
        // Test-CAT button must probe the (real) control channel — not report "no CAT".
        "vox" if t.rig_model != 0 => probe_cat_or_explain(rig, t.rigctld_port),
        _ => (None, "VOX — no CAT.".to_string()),
    }
}

/// Probe the live rig's CAT channel — but if it has NO control channel (open_cat fell
/// back to a control-less rig: serial-port conflict, or rigctld failed to launch),
/// `read_freq` would return a misleading "not a CAT rig" error. Detect that up front
/// and explain the real cause instead.
fn probe_cat_or_explain(rig: &mut Rig, port: u16) -> (Option<bool>, String) {
    if rig.has_control() {
        let p = probe_cat(rig, port);
        (p.ok, p.detail)
    } else {
        (
            Some(false),
            "CAT rig configured, but the control channel didn't open — check the rig model, \
             serial port, and that the bundled rigctld could start (or a port conflict)."
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;

    /// The IC-7610 zero-bytes saga, P1: Test CAT (and the CAT-down breaker) must SAY which
    /// backend was actually exercised — a native-CI-V radio silently falling back to
    /// rigctld otherwise reads as "native was tested and failed", and the operator debugs
    /// the wrong daemon.
    #[test]
    fn backend_attribution_names_the_daemon_that_was_actually_tested() {
        assert_eq!(cat_backend_label(true, Some(true)), "native CI-V");
        assert_eq!(cat_backend_label(false, Some(false)), "Hamlib rigctld");
        assert_eq!(
            cat_backend_label(true, Some(false)),
            "Hamlib rigctld — the native CI-V daemon didn't start"
        );
        assert_eq!(cat_backend_label(false, None), "a shared external rigctld");
        assert_eq!(
            with_backend(
                "Connected — 14.074 MHz".to_string(),
                cat_backend_label(true, Some(true))
            ),
            "Connected — 14.074 MHz (via native CI-V)"
        );
    }

    #[test]
    fn an_unanswerable_dsp_func_backs_off_instead_of_stalling_forever() {
        // Model the state machine exactly as the poll site drives it.
        let mut supported: Option<bool> = None;
        let mut misses: u8 = 0;
        let mut retry_at: u32 = 0;
        let mut backoff: u32 = FUNC_RETRY_BACKOFF_BASE;
        let mut probes: Vec<u32> = Vec::new();
        // When the func latched off — the boundary between probe BURSTS. Within a burst the
        // three probes are always 20 ticks apart; it is the gap BETWEEN bursts that must widen.
        let mut latch_offs: Vec<u32> = Vec::new();

        // 4000 heavy polls ≈ 50 minutes of operating. The rig NEVER answers this func.
        for tick in 0..4000u32 {
            if supported == Some(false) && tick >= retry_at {
                supported = None; // re-arm
                misses = 0;
            }
            // One func is probed per 20 heavy polls (round-robin over 5, on `%4==2`).
            if tick % 20 == 2 && supported != Some(false) {
                probes.push(tick); // this probe BLOCKS to the CAT deadline
                misses = misses.saturating_add(1);
                if misses >= 3 {
                    supported = Some(false);
                    latch_offs.push(tick);
                    retry_at = tick.saturating_add(backoff);
                    backoff = backoff.saturating_mul(2).min(FUNC_RETRY_BACKOFF_MAX);
                }
            }
        }

        // OLD behaviour (fixed 40-tick re-arm) probed indefinitely — roughly one stall per
        // 30 s for the whole session. Backing off must cut that hard.
        assert!(
            probes.len() < 40,
            "an unanswerable func must stop being re-probed on a fixed cycle; got {} probes \
             in 4000 heavy polls (~50 min): {:?}",
            probes.len(),
            probes
        );
        // And the interval between BURSTS must grow — that is what makes a permanently-absent
        // func eventually stop costing anything.
        assert!(
            latch_offs.len() >= 3,
            "expected several give-up cycles to compare, got {latch_offs:?}"
        );
        let first_gap = latch_offs[1] - latch_offs[0];
        let last_gap = latch_offs[latch_offs.len() - 1] - latch_offs[latch_offs.len() - 2];
        assert!(
            last_gap > first_gap * 2,
            "retry interval must widen sharply (first gap {first_gap}, last {last_gap}, \
             latch-offs {latch_offs:?})"
        );
    }

    /// The backoff must NOT persist once the rig starts answering: a func that recovers has to
    /// regain full responsiveness, or a transient CAT hiccup would permanently degrade it.
    #[test]
    fn a_recovered_dsp_func_resets_its_backoff() {
        let mut backoff: u32 = FUNC_RETRY_BACKOFF_BASE;
        // Three latch-offs in a row grow the backoff.
        for _ in 0..3 {
            backoff = backoff.saturating_mul(2).min(FUNC_RETRY_BACKOFF_MAX);
        }
        assert!(
            backoff > FUNC_RETRY_BACKOFF_BASE,
            "backoff grew while failing"
        );
        // A successful read resets it (the `Some(on)` arm at the poll site).
        backoff = FUNC_RETRY_BACKOFF_BASE;
        assert_eq!(
            backoff, FUNC_RETRY_BACKOFF_BASE,
            "a func that answers again is probed at the base cadence, not the degraded one"
        );
    }

    /// The decode worker roundtrips a real job off the calling thread: build a job
    /// under the "engine lock", dispatch it, receive the result, fold it. This is
    /// the whole async path minus the radio loop — the decode ran on the worker
    /// thread, never touching the engine.
    #[test]
    fn decode_worker_roundtrips_a_job() {
        let mut eng = Engine::new("KD9TAW", "EN52", 0);
        eng.set_tier(Tier::Ft8);
        let worker = DecodeWorker::spawn();
        let job = eng.build_decode_job(
            vec![0.0f32; eng.active_capture_samples()],
            4,
            DecodePass::Boundary,
        );
        worker.dispatch(job);
        // Wait (bounded) for the worker to finish — it runs on its own thread.
        let mut result = None;
        for _ in 0..500 {
            if let Some(r) = worker.try_recv() {
                result = Some(r);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let result = result.expect("worker returned a result");
        assert!(
            matches!(
                eng.apply_decode_result(result),
                DecodeApplied::Boundary { .. }
            ),
            "the worker's result folds as a boundary decode"
        );
        // Drop joins the worker thread cleanly (no leak).
        drop(worker);
    }

    /// In-flight guard: at most one decode is dispatched at a time. This mirrors the
    /// exact predicate `step` uses — a boundary that wants a decode dispatches only
    /// when `!decode_in_flight`, so an early pass in flight defers the boundary a
    /// tick (its `early_seen` folds first) instead of racing the single worker.
    #[test]
    fn in_flight_guard_serializes_dispatch() {
        let mut eng = Engine::new("KD9TAW", "EN52", 0);
        eng.set_tier(Tier::Ft8);
        let worker = DecodeWorker::spawn();
        let mut in_flight = false;

        // First boundary that wants a decode: dispatched, flag raised.
        let wants = crate::slot::slot_wants_decode(false, false, false);
        assert!(wants);
        let mut dispatched = 0;
        if wants && !in_flight {
            worker.dispatch(eng.build_decode_job(
                vec![0.0f32; eng.active_capture_samples()],
                1,
                DecodePass::Boundary,
            ));
            in_flight = true;
            dispatched += 1;
        }
        // A second boundary arriving before the first drains must NOT dispatch.
        if wants && !in_flight {
            worker.dispatch(eng.build_decode_job(
                vec![0.0f32; eng.active_capture_samples()],
                2,
                DecodePass::Boundary,
            ));
            dispatched += 1;
        }
        assert_eq!(
            dispatched, 1,
            "the guard blocks a second concurrent dispatch"
        );

        // Drain the one result → flag clears → the next dispatch is allowed again.
        let mut got = false;
        for _ in 0..500 {
            if let Some(r) = worker.try_recv() {
                let _ = eng.apply_decode_result(r);
                in_flight = false;
                got = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(got, "the in-flight decode completed and drained");
        assert!(!in_flight, "the guard is cleared once the result drains");
    }

    #[test]
    fn tier_mode_maps_each_tier() {
        assert_eq!(tier_mode(Tier::TempoFast), "TempoFast");
        assert_eq!(tier_mode(Tier::TempoDeep), "TempoDeep");
        assert_eq!(tier_mode(Tier::Ft8), "FT8");
        assert_eq!(tier_mode(Tier::Ft4), "FT4");
    }

    #[test]
    fn build_wsjtx_server_gates_on_enable_and_valid_addr() {
        // Disabled → no socket regardless of address (the state before a toggle-on).
        assert!(build_wsjtx_server(false, "127.0.0.1:2237").is_none());
        // Enabled but unparseable target → None, not a panic.
        assert!(build_wsjtx_server(true, "not-an-address").is_none());
        assert!(build_wsjtx_server(true, "").is_none());
        // Enabled + valid loopback target → a bound emitter (this is what a live
        // toggle-on rebuilds; the opening Heartbeat to :2237 is harmless if unheard).
        assert!(build_wsjtx_server(true, "127.0.0.1:2237").is_some());
    }

    #[test]
    fn mode_is_data_classifies_pkt_and_data_modes() {
        // FT8 (PKTUSB) etc. are data modes → the Icom tune must NOT flip DATA off on release.
        assert!(mode_is_data("PKTUSB"));
        assert!(mode_is_data("PKTLSB"));
        assert!(mode_is_data("data-u"));
        assert!(mode_is_data(" DATA-L "));
        // Plain voice/CW modes are NOT — tune temporarily flips them into DATA and restores.
        assert!(!mode_is_data("USB"));
        assert!(!mode_is_data("LSB"));
        assert!(!mode_is_data("CW"));
        assert!(!mode_is_data("FM"));
        assert!(!mode_is_data(""));
    }

    #[test]
    fn mode_command_failed_distinguishes_the_three_cat_faults() {
        use std::io::{Error, ErrorKind};
        // No CAT endpoint listening (`os error 10061`) — the operator must START the
        // bridge (rigctld / SmartSDR). Not a mode problem, not a mute-rig problem.
        for kind in [ErrorKind::ConnectionRefused, ErrorKind::NotConnected] {
            let note = mode_command_failed("PKTUSB", &Error::new(kind, "actively refused it"));
            assert!(note.contains("can't reach the radio's CAT link"), "{note}");
            assert!(
                !note.contains("rejected"),
                "must not blame the mode: {note}"
            );
        }
        // Bridge reached but the radio never answered — the `rig reply incomplete after
        // 2500 ms` case. Reported as "no reply from the rig", NOT "rig rejected".
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::UnexpectedEof,
            ErrorKind::ConnectionReset,
            ErrorKind::BrokenPipe,
        ] {
            let note = mode_command_failed("PKTUSB", &Error::new(kind, "rig reply incomplete"));
            assert!(note.contains("no reply from the rig"), "{note}");
            assert!(
                !note.contains("rejected"),
                "must not blame the mode: {note}"
            );
        }
        // A genuine rejection — set_mode surfaces `RPRT -1` as ErrorKind::Other — keeps
        // the "rig rejected" wording, the accurate diagnosis there.
        let note = mode_command_failed(
            "PKTUSB",
            &Error::other("rigctld mode error: \"RPRT -1\\n\""),
        );
        assert!(
            note.contains("rig rejected PKTUSB"),
            "rejection note: {note}"
        );
    }

    #[test]
    fn retry_passband_goes_filter_agnostic_only_for_data_modes() {
        // Rung 1 of the ladder: DATA modes open with the full 3 kHz passband…
        assert_eq!(retry_passband("PKTUSB", 0), 3000);
        assert_eq!(retry_passband("PKTLSB", MODE_SET_PASSBAND0_AFTER - 1), 3000);
        // Rung 2: …then drop to 0 (the rig's own default width) once the run keeps
        // failing — a backend that rejects the width→DATA-filter mapping (not the
        // mode itself) still gets the MODE set.
        assert_eq!(retry_passband("PKTUSB", MODE_SET_PASSBAND0_AFTER), 0);
        assert_eq!(retry_passband("PKTLSB", MODE_SET_MAX_TRIES - 1), 0);
        // Voice/CW never leave -1 (NOCHANGE): 0 would actively re-command the default
        // width and pop the rig's Width display — the bug passband_for exists to avoid.
        assert_eq!(retry_passband("USB", 0), -1);
        assert_eq!(retry_passband("CW", MODE_SET_MAX_TRIES), -1);
    }

    #[test]
    fn fallback_sideband_maps_data_submodes_to_their_plain_sideband() {
        assert_eq!(fallback_sideband("PKTUSB"), Some("USB"));
        assert_eq!(fallback_sideband(" pktlsb "), Some("LSB"));
        assert_eq!(fallback_sideband("DATA-U"), Some("USB"));
        assert_eq!(fallback_sideband("DATA-L"), Some("LSB"));
        // Non-DATA modes have no sensible sideband fallback — give up in place.
        assert_eq!(fallback_sideband("CW"), None);
        assert_eq!(fallback_sideband("USB"), None);
        assert_eq!(fallback_sideband("FM"), None);
    }

    #[test]
    fn mode_giveup_note_blames_the_link_not_the_mode_on_timeouts() {
        // An all-timeout run (the IC-7610 @ 19200 CI-V baud case) proves nothing about
        // the rig's modes — the old "rig has no PKTUSB mode" wording sent the operator
        // chasing a mode the rig has always had (USB-D). The note must blame the LINK
        // and say what to do about it.
        let n = mode_giveup_note("PKTUSB", false, None);
        assert!(!n.contains("has no"), "must not blame the mode: {n}");
        assert!(!n.contains("refused"), "must not blame the rig: {n}");
        assert!(n.contains("CI-V baud"), "must be actionable: {n}");

        // Active rejection + the plain-sideband fallback landed: one front-panel DATA
        // press from working — the note says exactly that.
        let n = mode_giveup_note("PKTUSB", true, Some("USB"));
        assert!(n.contains("refused PKTUSB"), "{n}");
        assert!(n.contains("USB-D"), "must name the rig-side mode: {n}");

        // Active rejection and even plain USB failed: still actionable for DATA modes.
        let n = mode_giveup_note("PKTUSB", true, None);
        assert!(n.contains("USB-D/DATA"), "{n}");

        // A non-DATA rejection: honest, no bogus DATA advice.
        let n = mode_giveup_note("CW", true, None);
        assert!(n.contains("refused CW"), "{n}");
        assert!(!n.contains("USB-D"), "{n}");
    }

    #[test]
    fn build_decode_carries_decode_fields() {
        let d = build_decode("CQ W1AW FN31", -7, 0.1, 1200.0, "FT8", 5000, false);
        assert_eq!(d.message, "CQ W1AW FN31");
        assert_eq!(d.snr, -7);
        assert_eq!(d.mode, "FT8");
        assert_eq!(d.delta_freq, 1200);
        assert!((d.delta_time - 0.1).abs() < 1e-6);
        assert_eq!(d.time_ms, 5000);
        assert!(d.new && !d.off_air);
    }

    #[test]
    fn build_spot_reports_sender_at_rf_frequency() {
        // Audio offset adds onto the dial: 14.074 MHz + 1200 Hz audio.
        let spot = build_spot("CQ W1AW FN31", -7, 1200.0, "FT8", 14_074_000, 1_700_000_000)
            .expect("a CQ has a sender");
        assert_eq!(spot.call, "W1AW");
        assert_eq!(spot.freq_hz, 14_074_000 + 1200);
        assert_eq!(spot.snr, -7);
        assert_eq!(spot.mode, "FT8");
        assert_eq!(spot.time_secs, 1_700_000_000);
    }

    #[test]
    fn build_spot_skips_senderless_text() {
        // Free text (no `de` callsign) is never reported to PSK Reporter.
        assert!(build_spot("thanks for the qso", -7, 1200.0, "FT8", 14_074_000, 0).is_none());
    }

    fn test_settings() -> Settings {
        Settings {
            ptt_method: "cat".to_string(),
            rig_model: 1035,
            serial_port: "/dev/ttyUSB0".to_string(),
            baud: 38400,
            rigctld_port: 4532,
            audio_in: "USB Audio CODEC".to_string(),
            audio_out: "USB Audio CODEC".to_string(),
            tx_level: 0.8,
            ..Settings::default()
        }
    }

    #[test]
    fn transport_from_settings_maps_fields() {
        let t = Transport::from_settings(&test_settings());
        assert_eq!(t.ptt_method, "cat");
        assert_eq!(t.rig_model, 1035);
        assert_eq!(t.serial_port, "/dev/ttyUSB0");
        assert_eq!(t.baud, 38400);
        assert_eq!(t.rigctld_port, 4532);
        assert_eq!(t.audio_in, "USB Audio CODEC");
        assert_eq!(t.audio_out, "USB Audio CODEC");
    }

    #[test]
    fn transport_rig_differs_on_cat_changes_not_audio() {
        let base = Transport::from_settings(&test_settings());
        // Identical → no rig rebuild.
        assert!(!base.rig_differs(&base.clone()));

        // Each CAT-affecting field triggers a rebuild ("CAT reconnects on Save").
        let mutations: [fn(&mut Settings); 5] = [
            |s| s.ptt_method = "vox".to_string(),
            |s| s.rig_model = 311,
            |s| s.serial_port = "/dev/ttyUSB1".to_string(),
            |s| s.baud = 19200,
            |s| s.rigctld_port = 4533,
        ];
        for mutate in mutations {
            let mut s = test_settings();
            mutate(&mut s);
            assert!(
                base.rig_differs(&Transport::from_settings(&s)),
                "a CAT-affecting change should rebuild the rig"
            );
        }

        // An audio-only change must NOT rebuild the rig.
        let mut s = test_settings();
        s.audio_in = "Other Card".to_string();
        assert!(!base.rig_differs(&Transport::from_settings(&s)));
    }

    #[test]
    fn transport_monitor_differs_on_monitor_settings_only() {
        let base = Transport::from_settings(&test_settings());
        assert!(!base.monitor_differs(&base.clone()));

        // Each monitor field flags a change (drives an in-place reconfigure).
        let mutations: [fn(&mut Settings); 3] = [
            |s| s.monitor_enabled = true,
            |s| s.monitor_device = "Headphones".to_string(),
            |s| s.monitor_level = 0.9,
        ];
        for mutate in mutations {
            let mut s = test_settings();
            mutate(&mut s);
            assert!(base.monitor_differs(&Transport::from_settings(&s)));
        }

        // A monitor change must NOT rebuild the rig OR re-open the capture streams
        // (the decode path never restarts for a monitor toggle).
        let mut s = test_settings();
        s.monitor_enabled = true;
        s.monitor_device = "Headphones".to_string();
        let want = Transport::from_settings(&s);
        assert!(
            !base.rig_differs(&want),
            "monitor change never rebuilds the rig"
        );
        assert!(
            !base.audio_differs(&want),
            "monitor change never re-opens the capture/TX streams"
        );
    }

    #[test]
    fn transport_audio_differs_on_device_change_only() {
        let base = Transport::from_settings(&test_settings());
        assert!(!base.audio_differs(&base.clone()));

        let mut s = test_settings();
        s.audio_out = "Speakers".to_string();
        assert!(base.audio_differs(&Transport::from_settings(&s)));

        // A rig-only change must NOT re-open the sound card.
        let mut s = test_settings();
        s.rig_model = 1;
        assert!(!base.audio_differs(&Transport::from_settings(&s)));
    }

    // ---- the full loop core (RadioLoop::step), driven hardware-free ----

    fn loop_state() -> RadioLoop {
        RadioLoop::new(
            Transport::from_cfg(&RadioConfig::default()),
            None,
            &RadioConfig::default(),
        )
    }
    fn no_sinks() -> Sinks<'static> {
        Sinks {
            wsjtx: None,
            psk: None,
            cfg_dial_hz: 14_090_500,
        }
    }
    fn mock_reopen_audio() -> impl FnMut(&Transport) -> Result<MockBackend, String> {
        |_t: &Transport| Ok(MockBackend::new())
    }
    fn mock_reopen_rig() -> impl FnMut(&Transport, bool) -> RigOpen {
        |_t: &Transport, _coexist: bool| (Rig::vox(), None, CatProbe::status(None, ""))
    }

    /// A rigctld that REJECTS every DATA-mode set (`M PKT*` → `RPRT -1`) but accepts
    /// everything else — the "rig refused PKTUSB" shape of the IC-7610 report — while
    /// logging every command line it was sent. Serves connections sequentially forever.
    fn mock_pkt_rejecting_rigctld() -> (String, Arc<Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let log2 = Arc::clone(&log);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(r) => r,
                    Err(_) => continue,
                });
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let l = line.trim().to_string();
                    log2.lock().unwrap().push(l.clone());
                    let reply = if l == "f" {
                        "14074000\n"
                    } else if l.starts_with("M PKT") {
                        "RPRT -1\n"
                    } else {
                        "RPRT 0\n"
                    };
                    if stream.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        });
        (addr, log)
    }

    #[test]
    fn spectrum_source_reconcile_gates_on_capability() {
        // The native panadapter worker is started ONLY for a native-scope rig, and stays inert
        // without the config it needs — so a Yaesu/Icom-serial station never spawns Flex threads.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut state = loop_state();

        // A Yaesu FTDX10 (model 1042) has no native RF scope → nothing started.
        state.reconcile_spectrum_source(&engine, 1042, false);
        assert!(state.spectrum_src_key.is_none());
        assert!(state.spectrum_src.is_none());

        // A Flex (model 2036, network) IS scope-capable, but the native panadapter is OPT-IN and
        // OFF by default → no worker, and (key folds in the gate) no key either.
        state.reconcile_spectrum_source(&engine, 2036, true);
        assert!(
            state.spectrum_src_key.is_none(),
            "flex_native_pan off → no worker, no key (unverified feature stays inert)"
        );
        assert!(state.spectrum_src.is_none());

        // Enable the opt-in. Still no `flex_radio_ip`, so the worker is inert — but the key is now
        // remembered so a network Flex's ticks are a no-op, and no connection is made.
        {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.flex_native_pan = true;
            e.apply_settings(s);
        }
        state.reconcile_spectrum_source(&engine, 2036, true);
        assert_eq!(
            state.spectrum_src_key,
            Some((2036, true)),
            "opt-in on → key remembered"
        );
        assert!(
            state.spectrum_src.is_none(),
            "empty flex_radio_ip → no worker started (no network I/O)"
        );

        // Switching back to the Yaesu clears the key (would tear down a running worker).
        state.reconcile_spectrum_source(&engine, 1042, false);
        assert!(state.spectrum_src_key.is_none());
    }

    #[test]
    fn switch_reusing_own_port_forces_a_fresh_daemon() {
        // Dual-radio: two radios sharing a rigctld port. Switching between them must NOT coexist onto
        // the just-killed daemon (that kept commanding the old rig — the "switch back to HF still
        // drives the 2 m Icom" bug); it must spawn fresh. Distinct ports coexist normally, and a
        // switch where we owned no daemon (we were sharing an external rigctld) still coexists.
        assert!(
            !allow_coexist_on_swap(true, 4532, 4532),
            "own daemon + same port → spawn fresh"
        );
        assert!(
            allow_coexist_on_swap(true, 4532, 4534),
            "own daemon + different port → normal probe"
        );
        assert!(
            allow_coexist_on_swap(false, 4532, 4532),
            "no owned daemon (external share) → coexist"
        );
        assert!(
            allow_coexist_on_swap(false, 4532, 4534),
            "no owned daemon, different port → coexist"
        );
    }

    /// A pool with THREE radios configured: radio 0 active, radios 1 and 2 as live monitors. Returns
    /// `(engine, pool, [port0, port1, port2])`. Every radio test in this file until now built exactly
    /// "profile 0 + one add_radio", so nothing exercised a pool holding more than ONE monitor — which
    /// is where every "the other radio" assumption would show up.
    #[allow(clippy::type_complexity)]
    fn three_radio_pool() -> (Arc<Mutex<Engine>>, MonitorPool, [u16; 3]) {
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        let (r1, r2, ports, transports) = {
            let mut e = engine.lock().unwrap();
            let r1 = e.add_radio();
            let r2 = e.add_radio();
            e.set_active_radio(0);
            let prof = |id: u32| {
                e.settings()
                    .radios
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap()
                    .clone()
            };
            let (p0, p1, p2) = (prof(0), prof(r1), prof(r2));
            (
                r1,
                r2,
                [p0.rigctld_port, p1.rigctld_port, p2.rigctld_port],
                [Transport::from_profile(&p1), Transport::from_profile(&p2)],
            )
        };
        let conn = |id: u32, port: u16, transport: Transport| MonitorConn {
            id,
            transport,
            rig: Rig::with_control(Some(format!("127.0.0.1:{port}")), PttMode::Vox),
            rigctld_proc: None,
            last_poll: 0.0,
            ticks: 0,
            smeter_supported: None,
            freq_misses: 0,
            open_failures: 0,
            retry_after_ms: 0.0,
        };
        let pool: MonitorPool = Arc::new(Mutex::new(vec![
            conn(r1, ports[1], transports[0].clone()),
            conn(r2, ports[2], transports[1].clone()),
        ]));
        (engine, pool, ports)
    }

    #[test]
    fn three_radios_get_distinct_daemon_ports_and_two_live_monitors() {
        // Two live rigctld daemons already needed distinct ports; a third must too, and the pool must
        // actually hold TWO monitors rather than collapsing to one.
        let (engine, pool, ports) = three_radio_pool();
        let mut sorted = ports.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            3,
            "three radios, three distinct ports: {ports:?}"
        );
        assert_eq!(engine.lock().unwrap().settings().radios.len(), 3);
        assert_eq!(
            pool.lock().unwrap().len(),
            2,
            "both non-active radios monitored"
        );
    }

    #[test]
    fn a_handoff_across_three_radios_adopts_the_right_one_and_leaves_the_third_alone() {
        // With TWO radios a handoff is unambiguous: there is exactly one conn in the pool, so
        // "swap with the pool" cannot pick wrong. With three it can. Switching 0 → 2 must adopt
        // radio 2's conn, demote radio 0 into the pool as Vox (a monitor must never key), and leave
        // radio 1's monitor completely untouched — still monitored, still unable to transmit.
        let (engine, pool, ports) = three_radio_pool();
        let (r1, r2) = (1u32, 2u32);
        let mut state = loop_state();
        state.applied = cat_transport(ports[0], None);
        // Radio 0 is a live CAT rig (the operating radio), so the demotion to Vox is observable.
        let mut rig = Rig::with_control(Some(format!("127.0.0.1:{}", ports[0])), PttMode::Cat);
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        engine.lock().unwrap().set_active_radio(r2);

        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );

        assert_eq!(last_active, r2, "switched to radio 2, not radio 1");
        assert_eq!(
            state.applied.rigctld_port, ports[2],
            "the ADOPTED transport is radio 2's — picking radio 1's would drive the wrong rig"
        );
        let mut p = pool.lock().unwrap();
        p.sort_by_key(|c| c.id);
        assert_eq!(
            p.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![0, r1],
            "radio 0 demoted in, radio 2 taken out, radio 1 still pooled"
        );
        assert_eq!(
            p[0].rig.ptt_mode(),
            &PttMode::Vox,
            "the demoted radio 0 can never key while it is a read-only monitor"
        );
        assert_eq!(
            p[1].rig.ptt_mode(),
            &PttMode::Vox,
            "the untouched third radio is still a read-only monitor"
        );
        assert_eq!(
            p[1].transport.rigctld_port, ports[1],
            "radio 1's conn was not rebuilt or repointed by the handoff"
        );
    }

    #[test]
    fn two_monitors_take_turns_so_neither_starves() {
        // `poll_monitors` services ONE conn per call. With a single monitor that is trivially fair;
        // with two, an unfair pick (e.g. always the first, or always the same one on a tie) would
        // leave one radio's pill frozen forever. It must always take the MOST OVERDUE.
        let (engine, pool, _ports) = three_radio_pool();
        {
            let mut p = pool.lock().unwrap();
            p[0].last_poll = 0.0; // radio 1 — most overdue
            p[1].last_poll = 100.0; // radio 2
        }
        let pending = std::sync::atomic::AtomicBool::new(false);
        poll_monitors(&pool, 0, &engine, &pending);
        {
            let p = pool.lock().unwrap();
            assert_eq!(p[0].ticks, 1, "the most-overdue monitor was polled");
            assert_eq!(
                p[1].ticks, 0,
                "…and only that one (one read burst per call)"
            );
        }
        // Now radio 2 is the most overdue, so the NEXT call must serve it — not radio 1 again.
        poll_monitors(&pool, 0, &engine, &pending);
        let p = pool.lock().unwrap();
        assert_eq!(p[0].ticks, 1);
        assert_eq!(p[1].ticks, 1, "the second monitor got its turn");
    }

    #[test]
    fn handoff_swaps_active_radio_with_the_pool_no_teardown() {
        // Durable dual-radio: switching the active radio HANDS the (already-connected) new active Rig
        // OUT of the monitor pool into the active slot, and pushes the OLD active back INTO the pool —
        // no teardown/rebuild, so the dial can't race back to the old rig. `self.applied` becomes the
        // new radio's transport, which is exactly why the `rig_differs` teardown then never fires.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let (r1, r1_transport, r1_port) = {
            let mut e = engine.lock().unwrap();
            let r1 = e.add_radio(); // radios = [0, 1]; active still 0
            let p = e
                .settings()
                .radios
                .iter()
                .find(|p| p.id == r1)
                .unwrap()
                .clone();
            // The monitor conn's transport must equal what `from_settings` yields once r1 is active
            // (i.e. r1's profile) — else the handoff correctly REFUSES to adopt a stale conn (fix #3).
            (r1, Transport::from_profile(&p), p.rigctld_port)
        };
        let mut state = loop_state();
        state.applied = cat_transport(4532, None); // radio 0 (active) on its port
        let mut rig = Rig::vox();
        // Radio 1 is already LIVE in the monitor pool with a transport matching its profile. A live
        // monitor conn holds a control-bearing Rig (`with_control`) + its own daemon — only such a conn
        // is adopted (a dead `Rig::vox()` conn is rejected; see `handoff_skips_a_dead_conn…`).
        let pool: MonitorPool = Arc::new(Mutex::new(vec![MonitorConn {
            id: r1,
            transport: r1_transport,
            rig: Rig::with_control(Some(format!("127.0.0.1:{r1_port}")), PttMode::Vox),
            rigctld_proc: None,
            last_poll: 0.0,
            ticks: 0,
            smeter_supported: None,
            freq_misses: 0,
            open_failures: 0,
            retry_after_ms: 0.0,
        }]));
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        engine.lock().unwrap().set_active_radio(r1); // operator switches to radio 1
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );

        assert_eq!(last_active, r1, "active tracked to radio 1");
        assert!(
            state.force_audio_rebuild,
            "a switch forces the RX audio to rebuild to the new radio's device (even if names match)"
        );
        assert_eq!(
            state.applied.rigctld_port, r1_port,
            "active transport is now radio 1's — a HANDOFF, so rig_differs won't rebuild"
        );
        assert_eq!(
            state.last_dial, 0,
            "caches reset so the retune re-asserts the restored dial"
        );
        let p = pool.lock().unwrap();
        assert_eq!(p.len(), 1, "pool still holds exactly one monitor");
        assert_eq!(
            p[0].id, 0,
            "the OLD active (radio 0) is now the monitor — stayed live, not torn down"
        );
        assert_eq!(
            p[0].transport.rigctld_port, 4532,
            "old active's transport preserved in the pool"
        );
    }

    /// A minimal in-test rigctld: answers every request line with "RPRT 0" and records each
    /// received line. Enough for command-class verbs (F/M/T/\stop_morse) — exactly what the
    /// contended-switch test needs to observe going to the OLD rig.
    fn recording_rigctld_stub() -> (String, Arc<Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut out = match stream.try_clone() {
                    Ok(o) => o,
                    Err(_) => return,
                };
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    rec.lock().unwrap().push(line);
                    if out.write_all(b"RPRT 0\n").is_err() {
                        break;
                    }
                }
            }
        });
        (addr, seen)
    }

    /// A rigctld that behaves like a REAL rig: it remembers the frequency it was set to and
    /// answers a dial READ with it — but reports the PREVIOUS value for `lag` reads after a
    /// change, modelling Hamlib's get-cache / a slow serial chain. That lag is the documented
    /// hazard behind the read-back guard: a stale read adopted as a knob QSY reverts the QSY.
    fn lagging_rigctld_stub(lag: usize) -> (String, Arc<Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = seen.clone();
        std::thread::spawn(move || {
            let mut cur: u64 = 144_174_000;
            let mut stale: u64 = 144_174_000;
            let mut pending = 0usize;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut out = match stream.try_clone() {
                    Ok(o) => o,
                    Err(_) => return,
                };
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    rec.lock().unwrap().push(line.clone());
                    let reply = if let Some(hz) = line.strip_prefix("F ") {
                        if let Ok(v) = hz.trim().parse::<u64>() {
                            cur = v;
                            pending = lag;
                        }
                        "RPRT 0\n".to_string()
                    } else if line.trim() == "f" {
                        let report = if pending > 0 {
                            pending -= 1;
                            stale
                        } else {
                            stale = cur;
                            cur
                        };
                        format!("{report}\n")
                    } else {
                        "RPRT 0\n".to_string()
                    };
                    if out.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        });
        (addr, seen)
    }

    /// Arrange the standard two-radio switch scene: engine with radio 0 active + radio 1 LIVE
    /// in the monitor pool (a control-bearing conn matching r1's profile transport).
    fn switch_scene() -> (Arc<Mutex<Engine>>, MonitorPool, RadioLoop, u32, u16) {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let (r1, r1_transport, r1_port) = {
            let mut e = engine.lock().unwrap();
            let r1 = e.add_radio();
            e.set_active_radio(0); // deterministic start: radio 0 active
            let p = e
                .settings()
                .radios
                .iter()
                .find(|p| p.id == r1)
                .unwrap()
                .clone();
            (r1, Transport::from_profile(&p), p.rigctld_port)
        };
        let mut state = loop_state();
        state.applied = cat_transport(4532, None);
        let pool: MonitorPool = Arc::new(Mutex::new(vec![MonitorConn {
            id: r1,
            transport: r1_transport,
            rig: Rig::with_control(Some(format!("127.0.0.1:{r1_port}")), PttMode::Vox),
            rigctld_proc: None,
            last_poll: 0.0,
            ticks: 0,
            smeter_supported: None,
            freq_misses: 0,
            open_failures: 0,
            retry_after_ms: 0.0,
        }]));
        (engine, pool, state, r1, r1_port)
    }

    #[test]
    fn deferred_handoff_never_claims_applied_and_the_fallback_still_rebuilds() {
        // THE 2026-07-11 on-rig regression ("pill says Icom, CAT still controls the Yaesu"):
        // while a handoff is DEFERRED (pool contended), a step() tick must not stamp
        // `applied = want` — that poisons rig_differs, so when the handoff later lands in the
        // FALLBACK branch (reconcile closed the new radio's conn first) the promised fresh
        // rebuild never fires and the loop drives the OLD radio with the NEW radio's settings
        // until the operator switches again.
        let (engine, pool, mut state, r1, r1_port) = switch_scene();
        let mut rig = Rig::vox();
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        let mut backend = MockBackend::new();
        let (sinks, mut ra) = (no_sinks(), mock_reopen_audio());
        let mut station = StationSinks::new();
        let calls = std::cell::Cell::new(0u32);
        let captured_port = std::cell::Cell::new(0u16);
        let mut rr = |t: &Transport, _c: bool| {
            calls.set(calls.get() + 1);
            captured_port.set(t.rigctld_port);
            (Rig::vox(), None, CatProbe::status(None, ""))
        };

        // Act A: the switch lands while the monitor thread holds the pool → deferred.
        let guard = pool.lock().unwrap();
        engine.lock().unwrap().set_active_radio(r1);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert!(state.handoff_deferred, "contended pool → handoff deferred");
        assert_eq!(last_active, 0, "switch not yet completed");

        // Act B: one deferred tick. The transport claim must NOT happen.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                1.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert_eq!(
            state.applied.rigctld_port, 4532,
            "a deferred tick must not claim the new radio's transport (the poison)"
        );

        // Act C: reconcile won the race and closed the new radio's conn → fallback path.
        drop(guard);
        pool.lock().unwrap().clear();
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert_eq!(last_active, r1, "fallback completed the switch intent");
        assert!(
            !state.handoff_deferred,
            "completed handoff clears the deferral"
        );

        // Act D: the fallback's contract — step()'s rig_differs opens the new radio FRESH.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                2.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert_eq!(calls.get(), 1, "the fallback rebuild fired within one tick");
        assert_eq!(
            captured_port.get(),
            r1_port,
            "…and it opened the NEW radio's transport"
        );
    }

    #[test]
    fn handoff_deferred_never_survives_early_return_or_completion() {
        // The deferral only ever protects the switch currently in flight: if the switch intent
        // vanishes (operator flips back / band-routing bounces) the guard must vanish with it,
        // or step() skips every future rig_differs rebuild forever.
        let (engine, pool, mut state, r1, _r1_port) = switch_scene();
        let mut rig = Rig::vox();
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);

        // Defer a switch to r1…
        let guard = pool.lock().unwrap();
        engine.lock().unwrap().set_active_radio(r1);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert!(state.handoff_deferred);
        // …then the intent vanishes before the handoff ever wins the lock.
        engine.lock().unwrap().set_active_radio(0);
        drop(guard);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert!(
            !state.handoff_deferred,
            "a vanished switch intent must drop the deferral guard"
        );

        // And a COMPLETED handoff clears it too (pins the happy path).
        engine.lock().unwrap().set_active_radio(r1);
        let guard = pool.lock().unwrap();
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert!(state.handoff_deferred);
        drop(guard);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert_eq!(last_active, r1, "adopt completed");
        assert!(
            !state.handoff_deferred,
            "completed adopt clears the deferral"
        );
    }

    #[test]
    fn handoff_refuses_a_conn_with_a_dead_daemon_and_reopens_fresh() {
        // A monitor conn can hold a live TCP control channel over a DEAD CivDaemon (the 9700's
        // flapping daemon, between reconcile passes). Adopting that zombie installs dead CAT as
        // the active radio with `applied` matching — rig_differs would never rebuild it. The
        // adopt gate must mirror reconcile's is_alive keep-gate and fall through to the
        // fallback, whose fresh-open self-heals.
        use crate::civ::engine::tests_support::FakeRadio;
        let (engine, pool, mut state, r1, _r1_port) = switch_scene();
        // A real native daemon over an in-memory radio whose I/O fails hard → engine exits.
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let (mut radio, _push) = FakeRadio::new(0xA2);
        radio.dead = true;
        let daemon = crate::civ::broker::CivDaemon::start_with_io(Box::new(radio), 0xA2, port)
            .expect("daemon starts (TCP binds) even though the radio I/O is dead");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut cat = CatDaemon::Native(daemon);
        while cat.is_alive() {
            assert!(
                std::time::Instant::now() < deadline,
                "dead-radio engine should exit within 2 s"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        {
            // Swap the scene's live conn for the zombie shape: control-bearing rig, dead daemon.
            let mut p = pool.lock().unwrap();
            p[0].rig = Rig::with_control(Some(format!("127.0.0.1:{port}")), PttMode::Vox);
            p[0].rigctld_proc = Some(cat);
        }
        let mut rig = Rig::vox();
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        engine.lock().unwrap().set_active_radio(r1);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );

        assert_eq!(last_active, r1, "fallback completed the switch intent");
        assert_eq!(
            state.applied.rigctld_port, 4532,
            "applied unchanged → step()'s rig_differs reopens the new radio FRESH"
        );
        assert!(
            pool.lock().unwrap().is_empty(),
            "the zombie conn was dropped (daemon reaped), not adopted"
        );
    }

    #[test]
    fn reconcile_never_closes_the_new_actives_conn_mid_switch() {
        // Right after a switch the new active leaves reconcile's want-list, but its conn is
        // exactly what the handoff adopts for the instant switch. Reconcile must leave it
        // alone (the handoff's fallback drops it if stale — nothing leaks).
        let (engine, pool, _state, r1, _r1_port) = switch_scene();
        // Post-switch view: r1 is now active → want excludes it.
        reconcile_pool(&pool, &[], r1, &engine, 0.0);
        assert_eq!(
            pool.lock().unwrap().len(),
            1,
            "the new active's conn survives for the handoff to adopt"
        );
        // …but once some OTHER radio is active and r1 is genuinely unwanted, it IS closed.
        reconcile_pool(&pool, &[], 0, &engine, 0.0);
        assert!(
            pool.lock().unwrap().is_empty(),
            "an unwanted non-active conn is still reaped as before"
        );
    }

    #[test]
    fn contended_switch_never_commands_the_old_rig_with_the_new_radios_settings() {
        // While a switch is pending (deferred), the OLD rig must receive NO retune — the
        // regression's literal symptom was the FTDX10 being driven with the 9700's dial — and
        // the switch-unkey must run ONCE per switch intent, not once per 20 ms retry tick.
        let (engine, pool, mut state, r1, r1_port) = switch_scene();
        let (stub_addr, seen) = recording_rigctld_stub();
        let mut rig = Rig::with_control(Some(stub_addr), PttMode::Cat);
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        let mut backend = MockBackend::new();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // Five deferred retry ticks with the pool held.
        let guard = pool.lock().unwrap();
        engine.lock().unwrap().set_active_radio(r1);
        for i in 0..5 {
            handoff_if_switched(
                &engine,
                &pool,
                &mut rig,
                &mut state,
                &mut last_active,
                &pending,
            );
            assert!(state.handoff_deferred);
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    i as f64,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
        {
            let lines = seen.lock().unwrap();
            assert!(
                !lines
                    .iter()
                    .any(|l| l.starts_with("F ") || l.starts_with("M ")),
                "old rig retuned/re-moded during the deferral: {lines:?}"
            );
            assert_eq!(
                lines.iter().filter(|l| l.as_str() == "T 0").count(),
                1,
                "exactly ONE switch-unkey per switch intent: {lines:?}"
            );
        }

        // Release the pool → the adopt lands within a tick.
        drop(guard);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert_eq!(last_active, r1, "adopt landed once the pool freed");
        assert_eq!(state.applied.rigctld_port, r1_port);
        assert!(!state.handoff_deferred);
    }

    #[test]
    fn ptt_mode_for_maps_the_transport_ptt_method() {
        // The adopted-radio PTT fix depends on this mapping mirroring open_rig's dispatch: a monitor is
        // opened Vox (read-only), and on adopt it MUST regain the profile's real keying or ptt() no-ops.
        let mut t = cat_transport(4532, None);
        t.ptt_method = "cat".into();
        t.rig_model = 1042;
        assert_eq!(ptt_mode_for(&t), PttMode::Cat);

        t.rig_model = 0; // CAT selected but no model → can't key via CAT → Vox
        assert_eq!(ptt_mode_for(&t), PttMode::Vox);

        t.serial_port = "/dev/ttyUSB0".into();
        t.ptt_method = "rts".into();
        assert_eq!(
            ptt_mode_for(&t),
            PttMode::Serial {
                port: "/dev/ttyUSB0".into(),
                line: SerialLine::Rts,
            }
        );

        t.ptt_method = "dtr".into();
        assert_eq!(
            ptt_mode_for(&t),
            PttMode::Serial {
                port: "/dev/ttyUSB0".into(),
                line: SerialLine::Dtr,
            }
        );

        t.ptt_method = "vox".into();
        assert_eq!(ptt_mode_for(&t), PttMode::Vox);
    }

    /// Digirig Mobile and every other single-cable interface: ONE port carries CAT and the RTS
    /// keying line. Nexus only ever detected the OPPOSITE case (a dedicated keying port, e.g. an
    /// SO2R controller) and fell through to "serial keying, no CAT" — so the commonest interface
    /// in the hobby ran with NO CAT AT ALL while reporting success. rigctld now owns the port and
    /// does both, which means keying goes through the DAEMON, not our own serial line.
    #[test]
    fn shared_cat_and_keying_port_keys_through_the_daemon() {
        let mut t = cat_transport(4532, None);
        t.rig_model = 3073;
        t.serial_port = "COM5".into();
        t.ptt_serial_port = String::new(); // blank ⇒ ptt_port() falls back to the CAT port
        t.ptt_method = "rts".into();

        assert!(keys_on_the_cat_port(&t));
        assert_eq!(
            ptt_mode_for(&t),
            PttMode::Cat,
            "rigctld holds the port, so PttMode::Serial could not open it — on Windows that \
             fails outright and the rig tunes but never keys"
        );

        // Spelling the same port explicitly is the same case.
        t.ptt_serial_port = "com5".into(); // case-insensitive on purpose
        assert!(keys_on_the_cat_port(&t));
        assert_eq!(ptt_mode_for(&t), PttMode::Cat);
    }

    /// The boundaries. Each of these must KEEP the old behaviour, because in each the daemon
    /// either isn't there to key or has no line to key with.
    #[test]
    fn shared_port_keying_does_not_capture_the_other_ptt_shapes() {
        let mut t = cat_transport(4532, None);
        t.rig_model = 3073;
        t.serial_port = "COM5".into();
        t.ptt_method = "rts".into();

        // SO2R: a DEDICATED keying port. We key it ourselves and run CAT separately — unchanged.
        t.ptt_serial_port = "COM9".into();
        assert!(!keys_on_the_cat_port(&t));
        assert_eq!(
            ptt_mode_for(&t),
            PttMode::Serial {
                port: "COM9".into(),
                line: SerialLine::Rts,
            }
        );

        // No rig model: there is no CAT daemon at all, so keying stays ours.
        t.ptt_serial_port = String::new();
        t.rig_model = 0;
        assert!(!keys_on_the_cat_port(&t));
        assert!(matches!(ptt_mode_for(&t), PttMode::Serial { .. }));

        // Network rig: a TCP transport has no RTS line to assert.
        t.rig_model = 23005;
        t.rig_conn = "network".into();
        t.rig_addr = "192.168.1.50:4992".into();
        assert!(!keys_on_the_cat_port(&t));

        // No serial device named at all.
        t.rig_conn = "serial".into();
        t.rig_addr = String::new();
        t.serial_port = String::new();
        assert!(!keys_on_the_cat_port(&t));

        // CAT and VOX keying are untouched by any of this.
        t.serial_port = "COM5".into();
        t.ptt_method = "cat".into();
        assert!(!keys_on_the_cat_port(&t));
        t.ptt_method = "vox".into();
        assert!(!keys_on_the_cat_port(&t));
    }

    #[test]
    fn handoff_gives_the_adopted_radio_its_real_ptt_mode() {
        // Bug: TX dead on the FTDX10 after switching to it (freq/mode still work). The monitor opens
        // every non-active radio Vox (read-only); the handoff installs that Vox rig as the active radio,
        // so `ptt()` silently no-ops. The adopt must give the adopted rig the profile's REAL keying
        // (Cat) AND demote the outgoing rig to Vox (a monitor must never key).
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let (r1, r1_transport, r1_port) = {
            let mut e = engine.lock().unwrap();
            let r1 = e.add_radio(); // active becomes r1 (add_radio switches to the new radio)
                                    // Configure r1 (now the active/form radio) as a real CAT rig via the public settings path.
            let mut s = e.settings().clone();
            s.ptt_method = "cat".into();
            s.rig_model = 1042; // FTDX10 — a real model, so ptt_mode_for → Cat
            e.apply_settings(s);
            let p = e
                .settings()
                .radios
                .iter()
                .find(|p| p.id == r1)
                .unwrap()
                .clone();
            (r1, Transport::from_profile(&p), p.rigctld_port)
        };
        let mut state = loop_state();
        state.applied = cat_transport(4532, None); // radio 0 (the OUTGOING active) on its port
                                                   // Radio 0 is a live CAT rig — after the swap it must be DEMOTED to Vox in the pool.
        let mut rig = Rig::with_control(Some("127.0.0.1:4532".to_string()), PttMode::Cat);
        let pool: MonitorPool = Arc::new(Mutex::new(vec![MonitorConn {
            id: r1,
            transport: r1_transport,
            rig: Rig::with_control(Some(format!("127.0.0.1:{r1_port}")), PttMode::Vox),
            rigctld_proc: None,
            last_poll: 0.0,
            ticks: 0,
            smeter_supported: None,
            freq_misses: 0,
            open_failures: 0,
            retry_after_ms: 0.0,
        }]));
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );

        assert_eq!(last_active, r1, "switched to radio 1");
        assert_eq!(
            rig.ptt_mode(),
            &PttMode::Cat,
            "the adopted FTDX10 regains CAT keying (was Vox as a monitor) — else TX is dead"
        );
        let p = pool.lock().unwrap();
        assert_eq!(p[0].id, 0, "old active demoted into the pool");
        assert_eq!(
            p[0].rig.ptt_mode(),
            &PttMode::Vox,
            "the demoted radio can never key while it's a read-only monitor"
        );
    }

    #[test]
    fn handoff_skips_a_dead_conn_and_reopens_fresh() {
        // The IC-9700 CAT-dead bug: a monitor conn whose rigctld failed to bind is parked as a
        // control-less `Rig::vox()`. Adopting it would install a dead rig as the active radio AND
        // (because applied becomes its transport) step()'s rig_differs would never rebuild it → CAT
        // permanently dead. The handoff must REJECT a dead conn and fall through to the fresh-open path.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let (r1, r1_transport) = {
            let mut e = engine.lock().unwrap();
            let r1 = e.add_radio();
            let p = e
                .settings()
                .radios
                .iter()
                .find(|p| p.id == r1)
                .unwrap()
                .clone();
            (r1, Transport::from_profile(&p))
        };
        let mut state = loop_state();
        state.applied = cat_transport(4532, None); // radio 0 (active) on its port
        let mut rig = Rig::vox();
        // Radio 1's monitor conn is DEAD: a `Rig::vox()` with no control channel + no daemon.
        let pool: MonitorPool = Arc::new(Mutex::new(vec![MonitorConn {
            id: r1,
            transport: r1_transport,
            rig: Rig::vox(),
            rigctld_proc: None,
            last_poll: 0.0,
            ticks: 0,
            smeter_supported: None,
            freq_misses: 0,
            open_failures: 0,
            retry_after_ms: 0.0,
        }]));
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        engine.lock().unwrap().set_active_radio(r1);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );

        assert_eq!(
            last_active, r1,
            "still tracks the switch (doesn't spin every tick)"
        );
        assert!(
            state.force_audio_rebuild,
            "fallback forces the RX audio to rebuild to the new radio's device"
        );
        assert_eq!(
            state.applied.rigctld_port, 4532,
            "applied UNCHANGED → step()'s rig_differs opens radio 1 FRESH via open_cat (self-heal)"
        );
        let p = pool.lock().unwrap();
        assert!(
            !p.iter().any(|c| c.id == r1),
            "the dead conn is dropped so its (stale) daemon is reaped + the id can reopen clean"
        );
    }

    #[test]
    fn handoff_unkeys_a_keyed_outgoing_rig() {
        // TX-safety: if the operator switches radios mid-transmission, the OUTGOING rig must be
        // unkeyed before it goes into the read-only monitor pool — else it's a stuck carrier.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let (r1, r1_transport) = {
            let mut e = engine.lock().unwrap();
            let r1 = e.add_radio();
            let p = e
                .settings()
                .radios
                .iter()
                .find(|p| p.id == r1)
                .unwrap()
                .clone();
            (r1, Transport::from_profile(&p))
        };
        let mut state = loop_state();
        state.applied = cat_transport(4532, None);
        // Mid-TX on the active radio (a slot over in flight + manual PTT held).
        state.tx_until_ms = Some(now_unix_ms() + 5000.0);
        state.manual_ptt_applied = true;
        let mut rig = Rig::vox();
        let pool: MonitorPool = Arc::new(Mutex::new(vec![MonitorConn {
            id: r1,
            rig: Rig::with_control(
                Some(format!("127.0.0.1:{}", r1_transport.rigctld_port)),
                PttMode::Vox,
            ),
            transport: r1_transport,
            rigctld_proc: None,
            last_poll: 0.0,
            ticks: 0,
            smeter_supported: None,
            freq_misses: 0,
            open_failures: 0,
            retry_after_ms: 0.0,
        }]));
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        engine.lock().unwrap().set_active_radio(r1);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert!(
            state.tx_until_ms.is_none(),
            "slot-TX state cleared → no stuck carrier in the pool"
        );
        assert!(!state.manual_ptt_applied, "manual PTT cleared on handoff");
        assert!(!state.tuning_keyed);
        assert_eq!(last_active, r1, "still completed the switch");
    }

    #[test]
    fn handoff_falls_back_when_new_active_not_in_pool() {
        // If the new active radio has no live monitor conn (never opened), the handoff is a no-op on
        // the pool (leaves the fresh-open to step()'s rig_differs path) but still tracks last_active
        // so it doesn't spin every tick.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let r1 = {
            let mut e = engine.lock().unwrap();
            e.add_radio()
        };
        let mut state = loop_state();
        state.applied = cat_transport(4532, None);
        let mut rig = Rig::vox();
        let pool: MonitorPool = Arc::new(Mutex::new(Vec::new())); // empty pool
        let mut last_active = 0u32;
        let pending = std::sync::atomic::AtomicBool::new(false);
        engine.lock().unwrap().set_active_radio(r1);
        handoff_if_switched(
            &engine,
            &pool,
            &mut rig,
            &mut state,
            &mut last_active,
            &pending,
        );
        assert_eq!(
            last_active, r1,
            "tracked the switch even with no pool conn (fallback to rebuild)"
        );
        assert_eq!(
            state.applied.rigctld_port, 4532,
            "applied unchanged → step()'s rig_differs opens it fresh"
        );
        assert!(pool.lock().unwrap().is_empty());
    }

    #[test]
    fn step_keys_ptt_and_plays_on_a_tx_slot() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        engine.lock().unwrap().broadcast("CQ TEST W9XYZ EN37");
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // now = 0 → slot 0 (even); a tx_parity-0 engine transmits there.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(rig.keyed, "PTT keyed on the TX slot");
        assert!(state.tx_until_ms.is_some(), "TX hold deadline set");
        assert!(!backend.played.is_empty(), "TX audio played to the backend");
    }

    /// The FT8 late-TX fix (operator-approved key-at-boundary, 2026-07-21): when the
    /// just-ended RX slot's EARLY decode has already folded, the boundary step keys
    /// PTT and plays the TX audio ON THAT SAME TICK — not 1–2 s later when the
    /// straggler boundary decode drains (the old deferred ordering that made every
    /// over start late; WSJT-X keys at t=0 and decodes in parallel). And when the
    /// straggler result DOES drain, the boundary_keyed guard must make it
    /// housekeeping-only: no second key, no second wave (the double-TX guard).
    #[test]
    fn early_folded_boundary_keys_at_boundary_and_never_double_keys() {
        // parity 0 → even slots transmit; FT8 → 15 s slots (the reported mode).
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        engine.lock().unwrap().set_tier(Tier::Ft8);
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // Slot 0 boundary: nothing queued, empty ring — consumed with no TX.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                100.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(!rig.keyed, "nothing to send yet");

        // Queue an over, then strip the broadcast's immediate-TX arming so the ONLY
        // way it can key is the boundary path under test.
        {
            let mut e = engine.lock().unwrap();
            e.broadcast("CQ TEST W9XYZ EN37");
            let _ = e.take_immediate_tx();
        }
        // Slot 1 boundary: ring still empty → no decode; odd slot → not our parity.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                15_020.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(!rig.keyed, "odd slot: not our TX parity");

        // Mid slot 1: capture RX audio (so the slot-2 boundary wants a decode) and
        // mark slot 1's early pass as folded — the key-at-boundary precondition.
        backend.queue_capture(vec![0.001f32; 12_000]);
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                22_000.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(!state.rx.is_empty(), "capture landed in the ring");
        state.early_done_slot = Some(1);

        // Slot 2 boundary — THE assertion: keyed on this very tick, straggler decode
        // dispatched in parallel and still in flight.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                30_020.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(
            rig.keyed,
            "keyed AT the boundary, not after the decode drained"
        );
        assert!(
            !backend.played.is_empty(),
            "TX audio played at the boundary"
        );
        assert!(
            state.decode_in_flight,
            "straggler boundary decode running in parallel with the over"
        );
        let played_after_key = backend.played.len();

        // Let the straggler decode drain (real worker thread). Its drain must be
        // housekeeping ONLY — the played sample count must not grow.
        let mut drained = false;
        for i in 0..500 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    30_040.0 + f64::from(i),
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            if !state.decode_in_flight {
                drained = true;
                break;
            }
        }
        assert!(drained, "straggler decode drained");
        assert_eq!(
            backend.played.len(),
            played_after_key,
            "no second wave: the straggler drain is housekeeping only (double-TX guard)"
        );
    }

    #[test]
    fn step_drops_ptt_after_the_hold_deadline() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let _ = rig.ptt(true); // pretend we are mid-over
        let mut state = loop_state();
        state.tx_until_ms = Some(500.0);
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // now past the hold deadline → PTT released.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                1000.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(!rig.keyed, "PTT released after the hold deadline");
        assert!(state.tx_until_ms.is_none());
    }

    #[test]
    fn slot_clock_steers_to_utc_with_the_measured_offset() {
        // The measured PC-clock-vs-UTC offset must actually steer the slot clock
        // (not just be displayed), or TX/RX land off the UTC grid on a skewed PC.
        let now = 101_000.0; // arbitrary; FT1 SlotClock has 4 s (4000 ms) slots
        let next_ms = |offset_ms: i64| -> u64 {
            let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
            engine.lock().unwrap().set_clock_offset_ms(Some(offset_ms));
            let mut backend = MockBackend::new();
            let mut rig = Rig::vox();
            let mut state = loop_state();
            let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
            let mut station = StationSinks::new();
            // First step picks the offset up off the engine; second applies it.
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    now,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            assert_eq!(state.clock_offset_ms, offset_ms, "offset read from engine");
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    now,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            // Bind out of the tail expression so the MutexGuard temporary drops
            // before `engine` (the local) does — else the guard outlives its lock.
            let next_slot_ms = engine.lock().unwrap().snapshot().radio.next_slot_ms;
            next_slot_ms
        };
        // A 3 s clock skew shifts the next-slot countdown by 3 s (mod the 4 s slot)
        // — proof the offset reaches the slot clock, not just the UI chip.
        assert_ne!(
            next_ms(0),
            next_ms(3000),
            "clock offset must move the slot grid"
        );
    }

    #[test]
    fn stop_tx_mid_over_hard_stops_immediately() {
        // Mid-transmission (PTT keyed, hold deadline far in the future), the
        // operator hits Stop TX (engine.halt_tx → tx disabled). The next loop
        // iteration must cut it NOW: drop PTT, flush the queued audio, clear hold.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let _ = rig.ptt(true);
        let mut state = loop_state();
        state.tx_until_ms = Some(9_999_999.0); // long hold — would NOT expire on its own
        engine.lock().unwrap().halt_tx(); // operator hit Stop TX
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                100.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(!rig.keyed, "PTT dropped immediately on Stop TX");
        assert!(state.tx_until_ms.is_none(), "TX hold cleared");
        assert!(backend.flush_calls > 0, "queued TX audio was flushed");
    }

    /// A Phone-armed engine on a legal 20 m phone frequency with an SSTV image queued.
    fn sstv_ready_engine(samples: Vec<f32>) -> Arc<Mutex<Engine>> {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_frequency(14.290, "20m", "USB");
            e.set_operating_mode("phone", false);
            e.sstv_send(samples, "PD-120".to_string()).unwrap();
        }
        engine
    }

    #[test]
    fn sstv_send_keys_streams_progress_and_stop_unkeys() {
        // A ~3 s image (36 000 samples at 12 kHz) fits under the 10 s look-ahead → the
        // whole buffer streams in one tick.
        let engine = sstv_ready_engine(vec![0.2f32; 36_000]);
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // Tick 1: keys PTT for the precomputed duration and streams the image.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                1000.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(rig.keyed, "PTT keyed for the image");
        assert!(
            state.tx_until_ms.is_some(),
            "PTT held for the precomputed duration"
        );
        assert_eq!(
            backend.played.len(),
            36_000,
            "entire image streamed (fits the look-ahead window)"
        );
        {
            let e = engine.lock().unwrap();
            assert!(e.sstv_sending(), "engine marked sending");
            let (_, total) = e.sstv_tx_progress().expect("progress published");
            assert!(
                (total - 3000.0).abs() < 1.0,
                "progress total = 3 s of key-down"
            );
        }

        // Operator hits Stop mid-hold → the next tick flushes queued audio + unkeys NOW.
        engine.lock().unwrap().sstv_stop();
        let flushes_before = backend.flush_calls;
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                1500.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(!rig.keyed, "PTT dropped immediately on Stop");
        assert!(state.tx_until_ms.is_none(), "hold cleared");
        assert!(
            backend.flush_calls > flushes_before,
            "queued image audio flushed on Stop"
        );
        assert!(state.sstv_feed.is_none(), "feed dropped on Stop");
        assert!(
            !engine.lock().unwrap().sstv_sending(),
            "sending cleared on Stop"
        );
    }

    #[test]
    fn sstv_image_unkeys_at_the_precomputed_duration() {
        // The guaranteed unkey: PTT drops at the precomputed tx_until_ms even with no Stop.
        let engine = sstv_ready_engine(vec![0.2f32; 36_000]); // 3 s
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // Tick 1 keys + streams; the hold is exactly image duration + the TX tail.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                1000.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(rig.keyed);
        let hold = state.tx_until_ms.unwrap();
        assert!(
            (hold - (1000.0 + 3000.0 + crate::slot::TX_TAIL_MS)).abs() < 1.0,
            "PTT held exactly the image duration + TX tail"
        );

        // Tick 2 past the hold deadline → the guaranteed unkey fires; sending clears.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                hold + 1.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(!rig.keyed, "PTT dropped at the precomputed duration");
        assert!(state.tx_until_ms.is_none());
        assert!(state.sstv_feed.is_none(), "feed cleared on completion");
        assert!(
            !engine.lock().unwrap().sstv_sending(),
            "sending cleared on completion"
        );
    }

    #[test]
    fn step_rebuilds_the_clock_on_a_tier_change() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        engine.lock().unwrap().set_tier(Tier::Ft8);
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        assert_eq!(state.cur_tier, Tier::TempoFast);
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert_eq!(
            state.cur_tier,
            Tier::Ft8,
            "loop followed the tier switch (clock + capture ring rebuilt)"
        );
    }

    #[test]
    fn step_tunes_carrier_and_skips_the_slot() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        engine.lock().unwrap().set_tune(true);
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(rig.keyed, "tune keys a steady carrier");
        assert!(!backend.played.is_empty(), "carrier audio played");
        assert!(state.tuning_keyed);
        assert!(
            state.last_slot.is_none(),
            "slot decode skipped while tuning"
        );
    }

    fn cat_transport(rigctld_port: u16, broker_self_port: Option<u16>) -> Transport {
        Transport {
            ptt_method: "cat".to_string(),
            rig_model: 1035,
            serial_port: "/dev/ttyUSB0".to_string(),
            ptt_serial_port: String::new(),
            baud: 38400,
            icom_native_cat: false,
            rig_conn: "serial".to_string(),
            rig_addr: String::new(),
            rigctld_port,
            broker_self_port,
            audio_in: String::new(),
            audio_out: String::new(),
            voice_mic_device: String::new(),
            tx_level: 0.9,
            rx_gain: 1.0,
            monitor_enabled: false,
            monitor_device: String::new(),
            monitor_level: 0.5,
        }
    }

    #[test]
    fn ptt_port_prefers_dedicated_else_falls_back_to_cat_port() {
        // The SO2R fix: RTS/DTR keying uses a dedicated PTT COM port when set (a u2R/MK2R
        // routes keying on its own port), else the CAT serial port (prior single-port behavior).
        let mut t = cat_transport(4532, None); // serial_port = /dev/ttyUSB0, ptt_serial_port = ""
        assert_eq!(
            t.ptt_port(),
            "/dev/ttyUSB0",
            "empty dedicated port → CAT serial port"
        );
        t.ptt_serial_port = "COM16".to_string();
        assert_eq!(t.ptt_port(), "COM16", "dedicated PTT port wins");
        t.ptt_serial_port = "   ".to_string();
        assert_eq!(
            t.ptt_port(),
            "/dev/ttyUSB0",
            "whitespace-only → fall back to CAT port"
        );
        // A changed PTT port must rebuild the rig so the keying line rebinds.
        let mut t2 = cat_transport(4532, None);
        t2.ptt_serial_port = "COM16".to_string();
        assert!(
            t.rig_differs(&t2) || t2.rig_differs(&t),
            "PTT port change triggers a rig rebuild"
        );
    }

    /// Native Flex DAX RX cannot be verified on this bench — there is no Flex here. That is
    /// exactly why its FAILURE path must be testable: when the tester reports "no audio", the
    /// build has to have already told them which of the four causes it was.
    ///
    /// The trap being pinned: selecting DAX makes the loop take DAX audio INSTEAD of the sound
    /// card, so a source that never streams leaves the operator deaf with silence that looks
    /// exactly like a dead band.
    #[test]
    fn dax_that_never_streams_gives_up_and_falls_back() {
        let t0 = Instant::now();

        // Just started, nothing yet — well inside the grace window, so no complaint.
        assert!(!RadioLoop::dax_starved(Some(t0), false, t0));
        assert!(!RadioLoop::dax_starved(
            Some(t0),
            false,
            t0 + Duration::from_secs(2)
        ));

        // Past the window with nothing ever received → give up.
        assert!(RadioLoop::dax_starved(
            Some(t0),
            false,
            t0 + DAX_STARVE_AFTER
        ));
        assert!(RadioLoop::dax_starved(
            Some(t0),
            false,
            t0 + Duration::from_secs(60)
        ));

        // A source that HAS delivered audio is proven. A quiet band, a between-slots gap, or a
        // long listening pause must never trip this — that would yank a working native feed.
        assert!(!RadioLoop::dax_starved(
            Some(t0),
            true,
            t0 + Duration::from_secs(600)
        ));

        // No DAX source selected at all: nothing to starve.
        assert!(!RadioLoop::dax_starved(None, false, t0 + DAX_STARVE_AFTER));
        assert!(!RadioLoop::dax_starved(None, true, t0 + DAX_STARVE_AFTER));
    }

    #[test]
    fn report_ptt_surfaces_a_key_nak_and_respects_error_ownership() {
        let mut state = loop_state();
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let banner =
            |e: &Arc<Mutex<Engine>>| e.lock().unwrap().snapshot().radio.audio_error.clone();

        // A keying NAK surfaces a PTT status on the shared banner; a good key clears OUR status.
        state.report_ptt(&engine, true);
        assert!(banner(&engine).is_some(), "PTT NAK shows the banner");
        assert_eq!(state.err_owner, ErrOwner::Ptt);
        state.report_ptt(&engine, false);
        assert!(
            banner(&engine).is_none(),
            "a good key clears the PTT status"
        );
        assert_eq!(state.err_owner, ErrOwner::None);

        // A PTT status must NOT clobber a higher-priority device error, and clearing PTT
        // must not wipe the device error either.
        state.err_owner = ErrOwner::Device;
        engine
            .lock()
            .unwrap()
            .set_audio_error(Some("Sound card failed".to_string()));
        state.report_ptt(&engine, true);
        assert_eq!(
            banner(&engine).as_deref(),
            Some("Sound card failed"),
            "device error wins"
        );
        state.report_ptt(&engine, false);
        assert_eq!(
            banner(&engine).as_deref(),
            Some("Sound card failed"),
            "clearing PTT leaves a device error intact"
        );
    }

    #[test]
    fn open_rig_flags_broker_port_conflict() {
        // CAT broker and the launched rigctld both on the same port → no self-connect,
        // no doomed spawn; a clear message instead. Pure (no I/O before the guard).
        let t = cat_transport(4532, Some(4532));
        let (_rig, proc, probe) = open_rig(&t, true);
        assert!(proc.is_none());
        assert_eq!(probe.ok, Some(false));
        assert!(
            probe.detail.contains("different ports"),
            "got: {}",
            probe.detail
        );
    }

    #[test]
    fn open_rig_coexists_with_an_existing_rigctld() {
        use crate::rigctld_server::RigBackend;
        struct CoexistRig(std::sync::Mutex<u64>);
        impl RigBackend for CoexistRig {
            fn freq_hz(&self) -> u64 {
                *self.0.lock().unwrap()
            }
            fn mode(&self) -> (String, u32) {
                ("USB".into(), 2700)
            }
            fn ptt(&self) -> bool {
                false
            }
            fn set_freq(&self, hz: u64) -> bool {
                *self.0.lock().unwrap() = hz;
                true
            }
            fn set_mode(&self, _m: &str, _p: u32) -> bool {
                true
            }
            fn set_ptt(&self, _on: bool) -> bool {
                true
            }
        }

        // Stand up a broker that plays the role of an already-running (foreign)
        // rigctld on some port.
        let backend: Arc<dyn RigBackend> = Arc::new(CoexistRig(std::sync::Mutex::new(14_074_000)));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || crate::rigctld_server::serve(listener, backend));

        // open_rig must SHARE it (no spawn), not fight for the serial port.
        let t = cat_transport(port, None);
        let (_rig, proc, probe) = open_rig(&t, true);
        let (ok, detail) = (probe.ok, probe.detail);
        assert!(
            proc.is_none(),
            "shared the existing rigctld — did not spawn one"
        );
        assert_eq!(ok, Some(true), "connected through it: {detail}");
        assert!(detail.contains("Sharing"), "got: {detail}");
    }

    /// Shared recording backend for the read-only-launch tests: a stand-in rig that
    /// logs every COMMAND (set_freq/set_mode/set_ptt) in order while serving reads
    /// from fixed state ("the rig was left on 40 m LSB last night").
    struct RecordingRig {
        log: Arc<Mutex<Vec<String>>>,
    }
    impl crate::rigctld_server::RigBackend for RecordingRig {
        fn freq_hz(&self) -> u64 {
            7_200_000 // 40 m — NOT the app's persisted 20 m dial
        }
        fn mode(&self) -> (String, u32) {
            ("LSB".into(), 2400)
        }
        fn ptt(&self) -> bool {
            false
        }
        fn set_freq(&self, hz: u64) -> bool {
            self.log.lock().unwrap().push(format!("F {hz}"));
            true
        }
        fn set_mode(&self, m: &str, _p: u32) -> bool {
            self.log.lock().unwrap().push(format!("M {m}"));
            true
        }
        fn set_ptt(&self, on: bool) -> bool {
            self.log.lock().unwrap().push(format!("T {}", u8::from(on)));
            true
        }
    }

    fn recording_backend() -> (u16, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let backend: Arc<dyn crate::rigctld_server::RigBackend> =
            Arc::new(RecordingRig { log: log.clone() });
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || crate::rigctld_server::serve(listener, backend));
        (port, log)
    }

    /// Read-only launch #2: the open reports the RIG's own dial/mode (the seed), not
    /// the app's persisted values — the whole point of read-and-display.
    #[test]
    fn open_reports_the_rigs_own_dial_and_mode() {
        let (port, _log) = recording_backend();
        let t = cat_transport(port, None);
        // The app's persisted dial is 20 m; the rig sits on 40 m LSB.
        let (_rig, _proc, probe) = open_rig(&t, true);
        assert_eq!(probe.ok, Some(true), "{}", probe.detail);
        assert_eq!(
            probe.freq_hz,
            Some(7_200_000),
            "the seed is the rig's own frequency, not the argument"
        );
        assert_eq!(probe.mode.as_deref(), Some("LSB"), "and the rig's own mode");
    }

    /// Read-only launch #1 (THE flip test): opening the rig performs NO commands —
    /// no set_freq, no set_mode — while still READING (the probe succeeded above).
    #[test]
    fn launch_never_commands_the_rig() {
        let (port, log) = recording_backend();
        let t = cat_transport(port, None);
        let (_rig, _proc, probe) = open_rig(&t, true);
        assert_eq!(probe.ok, Some(true), "{}", probe.detail);
        let lines = log.lock().unwrap().clone();
        assert!(
            !lines
                .iter()
                .any(|l| l.starts_with("F ") || l.starts_with("M ")),
            "read-only launch: the open must not command freq/mode — commands seen: {lines:?}"
        );
    }

    /// Read-only launch #6: a serial-PTT rig sharing the CAT port has NO control
    /// channel — cat_ok may read true (the PTT line opened) but there is no read, so
    /// the probe must carry no freq/mode and the engine stays rig-unconfirmed.
    #[test]
    fn serial_ptt_probe_carries_no_read_so_rig_stays_unconfirmed() {
        let mut t = cat_transport(0, None);
        t.ptt_method = "rts".to_string();
        t.serial_port = String::new(); // shared/empty → pure serial keying, no CAT
        let (_rig, _proc, probe) = open_rig(&t, true);
        assert!(
            probe.freq_hz.is_none() && probe.mode.is_none(),
            "no control channel ⇒ no read ⇒ nothing to confirm"
        );
    }

    /// Read-only launch #3/#4 (the latch): with the mode never asserted this session,
    /// ensure_commanded pushes dial+mode ONCE; the second call is a no-op.
    #[test]
    fn latch_asserts_mode_once_before_keying() {
        let (port, log) = recording_backend();
        let mut rig = Rig::with_control(Some(format!("127.0.0.1:{port}")), PttMode::Cat);
        let mut state = loop_state();
        state.rig_asserted = false;
        state.cur_dial = 14_074_000;
        state.cur_md = "PKTUSB".to_string();
        state.last_dial = 0;
        state.ensure_commanded(&mut rig);
        state.ensure_commanded(&mut rig); // second call: latched, no-op
        let lines = log.lock().unwrap().clone();
        let m_count = lines.iter().filter(|l| l.starts_with("M ")).count();
        assert_eq!(m_count, 1, "exactly one mode assert: {lines:?}");
        assert!(
            lines.iter().any(|l| l == "F 14074000"),
            "the dial was asserted too: {lines:?}"
        );
        assert!(state.rig_asserted, "latched after the successful assert");
    }

    #[test]
    fn step_reopens_rig_when_settings_change() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        engine.lock().unwrap().apply_settings(Settings {
            ptt_method: "cat".to_string(),
            rig_model: 1035,
            ..Settings::default()
        });
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state(); // applied = defaults (vox / model 0)
        let sinks = no_sinks();
        let mut station = StationSinks::new();
        let reopened = std::cell::Cell::new(false);
        let mut ra = mock_reopen_audio();
        let mut rr = |_t: &Transport, _c: bool| {
            reopened.set(true);
            (Rig::vox(), None, CatProbe::status(None, "test"))
        };

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(
            reopened.get(),
            "a rig-affecting Settings change triggers reopen_rig"
        );
    }

    // ---- the 0.24.3 sat-pick hang: engine-mutex self-deadlock in the split apply ----
    //
    // Field report (IC-9700 on the native CI-V daemon, Windows Event 1002
    // Application Hang, 100 %-reproducible): Satellites → pick a bird → click a
    // transponder froze the whole window. Root cause: the split-apply's
    // per-mapping branch took the engine lock in a `match` SCRUTINEE — under
    // edition 2021 that temporary lives to the END of the match, so the arms'
    // re-locks (`rig_split_applied` / `split_rejected` /
    // `sat_tx_mode_for_split`) deadlocked the loop thread on itself while it
    // HELD the engine mutex, and every Tauri command queued behind it forever.
    // These tests run the real pick through one `RadioLoop::step` against the
    // native daemon under a watchdog: pre-fix the step never returns (the
    // watchdog fails the test in bounded time); post-fix it returns in
    // milliseconds AND the 9700 satellite tune actually lands on the wire.

    use crate::civ::broker::CivDaemon;
    use crate::civ::engine::tests_support::{FakeRadio, Regs};

    /// The native CI-V daemon over a fake IC-9700, plus a Rig whose CAT control
    /// channel points at it. A REAL control channel is load-bearing here:
    /// `Rig::vox()` answers every verb `Ok` without a byte on the wire and
    /// fails `has_control()`, which skips the `rig_split_applied` re-lock —
    /// exactly the shape that let the deadlock ship untested.
    fn civ_daemon_rig(mute: bool) -> (CivDaemon, Rig, Arc<Mutex<Regs>>) {
        // Race-free enough for tests: bind :0 to learn a free port, drop, rebind.
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let (mut radio, _push) = FakeRadio::new(0xA2);
        radio.mute = mute;
        let regs = radio.regs();
        let d = CivDaemon::start_with_io(Box::new(radio), 0xA2, port).unwrap();
        (d, Rig::rigctld(&format!("127.0.0.1:{port}")), regs)
    }

    /// An engine that has just performed THE pick: RS-44 held, Doppler on,
    /// Main = downlink / Sub = uplink — `sat_tune_nominal` arms the same
    /// one-shots (`take_immediate_retune` + `take_split_request`) the Tauri
    /// `set_sat_transponder` command arms, so the step under test consumes the
    /// field-reproduced state, not a synthetic one.
    fn sat_pick_engine() -> Arc<Mutex<Engine>> {
        let tp = tempo_core::doppler::Transponder {
            uplink_centre_hz: 145_965_000,
            downlink_centre_hz: 435_640_000,
            invert: true,
            half_width_hz: 30_000,
        };
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut eng = engine.lock().unwrap();
            let mut s = eng.settings().clone();
            s.sat_doppler = true;
            s.sat_vfo_map = tempo_app::settings::SatVfoMap::MainDownSubUp;
            eng.apply_settings(s);
            eng.set_sat_transponder(Some(("RS-44|linear".into(), 0, tp)));
            eng.sat_tune_nominal(false, 1_000_000);
        }
        engine
    }

    /// One `RadioLoop::step` on its own thread, gated by a watchdog. A
    /// deadlocked step cannot fail an assertion — it never returns — so the
    /// only way to demonstrate the wedge in bounded time is to time out
    /// waiting for it. On timeout the wedged thread is deliberately leaked
    /// (it holds the engine mutex forever; joining it would hang the suite).
    fn step_with_watchdog(engine: &Arc<Mutex<Engine>>, rig: Rig, watchdog: Duration) {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let eng = Arc::clone(engine);
        std::thread::Builder::new()
            .name("sat-pick-step".into())
            .spawn(move || {
                let mut rig = rig;
                let mut backend = MockBackend::new();
                let mut state = loop_state();
                let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
                let mut station = StationSinks::new();
                let res = state.step(
                    &eng,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    0.0,
                    &mut ra,
                    &mut rr,
                    &mut station,
                );
                let _ = done_tx.send(res);
            })
            .unwrap();
        match done_rx.recv_timeout(watchdog) {
            Ok(res) => res.unwrap(),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!(
                "RadioLoop::step wedged applying the split — the engine-mutex \
                 self-deadlock of the 0.24.3 sat-pick hang (a lock taken in a match \
                 scrutinee outlives the arms under edition 2021)"
            ),
            // A panicking step also drops `done_tx` — name that for what it is,
            // or the next person hunts a deadlock that isn't there.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!(
                "the step thread PANICKED (its own message is above) — a step bug, \
                 not the sat-pick wedge"
            ),
        }
    }

    #[test]
    fn sat_pick_split_apply_never_wedges_the_loop_and_lands_the_uplink() {
        let (_d, rig, regs) = civ_daemon_rig(false);
        let engine = sat_pick_engine();

        step_with_watchdog(&engine, rig, Duration::from_secs(10));

        // Liveness alone is not the feature — the pick must also have LANDED:
        // satellite mode engaged, downlink on Main, uplink select-written into
        // the Sub band, selection handed back to Main.
        {
            let r = regs.lock().unwrap();
            assert!(r.satmode, "satellite mode engaged (16 5A 01)");
            assert_eq!(r.main_hz, 435_640_000, "downlink on Main");
            assert_eq!(r.sub_hz, 145_965_000, "uplink in the Sub band");
            assert!(!r.sel_sub, "selection handed back to Main");
        }
        // And the binding rail reports what was DONE, from the wire acks the
        // loop itself delivered (`rig_dial_applied` / `rig_split_applied`).
        let eng = engine.lock().unwrap();
        let b = eng.sat_binding().expect("the pick left a binding");
        assert_eq!(
            b.downlink_mhz.map(|m| (m * 1e6).round() as u64),
            Some(435_640_000),
            "downlink confirmed by the rig's ack"
        );
        assert_eq!(
            b.uplink_mhz.map(|m| (m * 1e6).round() as u64),
            Some(145_965_000),
            "uplink confirmed by the rig's ack"
        );
    }

    #[test]
    fn terrestrial_up_split_apply_stays_live_and_rides_vfob() {
        // The same one-shot serves every pile-up "UP n" spot — pre-fix those
        // wedged identically (the scrutinee guard covered both arms), so the
        // A/B leg gets its own liveness pin.
        let (_d, rig, regs) = civ_daemon_rig(false);
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        engine.lock().unwrap().request_split(Some(14.235));

        step_with_watchdog(&engine, rig, Duration::from_secs(10));

        let r = regs.lock().unwrap();
        assert!(r.split, "0F 01 — the shipped A/B split");
        assert!(!r.satmode, "no satellite mode on a terrestrial split");
        assert_eq!(r.unselected_hz, 14_235_000, "the TX dial rides 25 01");
    }

    #[test]
    fn sat_pick_split_apply_survives_every_satmode_fault_without_wedging() {
        // The broker pins each fault's WIRE outcome; this pins the LOOP's
        // liveness through them — a refused or half-landed split must produce
        // a note, never a wedge. The generous watchdog covers the silent-rig
        // case, where every verb burns a client read deadline before failing.
        /// (name, register fault, mute) — mute is the whole-rig fault, so it
        /// has no register knob.
        type Fault = (&'static str, fn(&mut Regs), bool);
        let faults: [Fault; 4] = [
            ("nak_main_select", |r| r.nak_main_select = 1, false),
            ("nak_satmode_set", |r| r.nak_satmode_set = 1, false),
            ("drop_satmode_reads", |r| r.drop_satmode_reads = 1, false),
            ("silent rig", |_| {}, true),
        ];
        for (name, inject, mute) in faults {
            let (_d, rig, regs) = civ_daemon_rig(mute);
            inject(&mut regs.lock().unwrap());
            let engine = sat_pick_engine();
            step_with_watchdog(&engine, rig, Duration::from_secs(30));
            // Anchor: the step must have REACHED the split apply, or this test
            // is green while exercising nothing. A responsive-but-faulted rig
            // shows the attempt on the wire (`16 5A` — the satellite-mode
            // session's engage/verify); a mute rig logs nothing (FakeRadio
            // drops frames pre-log), so there the anchor is the engine-side
            // outcome: every verb timed out, so the apply REJECTED the split
            // and cleared the desired state. Skipping the apply would leave
            // `split_tx_mhz` holding the consumed request instead.
            if mute {
                assert!(
                    engine.lock().unwrap().split_tx_mhz().is_none(),
                    "the apply never ran — the consumed split was neither \
                     applied nor rejected ({name})"
                );
            } else {
                assert!(
                    regs.lock()
                        .unwrap()
                        .log
                        .iter()
                        .any(|(cmd, data)| *cmd == 0x16 && data.first() == Some(&0x5A)),
                    "no 16 5A frame on the wire — the split apply never ran ({name})"
                );
            }
            // The loop released the engine mutex — the app stays interactive.
            assert!(
                engine.try_lock().is_ok(),
                "engine mutex still held after the step ({name})"
            );
        }
    }

    // ---- voice-mic recording source (the pure predicate is tested in backend.rs) ----

    /// Helper: an engine with a configured voice mic and a voice-message recording started.
    fn recording_engine(voice_mic_device: &str) -> Arc<Mutex<Engine>> {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut eng = engine.lock().unwrap();
            eng.apply_settings(Settings {
                voice_mic_device: voice_mic_device.to_string(),
                ..Settings::default()
            });
            eng.start_recording();
        }
        engine
    }

    #[test]
    fn recording_with_a_voice_mic_feeds_the_recorder_from_the_mic_not_the_band() {
        let engine = recording_engine("USB Mic");
        let mut backend = MockBackend::new();
        backend.queue_capture(vec![0.9, 0.9, 0.9]); // shared input = the rig codec / the band
        backend.queue_voice_capture(vec![0.1, 0.2, 0.3]); // the operator's actual mic
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert_eq!(
            backend.voice_mic_calls,
            vec![Some("USB Mic".to_string())],
            "opened the configured mic exactly once"
        );
        assert!(state.voice_mic_open);
        let recorded = engine.lock().unwrap().stop_recording();
        assert_eq!(
            recorded,
            vec![0.1, 0.2, 0.3],
            "the recording captured the mic, never the shared band audio"
        );
    }

    #[test]
    fn audio_rebuild_mid_recording_reopens_the_mic_on_the_new_backend() {
        // Review MAJOR: swapping the backend (audio_in/out change mid-recording)
        // left voice_mic_open stale-true — the recorder then read the NEW
        // backend's nonexistent mic and captured silence for the rest of the
        // recording, with no error. The Ok arm now resets the flag so the
        // rising edge re-opens the mic on the fresh backend.
        let engine = recording_engine("USB Mic");
        let mut backend = MockBackend::new();
        backend.queue_voice_capture(vec![0.1, 0.2]);
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(state.voice_mic_open, "mic live on the first backend");

        // The operator changes the audio device mid-recording → rebuild.
        engine.lock().unwrap().apply_settings(Settings {
            voice_mic_device: "USB Mic".to_string(),
            audio_in: "Different Device".to_string(),
            ..Settings::default()
        });
        engine.lock().unwrap().start_recording(); // apply_settings reset the engine's flag? keep recording on
        let mut fresh = MockBackend::new();
        fresh.queue_voice_capture(vec![0.5, 0.6]);
        let mut ra2 = {
            let fresh = std::cell::RefCell::new(Some(fresh));
            move |_t: &Transport| -> Result<MockBackend, String> {
                Ok(fresh.borrow_mut().take().expect("one rebuild"))
            }
        };
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra2,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(
            state.voice_mic_open,
            "mic re-opened on the REBUILT backend (stale flag would fake this — check calls)"
        );
        assert_eq!(
            backend.voice_mic_calls,
            vec![Some("USB Mic".to_string())],
            "the swapped-in backend saw its own mic open (not inherited state)"
        );
        let recorded = engine.lock().unwrap().stop_recording();
        assert!(
            !recorded.is_empty(),
            "recording keeps receiving real audio across the rebuild — never silence"
        );
    }

    #[test]
    fn recording_without_a_voice_mic_records_from_the_shared_input() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        engine.lock().unwrap().start_recording(); // no voice_mic_device configured
        let mut backend = MockBackend::new();
        backend.queue_capture(vec![0.5, 0.6]);
        backend.queue_voice_capture(vec![0.1]); // must be ignored — no mic stream
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(
            backend.voice_mic_calls.is_empty(),
            "no configured mic → never opens a second input stream"
        );
        assert!(!state.voice_mic_open);
        assert_eq!(engine.lock().unwrap().stop_recording(), vec![0.5, 0.6]);
    }

    #[test]
    fn voice_mic_open_failure_falls_back_to_the_shared_input_and_surfaces_it() {
        let engine = recording_engine("Missing Mic");
        let mut backend = MockBackend::new();
        backend.voice_mic_fail = true; // the configured mic can't open
        backend.queue_capture(vec![0.9, 0.8, 0.7]); // the shared input (the fallback)
        backend.queue_voice_capture(vec![0.1, 0.2]); // must NOT be used (mic never opened)
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(!state.voice_mic_open, "a failed open is never marked live");
        assert!(
            state.voice_mic_failed,
            "the failure is latched, which also suppresses a per-loop reopen storm"
        );
        assert!(
            matches!(state.err_owner, super::ErrOwner::VoiceMic),
            "the surfaced notice is owned by the voice-mic writer"
        );
        let recorded = engine.lock().unwrap().stop_recording();
        assert_eq!(
            recorded,
            vec![0.9, 0.8, 0.7],
            "a failed mic falls back to the shared input — never records silence"
        );
        let err = engine.lock().unwrap().snapshot().radio.audio_error;
        assert!(
            err.as_deref()
                .unwrap_or("")
                .contains("Voice mic could not open"),
            "the failure is surfaced on the audio-status line, got {err:?}"
        );
    }

    #[test]
    fn stopping_a_recording_closes_the_voice_mic_stream() {
        let engine = recording_engine("USB Mic");
        let mut backend = MockBackend::new();
        backend.queue_capture(vec![0.9]);
        backend.queue_voice_capture(vec![0.1]);
        backend.queue_capture(vec![0.9]); // second step's shared frame
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // Step 1: recording in progress → the mic opens.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(state.voice_mic_open);

        // Operator stops recording; the next step tears the mic stream down.
        let _ = engine.lock().unwrap().stop_recording();
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                20.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(
            !state.voice_mic_open,
            "the mic stream closed once recording ended"
        );
        assert_eq!(
            backend.voice_mic_calls,
            vec![Some("USB Mic".to_string()), None],
            "opened on the rising edge, closed on the falling edge"
        );
    }

    #[test]
    fn audio_rebuild_mid_over_cuts_the_over_instead_of_holding_a_dead_carrier() {
        // Mid-transmission (PTT keyed, hold deadline far in the future) the operator
        // changes the audio device and saves. The backend rebuild discards the
        // queued modem samples; if it left PTT keyed with tx_until_ms still set, the
        // rig would hold a DEAD unmodulated carrier for the rest of the over while
        // the sequencer counted it as sent. The rebuild must end the over cleanly
        // first: unkey and clear the hold. (Mirrors the rig-rebuild path.)
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let _ = rig.ptt(true); // pretend we are mid-over
        let mut state = loop_state();
        state.tx_until_ms = Some(9_999_999.0); // long hold — would NOT expire on its own

        // The operator picks a different output device → audio_differs → rebuild.
        // (Rig fields stay at the defaults, so this is an audio-only change and does
        // NOT go down the already-guarded rig-rebuild path.)
        engine.lock().unwrap().apply_settings(Settings {
            audio_out: "Different Speakers".to_string(),
            ..Settings::default()
        });
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                100.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        assert!(
            !rig.keyed,
            "the over was cut before the backend swap — no keyed dead carrier"
        );
        assert!(
            state.tx_until_ms.is_none(),
            "the TX hold was cleared so the loop no longer thinks it's transmitting"
        );
    }

    #[test]
    fn poll_read_freq_failure_trips_the_cat_circuit_breaker() {
        // A half-open CAT link (writes succeed, replies never arrive) makes every
        // read_freq block to the deadline and error. Without a runtime trip the poll
        // guard (cat_ok != Some(false)) never fires and the slot loop blocks every
        // cycle, keying overs seconds late. Consecutive read_freq failures on a REAL
        // CAT rig must set cat_ok = Some(false) so the guard disables further blocking
        // polls until a successful command / reprobe — but a SINGLE miss is tolerated
        // (one slow reply cut off by the short serial deadline must not kill read-back).
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        // A CAT rig pointed at a definitely-closed port: has_control() is true but
        // every command errors (connection refused) — standing in for a mute link.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_port = listener.local_addr().unwrap().port();
        drop(listener); // free the port so a connect is refused
        let mut rig = Rig::rigctld(&format!("127.0.0.1:{dead_port}"));
        let mut state = loop_state();
        assert_ne!(
            state.cat_ok,
            Some(false),
            "precondition: the breaker has not tripped yet"
        );
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut poll_once = |state: &mut RadioLoop, backend: &mut MockBackend, rig: &mut Rig| {
            state.last_rig_poll = -1000.0; // force the heavy read-back poll due (at now = 0)
            state
                .step(
                    &engine,
                    backend,
                    rig,
                    &sinks,
                    0.0,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        };

        // One miss: tolerated (the breaker rides out a single slow/failed reply).
        poll_once(&mut state, &mut backend, &mut rig);
        assert_ne!(
            state.cat_ok,
            Some(false),
            "a single dial-read miss is tolerated, not tripped"
        );

        // FREQ_MISS_LIMIT consecutive misses: the breaker trips.
        for _ in 1..FREQ_MISS_LIMIT {
            poll_once(&mut state, &mut backend, &mut rig);
        }
        assert_eq!(
            state.cat_ok,
            Some(false),
            "consecutive dial-read misses trip the breaker so the loop stops blocking \
             on a dead read every cycle"
        );
    }

    #[test]
    fn mode_retry_ladder_tries_passband0_then_falls_back_to_plain_usb() {
        // A rig whose CAT actively refuses the DATA submode (RPRT -1 to every `M PKT*`,
        // the IC-7610-report shape). The bounded retry must walk the resilience ladder:
        //   rung 1: `M PKTUSB 3000` (the full DATA passband),
        //   rung 2: `M PKTUSB 0` (filter-agnostic) past MODE_SET_PASSBAND0_AFTER fails,
        //   rung 3: at the budget, ONE plain `M USB -1` — landing the operator a single
        //           front-panel DATA press from working, not on a dead-end note.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let (addr, log) = mock_pkt_rejecting_rigctld();
        let mut rig = Rig::rigctld(&addr);
        let mut state = loop_state();
        // The default section mode is PKTUSB (Digital); make it pending vs last_mode.
        state.last_mode = String::new();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        for i in 0..MODE_SET_MAX_TRIES {
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    f64::from(i),
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }

        let cmds = log.lock().unwrap().clone();
        let modes: Vec<&String> = cmds.iter().filter(|c| c.starts_with("M ")).collect();
        assert_eq!(
            modes.first().map(|s| s.as_str()),
            Some("M PKTUSB 3000"),
            "rung 1 — the normal DATA passband: {modes:?}"
        );
        assert!(
            modes.iter().any(|c| c.as_str() == "M PKTUSB 0"),
            "rung 2 — the filter-agnostic retry was sent: {modes:?}"
        );
        assert_eq!(
            modes.last().map(|s| s.as_str()),
            Some("M USB -1"),
            "rung 3 — exactly one plain-sideband fallback, filter untouched: {modes:?}"
        );
        assert_eq!(
            modes.iter().filter(|c| c.as_str() == "M USB -1").count(),
            1,
            "the fallback is sent ONCE (no CAT spam): {modes:?}"
        );
        assert_eq!(
            state.mode_giveup.as_deref(),
            Some("PKTUSB"),
            "PKTUSB is given up — no further retries until the target mode changes"
        );
        assert_eq!(
            state.last_mode, "USB",
            "last_mode tracks what was actually applied (the fallback)"
        );
    }

    // ---- APRS Tune vs a running FT8 session (operator report, 0.21.1) ----
    //
    // "I clicked APRS Tune while FT8 was running and the radio did not move. No error." The
    // engine-level test `aprs_tune_switches_to_the_2m_radio_like_every_other_qsy` passes, so the
    // gap is specifically the FT-ACTIVE state it does not model. Drive the REAL loop against a
    // recording rigctld and watch what actually reaches the rig.

    /// Freqs (Hz) the rig was commanded, in order, from a recording rigctld's log.
    fn commanded_freqs(log: &Arc<Mutex<Vec<String>>>) -> Vec<u64> {
        log.lock()
            .unwrap()
            .iter()
            .filter_map(|c| {
                c.strip_prefix("F ")
                    .and_then(|h| h.trim().parse::<u64>().ok())
            })
            .collect()
    }

    #[test]
    fn aprs_tune_lands_and_stays_while_ft8_is_running() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        // A rig whose dial read-back LAGS two polls behind a set — the documented hazard the
        // read-back guard exists for, and the state the operator's report points at.
        let (addr, log) = lagging_rigctld_stub(2);
        let mut rig = Rig::rigctld(&addr);
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut tick = 0.0f64;
        let mut run = |state: &mut RadioLoop,
                       rig: &mut Rig,
                       backend: &mut MockBackend,
                       n: usize,
                       tick: &mut f64| {
            for _ in 0..n {
                *tick += 400.0;
                state
                    .step(
                        &engine,
                        backend,
                        rig,
                        &sinks,
                        *tick,
                        &mut ra,
                        &mut rr,
                        &mut station,
                    )
                    .unwrap();
            }
        };

        // The operator is running FT8 on 2 m: Digital section, 144.174, TX armed.
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("digital", false);
            e.set_frequency(144.174, "2m", "USB");
            e.set_tx_enabled(true);
        }
        run(&mut state, &mut rig, &mut backend, 3, &mut tick);
        assert!(
            commanded_freqs(&log).contains(&144_174_000),
            "scene: the loop settled on the FT8 frequency: {:?}",
            commanded_freqs(&log)
        );

        // A slot over is in flight — the rig is KEYED. A QSY must not happen mid-TX...
        state.tx_until_ms = Some(now_unix_ms() + 60_000.0);
        engine.lock().unwrap().aprs_tune(144.390).unwrap();
        run(&mut state, &mut rig, &mut backend, 3, &mut tick);
        assert!(
            !commanded_freqs(&log).contains(&144_390_000),
            "a QSY must never be pushed while the rig is keyed"
        );

        // ...but the moment the over ends it must LAND. The operator pressed a button whose
        // entire meaning is "move the radio"; dropping that intent silently is the bug.
        state.tx_until_ms = None;
        run(&mut state, &mut rig, &mut backend, 3, &mut tick);
        assert!(
            commanded_freqs(&log).contains(&144_390_000),
            "the deferred APRS tune must land once TX ends: {:?}",
            commanded_freqs(&log)
        );

        // And it must STAY. This half catches the FT machinery re-asserting its own frequency
        // after an initially-successful QSY (the section-follow class).
        run(&mut state, &mut rig, &mut backend, 8, &mut tick);
        assert_eq!(
            commanded_freqs(&log).last().copied(),
            Some(144_390_000),
            "APRS must still own the dial after further FT8 loop iterations: {:?}",
            commanded_freqs(&log)
        );
        assert_eq!(
            engine.lock().unwrap().snapshot().radio.dial_mhz,
            144.390,
            "and the app agrees the radio is on the APRS frequency"
        );

        // What MODE token actually went on the wire. APRS is FM: a 2 m packet signal
        // demodulated as SSB is garbled audio, so an APRS tune that lands the frequency but
        // leaves the rig in USB would decode nothing while looking perfectly tuned.
        //
        // ⚠️ SCOPE: this proves what we SEND, not what a rig accepts — a rigctld dummy accepts
        // bogus mode tokens (see reference-build-verify-limits), so only the radio can confirm
        // the far end. KD9TAW confirmed on the real IC-9700 that APRS Tune sets FM.
        let modes: Vec<String> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| c.strip_prefix("M ").map(|m| m.to_string()))
            .collect();
        assert!(
            modes.iter().any(|m| m.starts_with("FM")),
            "the APRS tune must command FM, not a data/SSB submode: {modes:?}"
        );
        assert_eq!(
            modes.last().map(|m| m.split_whitespace().next().unwrap_or("")),
            Some("FM"),
            "and FM must still be the last mode commanded — nothing re-asserts SSB after it: {modes:?}"
        );
    }

    // ---- An HF-only rig commanded to a frequency it cannot reach (FTdx10 + APRS, 0.21.x) ----
    //
    // Field report: CAT works in Phone/CW; opening the APRS cockpit auto-tunes 144.390, the
    // FTdx10 (HF/50 MHz only) refuses it, and CAT is dead until Nexus restarts — with the dial
    // still reading 144.390 because no read-back ever corrects it. Drive the REAL loop against a
    // rigctld that refuses out-of-range frequencies and watch what the loop does.

    /// A rigctld standing in for an **HF-only rig**: it accepts `F` only inside `lo..=hi` Hz and
    /// answers `RPRT -1` to anything outside, WITHOUT moving — exactly what Hamlib's newcat
    /// backend does when asked for 2 m on a rig whose range list stops at 54 MHz. `f` always
    /// reports where the rig really is, so a refused set is observable as "the dial never moved".
    fn range_limited_rigctld(lo: u64, hi: u64, start: u64) -> (String, Arc<Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = seen.clone();
        std::thread::spawn(move || {
            let mut cur = start;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut out = match stream.try_clone() {
                    Ok(o) => o,
                    Err(_) => return,
                };
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    rec.lock().unwrap().push(line.clone());
                    let reply = if let Some(hz) = line.strip_prefix("F ") {
                        match hz.trim().parse::<u64>() {
                            // In range: the rig moves and confirms.
                            Ok(v) if (lo..=hi).contains(&v) => {
                                cur = v;
                                "RPRT 0\n".to_string()
                            }
                            // Out of range: refused, and the dial STAYS where it was.
                            _ => "RPRT -1\n".to_string(),
                        }
                    } else if line.trim() == "f" {
                        format!("{cur}\n")
                    } else if line.trim() == "m" {
                        "USB\n2400\n".to_string()
                    } else {
                        "RPRT 0\n".to_string()
                    };
                    if out.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        });
        (addr, seen)
    }

    #[test]
    fn a_refused_out_of_range_qsy_never_wedges_cat_or_pollutes_the_dial() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        // The tester's radio: an FTdx10 — HF + 6 m, nothing above 54 MHz. Sitting on 20 m.
        let (addr, log) = range_limited_rigctld(1_800_000, 54_000_000, 14_074_000);
        let mut rig = Rig::rigctld(&addr);
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut tick = 0.0f64;
        let mut run = |state: &mut RadioLoop,
                       rig: &mut Rig,
                       backend: &mut MockBackend,
                       n: usize,
                       tick: &mut f64| {
            for _ in 0..n {
                *tick += 400.0;
                state
                    .step(
                        &engine,
                        backend,
                        rig,
                        &sinks,
                        *tick,
                        &mut ra,
                        &mut rr,
                        &mut station,
                    )
                    .unwrap();
            }
        };

        // Scene: working 20 m phone. CAT is healthy — this rig answers everything in band.
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.set_frequency(14.250, "20m", "USB");
        }
        run(&mut state, &mut rig, &mut backend, 4, &mut tick);
        assert!(
            commanded_freqs(&log).contains(&14_250_000),
            "scene: the in-range QSY reached the rig: {:?}",
            commanded_freqs(&log)
        );
        assert_ne!(state.cat_ok, Some(false), "scene: CAT is healthy to start");

        // The operator opens the APRS cockpit, which auto-tunes the 2 m APRS channel.
        engine.lock().unwrap().aprs_tune(144.390).unwrap();
        run(&mut state, &mut rig, &mut backend, 12, &mut tick);

        // 1. The refusal must not be read as proof the link is alive.
        let snap = engine.lock().unwrap().snapshot();
        assert!(
            !snap.radio.cat_detail.contains("rig accepted a command"),
            "a REFUSED command must never be reported as CAT confirmation: {:?}",
            snap.radio.cat_detail
        );

        // 2. The dial state must not keep a frequency the radio refused. With CAT alive the
        //    read-back knows exactly where the rig is; the app must agree with it.
        assert!(
            (snap.radio.dial_mhz - 144.390).abs() > 1e-6,
            "the app adopted a commanded-but-refused dial and no read-back corrected it \
             (dial reads {} MHz)",
            snap.radio.dial_mhz
        );

        // 3. CAT must still be alive — the whole field report is that it is not.
        assert_ne!(
            state.cat_ok,
            Some(false),
            "a refused out-of-range frequency wedged the CAT link"
        );
        assert!(
            rig.read_freq().is_ok(),
            "the CAT session must survive a refused command"
        );

        // 4. And the refusal must not become a per-tick retry storm on the CAT link.
        let attempts = commanded_freqs(&log)
            .iter()
            .filter(|hz| **hz == 144_390_000)
            .count();
        assert!(
            attempts <= 3,
            "a definitively refused frequency must not be re-sent every loop tick \
             (sent {attempts} times)"
        );
    }

    #[test]
    fn a_tripped_cat_breaker_recovers_instead_of_latching_for_the_session() {
        // ⭐ THE WEDGE. `cat_ok = Some(false)` gates BOTH read-back paths, and the only thing that
        // used to clear it was a successful set_freq/set_mode from the retune block — which does
        // not fire while the commanded dial and mode already equal `last_dial`/`last_mode`. So any
        // transient that tripped the breaker killed CAT for the rest of the session.
        //
        // Measured before the fix: 40 loop ticks against a perfectly healthy rigctld produced ZERO
        // commands on the wire. Not a read, not a set. That is the "CAT is dead until I restart
        // Nexus" the FTdx10 tester reported, and it is a bug about the breaker, not about APRS.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let (addr, log) = range_limited_rigctld(1_800_000, 54_000_000, 14_250_000);
        let mut rig = Rig::rigctld(&addr);
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut tick = 0.0f64;
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.set_frequency(14.250, "20m", "USB");
        }
        for _ in 0..4 {
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
        assert_ne!(state.cat_ok, Some(false), "scene: CAT healthy");

        // A transient trips the breaker (a few slow replies: reconnect churn, a USB spike, or the
        // stalls a refused out-of-range command causes). The link itself is fine.
        state.cat_ok = Some(false);
        state.freq_misses = FREQ_MISS_LIMIT;
        state.cat_retry_at = tick + CAT_RETRY_BASE_MS;
        let before = log.lock().unwrap().len();

        // Run well past the first re-probe window.
        for _ in 0..40 {
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
        let after: Vec<String> = log.lock().unwrap()[before..].to_vec();
        assert!(
            !after.is_empty(),
            "a tripped breaker spoke to the radio ZERO times in 40 ticks — the link is healthy \
             and Nexus never tried again (this is the session-long wedge)"
        );
        assert_eq!(
            state.cat_ok,
            Some(true),
            "the re-probe succeeded, so the breaker must reset: CAT is answering"
        );
        assert_eq!(
            engine.lock().unwrap().snapshot().radio.cat_ok,
            Some(true),
            "and the operator must be told the link came back"
        );
    }

    #[test]
    fn the_breaker_re_probe_backs_off_on_a_link_that_stays_dead() {
        // The breaker's PURPOSE is to stop the loop blocking on a dead read every cycle, so the
        // recovery path must not undo it: a link that stays dead has to cost progressively less,
        // not one timeout per tick.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_port = listener.local_addr().unwrap().port();
        drop(listener); // nothing listening: every command errors instantly
        let mut rig = Rig::rigctld(&format!("127.0.0.1:{dead_port}"));
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut tick = 0.0f64;
        // Trip it the honest way: consecutive heavy-poll read failures.
        for _ in 0..FREQ_MISS_LIMIT {
            state.last_rig_poll = tick - RIG_POLL_MS - 1.0;
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
        assert_eq!(state.cat_ok, Some(false), "breaker tripped");
        let first_retry = state.cat_retry_at;
        assert!(
            first_retry > tick,
            "a re-probe must be SCHEDULED, not left to chance"
        );

        // Each failed re-probe pushes the next one further out, to the ceiling.
        let mut last_gap = 0.0f64;
        for _ in 0..8 {
            tick = state.cat_retry_at + 1.0;
            let at_before = state.cat_retry_at;
            state.last_rig_poll = tick - RIG_POLL_MS - 1.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            let gap = state.cat_retry_at - at_before;
            assert!(
                gap >= last_gap,
                "the re-probe interval must never shrink while the link stays dead"
            );
            last_gap = gap;
            assert_eq!(state.cat_ok, Some(false), "still dead — breaker stays open");
        }
        assert!(
            state.cat_retry_ms >= CAT_RETRY_MAX_MS,
            "backoff reached its ceiling ({} ms) so a dead rig costs ~one timeout per 30 s",
            state.cat_retry_ms
        );
    }

    #[test]
    fn a_refused_dial_is_given_up_on_and_the_app_stops_showing_it() {
        // The APRS-on-an-HF-rig path, end to end through the real loop: the cockpit's auto-tune
        // asks for 144.390, the radio refuses it, and the operator must be left looking at where
        // the radio ACTUALLY is — with the CAT link intact and no per-tick retry storm.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let (addr, log) = range_limited_rigctld(1_800_000, 54_000_000, 14_250_000);
        let mut rig = Rig::rigctld(&addr);
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut tick = 0.0f64;
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.set_frequency(14.250, "20m", "USB");
        }
        for _ in 0..4 {
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }

        // No CAT capability data in this scene (the stub does not answer `\dump_state` with a
        // limited list), so the engine-level gate cannot know — this exercises the BACKSTOP.
        engine.lock().unwrap().aprs_tune(144.390).unwrap();
        for _ in 0..20 {
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }

        let snap = engine.lock().unwrap().snapshot();
        assert!(
            !snap.radio.cat_detail.contains("rig accepted a command"),
            "a REFUSED command must never be reported as CAT confirmation: {:?}",
            snap.radio.cat_detail
        );
        assert!(
            (snap.radio.dial_mhz - 14.250).abs() < 1e-6,
            "the app must show where the radio really is, not the frequency it refused \
             (dial reads {} MHz)",
            snap.radio.dial_mhz
        );
        assert_eq!(
            state.dial_giveup,
            Some(144_390_000),
            "the refused dial is given up on, so it is not re-sent every tick"
        );
        assert_ne!(
            state.cat_ok,
            Some(false),
            "and the CAT session survives a refused command"
        );
        assert!(rig.read_freq().is_ok(), "the link still answers");
        let attempts = commanded_freqs(&log)
            .iter()
            .filter(|hz| **hz == 144_390_000)
            .count();
        assert!(
            attempts <= DIAL_SET_MAX_TRIES as usize,
            "a definitively refused frequency must not be re-sent every loop tick \
             (sent {attempts} times)"
        );
    }

    #[test]
    fn the_refusal_is_what_the_operator_is_told_not_the_mode_note() {
        // The dial and the mode are commanded in the same pass, and the mode SUCCEEDS on a rig that
        // has FM but not 2 m. A cheerful "rig set to FM" beside a dial that never moved is exactly
        // how this bug stayed invisible, so the refusal has to win the status line.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let (addr, _log) = range_limited_rigctld(1_800_000, 54_000_000, 14_250_000);
        let mut rig = Rig::rigctld(&addr);
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut tick = 0.0f64;
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.set_frequency(14.250, "20m", "USB");
        }
        for _ in 0..4 {
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
        engine.lock().unwrap().aprs_tune(144.390).unwrap();
        // Step until the give-up fires, capturing the status AT THAT MOMENT. The CAT detail is a
        // running commentary, not a latch — once the dial has healed back to HF the loop legitimately
        // re-commands USB and says so, which is current news. What must never happen is a mode
        // success burying the refusal in the very tick the refusal occurred.
        let mut detail_at_giveup = None;
        for _ in 0..20 {
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            let snap = engine.lock().unwrap().snapshot();
            if snap.radio.refused_dial_mhz.is_some() && detail_at_giveup.is_none() {
                detail_at_giveup = Some(snap.radio.cat_detail.clone());
            }
        }
        let detail = detail_at_giveup.expect("the loop must give up on the refused dial");
        assert!(
            detail.contains("144.3900") && detail.contains("does not cover"),
            "the CAT status must name the refusal, not a mode success: {detail:?}"
        );
        // And the refusal survives as a durable fact for the UI, independent of the status line.
        assert_eq!(
            engine.lock().unwrap().snapshot().radio.refused_dial_mhz,
            Some(144.39),
            "the refused frequency stays recorded so the cockpit can name it"
        );
    }

    #[test]
    fn a_radio_handoff_drops_the_previous_radios_coverage() {
        // ⚠️ FAIL-OPEN, NOT FAIL-CLOSED. Coverage belongs to the radio: carrying an HF-only rig's
        // range list across a handoff would block a QSY on the VHF radio that just became active —
        // the exact inverse of what the capability gate is for, and it would break the operator's
        // real FTDX10 + IC-9700 setup.
        let mut state = loop_state();
        state.rx_ranges = Some(vec![(30_000, 60_000_000)]);
        state.rx_ranges_probed = true;
        state.dial_giveup = Some(144_390_000);
        state.cat_ok = Some(false);
        state.cat_retry_ms = CAT_RETRY_MAX_MS;

        state.reset_for_handoff();

        assert_eq!(state.rx_ranges, None, "the new radio's coverage is unknown");
        assert!(!state.rx_ranges_probed, "so it must be re-probed");
        assert_eq!(
            state.dial_giveup, None,
            "a dial the OLD radio refused may be perfectly fine on the new one"
        );
        assert_eq!(
            state.cat_retry_ms, CAT_RETRY_BASE_MS,
            "backoff starts fresh"
        );

        // …and the engine drops it on the switch too, so the window before the next poll is open.
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        let r1 = e.add_radio();
        e.set_active_radio(0);
        e.observe_rig_rx_ranges(Some(vec![(30_000, 60_000_000)]));
        assert_eq!(e.rig_covers_mhz(144.390), Some(false));
        e.set_active_radio(r1);
        assert_eq!(
            e.rig_covers_mhz(144.390),
            None,
            "unknown (allow) after the switch — never the old radio's answer"
        );
    }

    #[test]
    fn an_explicit_retune_retries_a_given_up_dial() {
        // Giving up must not be permanent — the operator may have just switched to a radio that
        // CAN reach it. Same principle as `mode_giveup`, and the reason a re-click of a given-up
        // mode is never ignored.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let (addr, log) = range_limited_rigctld(1_800_000, 54_000_000, 14_250_000);
        let mut rig = Rig::rigctld(&addr);
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        let mut tick = 0.0f64;
        state.dial_giveup = Some(144_390_000);
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.aprs_tune(144.390).unwrap(); // an explicit operator retune arms immediate_retune
        }
        let before = commanded_freqs(&log).len();
        for _ in 0..3 {
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
        assert!(
            commanded_freqs(&log)[before..].contains(&144_390_000),
            "an explicit retune must try a given-up dial again: {:?}",
            commanded_freqs(&log)
        );
    }

    #[test]
    fn the_loop_reads_the_radios_frequency_ranges_and_the_gate_uses_them() {
        // The capability gate is only as good as the data behind it: prove the loop actually asks
        // the radio what it covers and that the answer reaches the engine, so `aprs_tune` can refuse
        // 2 m on an HF-only rig BEFORE commanding it there.
        struct HfOnly;
        impl crate::rigctld_server::RigBackend for HfOnly {
            fn freq_hz(&self) -> u64 {
                14_250_000
            }
            fn mode(&self) -> (String, u32) {
                ("USB".into(), 2400)
            }
            fn ptt(&self) -> bool {
                false
            }
            fn set_freq(&self, hz: u64) -> bool {
                (1_800_000..=54_000_000).contains(&hz)
            }
            fn set_mode(&self, _m: &str, _p: u32) -> bool {
                true
            }
            fn set_ptt(&self, _on: bool) -> bool {
                true
            }
            /// An FTdx10's real receive coverage: 30 kHz – 60 MHz. No 2 m.
            fn rx_ranges(&self) -> Option<Vec<(u64, u64)>> {
                Some(vec![(30_000, 60_000_000)])
            }
        }
        let backend_rig: Arc<dyn crate::rigctld_server::RigBackend> = Arc::new(HfOnly);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || crate::rigctld_server::serve(listener, backend_rig));

        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut backend = MockBackend::new();
        let mut rig = Rig::rigctld(&format!("127.0.0.1:{port}"));
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("phone", false);
            e.set_frequency(14.250, "20m", "USB");
        }
        let mut tick = 0.0f64;
        for _ in 0..6 {
            state.last_rig_poll = tick - RIG_POLL_MS - 1.0; // make the heavy poll due every tick
            tick += 400.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    tick,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }

        assert_eq!(
            state.rx_ranges,
            Some(vec![(30_000, 60_000_000)]),
            "the loop read the radio's RX range table over CAT"
        );
        let eng = engine.lock().unwrap();
        assert_eq!(
            eng.rig_covers_mhz(144.390),
            Some(false),
            "so the engine KNOWS this radio cannot reach the APRS channel"
        );
        assert_eq!(eng.rig_covers_mhz(14.250), Some(true));
        // …and the snapshot carries it, so the cockpit chip can be honest about it too.
        let snap = eng.snapshot();
        assert_eq!(snap.radio.rx_ranges_mhz.len(), 1);
        assert!((snap.radio.rx_ranges_mhz[0].1 - 60.0).abs() < 1e-6);
    }

    #[test]
    fn field_day_club_push_fires_without_wsjtx_or_psk_sinks() {
        // Field Day club logging (N3FJP) with WSJT-X UDP and PSK Reporter both OFF
        // (the shipped defaults). A completed FD QSO must still reach the club
        // master log — the push used to be nested UNDER the WSJT-X/PSK gate, so it
        // never ran when both sinks were off. Stand up a listener as the N3FJP box
        // and prove the spawned push connects to it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut eng = engine.lock().unwrap();
            eng.apply_settings(Settings {
                // Master switch ON — the snapshot only exposes `field_day` (and so
                // the club push only fires) while `fd_active` is true.
                fd_active: true,
                fd_class: "1D".to_string(),
                fd_section: "WI".to_string(),
                n3fjp_host: "127.0.0.1".to_string(),
                n3fjp_port: port,
                ..Settings::default()
            });
            eng.set_mode("fieldday-run").unwrap();
        }

        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        // Sinks OFF — the pre-fix bug means the club push is never reached.
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // First boundary registers the live (empty) session — a contact already
        // present here would read as a restored journal row and never push.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        assert!(engine
            .lock()
            .unwrap()
            .fd_log_manual("K1ABC", "2A", "EMA", "CW")
            .unwrap());
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                16_000.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        // The push runs on a detached thread; wait (bounded) for it to connect.
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut connected = false;
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok(_) => {
                    connected = true;
                    break;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
        assert!(
            connected,
            "the N3FJP club push fired with WSJT-X and PSK sinks both off"
        );
        assert_eq!(
            station.last_fd_qsos, 1,
            "the FD cursor advanced past the pushed QSO"
        );
    }

    #[test]
    fn field_day_restored_journal_is_not_repushed_to_club_sinks() {
        // Entering FD mode restores the durable ADIF journal, so the loop's
        // FIRST boundary already sees qso_count > 0. Those rows were pushed to
        // the club network in a previous session — re-pushing them dupe-spams
        // N3FJP/N1MM/WSJT-X sinks. Only contacts logged AFTER the loop has seen
        // the live session may push. Stand up a listener as the N3FJP box and
        // prove exactly the ONE new QSO reaches it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();

        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut eng = engine.lock().unwrap();
            eng.apply_settings(Settings {
                // Master switch ON — the snapshot only exposes `field_day` (and so
                // the club push only fires) while `fd_active` is true.
                fd_active: true,
                fd_class: "1D".to_string(),
                fd_section: "WI".to_string(),
                n3fjp_host: "127.0.0.1".to_string(),
                n3fjp_port: port,
                ..Settings::default()
            });
            eng.set_mode("fieldday-run").unwrap();
            // Stands in for the journal restore: a contact already in the log
            // before the loop's first boundary observes the session.
            assert!(eng.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        }

        let mut backend = MockBackend::new();
        let mut rig = Rig::vox();
        let mut state = loop_state();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();

        // First boundary: the restored row must NOT push.
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                0.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        // A NEW contact once the session is live: exactly this one pushes.
        assert!(engine
            .lock()
            .unwrap()
            .fd_log_manual("W2NEW", "3A", "ENY", "PH")
            .unwrap());
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                16_000.0,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();

        // Collect every connection the spawned pushes make: wait (bounded) for
        // the first, then a short grace window so a buggy SECOND push (the
        // restored row) would still be caught.
        use std::io::Read;
        let mut payload = String::new();
        let mut connections = 0;
        let mut stop_at = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < stop_at {
            match listener.accept() {
                Ok((mut s, _)) => {
                    connections += 1;
                    s.set_read_timeout(Some(std::time::Duration::from_millis(500)))
                        .unwrap();
                    let mut buf = String::new();
                    let _ = s.read_to_string(&mut buf); // sender closes → EOF
                    payload.push_str(&buf);
                    stop_at = std::time::Instant::now() + std::time::Duration::from_millis(500);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }

        assert!(
            payload.contains("W2NEW"),
            "the newly logged contact reached the club log"
        );
        assert!(
            !payload.contains("K1ABC"),
            "the restored journal row was re-pushed to the club log"
        );
        assert_eq!(connections, 1, "exactly one push fired (the new QSO only)");
        assert_eq!(
            station.last_fd_qsos, 2,
            "the FD cursor covers restored + new rows"
        );
    }
}
