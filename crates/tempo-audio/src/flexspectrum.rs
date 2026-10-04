//! FlexRadio native panadapter orchestrator (Wave 7) — behind the `device` feature (uses
//! tempo-net's SmartSDR/VITA parsers and tempo-app's `Engine`).
//!
//! Two threads make a Flex slice's real RF panadapter appear in Nexus's waterfall:
//! - a **TCP control** thread ([`FlexCat`]) registers Nexus as a SmartSDR client, creates a
//!   panafall (a panadapter and the waterfall the radio pairs with it), sizes, centres and paces
//!   the pan once its reply names it, follows the pan's own status (its row scale, its waterfall,
//!   its centre and span), retunes the pan as the operator turns the dial, keeps the session
//!   alive, and removes the pan AND its waterfall on teardown; and
//! - a **UDP FFT** thread receives VITA-49 datagrams, keeps our pan's FFT stream (its stream id
//!   is the pan's id), assembles each frame with the ported decoders ([`FftAssembler`]: every bin
//!   present, the `x_pixels` growth placeholder held back), reads each bin as the pixel row it is,
//!   and hands the frame to the spectrum feed, tagged with the absolute RF span so the UI draws a
//!   true RF scale.
//!
//! It coexists with the shipped Hamlib network-CAT path (this is a *second*, read-only TCP
//! client). Dropping the [`FlexSpectrum`] stops both threads and removes the pan.
//!
//! The pure helpers (command strings, reply and status reading, the stream filter, RF span) are
//! unit-tested here; the thread orchestration is verified on a Flex.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use std::collections::HashMap;

use tempo_app::dto::{Spectrum, SpectrumScale};
use tempo_app::engine::{engine_lock, engine_lock_result, Engine, MeterFeed, SpectrumFeed};
use tempo_net::flex::ownership::parse_panafall_create_pan_id;
use tempo_net::flex::status::{decode, Decoded};
use tempo_net::flex::vita::{decode_fft, FftAssembler, FftFrame, DEFAULT_DISPLAY_MAX, FFT_CLASS};
use tempo_net::flex::wire::{split_status, Kvs, Status};
use tempo_net::flexcat::{parse_meter_defs, FlexCat, FlexMsg, FlexRecv, MeterDef};
use tempo_net::flexvita::{
    convert_meter_raw, dbm_to_watts, parse_meter_values, parse_vita, METER_PACKET_CLASS,
};

/// Panadapter span: a 200 kHz window centered on the dial.
const SPAN_HZ: f64 = 200_000.0;
const FPS: u32 = 15;
/// The pan's width in FFT bins; the UI downsamples to its pixel width.
const X_PIXELS: u32 = 2048;
/// The pan's height in rows: the scale its FFT bins are counted on, row 0 at the top. Upstream's
/// value (A). Any height above the radio's default reads the same window; this one resolves a
/// 100 dB window to a seventh of a dB.
const Y_PIXELS: u32 = 700;
/// The shortest gap between two re-sends of our pan's size after the radio reports its default
/// size back: soon enough to recover from a profile load at once, never a command per status.
const SIZE_PUSH_MIN: Duration = Duration::from_secs(1);
/// Keep the SmartSDR client session alive with periodic traffic.
const KEEPALIVE: Duration = Duration::from_secs(5);
/// Retune the pan when the dial moves more than this (MHz) — ~500 Hz.
const RETUNE_EPS_MHZ: f64 = 0.0005;
/// How long a worker `Drop` may hold the RADIO LOOP waiting for its threads (see [`reap_workers`]).
/// Covers a healthy teardown — the control thread wakes within its 300 ms `recv`, the UDP thread
/// within its 400 ms `recv_from` — so the ordinary toggle-off still completes in place.
const REAP_BUDGET: Duration = Duration::from_millis(600);
/// Poll interval while waiting out [`REAP_BUDGET`].
const REAP_POLL: Duration = Duration::from_millis(10);
/// First delay before a worker re-dials the radio after a lost or refused session.
pub(crate) const RECONNECT_FIRST: Duration = Duration::from_millis(500);
/// Ceiling on the reconnect backoff — a radio that is off for an hour must still be re-found
/// within half a minute of coming back, without either worker retrying in a tight loop meanwhile.
pub(crate) const RECONNECT_MAX: Duration = Duration::from_secs(30);
/// A session that lasted this long counts as HEALTHY: the ladder resets, so an evening's second
/// network blip waits half a second, not half a minute. Shorter than this and the radio is
/// refusing us in a way that repeating faster will not fix.
pub(crate) const SESSION_STABLE_AFTER: Duration = Duration::from_secs(10);
/// How often a backing-off worker re-checks its stop flag. Must stay well inside [`REAP_BUDGET`]:
/// a worker asleep in a backoff is a worker its `Drop` — which runs ON THE RADIO LOOP — is waiting
/// for.
const RECONNECT_POLL: Duration = Duration::from_millis(50);

/// The next reconnect delay: double, capped. Pure, so the ladder is testable with no radio.
pub(crate) fn next_backoff(prev: Duration) -> Duration {
    (prev * 2).min(RECONNECT_MAX)
}

/// Wait out a reconnect delay, in slices, watching `stop`. Returns `false` when the worker must
/// exit instead of re-dialling.
///
/// ⚠️ NOT A BUSY RETRY, AND NOT AN UNINTERRUPTIBLE SLEEP. Both Flex workers used to `return` the
/// moment their session ended — a network blip, a radio reboot, a SmartSDR restart ended native
/// audio and the native panadapter for the rest of the session, with nothing on screen saying so
/// and no way back but a settings save (Flex audit 2026-08-17, #1043's no-reconnect leg). The
/// opposite failure is just as real and this project has already shipped it once: a worker that
/// retries without pacing starves the radio loop (#1000). Hence a doubling ladder, and a sleep
/// broken into [`RECONNECT_POLL`] slices so teardown is never held up by one.
pub(crate) fn backoff_wait(stop: &AtomicBool, delay: Duration) -> bool {
    let until = Instant::now() + delay;
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        std::thread::sleep(RECONNECT_POLL.min(remaining));
    }
}

/// Stop-and-reap worker threads WITHOUT letting a network peer block the caller indefinitely.
///
/// ⚠️ THE CALLER IS THE RADIO LOOP. `FlexSpectrum` and `FlexDax` are dropped from inside `step()` —
/// a settings save, a radio switch, the DAX starvation fallback — and that loop is the only thing
/// that can unkey the transmitter. A plain `join()` there hands a network peer the power to stall
/// it: before the connect was bounded, a black-holed Flex IP froze the loop for the OS SYN timeout,
/// ~21 s on Windows and ~127 s on Linux (Flex audit 2026-08-17, finding #1003).
///
/// So: wait up to `REAP_BUDGET` for the threads to notice their `stop` flag — which is all a
/// healthy teardown needs, and keeps the common case ordered the way it has always been (the old
/// worker's `stream remove` / pan removal completes before a restart creates the new one) — then
/// hand whatever is still running to a detached reaper. `after` runs once every handle is joined,
/// on whichever thread that turns out to be, because callers have teardown work that must not race
/// a live worker (`withdraw_meters` must not be overwritten by a dying meter thread).
pub(crate) fn reap_workers(handles: Vec<JoinHandle<()>>, after: impl FnOnce() + Send + 'static) {
    let deadline = Instant::now() + REAP_BUDGET;
    while Instant::now() < deadline && handles.iter().any(|h| !h.is_finished()) {
        std::thread::sleep(REAP_POLL);
    }
    if handles.iter().all(|h| h.is_finished()) {
        for h in handles {
            let _ = h.join();
        }
        after();
        return;
    }
    // Still wedged (a worker parked in a connect, a socket that will not wake): detach. The
    // threads own everything they touch through `Arc`s, so they are safe to outlive us by the
    // few hundred ms it takes them to fall out of their loops.
    std::thread::spawn(move || {
        for h in handles {
            let _ = h.join();
        }
        after();
    });
}

