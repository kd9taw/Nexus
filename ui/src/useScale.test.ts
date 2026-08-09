import { describe, it, expect } from 'vitest'
import type { Scale } from './useScale'
import {
  capPinnedScale,
  fitScale,
  pickInitialZoom,
  naturalFor,
  MAIN_NATURAL,
  SCALE_STEPS,
  fieldFitScale,
  PANEL_NATURAL,
} from './useScale'

// Fit model: the MAIN window fits against MAIN_NATURAL (1200×900). Auto NEVER upscales
// (default cap 100), so 1080p full-screen and anything bigger sit at 100%; only SMALLER
// windows scale down (gently) toward the 65% floor. A raised cap (Settings) lets big
// panels go above 100%. A POP-OUT fits against its OWN natural — see the per-surface
// block at the bottom of this file.

describe('fitScale', () => {
  it('keeps 1080p full-screen (and bigger) at 100% — no upscaling by default', () => {
    expect(fitScale(1920, 1080)).toBe(100)
    expect(fitScale(1920, 1040)).toBe(100) // maximized (taskbar eats a little height)
    expect(fitScale(2560, 1080)).toBe(100) // ultrawide 1080-tall
    expect(fitScale(3840, 1080)).toBe(100) // very wide but short
    expect(fitScale(3840, 2160)).toBe(100) // 4K — capped at 100 unless the operator raises it
  })

  it('opens the default 1200×720 window roomy (~80%)', () => {
    expect(fitScale(1200, 720)).toBe(80) // 720/900=0.80 → 80
  })

  it('scales DOWN gently on smaller windows, roomy then smaller as needed', () => {
    expect(fitScale(1366, 768)).toBe(85) // 768/900=0.853 → 85
    expect(fitScale(1280, 720)).toBe(80) // 720/900=0.80 → 80
    expect(fitScale(1100, 700)).toBe(75) // 700/900=0.777 → 75
    expect(fitScale(1536, 864)).toBe(90) // "1080p @125% OS" — 864/900=0.96 → 90
  })

  it('floors at 65% and never below', () => {
    expect(fitScale(900, 600)).toBe(65) // 600/900=0.667 → floored to 65
    expect(fitScale(700, 500)).toBe(65) // tiny → still 65
  })

  it('scales UP on big panels only when the cap is raised', () => {
    expect(fitScale(3840, 2160, 125)).toBe(125) // 2160/900=2.4, cap 125
    expect(fitScale(2560, 1440, 125)).toBe(125) // 1440/900=1.6, cap 125
  })

  it('honours a raised cap and snaps DOWN to a real step', () => {
    // 1440/900 = 1.6 → target 160. Steps ≤160 and ≤cap 175: largest is 150.
    expect(fitScale(2560, 1440, 175)).toBe(150)
    // 2160/900 = 2.4 → target 240. Cap 150 → largest step ≤150 is 150.
    expect(fitScale(3840, 2160, 150)).toBe(150)
    expect(fitScale(3840, 2160, 175)).toBe(175)
  })

  it('window-limited auto: raising the cap changes NOTHING when the window binds', () => {
    // Win10 laptop reality — 1920×1080 at 125% OS display scaling = 1536×864 CSS px.
    // Fit target is 96, below every cap chip Settings offers (100–175), so ALL of
    // them yield the same 90%. This is the by-design "auto never upscales past fit"
    // rule; the Settings hint must explain it (operator report: "scaling settings
    // are not visually doing anything").
    for (const cap of [100, 110, 125, 150, 175] as const) {
      expect(fitScale(1536, 864, cap)).toBe(90)
    }
    // Same window, Manual mode is the escape hatch (no fit involved) — and on a
    // big panel the cap DOES bite, so the chips are not globally inert.
    expect(fitScale(3840, 2160, 150)).toBe(150)
  })

  it('exposes a window fit-ceiling (fitScale at max cap) that SettingsPanel disables above', () => {
    // SettingsPanel computes autoCeil = fitScale(w, h, 175) and disables every cap
    // chip whose value exceeds it (they would all yield this same scale). These are
    // the exact ceilings that gate which chips are live.
    expect(fitScale(1536, 864, 175)).toBe(90) // 1080p @125% OS → only ≤90 live (none, since chips start at 100)
    expect(fitScale(1920, 1080, 175)).toBe(110) // 1080p → chips 100,110 live; 125+ dead
    expect(fitScale(2560, 1440, 175)).toBe(150) // 1440p → up to 150 live; 175 dead
    expect(fitScale(3840, 2160, 175)).toBe(175) // 4K → every chip live
  })

  it('respects width when width is the binding axis', () => {
    // Very tall, narrow window: 900 wide / 1200 = 0.75 binds over height (2000/900).
    expect(fitScale(900, 2000)).toBe(75)
  })

  it('applies hysteresis: holds the current step within the dead-band', () => {
    // target ~99 (891/900): without prev picks 90; with prev=100 and |99-100|=1 ≤ 100*0.03 → holds 100.
    expect(fitScale(1920, 891)).toBe(90)
    expect(fitScale(1920, 891, 125, 100)).toBe(100)
    // Far from prev → releases: prev=100 but a 768-tall window demands ~85.
    expect(fitScale(1366, 768, 125, 100)).toBe(85)
  })

  it('is a fixed point (no oscillation): feeding the result back does not move it', () => {
    const z = fitScale(1600, 900)
    expect(fitScale(1600, 900, 125, z)).toBe(z)
  })
})

