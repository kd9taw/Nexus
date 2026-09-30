// @vitest-environment jsdom
//
// THE POTA / SOTA BOARD'S HUNT BUTTON AND PARK REFERENCE READ IN EVERY LIGHT THEME, IN EVERY HOST (operator, 2026-09-30:
// "Separate fix, every host": "A small light-theme fix to the board wherever it shows").
//
// The board letters its HUNT button and each row's park or summit reference in the accent, which was tuned as a mark: as
// lettering in the light themes the HUNT read 3.45:1 on its own accent tint and the reference 4.25:1 on the row (Chrome,
// the POTA view), under the 4.5:1 floor. In the light themes each takes the theme's ink with the accent kept: HUNT on its
// tint and its border, now at full strength, and the reference as its underline. Dark is untouched.
//
// THE HOSTS. The board (PotaSotaView) renders in the POTA / SOTA view, in its pop-out, and on the Remote page (observing,
// with HUNT offered). Each is rendered here in its chain, with rows in every state a row has (plain, the hunted row, a band
// that is open, a new park), and the cascade is resolved with the app's own resolver (cssCascade.ts).
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { ReactNode } from 'react'
import type { AppSnapshot, OtaSpot } from '../types'
import type { ObservedOta } from '../otaHunt'
import { PALETTE_ROLES } from '../features/paletteRoles'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  SENTINEL_MODES,
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

const spot = (activator: string, reference: string, over: Partial<OtaSpot> = {}): OtaSpot => ({
  program: 'POTA', reference, name: 'Test park', activator, freqKhz: 14_285, mode: 'SSB', spotter: null, comment: null, grid: null,
  newPark: false, bandOpen: false, huntedToday: false, ...over,
})
/** A row in every state a row has: plain, the hunted one, a band that is open, a new park (and a summit, the other program). */
const SPOTS: OtaSpot[] = [
  spot('K1ABC', 'US-0001'),
  spot('W9XYZ', 'US-0002', { freqKhz: 7_185 }),
  spot('N0OPN', 'US-0003', { freqKhz: 21_285, bandOpen: true }),
  spot('K4NEW', 'US-0004', { freqKhz: 18_130, newPark: true }),
]
const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
  selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'

beforeAll(() => {
  ;(Element.prototype as unknown as { scrollIntoView: () => void }).scrollIntoView = () => {}
})
afterEach(cleanup)

const snap = { hunt: { program: 'POTA', reference: 'US-0002', call: 'W9XYZ' }, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot
const OBSERVED: ObservedOta = {
  feeds: [{ program: 'POTA', status: 'ready', sourceAgeMs: 1000, spots: SPOTS }],
  activation: { program: null, reference: null, qsoCount: 0 } as ObservedOta['activation'],
  hunt: snap.hunt,
  parkCount: 0,
  huntedCount: 0,
}
const HOSTS: Array<[string, () => ReactNode]> = [
  ['the POTA / SOTA view', () => <div className="app"><main className="layout single"><PotaSotaView snap={snap} onHunt={() => {}} onSnap={() => {}} /></main></div>],
  ['the pop-out', () => <div className="app detached"><PotaSotaView snap={snap} onSnap={() => {}} detached onHunt={() => {}} /></div>],
  ['the Remote page', () => (
    <div className="app">
      <div className="remote-insights-view remote-ota-view">
        <div className="remote-ota-bank">
          <PotaSotaView snap={snap} observation={OBSERVED} remote={{ hunt: () => {} }} />
        </div>
      </div>
    </div>
  )],
]

interface Word {
  host: string
  what: string
  own: string
  chain: El[]
}
const BODY: El = { tag: 'body', classes: [], attrs: {} }
async function renderWords(): Promise<Word[]> {
  const out: Word[] = []
  for (const [host, make] of HOSTS) {
    api.getOtaSpots.mockResolvedValue(SPOTS)
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<>{make()}</>)
    })
    for (let k = 0; k < 4; k++)
      await act(async () => {
        await new Promise((res) => setTimeout(res, 0))
      })
    const seen = new Set<Element>()
    const walker = document.createTreeWalker(r.container, NodeFilter.SHOW_TEXT)
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const el = n.parentElement
      if (!el || seen.has(el) || !/[\p{L}\p{N}]/u.test(n.textContent ?? '') || el.closest('[hidden]') || !el.closest('li.pota-spot')) continue
      seen.add(el)
      const own = el.classList.length ? `.${[...el.classList].join('.')}` : el.tagName.toLowerCase()
      const row = el.closest('li.pota-spot')!
      out.push({ host, own, what: `${host} ${[...row.classList].join('.')} ${own} "${(n.textContent ?? '').trim().slice(0, 16)}"`, chain: [BODY, ...chainOf(el)] })
    }
    cleanup()
  }
  return out
}

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The board as it shipped: the light-theme rules on its HUNT button and park reference removed. */
const SHIPPED = RULES.filter((r) => !(r.selector.startsWith("[data-theme='light'] ") && /\.pota-(hunt-btn|spot-ref)\b/.test(r.selector)))

