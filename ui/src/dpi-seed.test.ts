import { describe, it, expect } from 'vitest'
import { dpiSeedCap, fitScale, SCALE_STEPS, MAIN_NATURAL } from './useScale'

// THE FIRST-LAUNCH DPI SEED.
//
// `DEFAULT_CAP` is 100, so auto-fit can shrink the UI for a small window and can never grow
// it for a dense display. On Windows and macOS that is fine — the OS picks a devicePixelRatio
// that already sizes the CSS pixel — but X11 reports 1 whatever the panel is, and the GNOME
// setting an operator reaches for (text-scaling-factor → gtk-xft-dpi) was measured against
// webkit2gtk-4.1 2.52.3 not to reach web content at all. So a Linux operator on a dense panel
// gets the app at 1:1 physical pixels and no control that can grow it without them finding
// the scale setting themselves.
//
// The rules these tests pin, in the order they matter:
//   1. an ordinary display must come out UNCHANGED,
//   2. the seed may only ever RAISE,
//   3. Windows/macOS take the same code path and it must be a no-op there,
//   4. bad or missing data changes nothing.

describe('dpiSeedCap — what an ordinary display sees', () => {
  it('does nothing at the 96-dpi reference', () => {
    expect(dpiSeedCap(96, 1)).toBeNull()
  })

  it('does nothing on the common desktop panels', () => {
    expect(dpiSeedCap(92, 1)).toBeNull() // 24" 1080p
    expect(dpiSeedCap(82, 1)).toBeNull() // 27" 1080p
    expect(dpiSeedCap(109, 1)).toBeNull() // 27" 1440p — inside the dead-band, on purpose
  })

  it('does nothing where the OS already scaled for us', () => {
    // Windows at 150% on a 141-dpi laptop: dpr 1.5, so a CSS pixel is ~94 dpi. macOS Retina:
    // 227-dpi panel at dpr 2 is ~113. Both are the platform doing its job — the seed must be
    // able to tell, and must keep its hands off.
    expect(dpiSeedCap(141, 1.5)).toBeNull()
    expect(dpiSeedCap(227, 2)).toBeNull()
    expect(dpiSeedCap(192, 2)).toBeNull()
  })

  it('does nothing without a usable answer', () => {
    expect(dpiSeedCap(null, 1)).toBeNull()
    expect(dpiSeedCap(undefined, 1)).toBeNull()
    expect(dpiSeedCap(Number.NaN, 1)).toBeNull()
    expect(dpiSeedCap(0, 1)).toBeNull()
    expect(dpiSeedCap(-184, 1)).toBeNull()
    expect(dpiSeedCap(184, 0)).toBeNull()
    expect(dpiSeedCap(184, Number.NaN)).toBeNull()
  })
})

describe('dpiSeedCap — what a dense display gets', () => {
  it('raises for the reported field case (1920×1080 on a 12" panel, X11)', () => {
    // ~184 dpi with no OS scaling at all. This is the machine in the 1.9.0 report.
    const seed = dpiSeedCap(184, 1)
    expect(seed).not.toBeNull()
    expect(seed).toBe(175)
  })

  it('raises for a 4K desktop panel, which is where it pays most', () => {
    expect(dpiSeedCap(163, 1)).toBe(150) // 27" 4K
    expect(dpiSeedCap(140, 1)).toBe(125) // 15.6" 1080p laptop
  })

  it('never returns a step at or below the default cap', () => {
    for (let dpi = 1; dpi <= 400; dpi++) {
      const seed = dpiSeedCap(dpi, 1)
      if (seed != null) expect(seed).toBeGreaterThan(100)
    }
  })

  it('is monotonic and stays on the ladder', () => {
    let last = 0
    for (let dpi = 96; dpi <= 400; dpi++) {
      const seed = dpiSeedCap(dpi, 1)
      if (seed == null) continue
      expect(SCALE_STEPS).toContain(seed)
      expect(seed).toBeGreaterThanOrEqual(last)
      last = seed
    }
  })

  it('snaps DOWN, never up', () => {
    // 1.30× wants 130; the ladder has 125 and 150. Too small is a squint, too large hides
    // controls, so it must take 125.
    expect(dpiSeedCap(96 * 1.3, 1)).toBe(125)
    expect(dpiSeedCap(96 * 1.49, 1)).toBe(125)
    expect(dpiSeedCap(96 * 1.5, 1)).toBe(150)
  })
})

describe('the seed is a CEILING, not a scale — what the operator actually ends up at', () => {
  // The half that is easy to overstate: raising the cap does not set the zoom. Auto-fit still
  // takes min(what fits this window, the cap), so a window too small to hold the content
  // whole is unaffected by any of this.
  const fitAt = (w: number, h: number, cap: number) =>
    fitScale(w, h, cap as never, undefined, MAIN_NATURAL)

  it('gives the reporter 110% instead of 100% — his window, not his panel, is the binding constraint', () => {
    const seed = dpiSeedCap(184, 1)!
    expect(fitAt(1920, 1080, 100)).toBe(100) // today
    expect(fitAt(1920, 1080, seed)).toBe(110) // after the seed
  })

  it('gives a 4K window the large change, because there the cap is what binds', () => {
    const seed = dpiSeedCap(163, 1)!
    expect(fitAt(3840, 2160, 100)).toBe(100)
    expect(fitAt(3840, 2160, seed)).toBe(150)
  })

  it('leaves a small window exactly where it was, however dense the panel', () => {
    // 1024×768, the supported floor. The content fit is far below any seeded cap, so the
    // seed is inert — it can never make a cramped window worse.
    expect(fitAt(1024, 768, 100)).toBe(fitAt(1024, 768, dpiSeedCap(184, 1)!))
  })
})
