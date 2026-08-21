//! OmniRig (VE3NEA) as a CAT backend — a **local rigctld-protocol shim over COM**.
//!
//! OmniRig is the Windows COM rig-control server most of the Windows contest/logging
//! ecosystem standardises on: the operator configures the radio ONCE inside OmniRig (rig
//! type, COM port, baud, polling), and every client program drives it through the same
//! server instead of fighting for the serial port.
//!
//! **The shape, and why it is this shape.** Nexus's whole CAT stack — [`crate::rig::Rig`],
//! `probe_cat`, the dual-radio monitors, the handoff, every transmit path — speaks the
//! rigctld TEXT protocol to `127.0.0.1:<port>` and does not care what serves it. So OmniRig
//! is not a new transport: it is a second thing that can LISTEN on a radio's rigctld port,
//! exactly like the native CI-V daemon ([`crate::civ::broker::CivDaemon`]). The protocol
//! itself is NOT re-implemented here — [`OmniBackend`] implements
//! [`crate::rigctld_server::RigBackend`] and every byte on the wire is produced by
//! [`crate::rigctld_server::handle_command`], the same encoder every other backend uses. A
//! second encoder could drift from the first; this one cannot.
//!
//! **The COM boundary is a trait.** [`OmniRigClient`] is the whole of it — get/set
//! frequency, mode, PTT and split, plus a status read. The real implementation lives in
//! [`com`] behind `#[cfg(windows)]`; off Windows [`connect`] returns
//! [`OmniError::Unsupported`], an honest error the UI can show rather than a panic or a
//! no-op that pretends to work. Everything above the trait — the protocol mapping, the
//! offline behaviour, the daemon — is unit-tested on Linux against [`tests::MockOmni`].
//!
//! **COM APARTMENT — the threading choice, stated because it is load-bearing.** A COM
//! object may only be used from the apartment that created it, and `IDispatch` is neither
//! `Send` nor `Sync`, while [`RigBackend`](crate::rigctld_server::RigBackend) demands both.
//! So the object is owned by ONE dedicated worker thread ([`OmniWorker`]) which calls
//! `CoInitializeEx(COINIT_APARTMENTTHREADED)` on itself, creates the object, and then serves
//! requests off a channel; the backend holds only the channel. That gives a single apartment
//! for the object's whole life, serialises every call to it (OmniRig's own queue is
//! per-rig anyway), and keeps the pointer off every other thread. `CoUninitialize` runs on
//! the same thread as the object's release, in that order.
//!
//! **Facts about OmniRig used here are read off its own type library**
//! (`OmniRig_TLB.pas`, VE3NEA/OmniRig): ProgID `OmniRig.OmniRigX`; `Rig1`/`Rig2` on the
//! server object; `Freq` (Hz, `Integer`), `Mode`, `Tx`, `Split`, `Status`, `StatusStr` on
//! the rig object; and the `PM_*`/`ST_*` values in [`param`] and [`OmniStatus`]. **v1 only**
//! (operator ruling) — there is no v2 path here.

#[cfg(windows)]
pub mod com;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::rigctld_server::{serve_connection, RigBackend};

/// The COM ProgID Nexus creates. **v1 only** — `OmniRig.OmniRigX` is the v1 server; a v2
/// path is deliberately not built (operator ruling).
pub const PROGID: &str = "OmniRig.OmniRigX";

/// How long a single COM round-trip may take before the caller gives up. OmniRig answers a
/// property read out of its own polled cache (microseconds); a write is queued to the rig
/// and still returns at once. This budget exists only so a wedged server cannot hang the
/// radio loop — it is not a rig timeout.
const CALL_TIMEOUT: Duration = Duration::from_millis(1500);

/// OmniRig's `RigParamX` bit values, verbatim from `OmniRig_TLB.pas`. They are a BITMASK,
/// so every value is a distinct power of two and `PM_UNKNOWN` is 1, not 0 — a detail worth
/// writing down, because "unknown is zero" is the natural guess and it is wrong here.
pub mod param {
    pub const PM_UNKNOWN: i32 = 0x0000_0001;
    pub const PM_SPLITON: i32 = 0x0000_8000;
    pub const PM_SPLITOFF: i32 = 0x0001_0000;
    pub const PM_RX: i32 = 0x0020_0000;
    pub const PM_TX: i32 = 0x0040_0000;
    pub const PM_CW_U: i32 = 0x0080_0000;
    pub const PM_CW_L: i32 = 0x0100_0000;
    pub const PM_SSB_U: i32 = 0x0200_0000;
    pub const PM_SSB_L: i32 = 0x0400_0000;
    pub const PM_DIG_U: i32 = 0x0800_0000;
    pub const PM_DIG_L: i32 = 0x1000_0000;
    pub const PM_AM: i32 = 0x2000_0000;
    pub const PM_FM: i32 = 0x4000_0000;
}

/// Which of OmniRig's two rig slots this radio drives. OmniRig has exactly two, RIG 1 and
/// RIG 2, each with its own port and rig type; the operator picks one per Nexus radio
/// (default RIG1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RigSlot {
    Rig1,
    Rig2,
}

impl RigSlot {
    /// From the stored setting (`RadioProfile::omnirig_slot`). Anything that is not 2 is
    /// RIG1 — a settings file predating the field loads as 0 and must mean the default.
    pub fn from_setting(n: u8) -> Self {
        if n == 2 {
            RigSlot::Rig2
        } else {
            RigSlot::Rig1
        }
    }

    /// The COM property name on the `OmniRigX` object.
    pub fn property(self) -> &'static str {
        match self {
            RigSlot::Rig1 => "Rig1",
            RigSlot::Rig2 => "Rig2",
        }
    }

    /// What OmniRig's own window calls it, for operator-facing text.
    pub fn label(self) -> &'static str {
        match self {
            RigSlot::Rig1 => "RIG 1",
            RigSlot::Rig2 => "RIG 2",
        }
    }
}

