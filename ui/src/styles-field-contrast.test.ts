// FIELD MODE's success criterion, as a computed relationship (POTA field report, 2026-08-09:
// "very difficult to see outdoors, even shaded, light or dark mode").
//
// Outdoor light adds a constant veiling luminance to ink and surface alike, compressing every
// contrast ratio toward 1:1 — the de-emphasised text dies first, and ~73% of this sheet's
// coloured text is deliberately de-emphasised (--text-dim/--text-faint). So the criterion is
// not "looks darker": it is that under a veiling term of R=0.15 (open shade), all three ink
// tokens still clear 4.5:1 on --panel, in BOTH high-contrast modes, computed from the CASCADE
// WINNER — a high block that loses the cascade is silently inert, and that failure mode has
// shipped before (--need-* light palette, dead for a month behind a passing presence test).
//
// Written BEFORE the implementation and watched fail: with no [data-contrast='high'] block in
// the sheet, a high mode resolves identically to its base theme, and the base themes measure
// 2.8:1 (dark dim) and 2.2:1 (dark faint) at R=0.15 — below even the 3:1 non-text floor.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  baseTheme,
  contrastUnderGlare,
  expandWith,
  parseHex,
  parseRules,
  rgbHex,
  rootTokensFrom,
  toRgb,
  type Mode,
  type Rgb,
} from './cssCascade'

const CSS_PATH = fileURLToPath(new URL('./styles.css', import.meta.url))
const CSS = readFileSync(CSS_PATH, 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) =>
  m.replace(/[^\n]/g, ' '),
)
const RULES = parseRules(CSS)

const HIGH: Mode[] = ['dark-high', 'light-high']
const INKS = ['--text', '--text-dim', '--text-faint'] as const
/** Veiling reflection ≈ open shade. Bright shade is ~0.30 and is not the bar — an emissive
 *  panel in bright shade needs brightness the hardware may not have; open shade is the
 *  condition the reporter named ("even shaded"). */
const R = 0.15
const MIN = 4.5

function resolved(mode: Mode, token: string): Rgb {
  const tokens = rootTokensFrom(RULES, mode)
  const page = parseHex(expandWith(tokens, 'var(--bg)')) ?? ([255, 255, 255] as const)
  const c = toRgb(expandWith(tokens, `var(${token})`), page)
  expect(c, `${token} did not resolve to a colour in ${mode}`).not.toBeNull()
  return c!
}

describe('field mode inks survive open shade', () => {
  const cases = HIGH.flatMap((m) => INKS.map((ink) => [ink, m] as const))
  it.each(cases)('%s clears 4.5:1 on --panel under glare in %s', (ink, mode) => {
    const fg = resolved(mode, ink)
    const bg = resolved(mode, '--panel')
    const ratio = contrastUnderGlare(fg, bg, R)
    expect(
      ratio,
      `${ink} ${rgbHex(fg)} on --panel ${rgbHex(bg)} in ${mode}: ${ratio.toFixed(2)}:1 at R=${R}`,
    ).toBeGreaterThanOrEqual(MIN)
  })
})

describe('the high block actually wins the cascade', () => {
  // "A fall-through wearing a declaration": if the high block exists but loses (or never
  // matches), every token resolves to its base-theme value and the suite above could pass on
  // a future re-tuned base theme while field mode does nothing. Each high surface/ink token
  // must DIFFER from its base theme, which is only possible if the block is winning.
  const MUST_DIFFER = ['--text', '--text-dim', '--text-faint', '--bg', '--panel', '--border'] as const
  const cases = HIGH.flatMap((m) => MUST_DIFFER.map((t) => [t, m] as const))
  it.each(cases)('%s differs from the base theme in %s', (token, mode) => {
    const high = rgbHex(resolved(mode, token))
    const base = rgbHex(resolved(baseTheme(mode), token))
    expect(high, `${token} in ${mode} resolves to the ${baseTheme(mode)} value — inert block`).not.toBe(
      base,
    )
  })

  it('widens the page→panel elevation step rather than flattening it', () => {
    // styles.css records why all-white surfaces read washed out: the structure went flat.
    // High contrast must keep page and panel DISTINCT in both high modes.
    for (const mode of HIGH) {
      expect(rgbHex(resolved(mode, '--bg')), `--bg == --panel in ${mode} — flat structure`).not.toBe(
        rgbHex(resolved(mode, '--panel')),
      )
    }
  })
})
