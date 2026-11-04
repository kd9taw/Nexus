// @vitest-environment jsdom
//
// THE ROTOR PANE'S ■ STOP READS IN EVERY BUILT-IN THEME (operator, 2026-09-29: "a small follow-up
// fixes STOP's low-contrast red in the light theme").
//
// STOP was hard-coded `#f87171`, a red chosen for the dark theme: on the light theme's panel it
// letters at about 2.6:1, under the 4.5:1 text needs, and its border sits at the same ratio,
// under the 3:1 a control's outline needs. Its hover turned the face that red with `#111`
// lettering, which only a light red can carry. The pane's STOP now takes the stop colour every
// other stop control in the app takes (`--state-weak`: the cockpits' Stop TX, Operate's Stop),
// and its hover letters in the panel's own colour on that red, so each theme's red is matched by
// its own ink.
//
// Measured, not asserted by name: the button is rendered in the Connect pane's host chain, its
// face, ink and border are the cascade winners (cssCascade) under every base mode and every
// built-in skin in each of its base's modes, on the desktop sheet, and the surface under it is
// composited from every ancestor's own background.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { RotorPane } from './RotorPane'
import { SKINS } from '../../features/skins'
import {
  BASE_MODES,
  MODES as CASCADE_MODES,
  SKIN_MODES,
  baseOf,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  skinBaseModes,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../../cssCascade'
import { isLockedToken } from '../../features/paletteRoles'
import type { CallBearing, RotatorState } from '../../types'

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(123)),
  readRotatorState: vi.fn(
    (): Promise<RotatorState | null> =>
      Promise.resolve({ azDeg: 123, reading: 'position', elDeg: 45, elRange: [0, 180] }),
  ),
  getMoonStatus: vi.fn(async () => null),
  startMoonTrack: vi.fn(async () => ({ state: 'following', elevation: true, azDeg: null, elDeg: null })),
  pointRotator: vi.fn(() => Promise.resolve()),
  pointRotatorElevation: vi.fn(() => Promise.resolve()),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never)),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  rotatorBearingToCall: vi.fn(
    (): Promise<CallBearing> =>
      Promise.resolve({ pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 }),
  ),
}))
vi.mock('../../api', () => api)
vi.mock('../../toast', () => ({ pushToast: vi.fn() }))

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
/** The resolver reads `:hover` as a state the element is not in; the hovered face is measured by
 *  naming that state as an attribute on the button's own chain. */
const withHover = (rules: Rule[]) =>
  rules.map((r) => (r.selector.includes(':hover') ? { ...r, selector: r.selector.replace(/:hover/g, '[data-hover]') } : r))
