//! Main-window geometry persistence — size, position and maximised state.
//!
//! The operator's report (bug #10, 1.0.1, Kubuntu 26.04 AppImage): set a custom UI
//! scale for a 4K HiDPI panel, resize the window to fit it, quit — and the next launch
//! restores the scale but not the window. Nothing persisted the window box at all, so
//! every session started at the `tauri.conf.json` default and the operator re-dragged
//! the same corner every day.
//!
//! This module is the POLICY half, deliberately free of Tauri: what to write on close
//! ([`capture`]) and what to apply on launch ([`restore`]), plus the on-disk record
//! ([`WindowGeometry`], [`load`], [`save`]). The shell supplies the live monitor list
//! and applies the answer. Being pure is what makes the interesting cases — a monitor
//! unplugged between sessions, a smaller screen than last time, mixed DPI — testable
//! at all; none of them can be reproduced in CI with a real window.
//!
//! **The rule this exists to satisfy** (CLAUDE.md, "UI layout contract"): *anything
//! persisted that encodes a size/position/scale is CLAMPED ON LOAD against the current
//! window/monitors, not just at drag time.* A verbatim replay of a saved rect is the
//! shipped bug it was written for — the band map reopened fully off-screen after a
//! monitor was unplugged, and the only in-app command that could have moved it back
//! lived ON the window nobody could reach. The main window has no such command at all,
//! so here the failure is terminal: an operator who cannot see the window cannot fix it.
//!
//! **UI scale is not part of this record.** `--ui-zoom` persists separately
//! (`ui/src/useScale.ts`) and the geometry below is LOGICAL px, which is the unit the
//! zoom is applied *inside*. The two compose without either knowing about the other:
//! the shell restores the box before the webview paints, and `useViewport`/`useScale`
//! recompute from the real `innerWidth`/`innerHeight` they are then handed.
//!
//! Sibling: `sanitize_free_bandmap_rect` in `src-tauri/src/lib.rs` does the same job
//! for the torn-off band map, and the coordinate reasoning below is lifted from its
//! header (tao resolves a logical position by hit-testing each candidate monitor
//! scaled by THAT monitor's DPI, first match wins). The two are separate on purpose:
//! that one lives behind Tauri types, this one is testable.
//!
//! **The third window takes this policy, not the band map's**: the Connect dashboard
//! pop-out (`src-tauri/src/window_state.rs`) is restored and captured by these same two
//! functions, with its own minimum passed to [`restore`]. The band map keeps its own path
//! because it is entangled with its dock state.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The shell's minimum inner size, in logical px. **Must match `set_min_size` in
/// `src-tauri/src/lib.rs` and `minWidth`/`minHeight` in `tauri.conf.json`** — pass it
/// to [`restore`] rather than re-typing the numbers, so a restore can never ask for a
/// window smaller than the one the operator is allowed to drag to.
///
/// Note this is NOT the 1024×768 *supported floor* from CLAUDE.md: that is the size
/// the layout is designed for, this is the size the window is physically bounded at.
pub const MIN_INNER: (f64, f64) = (900.0, 600.0);

/// Reachability margin in LOGICAL px — how much of the window must remain on a work
/// area for its saved position to be believed. See [`lands_on`] for why the band is
/// asymmetric.
const MARGIN: f64 = 32.0;

/// Smallest rect worth believing at all. Below this the record is corrupt, a
/// transient 0×0 from a window manager, or the ≈-32000 park position Windows gives a
/// minimised window — none of which should displace a good rect or size a window.
const PLAUSIBLE_MIN: f64 = 200.0;

/// The persisted main-window record (logical px), a tiny sibling of `settings.json`.
///
/// `w`/`h`/`x`/`y` are always the **restore rect** — the box the window returns to when
/// un-maximised — never the maximised box. [`capture`] is what keeps that true.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct WindowGeometry {
    pub w: f64,
    pub h: f64,
    pub x: f64,
    pub y: f64,
    /// Whether the window was left maximised. `#[serde(default)]` so a record written
    /// before this field existed still loads (as not-maximised).
    #[serde(default)]
    pub maximized: bool,
}

/// One monitor's usable region — the work area, which EXCLUDES the taskbar/panel/dock,
/// unlike the full monitor size — in **physical** px, plus that monitor's own scale
/// factor. Mirrors `tauri::Monitor::work_area()` + `scale_factor()` one for one so the
/// shell adapter is a transcription with no arithmetic in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkArea {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