/// Why an OmniRig call could not be served. Every arm is something an operator can act on,
/// which is the point: "OmniRig isn't installed", "RIG 1 isn't configured" and "the COM call
/// failed" have three different cures and must never collapse into one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmniError {
    /// Not Windows: there is no COM here at all. The feature is honestly unavailable.
    Unsupported,
    /// The ProgID is not registered — OmniRig is not installed on this machine.
    NotInstalled,
    /// OmniRig is running, but this slot is not usable. Carries OmniRig's OWN status text.
    RigOffline(String),
    /// Windows refused to LAUNCH the OmniRig server for us because it wants elevation
    /// (`ERROR_ELEVATION_REQUIRED`, 0x800702E4). This is not a missing Nexus setting and it is
    /// not a broken install — it is Windows declining to start one process at a higher
    /// integrity level on behalf of a lower one, and COM cannot raise a UAC prompt to fix it.
    /// It has its own arm because its cure has nothing in common with the other four: the
    /// operator starts OmniRig themselves, or takes the elevation requirement off it.
    NeedsElevation,
    /// A COM call failed, or answered something we cannot read.
    Com(String),
}

impl std::fmt::Display for OmniError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OmniError::Unsupported => write!(
                f,
                "OmniRig is Windows-only — it is a Windows COM server, so this connection \
                 type cannot run on this machine. Use Serial or Network instead."
            ),
            OmniError::NotInstalled => write!(
                f,
                "OmniRig does not appear to be installed — Windows has no {PROGID} \
                 registered. Install OmniRig (dxatlas.com/OmniRig) and run it once so it \
                 registers itself, then try again."
            ),
            OmniError::RigOffline(s) => write!(f, "OmniRig says: {s}"),
            OmniError::NeedsElevation => write!(
                f,
                "Windows would not let Nexus start OmniRig — it is set to run as \
                 administrator, and Nexus is not (0x800702E4). Start OmniRig yourself and \
                 leave it running, then try again: Nexus attaches to the copy already up. \
                 If it still refuses, right-click OmniRig.exe → Properties → Compatibility \
                 and clear \"Run this program as an administrator\" — or run both as \
                 administrator, so the two are at the same level."
            ),
            OmniError::Com(s) => write!(f, "OmniRig COM call failed: {s}"),
        }
    }
}

impl std::error::Error for OmniError {}

impl From<OmniError> for std::io::Error {
    fn from(e: OmniError) -> Self {
        std::io::Error::other(e.to_string())
    }
}

/// OmniRig's `RigStatusX`, verbatim from `OmniRig_TLB.pas`. Only `Online` can drive a rig;
/// every other value is a distinct thing to tell the operator, which is why this is an enum
/// and not a bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmniStatus {
    NotConfigured,
    Disabled,
    PortBusy,
    NotResponding,
    Online,
    /// A value this build does not know — reported rather than guessed at.
    Other(i32),
}

impl OmniStatus {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => OmniStatus::NotConfigured,
            1 => OmniStatus::Disabled,
            2 => OmniStatus::PortBusy,
            3 => OmniStatus::NotResponding,
            4 => OmniStatus::Online,
            other => OmniStatus::Other(other),
        }
    }

    pub fn online(self) -> bool {
        self == OmniStatus::Online
    }

    /// A fallback description for when OmniRig's own `StatusStr` is empty. OmniRig's text is
    /// preferred everywhere it is available — this is only so a blank string never becomes a
    /// blank error message.
    pub fn describe(self, slot: RigSlot) -> String {
        let what = match self {
            OmniStatus::NotConfigured => {
                "is not configured — pick its rig type and COM port in OmniRig"
            }
            OmniStatus::Disabled => "is switched off in OmniRig",
            OmniStatus::PortBusy => "cannot open its COM port — another program is holding it",
            OmniStatus::NotResponding => {
                "is not responding — check the radio is on and its CAT settings match OmniRig"
            }
            OmniStatus::Online => "is online",
            OmniStatus::Other(_) => "reported a status this build does not recognise",
        };
        format!("OmniRig {} {what}", slot.label())
    }
}

/// The modes this shim translates between the rigctld protocol and OmniRig's `PM_*` mode
/// params. Deliberately an enum over the eight OmniRig actually has: a rigctld mode word
/// with no OmniRig equivalent must be REFUSED (`RPRT -1`), never silently mapped to the
/// nearest thing, because "the rig is in some other mode than you asked for" is how an
/// operator transmits the wrong emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmniMode {
    CwU,
    CwL,
    SsbU,
    SsbL,
    DigU,
    DigL,
    Am,
    Fm,
}

impl OmniMode {
    /// The OmniRig `PM_*` value.
    pub fn param(self) -> i32 {
        match self {
            OmniMode::CwU => param::PM_CW_U,
            OmniMode::CwL => param::PM_CW_L,
            OmniMode::SsbU => param::PM_SSB_U,
            OmniMode::SsbL => param::PM_SSB_L,
            OmniMode::DigU => param::PM_DIG_U,
            OmniMode::DigL => param::PM_DIG_L,
            OmniMode::Am => param::PM_AM,
            OmniMode::Fm => param::PM_FM,
        }
    }

    /// Read an OmniRig `Mode` property back. `None` for `PM_UNKNOWN` (the rig has not
    /// reported yet) and for anything else — an unexpected value is not a guess.
    pub fn from_param(p: i32) -> Option<Self> {
        Some(match p {
            param::PM_CW_U => OmniMode::CwU,
            param::PM_CW_L => OmniMode::CwL,
            param::PM_SSB_U => OmniMode::SsbU,
            param::PM_SSB_L => OmniMode::SsbL,
            param::PM_DIG_U => OmniMode::DigU,
            param::PM_DIG_L => OmniMode::DigL,
            param::PM_AM => OmniMode::Am,
            param::PM_FM => OmniMode::Fm,
            _ => return None,
        })
    }

