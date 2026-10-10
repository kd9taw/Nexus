//! Window geometry persistence — the SHELL half, for the main window and (below
//! [`capture_now`]) the Connect dashboard pop-out.
//!
//! Bug #10 (1.0.1, Kubuntu 26.04 AppImage): the operator sets a custom UI scale for a
//! 4K HiDPI panel, drags the window to fit it, quits — and the next launch restores the
//! scale but opens the window at the `tauri.conf.json` default box. Nothing persisted
//! the window at all, so the same corner got re-dragged every day.
//!
//! The POLICY lives in [`tempo_app::window_geometry`], deliberately free of Tauri and
//! fully unit-tested: what to write on close, and what to apply at launch given the
//! monitors attached *right now*. This module is the adapter — it reads the live window,
//! transcribes the monitor list, and applies the answer. It holds no arithmetic beyond
//! the physical→logical divide, on purpose: the interesting cases (a monitor unplugged
//! between sessions, a smaller screen than last time, mixed DPI) cannot be reproduced in
//! CI with a real window, so they are decided where they *can* be tested.
//!
//! **The rule this satisfies** (CLAUDE.md, "UI layout contract"): *anything persisted
//! that encodes a size/position/scale is CLAMPED ON LOAD against the current
//! window/monitors, not just at drag time.* The band map shipped the other way once and
//! reopened fully off-screen after a monitor was unplugged; its only rescue command
//! lived ON the window nobody could reach. The main window has no such command at all,
//! so here an unreachable window is terminal — [`restore`] drops a position that lands
//! nowhere and centres instead, every time, and that is the whole reason the check is on
//! the LOAD path rather than the drag path.
//!
//! Sibling: `sanitize_free_bandmap_rect` in `lib.rs` does this job for the torn-off band
//! map. The two are deliberately not merged — that one is entangled with the band map's
//! dock state, and unifying them would mean moving dock policy too.
//!
//! **The third window is the Connect dashboard**: Connect's pop-out opens at a
//! dashboard size and reopens on the monitor and in the place the operator left it. It takes
//! the MAIN window's policy — the same [`geom::restore`] against the monitors attached now and
//! the same [`geom::capture`] on close — not the band map's, so the band map's dock state is
//! left where it is. Its record adds one thing the main window has no use for: whether the
//! operator asked it to stay behind other windows ([`PanelRecord::behind`]).

use crate::chains::{panel_key, Instance};
use tauri::{Manager, WindowEvent};
use tempo_app::window_geometry::{self as geom, WindowGeometry, WorkArea};

/// Where the record lives: `<config_dir>/window-main.json`, a sibling of `settings.json`
/// and so automatically PER-PROFILE — a window bound to a second radio remembers its own
/// box instead of fighting the first one's. Same shape as `bandmap_window_path`.
fn geometry_path() -> std::path::PathBuf {
    crate::settings_path().with_file_name("window-main.json")
}

/// Transcribe a Tauri monitor into the policy layer's [`WorkArea`].
///
/// `work_area()` — not `size()`/`position()` — because the taskbar/panel/dock is not
/// usable window space: capping against the full monitor rect is how a restored window
/// ends up with its bottom edge under the panel. Physical px throughout, plus the
/// monitor's OWN scale factor, because that is what the policy layer needs to hit-test a
/// logical point the way tao does (each candidate scaled by its own DPI, first match
/// wins).
fn work_area_of(m: &tauri::Monitor) -> WorkArea {
    let wa = m.work_area();
    WorkArea {
        x: wa.position.x as f64,
        y: wa.position.y as f64,
        w: wa.size.width as f64,
        h: wa.size.height as f64,
        scale: m.scale_factor(),
    }
}

/// The monitors attached right now, and the primary (where a dropped position re-centres).
/// Both best-effort: a monitor API failure yields an empty list, which [`geom::restore`]
/// reads as "cap against nothing, floor at the minimum, centre" — never as a reason to
/// replay a stale rect.
fn monitors(app: &tauri::AppHandle) -> (Vec<WorkArea>, Option<WorkArea>) {
    let all = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(work_area_of)
        .collect();
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| work_area_of(&m));
    (all, primary)
}

