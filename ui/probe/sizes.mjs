// The screens every desktop probe draws its window sizes from — the ONE list, so the probes cannot drift apart.
//
// THE RULING (operator, 2026-11-11): support the laptop screens of the last 8–10 years and up to 4K, and no further
// down — "no one should be running 1024x768". So the matrix is:
//   screens  1366×768 (the floor), 1440×900, 1536×864, 1920×1080, 1920×1200, 2560×1440, 2560×1600, 3840×2160
//   scaling  100, 125, 150 and 175 % on every screen, and 200 % on 4K.
// 1024×768 is gone from every probe. A probe that tests the app's OWN UI zoom (Settings ▸ Appearance, 65–175 %) may
// still pin it — 65 % included — on a matrix screen; that is a different knob from the OS scaling here.
//
// HOW OS SCALING IS EMULATED, as Windows/WebView2 sees it (WebView2 is Chromium): at 175 % it lays out a CSS window
// of the client area divided by 1.75, at devicePixelRatio 1.75. `metrics()` asks Chrome for exactly that (to a px of
// rounding). macOS has no fractional ratio — its scaled modes are a looks-like CSS window at ratio 2 — so the
// 125–175 % entries are Windows'; the layout reads the CSS window, which is the same either way. In THIS harness the
// ratio reaches the canvases' backing stores and nothing else: the app's scaled auto-fit rule (useScale.ts
// `scaledFitScale`) runs only under Tauri, so a probe's 'auto' zoom is the classic fit (never smaller than the
// app's, so the tighter case). `w`/`h` below are that CSS window (what `innerWidth`/`innerHeight` read); `name` is
// the screen. The window is the whole screen. A real maximised one also loses the taskbar (48 px at 100 %, so about
// 27 CSS px at 175 %) and the title bar (about 30), roughly 13 % of the floor's 438 — and the app's own minimum
// window (900×600 logical, tauri.conf.json) is larger than the floor screen at 175 %, so the floor as drawn here is
// what the layout must hold, not yet a window the shell can open.
//
// THE REPRESENTATIVE SET (`CORE`), and how a probe picks: every probe covers the smallest effective viewport
// (`FLOOR`, 1366×768 at 175 % → 780×438 CSS px), a mid laptop, 1080p, 1440p and 4K at 200 %. A cheap probe runs
// `CORE` whole; a probe whose cost is per size (toastcorner, digishell, stopvisible, alarmcell) runs the ends and
// says which in its own header. The rest of the matrix is here for the probes that sweep it.
//
// THE SIZE CLASSES this matrix reaches (useViewport.ts classifyViewport, effective = CSS width / app zoom; at the
// app's 100 %): sm (every laptop screen at 175 %, 1366–1536 at 150 %, 1366×768 at 125 %; FLOOR is 780 px), md
// (1366–1536 at 100 %, 1080p/1200p at 125–150 %, 1440p/1600p at 175 %), lg (1080p/1200p at 100 %, 1440p/1600p at
// 125–150 %, 4K at 175–200 %), xl (1440p/1600p at 100 %, 4K at 100–150 %). `xs` (< 768) is reached by NO matrix
// entry at the app's 100 % — only by the app's own zoom on top of OS scaling, or by a window dragged smaller than
// its screen — so the probes that hold the xs layout keep an explicit sub-floor window for it.

/** One screen at one OS scaling, as the page sees it: `at(1920, 1200, 150)`. */
export function at(sw, sh, pct) {
  const scale = pct / 100
  // Floor, not round: a real scaled window never reports more CSS px than the screen divided by the scale.
  return Object.freeze({ w: Math.floor(sw / scale), h: Math.floor(sh / scale), scale, name: `${sw}×${sh}${pct === 100 ? '' : ` @${pct} %`}` })
}

export const SCREENS = [[1366, 768], [1440, 900], [1536, 864], [1920, 1080], [1920, 1200], [2560, 1440], [2560, 1600], [3840, 2160]]

/** Every screen × every scaling it is supported at — 33 entries. For a full sweep (STOPVIS_FULL and the like). */
export const MATRIX = SCREENS.flatMap(([w, h]) => [100, 125, 150, 175, ...(w === 3840 ? [200] : [])].map((p) => at(w, h, p)))

// The named entries the probes use.
export const FLOOR = at(1366, 768, 175) //       780×438 — the smallest effective viewport there is
export const FLOOR_150 = at(1366, 768, 150) //   910×512
export const FLOOR_125 = at(1366, 768, 125) //   1092×614
export const LAPTOP = at(1366, 768, 100) //      the floor screen unscaled
export const MID = at(1440, 900, 100) //         a mid laptop (16:10)
export const MID_1536 = at(1536, 864, 100) //    a mid laptop (16:9)
export const FHD = at(1920, 1080, 100)
export const FHD_125 = at(1920, 1080, 125) //    1536×864
export const FHD_150 = at(1920, 1080, 150) //    1280×720
export const FHD_175 = at(1920, 1080, 175) //    1097×617
export const WUXGA = at(1920, 1200, 100)
export const QHD = at(2560, 1440, 100)
export const WQXGA = at(2560, 1600, 100)
export const UHD = at(3840, 2160, 100)
export const UHD_150 = at(3840, 2160, 150) //    2560×1440 at ratio 1.5
export const UHD_200 = at(3840, 2160, 200) //    1920×1080 at ratio 2

/** The representative set: the floor, a mid laptop, 1080p, 1440p and 4K at 200 %. */
export const CORE = [FLOOR, MID, FHD, QHD, UHD_200]

/** A window OFF the matrix, at the OS's 100 %: a pop-out's floor, a window dragged below its screen (the xs layout
 *  lives only there), a super-wide. Named so a reader sees it is deliberate, never a supported screen. */
export function offMatrix(w, h) {
  return Object.freeze({ w, h, scale: 1, name: `${w}×${h} (off-matrix)` })
}

/** `Emulation.setDeviceMetricsOverride` params for an entry: its CSS window at its OS scaling. */
export function metrics(s) {
  return { width: s.w, height: s.h, deviceScaleFactor: s.scale, mobile: false }
}

/** The CSS window at the app's 100 %, the way the probes' own messages print a size. */
export function css(s) {
  return `${s.w}×${s.h}`
}
