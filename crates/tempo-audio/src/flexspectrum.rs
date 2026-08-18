//! FlexRadio native panadapter orchestrator (Wave 7) — behind the `device` feature (uses
//! tempo-net's SmartSDR/VITA parsers and tempo-app's `Engine`).
//!
//! Two threads make a Flex slice's real RF panadapter appear in Nexus's waterfall:
//! - a **TCP control** thread ([`FlexCat`]) registers Nexus as a SmartSDR client, creates a
//!   panadapter centered on the dial, learns the pan's VITA **stream id** from the async status
//!   stream, retunes the pan as the operator turns the dial, keeps the session alive, and
//!   removes the pan on teardown; and
//! - a **UDP FFT** thread receives VITA-49 datagrams, filters to that stream, reassembles the
//!   sweep ([`FftReassembler`]), and hands each completed row to [`Engine::set_spectrum_rf`],
//!   tagged with the absolute RF span so the UI draws a true RF scale.
//!
//! It coexists with the shipped Hamlib network-CAT path (this is a *second*, read-only TCP
//! client). Dropping the [`FlexSpectrum`] stops both threads and removes the pan.
//!
//! The pure helpers (command strings, RF span, bin normalization) are unit-tested here; the
//! thread orchestration + exact SmartSDR command syntax are verified on a Flex.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use std::collections::HashMap;

use tempo_app::dto::Spectrum;
use tempo_app::engine::{engine_lock, Engine, MeterFeed};
use tempo_net::flexcat::{
    parse_create_stream_id, parse_meter_defs, parse_pan_status, FlexCat, FlexMsg, FlexRecv,
    MeterDef, PanStatus,
};
use tempo_net::flexvita::{
    convert_meter_raw, dbm_to_watts, parse_fft, parse_meter_values, parse_vita, FftReassembler,
    FFT_PACKET_CLASS, METER_PACKET_CLASS,
};

/// Panadapter span: a 200 kHz window centered on the dial.
const SPAN_HZ: f64 = 200_000.0;
const FPS: u32 = 15;
/// Flex FFT width cap; the UI downsamples to its pixel width.
const X_PIXELS: u32 = 2048;
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

/// Command to create a panadapter `x_pixels` wide, centered at `center_mhz`, spanning `span_hz`
/// (SmartSDR takes MHz for center and bandwidth).
pub fn create_pan_command(center_mhz: f64, span_hz: f64, x_pixels: u32, fps: u32) -> String {
    format!(
        "display pan create x={x_pixels} center={center_mhz:.6} bw={:.6} fps={fps}",
        span_hz / 1_000_000.0
    )
}

/// Command to retune an existing pan (by object id) to a new center.
pub fn set_pan_center_command(pan_id: u32, center_mhz: f64) -> String {
    format!("display pan set 0x{pan_id:08X} center={center_mhz:.6}")
}