/// The window's live box in LOGICAL px, with its maximised flag.
///
/// `inner_size` is the content box (what `set_size` sets) and `outer_position` the frame
/// origin (what `set_position` sets), so each round-trips on its own axis. Both come back
/// physical and are divided by this window's scale factor — the same conversion
/// `capture_bandmap_window` uses. Under mixed DPI that logical position is re-resolved by
/// tao against each monitor's own DPI on the way back in, which is exactly why
/// [`geom::restore`] validates per monitor rather than trusting this one scale factor.
fn live_geometry(window: &tauri::WebviewWindow) -> Option<WindowGeometry> {
    let scale = window.scale_factor().unwrap_or(1.0);
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let (Ok(size), Ok(pos)) = (window.inner_size(), window.outer_position()) else {
        return None;
    };
    Some(WindowGeometry {
        w: size.width as f64 / scale,
        h: size.height as f64 / scale,
        x: pos.x as f64 / scale,
        y: pos.y as f64 / scale,
        maximized: window.is_maximized().unwrap_or(false),
    })
}

/// Apply the saved box to the main window, clamped against the monitors attached NOW.
///
/// Called from `setup()`, where the window still has `"visible": false` from
/// `tauri.conf.json` — the box is in place before the operator ever sees the window, so
/// there is no open-then-jump. (That hidden window is also why the size is applied before
/// `center()`: centring the default box and *then* resizing would leave it off-centre.)
fn restore(app: &tauri::AppHandle, window: &tauri::WebviewWindow) {
    let (all, primary) = monitors(app);
    let Some(r) = geom::restore(geom::load(&geometry_path()), &all, primary, geom::MIN_INNER)
    else {
        // First launch, no file, or a corrupt one: leave the window exactly as the shell
        // built it. `tauri.conf.json` already centres it at its default size, and this
        // module deliberately never invents a second default.
        return;
    };
    let _ = window.set_size(tauri::LogicalSize::new(r.w, r.h));
    match r.position {
        Some((x, y)) => {
            let _ = window.set_position(tauri::LogicalPosition::new(x, y));
        }
        // The saved top-left lands on no attached monitor — the unplugged-monitor case.
        // Centre instead of replaying a rect nothing displays.
        None => {
            let _ = window.center();
        }
    }
    if r.maximized {
        // Best-effort on a not-yet-mapped window: some window managers only honour a
        // maximise request once the window is shown. Failing here degrades to the restore
        // rect — the right size, on screen, un-maximised — never to a lost window.
        let _ = window.maximize();
    }
}

/// Snapshot the window on close so the next launch reopens where it was.
///
/// On close, not on every `Resized`/`Moved`: one write per session instead of one per
/// drag frame, matching `capture_bandmap_window`. The two cases the policy layer refuses
/// to write — a minimised window (Windows parks it near -32000,-32000) and a maximised
/// one (whose `inner_size` is the maximised box, not the restore rect) — are decided in
/// [`geom::capture`], with the previous record read back here as its fallback.
fn capture(window: &tauri::WebviewWindow) {
    let Some(cur) = live_geometry(window) else {
        return;
    };
    let path = geometry_path();
    let minimized = window.is_minimized().unwrap_or(false);
    if let Some(g) = geom::capture(geom::load(&path), cur, minimized) {
        let _ = geom::save(&path, &g);
    }
}

/// Restore the main window's geometry and arm the save-on-close.
///
/// One call from `setup()` is the whole shell footprint. The close handler is registered
/// on the window itself rather than added to the app-wide `on_window_event` so that all
/// of this stays here — the app-wide handler already has a band-map job and a
/// quit-cascade job, and a third concern in it would be a third reason to edit that file.
///
/// Not covered, and cheaply: a process that dies without a close event (a native crash —
/// see `main.rs`'s crash reporter — or a session logout) keeps the previous record. And
/// `choose_radio`'s `app.exit(0)` relaunch skips it too, which is correct: that is the
/// radio picker, a window the operator sees for a moment and never sizes (`quit_cleanup`
/// preserves the skip via its geometry flag).
pub fn install(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    restore(app, &window);
    let w = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::CloseRequested { .. }) {
            capture(&w);
        }
    });
}

