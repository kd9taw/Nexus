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
    let Some(r) = geom::restore_in_frame(
        geom::load(&geometry_path()),
        &all,
        primary,
        geom::MIN_INNER,
        frame_of(window),
    ) else {
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
        maximize_hidden(app, window);
    }
}

/// The window's frame — title bar and borders, outer size less inner — in logical px, so the
/// restore can fit the WHOLE window on the monitor rather than only its content. The window is
/// built (hidden, decorated) by the time [`restore`] runs, so both sizes are its own; `(0, 0)`
/// where either cannot be read, which fits the content as before.
fn frame_of(window: &tauri::WebviewWindow) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let scale = if scale > 0.0 { scale } else { 1.0 };
    match (window.outer_size(), window.inner_size()) {
        (Ok(outer), Ok(inner)) => (
            f64::from(outer.width.saturating_sub(inner.width)) / scale,
            f64::from(outer.height.saturating_sub(inner.height)) / scale,
        ),
        _ => (0.0, 0.0),
    }
}

/// Maximise the main window while it is still hidden, without the operator ever seeing it.
///
/// On Windows a plain `maximize()` shows it: tao maximises with `ShowWindow(SW_MAXIMIZE)`,
/// which also shows a hidden window, and only then hides it again with `SW_HIDE`. A launch
/// that restored a maximised window drew an empty full-screen frame around the splash for a
/// moment, then took it away until the app was ready. So the window is cloaked
/// (`DWMWA_CLOAK`: Windows keeps it, nothing draws it) for that one call and uncloaked
/// straight after, still hidden and now maximised — the show at the end of the launch is
/// untouched. All three run on the main thread, where tao applies the maximise as it is
/// called, so the uncloak cannot overtake it; and if the window handle is unavailable the
/// maximise is made as before, flash and all, never skipped.
#[cfg(windows)]
fn maximize_hidden(app: &tauri::AppHandle, window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
    let Ok(hwnd) = window.hwnd() else {
        let _ = window.maximize();
        return;
    };
    let hwnd = hwnd.0 as isize;
    let cloak = move |on: bool| {
        let on = i32::from(on);
        // SAFETY: `hwnd` is the live main window's handle, and the attribute is a BOOL read
        // from a local that outlives the call.
        unsafe {
            DwmSetWindowAttribute(
                hwnd as _,
                DWMWA_CLOAK as u32,
                &on as *const i32 as *const _,
                std::mem::size_of::<i32>() as u32,
            );
        }
    };
    let w = window.clone();
    let sent = app.run_on_main_thread(move || {
        cloak(true);
        let _ = w.maximize();
        cloak(false);
    });
    if sent.is_err() {
        let _ = window.maximize();
    }
}

/// Elsewhere a maximise request on the hidden window is made as it always was.
#[cfg(not(windows))]
fn maximize_hidden(_app: &tauri::AppHandle, window: &tauri::WebviewWindow) {
    let _ = window.maximize();
}