    /// The rigctld mode word Nexus (and any Hamlib client) sends → an OmniRig mode.
    ///
    /// The DATA submodes matter most: everything Nexus does on the sound card (FT8/FT4,
    /// RTTY-AFSK, SSTV, PSK) commands `PKTUSB`/`PKTLSB`, and OmniRig's `PM_DIG_U`/`PM_DIG_L`
    /// are exactly that — the rig's DATA/PKT submode. Plain `USB`/`LSB` stay plain.
    pub fn from_rigctld(word: &str) -> Option<Self> {
        Some(match word.trim().to_ascii_uppercase().as_str() {
            "USB" => OmniMode::SsbU,
            "LSB" => OmniMode::SsbL,
            "CW" | "CW-U" | "CWU" => OmniMode::CwU,
            "CWR" | "CW-L" | "CWL" => OmniMode::CwL,
            "PKTUSB" | "DATA-U" | "PKT-U" | "RTTYR" | "USB-D" => OmniMode::DigU,
            "PKTLSB" | "DATA-L" | "PKT-L" | "RTTY" | "LSB-D" => OmniMode::DigL,
            "AM" => OmniMode::Am,
            // OmniRig has ONE FM param; the rig's own DATA switch decides where the audio
            // comes from, so PKTFM maps to it too (an SSTV picture on an FM channel must
            // still be FM, and refusing here would leave the emission wherever it was).
            "FM" | "PKTFM" | "FM-D" | "PKT-FM" => OmniMode::Fm,
            _ => return None,
        })
    }

    /// The rigctld mode word to REPORT for an OmniRig mode. `PM_DIG_*` reports as
    /// `PKTUSB`/`PKTLSB` — the names the rest of Nexus speaks, and what the radio loop
    /// compares its commanded mode against (see `CivBackend::mode`, same rule).
    pub fn to_rigctld(self) -> &'static str {
        match self {
            OmniMode::CwU => "CW",
            OmniMode::CwL => "CWR",
            OmniMode::SsbU => "USB",
            OmniMode::SsbL => "LSB",
            OmniMode::DigU => "PKTUSB",
            OmniMode::DigL => "PKTLSB",
            OmniMode::Am => "AM",
            OmniMode::Fm => "FM",
        }
    }
}

/// **The COM boundary.** Everything Nexus asks of OmniRig, and nothing else.
///
/// Implemented once for real by [`com::OmniRigCom`] (`#[cfg(windows)]`) and once by a mock
/// in this module's tests.
///
/// ⚠️ **Deliberately NOT `Send`.** A COM interface pointer is `!Send` by construction
/// (`IDispatch` wraps a `NonNull`), and requiring `Send` here would be requiring the one
/// thing the apartment rule forbids. The implementation is CREATED on the worker thread by a
/// `Send` factory closure and never leaves it, so a call is only ever made from the thread
/// that owns the object — which is also why an implementation need be neither `Sync` nor
/// re-entrant.
pub trait OmniRigClient {
    /// OmniRig's own view of this slot. Read before every write, so a write to a rig that is
    /// not online is refused rather than silently dropped into OmniRig's queue.
    fn status(&self) -> Result<(OmniStatus, String), OmniError>;
    fn freq_hz(&self) -> Result<u64, OmniError>;
    fn set_freq_hz(&self, hz: u64) -> Result<(), OmniError>;
    /// `Ok(None)` = OmniRig has not learned the rig's mode yet (`PM_UNKNOWN`).
    fn mode(&self) -> Result<Option<OmniMode>, OmniError>;
    fn set_mode(&self, m: OmniMode) -> Result<(), OmniError>;
    fn ptt(&self) -> Result<bool, OmniError>;
    fn set_ptt(&self, on: bool) -> Result<(), OmniError>;
    fn split(&self) -> Result<bool, OmniError>;
    fn set_split(&self, on: bool) -> Result<(), OmniError>;
}

/// Build the real COM client for `slot`, or say honestly why we cannot.
///
/// **This runs ON the worker thread** — see the module header: COM is initialised there and
/// the object never leaves it.
pub fn connect(slot: RigSlot) -> Result<Box<dyn OmniRigClient>, OmniError> {
    #[cfg(windows)]
    {
        com::OmniRigCom::connect(slot).map(|c| Box::new(c) as Box<dyn OmniRigClient>)
    }
    #[cfg(not(windows))]
    {
        let _ = slot;
        Err(OmniError::Unsupported)
    }
}

// ---------------------------------------------------------------------------------------
// The worker thread — the COM apartment
// ---------------------------------------------------------------------------------------

/// A unit of work for the worker thread: a closure run against the live COM client.
type Job = Box<dyn FnOnce(&dyn OmniRigClient) + Send>;

/// The handle every other thread holds. `Send + Sync` (a `Mutex<Sender>`), while the COM
/// object it fronts is neither.
pub struct OmniLink {
    tx: Mutex<mpsc::Sender<Job>>,
    alive: Arc<AtomicBool>,
}

impl OmniLink {
    /// Run `f` on the worker thread and wait for its answer.
    fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn OmniRigClient) -> Result<T, OmniError> + Send + 'static,
    ) -> Result<T, OmniError> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err(OmniError::Com("the OmniRig link has stopped".into()));
        }
        let (rtx, rrx) = mpsc::sync_channel::<Result<T, OmniError>>(1);
        let job: Job = Box::new(move |c| {
            let _ = rtx.send(f(c));
        });
        {
            let tx = self
                .tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if tx.send(job).is_err() {
                self.alive.store(false, Ordering::Relaxed);
                return Err(OmniError::Com(
                    "the OmniRig worker thread has stopped".into(),
                ));
            }
        }
        match rrx.recv_timeout(CALL_TIMEOUT) {
            Ok(r) => r,
            Err(_) => Err(OmniError::Com(
                "OmniRig did not answer within the call budget".into(),
            )),
        }
    }

    /// False once the worker thread has exited (OmniRig closed, or the daemon torn down).
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