impl WorkArea {
    /// This work area's size in LOGICAL px — the unit a saved rect is in.
    fn logical_size(&self) -> (f64, f64) {
        if self.scale > 0.0 {
            (self.w / self.scale, self.h / self.scale)
        } else {
            // A monitor reporting a non-positive scale factor is a driver bug, not a
            // reason to divide by zero and hand the shell a NaN window size.
            (self.w, self.h)
        }
    }
}

/// What the shell should actually apply to the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restored {
    /// Inner size in logical px — capped at the target monitor's work area, floored at
    /// the shell minimum.
    pub w: f64,
    pub h: f64,
    /// `Some(logical top-left)` to place it there; `None` means **centre it** — the
    /// saved position no longer lands on any attached monitor.
    pub position: Option<(f64, f64)>,
    /// Re-maximise after the size/position are applied, so un-maximising later gives
    /// back the restore rect above rather than the maximised box.
    pub maximized: bool,
}

/// A rect worth believing: finite, and not degenerate. Rejects the minimised-window
/// park rect, a torn/hand-edited file, and a 0×0 first-fire.
fn is_plausible(g: &WindowGeometry) -> bool {
    g.w.is_finite()
        && g.h.is_finite()
        && g.x.is_finite()
        && g.y.is_finite()
        && g.w >= PLAUSIBLE_MIN
        && g.h >= PLAUSIBLE_MIN
}

/// True ⇔ a window whose top-left is at `g`'s logical position leaves a grabbable
/// sliver of itself on `m`'s work area.
///
/// Validated in **physical** px against THIS monitor's scale, mirroring how tao
/// resolves a logical position (hit-test each candidate scaled by its own DPI, first
/// match wins). Under mixed DPI one logical point names a different physical point per
/// monitor, so trusting any single scale factor would mis-place the window on exactly
/// the multi-monitor setups this check exists for.
///
/// The band is INFLATED by the margin past the left/top edges — a window snapped to a
/// screen edge parks its outer position a few px off-screen (Windows' invisible resize
/// borders) and must still restore in place — but DEFLATED by it at the right/bottom:
/// the window extends right and down from its top-left, so a top-left past those edges
/// puts nothing of it on this monitor.
fn lands_on(g: &WindowGeometry, m: &WorkArea) -> bool {
    let sf = if m.scale > 0.0 { m.scale } else { 1.0 };
    let (px, py) = (g.x * sf, g.y * sf);
    let mg = MARGIN * sf;
    px >= m.x - mg && px <= m.x + m.w - mg && py >= m.y - mg && py <= m.y + m.h - mg
}

/// Decide what to APPLY at launch, given the monitors attached **right now**.
///
/// `None` ⇒ there is nothing usable to restore; leave the window exactly as the shell
/// built it (`tauri.conf.json` centres it at its default size). That is the answer for
/// a first launch, a missing file, and a corrupt one alike — the module never invents a
/// default size, because the default already has one home and two would drift.
///
/// `primary` is where a window whose saved position was dropped will be centred; its
/// work area is therefore what caps the size in that case.
pub fn restore(
    saved: Option<WindowGeometry>,
    monitors: &[WorkArea],
    primary: Option<WorkArea>,
    min: (f64, f64),
) -> Option<Restored> {
    restore_in_frame(saved, monitors, primary, min, (0.0, 0.0))
}