/// Snapshot the main window's geometry right now, if it still exists — the QUIT-path
/// capture, called from `quit_cleanup` in `lib.rs`.
///
/// Exists because macOS Cmd+Q never sends `CloseRequested` to any window (NSApp
/// `terminate:` tears the app down without a per-window close), so the handler
/// [`install`] arms was never reached and a Mac operator who always quit with Cmd+Q got
/// bug #10 back: every launch at the `tauri.conf.json` default box. On the window-close
/// path the windows are already destroyed by the time the quit events fire, so the lookup
/// misses and this is a no-op — `CloseRequested` captured the geometry moments earlier.
pub fn capture_now(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        capture(&window);
    }
}

// ---- The Connect dashboard pop-out ---------------------------------------------------------

/// Which pop-outs remember their window: the Connect dashboard and the contest logger. The
/// dashboard is the one a station leaves up all day, on a second monitor or full screen behind
/// Nexus, and "it comes back where I put it" is half of what makes it a dashboard; the logger
/// lives on the second monitor its logger sits at, and opens there again. The band map
/// remembers through its own dock-aware path (`load_bandmap_window` in `lib.rs`); every other
/// pop-out opens at its fixed default, as it always has.
pub(crate) fn remembers(slug: &str) -> bool {
    slug == "connect" || slug == crate::CONTEST_LOGGER_SLUG
}

/// A remembered pop-out's record: its box — the main window's own shape, restored and
/// captured by the main window's own policy — and whether it stays behind other windows.
///
/// Flattened, so the file reads as `window-main.json` with one more key. `behind` defaults to
/// false, so a record without it loads as an ordinary window.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct PanelRecord {
    #[serde(flatten)]
    pub geom: WindowGeometry,
    /// The operator asked this window to stay behind other windows ([`set_window_behind`]).
    #[serde(default)]
    pub behind: bool,
}

/// Where a remembered pop-out's record lives: `<config_dir>/window-<slug>.json`, a sibling of
/// `settings.json` and so per profile, like `window-main.json`. `None` for a surface that must
/// never persist its geometry — see `Instance::persists_geometry` (recycled `w<n>` ids).
fn panel_path(slug: &str, inst: Instance) -> Option<std::path::PathBuf> {
    if !inst.persists_geometry() {
        return None;
    }
    let name = match inst {
        Instance::Main => format!("window-{slug}.json"),
        other => format!("window-{slug}-{other}.json"),
    };
    Some(crate::settings_path().with_file_name(name))
}

/// Read a record. `None` for missing, unreadable or unparsable, each of which means "nothing
/// saved" — [`restore_from`] already handles that.
fn load_panel(path: &std::path::Path) -> Option<PanelRecord> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Write a record: a plain write, for the reason `geom::save` gives.
fn save_panel(path: &std::path::Path, rec: &PanelRecord) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string(rec).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// What a remembered pop-out's record says about opening it now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PanelRestore {
    /// The box to open at, clamped against the monitors attached now. `None` on a first open,
    /// or for a record with no believable box: the caller opens at its default size instead,
    /// fitted to the work area and centred.
    pub rect: Option<geom::Restored>,
    /// The operator asked it to stay behind other windows.
    pub behind: bool,
}

/// The open decision, pure. `behind` survives a record with no believable box: the toggle can
/// be pressed before the window has ever been closed, so before any box was saved.
fn restore_from(
    rec: Option<PanelRecord>,
    monitors: &[WorkArea],
    primary: Option<WorkArea>,
    min: (f64, f64),
) -> PanelRestore {
    PanelRestore {
        rect: geom::restore(rec.map(|r| r.geom), monitors, primary, min),
        behind: rec.is_some_and(|r| r.behind),
    }
}

/// What to write on close, pure: the main window's rule for the box (minimised writes nothing,
/// maximised keeps the restore rect), with the stay-behind choice carried over. A close is not
/// a vote on it.
fn capture_from(
    prev: Option<PanelRecord>,
    cur: WindowGeometry,
    minimized: bool,
) -> Option<PanelRecord> {
    let geom = geom::capture(prev.map(|r| r.geom), cur, minimized)?;
    Some(PanelRecord {
        geom,
        behind: prev.is_some_and(|r| r.behind),
    })
}