/// The thread that owns the COM object for its whole life.
///
/// Its loop polls `stop` rather than ending when the job channel closes, because it must be
/// joinable even while a client connection thread still holds an [`OmniLink`] clone — a
/// browser-tab-shaped logger can sit on a socket for minutes, and the COM object must not
/// wait for it.
struct OmniWorker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl OmniWorker {
    /// Spawn the worker, run `factory` ON it, and report the factory's verdict back here —
    /// so "OmniRig is not installed" surfaces as a start error the caller can show, not as a
    /// thread that quietly died.
    fn start(
        factory: Box<dyn FnOnce() -> Result<Box<dyn OmniRigClient>, OmniError> + Send>,
    ) -> Result<(OmniLink, OmniWorker), OmniError> {
        let (jtx, jrx) = mpsc::channel::<Job>();
        let (otx, orx) = mpsc::sync_channel::<Result<(), OmniError>>(1);
        let alive = Arc::new(AtomicBool::new(true));
        let alive_w = alive.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = stop.clone();
        let handle = std::thread::Builder::new()
            .name("omnirig-com".into())
            .spawn(move || {
                let client = match factory() {
                    Ok(c) => {
                        let _ = otx.send(Ok(()));
                        c
                    }
                    Err(e) => {
                        let _ = otx.send(Err(e));
                        alive_w.store(false, Ordering::Relaxed);
                        return;
                    }
                };
                // Every job runs here, on the apartment that created the object.
                while !stop_w.load(Ordering::Relaxed) {
                    match jrx.recv_timeout(Duration::from_millis(100)) {
                        Ok(job) => job(client.as_ref()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                alive_w.store(false, Ordering::Relaxed);
                // `client` drops here — the COM release happens on this thread, before the
                // implementation's own Drop runs CoUninitialize.
            })
            .map_err(|e| OmniError::Com(format!("could not start the OmniRig thread: {e}")))?;
        // A start error must be the START's error, so wait for the factory's verdict.
        match orx.recv_timeout(CALL_TIMEOUT) {
            Ok(Ok(())) => Ok((
                OmniLink {
                    tx: Mutex::new(jtx),
                    alive,
                },
                OmniWorker {
                    stop,
                    handle: Some(handle),
                },
            )),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err(OmniError::Com(
                    "OmniRig did not finish starting within the call budget".into(),
                ))
            }
        }
    }
}

impl Drop for OmniWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

// ---------------------------------------------------------------------------------------
// The rigctld-protocol backend
// ---------------------------------------------------------------------------------------

/// rigctld-protocol backend that translates every verb into an OmniRig COM call.
///
/// Serves the CORE verb set only (freq / mode / PTT / VFO / split). The extended verbs
/// (levels, funcs, morse, RIT/XIT, repeater) stay at the trait's `None` default → `RPRT -11`,
/// exactly like the pre-extension broker, because OmniRig's v1 interface does not expose
/// them in a way we could answer honestly.
pub struct OmniBackend {
    link: OmniLink,
    slot: RigSlot,
    /// True while Nexus itself intends to transmit — the broker's disconnect fail-safe unkey
    /// stands down then (Nexus's own `Rig` is a client of this shim, and a transient
    /// reconnect must not steal the over). Same contract as `CivBackend::tx_intent`.
    tx_intent: Arc<AtomicBool>,
    /// Last mode OmniRig reported. A read failure serves this rather than inventing one; the
    /// probe judges health on `freq_hz`, which answers 0 for the same failure.
    last_mode: Mutex<Option<OmniMode>>,
}

impl OmniBackend {
    pub fn new(link: OmniLink, slot: RigSlot, tx_intent: Arc<AtomicBool>) -> Self {
        OmniBackend {
            link,
            slot,
            tx_intent,
            last_mode: Mutex::new(None),
        }
    }

    /// The slot this backend drives (operator-facing text).
    pub fn slot(&self) -> RigSlot {
        self.slot
    }

    /// OmniRig's status for this slot, as a sentence — `Ok(())` when it is online.
    ///
    /// Every WRITE goes through this first. OmniRig accepts a write to an offline rig and
    /// drops it, so without the gate a dial move (or a key-down) would answer `RPRT 0` and do
    /// nothing at all: the "healthy pill, dead radio" failure this project keeps paying for.
    pub fn require_online(&self) -> Result<(), OmniError> {
        let slot = self.slot;
        let (st, text) = self.link.call(|c| c.status())?;
        if st.online() {
            return Ok(());
        }
        let text = if text.trim().is_empty() {
            st.describe(slot)
        } else {
            format!("{} — {}", st.describe(slot), text.trim())
        };
        Err(OmniError::RigOffline(text))
    }
}

impl RigBackend for OmniBackend {
    fn owner_transmitting(&self) -> bool {
        self.tx_intent.load(Ordering::Relaxed)
    }

    fn freq_hz(&self) -> u64 {
        // 0 = "no honest reading", the same answer `CivBackend` gives a dead link. `Rig::
        // read_freq` rejects 0, so a failure surfaces as a CAT error instead of a green pill
        // reading 0.000 MHz.
        if self.require_online().is_err() {
            return 0;
        }
        self.link.call(|c| c.freq_hz()).unwrap_or(0)
    }

    fn mode(&self) -> (String, u32) {
        let read = self.link.call(|c| c.mode()).ok().flatten();
        let mut cache = self
            .last_mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if read.is_some() {
            *cache = read;
        }
        // Passband 0 = unknown to a Hamlib client. OmniRig's v1 interface has no filter
        // width, so reporting a number would be inventing one.
        match *cache {
            Some(m) => (m.to_rigctld().to_string(), 0),
            None => ("USB".to_string(), 0),
        }
    }

    fn ptt(&self) -> bool {
        self.link.call(|c| c.ptt()).unwrap_or(false)
    }

    fn split(&self) -> bool {
        self.link.call(|c| c.split()).unwrap_or(false)
    }

    fn set_freq(&self, hz: u64) -> bool {
        self.require_online().is_ok() && self.link.call(move |c| c.set_freq_hz(hz)).is_ok()
    }

    fn set_mode(&self, mode: &str, _passband_hz: u32) -> bool {
        // An unmappable mode word is REFUSED, never approximated — see `OmniMode`.
        let Some(m) = OmniMode::from_rigctld(mode) else {
            return false;
        };
        if self.require_online().is_err() {
            return false;
        }
        let ok = self.link.call(move |c| c.set_mode(m)).is_ok();
        if ok {
            *self
                .last_mode
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(m);
        }
        ok
    }

    fn set_ptt(&self, on: bool) -> bool {
        // ⚠️ TX SAFETY: an UNKEY is attempted even when the status read says the rig is
        // offline. `require_online` costs a COM round-trip and can fail for reasons that have
        // nothing to do with whether the radio is keyed; refusing an unkey on that basis would
        // be the one direction of this gate that can leave a transmitter on the air. A key-DOWN
        // still needs the gate: keying a rig OmniRig is not driving would answer RPRT 0 while
        // nothing happens.
        if on && self.require_online().is_err() {
            return false;
        }
        self.link.call(move |c| c.set_ptt(on)).is_ok()
    }

    fn set_split(&self, on: bool, _tx_vfo: &str) -> Option<bool> {
        if self.require_online().is_err() {
            return Some(false);
        }
        Some(self.link.call(move |c| c.set_split(on)).is_ok())
    }
}

// ---------------------------------------------------------------------------------------
// The daemon
// ---------------------------------------------------------------------------------------

/// **What listens on the radio's rigctld TCP port** when its connection type is OmniRig.
///
/// Same shape and the same contract as [`crate::civ::broker::CivDaemon`]: bind
/// `127.0.0.1:<the radio's own rigctld_port>` (the per-radio port scheme, validated pairwise
/// distinct by `settings::validate_radio_ports` — no second allocation scheme exists), serve
/// [`serve_connection`] per client against one shared backend, and key the rig DOWN at Drop.
pub struct OmniDaemon {
    slot: RigSlot,
    stop: Arc<AtomicBool>,
    tcp_thread: Option<std::thread::JoinHandle<()>>,
    /// Held for its `Drop` alone: it stops and joins the COM apartment thread. Never read.
    _worker: OmniWorker,
    backend: Arc<OmniBackend>,
    tx_intent: Arc<AtomicBool>,
}

impl OmniDaemon {
    /// Start against a caller-supplied COM client factory. The factory runs on the worker
    /// thread — this is the seam the Linux tests drive with a mock.
    pub fn start_with(
        factory: Box<dyn FnOnce() -> Result<Box<dyn OmniRigClient>, OmniError> + Send>,
        slot: RigSlot,
        tcp_port: u16,
    ) -> std::io::Result<OmniDaemon> {
        let (link, worker) = OmniWorker::start(factory).map_err(std::io::Error::from)?;
        let listener = std::net::TcpListener::bind(("127.0.0.1", tcp_port))?;
        listener.set_nonblocking(true)?;
        let tx_intent = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(OmniBackend::new(link, slot, tx_intent.clone()));
        let served: Arc<dyn RigBackend> = backend.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let tcp_thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("omnirig-tcp".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                // WINDOWS GOTCHA (the CI-V daemon paid for this one): WinSock
                                // accept() INHERITS the listener's non-blocking mode, and this
                                // listener is non-blocking so the loop can poll `stop`. Without
                                // the reset, every accepted connection's first idle read hits
                                // WouldBlock, `serve_connection` treats it as an error, and the
                                // client churns reconnects — which, mid-key, trips the
                                // disconnect fail-safe and unkeys the radio.
                                let _ = stream.set_nonblocking(false);
                                let _ = stream.set_nodelay(true);
                                let b = Arc::clone(&served);
                                std::thread::spawn(move || serve_connection(stream, b));
                            }
                            // Transient accept errors must NOT kill the listener, or a healthy
                            // daemon turns permanently connection-refused.
                            Err(_) => std::thread::sleep(Duration::from_millis(50)),
                        }
                    }
                })
                .map_err(std::io::Error::other)?
        };
        Ok(OmniDaemon {
            slot,
            stop,
            tcp_thread: Some(tcp_thread),
            _worker: worker,
            backend,
            tx_intent,
        })
    }

    /// Start against the real OmniRig COM server (the production entry).
    pub fn start(slot: RigSlot, tcp_port: u16) -> std::io::Result<OmniDaemon> {
        Self::start_with(Box::new(move || connect(slot)), slot, tcp_port)
    }

    /// False once the COM worker has gone (OmniRig closed / the object died).
    pub fn is_alive(&self) -> bool {
        self.backend.link.is_alive()
    }

    /// The slot this daemon drives.
    pub fn slot(&self) -> RigSlot {
        self.slot
    }

    /// OmniRig's own status sentence for this slot — what the CAT pill shows when the shim is
    /// up but the radio is not. `Ok(())` when it is online.
    pub fn health(&self) -> Result<(), OmniError> {
        self.backend.require_online()
    }

    /// Tell the shim whether Nexus itself is transmitting, so the broker's disconnect
    /// fail-safe unkey stands down while we are on the air. Same call as
    /// `CivDaemon::set_tx_intent`.
    pub fn set_tx_intent(&self, on: bool) {
        self.tx_intent.store(on, Ordering::Relaxed);
    }
}