describe('capPinnedScale (per-surface pin policy)', () => {
  it('main window: the pin is an operator choice — returned verbatim, any geometry', () => {
    expect(capPinnedScale(175, true, 900, 600)).toBe(175)
    expect(capPinnedScale(65, true, 3840, 2160)).toBe(65)
  })

  it('pop-out: caps the pin at the window fit ceiling (fitScale at the max step)', () => {
    // Torn-off waterfall at min 380×180-ish: ceiling is the 65 floor.
    expect(capPinnedScale(175, false, 900, 300)).toBe(65)
    // Default operate pop-out 1140×760: fitScale(…, 175) = 80.
    expect(capPinnedScale(175, false, 1140, 760)).toBe(80)
  })

  it('pop-out: a pin at or under the ceiling passes through', () => {
    expect(capPinnedScale(70, false, 1140, 760)).toBe(70)
    expect(capPinnedScale(80, false, 1140, 760)).toBe(80)
  })
})

describe('pickInitialZoom (synchronous seed)', () => {
  it('matches fitScale at the default cap', () => {
    expect(pickInitialZoom(1920, 1080)).toBe(fitScale(1920, 1080))
    expect(pickInitialZoom(1366, 768)).toBe(fitScale(1366, 768))
  })

  it('only ever returns a valid scale step', () => {
    for (const [w, h] of [
      [800, 600],
      [1366, 768],
      [1920, 1080],
      [2560, 1440],
      [3840, 2160],
      [1024, 700],
    ] as const) {
      expect(SCALE_STEPS).toContain(pickInitialZoom(w, h))
    }
  })
})

// ---------------------------------------------------------------------------
// Per-surface natural footprint (operator report: "the font is very, very small
// when opening the CW band map").
//
// A pop-out is a small window the APP sized for its own content. Fitting one against
// the dense cockpit's 1200×900 footprint asks whether the FT8 cockpit fits inside a
// 420-px strip: 420/1200 = 0.35, below every step, so the width term wins the min()
// and the window pins to the 65% floor at EVERY size it can legally have. Permanent,
// not transient. `naturalFor(panel)` supplies the right denominator per surface.
// ---------------------------------------------------------------------------