/// What opening `slug` should do, from its record and the monitors attached now — or `None`
/// for a pop-out that does not remember its window.
pub(crate) fn restore_panel(
    app: &tauri::AppHandle,
    slug: &str,
    inst: Instance,
    min: (f64, f64),
) -> Option<PanelRestore> {
    if !remembers(slug) {
        return None;
    }
    let (all, primary) = monitors(app);
    let rec = panel_path(slug, inst).and_then(|p| load_panel(&p));
    Some(restore_from(rec, &all, primary, min))
}

/// The record path of a window that is a remembered pop-out, else `None`.
fn record_of(window: &tauri::WebviewWindow) -> Option<std::path::PathBuf> {
    let (slug, inst) = panel_key(window.label())?;
    if !remembers(slug) {
        return None;
    }
    panel_path(slug, inst)
}

/// Snapshot a remembered pop-out's box. A no-op for any other window, so it is safe to sweep
/// every window with it, which is what the quit path does.
pub(crate) fn capture_panel(window: &tauri::WebviewWindow) {
    let Some(path) = record_of(window) else {
        return;
    };
    let Some(cur) = live_geometry(window) else {
        return;
    };
    let minimized = window.is_minimized().unwrap_or(false);
    if let Some(rec) = capture_from(load_panel(&path), cur, minimized) {
        let _ = save_panel(&path, &rec);
    }
}

/// Arm the save-on-close on a remembered pop-out, on the window itself — [`install`]'s shape,
/// so the app-wide handler in `lib.rs` gains no new job. The main window's quit cascade closes
/// pop-outs with `close()`, which sends `CloseRequested` first, so that close is caught too.
pub(crate) fn arm_panel_capture(window: &tauri::WebviewWindow) {
    let w = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::CloseRequested { .. }) {
            capture_panel(&w);
        }
    });
}

// ---- The Remote stations window ------------------------------------------------------------

// The fourth window, and the fourth record: the hosted Remote page in a window of its own
// (`crate::remote_window`). It takes the main window's policy, as the Connect dashboard does, with
// its own minimum, plus one rule of its own: a window closed in FULL SCREEN writes nothing. Full
// screen is the monitor's box, not one the operator chose; stored, it would reopen as a borderless
// monitor-sized window that is no longer full screen. The last windowed box stays, as it does for
// a minimised close.

/// `<config_dir>/window-remote-stations.json`, beside the other records and so per profile.
fn remote_path() -> std::path::PathBuf {
    crate::settings_path().with_file_name("window-remote-stations.json")
}

/// The open decision, pure: the record at `path` clamped against the monitors attached now.
/// `None` on a first open or for a record with no believable box: the caller opens at its
/// default size, fitted to the work area and centred.
fn restore_remote_from(
    path: &std::path::Path,
    monitors: &[WorkArea],
    primary: Option<WorkArea>,
    min: (f64, f64),
) -> Option<geom::Restored> {
    geom::restore(geom::load(path), monitors, primary, min)
}

/// What to write on close, pure: the main window's rule, and nothing at all from full screen.
fn capture_remote_from(
    prev: Option<WindowGeometry>,
    cur: WindowGeometry,
    minimized: bool,
    fullscreen: bool,
) -> Option<WindowGeometry> {
    if fullscreen {
        return None;
    }
    geom::capture(prev, cur, minimized)
}

/// What opening the Remote stations window should do, against the monitors attached now.
pub(crate) fn restore_remote(app: &tauri::AppHandle, min: (f64, f64)) -> Option<geom::Restored> {
    let (all, primary) = monitors(app);
    restore_remote_from(&remote_path(), &all, primary, min)
}