/// Snapshot the window on close so the next launch reopens where it was.
///
/// On close, not on every `Resized`/`Moved`: one write per session instead of one per
/// drag frame, matching `capture_bandmap_window`. The two cases the policy layer refuses
/// to write — a minimised window (Windows parks it near -32000,-32000) and a maximised
/// one (whose `inner_size` is the maximised box, not the restore rect) — are decided in
/// [`geom::capture`], with the previous record read back here as its fallback.
fn capture(window: &tauri::WebviewWindow) {
    // A window in a streamed page's shape records the box it will go back to, not that shape.
    let Some(cur) = followed_from().or_else(|| live_geometry(window)) else {
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

// ---- The main window in a streamed page's shape --------------------------------------------

/// What the main window was before it took a streamed page's shape, to go back to.
struct Before {
    /// The box [`capture`] records while the window is in the page's shape.
    geometry: Option<WindowGeometry>,
    fullscreen: bool,
    maximized: bool,
    /// The restored box, once out of full screen and maximised: physical px.
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    /// The monitor it was on, which it never leaves, and its frame (outer size less inner).
    work: WorkArea,
    frame: (f64, f64),
}

/// Set from the first shape a stream gives the window until it goes back. Process-wide, because
/// the close path's [`capture`] reads it as well as the stream's thread.
static FOLLOWED: std::sync::Mutex<Option<Before>> = std::sync::Mutex::new(None);

fn followed() -> std::sync::MutexGuard<'static, Option<Before>> {
    FOLLOWED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn followed_from() -> Option<WindowGeometry> {
    followed().as_ref().and_then(|before| before.geometry)
}

/// The main window taking a streamed page's shape ([`geom::follow`]) and going back. Its calls
/// come from the stream's own thread; each window call is carried to the event loop in order.
pub(crate) struct MainShape(pub tauri::AppHandle);

impl crate::remote_service::stream::ShapeWindow for MainShape {
    fn follow(&self, view: (u32, u32)) {
        let Some(window) = self.0.get_webview_window("main") else {
            return;
        };
        // A minimized window shows the page nothing; it is left where the operator put it.
        if window.is_minimized().unwrap_or(false) {
            return;
        }
        let mut before = followed();
        if before.is_none() {
            let Some(monitor) = window.current_monitor().ok().flatten() else {
                return;
            };
            let geometry = live_geometry(&window);
            let fullscreen = window.is_fullscreen().unwrap_or(false);
            if fullscreen {
                let _ = window.set_fullscreen(false);
            }
            let maximized = window.is_maximized().unwrap_or(false);
            if maximized {
                let _ = window.unmaximize();
            }
            let (Ok(position), Ok(size), Ok(outer)) = (
                window.outer_position(),
                window.inner_size(),
                window.outer_size(),
            ) else {
                if maximized {
                    let _ = window.maximize();
                }
                if fullscreen {
                    let _ = window.set_fullscreen(true);
                }
                return;
            };
            *before = Some(Before {
                geometry,
                fullscreen,
                maximized,
                position,
                size,
                work: work_area_of(&monitor),
                frame: (
                    f64::from(outer.width.saturating_sub(size.width)),
                    f64::from(outer.height.saturating_sub(size.height)),
                ),
            });
        }
        let Some(before) = before.as_ref() else {
            return;
        };
        if let Some(to) = geom::follow(view, &before.work, before.frame, geom::MIN_INNER) {
            let _ = window.set_size(tauri::PhysicalSize::new(to.w, to.h));
            let _ = window.set_position(tauri::PhysicalPosition::new(to.x, to.y));
        }
    }

    fn restore(&self) {
        let Some(before) = followed().take() else {
            return;
        };
        let Some(window) = self.0.get_webview_window("main") else {
            return;
        };
        // Minimized since: put back behind the minimize, so restoring it later gives its own box.
        let minimized = window.is_minimized().unwrap_or(false);
        if minimized {
            let _ = window.unminimize();
        }
        let _ = window.set_size(before.size);
        let _ = window.set_position(before.position);
        if before.maximized {
            let _ = window.maximize();
        }
        if before.fullscreen {
            let _ = window.set_fullscreen(true);
        }
        if minimized {
            let _ = window.minimize();
        }
    }
}

// ---- The Connect dashboard pop-out ---------------------------------------------------------

/// Which pop-outs remember their window: the Connect dashboard, the contest logger and the
/// contest screen. The dashboard is the one a station leaves up all day, on a second monitor or
/// full screen behind Nexus, and "it comes back where I put it" is half of what makes it a
/// dashboard; the logger lives on the second monitor its logger sits at, and opens there again;
/// the contest screen is popped out to put it on another monitor, and goes back there. The band
/// map remembers through its own dock-aware path (`load_bandmap_window` in `lib.rs`); every other
/// pop-out opens at its fixed default, as it always has.
pub(crate) fn remembers(slug: &str) -> bool {
    slug == "connect" || slug == crate::CONTEST_LOGGER_SLUG || slug == crate::CONTEST_SCREEN_SLUG
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
    /// The operator pinned this window in front of other windows ([`set_window_on_top`]).
    #[serde(default)]
    pub on_top: bool,
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
    /// The operator pinned it in front of other windows.
    pub on_top: bool,
}

/// The open decision, pure. `behind` and `on_top` survive a record with no believable box: the
/// toggle can be pressed before the window has ever been closed, so before any box was saved. A
/// record that says both (a hand edit) opens pinned: a window in front cannot also be behind.
fn restore_from(
    rec: Option<PanelRecord>,
    monitors: &[WorkArea],
    primary: Option<WorkArea>,
    min: (f64, f64),
) -> PanelRestore {
    let on_top = rec.is_some_and(|r| r.on_top);
    PanelRestore {
        rect: geom::restore(rec.map(|r| r.geom), monitors, primary, min),
        behind: !on_top && rec.is_some_and(|r| r.behind),
        on_top,
    }
}

/// What to write on close, pure: the main window's rule for the box (minimised writes nothing,
/// maximised keeps the restore rect), with the stay-behind and pin choices carried over. A close
/// is not a vote on either.
fn capture_from(
    prev: Option<PanelRecord>,
    cur: WindowGeometry,
    minimized: bool,
) -> Option<PanelRecord> {
    let geom = geom::capture(prev.map(|r| r.geom), cur, minimized)?;
    Some(PanelRecord {
        geom,
        behind: prev.is_some_and(|r| r.behind),
        on_top: prev.is_some_and(|r| r.on_top),
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
    let prev = load_panel(&path).unwrap_or_default();
    let rec = kept_behind(prev, on);
    save_panel(&path, &rec).map_err(|e| e.to_string())?;
    if prev.on_top && !rec.on_top {
        window.set_always_on_top(false).map_err(|e| e.to_string())?;
    }
    window.set_always_on_bottom(on).map_err(|e| e.to_string())?;
    Ok(WindowBehind {
        supported: true,
        on,
    })
}

/// The record with stay-behind set or cleared, pure. A window kept behind is not also pinned in
/// front, so keeping it behind takes the pin off.
fn kept_behind(rec: PanelRecord, on: bool) -> PanelRecord {
    PanelRecord {
        behind: on,
        on_top: rec.on_top && !on,
        ..rec
    }
}

// ---- The pin: in front of other windows ----------------------------------------------------

/// Which pop-outs offer the pin: the contest logger, which a logger drops beside a program or a
/// corner of a screen and keeps in front while working in something else (the operator's ask from
/// a contest tester). Only a remembered pop-out can offer it, since the record is where it lives.
pub(crate) fn pins(slug: &str) -> bool {
    slug == crate::CONTEST_LOGGER_SLUG && remembers(slug)
}

/// The record with the pin set or cleared, pure. A window pinned in front is not also behind, so
/// pinning it takes stay-behind off. The box is left as it was: only a close writes that.
fn pinned(rec: PanelRecord, on: bool) -> PanelRecord {
    PanelRecord {
        on_top: on,
        behind: rec.behind && !on,
        ..rec
    }
}

/// The record path of a window that offers the pin, else `None`.
fn pin_record_of(window: &tauri::WebviewWindow) -> Option<std::path::PathBuf> {
    let (slug, _) = panel_key(window.label())?;
    if !pins(slug) {
        return None;
    }
    record_of(window)
}

/// The calling window's pin, for the logger's Pin button: whether it offers one, and whether it
/// is on.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct WindowOnTop {
    supported: bool,
    on: bool,
}

/// Read the calling window's pin.
#[tauri::command]
pub fn get_window_on_top(window: tauri::WebviewWindow) -> WindowOnTop {
    let path = pin_record_of(&window);
    WindowOnTop {
        supported: path.is_some(),
        on: path.and_then(|p| load_panel(&p)).is_some_and(|r| r.on_top),
    }
}

/// Pin the calling window in front of other windows, Nexus's and other programs', or unpin it to
/// stack as an ordinary window again: applied to the window now and written to its record, so it
/// opens that way next time.
#[tauri::command]
pub fn set_window_on_top(window: tauri::WebviewWindow, on: bool) -> Result<WindowOnTop, String> {
    let path = pin_record_of(&window)
        .ok_or_else(|| "only the contest logger window can be pinned in front".to_string())?;
    // The record first, as `set_window_behind` does: a write that fails changes nothing.
    let prev = load_panel(&path).unwrap_or_default();
    let rec = pinned(prev, on);
    save_panel(&path, &rec).map_err(|e| e.to_string())?;
    if prev.behind && !rec.behind && behind_supported() {
        window
            .set_always_on_bottom(false)
            .map_err(|e| e.to_string())?;
    }
    window.set_always_on_top(on).map_err(|e| e.to_string())?;
    Ok(WindowOnTop {
        supported: true,
        on,
    })
}

/// Bring the calling window to the front and give it the keyboard: un-minimised, shown and focused.
/// The main window asks this for itself when a contest window sends the operator to a cockpit in it
/// ("Work this contest in CW"): the screen they asked for is then the one in front of them, not one
/// left behind the contest window. Nothing at the radio; only this window's place on the desktop.
#[tauri::command]
pub fn focus_window(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_minimized().unwrap_or(false) {
        window.unminimize().map_err(|e| e.to_string())?;
    }
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

/// Flash the calling window's taskbar button (macOS: bounce the dock icon once) until the operator
/// looks at it, without taking the keyboard from whatever they are in. The FT8/FT4 strip asks this
/// once when the TX watchdog stops a call, as WSJT-X alerts its window on the same trip. Nothing at
/// the radio; no effect when the window already has focus.
#[tauri::command]
pub fn request_window_attention(window: tauri::WebviewWindow) -> Result<(), String> {
    window
        .request_user_attention(Some(tauri::UserAttentionType::Informational))
        .map_err(|e| e.to_string())
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

    /// A launch that restores a maximised main window must not show it before the launch does.
    /// tao maximises a hidden window with `ShowWindow(SW_MAXIMIZE)`, which shows it, and hides it
    /// again only afterwards: on Windows the operator saw an empty full-screen frame flash
    /// around the splash. So [`restore`] maximises through [`maximize_hidden`], which on Windows
    /// cloaks the window for exactly that call. Source-read, because what is under test is the
    /// order of calls on a real window, which no test here can make.
    #[test]
    fn a_maximised_restore_maximises_the_hidden_window_cloaked() {
        let src = include_str!("window_state.rs");
        let body = |sig: &str| {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig} is defined"));
            let rest = &src[start..];
            &rest[..rest.find("\n}\n").expect("the end of the function")]
        };
        let restore = body("\nfn restore(");
        assert!(
            !restore.contains(".maximize()"),
            "restore never maximises the hidden window itself"
        );
        assert!(
            restore.contains("maximize_hidden(app, window)"),
            "it maximises through maximize_hidden"
        );
        let windows = body("#[cfg(windows)]\nfn maximize_hidden(");
        let at = |needle: &str| {
            windows
                .find(needle)
                .unwrap_or_else(|| panic!("`{needle}` in maximize_hidden"))
        };
        assert!(
            at("cloak(true)") < at("let _ = w.maximize();")
                && at("let _ = w.maximize();") < at("cloak(false)"),
            "on Windows the maximise happens inside the cloak, and the cloak is lifted after it"
        );
        assert!(
            at("run_on_main_thread(") < at("cloak(true)"),
            "all three on the main thread, where each runs as it is called"
        );
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
            on_top: false,
        }
    }

    #[test]
    fn the_connect_dashboard_and_the_contest_logger_remember_their_windows() {
        assert!(remembers("connect"));
        assert!(remembers("contestlog"));
        // …and the contest screen window, which goes to a monitor of its own as the logger does.
        assert!(remembers("contestscreen"));
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
                behind: false,
                on_top: false
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
                on_top: false,
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
                behind: true,
                on_top: false
            })
        );
        // …and a first close, with no record yet, is not behind.
        assert_eq!(
            capture_from(None, cur, false),
            Some(PanelRecord {
                geom: cur,
                behind: false,
                on_top: false
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

    // ---- the contest logger's pin -------------------------------------------------------------

    /// The logger window's own floor, `panel_min_inner("contestlog")`.
    const LOGGER_MIN: (f64, f64) = (420.0, 320.0);

    #[test]
    fn the_contest_logger_drags_down_to_its_documented_minimum() {
        // 420×320: the call, the exchange boxes, Log, the dupe warning and the newest contact still
        // on screen (measured in Chrome, `scripts/browser-probe --desktop contestlog`).
        assert_eq!(
            crate::panel_min_inner(crate::CONTEST_LOGGER_SLUG),
            LOGGER_MIN
        );
        assert!(
            remembers(crate::CONTEST_LOGGER_SLUG),
            "it remembers its box"
        );
    }

    #[test]
    fn the_contest_logger_is_the_window_that_offers_the_pin() {
        assert!(pins(crate::CONTEST_LOGGER_SLUG));
        for other in [
            "connect",
            "contestscreen",
            "fieldday",
            "waterfall",
            "operate",
            "",
        ] {
            assert!(!pins(other), "{other:?} offers no pin");
        }
    }

    #[test]
    fn a_logger_saved_off_screen_comes_back_on_screen() {
        // Left on a monitor to the left of the primary that is gone now.
        let saved = rec(700.0, 500.0, -1500.0, 200.0, false);
        let rect = restore_from(Some(saved), &[primary()], Some(primary()), LOGGER_MIN)
            .rect
            .unwrap();
        assert_eq!(
            rect.position, None,
            "centred on the primary, never replayed off-screen"
        );
        assert_eq!((rect.w, rect.h), (700.0, 500.0), "its own size kept");
        // …and one shrunk under the floor by a hand edit opens at the floor, not smaller.
        let tiny = restore_from(
            Some(rec(300.0, 250.0, 40.0, 40.0, false)),
            &[primary()],
            Some(primary()),
            LOGGER_MIN,
        )
        .rect
        .unwrap();
        assert_eq!((tiny.w, tiny.h), LOGGER_MIN);
        assert_eq!(tiny.position, Some((40.0, 40.0)));
    }

    #[test]
    fn the_pin_is_remembered_and_reopens_the_window_in_front() {
        let pin = pinned(rec(700.0, 500.0, 40.0, 40.0, false), true);
        assert!(pin.on_top);
        assert_eq!(
            (pin.geom.w, pin.geom.h),
            (700.0, 500.0),
            "the box is left as it was"
        );
        let r = restore_from(Some(pin), &[primary()], Some(primary()), LOGGER_MIN);
        assert!(r.on_top && !r.behind);
        // A close keeps it…
        let closed = capture_from(Some(pin), pin.geom, false).unwrap();
        assert!(closed.on_top);
        // …and a pin pressed before any close, on a zero box, opens pinned at the default size.
        let first = pinned(PanelRecord::default(), true);
        let r = restore_from(Some(first), &[primary()], Some(primary()), LOGGER_MIN);
        assert_eq!((r.rect, r.on_top), (None, true));
        // Unpinned, it reopens as an ordinary window.
        let off = pinned(pin, false);
        assert!(!restore_from(Some(off), &[primary()], Some(primary()), LOGGER_MIN).on_top);
    }

    #[test]
    fn in_front_and_behind_never_hold_together() {
        let behind = rec(700.0, 500.0, 40.0, 40.0, true);
        let pin = pinned(behind, true);
        assert_eq!(
            (pin.on_top, pin.behind),
            (true, false),
            "pinning takes stay-behind off"
        );
        assert_eq!(
            (pinned(behind, false).on_top, pinned(behind, false).behind),
            (false, true)
        );
        let back = kept_behind(pin, true);
        assert_eq!(
            (back.on_top, back.behind),
            (false, true),
            "staying behind takes the pin off"
        );
        assert!(
            kept_behind(pin, false).on_top,
            "letting it forward keeps the pin"
        );
        // A hand-edited record saying both opens pinned, not both.
        let both = PanelRecord {
            on_top: true,
            ..behind
        };
        let r = restore_from(Some(both), &[primary()], Some(primary()), LOGGER_MIN);
        assert_eq!((r.on_top, r.behind), (true, false));
    }

    #[test]
    fn a_record_without_the_pin_loads_unpinned_and_a_pinned_one_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "nexus_panelrec_{}_{}.json",
            std::process::id(),
            line!()
        ));
        let r = pinned(rec(700.0, 500.0, 40.0, 40.0, false), true);
        save_panel(&path, &r).unwrap();
        assert_eq!(load_panel(&path), Some(r));
        std::fs::write(
            &path,
            r#"{"w":700,"h":500,"x":40,"y":40,"maximized":false,"behind":false}"#,
        )
        .unwrap();
        assert!(!load_panel(&path).unwrap().on_top);
        let _ = std::fs::remove_file(&path);
    }
}
