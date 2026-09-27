// NIGHT — a darker, warmer screen for operating after dark (Settings ▸ Appearance ▸ Workspace ▸
// Night: Off / On / Auto at dusk). The operator's picks of 2026-09-26: "Dim + warm, TX red" (TX,
// alert and signal colours unchanged), "Dim the light theme" (Night dims WHATEVER theme is on, so
// there are two night looks, dark + night and light + night, and each has a high-contrast
// variant), "Settings + Auto by sun", "At dusk, sun 6° down".
//
// THE AMBER LESSON is why this file exists (ui/DESIGN.md): the amber theme once showed Stop TX
// outlined in the same amber as Monitor, and the TX red was simply gone. A night look is exactly
// the kind of retune that could repeat that, so nothing here trusts a declaration: every
// assertion is computed on the cascade WINNER, in all four night looks and under every colour-role
// preset set (MODES), at <html> and inside a display well, against the same mode with Night off.
//
// What Night may change is a closed list (NIGHT_TOKENS): the neutral surfaces and inks, the
// accent family, the chat bubbles, the frequency readout and the well tokens. EVERYTHING ELSE
// must resolve at night to exactly its day value — the TX red, the ON AIR fill and ink, every
// alert, SNR, status, band and need colour — and no night rule may even declare one. Two tokens
// follow an ink by their own definition and are allowed to move with it: the ON AIR rim is the
// theme's full-strength ink (`--text`) and the unconfirmed-dial ink is `--text-dim`.
//
// The floors: text 4.5:1 on every surface text sits on (7:1, WCAG AAA, under high contrast — at
// night there is no sun, so the daylight glare model of styles-field-contrast.test.ts is not the
// bar, but high contrast still means more contrast than Night alone); `--tx` 3:1 on the panel and
// the page; every status colour 3:1 on the panel and on a well; the accent ΔE_OK ≥ 0.15 from the
// TX red and the critical orange (the colour-roles floor). "Dimmer" and "warmer" are measured too,
// or a night block that changed nothing visible would pass.
//
// Written BEFORE the night block and watched fail: with no `[data-night='1']` rules in the sheet
// every night mode resolves identically to its day twin, so "the night block is live" is red.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  BASE_MODES,
  MODES,
  baseTheme,
  contrast,
  dayOf,
  deltaE,
  expandWith,
  isHigh,
  isNight,
  luminance,
  oklch,
  parseRules,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  tokensAt,
  withRoles,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'
import { PALETTE_ROLES } from './features/paletteRoles'

const blank = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const read = (name: string) => readFileSync(fileURLToPath(new URL(`./${name}`, import.meta.url)), 'utf8')
const ORDER = { n: 0 }
const RULES = parseRules(blank(read('styles.css')) + '\n' + blank(read('cockpit-panes.css')), ORDER)

const NIGHT_BASES = BASE_MODES.filter((b) => isNight(b))
const NIGHT_MODES = MODES.filter((m) => isNight(m))

const SURFACES = ['--bg', '--panel', '--bg-elev', '--bg-elev-2', '--border', '--border-soft'] as const
const INKS = ['--text', '--text-dim', '--text-faint'] as const
/** Every token a night rule may declare. Anything else is somebody else's colour. */
const NIGHT_TOKENS: ReadonlySet<string> = new Set([
  ...SURFACES,
  ...INKS,
  '--accent',
  '--accent-ink',
  '--focus-ring',
  '--bubble-mine',
  '--bubble-mine-text',
  '--bubble-theirs',
  '--bubble-theirs-text',
  '--readout',
  '--well-bg',
  '--well-ink',
  '--well-grid',
])
/** Tokens defined as an ink, which therefore move with it at night — and only that way. */
const FOLLOWS_INK: Readonly<Record<string, string>> = { '--on-air-rim': '--text', '--state-pending': '--text-dim' }
/** DESIGN.md's status roles, less the two de-emphasis states that sit under 3:1 on purpose. */
const STATUS = [
  '--status-new-entity',
  '--status-new-band',
  '--status-new-mode',
  '--status-worked',
  '--status-confirmed',
  '--snr-strong',
  '--snr-marginal',
  '--snr-weak',
  '--tx',
  '--rx',
  '--band-open',
  '--band-marginal',
  '--alert-critical',
  '--alert-warning',
  '--alert-info',
] as const

const TEXT_MIN = 4.5
const TEXT_MIN_HIGH = 7
const STATUS_MIN = 3
const TX_MIN = 3
const LOCKED_DE = 0.15
/** How much of its day luminance the brightest ink may keep at night, in a dark theme and in a
 *  well — the light that dark adaptation is about. */
