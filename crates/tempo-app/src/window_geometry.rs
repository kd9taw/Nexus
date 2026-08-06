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
//! that one lives behind Tauri types, this one is testable. If a third window ever
//! needs it, that is the moment to unify them here.

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
    if let Some(m) = hit.or(primary) {
        let (lw, lh) = m.logical_size();
        w = w.min(lw);
        h = h.min(lh);
    }
    // Floor LAST, so it always wins: a work area smaller than the window minimum (or a
    // small-but-plausible saved rect) can never produce a window the operator is not
    // even allowed to drag to. Overhang on a tiny screen is the window manager's problem
    // and is what `min_inner_size` would do anyway; a sub-minimum window is ours.
    w = w.max(min.0);
    h = h.max(min.1);
    Some(Restored {
        w,
        h,
        position: hit.map(|_| (g.x, g.y)),
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
            Some((2800.0, 100.0))
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
}