impl Drop for OmniDaemon {
    fn drop(&mut self) {
        // TX SAFETY: a rig keyed through OmniRig stays keyed when we merely go away — send a
        // best-effort key-up FIRST, while the worker is still serving. Idempotent, and it
        // covers every teardown path (rig rebuild, monitor recycle, handoff, app exit).
        if self.backend.link.is_alive() {
            let _ = self.backend.link.call(|c| c.set_ptt(false));
        }
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.tcp_thread.take() {
            let _ = t.join();
        }
        // `OmniWorker`'s own Drop stops and joins the COM thread — it does not wait for the
        // job channel to close, because a live client connection still holds a backend clone.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rigctld_server::{handle_command, Handled};
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Mutex as StdMutex;

    /// The whole COM boundary, faked. Everything above the [`OmniRigClient`] trait is real
    /// code — the protocol mapping, the online gate, the worker thread, the daemon and the
    /// rigctld encoder — which is the point of the trait: a Windows-only feature with no
    /// tests on the dev box is how this ships broken.
    pub struct MockOmni {
        pub status: StdMutex<(OmniStatus, String)>,
        pub freq: StdMutex<u64>,
        /// `None` = OmniRig reported PM_UNKNOWN.
        pub mode: StdMutex<Option<OmniMode>>,
        pub ptt: StdMutex<bool>,
        pub split: StdMutex<bool>,
        /// Every call that reached the COM boundary, in order — so a test can prove a
        /// refusal never touched the radio.
        pub calls: StdMutex<Vec<String>>,
    }

    impl MockOmni {
        fn online() -> Self {
            MockOmni {
                status: StdMutex::new((OmniStatus::Online, "on-line".into())),
                freq: StdMutex::new(14_074_000),
                mode: StdMutex::new(Some(OmniMode::SsbU)),
                ptt: StdMutex::new(false),
                split: StdMutex::new(false),
                calls: StdMutex::new(Vec::new()),
            }
        }
        fn note(&self, what: &str) {
            self.calls.lock().unwrap().push(what.to_string());
        }
    }

    impl OmniRigClient for Arc<MockOmni> {
        fn status(&self) -> Result<(OmniStatus, String), OmniError> {
            Ok(self.status.lock().unwrap().clone())
        }
        fn freq_hz(&self) -> Result<u64, OmniError> {
            Ok(*self.freq.lock().unwrap())
        }
        fn set_freq_hz(&self, hz: u64) -> Result<(), OmniError> {
            self.note(&format!("set_freq {hz}"));
            *self.freq.lock().unwrap() = hz;
            Ok(())
        }
        fn mode(&self) -> Result<Option<OmniMode>, OmniError> {
            Ok(*self.mode.lock().unwrap())
        }
        fn set_mode(&self, m: OmniMode) -> Result<(), OmniError> {
            self.note(&format!("set_mode {m:?}"));
            *self.mode.lock().unwrap() = Some(m);
            Ok(())
        }
        fn ptt(&self) -> Result<bool, OmniError> {
            Ok(*self.ptt.lock().unwrap())
        }
        fn set_ptt(&self, on: bool) -> Result<(), OmniError> {
            self.note(&format!("set_ptt {on}"));
            *self.ptt.lock().unwrap() = on;
            Ok(())
        }
        fn split(&self) -> Result<bool, OmniError> {
            Ok(*self.split.lock().unwrap())
        }
        fn set_split(&self, on: bool) -> Result<(), OmniError> {
            self.note(&format!("set_split {on}"));
            *self.split.lock().unwrap() = on;
            Ok(())
        }
    }

    /// A backend wired to `mock`, with the worker thread in between — the real link, not a
    /// direct call.
    fn backend_over(mock: Arc<MockOmni>) -> (OmniBackend, OmniWorker) {
        let m = mock.clone();
        let (link, worker) =
            OmniWorker::start(Box::new(move || Ok(Box::new(m) as Box<dyn OmniRigClient>)))
                .expect("the mock factory starts");
        (
            OmniBackend::new(link, RigSlot::Rig1, Arc::new(AtomicBool::new(false))),
            worker,
        )
    }

    fn reply(line: &str, b: &dyn RigBackend) -> String {
        match handle_command(line, b) {
            Handled::Reply(r) => r,
            Handled::Close => "\0CLOSE".into(),
        }
    }

    /// The core verbs, end to end through the SHARED rigctld encoder — no second protocol
    /// implementation exists here, and this is what proves it.
    #[test]
    fn omnirig_round_trips_frequency_mode_and_ptt_through_the_rigctld_protocol() {
        let mock = Arc::new(MockOmni::online());
        let (b, _w) = backend_over(mock.clone());
        assert_eq!(reply("f", &b), "14074000\n");
        assert_eq!(reply("F 7035000", &b), "RPRT 0\n");
        assert_eq!(*mock.freq.lock().unwrap(), 7_035_000);
        assert_eq!(reply("f", &b), "7035000\n");
        // Hamlib's float wire form (`F 14074000.000000`) is handled by the shared encoder.
        assert_eq!(reply("F 14074000.000000", &b), "RPRT 0\n");
        assert_eq!(*mock.freq.lock().unwrap(), 14_074_000);

        assert_eq!(reply("m", &b), "USB\n0\n");
        assert_eq!(reply("M PKTUSB 3000", &b), "RPRT 0\n");
        assert_eq!(*mock.mode.lock().unwrap(), Some(OmniMode::DigU));
        assert_eq!(reply("m", &b), "PKTUSB\n0\n");

        assert_eq!(reply("t", &b), "0\n");
        assert_eq!(reply("T 1", &b), "RPRT 0\n");
        assert!(*mock.ptt.lock().unwrap(), "T 1 keys through OmniRig");
        // WSJT-X with a Rear/Data audio source sends `T 3` — still key-down.
        assert_eq!(reply("T 0", &b), "RPRT 0\n");
        assert_eq!(reply("T 3", &b), "RPRT 0\n");
        assert!(*mock.ptt.lock().unwrap(), "T 3 (ON_DATA) keys too");
        assert_eq!(reply("T 0", &b), "RPRT 0\n");
        assert!(!*mock.ptt.lock().unwrap());

        // Split rides the extended verb; OmniRig has it, so it answers rather than -11.
        assert_eq!(reply("S 1 VFOB", &b), "RPRT 0\n");
        assert!(*mock.split.lock().unwrap());
        assert_eq!(reply("s", &b), "1\nVFOA\n");
    }

    /// Every rigctld mode word Nexus commands survives the trip to OmniRig and back, and an
    /// unmappable one is REFUSED rather than approximated. Both directions — a mapping shown
    /// only to accept is half a test.
    #[test]
    fn omnirig_mode_mapping_round_trips_and_refuses_what_it_cannot_express() {
        for (word, expect) in [
            ("USB", OmniMode::SsbU),
            ("LSB", OmniMode::SsbL),
            ("CW", OmniMode::CwU),
            ("CWR", OmniMode::CwL),
            ("PKTUSB", OmniMode::DigU),
            ("PKTLSB", OmniMode::DigL),
            ("RTTY", OmniMode::DigL),
            ("AM", OmniMode::Am),
            ("FM", OmniMode::Fm),
            ("PKTFM", OmniMode::Fm),
        ] {
            assert_eq!(
                OmniMode::from_rigctld(word),
                Some(expect),
                "{word} must map to {expect:?}"
            );
            // …and the param round-trips through OmniRig's own bit values.
            assert_eq!(OmniMode::from_param(expect.param()), Some(expect));
        }
        // The positive control for the refusal: a word OmniRig has no param for.
        assert_eq!(OmniMode::from_rigctld("WFM"), None);
        assert_eq!(OmniMode::from_rigctld(""), None);
        // PM_UNKNOWN is 1, not 0 — and neither is a mode.
        assert_eq!(OmniMode::from_param(param::PM_UNKNOWN), None);
        assert_eq!(OmniMode::from_param(0), None);

        let mock = Arc::new(MockOmni::online());
        let (b, _w) = backend_over(mock.clone());
        assert_eq!(
            reply("M WFM 0", &b),
            "RPRT -1\n",
            "an unmappable mode is refused"
        );
        assert!(
            mock.calls.lock().unwrap().is_empty(),
            "a refused mode must never reach the radio: {:?}",
            mock.calls.lock().unwrap()
        );
    }

    /// A rig OmniRig is NOT driving. Reads answer "nothing honest" (0 — which `Rig::read_freq`
    /// rejects, so the pill goes red) and every write is refused instead of being accepted
    /// into a queue nobody serves. THE UNKEY IS THE EXCEPTION and is deliberate.
    #[test]
    fn a_disconnected_rig_reads_as_nothing_and_refuses_writes_but_still_unkeys() {
        let mock = Arc::new(MockOmni::online());
        *mock.status.lock().unwrap() = (
            OmniStatus::NotResponding,
            "RIG 1 is not responding".to_string(),
        );
        let (b, _w) = backend_over(mock.clone());

        assert_eq!(reply("f", &b), "0\n", "no honest dial reading");
        assert_eq!(reply("F 7035000", &b), "RPRT -1\n");
        assert_eq!(reply("M USB 0", &b), "RPRT -1\n");
        assert_eq!(reply("T 1", &b), "RPRT -1\n", "a key-down is refused");
        assert_eq!(reply("S 1 VFOB", &b), "RPRT -1\n");
        assert!(
            mock.calls.lock().unwrap().is_empty(),
            "no refused write may reach the radio: {:?}",
            mock.calls.lock().unwrap()
        );

        // …and the one write that must ALWAYS be attempted.
        assert_eq!(reply("T 0", &b), "RPRT 0\n", "an unkey is never gated off");
        assert_eq!(
            *mock.calls.lock().unwrap(),
            vec!["set_ptt false".to_string()],
            "the unkey reached the radio"
        );

        // The status sentence names the slot AND quotes OmniRig's own words.
        let err = b.require_online().expect_err("offline");
        let msg = err.to_string();
        assert!(msg.contains("RIG 1"), "names the slot: {msg}");
        assert!(msg.contains("not responding"), "quotes OmniRig: {msg}");
    }

    /// OmniRig not installed: a clear message from `start`, not a crash and not a daemon that
    /// pretends to be up. The COM factory is where that verdict is made, so this drives the
    /// same seam production uses.
    #[test]
    fn omnirig_not_installed_is_a_clear_start_error() {
        let Err(e) = OmniDaemon::start_with(
            Box::new(|| Err(OmniError::NotInstalled)),
            RigSlot::Rig1,
            free_port(),
        ) else {
            panic!("a missing OmniRig must not start a daemon")
        };
        let msg = e.to_string();
        assert!(msg.contains("OmniRig"), "names the program: {msg}");
        assert!(
            msg.contains(PROGID),
            "names the ProgID it looked for: {msg}"
        );
        assert!(
            msg.to_ascii_lowercase().contains("install"),
            "says what to do: {msg}"
        );
        // Positive control: the SAME call with a working factory does start.
        let d = OmniDaemon::start_with(
            Box::new(|| Ok(Box::new(Arc::new(MockOmni::online())) as Box<dyn OmniRigClient>)),
            RigSlot::Rig1,
            free_port(),
        )
        .expect("a present OmniRig starts");
        assert!(d.is_alive());
    }

    /// Windows refusing to LAUNCH OmniRig for us is its own verdict with its own cure, and the
    /// sentence has to carry that cure — the operator who hit this asked "are there missing
    /// settings?", because `0x800702E4` on its own reads like one. There are none: the cure is
    /// to start OmniRig yourself, or to take the run-as-administrator flag off it.
    #[test]
    fn an_elevation_refusal_says_what_to_do_about_it() {
        let Err(e) = OmniDaemon::start_with(
            Box::new(|| Err(OmniError::NeedsElevation)),
            RigSlot::Rig1,
            free_port(),
        ) else {
            panic!("an elevation refusal must not start a daemon")
        };
        let msg = e.to_string();
        let lower = msg.to_ascii_lowercase();
        assert!(lower.contains("administrator"), "names the cause: {msg}");
        assert!(
            msg.contains("0x800702E4"),
            "keeps the code support asks for: {msg}"
        );
        assert!(
            lower.contains("start omnirig yourself"),
            "leads with the thing to try first: {msg}"
        );
        // It must NOT read as a broken install — that is the other arm, with the other cure.
        assert!(
            !lower.contains("install omnirig"),
            "does not send them to reinstall: {msg}"
        );
        // Positive control: the not-installed arm DOES say install, so the check above is
        // discriminating between the two arms rather than passing on any text at all.
        assert!(
            OmniError::NotInstalled
                .to_string()
                .to_ascii_lowercase()
                .contains("install"),
            "the control arm still says install"
        );
    }

    /// An unexpected reply from the COM boundary is an error, never a guess: a mode param
    /// OmniRig never documents reads as "no mode" (the cache stands), and a COM failure on a
    /// read leaves the dial at 0 rather than at a number we made up.
    #[test]
    fn an_unexpected_com_reply_is_never_guessed_at() {
        struct Odd;
        impl OmniRigClient for Odd {
            fn status(&self) -> Result<(OmniStatus, String), OmniError> {
                // A status code outside the documented five.
                Ok((OmniStatus::from_code(99), String::new()))
            }
            fn freq_hz(&self) -> Result<u64, OmniError> {
                Err(OmniError::Com("E_UNEXPECTED".into()))
            }
            fn set_freq_hz(&self, _: u64) -> Result<(), OmniError> {
                Ok(())
            }
            fn mode(&self) -> Result<Option<OmniMode>, OmniError> {
                Ok(None) // PM_UNKNOWN
            }
            fn set_mode(&self, _: OmniMode) -> Result<(), OmniError> {
                Ok(())
            }
            fn ptt(&self) -> Result<bool, OmniError> {
                Err(OmniError::Com("E_UNEXPECTED".into()))
            }
            fn set_ptt(&self, _: bool) -> Result<(), OmniError> {
                Ok(())
            }
            fn split(&self) -> Result<bool, OmniError> {
                Ok(false)
            }
            fn set_split(&self, _: bool) -> Result<(), OmniError> {
                Ok(())
            }
        }
        assert_eq!(OmniStatus::from_code(99), OmniStatus::Other(99));
        assert!(!OmniStatus::from_code(99).online(), "unknown is not online");

        let (link, _w) =
            OmniWorker::start(Box::new(|| Ok(Box::new(Odd) as Box<dyn OmniRigClient>))).unwrap();
        let b = OmniBackend::new(link, RigSlot::Rig2, Arc::new(AtomicBool::new(false)));
        assert_eq!(reply("f", &b), "0\n");
        assert_eq!(reply("t", &b), "0\n", "an unreadable PTT reads un-keyed");
        assert_eq!(
            reply("m", &b),
            "USB\n0\n",
            "PM_UNKNOWN falls back, never invents"
        );
        let msg = b.require_online().expect_err("not online").to_string();
        assert!(msg.contains("RIG 2"), "names the slot: {msg}");
    }

    /// The daemon really serves the rigctld protocol on its own port — the same seam
    /// `Rig`, the monitors and any external client use. This is the shim's delivery path.
    #[test]
    fn the_daemon_serves_the_rigctld_protocol_on_its_port() {
        let mock = Arc::new(MockOmni::online());
        let m = mock.clone();
        let port = free_port();
        let d = OmniDaemon::start_with(
            Box::new(move || Ok(Box::new(m) as Box<dyn OmniRigClient>)),
            RigSlot::Rig2,
            port,
        )
        .expect("daemon starts");
        assert_eq!(d.slot(), RigSlot::Rig2);
        assert!(d.health().is_ok(), "an online mock is healthy");

        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut rd = BufReader::new(c.try_clone().unwrap());
        let mut line = String::new();

        c.write_all(b"f\n").unwrap();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "14074000\n");

        line.clear();
        c.write_all(b"F 21074000\n").unwrap();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "RPRT 0\n");
        assert_eq!(*mock.freq.lock().unwrap(), 21_074_000);