const INK_DIM = 0.8
/** Warm: OKLCH hue between red-orange and yellow, and neutral enough never to read as a status. */
const WARM: readonly [number, number] = [30, 110]
const NEUTRAL_C = 0.04

type Scope = 'root' | 'well'
const SCOPES: readonly Scope[] = ['root', 'well']
const WELL: El[] = [{ tag: 'div', classes: ['well'], attrs: {} }]

const memo = new WeakMap<Rule[], Map<string, Map<string, string>>>()
/** Every custom property visible in `scope` under `mode`, var()s expanded. */
function tokensIn(rules: Rule[], mode: Mode, scope: Scope): Map<string, string> {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  const key = `${mode}|${scope}`
  let out = byKey.get(key)
  if (!out) {
    if (scope === 'well') out = tokensAt(rules, mode, WELL)
    else {
      const raw = rootTokensFrom(rules, mode)
      out = new Map([...raw].map(([k, v]) => [k, expandWith(raw, v).trim()]))
    }
    byKey.set(key, out)
  }
  return out
}
const valueOf = (tk: Map<string, string>, token: string) => expandWith(tk, `var(${token})`).trim().toLowerCase()
function rgbOf(tk: Map<string, string>, token: string): Rgb {
  // An undeclared token expands to "", which `toRgb` would hand back as the backdrop, and a
  // measurement against a colour nobody chose passes anything.
  const v = valueOf(tk, token)
  const c = v === '' ? null : toRgb(v, [0, 0, 0])
  if (!c) throw new Error(`${token} does not resolve to a colour: "${v}"`)
  return c
}
const where = (mode: Mode, scope: Scope) => `${mode} ${scope}`

// ── The checks, each a pure function of a rule set so the controls below can break one ────────

/** The night block wins: every surface and ink differs from the same mode with Night off —
 *  except a surface that is pure black by day (high contrast's page), which nothing can dim. */
function liveProblems(rules: Rule[]): string[] {
  const out: string[] = []
  for (const mode of NIGHT_BASES) {
    for (const scope of SCOPES) {
      const [night, day] = [tokensIn(rules, mode, scope), tokensIn(rules, dayOf(mode), scope)]
      for (const t of [...SURFACES, ...INKS]) {
        if (valueOf(day, t) === '#000000') continue
        if (valueOf(night, t) === valueOf(day, t)) out.push(`${where(mode, scope)}: ${t} is ${valueOf(night, t)}, the same as with Night off — the night block is inert`)
      }
    }
  }
  return out
}

/** Night gives out less light. In a dark theme and in every well: no surface or ink gets
 *  brighter, and the brightest ink loses at least a fifth. In the light theme: no surface gets
 *  brighter and the panel dims — only a little, because the locked inks painted on it hold it
 *  up (styles.css NIGHT has the numbers). The accent is dimmer everywhere, for every preset; the
 *  readout is dimmer wherever it glows (the dark theme, every well) and never brighter in the
 *  light theme, where its digits are dark ink on a light panel. */
function darkerProblems(rules: Rule[]): string[] {
  const out: string[] = []
  const Y = (tk: Map<string, string>, t: string) => luminance(rgbOf(tk, t))
  for (const mode of NIGHT_BASES) {
    for (const scope of SCOPES) {
      const [night, day] = [tokensIn(rules, mode, scope), tokensIn(rules, dayOf(mode), scope)]
      const w = where(mode, scope)
      const darkLook = scope === 'well' || baseTheme(mode) === 'dark'
      for (const t of darkLook ? [...SURFACES, ...INKS] : SURFACES) {
        if (Y(night, t) > Y(day, t) + 1e-9) out.push(`${w}: ${t} ${hex(rgbOf(night, t))} is brighter than by day ${hex(rgbOf(day, t))}`)
      }
      if (darkLook) {
        for (const t of ['--text', '--well-ink']) {
          if (Y(night, t) > INK_DIM * Y(day, t)) out.push(`${w}: ${t} ${hex(rgbOf(night, t))} keeps ${((100 * Y(night, t)) / Y(day, t)).toFixed(0)}% of its day light (${hex(rgbOf(day, t))}); at most ${INK_DIM * 100}%`)
        }
      } else if (Y(night, '--panel') >= Y(day, '--panel')) {
        out.push(`${w}: the panel --panel ${hex(rgbOf(night, '--panel'))} is no dimmer than by day ${hex(rgbOf(day, '--panel'))}`)
      }
    }
  }
  // The accent and the readout (the roles Night dims), every preset, measured on the colour the
  // role is named for.
  for (const r of PALETTE_ROLES.filter((x) => x.dimsAtNight)) {
    const t = r.swatch
    for (const p of r.presets) {
      for (const base of NIGHT_BASES) {
        const mode = p === r.presets[0] ? base : withRoles(base, { [r.id]: p.id })
        for (const scope of SCOPES) {
          const [night, day] = [tokensIn(rules, mode, scope), tokensIn(rules, dayOf(mode), scope)]
          const [yn, yd] = [luminance(rgbOf(night, t)), luminance(rgbOf(day, t))]
          // Strictly dimmer: the accent everywhere (the spec's "dimmer accent"), the readout where
          // it glows. The light theme's readout only may not brighten.
          const strict = r.id === 'accent' || scope === 'well' || baseTheme(mode) === 'dark'
          if (strict ? yn >= yd : yn > yd) {
            out.push(`${r.id}=${p.id} ${where(mode, scope)}: ${t} ${hex(rgbOf(night, t))} is ${strict ? 'not dimmer than' : 'brighter than'} by day ${hex(rgbOf(day, t))}`)
          }
        }
      }
    }
  }
  return out
}