// ---- pure helpers (unit-tested) ----

/// Pan center (MHz) for a dial reading in Hz.
pub fn pan_center_mhz(dial_hz: u64) -> f64 {
    dial_hz as f64 / 1_000_000.0
}

/// Absolute RF span `(lo_hz, hi_hz)` for a pan centered at `center_mhz` spanning `span_hz`.
pub fn rf_span_hz(center_mhz: f64, span_hz: f64) -> (f64, f64) {
    let center = center_mhz * 1_000_000.0;
    (center - span_hz / 2.0, center + span_hz / 2.0)
}

/// The static SmartSDR commands to register Nexus as a client and route the UDP FFT + meters to us.
pub fn register_commands(udp_port: u16) -> Vec<String> {
    vec![
        "client program Nexus".to_string(),
        format!("client udpport {udp_port}"),
        "sub pan all".to_string(),
        "sub meter all".to_string(),
    ]
}

/// Convert a Flex S-meter reading (dBm, from the `SLC`/`LEVEL` meter) to Nexus's "dB relative to S9"
/// convention (what `observe_rig_smeter` + the cockpit S-meter expect). S9 is −73 dBm on HF and
/// −93 dBm at/above 30 MHz (VHF/UHF). Pure.
pub fn smeter_dbm_to_rel_s9(dbm: f32, dial_hz: u64) -> i32 {
    let s9_dbm = if dial_hz >= 30_000_000 { -93.0 } else { -73.0 };
    (dbm - s9_dbm).round() as i32
}

/// Command to create our panafall: a panadapter and the waterfall the radio pairs with it.
///
/// ⚠️ NOT `display pan create x=… center=… bw=… fps=…`, which this sent until 2026-10: D
/// documents the create as `display panafall create` (with `freq=`, `x=`, `y=`, `ant=`), and `bw`
/// is not a pan key anywhere. The dial, span and frame rate follow in [`pan_setup_commands`] once
/// the reply names the pan.
pub fn create_panafall_command() -> String {
    format!("display panafall create x={X_PIXELS} y={Y_PIXELS}")
}

/// Command to set our pan's size: [`X_PIXELS`] bins of [`Y_PIXELS`] rows.
///
/// ⚠️ THE ROWS ARE THE SCALE. A new pan is the radio's default 50 × 20 display until a client
/// sets its own (A), and nothing set ours until 2026-10, so every bin was one of 20 rows. Sent once
/// the pan is ours, and again whenever its status reports the default size back (a profile load,
/// a reconnect).
pub fn set_pan_size_command(pan_id: u32) -> String {
    format!("display pan set 0x{pan_id:08X} xpixels={X_PIXELS} ypixels={Y_PIXELS}")
}

/// What follows the create once its reply names our pan: the size first, so frames on the
/// radio's default scale end as early as they can, then the dial, the span, the frame rate, and
/// the operator's reference when one is chosen. One key per command, as upstream sends them, so a
/// key the radio refuses costs only itself.
pub fn pan_setup_commands(
    pan_id: u32,
    center_mhz: f64,
    span_hz: f64,
    ref_dbm: Option<i32>,
) -> Vec<String> {
    let mut cmds = vec![
        set_pan_size_command(pan_id),
        set_pan_center_command(pan_id, center_mhz),
        set_pan_bw_command(pan_id, span_hz),
        format!("display pan set 0x{pan_id:08X} fps={FPS}"),
    ];
    cmds.extend(ref_dbm.map(|r| set_pan_ref_command(pan_id, r)));
    cmds
}

/// Command to retune an existing pan (by object id) to a new center.
pub fn set_pan_center_command(pan_id: u32, center_mhz: f64) -> String {
    format!("display pan set 0x{pan_id:08X} center={center_mhz:.6}")
}

/// Command to change an existing pan's BANDWIDTH (span) — SmartSDR takes MHz. The key is
/// `bandwidth` (D); until 2026-10 this sent `bw=`, which is no pan key, so no span change reached
/// the radio.
pub fn set_pan_bw_command(pan_id: u32, span_hz: f64) -> String {
    format!(
        "display pan set 0x{pan_id:08X} bandwidth={:.6}",
        span_hz / 1_000_000.0
    )
}

/// Command to set an existing pan's reference window (SmartSDR `min_dbm`/`max_dbm`): `ref_dbm` is
/// the TOP of the window; a fixed 100 dB range sits below it. NOTE: verified on a real Flex
/// pending — the exact field names/effect are from the SmartSDR API docs, not hardware here.
pub fn set_pan_ref_command(pan_id: u32, ref_dbm: i32) -> String {
    format!(
        "display pan set 0x{pan_id:08X} max_dbm={ref_dbm} min_dbm={}",
        ref_dbm - 100
    )
}

/// Command to remove a pan on teardown.
pub fn remove_pan_command(pan_id: u32) -> String {
    format!("display pan remove 0x{pan_id:08X}")
}

/// Command to remove our pan's waterfall on teardown.
pub fn remove_waterfall_command(waterfall_id: u32) -> String {
    format!("display panafall remove 0x{waterfall_id:08X}")
}

/// What teardown sends: our pan, then its waterfall.
///
/// ⚠️ THE RADIO DOES NOT FREE A PAN'S WATERFALL WITH THE PAN (A, after FlexLib 4.2.18's
/// `Panadapter.Close` and `Waterfall.Close`). Until 2026-10 only the pan was removed, which left a
/// waterfall on the radio after every session. Both ids are ours: the waterfall's comes from our
/// create reply or our own pan's status, never from another client's.
pub fn teardown_commands(pan_id: Option<u32>, waterfall_id: Option<u32>) -> Vec<String> {
    pan_id
        .map(remove_pan_command)
        .into_iter()
        .chain(waterfall_id.map(remove_waterfall_command))
        .collect()
}

/// The pan and waterfall a `display panafall create` REPLY grants us — or `None` (refused, or no
/// pan id in the body), meaning we own no panadapter and must steer or remove none.
///
/// ⚠️ THE REPLY IS THE ONLY THING THAT PROVES A PAN IS OURS. Same rule, same reason, as
/// `flexdax::stream_from_create_reply`: `FlexCat::command`/`send` replies carry a code, and
/// `R7|50000015|bad` parses as a perfectly good reply, so a refusal must not read as a grant.
/// The body is `<pan>,<waterfall>` (D) or `pan=… waterfall=…`; a parser that expected one id read
/// neither. The waterfall also rides our pan's own status ([`our_pan_status`]).
pub fn created_panafall(code: u32, body: &str) -> Option<(u32, Option<u32>)> {
    if code != 0 {
        return None;
    }
    let pan = parse_panafall_create_pan_id(body)?;
    let waterfall = match Kvs::parse(body).get("waterfall") {
        Some(id) => object_id(id),
        None => body.trim().split(',').nth(1).and_then(object_id),
    };
    Some((pan, waterfall))
}

/// An object id with or without its `0x` (a create reply may omit it): one to eight hex digits,
/// and never zero.
fn object_id(text: &str) -> Option<u32> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().filter(|&id| id != 0)
}