/// Command to change an existing pan's BANDWIDTH (span) — SmartSDR takes MHz.
pub fn set_pan_bw_command(pan_id: u32, span_hz: f64) -> String {
    format!(
        "display pan set 0x{pan_id:08X} bw={:.6}",
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

/// The pan object id a `display pan create` REPLY grants us — or `None` (refused, or no id in the
/// body), meaning we own no panadapter and must steer or remove none.
///
/// ⚠️ THE REPLY IS THE ONLY THING THAT PROVES A PAN IS OURS. Same rule, same reason, as
/// `flexdax::stream_from_create_reply`: `FlexCat::command`/`send` replies carry a code, and
/// `R7|50000015|bad` parses as a perfectly good reply, so a refusal must not read as a grant.
pub fn created_pan_id(code: u32, body: &str) -> Option<u32> {
    if code != 0 {
        return None;
    }
    parse_create_stream_id(body)
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
pub fn pan_status_is_ours(pan_id: Option<u32>, our_handle: Option<u32>, st: &PanStatus) -> bool {
    match pan_id {
        Some(ours) => st.pan_id == Some(ours),
        None => our_handle.is_some() && st.client_handle == our_handle,
    }
}

/// Normalize reassembled u16 FFT bins to the UI's 0..1 waterfall scale (the UI's AGC/LUT does
/// the display stretch, same contract as the audio-FFT path). Monotonic; the on-Flex test
/// calibrates the reference level.
pub fn fft_to_row(bins: &[u16]) -> Vec<f32> {
    bins.iter()
        .map(|&b| f32::from(b) / f32::from(u16::MAX))
        .collect()
}

/// The active radio's current dial in Hz, from the engine (0 if the lock is unavailable).
fn engine_dial_hz(engine: &Arc<Mutex<Engine>>) -> u64 {
    engine
        .lock()
        .ok()
        .map(|e| (e.settings().dial_mhz * 1_000_000.0) as u64)
        .unwrap_or(0)
}

/// The operator's desired pan bandwidth (Hz) + reference (dBm, `None`=auto), from the engine.
fn engine_flex_controls(engine: &Arc<Mutex<Engine>>) -> (f64, Option<i32>) {
    engine
        .lock()
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
        let our_slice = engine
            .lock()
            .ok()
            .map(|e| e.settings().rig_addr.clone())
            .and_then(|addr| crate::rigmodels::flex_slice_for_cat_addr(&addr))
            .map(|n| n as u16);
        // Bind the UDP FFT socket FIRST so we can tell SmartSDR which port to stream to.
        let udp = UdpSocket::bind("0.0.0.0:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(400)))?;
        let udp_port = udp.local_addr()?.port();

        let stop = Arc::new(AtomicBool::new(false));
        // Shared: the pan's VITA stream id (learned from the async status stream) + its live RF
        // center (MHz), so the UDP thread can filter/label and the TCP thread can retune.
        let stream_id = Arc::new(Mutex::new(None::<u32>));
        let center = Arc::new(Mutex::new(pan_center_mhz(dial_hz)));
        // Live pan bandwidth (Hz), driven by the operator's Flex span control: the TCP thread
        // applies changes to the rig, the UDP thread reads it to label the emitted RF span.
        let (init_span, init_ref) = engine_flex_controls(&engine);
        let span_hz = Arc::new(Mutex::new(init_span));
        // Meter registry (id → definition), learned from the control plane; the UDP thread decodes
        // 0x8002 value packets against it (match by source+name — ids are per-session).
        let meters = Arc::new(Mutex::new(HashMap::<u16, MeterDef>::new()));
        let mut handles = Vec::new();

        // --- TCP control thread ---
        {
            let stop = stop.clone();
            let stream_id = stream_id.clone();
            let center = center.clone();
            let span_hz = span_hz.clone();
            let engine = engine.clone();
            let meters = meters.clone();
            handles.push(std::thread::spawn(move || {
                let Ok(mut flex) = FlexCat::connect(&ip) else {
                    return;
                };
                // Dropped while we were connecting (bounded, but not instant): the radio has heard
                // nothing from us, so leave it exactly as we found it rather than create a pan
                // whose teardown would then race the worker that replaced us.
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                for cmd in register_commands(udp_port) {
                    let _ = flex.send(&cmd);
                }
                // Keep the create's sequence number: its reply is what proves the pan we steer is
                // one WE created (see `pan_status_is_ours`). `send` rather than `command` so the
                // async status stream still comes through this loop.
                let create_seq = flex
                    .send(&create_pan_command(
                        *center.lock().unwrap(),
                        init_span,
                        X_PIXELS,
                        FPS,
                    ))
                    .ok();
                let mut pan_id: Option<u32> = None;
                let mut last_ka = Instant::now();
                let mut last_center = *center.lock().unwrap();
                let mut last_span = init_span;
                let mut last_ref = init_ref;
                while !stop.load(Ordering::Relaxed) {
                    // Drain async status → learn the pan id + VITA stream id (send() left the
                    // status stream for us; command() would have swallowed it).
                    match flex.recv(Duration::from_millis(300)) {
                        // OUR create's reply: the authoritative id of the pan we own.
                        FlexRecv::Msg(FlexMsg::Reply { seq, code, msg })
                            if Some(seq) == create_seq =>
                        {
                            if let Some(pid) = created_pan_id(code, &msg) {
                                pan_id = Some(pid);
                            }
                        }
                        FlexRecv::Msg(FlexMsg::Status { body, .. }) => {
                            // ⚠️ ONLY OUR OWN PAN — `sub pan all` also delivers SmartSDR's
                            // (audit #1012/#1023; the rule and its bench recipe are on
                            // `pan_status_is_ours`).
                            if let Some(st) = parse_pan_status(&body) {
                                if pan_status_is_ours(pan_id, flex.handle(), &st) {
                                    if let Some(pid) = st.pan_id {
                                        pan_id = Some(pid);
                                    }
                                    if let Some(sid) = st.stream_id {
                                        *stream_id.lock().unwrap() = Some(sid);
                                    }
                                }
                            }
                            // Learn meter definitions (id → source/name/unit) as they arrive.
                            for def in parse_meter_defs(&body) {
                                meters.lock().unwrap().insert(def.index, def);
                            }
                        }
                        FlexRecv::Msg(_) | FlexRecv::Idle => {}
                        // ⚠️ THE CONNECTION IS GONE — EXIT, do not loop (audit #1000). This is the
                        // ONLY thing pacing this loop: a disconnected channel returns instantly, so
                        // continuing free-runs, and every iteration below takes the shared engine
                        // mutex twice, starving the radio loop and the whole UI with it. Nothing
                        // below can work without the socket anyway; a Flex power-cycle or a
                        // SmartSDR client drop now ends the worker instead of pinning a core.
                        // (Reconnect-with-backoff is deliberately NOT here: it is its own change.)
                        FlexRecv::Closed => break,
                    }
                    if let Some(pid) = pan_id {
                        // Retune the pan when the operator's dial moves.
                        let want = pan_center_mhz(engine_dial_hz(&engine));
                        if want > 0.0 && (want - last_center).abs() > RETUNE_EPS_MHZ {
                            let _ = flex.send(&set_pan_center_command(pid, want));
                            *center.lock().unwrap() = want;
                            last_center = want;
                        }
                        // Apply the operator's span + reference controls when they change.
                        let (want_span, want_ref) = engine_flex_controls(&engine);
                        if (want_span - last_span).abs() > 1.0 {
                            let _ = flex.send(&set_pan_bw_command(pid, want_span));
                            *span_hz.lock().unwrap() = want_span;
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
                // Remove only what we created. `pan_id` can now only have come from our own
                // create reply or from a status the radio stamped with OUR client handle, so this
                // can no longer delete the operator's SmartSDR panadapter (audit #1012/#1023).
                if let Some(pid) = pan_id {
                    let _ = flex.send(&remove_pan_command(pid));
                }
            }));
        }

        // --- UDP FFT thread ---
        {
            let stop = stop.clone();
            let stream_id = stream_id.clone();
            let center = center.clone();
            let span_hz = span_hz.clone();
            let meters = meters.clone();
            let engine = engine.clone();
            let meters_out = meters_out.clone();
            handles.push(std::thread::spawn(move || {
                let mut asm = FftReassembler::new();
                let mut dg = vec![0u8; 16 * 1024];
                while !stop.load(Ordering::Relaxed) {
                    let Ok((n, _)) = udp.recv_from(&mut dg) else {
                        continue; // timeout → re-check stop
                    };
                    let Some(pkt) = parse_vita(&dg[..n]) else {
                        continue;
                    };
                    match pkt.packet_class {
                        Some(FFT_PACKET_CLASS) => {
                            // Filter to our pan's stream once it's known (accept all until then).
                            // Read the id into a local FIRST: a guard built in an `if let`
                            // scrutinee lives through the whole body, and this body goes on to
                            // lock `center` and `span_hz` — an ordering hazard with no reason
                            // to exist, since only the value is wanted.
                            let want_id = *stream_id.lock().unwrap();
                            if let (Some(want), Some(got)) = (want_id, pkt.stream_id) {
                                if want != got {
                                    continue;
                                }
                            }
                            let Some(frame) = parse_fft(pkt.payload) else {
                                continue;
                            };
                            if let Some(bins) = asm.push(&frame) {
                                let (lo, hi) =
                                    rf_span_hz(*center.lock().unwrap(), *span_hz.lock().unwrap());
                                let spec = Spectrum {
                                    row: fft_to_row(&bins),
                                    lo_hz: lo,
                                    hi_hz: hi,
                                    source: "flex".into(),
                                };
                                // Straight to the feed. This used to take the ENGINE mutex —
                                // which the radio loop holds across its blocking boundary CAT —
                                // so the Flex panadapter was starved by the same hold that
                                // starved the audio row, despite already having its own thread.
                                feed.publish_rf(spec);
                            }
                        }
                        Some(METER_PACKET_CLASS) => {
                            route_meters(
                                &engine,
                                &meters_out,
                                &meters,
                                &parse_meter_values(pkt.payload, pkt.has_trailer),
                                (*center.lock().unwrap() * 1_000_000.0) as u64,
                                our_slice,
                            );
                        }
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
            create_pan_command(145.0, SPAN_HZ, 2048, 15),
            "display pan create x=2048 center=145.000000 bw=0.200000 fps=15"
        );
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
            "display pan set 0x40000000 bw=0.050000"
        );
        // ref = top of the window; a 100 dB range sits below.
        assert_eq!(
            set_pan_ref_command(0x40000000, -40),
            "display pan set 0x40000000 max_dbm=-40 min_dbm=-140"
        );
    }

    /// A pan status as the radio sends it, with an owner.
    fn pan_status(pan_id: u32, owner: u32) -> PanStatus {
        parse_pan_status(&format!(
            "display pan 0x{pan_id:08X} center=14.100 bandwidth=0.200 stream_id=0x42000000 \
             client_handle=0x{owner:08X}"
        ))
        .expect("a pan status body")
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

        // Before our create reply lands, the owning handle is the only proof available.
        assert!(
            !pan_status_is_ours(None, Some(OURS), &pan_status(0x4000_0001, SMARTSDR)),
            "SmartSDR's own panadapter must never be adopted"
        );
        assert!(
            pan_status_is_ours(None, Some(OURS), &pan_status(0x4000_0000, OURS)),
            "a pan the radio says is ours is ours"
        );
        // Fail closed: no handle yet (the first ms of a session) → adopt nothing.
        assert!(!pan_status_is_ours(
            None,
            None,
            &pan_status(0x4000_0000, OURS)
        ));

        // Once the create reply has named our pan, only that object id is ours — a foreign status
        // cannot overwrite it, which is what "last-writer-wins" did.
        let ours = created_pan_id(0, "0x40000000").expect("the create reply grants the id");
        assert_eq!(ours, 0x4000_0000);
        assert!(pan_status_is_ours(
            Some(ours),
            Some(OURS),
            &pan_status(ours, OURS)
        ));
        assert!(
            !pan_status_is_ours(Some(ours), Some(OURS), &pan_status(0x4000_0001, SMARTSDR)),
            "another pan's status must not repoint us at it"
        );
    }

    /// A REFUSED create is not a grant — the same rule the DAX TX create already follows.
    #[test]
    fn a_pan_is_owned_only_from_a_successful_create() {
        assert_eq!(created_pan_id(0, "0x40000000"), Some(0x4000_0000));
        assert_eq!(
            created_pan_id(0x5000_0015, "0x40000000"),
            None,
            "an error code with an id-shaped body is still a refusal"
        );
        assert_eq!(created_pan_id(0, ""), None);
    }

    #[test]
    fn fft_bins_normalize_to_unit_range() {
        let row = fft_to_row(&[0, 32768, 65535]);
        assert_eq!(row[0], 0.0);
        assert!((row[1] - 0.5).abs() < 0.01);
        assert_eq!(row[2], 1.0);
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
}
