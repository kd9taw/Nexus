//! Main-window geometry persistence — the SHELL half.
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
//! dock state, and unifying them would mean moving dock policy too. If a third window
//! ever needs this, that is the moment.

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
}