const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
const accent = PALETTE_ROLES.find((r) => r.id === 'accent')!
/** Every built-in light theme, and the standard light theme under each accent preset (the accent is these words' colour). */
const LIGHT: Mode[] = [
  ...BASE_MODES.filter((b) => baseTheme(b) === 'light').flatMap((b) => skinsOf('light').map((skin) => (skin ? withRoles(b, { skin }) : b))),
  ...accent.presets.slice(1).map((p) => withRoles('light', { accent: p.id })),
]
const DARK: Mode[] = [...BASE_MODES.filter((b) => baseTheme(b) === 'dark'), ...SENTINEL_MODES.filter((m) => m.startsWith('dark skin='))]

const memo = new Map<string, unknown>()
function once<T>(key: string, make: () => T): T {
  if (!memo.has(key)) memo.set(key, make())
  return memo.get(key) as T
}
const keyOf = (rules: Rule[], mode: Mode, at: El[]) => `${rules === SHIPPED ? 's' : 'r'}|${mode}|${JSON.stringify(at)}`
const tokens = (rules: Rule[], mode: Mode, at: El[]) => once(`t|${keyOf(rules, mode, at)}`, () => tokensAt(rules, mode, at))
const win = (rules: Rule[], mode: Mode, at: El[], ...props: string[]) => once(`w|${keyOf(rules, mode, at)}|${props.join()}`, () => winnerAt(rules, mode, at, ...props))
const BLANK = /^(inherit|transparent|none|initial|unset)$/i
const colourOf = (rules: Rule[], mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(tokens(rules, mode, at), value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}
/** What an element sits on: every background from it outward, down to the first opaque one, composited. */
function surfaceOf(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const layers: Array<{ value: string; at: El[] }> = []
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = win(rules, mode, at, 'background', 'background-color')
    if (!w || BLANK.test(w.value.trim())) continue
    layers.push({ value: w.value, at })
    if (hex(colourOf(rules, mode, at, w.value, [0, 0, 0])) === hex(colourOf(rules, mode, at, w.value, [255, 255, 255]))) break
  }
  return layers.reduceRight((under, l) => colourOf(rules, mode, l.at, l.value, under), colourOf(rules, mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0]))
}
/** The declaration that paints the word: its own, else the nearest ancestor's (it inherits). */
function inkOf(rules: Rule[], mode: Mode, chain: El[]) {
  for (let i = chain.length; i > 0; i--) {
    const w = win(rules, mode, chain.slice(0, i), 'color')
    if (w && !BLANK.test(w.value.trim())) return { value: w.value, at: chain.slice(0, i) }
  }
  throw new Error('nothing paints it')
}
function wordOf(rules: Rule[], mode: Mode, w: Word) {
  const bg = surfaceOf(rules, mode, w.chain)
  const ink = inkOf(rules, mode, w.chain)
  const fg = colourOf(rules, mode, ink.at, ink.value, bg)
  return { fg, bg, ratio: contrast(fg, bg), ink }
}
const accentAt = (rules: Rule[], mode: Mode, at: El[]) => hex(colourOf(rules, mode, at, 'var(--accent)', [0, 0, 0]))

/** The two words this is about, by the class that names them, and how each keeps the accent in the light themes. */
const KINDS: Record<string, 'border' | 'underline'> = { '.pota-hunt-btn': 'border', '.pota-spot-ref': 'underline' }
/** A border declaration's colour: the value itself, or a `border` shorthand without its width and style. */
const borderColour = (v: string) => v.replace(/^\s*[\d.]+(px|em|rem)\s+/, '').replace(/^(solid|dashed|dotted|double)\s+/, '').trim()