describe('fitScale against a per-surface natural', () => {
  it('the CW band map opens at 100%, not the 65% floor (operator report)', () => {
    const nat = naturalFor('bandmapCw')
    // open_panel_window's default inner size for a band map.
    expect(fitScale(420, 780, 100, undefined, nat)).toBe(100)
    // The Phone band map is the same window shape and must not diverge.
    expect(fitScale(420, 780, 100, undefined, naturalFor('bandmapPhone'))).toBe(100)
    // Docked as a full-height edge strip (snap_bandmap_to_edge keeps the width and
    // takes the work-area height) — same answer, no step change on dock.
    expect(fitScale(420, 1400, 100, undefined, nat)).toBe(100)
  })

  it('band map: auto still TAPERS as the window is squashed toward its minimum', () => {
    const nat = naturalFor('bandmapCw')
    // The natural is a CONTENT box (380×520), deliberately not the window minimum —
    // so the operator's resize lever stays live and the floor still protects the
    // 420×360 minimum, where the legend + the 240 px track floor stop fitting.
    expect(fitScale(420, 620, 100, undefined, nat)).toBe(100)
    expect(fitScale(420, 500, 100, undefined, nat)).toBe(90)
    expect(fitScale(420, 440, 100, undefined, nat)).toBe(80)
    expect(fitScale(420, 360, 100, undefined, nat)).toBe(65)
  })

  it('the waterfall strip is off the floor too, and tapers to it at its minimum', () => {
    const nat = naturalFor('waterfall')
    expect(fitScale(900, 300, 100, undefined, nat)).toBe(100) // default strip
    expect(fitScale(380, 180, 100, undefined, nat)).toBe(65) // min_inner_size
  })

  it('the Operate pop-out keeps the main cockpit footprint — it hosts that cockpit', () => {
    expect(naturalFor('operate')).toEqual(MAIN_NATURAL)
    expect(fitScale(1140, 760, 100, undefined, naturalFor('operate'))).toBe(80)
  })

  it('an unknown panel resolves to the generic pop-out natural, never the prototype', () => {
    // A Map, not an object literal: `?panel=constructor` must not resolve a function.
    expect(naturalFor('constructor')).toEqual(naturalFor('nosuchpanel'))
    expect(naturalFor('toString')).toEqual(naturalFor('nosuchpanel'))
    // No panel at all == the main window.
    expect(naturalFor(null)).toEqual(MAIN_NATURAL)
  })
})

describe('capPinnedScale against a per-surface natural', () => {
  it('main window: the pin is verbatim, whatever natural is passed', () => {
    expect(capPinnedScale(175, true, 900, 600, naturalFor('bandmapCw'))).toBe(175)
    expect(capPinnedScale(65, true, 3840, 2160, MAIN_NATURAL)).toBe(65)
  })

  it('pop-out: an inherited pin is honoured as far as THIS window can take it', () => {
    // Before the per-surface natural the ceiling in a 420×780 band map was the 65
    // floor, so every inherited pin was crushed to 65 — the pin half of the same bug.
    expect(capPinnedScale(175, false, 420, 780, naturalFor('bandmapCw'))).toBe(110)
    expect(capPinnedScale(100, false, 420, 780, naturalFor('bandmapCw'))).toBe(100)
    // Still bounded: squashed to its minimum, the window cannot show 175.
    expect(capPinnedScale(175, false, 420, 360, naturalFor('bandmapCw'))).toBe(65)
  })
})

// ---------------------------------------------------------------------------
// The main window did not move. `legacyFit` is the pre-change formula inlined
// VERBATIM (NAT_W/NAT_H = 1200/900); the grid below straddles every step boundary,
// every cap and every hysteresis dead-band, so "unchanged" is a computed fact rather
// than an argument. Any main-window (default-natural) divergence lands in `bad`.
// ---------------------------------------------------------------------------