        // A probe from `probe_cat_port` must recognise this as a rigctld, or `open_cat`
        // would refuse to share the port with our own shim.
        assert!(crate::rigctld_server::probe_rigctld(
            &format!("127.0.0.1:{port}"),
            Duration::from_millis(800)
        ));
    }

    /// TX SAFETY: a daemon torn down while the rig is keyed sends a key-up on the way out.
    /// Dropping the shim is not a reason for a transmitter to stay on the air.
    #[test]
    fn the_daemon_keys_the_rig_down_when_it_is_dropped() {
        let mock = Arc::new(MockOmni::online());
        let m = mock.clone();
        let d = OmniDaemon::start_with(
            Box::new(move || Ok(Box::new(m) as Box<dyn OmniRigClient>)),
            RigSlot::Rig1,
            free_port(),
        )
        .expect("daemon starts");
        *mock.ptt.lock().unwrap() = true; // on the air
        drop(d);
        assert!(
            !*mock.ptt.lock().unwrap(),
            "Drop must unkey the rig it was driving"
        );
    }

    /// Off Windows the feature is honestly unavailable — not a panic, and not a silent no-op
    /// that would let an operator configure a radio that can never work.
    #[test]
    #[cfg(not(windows))]
    fn omnirig_is_windows_only_and_says_so() {
        let Err(e) = connect(RigSlot::Rig1) else {
            panic!("there is no COM off Windows")
        };
        assert_eq!(e, OmniError::Unsupported);
        let msg = e.to_string();
        assert!(msg.contains("Windows-only"), "{msg}");
        // …and starting a daemon fails with that same sentence rather than binding a port.
        let Err(err) = OmniDaemon::start(RigSlot::Rig1, free_port()) else {
            panic!("a daemon must not start without COM")
        };
        assert!(err.to_string().contains("Windows-only"), "{err}");
    }

    /// The slot selector: 1 and anything unset mean RIG1 (a settings file predating the field
    /// loads as 0), 2 means RIG2.
    #[test]
    fn the_rig_slot_defaults_to_rig1_for_everything_but_two() {
        assert_eq!(RigSlot::from_setting(0), RigSlot::Rig1);
        assert_eq!(RigSlot::from_setting(1), RigSlot::Rig1);
        assert_eq!(RigSlot::from_setting(2), RigSlot::Rig2);
        assert_eq!(RigSlot::from_setting(7), RigSlot::Rig1);
        assert_eq!(RigSlot::Rig1.property(), "Rig1");
        assert_eq!(RigSlot::Rig2.property(), "Rig2");
    }

    /// OmniRig's own status ordinals, kept honest against its type library. `ST_ONLINE` is
    /// **4**, not 3 — the enum has five members (`ST_DISABLED` sits at 1), and reading it as
    /// the four-member set every casual reference quotes would call a NOT-RESPONDING rig
    /// online and put a green pill on a dead radio.
    #[test]
    fn omnirig_status_codes_match_its_type_library() {
        assert_eq!(OmniStatus::from_code(0), OmniStatus::NotConfigured);
        assert_eq!(OmniStatus::from_code(1), OmniStatus::Disabled);
        assert_eq!(OmniStatus::from_code(2), OmniStatus::PortBusy);
        assert_eq!(OmniStatus::from_code(3), OmniStatus::NotResponding);
        assert_eq!(OmniStatus::from_code(4), OmniStatus::Online);
        for code in [0, 1, 2, 3, 5, 99] {
            assert!(
                !OmniStatus::from_code(code).online(),
                "only 4 is ST_ONLINE, not {code}"
            );
        }
    }

    /// Bind :0 to learn a free port, then release it — the same trick the CI-V daemon's tests
    /// use, and race-free enough for a test.
    fn free_port() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    }
}
