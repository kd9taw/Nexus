// @vitest-environment jsdom
//
// THE PROVENANCE CHIP READS IN EVERY LIGHT THEME (found measuring the dashboard window, 2026-09-29).
//
// The chip that says where the propagation numbers came from — LIVE, PARTIAL, CACHED 12m, NO LIVE
// DATA — letters its word in the state's colour: the OK green for live, the warning amber for the
// rest. It is drawn in three places (Connect's Conditions pane, the map's bar, the DXpeditions view)
// and in all three it sits on the page colour, where the state colours read 3.4–4.3:1 as lettering
// in every light theme — measured in Chrome, CACHED #a76d00 on #e5eaf0 = 3.59:1 and LIVE #007f35 on
// it 4.25:1 in the standard light theme — under the 4.5:1 floor. In every dark theme they read
// 8.8:1 and up.
//
// The fix is the band chip's (#382), which the operator approved: in the light theme the word takes
// the theme's text colour and the state keeps its colour on the chip's border. A border is held to
// 3:1 (a state colour, WCAG 1.4.11), which these colours meet on these surfaces. Dark is untouched.
//
// This renders the three hosts and resolves, with the app's own resolver (cssCascade.ts), what
// paints each chip's word and what it sits on: in every light theme (the four light modes, bare and
// on each built-in light theme, under every OK and every amber preset) the word must clear 4.5:1 and
// the border 3:1; in every dark theme the word is painted by the same declaration as before.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { ConnectView } from './ConnectView'
import { DxpeditionsView } from './DxpeditionsView'
import { PALETTE_ROLES } from '../features/paletteRoles'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  baseTheme,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  toRgb,
  tokensAt,
  winnerAt,
  withRoles,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'
import type { PropagationSnapshot } from '../types'
import FIXTURE from '../remote-web/__fixtures__/navigation-connect.json'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathOutlook: vi.fn(async () => null),
  getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
  getKc2gMuf: vi.fn(async () => []),
  getXrayNow: vi.fn(async () => null),
  getDxpedWindows: vi.fn(async () => []),
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getContests: vi.fn(async () => []),
}))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The chip as it shipped before this fix: the light-theme rules this guard exists for, removed. */
const SHIPPED = RULES.filter((r) => !(r.selector.startsWith("[data-theme='light'] ") && /\.(prop|map)-prov\.prov-/.test(r.selector)))

const TEXT_MIN = 4.5
const EDGE_MIN = 3
const STATES = ['live', 'partial', 'cached', 'offline'] as const
type State = (typeof STATES)[number]
/** The token each state paints in (styles.css, `.prop-prov.prov-*` and `.map-prov.prov-*`). */
const STATE_INK: Record<State, string> = { live: '--band-open', partial: '--alert-warning', cached: '--alert-warning', offline: '--alert-warning' }

/** Every preset of the two roles that colour the chip, one at a time, and neither (the defaults). */
const role = (id: string) => PALETTE_ROLES.find((r) => r.id === id)!
const PRESETS: Record<string, string>[] = [
  {},
  ...role('amber').presets.slice(1).map((p) => ({ amber: p.id })),
  ...role('ok').presets.slice(1).map((p) => ({ ok: p.id })),
]
const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
/** Every built-in light theme: the four light modes, bare and on each light theme, under every preset. */
const LIGHT: Mode[] = BASE_MODES.filter((b) => baseTheme(b) === 'light').flatMap((b) =>
  skinsOf('light').flatMap((skin) => PRESETS.map((roles) => withRoles(b, skin ? { skin, ...roles } : roles))),
)
/** Every dark theme, bare: a preset declares only custom properties (styles-palette-roles.test.ts),
 *  so it cannot change WHICH declaration paints the word, which is what "unchanged" is held to. */
const DARK: Mode[] = BASE_MODES.filter((b) => baseTheme(b) === 'dark').flatMap((b) => skinsOf('dark').map((skin) => (skin ? withRoles(b, { skin }) : b)))

interface Chip {
  host: string
  state: State | 'loading'
  chain: El[]
}

