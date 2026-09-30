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
// built-in skin in each of its base's modes, on the desktop sheet and on the Remote page's, and the
// surface under it is composited from every ancestor's own background.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { RotorPane } from './RotorPane'
import { SKINS } from '../../features/skins'
import {
  BASE_MODES,
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
import type { RotatorState } from '../../types'

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(123)),
  readRotatorState: vi.fn(
    (): Promise<RotatorState | null> =>
      Promise.resolve({ azDeg: 123, reading: 'position', elDeg: 45, elRange: [0, 180] }),
  ),
  pointRotator: vi.fn(() => Promise.resolve()),
  pointRotatorElevation: vi.fn(() => Promise.resolve()),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never)),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
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
const REMOTE = withHover(
  parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css') + '\n' + sheet('remote-web/application.css')),
)

const TEXT_MIN = 4.5
const CONTROL_MIN = 3
/** Every base mode, and every built-in skin in each of its base's four modes. */
const MODES: Mode[] = [
  ...BASE_MODES,
  ...SKINS.flatMap((s) => skinBaseModes(s.id).map((b): Mode => `${b} skin=${s.id}`)),
]

afterEach(cleanup)

let stop: El[] = []
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

/** What lies under the button: every ancestor's own background, composited from the page down. */
function surfaceUnder(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  let under = colour(tokensAt(rules, mode, chain.slice(0, 1)), 'var(--bg)', [0, 0, 0])
  for (let i = 0; i < chain.length - 1; i++) {
    const at = chain.slice(0, i + 1)
    const bg = winnerAt(rules, baseOf(mode), at, 'background', 'background-color')
    if (bg) under = colour(tokensAt(rules, mode, at), bg.value, under)
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
  const tokens = tokensAt(rules, mode, chain)
  const under = surfaceUnder(rules, mode, chain)
  const face = winnerAt(rules, baseOf(mode), chain, 'background', 'background-color')
  const ink = winnerAt(rules, baseOf(mode), chain, 'color')
  const border = winnerAt(rules, baseOf(mode), chain, 'border', 'border-color', 'border-top-color')
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

  it('letters at 4.5:1 on its own face, idle and hovered, and keeps a 3:1 outline, on the desktop and on Remote', () => {
    expect(failures([['desktop', DESKTOP], ['Remote', REMOTE]])).toEqual([])
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