/// Is this `display pan …` status about OUR panadapter?
///
/// ⚠️ A FLEX IS A MULTI-CLIENT RADIO AND THIS CODE ASSUMED IT WAS ALONE (audit #1012/#1023).
/// `sub pan all` is the literal all-clients subscription: SmartSDR's own GUI pan, a Maestro's, a
/// second Nexus window's all arrive here. The worker took `pan_id` out of ANY `display pan …` body,
/// last-writer-wins, and then drove that borrowed object — `display pan set … center=`, `bw=`,
/// `max_dbm=` on every dial move, and `display pan remove` on teardown. On the default Flex desk
/// (model 2036's CAT is served by the SmartSDR suite, so the GUI is nearly always running) the
/// stolen pan is the operator's own SmartSDR window: retuned, rescaled, then deleted out from
/// under them.
///
/// Two proofs of ownership, in order:
/// 1. **The create reply.** Once `pan_id` is known it came from OUR `display pan create`, so only
///    that object id is ours — every other pan on the radio is somebody's to be left alone.
/// 2. **The owning client handle**, for the window before the reply lands (or if it is lost):
///    a status is ours only when its `client_handle` is the handle the radio greeted US with.
///
/// FAIL CLOSED — with no reply and no handle match we adopt nothing, and the cost is a blank
/// native waterfall on an opt-in, off-by-default, never-hardware-verified path. Removing another
/// client's panadapter is not a cost that trades against it.
///
/// NEEDS-BENCH: whether SmartSDR actually stamps `client_handle` on `display pan` status bodies
/// (the key is in the published API and this parser has always recognised it, but no capture from
/// a live radio exists here). RECIPE: attach with SmartSDR GUI running, `sub pan all`, and read
/// whether the GUI's pan status carries `client_handle=` — if it does not, path 2 never fires and
/// the create reply is the sole source, which is the intended primary anyway.
pub fn pan_status_is_ours(
    pan_id: Option<u32>,
    our_handle: Option<u32>,
    status_pan: u32,
    status_owner: Option<u32>,
) -> bool {
    match pan_id {
        Some(ours) => status_pan == ours,
        None => our_handle.is_some() && status_owner == our_handle,
    }
}

/// What one status line of OUR pan reported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanReport {
    pub pan: u32,
    /// The waterfall the radio paired with our pan: ours to remove with it.
    pub waterfall: Option<u32>,
    /// The height the radio now counts our pan's rows on. Only a size a client set: a report at
    /// or below [`DEFAULT_DISPLAY_MAX`] is the radio's default, which it can report while frames
    /// on our scale are still arriving (A), so it never becomes the scale.
    pub y_pixels: Option<u32>,
    /// The radio put a dimension back to its default size: set ours again.
    pub size_reset: bool,
    pub center_mhz: Option<f64>,
    pub bandwidth_mhz: Option<f64>,
}

/// Read a status body, through the ported strict decoders, as one of OUR pan's status lines
/// ([`pan_status_is_ours`]). `None` for anything else, our pan's removal included.
pub fn our_pan_status(
    pan_id: Option<u32>,
    our_handle: Option<u32>,
    body: &str,
) -> Option<PanReport> {
    let (object, kvs) = split_status(body);
    let status = Status {
        handle: 0,
        object,
        kvs,
        body: body.to_string(),
    };
    let Decoded::Pan {
        id,
        delta,
        removed: false,
    } = decode(&status)
    else {
        return None;
    };
    if !pan_status_is_ours(pan_id, our_handle, id, delta.client_handle) {
        return None;
    }
    let default_size = |v: Option<i32>| {
        v.and_then(|v| u32::try_from(v).ok())
            .is_some_and(|v| (1..=DEFAULT_DISPLAY_MAX).contains(&v))
    };
    Some(PanReport {
        pan: id,
        waterfall: delta.waterfall,
        y_pixels: delta
            .y_pixels
            .and_then(|y| u32::try_from(y).ok())
            .filter(|&y| y > DEFAULT_DISPLAY_MAX),
        size_reset: default_size(delta.x_pixels) || default_size(delta.y_pixels),
        center_mhz: delta.center_mhz,
        bandwidth_mhz: delta.bandwidth_mhz,
    })
}

/// Is this FFT packet from our pan? An FFT packet's stream id is its pan's id (A), so it is ours
/// exactly when it names the pan our create reply granted.
///
/// ⚠️ UNTIL 2026-10 THIS FILTERED NOTHING. It waited for a `stream_id=` key in our pan's status,
/// which pan status does not carry, and read every FFT stream reaching the port as ours meanwhile.
/// Before our pan is known, nothing is.
pub fn fft_is_ours(packet_stream: Option<u32>, pan_id: Option<u32>) -> bool {
    pan_id.is_some() && packet_stream == pan_id
}

/// Publish one assembled frame to the spectrum feed: ONE call per frame, and the feed stamps it
/// with the next frame number.
///
/// - **Each value is its row's place in the pan's window**, 1 at `max_dbm` and 0 at `min_dbm`
///   ([`FftFrame::levels`]): a bin is a pixel row counted from the top of a display `y_pixels`
///   rows tall (A, port plan §4.7). Until 2026-10 this read `bin / 65535`, which put a carrier at
///   the floor and the floor at the top, squashed into the bottom percent of the scale.
/// - **The scale stays RELATIVE.** The values are exact fractions of the window, so a dBm axis is
///   one label away; that label also needs the window the radio encoded each frame with, which
///   lags a reference change by a frame or two, and nothing here matches the two yet.
/// - The slice is the one the CAT link drives, when its address names one: our pan is centred on
///   that slice's dial and follows it.
fn publish_sweep(
    feed: &SpectrumFeed,
    frame: &FftFrame,
    y_pixels: u32,
    (lo_hz, hi_hz): (f64, f64),
    slice: Option<u8>,
) {
    feed.publish_rf_frame(
        Spectrum {
            row: frame.levels(y_pixels),
            lo_hz,
            hi_hz,
            source: "flex".into(),
        },
        SpectrumScale::Relative,
        slice,
    );
}

/// The active radio's current dial in Hz, from the engine (0 if the lock is unavailable).
fn engine_dial_hz(engine: &Arc<Mutex<Engine>>) -> u64 {
    engine_lock_result(engine)
        .ok()
        .map(|e| (e.settings().dial_mhz * 1_000_000.0) as u64)
        .unwrap_or(0)
}

/// The operator's desired pan bandwidth (Hz) + reference (dBm, `None`=auto), from the engine.
fn engine_flex_controls(engine: &Arc<Mutex<Engine>>) -> (f64, Option<i32>) {
    engine_lock_result(engine)
        .ok()
        .map(|e| (e.flex_pan_span_hz(), e.flex_pan_ref_dbm()))
        .unwrap_or((SPAN_HZ, None))
}

/// Does this slice meter belong to the slice Nexus is operating?
///
/// Rejects only what it can PROVE is another slice's: both numbers known and different. A meter
/// whose slice the radio did not state, or a CAT link whose port names no slice
/// ([`crate::rigmodels::flex_slice_for_cat_addr`]), still reads — a single-slice Flex, the common
/// desk, must not lose its S-meter to a stricter rule than the defect needs.
fn meter_is_ours(def: &MeterDef, our_slice: Option<u16>) -> bool {
    !matches!((def.num, our_slice), (Some(theirs), Some(ours)) if theirs != ours)
}