/// Snapshot the Remote stations window's box. A no-op for any other window, so the quit path
/// can sweep every window with it.
pub(crate) fn capture_remote(window: &tauri::WebviewWindow) {
    if window.label() != crate::remote_window::LABEL {
        return;
    }
    let Some(cur) = live_geometry(window) else {
        return;
    };
    let path = remote_path();
    let minimized = window.is_minimized().unwrap_or(false);
    let fullscreen = window.is_fullscreen().unwrap_or(false);
    if let Some(g) = capture_remote_from(geom::load(&path), cur, minimized, fullscreen) {
        let _ = geom::save(&path, &g);
    }
}

/// Arm the save-on-close on the Remote stations window, on the window itself
/// ([`arm_panel_capture`]'s shape). Closing the main window closes this one with `close()`, which
/// sends `CloseRequested` first, so that close is caught too.
pub(crate) fn arm_remote_capture(window: &tauri::WebviewWindow) {
    let w = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::CloseRequested { .. }) {
            capture_remote(&w);
        }
    });
}

/// Whether "stay behind other windows" is offered on this platform: Windows only, for now.
///
/// tao's Windows implementation holds the window at the bottom of the z-order on every
/// position change (`WM_WINDOWPOSCHANGING` → `HWND_BOTTOM`), so a click on it activates it
/// without lifting it over the window beside it. On Linux it is a hint to the window manager
/// (`_NET_WM_STATE_BELOW`), which some honour and a Wayland session ignores; on macOS it is a
/// window level. Neither of those has been seen to work on a real desktop, so neither is
/// offered until it has.
pub(crate) fn behind_supported() -> bool {
    cfg!(windows)
}

/// A remembered pop-out whose record says it stays behind. Such a window is never focused by
/// `open_panel_window`: focus would move the keyboard to a window drawn underneath the one the
/// operator is looking at.
pub(crate) fn stays_behind(window: &tauri::WebviewWindow) -> bool {
    behind_supported()
        && record_of(window)
            .and_then(|p| load_panel(&p))
            .is_some_and(|r| r.behind)
}

/// The calling window's stay-behind state, for the dashboard bar's toggle: whether it can
/// stay behind at all (a remembered pop-out on a platform that offers it), and whether it does.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct WindowBehind {
    supported: bool,
    on: bool,
}

/// Read the calling window's stay-behind state.
#[tauri::command]
pub fn get_window_behind(window: tauri::WebviewWindow) -> WindowBehind {
    let supported = behind_supported() && record_of(&window).is_some();
    WindowBehind {
        supported,
        on: supported && stays_behind(&window),
    }
}