const DESKTOP = withHover(parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css')))

const TEXT_MIN = 4.5
const CONTROL_MIN = 3
/** Every base mode, and every built-in skin in each of its base's four modes. */
const MODES: Mode[] = [
  ...BASE_MODES,
  ...SKINS.flatMap((s) => skinBaseModes(s.id).map((b): Mode => `${b} skin=${s.id}`)),
]

afterEach(cleanup)

let stop: El[] = []
/** The station's line and its bug (line 2's bearing, so a call in the entry), and the antenna's needle. */
let bearing: El[] = []
let bug: El[] = []
let needle: El[] = []
beforeAll(async () => {
  // The Connect pane's host chain: the app, the pane frame and its body, then the pane itself.
  const r = render(
    <div className="app">
      <div className="pane-frame" data-pane="rotor">
        <div className="pane-body">
          <RotorPane />
        </div>
      </div>
    </div>,
  )
  const button = await waitFor(() => {
    const b = r.container.querySelector<HTMLButtonElement>('.rotor-stop')
    if (!b) throw new Error('the Rotor pane rendered no STOP')
    return b
  })
  stop = chainOf(button)
  cleanup()
  const withCall = render(
    <div className="app">
      <div className="pane-frame" data-pane="rotor">
        <div className="pane-body">
          <RotorPane entryCall="EC1DD" />
        </div>
      </div>
    </div>,
  )
  const drawn = await waitFor(() => {
    const l = withCall.container.querySelector('.rotor-bearing-line')
    if (!l) throw new Error('the Rotor pane drew no station line for the call in the entry')
    return l
  })
  bearing = chainOf(drawn)
  bug = chainOf(withCall.container.querySelector('.rotor-bearing-bug')!)
  needle = chainOf(withCall.container.querySelector('.rotor-needle:not(.target)')!)
  cleanup()
})

const colour = (tokens: Map<string, string>, value: string, under: Rgb): Rgb => {
  const v = expandWith(tokens, value).trim()
  const c = toRgb(v === 'none' ? 'transparent' : v, under)
  if (!c) throw new Error(`not a colour: "${value}" → "${v}"`)
  return c
}

/** Split a value at the spaces outside any `(…)`: `1px solid color-mix(in srgb, …)` is three. */
function spaceSplit(s: string): string[] {
  const out: string[] = []
  let depth = 0
  let buf = ''
  for (const ch of s.trim()) {
    if (ch === '(') depth++
    else if (ch === ')') depth--
    if (/\s/.test(ch) && depth === 0) {
      if (buf) out.push(buf)
      buf = ''
    } else buf += ch
  }
  if (buf) out.push(buf)
  return out
}

/** The colour in a `border` shorthand, or a `border-color` value as it stands. */
const borderColour = (value: string): string =>
  spaceSplit(value)
    .filter((t) => !/^-?[\d.]+(px|em|rem)?$/.test(t) && !/^(solid|dashed|dotted|double|groove|ridge|inset|outset|none|hidden|thin|medium|thick)$/.test(t))
    .join(' ')

// Each answer the sweep needs is resolved once, keyed by the exact arguments it is computed from,
// so nothing below is approximated. Resolved afresh at every step, the desktop and Remote sweep
// took 4.9 s alone, and on a loaded full suite (2026-10-01) it ran past its 20 s budget: at a fifth
// of a CPU it took 27 s. Most of that was repeated work. The winners are resolved under the base
// mode, so the forty skin modes reuse their base's eight. The ancestors' winners and tokens are the
// same idle and hovered, because hover marks only the button. Tokens are resolved over the rules
// that declare a custom property (186 of the desktop sheet's 4269), since no other rule can set
// one. Shared this way, the sweep takes 0.9 s and yields the same colours in every case.
const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const tokenRules = (rules: Rule[]) => once(rules, 'token rules', () => rules.filter((r) => r.decls.some((d) => d.prop.startsWith('--'))))
const tokensFor = (rules: Rule[], mode: Mode, chain: El[]) =>
  once(rules, `t|${mode}|${JSON.stringify(chain)}`, () => tokensAt(tokenRules(rules), mode, chain))
const win = (rules: Rule[], mode: Mode, chain: El[], ...props: string[]) =>
  once(rules, `w|${baseOf(mode)}|${JSON.stringify(chain)}|${props.join()}`, () => winnerAt(rules, baseOf(mode), chain, ...props))

/** What lies under the button: every ancestor's own background, composited from the page down. */
function surfaceUnder(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  let under = colour(tokensFor(rules, mode, chain.slice(0, 1)), 'var(--bg)', [0, 0, 0])
  for (let i = 0; i < chain.length - 1; i++) {
    const at = chain.slice(0, i + 1)
    const bg = win(rules, mode, at, 'background', 'background-color')
    if (bg) under = colour(tokensFor(rules, mode, at), bg.value, under)
  }
  return under
}

interface Look {
  ink: Rgb
  face: Rgb
  border: Rgb
  under: Rgb
}

function look(rules: Rule[], mode: Mode, chain: El[]): Look {
  const tokens = tokensFor(rules, mode, chain)
  const under = surfaceUnder(rules, mode, chain)
  const face = win(rules, mode, chain, 'background', 'background-color')
  const ink = win(rules, mode, chain, 'color')
  const border = win(rules, mode, chain, 'border', 'border-color', 'border-top-color')
  if (!ink || !border) throw new Error(`the browser draws STOP's ${ink ? 'border' : 'ink'} under ${mode}`)
  const faceRgb = face ? colour(tokens, face.value, under) : under
  return {
    under,
    face: faceRgb,
    ink: colour(tokens, ink.value, faceRgb),
    border: colour(tokens, borderColour(border.value), under),
  }
}

const hovered = (chain: El[]): El[] => {
  const last = chain[chain.length - 1]
  return [...chain.slice(0, -1), { ...last, attrs: { ...last.attrs, 'data-hover': '' } }]
}

/** Every mode, sheet and state in which STOP falls under a floor. */
function failures(sets: [string, Rule[]][]): string[] {
  const out: string[] = []
  for (const [name, rules] of sets) {
    for (const mode of MODES) {
      for (const [state, chain] of [['idle', stop], ['hover', hovered(stop)]] as const) {
        const l = look(rules, mode, chain)
        const text = contrast(l.ink, l.face)
        if (text < TEXT_MIN) out.push(`${name} ${mode} ${state}: STOP ${hex(l.ink)} on ${hex(l.face)} = ${text.toFixed(2)}:1`)
        if (state === 'idle') {
          const edge = contrast(l.border, l.under)
          if (edge < CONTROL_MIN) out.push(`${name} ${mode}: STOP's border ${hex(l.border)} on ${hex(l.under)} = ${edge.toFixed(2)}:1`)
        }
      }
    }
  }
  return out
}

describe("the Rotor pane's STOP reads in every built-in theme", () => {
  it('measures the real button, in every base mode and every skin (the sweep cannot silently empty out)', () => {
    expect(stop[stop.length - 1]?.classes).toContain('rotor-stop')
    expect(MODES.length).toBe(BASE_MODES.length + SKINS.reduce((n, s) => n + skinBaseModes(s.id).length, 0))
    expect(MODES.filter((m) => m.startsWith('light')).length).toBeGreaterThan(4)
  })

  it('letters at 4.5:1 on its own face, idle and hovered, and keeps a 3:1 outline, on the desktop', () => {
    expect(failures([['desktop', DESKTOP]])).toEqual([])
  }, 20_000)

  it('FIRES: the STOP as it shipped, #f87171 on transparent and #111 on #f87171 hovered, is caught in the light theme', () => {
    const shipped = DESKTOP.map((r) =>
      r.selector.trim() === '.rotor-stop'
        ? { ...r, decls: [{ prop: 'background', value: 'transparent' }, { prop: 'border', value: '1px solid #f87171' }, { prop: 'color', value: '#f87171' }] }
        : r.selector.trim() === '.rotor-stop[data-hover]'
          ? { ...r, decls: [{ prop: 'background', value: '#f87171' }, { prop: 'color', value: '#111' }] }
          : r,
    )
    const found = failures([['shipped', shipped]])
    expect(found.some((m) => m.startsWith('shipped light idle: STOP #f87171')), found.join('\n')).toBe(true)
    expect(found.some((m) => m.startsWith("shipped light: STOP's border #f87171")), found.join('\n')).toBe(true)
  }, 20_000)
})

// THE STATION'S LINE (line 2's bearing on the rose) IS DRAWN IN ONE THEME COLOUR, the amber role: a
// variable both themes define, never a raw colour and never a locked one (features/paletteRoles), and so
// whatever the operator picks for Amber. A line a reading depends on keeps the 3:1 a graphical object needs
// (WCAG 1.4.11) on the surface under the rose, and it is never the antenna needle's own colour — though its
// shape, dotted with a bug on the rim, is what tells the two apart, never the colour alone.
const GRAPHIC_MIN = 3
/** Every base mode under every colour-role set, and every skin in each of its base modes, bare and under
 *  every set: the Amber role drives the line, so each of its presets is drawn. */
const LINE_MODES: Mode[] = [...new Set<Mode>([...CASCADE_MODES, ...SKIN_MODES])]

/** Every mode in which the station's line or its bug is not drawn as the rule above says. */
function lineFailures(rules: Rule[], modes: readonly Mode[] = LINE_MODES): string[] {
  const out: string[] = []
  for (const mode of modes) {
    const under = surfaceUnder(rules, mode, bearing)
    const n = win(rules, mode, needle, 'stroke')
    const needleInk = n ? colour(tokensFor(rules, mode, needle), n.value, under) : null
    for (const [what, chain, prop] of [['line', bearing, 'stroke'], ['bug', bug, 'fill']] as const) {
      const w = win(rules, mode, chain, prop)
      const token = w?.value.match(/^var\((--[\w-]+)\)$/)?.[1]
      if (!w || !token) {
        out.push(`${mode}: the ${what} is drawn in ${w?.value ?? 'nothing'}, not one theme colour`)
        continue
      }
      if (isLockedToken(token)) {
        out.push(`${mode}: the ${what} is drawn in the locked ${token}`)
        continue
      }
      const tokens = tokensFor(rules, mode, chain)
      if (!tokens.has(token)) {
        out.push(`${mode}: the ${what}'s ${token} is not defined`)
        continue
      }
      const ink = colour(tokens, w.value, under)
      const c = contrast(ink, under)
      if (c < GRAPHIC_MIN) out.push(`${mode}: the ${what} ${hex(ink)} on ${hex(under)} = ${c.toFixed(2)}:1`)
      if (needleInk && hex(needleInk) === hex(ink)) out.push(`${mode}: the ${what} is the needle's own ${hex(ink)}`)
    }
  }
  return out
}

/** The sheet with the station line's stroke written as `stroke`. */
const lineStroked = (stroke: string) =>
  DESKTOP.map((r) =>
    r.selector.trim() === '.rotor-bearing-line'
      ? { ...r, decls: r.decls.map((d) => (d.prop === 'stroke' ? { ...d, value: stroke } : d)) }
      : r,
  )

describe("the station's line on the rose is drawn in a theme colour of its own, in every built-in theme", () => {
  it('measures the real line, its bug and the needle, in both themes, every skin and every Amber preset', () => {
    expect(bearing[bearing.length - 1]?.classes).toContain('rotor-bearing-line')
    expect(bug[bug.length - 1]?.classes).toContain('rotor-bearing-bug')
    expect(needle[needle.length - 1]?.classes).toEqual(['rotor-needle'])
    expect(LINE_MODES.filter((m) => m.startsWith('light')).length).toBeGreaterThan(4)
    expect(LINE_MODES.filter((m) => m.includes('amber=')).length).toBeGreaterThan(0)
  })

  it('is one unlocked theme variable defined in every mode, keeps 3:1 on the rose, and is not the needle', () => {
    expect(lineFailures(DESKTOP)).toEqual([])
  }, 20_000)

  it('FIRES: a raw colour, the needle\'s own accent, a locked colour and an undefined one are each caught', () => {
    // Over the base modes only: each rule has to be seen to fire once, not swept again.
    const raw = lineFailures(lineStroked('#ffb000'), BASE_MODES)
    expect(raw.some((m) => m.endsWith('the line is drawn in #ffb000, not one theme colour')), raw[0]).toBe(true)
    const accent = lineFailures(lineStroked('var(--accent)'), BASE_MODES)
    expect(accent.some((m) => m.startsWith("dark: the line is the needle's own")), accent[0]).toBe(true)
    const locked = lineFailures(lineStroked('var(--tx)'), BASE_MODES)
    expect(locked.some((m) => m.endsWith('the line is drawn in the locked --tx')), locked[0]).toBe(true)
    const missing = lineFailures(lineStroked('var(--rotor-bearing-ink)'), BASE_MODES)
    expect(missing.some((m) => m.endsWith("the line's --rotor-bearing-ink is not defined")), missing[0]).toBe(true)
  })
})