/// Decode a batch of meter `(id, raw)` pairs against the registry and push the ones Nexus displays
/// (S-meter / SWR / ALC / forward power) into the engine. Matches by source+name (ids are
/// per-session), converts by unit. NOTE: the dBm→S9 and dBFS→ALC mappings are calibrated against the
/// SmartSDR docs, verified on hardware pending.
///
/// TWO-MIRROR RULE: the S-meter lands on the lock-free meter BUS (`meters_out` — what the
/// cockpit's `get_meters` poll displays) AND the engine snapshot copy, bus first, same as every
/// service.rs observe/clear site. Writing only the engine here left a Flex's native S-meter
/// refreshing a mirror nobody displays.
fn route_meters(
    engine: &Arc<Mutex<Engine>>,
    meters_out: &MeterFeed,
    meters: &Arc<Mutex<HashMap<u16, MeterDef>>>,
    pairs: &[(u16, i16)],
    dial_hz: u64,
    our_slice: Option<u16>,
) {
    let (mut smeter, mut swr, mut alc, mut po_w) = (None, None, None, None);
    {
        let defs = meters.lock().unwrap();
        for &(id, raw) in pairs {
            let Some(def) = defs.get(&id) else {
                continue;
            };
            let v = convert_meter_raw(&def.unit, raw);
            match (def.source.as_str(), def.name.as_str()) {
                // ⚠️ OUR SLICE'S S-METER, NOT WHICHEVER ARRIVED LAST (audit #1016/#1028).
                // `sub meter all` registers every slice's meters and this arm matched on
                // source+name alone, overwriting `smeter` for each SLC/LEVEL pair in the packet —
                // so on a multi-slice Flex the needle could be another slice's signal, scaled
                // against OUR dial. The wire carries the slice in `<i>.num=`; it was parsed away.
                // `meter_is_ours` deliberately accepts a meter whose slice the radio did not
                // state, so a single-slice radio keeps its S-meter.
                ("SLC", "LEVEL") if meter_is_ours(def, our_slice) => {
                    smeter = Some(smeter_dbm_to_rel_s9(v, dial_hz))
                }
                (s, "FWDPWR") if s.starts_with("TX") => po_w = Some(dbm_to_watts(v)),
                (s, "SWR") if s.starts_with("TX") => swr = Some(v),
                ("TX", "ALC") => alc = Some(10f32.powf(v / 20.0).clamp(0.0, 1.0)),
                _ => {}
            }
        }
    }
    if smeter.is_none() && swr.is_none() && alc.is_none() && po_w.is_none() {
        return;
    }
    if let Some(db) = smeter {
        meters_out.set_smeter_db(Some(db));
    }
    {
        let mut e = engine_lock(engine);
        if let Some(db) = smeter {
            e.observe_rig_smeter(db);
        }
        if swr.is_some() || alc.is_some() || po_w.is_some() {
            e.observe_rig_tx_meters(swr, alc, po_w, None);
        }
    }
}

/// Withdraw this worker's S-meter contribution from BOTH mirrors (bus + engine) — the teardown
/// half of the two-mirror rule. Called from `Drop` AFTER the threads are joined, so no in-flight
/// `route_meters` can race a dead reading back in. If the rig also answers Hamlib STRENGTH, the
/// radio loop re-populates both within its next cadence; otherwise "—" is the honest state
/// (never the departed stream's last needle).
fn withdraw_meters(engine: &Arc<Mutex<Engine>>, meters_out: &MeterFeed) {
    meters_out.set_smeter_db(None);
    engine_lock(engine).clear_rig_smeter();
}

/// What both threads share about the pan this session created. The control thread writes it; the
/// FFT thread copies it out per packet, so no guard is held across anything else.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PanView {
    /// Our pan, from our create reply (or a status the radio stamped with our handle). Its FFT
    /// stream id is this id (A).
    pan: Option<u32>,
    /// The height our pan's rows are counted on: what we asked for, then what the radio reports.
    y_pixels: u32,
    /// The pan's centre and span, for each frame's RF span: what we asked for, then what the
    /// radio reports, so a span it adjusts is labelled as the span it shows.
    center_mhz: f64,
    span_hz: f64,
}

// ---- orchestrator ----

/// A running Flex panadapter feed. Keep it alive while the Flex radio is the active scope
/// source; dropping it stops both threads, removes the pan, and withdraws its meter readings
/// from both S-meter mirrors (see [`withdraw_meters`]).
pub struct FlexSpectrum {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    engine: Arc<Mutex<Engine>>,
    meters_out: MeterFeed,
}