/** Night is warm: the neutral surfaces and inks move into the warm hues, and stay neutral enough
 *  that no ink reads as a status colour (a saturated warm ink would say "marginal"). */
function warmProblems(rules: Rule[]): string[] {
  const out: string[] = []
  for (const mode of NIGHT_BASES) {
    for (const scope of SCOPES) {
      const tk = tokensIn(rules, mode, scope)
      for (const t of [...SURFACES, ...INKS]) {
        const { C, H } = oklch(rgbOf(tk, t))
        // Pure black and white have no hue to be warm or cool; high contrast's page is black.
        if (C >= 0.005 && (H < WARM[0] || H > WARM[1])) out.push(`${where(mode, scope)}: ${t} ${hex(rgbOf(tk, t))} is not warm (hue ${H.toFixed(0)}°)`)
        if (C > NEUTRAL_C) out.push(`${where(mode, scope)}: ${t} ${hex(rgbOf(tk, t))} is a colour, not a warm neutral (chroma ${C.toFixed(3)})`)
      }
    }
  }
  return out
}

/** Nothing outside NIGHT_TOKENS moves at night, and no night rule declares anything outside it. */
function lockedProblems(rules: Rule[]): string[] {
  const out: string[] = []
  for (const rule of rules) {
    if (!rule.selector.includes('[data-night=')) continue
    for (const d of rule.decls) {
      if (d.prop.startsWith('--') && !NIGHT_TOKENS.has(d.prop)) out.push(`${rule.selector} { ${d.prop} } — not a night token`)
    }
  }
  for (const mode of NIGHT_MODES) {
    for (const scope of SCOPES) {
      const [night, day] = [tokensIn(rules, mode, scope), tokensIn(rules, dayOf(mode), scope)]
      for (const t of new Set([...day.keys(), ...night.keys()])) {
        if (NIGHT_TOKENS.has(t)) continue
        const follows = FOLLOWS_INK[t]
        if (follows) {
          // Measured at <html>, where both are declared. Inside a well the ON AIR rim is what
          // <html> computed (no well re-declares it) and the pending ink is the well's own
          // (styles-wells.test.ts holds a well to the dark palette, --state-pending included).
          if (scope === 'root' && valueOf(night, t) !== valueOf(night, follows)) out.push(`${where(mode, scope)}: ${t} no longer follows ${follows}`)
          continue
        }
        if (valueOf(night, t) !== valueOf(day, t)) out.push(`${where(mode, scope)}: ${t} is ${valueOf(night, t)} at night, ${valueOf(day, t)} by day — a locked colour moved`)
      }
    }
  }
  return out
}