describe("the POTA / SOTA board's HUNT and park reference read in every light theme, in every host", () => {
  let all: Word[] = []
  let words: Word[] = []
  beforeAll(async () => {
    all = await renderWords()
    words = all.filter((w) => w.own in KINDS)
  }, 60_000)

  // THE INVENTORY: each host, and each of the two words in each row state the data gives it. Exact, so a host that stops
  // rendering the board, or a row state that stops lettering them, cannot leave the sweep silently.
  const ROWS = ['pota-spot.pota-spot-v2', 'pota-spot.pota-spot-v2.pota-spot-new', 'pota-spot.pota-spot-v2.pota-spot-open', 'pota-spot.pota-spot-v2.selected']
  it('finds HUNT and the reference on every row, in every host (the census cannot silently empty out)', () => {
    const seen = [...new Set(words.map((w) => w.what.replace(/ "[^"]*"$/, '')))].sort()
    const want = HOSTS.flatMap(([h]) => ROWS.flatMap((r) => Object.keys(KINDS).map((k) => `${h} ${r} ${k}`))).sort()
    expect(seen).toEqual(want)
  })

  it('in every light theme and under every accent each reads 4.5:1 in the ink, the accent kept on its border or as its underline, 3:1', () => {
    const low: string[] = []
    for (const w of words)
      for (const mode of LIGHT) {
        const { fg, bg, ratio } = wordOf(RULES, mode, w)
        if (ratio < 4.5) low.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
        if (KINDS[w.own] === 'underline') {
          const line = win(RULES, mode, w.chain, 'text-decoration-line')?.value
          const mark = line === 'underline' ? win(RULES, mode, w.chain, 'text-decoration-color')?.value : null
          if (mark !== 'var(--accent)') {
            low.push(`${w.what} ${mode}: the accent is not its underline (${line} ${mark})`)
            continue
          }
          const c = colourOf(RULES, mode, w.chain, mark, bg)
          if (contrast(c, bg) < 3) low.push(`${w.what} ${mode}: the underline ${hex(c)} on ${hex(bg)} = ${contrast(c, bg).toFixed(2)}:1`)
        } else {
          const v = win(RULES, mode, w.chain, 'border-top-color', 'border-color', 'border')?.value ?? ''
          if (borderColour(v) !== 'var(--accent)') {
            low.push(`${w.what} ${mode}: its border is ${v}, not the accent at full strength`)
            continue
          }
          const under = surfaceOf(RULES, mode, w.chain.slice(0, -1))
          const c = colourOf(RULES, mode, w.chain, 'var(--accent)', under)
          if (contrast(c, under) < 3) low.push(`${w.what} ${mode}: the border ${hex(c)} on ${hex(under)} = ${contrast(c, under).toFixed(2)}:1`)
        }
      }
    expect(low).toEqual([])
  }, 120_000)

  it('in every dark theme each still letters in the accent, the reference with no underline and HUNT with its mixed border', () => {
    const moved: string[] = []
    for (const w of words)
      for (const mode of DARK) {
        const { fg, ink } = wordOf(RULES, mode, w)
        if (hex(fg) !== accentAt(RULES, mode, ink.at)) moved.push(`${w.what} ${mode}: lettered in ${hex(fg)}, not the accent`)
        if (KINDS[w.own] === 'underline') {
          const line = win(RULES, mode, w.chain, 'text-decoration-line', 'text-decoration')?.value
          if (line && line !== 'none') moved.push(`${w.what} ${mode}: underlined in dark`)
        } else {
          const v = win(RULES, mode, w.chain, 'border-top-color', 'border-color', 'border')?.value ?? ''
          if (!v.includes('color-mix(')) moved.push(`${w.what} ${mode}: its border is ${v}, not its mix`)
        }
      }
    expect(moved).toEqual([])
  }, 120_000)

  it('FIRES: the board as it shipped is caught in the light theme in every host, at the ratios Chrome measured', () => {
    const shipped: string[] = []
    for (const w of words) {
      const { fg, bg, ratio } = wordOf(SHIPPED, 'light', w)
      if (ratio < 4.5) shipped.push(`${w.what}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
    }
    const has = (re: RegExp) => shipped.some((m) => re.test(m))
    for (const [h] of HOSTS) {
      expect(has(new RegExp(`^${h} pota-spot\\.pota-spot-v2 \\.pota-hunt-btn "HUNT": #0174ab on #c1d7e5 = 3\\.45:1$`)), `${h}: HUNT`).toBe(true)
      expect(has(new RegExp(`^${h} pota-spot\\.pota-spot-v2 \\.pota-spot-ref "US-0001": #0174ab on #e5eaf0 = 4\\.25:1$`)), `${h}: the reference`).toBe(true)
    }
    // The hunted row, on its accent tint, is lower still (the same pair in Chrome, 3.20:1 from its unrounded colours).
    expect(has(/ pota-spot\.pota-spot-v2\.selected \.pota-hunt-btn "HUNT": #0174ab on #add2e4 = 3\.2[01]:1$/), 'HUNT on the hunted row').toBe(true)
  }, 60_000)
})