impl FlexSpectrum {
    /// Connect to the Flex at `ip`, create a pan centered on `dial_hz`, and stream its FFT into
    /// `engine`. Returns once the UDP socket is bound; the threads run until the value is dropped.
    pub fn start(
        engine: Arc<Mutex<Engine>>,
        feed: tempo_app::engine::SpectrumFeed,
        meters_out: MeterFeed,
        ip: String,
        dial_hz: u64,
    ) -> std::io::Result<FlexSpectrum> {
        // Which slice's meters are OURS: the one the CAT link drives (audit #1016/#1028 — see
        // `meter_is_ours`). Read once here, like the Flex IP; the worker restarts on a radio
        // re-select, which is when the CAT address can change.
        let our_slice = engine_lock_result(&engine)
            .ok()
            .map(|e| e.settings().rig_addr.clone())
            .and_then(|addr| crate::rigmodels::flex_slice_for_cat_addr(&addr))
            .map(|n| n as u16);
        // The same slice labels the pan's frames — see `publish_sweep`.
        let slice = our_slice.and_then(|n| u8::try_from(n).ok());
        // Bind the UDP FFT socket FIRST so we can tell SmartSDR which port to stream to.
        let udp = UdpSocket::bind("0.0.0.0:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(400)))?;
        let udp_port = udp.local_addr()?.port();

        let stop = Arc::new(AtomicBool::new(false));
        // Shared: our pan's id (its FFT stream id), the scale its rows are counted on, and its
        // live RF centre and span (driven by the dial and the operator's Flex span control, then by
        // the radio's own report), so the UDP thread can filter, read and label each frame and the
        // TCP thread can retune.
        let (init_span, init_ref) = engine_flex_controls(&engine);
        let view = Arc::new(Mutex::new(PanView {
            pan: None,
            y_pixels: Y_PIXELS,
            center_mhz: pan_center_mhz(dial_hz),
            span_hz: init_span,
        }));
        // Meter registry (id → definition), learned from the control plane; the UDP thread decodes
        // 0x8002 value packets against it (match by source+name — ids are per-session).
        let meters = Arc::new(Mutex::new(HashMap::<u16, MeterDef>::new()));
        let mut handles = Vec::new();

        // --- TCP control thread ---
        {
            let stop = stop.clone();
            let view = view.clone();
            let engine = engine.clone();
            let meters = meters.clone();
            handles.push(std::thread::spawn(move || {
                // ONE SESSION PER TURN OF THIS LOOP. A dropped connection ends the session and the
                // worker re-dials with backoff instead of dying (see `backoff_wait`).
                let mut backoff = RECONNECT_FIRST;
                while !stop.load(Ordering::Relaxed) {
                    let Ok(mut flex) = FlexCat::connect(&ip) else {
                        if !backoff_wait(&stop, backoff) {
                            return;
                        }
                        backoff = next_backoff(backoff);
                        continue;
                    };
                    // Dropped while we were connecting (bounded, but not instant): the radio has heard
                    // nothing from us, so leave it exactly as we found it rather than create a pan
                    // whose teardown would then race the worker that replaced us.
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let session_start = Instant::now();
                    for cmd in register_commands(udp_port) {
                        let _ = flex.send(&cmd);
                    }
                    // Keep the create's sequence number: its reply is what proves the pan we steer is
                    // one WE created (see `pan_status_is_ours`). `send` rather than `command` so the
                    // async status stream still comes through this loop.
                    let create_seq = flex.send(&create_panafall_command()).ok();
                    let mut pan_id: Option<u32> = None;
                    let mut waterfall_id: Option<u32> = None;
                    let mut last_ka = Instant::now();
                    let mut last_center = view.lock().unwrap().center_mhz;
                    let mut last_span = init_span;
                    let mut last_ref = init_ref;
                    let mut last_size_push: Option<Instant> = None;
                    while !stop.load(Ordering::Relaxed) {
                        // The pan id, the moment it is first known.
                        let mut learned: Option<u32> = None;
                        // Our pan, when its status reports the radio's default size.
                        let mut size_reset: Option<u32> = None;
                        // Drain async status → learn the pan, its waterfall and its scale (send()
                        // left the status stream for us; command() would have swallowed it).
                        match flex.recv(Duration::from_millis(300)) {
                            // OUR create's reply: the authoritative ids of the pan and waterfall we
                            // own.
                            FlexRecv::Msg(FlexMsg::Reply { seq, code, msg })
                                if Some(seq) == create_seq =>
                            {
                                if let Some((pid, wf)) = created_panafall(code, &msg) {
                                    learned = Some(pid);
                                    waterfall_id = waterfall_id.or(wf);
                                }
                            }
                            FlexRecv::Msg(FlexMsg::Status { body, .. }) => {
                                // ⚠️ ONLY OUR OWN PAN — `sub pan all` also delivers SmartSDR's
                                // (audit #1012/#1023; the rule and its bench recipe are on
                                // `pan_status_is_ours`).
                                if let Some(report) = our_pan_status(pan_id, flex.handle(), &body) {
                                    if pan_id.is_none() {
                                        learned = Some(report.pan);
                                    }
                                    waterfall_id = report.waterfall.or(waterfall_id);
                                    {
                                        let mut v = view.lock().unwrap();
                                        if let Some(y) = report.y_pixels {
                                            v.y_pixels = y;
                                        }
                                        if let Some(c) = report.center_mhz {
                                            v.center_mhz = c;
                                        }
                                        if let Some(b) = report.bandwidth_mhz {
                                            v.span_hz = b * 1_000_000.0;
                                        }
                                    }
                                    if report.size_reset {
                                        size_reset = Some(report.pan);
                                    }
                                }
                                // Learn meter definitions (id → source/name/unit) as they arrive.
                                for def in parse_meter_defs(&body) {
                                    meters.lock().unwrap().insert(def.index, def);
                                }
                            }
                            FlexRecv::Msg(_) | FlexRecv::Idle => {}
                            // ⚠️ THE CONNECTION IS GONE — END THE SESSION, never keep looping on it
                            // (audit #1000). This is the ONLY thing pacing this loop: a disconnected
                            // channel returns instantly, so continuing free-runs, and every iteration
                            // below takes the shared engine mutex twice, starving the radio loop and
                            // the whole UI with it. The outer loop re-dials with backoff.
                            FlexRecv::Closed => break,
                        }
                        // The pan just became ours: size, centre and pace it (the create carries
                        // no dial), and let the FFT thread read its stream.
                        if let Some(pid) = learned {
                            pan_id = Some(pid);
                            view.lock().unwrap().pan = Some(pid);
                            for cmd in pan_setup_commands(pid, last_center, last_span, last_ref) {
                                let _ = flex.send(&cmd);
                            }
                            last_size_push = Some(Instant::now());
                        }
                        // The radio put our pan back to its default size (a profile load, a
                        // reconnect): set ours again, at most once a second — and not on top of
                        // the setup just sent.
                        if let Some(pid) = size_reset {
                            if last_size_push.is_none_or(|t| t.elapsed() >= SIZE_PUSH_MIN) {
                                let _ = flex.send(&set_pan_size_command(pid));
                                last_size_push = Some(Instant::now());
                            }
                        }
                        if let Some(pid) = pan_id {
                            // Retune the pan when the operator's dial moves.
                            let want = pan_center_mhz(engine_dial_hz(&engine));
                            if want > 0.0 && (want - last_center).abs() > RETUNE_EPS_MHZ {
                                let _ = flex.send(&set_pan_center_command(pid, want));
                                view.lock().unwrap().center_mhz = want;
                                last_center = want;
                            }
                            // Apply the operator's span + reference controls when they change.
                            let (want_span, want_ref) = engine_flex_controls(&engine);
                            if (want_span - last_span).abs() > 1.0 {
                                let _ = flex.send(&set_pan_bw_command(pid, want_span));
                                view.lock().unwrap().span_hz = want_span;
                                last_span = want_span;
                            }
                            if want_ref != last_ref {
                                if let Some(r) = want_ref {
                                    let _ = flex.send(&set_pan_ref_command(pid, r));
                                }
                                last_ref = want_ref;
                            }
                        }
                        if last_ka.elapsed() >= KEEPALIVE {
                            let _ = flex.send("ping"); // keep the client session alive
                            last_ka = Instant::now();
                        }
                    }
                    // End of THIS session. Remove only what we created, the pan AND its waterfall
                    // (see `teardown_commands`) — both ids can only have come from our own create
                    // reply or from a status the radio stamped with OUR client handle, so this can no
                    // longer delete the operator's SmartSDR panadapter (audit #1012/#1023). On a
                    // dropped socket the send fails harmlessly and the radio reaps a departed client's
                    // objects itself.
                    for cmd in teardown_commands(pan_id, waterfall_id) {
                        let _ = flex.send(&cmd);
                    }
                    // The pan is per-session: the UDP thread keeps nothing until the next session's
                    // pan is ours, and that pan starts on our own scale again.
                    {
                        let mut v = view.lock().unwrap();
                        v.pan = None;
                        v.y_pixels = Y_PIXELS;
                    }
                    // A session that stood up for a while was healthy — the next blip starts the
                    // ladder over rather than inheriting a long wait.
                    if session_start.elapsed() >= SESSION_STABLE_AFTER {
                        backoff = RECONNECT_FIRST;
                    }
                    if !backoff_wait(&stop, backoff) {
                        return;
                    }
                    backoff = next_backoff(backoff);
                }
            }));
        }

        // --- UDP FFT thread ---
        {
            let stop = stop.clone();
            let view = view.clone();
            let meters = meters.clone();
            let engine = engine.clone();
            let meters_out = meters_out.clone();
            handles.push(std::thread::spawn(move || {
                let mut asm = FftAssembler::new();
                // The pan `asm` is assembling for: a new pan (a reconnect) starts afresh, so no
                // fragment or width of the old one carries over.
                let mut assembling: Option<u32> = None;
                let mut dg = vec![0u8; 16 * 1024];
                while !stop.load(Ordering::Relaxed) {
                    let Ok((n, _)) = udp.recv_from(&mut dg) else {
                        continue; // timeout → re-check stop
                    };
                    let Some(pkt) = parse_vita(&dg[..n]) else {
                        continue;
                    };
                    // A copy, taken in one statement: no guard outlives it, so nothing below
                    // runs under the lock.
                    let v = *view.lock().unwrap();
                    match pkt.packet_class {
                        Some(FFT_CLASS) => {
                            if !fft_is_ours(pkt.stream_id, v.pan) {
                                continue;
                            }
                            if assembling != v.pan {
                                asm = FftAssembler::new();
                                assembling = v.pan;
                            }
                            let Some(fragment) = decode_fft(pkt.payload, pkt.has_trailer) else {
                                continue;
                            };
                            if let Some(frame) = asm.push(&fragment) {
                                // Straight to the feed. This used to take the ENGINE mutex —
                                // which the radio loop holds across its blocking boundary CAT —
                                // so the Flex panadapter was starved by the same hold that
                                // starved the audio row, despite already having its own thread.
                                publish_sweep(
                                    &feed,
                                    &frame,
                                    v.y_pixels,
                                    rf_span_hz(v.center_mhz, v.span_hz),
                                    slice,
                                );
                            }
                        }
                        Some(METER_PACKET_CLASS) => {
                            route_meters(
                                &engine,
                                &meters_out,
                                &meters,
                                &parse_meter_values(pkt.payload, pkt.has_trailer),
                                (v.center_mhz * 1_000_000.0) as u64,
                                our_slice,
                            );
                        }
                        // Our waterfall's tiles (`0x8004`) are not drawn yet: the feed has no slot
                        // for the radio's own waterfall rows. `flex::vita::TileAssembler` decodes
                        // them when one exists.
                        _ => {}
                    }
                }
            }));
        }

        Ok(FlexSpectrum {
            stop,
            handles,
            engine,
            meters_out,
        })
    }
}

impl Drop for FlexSpectrum {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Bounded, never open-ended: this runs ON THE RADIO LOOP (see `reap_workers`).
        let engine = self.engine.clone();
        let meters_out = self.meters_out.clone();
        reap_workers(self.handles.drain(..).collect(), move || {
            // Joined first, then withdrawn: no meter thread survives to write a stale reading
            // back onto either mirror after this clear.
            withdraw_meters(&engine, &meters_out);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_and_rf_span() {
        assert_eq!(pan_center_mhz(145_000_000), 145.0);
        let (lo, hi) = rf_span_hz(145.0, SPAN_HZ);
        assert_eq!(lo, 144_900_000.0);
        assert_eq!(hi, 145_100_000.0);
        // f64 keeps a UHF edge exact (f32 could not).
        let (lo, hi) = rf_span_hz(432.1, SPAN_HZ);
        assert_eq!(lo, 432_000_000.0);
        assert_eq!(hi, 432_200_000.0);
    }

    #[test]
    fn register_commands_route_udp_to_us() {
        let cmds = register_commands(52001);
        assert_eq!(cmds[0], "client program Nexus");
        assert_eq!(cmds[1], "client udpport 52001");
        assert_eq!(cmds[2], "sub pan all");
        assert_eq!(cmds[3], "sub meter all");
    }

    #[test]
    fn smeter_dbm_maps_to_rel_s9_by_band() {
        // HF: S9 = −73 dBm → a −73 dBm reading is 0 dB rel S9; −53 dBm is +20.
        assert_eq!(smeter_dbm_to_rel_s9(-73.0, 14_074_000), 0);
        assert_eq!(smeter_dbm_to_rel_s9(-53.0, 14_074_000), 20);
        // VHF: S9 = −93 dBm.
        assert_eq!(smeter_dbm_to_rel_s9(-93.0, 144_174_000), 0);
    }

    #[test]
    fn pan_command_strings() {
        assert_eq!(
            set_pan_center_command(0x40000000, 144.2),
            "display pan set 0x40000000 center=144.200000"
        );
        assert_eq!(
            remove_pan_command(0x40000000),
            "display pan remove 0x40000000"
        );
    }

    #[test]
    fn span_and_ref_command_strings() {
        assert_eq!(
            set_pan_bw_command(0x40000000, 50_000.0),
            "display pan set 0x40000000 bandwidth=0.050000"
        );
        // ref = top of the window; a 100 dB range sits below.
        assert_eq!(
            set_pan_ref_command(0x40000000, -40),
            "display pan set 0x40000000 max_dbm=-40 min_dbm=-140"
        );
    }

    /// A pan status as the radio sends it, with an owner.
    fn pan_status(pan_id: u32, owner: u32) -> String {
        format!(
            "display pan 0x{pan_id:08X} center=14.100 bandwidth=0.200 waterfall=0x42000000 \
             client_handle=0x{owner:08X}"
        )
    }

    /// NEXUS STEERS ONLY THE PANADAPTER IT CREATED (audit #1012/#1023).
    ///
    /// `sub pan all` delivers every pan on the radio, and the worker adopted the id out of any of
    /// them, last-writer-wins — then retuned, rescaled and finally REMOVED that borrowed object.
    /// On a Flex desk the other pan is usually SmartSDR's own GUI window.
    #[test]
    fn a_pan_belonging_to_another_client_is_never_adopted() {
        const OURS: u32 = 0x2ABC;
        const SMARTSDR: u32 = 0x1234;

        // Through the decoder the worker uses, so the owner it reads is the owner on the wire.
        let adopted = |pan: Option<u32>, handle: Option<u32>, body: &str| {
            our_pan_status(pan, handle, body).is_some()
        };
        // Before our create reply lands, the owning handle is the only proof available.
        assert!(
            !adopted(None, Some(OURS), &pan_status(0x4000_0001, SMARTSDR)),
            "SmartSDR's own panadapter must never be adopted"
        );
        assert!(
            adopted(None, Some(OURS), &pan_status(0x4000_0000, OURS)),
            "a pan the radio says is ours is ours"
        );
        // Fail closed: no handle yet (the first ms of a session) → adopt nothing.
        assert!(!adopted(None, None, &pan_status(0x4000_0000, OURS)));

        // Once the create reply has named our pan, only that object id is ours — a foreign status
        // cannot overwrite it, which is what "last-writer-wins" did.
        let (ours, _) = created_panafall(0, "0x40000000").expect("the create reply grants the id");
        assert_eq!(ours, 0x4000_0000);
        assert!(adopted(Some(ours), Some(OURS), &pan_status(ours, OURS)));
        assert!(
            !adopted(Some(ours), Some(OURS), &pan_status(0x4000_0001, SMARTSDR)),
            "another pan's status must not repoint us at it"
        );
    }

    /// A REFUSED create is not a grant — the same rule the DAX TX create already follows.
    #[test]
    fn a_pan_is_owned_only_from_a_successful_create() {
        assert_eq!(created_panafall(0, "0x40000000"), Some((0x4000_0000, None)));
        assert_eq!(
            created_panafall(0x5000_0015, "0x40000000,0x42000000"),
            None,
            "an error code with an id-shaped body is still a refusal"
        );
        assert_eq!(created_panafall(0, ""), None);
        // Both shapes the reply comes in, and ids written without their `0x`.
        assert_eq!(
            created_panafall(0, "pan=0x40000001 waterfall=0x42000001"),
            Some((0x4000_0001, Some(0x4200_0001)))
        );
        assert_eq!(
            created_panafall(0, "40000000,42000000"),
            Some((0x4000_0000, Some(0x4200_0000)))
        );
        assert_eq!(
            created_panafall(0, "0x40000000,garbage"),
            Some((0x4000_0000, None)),
            "a waterfall id that does not parse is no waterfall, never a guess"
        );
    }

    /// The reconnect ladder: doubling, capped, never zero (audit #1043's no-reconnect leg).
    #[test]
    fn the_reconnect_ladder_doubles_up_to_the_ceiling() {
        let mut d = RECONNECT_FIRST;
        assert_eq!(next_backoff(d), RECONNECT_FIRST * 2);
        for _ in 0..20 {
            let next = next_backoff(d);
            assert!(next > d || next == RECONNECT_MAX, "the ladder must climb");
            assert!(next <= RECONNECT_MAX, "…and stop at the ceiling");
            d = next;
        }
        assert_eq!(d, RECONNECT_MAX);
    }

    /// A WORKER ASLEEP IN A BACKOFF IS A WORKER ITS `Drop` IS WAITING FOR, and that `Drop` runs on
    /// the radio loop (`reap_workers`). Both directions: a stop that is already set returns at
    /// once, a stop that arrives mid-wait is noticed inside the reap budget, and an untouched wait
    /// really does wait.
    ///
    /// ⚠️ **"AT ONCE" IS COUNTED AGAINST A REAL WAIT, NOT A CLOCK BUDGET.** The defect here is
    /// [`backoff_wait`] sleeping before it reads the flag instead of after, and that costs
    /// exactly one [`RECONNECT_POLL`] — so the `< 50 ms` this used to assert sat precisely ON
    /// the value it was supposed to discriminate, while also being a size of scheduling stall
    /// a loaded box hands out for free. One already-stopped wait cannot separate the two — it
    /// measures 8.6 µs here, and 50 ms either way. TWENTY can: under a millisecond in total as
    /// the code stands, a measured 1.003 s with the flag read late, against a 120 ms wait taken
    /// on the same box in the same run. A stall big enough to break that ratio breaks the
    /// yardstick with it.
    #[test]
    fn a_backing_off_worker_gives_up_as_soon_as_it_is_stopped() {
        // The delay the positive control below waits out — and the yardstick the
        // already-stopped waits are measured against.
        const REAL_WAIT: Duration = Duration::from_millis(120);
        const STOPPED_RUNS: u32 = 20;
        assert!(
            RECONNECT_POLL * STOPPED_RUNS > REAL_WAIT * 4,
            "the fixture no longer separates a sleep-first backoff from a stop-first one: \
             {STOPPED_RUNS} × {RECONNECT_POLL:?} must stay well clear of {REAL_WAIT:?}"
        );

        let stop = Arc::new(AtomicBool::new(true));
        let t = Instant::now();
        for _ in 0..STOPPED_RUNS {
            assert!(!backoff_wait(&stop, RECONNECT_MAX));
        }
        let stopped = t.elapsed();

        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            flag.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        assert!(!backoff_wait(&stop, RECONNECT_MAX));
        assert!(
            t.elapsed() < REAP_BUDGET,
            "a stop mid-backoff must be noticed inside the reap budget, not after {:?}",
            t.elapsed()
        );

        // Positive control: with nothing stopping it, the wait is actually waited out.
        let stop = Arc::new(AtomicBool::new(false));
        let t = Instant::now();
        assert!(backoff_wait(&stop, REAL_WAIT));
        let real_wait = t.elapsed();
        assert!(real_wait >= Duration::from_millis(100));

        assert!(
            stopped < real_wait,
            "{STOPPED_RUNS} already-stopped waits took {stopped:?}, against {real_wait:?} for \
             ONE real wait — the stop flag is being read after the sleep, not before it"
        );
    }

    /// The registry + value-pair shape [`slc_level_batch`] returns — named to keep
    /// clippy's type-complexity line, not because a second consumer exists.
    type SlcLevelBatch = (Arc<Mutex<HashMap<u16, MeterDef>>>, Vec<(u16, i16)>);

    /// One SLC/LEVEL registry entry + one value pair, as the VITA meter stream delivers them.
    fn slc_level_batch() -> SlcLevelBatch {
        let meters = Arc::new(Mutex::new(HashMap::new()));
        meters.lock().unwrap().insert(
            7,
            MeterDef {
                index: 7,
                source: "SLC".into(),
                name: "LEVEL".into(),
                unit: "dBm".into(),
                num: Some(0),
            },
        );
        // −53 dBm, ÷128-scaled on the wire; on 20 m (S9 = −73 dBm) that is S9+20.
        (meters, vec![(7, -53i16 * 128)])
    }

    #[test]
    fn a_native_meter_batch_reaches_both_smeter_mirrors() {
        // THE TWO-MIRROR RULE. The cockpit S-meter reads the lock-free meter BUS
        // (`get_meters` → MeterFeed); the snapshot carries the ENGINE copy. A site that
        // writes one and not the other splits the truth: a Flex with the native meter
        // stream active refreshed the mirror nobody displays while the cockpit read a
        // dead bus ("—" forever if Hamlib STRENGTH is also unsupported).
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let bus = MeterFeed::default();
        let (meters, pairs) = slc_level_batch();
        route_meters(&engine, &bus, &meters, &pairs, 14_074_000, Some(0));
        assert_eq!(
            bus.smeter_db(),
            Some(20),
            "the meter bus is what the cockpit S-meter displays"
        );
        assert_eq!(
            engine.lock().unwrap().snapshot().radio.smeter_db,
            Some(20),
            "the engine mirror carries the same reading"
        );
    }

    /// THE NEEDLE MUST BE OUR SLICE'S SIGNAL (audit #1016/#1028).
    ///
    /// `sub meter all` registers every slice's meters, and the S-meter arm matched on source+name
    /// alone — so with two slices open the LAST SLC/LEVEL pair in each packet won, whichever slice
    /// it came from, scaled against our dial. Here slice 1 is loud (S9+40) and slice 0, the one
    /// CAT drives, is weak (S9+20): the cockpit must read 20.
    #[test]
    fn the_smeter_reads_our_slice_and_not_the_loud_one_next_door() {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let bus = MeterFeed::default();
        let meters = Arc::new(Mutex::new(HashMap::new()));
        for (id, slice) in [(7u16, 0u16), (9, 1)] {
            meters.lock().unwrap().insert(
                id,
                MeterDef {
                    index: id,
                    source: "SLC".into(),
                    name: "LEVEL".into(),
                    unit: "dBm".into(),
                    num: Some(slice),
                },
            );
        }
        // Our slice first, the neighbour's LAST — the order that made last-wins visible.
        let pairs = vec![(7u16, -53i16 * 128), (9, -33i16 * 128)];
        route_meters(&engine, &bus, &meters, &pairs, 14_074_000, Some(0));
        assert_eq!(
            bus.smeter_db(),
            Some(20),
            "slice 1's stronger signal must not land on our needle"
        );

        // A radio that does not state the meter's slice still gets an S-meter (the single-slice
        // desk must not lose its needle to a stricter rule than the defect needs).
        let bus = MeterFeed::default();
        meters.lock().unwrap().get_mut(&7).expect("def").num = None;
        route_meters(&engine, &bus, &meters, &pairs[..1], 14_074_000, Some(0));
        assert_eq!(bus.smeter_db(), Some(20));
    }

    #[test]
    fn teardown_withdraws_the_smeter_from_both_mirrors() {
        // The teardown half of the two-mirror rule: when the native meter stream goes away
        // (pan toggled off / radio switched), its last needle must not stay frozen on either
        // mirror — "—" until a live source writes again.
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let bus = MeterFeed::default();
        let (meters, pairs) = slc_level_batch();
        route_meters(&engine, &bus, &meters, &pairs, 14_074_000, Some(0));
        assert_eq!(bus.smeter_db(), Some(20), "precondition: a live reading");
        withdraw_meters(&engine, &bus);
        assert_eq!(bus.smeter_db(), None, "bus cleared on teardown");
        assert_eq!(
            engine.lock().unwrap().snapshot().radio.smeter_db,
            None,
            "engine mirror cleared on teardown"
        );
    }

    /// ONE REASSEMBLED SWEEP, ONE FRAME. A fragment that completes nothing publishes nothing; the
    /// fragment that completes a sweep publishes it once, and the feed numbers it.
    #[test]
    fn each_reassembled_sweep_is_one_numbered_frame() {
        use tempo_net::flex::vita::FftFragment;
        let feed = SpectrumFeed::default();
        let newest = |last| {
            feed.scope_frame_after(0.0, 0.0, Default::default(), last, || {
                unreachable!("a sweep was published, so the fallback is never asked")
            })
        };
        let fragment = |frame_index: u32, start_bin: u16, rows: &[u16]| FftFragment {
            start_bin,
            total_bins: 4,
            frame_index,
            rows: rows.to_vec(),
        };
        let span = rf_span_hz(14.1, SPAN_HZ);
        let mut asm = FftAssembler::new();
        // Rows of a 701-row display: 700 is the bottom of the window, 0 its top.
        assert!(asm.push(&fragment(7, 0, &[700, 0])).is_none());
        let frame = asm
            .push(&fragment(7, 2, &[0, 700]))
            .expect("covered end to end");
        publish_sweep(&feed, &frame, 701, span, Some(1));
        let first = newest(0).expect("the sweep reached the feed");
        assert_eq!(first.seq, 1);
        assert_eq!(first.source, "flex");
        assert_eq!(first.scale, SpectrumScale::Relative);
        assert_eq!(first.slice, Some(1), "the slice the CAT link drives");
        assert_eq!(first.bins, vec![0.0, 1.0, 1.0, 0.0]);
        assert_eq!((first.lo_hz, first.hi_hz), span);

        assert!(asm.push(&fragment(8, 0, &[1, 1])).is_none());
        assert_eq!(
            newest(first.seq),
            None,
            "half of the next sweep is not a new frame"
        );
        let frame = asm.push(&fragment(8, 2, &[1, 1])).expect("the next sweep");
        publish_sweep(&feed, &frame, 701, span, Some(1));
        let second = newest(first.seq).expect("the next sweep is a new frame");
        assert_eq!(second.seq, first.seq + 1, "one sweep, one step");
    }

    // ── The shipped path's display defects (port plan §4.8) ─────────────────────────────────
    //
    // Each test below was first run against the code it replaces, reached through thin adapters,
    // and failed there on the defect it names.

    /// The pan status the radio sends for a new panafall (A: no `stream_id=` key; the FFT stream
    /// id is the pan id).
    const PAN_STATUS: &str =
        "display pan 0x40000000 client_handle=0x2B6E1F40 waterfall=0x42000000 \
         center=14.100000 bandwidth=0.200000 x_pixels=2048 y_pixels=700 fps=15 min_dbm=-140.00 \
         max_dbm=-40.00";

    fn publish_rows(feed: &SpectrumFeed, rows: &[u16], y_pixels: u32) {
        let frame = FftFrame {
            frame_index: 1,
            rows: rows.to_vec(),
            floor_from: None,
        };
        publish_sweep(feed, &frame, y_pixels, rf_span_hz(14.1, SPAN_HZ), Some(0));
    }

    /// The parts of a pan report the scale and the teardown depend on.
    fn pan_report(
        pan: Option<u32>,
        handle: Option<u32>,
        body: &str,
    ) -> Option<(Option<u32>, Option<u32>, bool)> {
        our_pan_status(pan, handle, body).map(|r| (r.waterfall, r.y_pixels, r.size_reset))
    }

    fn newest(feed: &SpectrumFeed) -> tempo_app::dto::SpectrumFrame {
        feed.scope_frame_after(0.0, 0.0, Default::default(), 0, || {
            unreachable!("a frame was published")
        })
        .expect("a frame")
    }

    /// (a) A BIN IS A PIXEL ROW COUNTED FROM THE TOP, NOT A MAGNITUDE. Row 0 is the pan's
    /// `max_dbm`, the strongest reading, and row `y_pixels - 1` its `min_dbm`. Read as
    /// `bin / 65535`, the carrier came out as the floor and the floor at the top: the trace
    /// upside down, and squashed into the bottom percent of the scale.
    #[test]
    fn the_strongest_row_is_the_top_of_the_trace() {
        let feed = SpectrumFeed::default();
        publish_rows(&feed, &[700, 0, 700, 350], 701);
        let frame = newest(&feed);
        assert_eq!(frame.bins, vec![0.0, 1.0, 0.0, 0.5]);
        assert_eq!(frame.scale, SpectrumScale::Relative);
    }

    /// (b) OUR PAN IS SIZED ONCE IT IS OURS. With no `ypixels` the radio keeps its default
    /// 50 × 20 display, so every bin is one of 20 rows and nothing says which scale they count.
    /// The create carries no dial, so the centre, span and frame rate follow it too.
    #[test]
    fn our_pan_is_sized_centred_and_paced_once_it_is_ours() {
        assert_eq!(
            pan_setup_commands(0x4000_0000, 14.1, 50_000.0, Some(-40)),
            vec![
                "display pan set 0x40000000 xpixels=2048 ypixels=700",
                "display pan set 0x40000000 center=14.100000",
                "display pan set 0x40000000 bandwidth=0.050000",
                "display pan set 0x40000000 fps=15",
                "display pan set 0x40000000 max_dbm=-40 min_dbm=-140",
            ]
        );
        assert!(
            !pan_setup_commands(0x4000_0000, 14.1, 50_000.0, None)
                .iter()
                .any(|c| c.contains("max_dbm")),
            "no reference chosen, none sent"
        );
    }

    /// (c) THE CREATE AND SPAN COMMANDS THE RADIO DOCUMENTS. `display pan create … bw=…` is not
    /// the documented create (D: `display panafall create`, which gives the pan its waterfall),
    /// and `bw` is not a pan key anywhere (D: `bandwidth`). The create reply names both objects
    /// (D: `<pan>,<waterfall>`); a parser expecting one id read neither.
    #[test]
    fn the_pan_is_created_and_spanned_with_documented_commands() {
        assert_eq!(
            create_panafall_command(),
            "display panafall create x=2048 y=700"
        );
        assert_eq!(
            set_pan_bw_command(0x4000_0000, 50_000.0),
            "display pan set 0x40000000 bandwidth=0.050000"
        );
        assert_eq!(
            created_panafall(0, "0x40000000,0x42000000"),
            Some((0x4000_0000, Some(0x4200_0000)))
        );
    }

    /// (d) THE WATERFALL GOES WITH ITS PAN. The radio does not free a pan's waterfall when the
    /// pan is removed (A, after FlexLib 4.2.18's close order), so removing only the pan left a
    /// waterfall on the radio after every session.
    #[test]
    fn teardown_removes_the_waterfall_with_its_pan() {
        assert_eq!(
            teardown_commands(Some(0x4000_0000), Some(0x4200_0000)),
            vec![
                "display pan remove 0x40000000",
                "display panafall remove 0x42000000",
            ]
        );
        assert_eq!(
            teardown_commands(None, None),
            Vec::<String>::new(),
            "nothing of ours, nothing removed"
        );
    }

    /// OUR PAN'S FFT STREAM ID IS THE PAN'S OWN ID (A). The filter waited for a `stream_id=` key
    /// that pan status does not carry, so it never filtered: every FFT stream reaching the port
    /// was read as ours. Before our pan is known, nothing is.
    #[test]
    fn only_our_pans_fft_stream_is_read() {
        assert!(fft_is_ours(Some(0x4000_0000), Some(0x4000_0000)));
        assert!(
            !fft_is_ours(Some(0x4000_0001), Some(0x4000_0000)),
            "another pan's stream"
        );
        assert!(
            !fft_is_ours(Some(0x4000_0000), None),
            "before our pan is known"
        );
        assert!(
            !fft_is_ours(None, Some(0x4000_0000)),
            "a packet with no stream id"
        );
    }

    /// OUR PAN'S STATUS SETS THE SCALE ITS ROWS ARE READ ON, and a size the radio put back to
    /// its default is set again. A default-sized report never becomes the scale: the radio can
    /// report its default while frames on our scale are still arriving (A).
    #[test]
    fn our_pans_status_sets_the_row_scale_and_a_reset_size_is_set_again() {
        const HANDLE: Option<u32> = Some(0x2B6E_1F40);
        assert_eq!(
            pan_report(Some(0x4000_0000), HANDLE, PAN_STATUS),
            Some((Some(0x4200_0000), Some(700), false))
        );
        let report = our_pan_status(Some(0x4000_0000), HANDLE, PAN_STATUS).expect("our pan");
        assert_eq!(
            (report.center_mhz, report.bandwidth_mhz),
            (Some(14.1), Some(0.2)),
            "the span a frame is labelled with is the one the radio reports"
        );
        assert_eq!(
            pan_report(
                Some(0x4000_0000),
                HANDLE,
                "display pan 0x40000000 x_pixels=50 y_pixels=20"
            ),
            Some((None, None, true)),
            "the radio's default size: set ours again, and keep the scale we had"
        );
        assert_eq!(
            pan_report(
                Some(0x4000_0000),
                HANDLE,
                "display pan 0x40000001 client_handle=0x7A3C0001 waterfall=0x42000001 y_pixels=480"
            ),
            None,
            "another client's pan tells us nothing"
        );
    }
}
