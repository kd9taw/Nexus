import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Light-mode readability guard (operator report: washed-out text/greys).
// Parses the [data-theme='light'] token block and enforces WCAG-ish minimums:
// body text 7:1, dim text 4.5:1, the shared need-chip palette 3:1 (UI
// components) — all against the white panel background. Also guards that the
// light block OVERRIDES every --need-* token, so a future need color added to
// the dark defaults can't silently fall through as an unreadable pastel.

const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')

function themeBlock(name: string): Record<string, string> {
  const out: Record<string, string> = {}
  let i = css.indexOf(`[data-theme='${name}']`)
  while (i !== -1) {
    const j = css.indexOf('{', i)
    const k = css.indexOf('}', j)
    for (const m of css.slice(j, k).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
      out[m[1]] = m[2].trim()
    }
    i = css.indexOf(`[data-theme='${name}']`, k)
  }
  return out
}

function rootBlock(): Record<string, string> {
  const out: Record<string, string> = {}
  let i = css.indexOf(':root')
  while (i !== -1) {
    const j = css.indexOf('{', i)
    const k = css.indexOf('}', j)
    for (const m of css.slice(j, k).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
      out[m[1]] = m[2].trim()
    }
    i = css.indexOf(':root', k)
  }
  return out
}

function luminance(hex: string): number {
  let h = hex.replace('#', '')
  if (h.length === 3) h = [...h].map((c) => c + c).join('')
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255)
  const f = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

function contrast(fg: string, bg: string): number {
  const [l1, l2] = [luminance(fg), luminance(bg)].sort((a, b) => b - a)
  return (l1 + 0.05) / (l2 + 0.05)
}

const light = themeBlock('light')
const WHITE = '#ffffff'

describe('light theme readability', () => {
  it('body and dim text clear WCAG on the panel background', () => {
    expect(contrast(light['--text'], light['--bg-elev'])).toBeGreaterThanOrEqual(7)
    expect(contrast(light['--text-dim'], light['--bg-elev'])).toBeGreaterThanOrEqual(4.5)
    expect(contrast(light['--text-faint'], light['--bg-elev'])).toBeGreaterThanOrEqual(4.5)
  })

  it('overrides EVERY --need-* token with a ≥3:1 ink (no dark-pastel fall-through)', () => {
    // NOTE: this merges the blocks by name and does NOT resolve the cascade, so it says only
    // that an override was WRITTEN. Between 2026-07-06 and 2026-08-05 every one of these
    // overrides was written and then lost to a later :root palette, and this test stayed green
    // throughout — the resolution guard lives in styles-theme-cascade.test.ts. --need-ink is excluded because
    // it is not an ink ON the panel: it is the ink that goes ON TOP of a --need-* fill, so it
    // is deliberately white in light mode. That pair is measured in the cascade guard.
    const dark = { ...rootBlock(), ...themeBlock('dark') }
    const needTokens = Object.keys(dark).filter((k) => k.startsWith('--need-') && k !== '--need-ink')
    expect(needTokens.length).toBeGreaterThan(0)
    for (const tok of needTokens) {
      expect(light[tok], `${tok} must be overridden for light mode`).toBeDefined()
      expect(
        contrast(light[tok], WHITE),
        `${tok} (${light[tok]}) vs white`,
      ).toBeGreaterThanOrEqual(3)
    }
  })

  it('the light-mode ink override section keeps its inks readable', () => {
    // Every hex inside the marked section must hold ≥4.5:1 on white (they are
    // TEXT colors on page backgrounds — that's why the section exists).
    const start = css.indexOf('LIGHT-MODE INK OVERRIDES')
    const end = css.indexOf('Stock WSJT-X decode', start)
    expect(start).toBeGreaterThan(-1)
    for (const m of css.slice(start, end).matchAll(/color:\s*(#[0-9a-fA-F]{6})/g)) {
      expect(contrast(m[1], WHITE), `${m[1]} vs white`).toBeGreaterThanOrEqual(4.5)
    }
  })

  it('palette-namespace tokens are always theme-defined (the band-good bug)', () => {
    // var(--x) referencing a token no theme defines silently drops the
    // declaration — exactly what happened with --band-good. Scoped to the
    // palette namespaces whose definitions MUST live in the theme blocks
    // (other tokens are legitimately defined in component selectors or
    // inline from TSX, which this parser can't see).
    const defined = new Set([
      ...Object.keys(rootBlock()),
      ...Object.keys(themeBlock('dark')),
      ...Object.keys(themeBlock('light')),
    ])
    const used = new Set([...css.matchAll(/var\((--[\w-]+)[),]/g)].map((m) => m[1]))
    const undefd = [...used].filter(
      (t) => /^--(band|snr|status)-/.test(t) && !defined.has(t),
    )
    expect(undefd, `undefined palette tokens referenced: ${undefd.join(', ')}`).toHaveLength(0)
  })
})

describe('the focus ring is visible on every surface it lands on', () => {
  // WCAG 1.4.11 (non-text contrast): a focus indicator needs 3:1 against what is behind
  // it. Nexus paints ONE ring token everywhere (`:focus-visible { outline: 2px solid
  // var(--focus-ring) }`, styles.css ~102), so it has to clear that bar on every surface a
  // focusable control sits on — the page, a panel, and the elevated overlays (the ⊞ Panels
  // popover is `--bg-elev-2`).
  //
  // Pinned DIRECTLY, because up to now it was held only by an absence guard in
  // panel-overflow.test.ts: "no rule may fade an entry in the ⊞ popover, because opacity
  // composites the outline too". That guard is worth keeping and it is not this — it stops
  // the ring being HALVED, and says nothing about whether the ring passes at full strength.
  // It did not: the first run of this test failed, because light-mode `--focus-ring` was
  // 2.999:1 on `--bg`. The token moved one shade darker (styles.css, light block) rather
  // than the bar moving. Margins in light mode are the tight ones, so this is the guard
  // that catches a nudge to the ring OR to a surface token.
  const SURFACES = ['--bg', '--bg-elev', '--bg-elev-2'] as const
  const root = rootBlock()
  it.each([
    ['dark', { ...root, ...themeBlock('dark') }],
    ['light', { ...root, ...themeBlock('light') }],
  ])('%s: --focus-ring clears 3:1 on every surface token', (_theme, tokens) => {
    const ring = tokens['--focus-ring']
    expect(ring, '--focus-ring is not theme-defined').toMatch(/^#[0-9a-fA-F]{3,6}$/)
    for (const s of SURFACES) {
      expect(tokens[s], `${s} is not theme-defined`).toMatch(/^#[0-9a-fA-F]{3,6}$/)
      expect(
        contrast(ring, tokens[s]),
        `--focus-ring (${ring}) on ${s} (${tokens[s]}) — WCAG 1.4.11 needs 3:1 for a focus ` +
          'indicator, and this is the ring for EVERY focusable control in the app',
      ).toBeGreaterThanOrEqual(3)
    }
  })
})