describe('main-window fit is byte-identical to the pre-change model', () => {
  const NAT_W = 1200
  const NAT_H = 900
  const HYST = 0.03
  function legacyFit(innerW: number, innerH: number, cap: Scale = 100, prev?: Scale): Scale {
    const target = Math.min(innerW / NAT_W, innerH / NAT_H) * 100
    const allowed = SCALE_STEPS.filter((s) => s <= cap)
    let z: Scale = allowed[0] ?? 65
    for (const s of allowed) if (s <= target) z = s
    if (prev != null && allowed.includes(prev) && Math.abs(target - prev) <= prev * HYST) {
      return prev
    }
    return z
  }

  const WIDTHS = [
    320, 380, 420, 560, 700, 760, 780, 900, 1024, 1100, 1140, 1200, 1280, 1366, 1440, 1536, 1600,
    1680, 1920, 2048, 2560, 3440, 3840,
  ]
  const HEIGHTS = [
    180, 200, 300, 360, 500, 585, 600, 630, 660, 675, 700, 720, 760, 768, 780, 810, 864, 891, 900,
    990, 1024, 1080, 1400, 1440, 2160,
  ]
  const PREVS: (Scale | undefined)[] = [undefined, ...SCALE_STEPS]

  it('fitScale matches legacyFit over the whole w × h × cap × prev grid', () => {
    const bad: string[] = []
    for (const w of WIDTHS) {
      for (const h of HEIGHTS) {
        for (const cap of SCALE_STEPS) {
          for (const prev of PREVS) {
            const got = fitScale(w, h, cap, prev)
            const want = legacyFit(w, h, cap, prev)
            if (got !== want) bad.push(`${w}×${h} cap=${cap} prev=${prev}: ${got} ≠ ${want}`)
          }
        }
      }
    }
    expect(bad).toEqual([])
    // Sanity: the grid actually ran (a silently empty loop would also pass above).
    expect(WIDTHS.length * HEIGHTS.length * SCALE_STEPS.length * PREVS.length).toBeGreaterThan(30000)
  })

  it('pickInitialZoom (no natural argument) is the legacy seed at the default cap', () => {
    const bad: string[] = []
    for (const w of WIDTHS) {
      for (const h of HEIGHTS) {
        if (pickInitialZoom(w, h) !== legacyFit(w, h)) bad.push(`${w}×${h}`)
      }
    }
    expect(bad).toEqual([])
  })

  it('the exact main-window sizes the operator sees are unmoved', () => {
    // Measured against the UNMODIFIED function before the change (see the report).
    expect(fitScale(1920, 1080)).toBe(100)
    expect(fitScale(1366, 768)).toBe(85)
    expect(fitScale(1200, 900)).toBe(100)
    expect(fitScale(1200, 720)).toBe(80)
    expect(fitScale(900, 600)).toBe(65)
    expect(fitScale(3440, 1440)).toBe(100)
    expect(fitScale(3440, 1440, 175)).toBe(150)
  })
})

describe('field mode scale (fieldFit)', () => {
  // The POTA report's size half. Field mode swaps fitScale's INPUTS — the natural shrunk by
  // FIELD_FIT and the cap raised to the ladder top — so switching it off restores the prior
  // scale exactly (nothing is ever written to the scale keys). These are pure-function tests;
  // written failing-first before fieldFitScale existed.
  it('is monotonic: field never yields a SMALLER step than auto, at any sweep size', () => {
    const sizes: Array<[number, number]> = [
      [1024, 768], [1280, 800], [1366, 768], [1536, 864], [1920, 1080], [2560, 1440], [3440, 1440],
    ]
    for (const [w, h] of sizes) {
      const auto = fitScale(w, h)
      const field = fieldFitScale(w, h)
      expect(field, `${w}x${h}: field ${field} < auto ${auto}`).toBeGreaterThanOrEqual(auto)
      for (const nat of PANEL_NATURAL.values()) {
        const a = fitScale(w, h, undefined, undefined, nat)
        const f = fieldFitScale(w, h, undefined, nat)
        expect(f, `${w}x${h} panel ${nat.w}x${nat.h}: field ${f} < auto ${a}`).toBeGreaterThanOrEqual(a)
      }
    }
  })

  it('delivers the designed bumps at the common field sizes', () => {
    expect(fieldFitScale(1366, 768)).toBe(100) // auto: 85
    expect(fieldFitScale(1536, 864)).toBe(110) // auto: 90
    expect(fieldFitScale(1920, 1080)).toBe(125) // auto: 100 (capped)
  })

  it('keeps the effective viewport at or above the supported floor', () => {
    // FIELD_FIT=0.85 is exactly the boundary: 0.85 x MAIN_NATURAL = 1020x765 vs the 1024x768
    // floor. A smaller factor would push the effective box BELOW the floor at real sizes —
    // this is the one genuine layout-contract risk, pinned here.
    const sizes: Array<[number, number]> = [[1024, 768], [1366, 768], [1920, 1080]]
    for (const [w, h] of sizes) {
      const z = fieldFitScale(w, h) / 100
      expect(w / z, `${w}x${h}: effective width ${(w / z).toFixed(0)} under the floor`).toBeGreaterThanOrEqual(1020)
      expect(h / z, `${w}x${h}: effective height ${(h / z).toFixed(0)} under the floor`).toBeGreaterThanOrEqual(765)
    }
  })

  it('never exceeds the ladder top', () => {
    expect(fieldFitScale(10000, 10000)).toBeLessThanOrEqual(175)
  })
})