function snapOf(source: State): PropagationSnapshot {
  return { ...(FIXTURE.prop as unknown as PropagationSnapshot), source, asOf: Math.floor(Date.now() / 1000) - 12 * 60 }
}

async function renderChips(): Promise<Chip[]> {
  const out: Chip[] = []
  const read = (host: string, root: ParentNode, sel: string) => {
    for (const el of root.querySelectorAll(sel)) {
      const state = [...el.classList].find((c) => c.startsWith('prov-'))!.slice(5) as Chip['state']
      // From <body>, which paints the page and the ink every word inherits (chainOf stops below it).
      out.push({ host, state, chain: [{ tag: 'body', classes: [], attrs: {} }, ...chainOf(el)] })
    }
  }
  for (const source of [...STATES, null]) {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(
        <div className="app">
          <ConnectView
            myGrid="EN52"
            theme="light"
            stations={[]}
            prop={source ? snapOf(source) : null}
            selectedCall={null}
            onSelectCall={() => {}}
            needByCall={new Map()}
            needAlerts={[]}
            amp={null}
          />
        </div>,
      )
    })
    read('Connect Conditions', r.container, '.pane-frame .prop-prov')
    read('the map bar', r.container, '.map-toolbar .map-prov')
    cleanup()
    if (!source) continue
    await act(async () => {
      r = render(
        <div className="app">
          <main className="layout single">
            <DxpeditionsView snap={snapOf(source)} />
          </main>
        </div>,
      )
    })
    read('DXpeditions', r.container, '.prop-prov')
    cleanup()
  }
  return out
}

/** A chain's shape as the sheet sees it (a render's ids and labels are not selected on). */
const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-\w+)$/.test(k))]))
const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const tokensFor = (rules: Rule[], mode: Mode, chain: El[]) => once(rules, `t|${mode}|${chainKey(chain)}`, () => tokensAt(rules, mode, chain))
/** A preset changes only custom properties, so the mode without its presets decides a winner. */
const winnerMode = (mode: Mode): Mode => mode.split(' ').filter((p, i) => i === 0 || p.startsWith('skin=')).join(' ') as Mode
const win = (rules: Rule[], mode: Mode, chain: El[], ...props: string[]) =>
  once(rules, `w|${winnerMode(mode)}|${chainKey(chain)}|${props.join()}`, () => winnerAt(rules, winnerMode(mode), chain, ...props))

const colourOf = (rules: Rule[], mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(tokensFor(rules, mode, at), value), under)
  if (!c) throw new Error(`not a colour: "${value}" → "${expandWith(tokensFor(rules, mode, at), value)}"`)
  return c
}
const BLANK = /^(inherit|transparent|none|initial|unset)$/i
const opaque = (rules: Rule[], mode: Mode, at: El[], value: string) =>
  hex(colourOf(rules, mode, at, value, [0, 0, 0])) === hex(colourOf(rules, mode, at, value, [255, 255, 255]))

/** What the element sits on: every background from it outward, down to the first opaque one (the
 *  `.app` host paints the page colour), composited — a pane frame is a translucent glass. */
function surfaceOf(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const layers: Array<{ value: string; at: El[] }> = []
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = win(rules, mode, at, 'background', 'background-color')
    if (!w || BLANK.test(w.value.trim())) continue
    layers.push({ value: w.value, at })
    if (opaque(rules, mode, at, w.value)) break
  }
  let under = colourOf(rules, mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0])
  for (let k = layers.length - 1; k >= 0; k--) under = colourOf(rules, mode, layers[k].at, layers[k].value, under)
  return under
}

/** The declaration that paints the word: the chip's own, else the nearest ancestor's (it inherits). */
function inkDecl(rules: Rule[], mode: Mode, chain: El[]) {
  for (let i = chain.length; i > 0; i--) {
    const w = win(rules, mode, chain.slice(0, i), 'color')
    if (w && !BLANK.test(w.value.trim())) return { value: w.value, at: chain.slice(0, i), rule: w.rule }
  }
  return null
}