/** The floors, in every night mode and colour-role set. */
function floorProblems(rules: Rule[]): string[] {
  const out: string[] = []
  for (const mode of NIGHT_MODES) {
    const inkMin = isHigh(mode) ? TEXT_MIN_HIGH : TEXT_MIN
    for (const scope of SCOPES) {
      const tk = tokensIn(rules, mode, scope)
      const w = where(mode, scope)
      const floor = (fg: string, bg: string, min: number, kind: string) => {
        const v = contrast(rgbOf(tk, fg), rgbOf(tk, bg))
        if (v < min) out.push(`${w}: ${kind} ${fg} ${hex(rgbOf(tk, fg))} on ${bg} ${hex(rgbOf(tk, bg))} = ${v.toFixed(2)}:1 < ${min}:1`)
      }
      const textOn = scope === 'well' ? ['--well-bg'] : ['--panel', '--bg', '--bg-elev', '--bg-elev-2']
      for (const ink of INKS) for (const s of textOn) floor(ink, s, inkMin, 'text')
      for (const s of scope === 'well' ? ['--well-bg'] : ['--panel', '--bg']) floor('--tx', s, TX_MIN, 'TX')
      for (const t of STATUS) floor(t, scope === 'well' ? '--well-bg' : '--panel', STATUS_MIN, 'status')
      for (const t of ['--accent', '--focus-ring']) {
        for (const red of ['--tx', '--alert-critical']) {
          const d = deltaE(rgbOf(tk, t), rgbOf(tk, red))
          if (d < LOCKED_DE) out.push(`${w}: ${t} ${hex(rgbOf(tk, t))} is ΔE ${d.toFixed(3)} from ${red} ${hex(rgbOf(tk, red))} < ${LOCKED_DE}`)
        }
      }
    }
  }
  return out
}

// ── The night looks ─────────────────────────────────────────────────────────────────────────

describe('Night is live in both themes, with and without high contrast', () => {
  it('sweeps the four night looks under every colour-role set', () => {
    expect(NIGHT_BASES).toEqual(['dark-night', 'light-night', 'dark-night-high', 'light-night-high'])
    expect(NIGHT_MODES.length).toBe(NIGHT_BASES.length * (1 + (MODES.length / BASE_MODES.length - 1)))
  })

  it('the night block wins: every surface and ink changes, at <html> and inside a well', () => {
    expect(liveProblems(RULES)).toEqual([])
  })
})

describe('Night is darker and warmer', () => {
  it('gives out less light: nothing brighter, the brightest ink and the page dimmer, the accent and readout dimmer for every preset', () => {
    expect(darkerProblems(RULES)).toEqual([])
  })

  it('moves the surfaces and inks into warm, neutral hues', () => {
    expect(warmProblems(RULES)).toEqual([])
  })
})

describe('TX, alert and signal colours are exactly what they are by day', () => {
  it('no night rule declares anything but a night token, and nothing else resolves differently', () => {
    expect(lockedProblems(RULES)).toEqual([])
  })
})

describe('everything still reads at night', () => {
  it('text 4.5:1 (7:1 with high contrast), TX 3:1, status 3:1, and the accent clear of the reds', () => {
    expect(floorProblems(RULES)).toEqual([])
  })
})

// ── Positive controls: each check can say no ────────────────────────────────────────────────

/** The real sheet plus a block of our own, parsed on the same order counter (so it comes last). */
const withBlock = (css: string): Rule[] => [...RULES, ...parseRules(css, { n: ORDER.n })]

describe('the checks fire', () => {
  it('a night block that retunes --tx is caught twice: by its declaration and by what it paints', () => {
    const rules = withBlock(`[data-theme='dark'][data-night='1'] { --tx: #ff8080; }`)
    const found = lockedProblems(rules)
    expect(found).toContain(`[data-theme='dark'][data-night='1'] { --tx } — not a night token`)
    expect(found.some((m) => m.startsWith('dark-night root: --tx is #ff8080 at night'))).toBe(true)
  })

  it('a night accent that lands on the TX red is caught', () => {
    const rules = withBlock(`[data-theme='dark'][data-night='1'] { --accent: #d94848; }`)
    expect(floorProblems(rules).some((m) => m.startsWith('dark-night root: --accent #d94848 is ΔE') && m.includes('from --tx'))).toBe(true)
  })

  it('a sheet with no night rules is caught as inert', () => {
    const rules = RULES.filter((r) => !r.selector.includes('[data-night='))
    expect(liveProblems(rules).some((m) => m.startsWith('dark-night root: --text is'))).toBe(true)
  })

  it('a night ink brighter than by day, or a cold one, is caught', () => {
    const rules = withBlock(`[data-theme='dark'][data-night='1'] { --text: #ffffff; --text-dim: #9aa8bd; }`)
    expect(darkerProblems(rules).some((m) => m.startsWith('dark-night root: --text #ffffff is brighter than by day'))).toBe(true)
    expect(warmProblems(rules).some((m) => m.startsWith('dark-night root: --text-dim #9aa8bd is not warm'))).toBe(true)
  })

  it('a night panel too dark for the light theme’s locked inks is caught', () => {
    const rules = withBlock(`[data-theme='light'][data-night='1'] { --panel: #c8c0b4; }`)
    expect(floorProblems(rules).some((m) => m.startsWith('light-night root: status --status-new-band'))).toBe(true)
  })
})