/// [`restore`] for a window whose `frame` — its title bar and borders, outer size less inner, in
/// logical px — is known: the frame counts against the work area too, so the WHOLE window fits.
///
/// Capping the inner size alone left a window a frame wider and taller than the screen: a record
/// saved maximised holds the maximised inner box (the work area less the frame) at the frame's
/// negative offset, and reopened un-maximised it ran its borders off both sides and its bottom
/// edge under the taskbar. And a box whose top-left still landed was replayed however far its
/// size carried it past the right or bottom edge. So, against the monitor it lands on: the inner
/// box is capped at the work area less the frame, and a box that would run past the right or
/// bottom edge is pulled back until its far edge, frame included, is on it (never past the left
/// or top edge, where a snapped window's few px of overhang are left as they were).
pub fn restore_in_frame(
    saved: Option<WindowGeometry>,
    monitors: &[WorkArea],
    primary: Option<WorkArea>,
    min: (f64, f64),
    frame: (f64, f64),
) -> Option<Restored> {
    // Plausibility is enforced HERE rather than in `load`, so every path into a
    // restore — file, test, future caller — goes through the same gate as the monitor
    // clamp. "Clamped on load" is one place or it is nowhere.
    let g = saved.filter(is_plausible)?;
    // The monitor the saved top-left still lands on, if any. Its absence is the
    // unplugged-monitor case: the position is dropped rather than replayed off-screen.
    let hit = monitors.iter().copied().find(|m| lands_on(&g, m));
    let (mut w, mut h) = (g.w, g.h);
    // Cap at the work area of the monitor the window will actually open on — the one it
    // landed on, or the primary it is about to be centred on. A rect saved on a 1440-tall
    // screen must not reopen overhanging a 768-tall laptop. If no monitor resolves at all
    // (headless, API error) leave the size as saved; the shell's `min_inner_size` and the
    // window manager are still downstream of us.
    let (fx, fy) = (frame.0.max(0.0), frame.1.max(0.0));
    if let Some(m) = hit.or(primary) {
        let (lw, lh) = m.logical_size();
        w = w.min(lw - fx);
        h = h.min(lh - fy);
    }
    // Floor LAST, so it always wins: a work area smaller than the window minimum (or a
    // small-but-plausible saved rect) can never produce a window the operator is not
    // even allowed to drag to. Overhang on a tiny screen is the window manager's problem
    // and is what `min_inner_size` would do anyway; a sub-minimum window is ours.
    w = w.max(min.0);
    h = h.max(min.1);
    // Pulled back from the right and bottom edges of the monitor it landed on, in that monitor's
    // logical px (the unit `lands_on` reads the saved point in). Only ever towards the top-left
    // and never past that edge, so a box that is simply too big for the screen (the floor above
    // beat the cap) sits at its top-left corner rather than off the other side.
    let position = hit.map(|m| {
        let sf = if m.scale > 0.0 { m.scale } else { 1.0 };
        let (left, top) = (m.x / sf, m.y / sf);
        let (lw, lh) = m.logical_size();
        let pull = |at: f64, edge: f64, room: f64, size: f64| {
            let furthest = edge + room - size;
            if at > furthest {
                furthest.max(edge)
            } else {
                at
            }
        };
        (pull(g.x, left, lw, w + fx), pull(g.y, top, lh, h + fy))
    });
    Some(Restored {
        w,
        h,
        position,
        maximized: g.maximized,
    })
}

/// Decide what to WRITE, given the window's live state.
///
/// `cur` is the live rect with `cur.maximized` set from the window; `prev` is whatever
/// is already on disk. `None` ⇒ write nothing, the record on disk stays.
///
/// Two cases earn the whole function:
///
/// * **Minimised.** A minimised window reports a bogus rect (Windows parks it near
///   -32000,-32000), and a close-while-minimised — including the quit cascade — would
///   persist exactly that. The last good rect stays instead.
/// * **Maximised.** `inner_size()` on a maximised window is the *maximised* box. Storing
///   it as the restore rect would make the next un-maximise a no-op — the window would
///   "un-maximise" to full screen, forever. Keep the previous restore rect and record
///   only the flag. With no previous record (first-ever quit, maximised) the maximised
///   box is all there is; it is then capped to a work area on restore, so un-maximising
///   yields a work-area-sized window — large, but usable and self-correcting the first
///   time the operator resizes and quits un-maximised.
pub fn capture(
    prev: Option<WindowGeometry>,
    cur: WindowGeometry,
    minimized: bool,
) -> Option<WindowGeometry> {
    if minimized {
        return None;
    }
    if cur.maximized {
        let base = prev.filter(is_plausible).unwrap_or(cur);
        if !is_plausible(&base) {
            return None;
        }
        return Some(WindowGeometry {
            maximized: true,
            ..base
        });
    }
    is_plausible(&cur).then_some(cur)
}

/// The box the main window takes while a streamed page shows it: the inner size and the outer
/// top-left, in the monitor's **physical** px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Followed {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// The main window in the shape of a streamed page's picture area of `view` (only its shape is
/// read): the largest inner box of that shape that, with the window's `frame` (outer size less
/// inner, physical px) round it, fits `work`, centred there. `work` is the monitor the window was
/// on when the stream began, so the window never leaves it. A shape past the shell minimum
/// ([`MIN_INNER`], logical) is floored there, as the window manager would anyway, and the page
/// shows the picture with bars on that side; nothing is ever cut off the picture. `None` for a
/// view with no area, or a work area with no room for the frame.
pub fn follow(
    view: (u32, u32),
    work: &WorkArea,
    frame: (f64, f64),
    min_inner: (f64, f64),
) -> Option<Followed> {
    let (vw, vh) = (f64::from(view.0), f64::from(view.1));
    let room_w = (work.w - frame.0).floor();
    let room_h = (work.h - frame.1).floor();
    // Written so a NaN anywhere fails it.
    if !(vw >= 1.0 && vh >= 1.0 && room_w >= 1.0 && room_h >= 1.0) {
        return None;
    }
    // Whichever side of the room the shape reaches first, compared cross-multiplied.
    let (w, h) = if room_w * vh <= room_h * vw {
        (room_w, (room_w * vh / vw).round())
    } else {
        ((room_h * vw / vh).round(), room_h)
    };
    let scale = if work.scale > 0.0 { work.scale } else { 1.0 };
    let w = w.max((min_inner.0 * scale).ceil()).min(room_w);
    let h = h.max((min_inner.1 * scale).ceil()).min(room_h);
    Some(Followed {
        x: (work.x + ((work.w - w - frame.0) / 2.0).floor()) as i32,
        y: (work.y + ((work.h - h - frame.1) / 2.0).floor()) as i32,
        w: w as u32,
        h: h as u32,
    })
}