/// Keep the calling window behind other windows, or let it come forward again: applied to the
/// window now and written to its record, so it opens that way next time. The box in the record
/// is left as it was — only a close writes that.
#[tauri::command]
pub fn set_window_behind(window: tauri::WebviewWindow, on: bool) -> Result<WindowBehind, String> {
    if !behind_supported() {
        return Err("staying behind other windows is not available on this platform".into());
    }
    let path = record_of(&window)
        .ok_or_else(|| "only the Connect window can stay behind other windows".to_string())?;
    // The record first: a write that fails then changes nothing, and the toggle stays true to
    // both the window and what the next open will do.
    let mut rec = load_panel(&path).unwrap_or_default();
    rec.behind = on;
    save_panel(&path, &rec).map_err(|e| e.to_string())?;
    window.set_always_on_bottom(on).map_err(|e| e.to_string())?;
    Ok(WindowBehind {
        supported: true,
        on,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record must sit beside `settings.json`, which is what makes it per-profile:
    /// a window bound to a second radio has its own config dir and so its own box. A
    /// path built from `config_base()` directly would put every profile's window in one
    /// file and the radios would fight over it.
    #[test]
    fn the_record_is_a_per_profile_sibling_of_settings() {
        let p = geometry_path();
        assert_eq!(p.file_name().unwrap(), "window-main.json");
        assert_eq!(p.parent(), crate::settings_path().parent());
    }

    /// The floor handed to the policy layer must be the one the shell actually enforces
    /// (`set_min_size` in `lib.rs`, `minWidth`/`minHeight` in `tauri.conf.json`). If they
    /// drift, a restore asks for a window smaller than the operator is allowed to drag
    /// to and the window manager silently overrides it — a size that never restores.
    #[test]
    fn the_restore_floor_matches_the_shells_minimum_window() {
        assert_eq!(geom::MIN_INNER, (900.0, 600.0));
    }

    // ---- the Connect dashboard pop-out ---------------------------------------------------

    /// The pop-out's floor, `panel_min_inner("connect")` — the generic arm.
    const DASH_MIN: (f64, f64) = (420.0, 360.0);

    fn area(x: f64, y: f64, w: f64, h: f64, scale: f64) -> WorkArea {
        WorkArea { x, y, w, h, scale }
    }

    /// A 1920×1080 primary at the origin with a 40 px taskbar.
    fn primary() -> WorkArea {
        area(0.0, 0.0, 1920.0, 1040.0, 1.0)
    }

    fn rec(w: f64, h: f64, x: f64, y: f64, behind: bool) -> PanelRecord {
        PanelRecord {
            geom: WindowGeometry {
                w,
                h,
                x,
                y,
                maximized: false,
            },
            behind,
        }
    }

    #[test]
    fn the_connect_dashboard_and_the_contest_logger_remember_their_windows() {
        assert!(remembers("connect"));
        assert!(remembers("contestlog"));
        // The band map remembers through its own dock-aware file; the rest keep their defaults,
        // the contest scoreboard beside the logger among them.
        for other in [
            "bandmapCw",
            "bandmapPhone",
            "needed",
            "operate",
            "waterfall",
            "fieldday",
            "fdclub",
            "",
        ] {
            assert!(!remembers(other), "{other:?} must keep its fixed default");
        }
    }

    /// Beside `settings.json`, so per profile like `window-main.json`, and never for a
    /// recycled `w<n>` surface, which would hand a stranger's box to the next one.
    #[test]
    fn the_dashboard_record_is_a_per_profile_sibling_of_settings() {
        let p = panel_path("connect", Instance::Main).unwrap();
        assert_eq!(p.file_name().unwrap(), "window-connect.json");
        assert_eq!(p.parent(), crate::settings_path().parent());
        assert_ne!(
            p,
            geometry_path(),
            "the pop-out must not share the main window's file"
        );
        assert_eq!(
            panel_path("connect", Instance::Radio(2))
                .unwrap()
                .file_name()
                .unwrap(),
            "window-connect-r2.json"
        );
        assert_eq!(panel_path("connect", Instance::Window(3)), None);
    }

    #[test]
    fn a_first_open_leaves_the_box_to_the_caller() {
        // Nothing saved: the caller opens the dashboard size, fitted and centred.
        let r = restore_from(None, &[primary()], Some(primary()), DASH_MIN);
        assert_eq!(
            r,
            PanelRestore {
                rect: None,
                behind: false
            }
        );
    }

    #[test]
    fn a_saved_box_reopens_on_its_own_monitor_in_its_own_place() {
        // Left on a second 2560×1440 monitor to the right of the primary.
        let second = area(1920.0, 0.0, 2560.0, 1400.0, 1.0);
        let saved = rec(2400.0, 1300.0, 2000.0, 40.0, false);
        let r = restore_from(Some(saved), &[primary(), second], Some(primary()), DASH_MIN);
        let rect = r.rect.expect("a believable box restores");
        assert_eq!(rect.position, Some((2000.0, 40.0)));
        assert_eq!(
            (rect.w, rect.h),
            (2400.0, 1300.0),
            "capped to ITS monitor, not the primary"
        );
    }

    #[test]
    fn a_box_whose_monitor_is_gone_is_centred_on_the_primary_and_fitted_to_it() {
        let saved = rec(2400.0, 1300.0, 2000.0, 40.0, false);
        let rect = restore_from(Some(saved), &[primary()], Some(primary()), DASH_MIN)
            .rect
            .unwrap();
        assert_eq!(rect.position, None, "centred, never replayed off-screen");
        assert_eq!((rect.w, rect.h), (1920.0, 1040.0));
    }

    #[test]
    fn a_tiny_saved_box_is_floored_at_the_pop_outs_own_minimum() {
        // Not the main window's 900×600: a restore must never ask for a box the operator could
        // not drag this window to, in either direction.
        let rect = restore_from(
            Some(rec(300.0, 250.0, 40.0, 40.0, false)),
            &[primary()],
            Some(primary()),
            DASH_MIN,
        )
        .rect
        .unwrap();
        assert_eq!((rect.w, rect.h), DASH_MIN);
    }

    #[test]
    fn stay_behind_is_remembered_even_before_any_box_was_saved() {
        // The toggle is pressed on a window that has never been closed: the record carries
        // behind and a zero box, which must open at the default size AND behind.
        let r = restore_from(
            Some(PanelRecord {
                geom: WindowGeometry::default(),
                behind: true,
            }),
            &[primary()],
            Some(primary()),
            DASH_MIN,
        );
        assert_eq!(r.rect, None, "a zero box is not a box");
        assert!(r.behind);
    }

    #[test]
    fn a_close_records_the_box_and_keeps_the_stay_behind_choice() {
        let prev = rec(1600.0, 1000.0, 100.0, 20.0, true);
        let cur = WindowGeometry {
            w: 1700.0,
            h: 950.0,
            x: 60.0,
            y: 30.0,
            maximized: false,
        };
        assert_eq!(
            capture_from(Some(prev), cur, false),
            Some(PanelRecord {
                geom: cur,
                behind: true
            })
        );
        // …and a first close, with no record yet, is not behind.
        assert_eq!(
            capture_from(None, cur, false),
            Some(PanelRecord {
                geom: cur,
                behind: false
            })
        );
    }

    #[test]
    fn a_close_while_minimised_or_maximised_follows_the_main_windows_rule() {
        let prev = rec(1600.0, 1000.0, 100.0, 20.0, true);
        let parked = WindowGeometry {
            w: 1600.0,
            h: 1000.0,
            x: -32000.0,
            y: -32000.0,
            maximized: false,
        };
        assert_eq!(
            capture_from(Some(prev), parked, true),
            None,
            "minimised writes nothing"
        );
        let maxed = WindowGeometry {
            w: 1920.0,
            h: 1040.0,
            x: 0.0,
            y: 0.0,
            maximized: true,
        };
        let out = capture_from(Some(prev), maxed, false).unwrap();
        assert_eq!(
            (out.geom.w, out.geom.h, out.geom.x, out.geom.y),
            (1600.0, 1000.0, 100.0, 20.0),
            "the restore rect is kept"
        );
        assert!(out.geom.maximized && out.behind);
    }

    #[test]
    fn a_record_round_trips_and_one_without_behind_loads_as_an_ordinary_window() {
        let path = std::env::temp_dir().join(format!(
            "nexus_panelrec_{}_{}.json",
            std::process::id(),
            line!()
        ));
        let r = rec(1512.0, 945.5, -8.0, 24.0, true);
        save_panel(&path, &r).unwrap();
        assert_eq!(load_panel(&path), Some(r));
        std::fs::write(
            &path,
            r#"{"w":1400,"h":900,"x":10,"y":20,"maximized":true}"#,
        )
        .unwrap();
        let legacy = load_panel(&path).unwrap();
        assert!(!legacy.behind);
        assert!(legacy.geom.maximized);
        std::fs::write(&path, "{\"w\": 1200,").unwrap();
        assert_eq!(load_panel(&path), None, "a torn file is nothing saved");
        let _ = std::fs::remove_file(&path);
    }

    // ---- the Remote stations window ----------------------------------------------------------

    const REMOTE_MIN: (f64, f64) = crate::remote_window::MIN_INNER;

    fn windowed(w: f64, h: f64, x: f64, y: f64) -> WindowGeometry {
        WindowGeometry {
            w,
            h,
            x,
            y,
            maximized: false,
        }
    }

    /// A scratch record file of its own per test, written with `g` (or absent for `None`).
    fn remote_record(tag: u32, g: Option<WindowGeometry>) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("nexus_remoterec_{}_{tag}.json", std::process::id()));
        match g {
            Some(g) => geom::save(&path, &g).unwrap(),
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        path
    }

    #[test]
    fn the_remote_record_is_a_per_profile_sibling_of_settings_and_nobody_elses() {
        let p = remote_path();
        assert_eq!(p.file_name().unwrap(), "window-remote-stations.json");
        assert_eq!(p.parent(), crate::settings_path().parent());
        assert_ne!(p, geometry_path());
        assert_ne!(Some(p), panel_path("connect", Instance::Main));
    }

    /// Remembered, and clamped ON LOAD to the monitors attached at that moment — the rule this
    /// module exists for. Read through the window's own record file, as an open reads it.
    #[test]
    fn the_remote_window_reopens_where_it_was_clamped_to_the_monitors_attached_now() {
        let second = area(1920.0, 0.0, 2560.0, 1400.0, 1.0);
        let path = remote_record(1, Some(windowed(2400.0, 1300.0, 2000.0, 40.0)));
        // Its monitor still attached: the same place and the same box.
        let r = restore_remote_from(&path, &[primary(), second], Some(primary()), REMOTE_MIN)
            .expect("a believable box restores");
        assert_eq!(r.position, Some((2000.0, 40.0)));
        assert_eq!((r.w, r.h), (2400.0, 1300.0));
        // That monitor unplugged since: centred on the primary and fitted to it, never replayed
        // off-screen.
        let r = restore_remote_from(&path, &[primary()], Some(primary()), REMOTE_MIN).unwrap();
        assert_eq!(r.position, None, "centred");
        assert_eq!((r.w, r.h), (1920.0, 1040.0), "fitted to the work area");
        // A box too small to be believed opens at the default; a small believable one is held
        // up to the window's own minimum.
        let path = remote_record(2, Some(windowed(300.0, 250.0, 40.0, 40.0)));
        let r = restore_remote_from(&path, &[primary()], Some(primary()), REMOTE_MIN).unwrap();
        assert_eq!((r.w, r.h), REMOTE_MIN);
        let path = remote_record(3, Some(windowed(120.0, 90.0, 40.0, 40.0)));
        assert_eq!(
            restore_remote_from(&path, &[primary()], Some(primary()), REMOTE_MIN),
            None
        );
        // Nothing saved, or a torn file: the caller's default.
        let path = remote_record(4, None);
        assert_eq!(
            restore_remote_from(&path, &[primary()], Some(primary()), REMOTE_MIN),
            None
        );
        std::fs::write(&path, "{\"w\": 1280,").unwrap();
        assert_eq!(
            restore_remote_from(&path, &[primary()], Some(primary()), REMOTE_MIN),
            None
        );
        for tag in 1..=4 {
            let _ = std::fs::remove_file(remote_record(tag, None));
        }
    }

    #[test]
    fn a_remote_window_closed_full_screen_reopens_at_its_last_windowed_box() {
        let before = windowed(1280.0, 800.0, 100.0, 80.0);
        let full = windowed(1920.0, 1080.0, 0.0, 0.0);
        assert_eq!(
            capture_remote_from(Some(before), full, false, true),
            None,
            "full screen writes nothing"
        );
        // So the record still holds the windowed box, and the next open restores it.
        let path = remote_record(5, Some(before));
        if let Some(g) = capture_remote_from(geom::load(&path), full, false, true) {
            geom::save(&path, &g).unwrap();
        }
        let r = restore_remote_from(&path, &[primary()], Some(primary()), REMOTE_MIN).unwrap();
        assert_eq!((r.w, r.h, r.position), (1280.0, 800.0, Some((100.0, 80.0))));
        let _ = std::fs::remove_file(&path);
        // A windowed close is recorded as the main window's is: the box, never a minimised park
        // rect, and a maximised close keeps the box under it.
        let moved = windowed(1400.0, 900.0, 300.0, 120.0);
        assert_eq!(
            capture_remote_from(Some(before), moved, false, false),
            Some(moved)
        );
        let parked = windowed(1400.0, 900.0, -32000.0, -32000.0);
        assert_eq!(capture_remote_from(Some(before), parked, true, false), None);
        let maxed = WindowGeometry {
            maximized: true,
            ..windowed(1920.0, 1040.0, 0.0, 0.0)
        };
        assert_eq!(
            capture_remote_from(Some(before), maxed, false, false),
            Some(WindowGeometry {
                maximized: true,
                ..before
            })
        );
    }
}