function wordOf(rules: Rule[], mode: Mode, chip: Chip) {
  const bg = surfaceOf(rules, mode, chip.chain)
  const ink = inkDecl(rules, mode, chip.chain)
  if (!ink) throw new Error(`${chip.host} ${chip.state} ${mode}: nothing in the sheet paints the word`)
  const fg = colourOf(rules, mode, ink.at, ink.value, bg)
  return { fg, bg, ratio: contrast(fg, bg) }
}

/** Every chip × mode whose word reads under 4.5:1 on what it sits on. */
function unreadable(rules: Rule[], chips: Chip[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const chip of chips)
    for (const mode of modes) {
      const { fg, bg, ratio } = wordOf(rules, mode, chip)
      if (ratio < TEXT_MIN) out.push(`${chip.host} ${chip.state} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
    }
  return out
}

describe('the provenance chip reads in every light theme', () => {
  let chips: Chip[] = []
  beforeAll(async () => {
    chips = await renderChips()
  })
  const stated = () => chips.filter((c): c is Chip & { state: State } => c.state !== 'loading')

  it('renders the chip in every state it has, in all three places (the census cannot silently empty out)', () => {
    const seen = (host: string) => chips.filter((c) => c.host === host).map((c) => c.state).sort()
    // The Conditions pane shows no chip once there is no live data (it says so in words instead).
    expect(seen('Connect Conditions')).toEqual(['cached', 'live', 'partial'])
    expect(seen('the map bar')).toEqual(['cached', 'live', 'loading', 'offline', 'partial'])
    expect(seen('DXpeditions')).toEqual(['cached', 'live', 'offline', 'partial'])
  })

  it('every word clears 4.5:1 in every light theme, under every OK and amber preset', () => {
    expect(unreadable(RULES, chips, LIGHT)).toEqual([])
  }, 60_000)

  it('the state keeps its colour on the border, which stands 3:1 off the surface in every light theme', () => {
    const low: string[] = []
    for (const chip of stated())
      for (const mode of LIGHT) {
        const border = win(RULES, mode, chip.chain, 'border', 'border-color', 'border-top-color')
        expect(border?.value, `${chip.host} ${chip.state} ${mode}: the border`).toContain(`var(${STATE_INK[chip.state]})`)
        const bg = surfaceOf(RULES, mode, chip.chain)
        const edge = colourOf(RULES, mode, chip.chain, `var(${STATE_INK[chip.state]})`, bg)
        const r = contrast(edge, bg)
        if (r < EDGE_MIN) low.push(`${chip.host} ${chip.state} ${mode}: ${hex(edge)} on ${hex(bg)} = ${r.toFixed(2)}:1`)
      }
    expect(low).toEqual([])
  }, 60_000)

  it('in every dark theme the word is painted by the same declaration as before: the state colour', () => {
    const moved: string[] = []
    for (const chip of stated())
      for (const mode of DARK) {
        const now = inkDecl(RULES, mode, chip.chain)
        const was = inkDecl(SHIPPED, mode, chip.chain)
        if (now?.value !== was?.value || now?.rule.selector !== was?.rule.selector) moved.push(`${chip.host} ${chip.state} ${mode}: ${was?.value} → ${now?.value}`)
        if (now?.value !== `var(${STATE_INK[chip.state]})`) moved.push(`${chip.host} ${chip.state} ${mode}: not the state colour (${now?.value})`)
      }
    expect(moved).toEqual([])
  }, 60_000)

  it('FIRES: the chip as it shipped is caught in the light theme, at the ratios Chrome measured, and not in the dark one', () => {
    const found = unreadable(SHIPPED, chips, LIGHT)
    // The resolver must agree with what Chrome painted, or it is not measuring the same thing.
    expect(found).toContain('Connect Conditions cached light: #a76d00 on #e5eaf0 = 3.59:1')
    expect(found).toContain('the map bar live light: #007f35 on #e5eaf0 = 4.25:1')
    expect(found.some((m) => m.startsWith('DXpeditions offline light skin=paper')), 'the paper theme').toBe(true)
    expect(unreadable(SHIPPED, chips, BASE_MODES.filter((b) => baseTheme(b) === 'dark'))).toEqual([])
  }, 60_000)
})