/// Read the record at `path`. `None` for missing, unreadable or unparsable — every one
/// of which means "no saved geometry", which [`restore`] already handles. Sanity is
/// [`restore`]'s job, not this function's.
pub fn load(path: &Path) -> Option<WindowGeometry> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Persist the record to `path` (creating parent directories). Plain write, not the
/// atomic tmp+rename [`crate::settings::Settings::save`] uses: this file is five
/// numbers, is rewritten on every close, and a torn one costs one session's window
/// placement — `restore` turns any unparsable content into "no saved geometry".
/// Buying durability here would not be worth an fsync on the quit path.
pub fn save(path: &Path, g: &WindowGeometry) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string(g).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1920×1080 primary at the origin with a 40 px taskbar, scale 1.
    fn primary() -> WorkArea {
        WorkArea {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1040.0,
            scale: 1.0,
        }
    }

    /// A 1366×768 laptop panel with a 40 px panel — "smaller monitor than last time".
    fn laptop() -> WorkArea {
        WorkArea {
            x: 0.0,
            y: 0.0,
            w: 1366.0,
            h: 728.0,
            scale: 1.0,
        }
    }

    fn geom(w: f64, h: f64, x: f64, y: f64) -> WindowGeometry {
        WindowGeometry {
            w,
            h,
            x,
            y,
            maximized: false,
        }
    }

    // ---- restore: the ordinary case -------------------------------------------------

    #[test]
    fn nothing_saved_leaves_the_window_alone() {
        assert_eq!(
            restore(None, &[primary()], Some(primary()), MIN_INNER),
            None
        );
    }

    #[test]
    fn a_rect_that_still_fits_restores_verbatim() {
        let g = geom(1600.0, 900.0, 120.0, 60.0);
        let r = restore(Some(g), &[primary()], Some(primary()), MIN_INNER).unwrap();
        assert_eq!(
            r,
            Restored {
                w: 1600.0,
                h: 900.0,
                position: Some((120.0, 60.0)),
                maximized: false,
            }
        );
    }

    // ---- restore: the cases that make it exist --------------------------------------

    #[test]
    fn an_unplugged_monitor_drops_the_position_instead_of_going_off_screen() {
        // Saved on a second monitor to the right of the primary; that monitor is gone.
        let g = geom(1400.0, 900.0, 2400.0, 200.0);
        let r = restore(Some(g), &[primary()], Some(primary()), MIN_INNER).unwrap();
        assert_eq!(r.position, None, "centre it — nothing displays that point");
        assert_eq!((r.w, r.h), (1400.0, 900.0));
    }

    #[test]
    fn a_monitor_to_the_left_still_counts_as_on_screen() {
        // Negative coordinates are ordinary on a left-hand secondary; they are not
        // "off-screen" and must not be thrown away.
        let left = WorkArea {
            x: -1920.0,
            y: 0.0,
            w: 1920.0,
            h: 1040.0,
            scale: 1.0,
        };
        let g = geom(1200.0, 800.0, -1800.0, 100.0);
        let r = restore(Some(g), &[primary(), left], Some(primary()), MIN_INNER).unwrap();
        assert_eq!(r.position, Some((-1800.0, 100.0)));
    }

    #[test]
    fn a_smaller_monitor_caps_the_size_and_keeps_the_position() {
        // Saved full-screen-ish on the 1920×1080; today only the 1366×768 laptop.
        let g = geom(1920.0, 1040.0, 0.0, 0.0);
        let r = restore(Some(g), &[laptop()], Some(laptop()), MIN_INNER).unwrap();
        assert_eq!((r.w, r.h), (1366.0, 728.0));
        assert_eq!(r.position, Some((0.0, 0.0)));
    }

    #[test]
    fn a_dropped_position_is_capped_against_the_primary_it_will_be_centred_on() {
        // Position lands nowhere, so the window opens centred on the primary — and must
        // be capped against THAT work area, not left at its old monitor's size.
        let g = geom(1920.0, 1040.0, 4000.0, 2000.0);
        let r = restore(Some(g), &[laptop()], Some(laptop()), MIN_INNER).unwrap();
        assert_eq!(r.position, None);
        assert_eq!((r.w, r.h), (1366.0, 728.0));
    }

    #[test]
    fn the_shell_minimum_floors_the_restore() {
        // Plausible but small (a hand-edited file, or a work area below the minimum).
        let g = geom(300.0, 250.0, 40.0, 40.0);
        let r = restore(Some(g), &[primary()], Some(primary()), MIN_INNER).unwrap();
        assert_eq!((r.w, r.h), MIN_INNER);
    }

    #[test]
    fn the_floor_beats_the_cap_on_a_work_area_below_the_minimum() {
        let tiny = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 480.0,
            scale: 1.0,
        };
        let g = geom(1600.0, 900.0, 10.0, 10.0);
        let r = restore(Some(g), &[tiny], Some(tiny), MIN_INNER).unwrap();
        assert_eq!(
            (r.w, r.h),
            MIN_INNER,
            "never restore a window smaller than the one the operator can drag to"
        );
    }

    #[test]
    fn a_degenerate_record_is_ignored_entirely() {
        assert_eq!(
            restore(
                Some(geom(50.0, 40.0, 0.0, 0.0)),
                &[primary()],
                Some(primary()),
                MIN_INNER
            ),
            None
        );
    }

    #[test]
    fn a_non_finite_record_is_ignored_entirely() {
        for bad in [
            geom(f64::NAN, 900.0, 0.0, 0.0),
            geom(1600.0, f64::INFINITY, 0.0, 0.0),
            geom(1600.0, 900.0, f64::NAN, 0.0),
            geom(1600.0, 900.0, 0.0, f64::NAN),
        ] {
            assert_eq!(
                restore(Some(bad), &[primary()], Some(primary()), MIN_INNER),
                None,
                "{bad:?} must not reach a set_size/set_position call"
            );
        }
    }

    #[test]
    fn no_resolvable_monitor_still_floors_the_size() {
        // Headless / monitor API failure: nothing to cap against, but the floor holds.
        let r = restore(Some(geom(300.0, 250.0, 0.0, 0.0)), &[], None, MIN_INNER).unwrap();
        assert_eq!((r.w, r.h), MIN_INNER);
        assert_eq!(r.position, None, "no monitor landed → centre");
    }

    // ---- restore: the whole box on the monitor ----------------------------------------

    /// A box whose top-left lands but whose size runs it past the right and bottom edges opens
    /// pulled back onto the monitor, the same size: a window wider or taller than the screen it
    /// is on, with its controls off the edge, is what the clamp exists to prevent.
    #[test]
    fn a_box_that_runs_past_the_right_and_bottom_edges_is_pulled_back_on() {
        let g = geom(1600.0, 900.0, 1000.0, 400.0);
        let r = restore(Some(g), &[primary()], Some(primary()), MIN_INNER).unwrap();
        assert_eq!((r.w, r.h), (1600.0, 900.0));
        assert_eq!(r.position, Some((320.0, 140.0)));
    }

    /// The window's frame (its title bar and borders: outer size less inner) is part of the box
    /// the monitor has to hold. Capping only the INNER size at the work area left a window that
    /// had been saved maximised (the maximised box, at the frame's negative offset) reopening a
    /// frame wider and taller than the screen whenever it opened un-maximised.
    #[test]
    fn the_frame_counts_against_the_work_area() {
        let frame = (16.0, 39.0);
        // Saved maximised on this very screen: the maximised inner box at its outer offset.
        let g = WindowGeometry {
            maximized: true,
            ..geom(1920.0, 1009.0, -8.0, -8.0)
        };
        let r = restore_in_frame(Some(g), &[primary()], Some(primary()), MIN_INNER, frame).unwrap();
        assert_eq!(
            (r.w, r.h),
            (1920.0 - 16.0, 1040.0 - 39.0),
            "inner + frame = the work area"
        );
        assert_eq!(
            r.position,
            Some((-8.0, -8.0)),
            "a snapped corner stays where it was"
        );
        assert!(r.maximized);
        // Its far edges are on the monitor: right = x + w + frame, bottom = y + h + frame.
        let (x, y) = r.position.unwrap();
        assert!(x + r.w + frame.0 <= 1920.0 && y + r.h + frame.1 <= 1040.0);
        // Moved right, the same box comes back flush with the right edge, frame and all.
        let g = geom(1904.0, 1001.0, 200.0, 100.0);
        let r = restore_in_frame(Some(g), &[primary()], Some(primary()), MIN_INNER, frame).unwrap();
        assert_eq!(r.position, Some((0.0, 0.0)));
        // A box centred on the primary (its position dropped) is sized to fit there, frame included.
        let g = geom(2400.0, 1300.0, 5000.0, 100.0);
        let r = restore_in_frame(Some(g), &[laptop()], Some(laptop()), MIN_INNER, frame).unwrap();
        assert_eq!(r.position, None);
        assert_eq!((r.w, r.h), (1366.0 - 16.0, 728.0 - 39.0));
    }

    /// In logical px of the monitor it lands on: on a 150 % screen the work area is 2/3 its
    /// physical size, and the box is pulled back against that.
    #[test]
    fn the_pull_back_is_in_the_landing_monitors_logical_px() {
        let hidpi = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 2880.0,
            h: 1560.0,
            scale: 1.5,
        };
        let g = geom(1600.0, 900.0, 1000.0, 300.0);
        let r = restore(Some(g), &[hidpi], Some(hidpi), MIN_INNER).unwrap();
        assert_eq!(r.position, Some((1920.0 - 1600.0, 1040.0 - 900.0)));
    }

    // ---- restore: the grabbable-sliver boundary -------------------------------------

    #[test]
    fn a_window_parked_a_few_px_past_the_left_edge_still_restores_in_place() {
        // A screen-snapped window's outer position sits slightly negative.
        let g = geom(1200.0, 800.0, -8.0, -6.0);
        let r = restore(Some(g), &[primary()], Some(primary()), MIN_INNER).unwrap();
        assert_eq!(r.position, Some((-8.0, -6.0)));
    }

    #[test]
    fn the_last_grabbable_sliver_lands_and_one_px_further_does_not() {
        let inside = geom(1200.0, 800.0, 1920.0 - MARGIN, 100.0);
        let outside = geom(1200.0, 800.0, 1920.0 - MARGIN + 1.0, 100.0);
        assert!(
            restore(Some(inside), &[primary()], Some(primary()), MIN_INNER)
                .unwrap()
                .position
                .is_some()
        );
        assert_eq!(
            restore(Some(outside), &[primary()], Some(primary()), MIN_INNER)
                .unwrap()
                .position,
            None
        );
    }

    // ---- restore: HiDPI / mixed DPI -------------------------------------------------

    #[test]
    fn a_hidpi_work_area_caps_in_logical_px() {
        // 4K panel at 200%: 3840×2120 physical == 1920×1060 logical.
        let hidpi = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 3840.0,
            h: 2120.0,
            scale: 2.0,
        };
        let fits = restore(
            Some(geom(1800.0, 1000.0, 40.0, 40.0)),
            &[hidpi],
            Some(hidpi),
            MIN_INNER,
        )
        .unwrap();
        assert_eq!((fits.w, fits.h), (1800.0, 1000.0));

        let over = restore(
            Some(geom(3000.0, 1600.0, 40.0, 40.0)),
            &[hidpi],
            Some(hidpi),
            MIN_INNER,
        )
        .unwrap();
        assert_eq!(
            (over.w, over.h),
            (1920.0, 1060.0),
            "cap against the LOGICAL work area, not the physical pixel count"
        );
    }

    #[test]
    fn a_position_is_hit_tested_per_monitor_at_that_monitors_scale() {
        // Primary 1920×1040 @1× at the origin; a 4K secondary to its right, 3840×2120
        // PHYSICAL at 2×. That desktop runs to logical x=2880, NOT to 5760.
        //
        // x=2800 lands on the secondary and x=2900 does not, and the boundary between
        // them is the whole point: it exists only because the point is scaled by THIS
        // monitor's DPI before the hit test. Compare the raw logical number against the
        // physical work area instead and every x up to 5728 "lands" — the window reopens
        // off the right of the desktop, on exactly the HiDPI setup this was reported from.
        // (An earlier fixture used x=2000, which is inside the secondary either way and so
        // proved nothing; a scale-blind implementation passed it — found by mutation.)
        let secondary = WorkArea {
            x: 1920.0,
            y: 0.0,
            w: 3840.0,
            h: 2120.0,
            scale: 2.0,
        };
        let on = geom(1400.0, 900.0, 2800.0, 100.0);
        let off = geom(1400.0, 900.0, 2900.0, 100.0);
        assert!(!lands_on(&on, &primary()), "past the primary either way");
        assert!(lands_on(&on, &secondary));
        assert!(
            !lands_on(&off, &secondary),
            "logical 2900 is past this monitor's logical right edge (2880)"
        );
        assert_eq!(
            restore(
                Some(on),
                &[primary(), secondary],
                Some(primary()),
                MIN_INNER
            )
            .unwrap()
            .position,
            // It lands, so it is kept on the secondary, pulled back from that monitor's logical
            // right edge (2880) by its own width: 2800 + 1400 would run 1320 px off the desktop.
            Some((2880.0 - 1400.0, 100.0))
        );
        assert_eq!(
            restore(
                Some(off),
                &[primary(), secondary],
                Some(primary()),
                MIN_INNER
            )
            .unwrap()
            .position,
            None,
            "a point past the LOGICAL desktop is dropped, not replayed"
        );
    }

    #[test]
    fn a_hidpi_position_below_the_logical_work_area_is_dropped() {
        // Bug #10's own machine: one 4K panel at 200%, 3840×2120 physical == 1920×1060
        // logical. A rect saved at logical y=1500 — from a taller arrangement, or the
        // monitor this panel replaced — is BELOW the logical work area, but 1500 is
        // comfortably inside 2120 physical. A scale-blind check believes it and reopens
        // the window off the bottom with its title bar out of reach.
        let hidpi = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 3840.0,
            h: 2120.0,
            scale: 2.0,
        };
        let g = geom(1400.0, 900.0, 100.0, 1500.0);
        assert!(!lands_on(&g, &hidpi));
        assert_eq!(
            restore(Some(g), &[hidpi], Some(hidpi), MIN_INNER)
                .unwrap()
                .position,
            None,
            "centre it — logical y=1500 is off a 1060-tall logical screen"
        );
    }

    // ---- restore: maximised ---------------------------------------------------------

    #[test]
    fn a_maximised_window_restores_maximised_over_its_restore_rect() {
        let g = WindowGeometry {
            maximized: true,
            ..geom(1400.0, 900.0, 100.0, 80.0)
        };
        let r = restore(Some(g), &[primary()], Some(primary()), MIN_INNER).unwrap();
        assert!(r.maximized);
        assert_eq!((r.w, r.h), (1400.0, 900.0), "un-maximise gives this back");
        assert_eq!(r.position, Some((100.0, 80.0)));
    }

    #[test]
    fn a_maximised_window_whose_monitor_vanished_is_centred_and_still_maximised() {
        let g = WindowGeometry {
            maximized: true,
            ..geom(1400.0, 900.0, 3000.0, 100.0)
        };
        let r = restore(Some(g), &[laptop()], Some(laptop()), MIN_INNER).unwrap();
        assert!(r.maximized);
        assert_eq!(r.position, None);
        assert_eq!((r.w, r.h), (1366.0, 728.0));
    }

    // ---- capture --------------------------------------------------------------------

    #[test]
    fn an_ordinary_close_records_the_rect() {
        let cur = geom(1500.0, 950.0, 60.0, 30.0);
        assert_eq!(capture(None, cur, false), Some(cur));
    }

    #[test]
    fn closing_while_minimised_writes_nothing() {
        // Windows parks a minimised window near -32000,-32000; the quit cascade used to
        // persist exactly that for the band map.
        let parked = geom(1500.0, 950.0, -32000.0, -32000.0);
        assert_eq!(
            capture(Some(geom(1400.0, 900.0, 10.0, 10.0)), parked, true),
            None
        );
    }

    #[test]
    fn closing_while_maximised_keeps_the_previous_restore_rect() {
        let prev = geom(1400.0, 900.0, 100.0, 80.0);
        let cur = WindowGeometry {
            maximized: true,
            ..geom(1920.0, 1040.0, 0.0, 0.0)
        };
        assert_eq!(
            capture(Some(prev), cur, false),
            Some(WindowGeometry {
                maximized: true,
                ..prev
            }),
            "the maximised box must never become the restore rect"
        );
    }

    #[test]
    fn maximised_with_no_previous_record_falls_back_to_the_maximised_box() {
        let cur = WindowGeometry {
            maximized: true,
            ..geom(1920.0, 1040.0, 0.0, 0.0)
        };
        assert_eq!(capture(None, cur, false), Some(cur));
    }

    #[test]
    fn maximised_over_a_degenerate_previous_record_falls_back_too() {
        let cur = WindowGeometry {
            maximized: true,
            ..geom(1920.0, 1040.0, 0.0, 0.0)
        };
        assert_eq!(
            capture(Some(geom(0.0, 0.0, 0.0, 0.0)), cur, false),
            Some(cur)
        );
    }

    #[test]
    fn un_maximising_before_quit_clears_the_flag() {
        let prev = WindowGeometry {
            maximized: true,
            ..geom(1400.0, 900.0, 100.0, 80.0)
        };
        let cur = geom(1500.0, 950.0, 60.0, 30.0);
        assert_eq!(capture(Some(prev), cur, false), Some(cur));
    }

    #[test]
    fn a_degenerate_live_rect_writes_nothing() {
        // A 0×0 first-fire must not displace a good record.
        assert_eq!(
            capture(
                Some(geom(1400.0, 900.0, 10.0, 10.0)),
                geom(0.0, 0.0, 0.0, 0.0),
                false
            ),
            None
        );
    }

    // ---- load / save ----------------------------------------------------------------

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("nexus_wingeom_{name}_{}.json", std::process::id()))
    }

    #[test]
    fn a_saved_record_round_trips() {
        let path = tmp("roundtrip");
        let _ = std::fs::remove_file(&path);
        let g = WindowGeometry {
            maximized: true,
            ..geom(1512.0, 945.5, -8.0, 24.0)
        };
        save(&path, &g).unwrap();
        assert_eq!(load(&path), Some(g));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_or_corrupt_file_reads_as_no_geometry() {
        let path = tmp("corrupt");
        let _ = std::fs::remove_file(&path);
        assert_eq!(load(&path), None, "missing");
        std::fs::write(&path, "{\"w\": 1200, \"h\":").unwrap();
        assert_eq!(load(&path), None, "torn");
        assert_eq!(
            restore(load(&path), &[primary()], Some(primary()), MIN_INNER),
            None,
            "a corrupt file leaves the window as the shell built it"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_record_written_before_maximised_existed_still_loads() {
        let path = tmp("legacy");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, r#"{"w":1400,"h":900,"x":10,"y":20}"#).unwrap();
        assert_eq!(load(&path), Some(geom(1400.0, 900.0, 10.0, 20.0)));
        let _ = std::fs::remove_file(&path);
    }

    // ---- follow: the window in a streamed page's shape -------------------------------

    /// A decorated window's frame on Windows 11 at 100 %: the invisible 8 px resize borders on
    /// the sides and bottom, and the 31 px title bar over the top one.
    const FRAME: (f64, f64) = (16.0, 39.0);

    #[test]
    fn a_full_screen_browser_gets_the_largest_window_of_its_shape() {
        // Chrome full screen on 1920×1080, the page's header over the picture: a 1914×992 area.
        assert_eq!(
            follow((1914, 992), &primary(), FRAME, MIN_INNER),
            Some(Followed {
                x: 0,
                y: 7,
                w: 1904,
                h: 987
            })
        );
    }

    #[test]
    fn a_browser_wider_than_the_shack_is_matched_across_its_width() {
        assert_eq!(
            follow((3440, 1300), &primary(), FRAME, MIN_INNER),
            Some(Followed {
                x: 0,
                y: 140,
                w: 1904,
                h: 720
            })
        );
    }

    #[test]
    fn a_tall_view_is_matched_down_its_height_and_floored_at_the_minimum() {
        // 2560×1440 at 150 %, its taskbar 48 px; a phone held upright.
        let qhd = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 2560.0,
            h: 1392.0,
            scale: 1.5,
        };
        let frame = (24.0, 58.0);
        assert_eq!(
            follow((390, 700), &qhd, frame, MIN_INNER),
            Some(Followed {
                x: 593,
                y: 0,
                w: 1350,
                h: 1334
            }),
            "900 logical px is the narrowest the window goes"
        );
        // A 4:3 tablet on its side needs no floor: its own shape, the work area's full height.
        assert_eq!(
            follow((2048, 1536), &qhd, frame, MIN_INNER),
            Some(Followed {
                x: 378,
                y: 0,
                w: 1779,
                h: 1334
            })
        );
    }

    #[test]
    fn the_window_stays_on_the_monitor_it_was_on() {
        // A second monitor to the right of the primary, its top 200 px lower.
        let right = WorkArea {
            x: 1920.0,
            y: 200.0,
            w: 1920.0,
            h: 1040.0,
            scale: 1.0,
        };
        assert_eq!(
            follow((1914, 992), &right, FRAME, MIN_INNER),
            Some(Followed {
                x: 1920,
                y: 207,
                w: 1904,
                h: 987
            })
        );
    }

    #[test]
    fn a_view_with_no_area_or_no_room_for_the_frame_changes_nothing() {
        assert_eq!(follow((0, 992), &primary(), FRAME, MIN_INNER), None);
        assert_eq!(follow((1914, 0), &primary(), FRAME, MIN_INNER), None);
        assert_eq!(
            follow((1914, 992), &primary(), (1920.0, 39.0), MIN_INNER),
            None
        );
        assert_eq!(
            follow((1914, 992), &primary(), (f64::NAN, 39.0), MIN_INNER),
            None
        );
    }
}
