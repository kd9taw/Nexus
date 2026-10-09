// @vitest-environment jsdom
//
// EVERY WORD ON THE DASHBOARD BAR READS, IN EVERY THEME AND MODE — the bar across the top of the
// Connect pop-out and the TV page. Computed from the sheet with the app's own resolver
// (cssCascade.ts), the way the band chip and the Operate button are held. jsdom lays nothing
// out, so this measures what the cascade PAINTS, not where it lands (the harness in the report
// measured that in Chrome).
//
// Why it exists: the bar first reused the panes' provenance chip, which letters its warning in
// the warning colour. Chrome measured that at 4.23:1 on this bar in the light theme, under the
// 4.5:1 floor, and this guard went red on the same chip before the fix. The warning now rides on
// the chip's EDGE, held to 3:1 like any state colour, and the word is in ink.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import {
  MODES,
  baseTheme,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'
import type { PropagationSnapshot } from '../types'

vi.mock('../api', () => ({
  getWindowBehind: vi.fn(),
  setWindowBehind: vi.fn(),
  // NOAA's daily file, so the SSN's date is lettered on the bar and read below.
  getSolarIndices: vi.fn(() => Promise.resolve({ days: [{ dayUnix: Math.floor(Date.now() / 86_400_000) * 86_400 - 86_400, sfi: 142, ssn: 46 }] })),
}))

import { getWindowBehind } from '../api'
import { DashboardBar, StayBehindToggle } from './DashboardBar'

// THE BUDGET (2026-10-09). The slowest case without a budget of its own, "FIRES: a word lettered in the warning
// colour is caught…", takes 0.65 s and 0.59 s on one core (two runs); a loaded full suite on this box has run cases
// up to 20 times slower than one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still
// fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The FIRES control's own rule, planted here and never shipped: a word lettered in the warning
 *  colour, the style this bar first reused, which Chrome measured under the floor in the light theme
 *  (#a76d00 on #fbfcfe = 4.23:1) and well over it in the dark one. A fix to any shipped rule cannot
 *  silence it: the panes' own chip was this control until it was fixed to read in the light themes. */
const FIXTURE = parseRules('.dash-fires-fixture { color: var(--alert-warning); }', { n: RULES.length + 1 })

const TEXT_MIN = 4.5
const EDGE_MIN = 3
/** The two hosts the bar is drawn in. */
const HOSTS = ['app detached', 'app tv-app'] as const

/** A CACHED snapshot, so the provenance chip is on the bar. */
const PROP = {
  advisory: { headline: '', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], active: [], upcoming: [] },
  spaceWx: { sfi: 142, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'cached',
  asOf: Math.floor(Date.now() / 1000) - 12 * 60,
} as unknown as PropagationSnapshot

/** Every lettered element on the bar (`.dash-fires-fixture` is the planted word for the control below). */
const LETTERED =
  '.dash-call, .dash-grid, .dash-time-v, .dash-time-k, .dash-index-k, .dash-index-v, .dash-index-d, .dash-prov, .dash-fires-fixture, .dash-behind'

interface Word {
  what: string
  chain: El[]
}

/** Render the bar in `host` (its toggle pressed or not, `extra` inside it) and read every word. */
async function wordsOn(host: string, pressed: boolean, extra?: React.ReactNode, prop: PropagationSnapshot = PROP): Promise<Word[]> {
  vi.mocked(getWindowBehind).mockResolvedValue({ supported: true, on: pressed })
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(
      <div className={host}>
        <DashboardBar call="KD9TAW" grid="EN52" prop={prop}>
          <StayBehindToggle />
          {extra}
        </DashboardBar>
      </div>,
    )
  })
  const out = [...r.container.querySelectorAll(LETTERED)].map((el) => ({
    what: `${host} .${[...el.classList].join('.')}${el.classList.contains('dash-behind') && pressed ? ' (pressed)' : ''}`,
    chain: chainOf(el),
  }))
  cleanup()
  return out
}

/** A chain's shape, as the sheet sees it: the ids and labels a render mints are not selected on.
 *  The key the memos below share, so the four renders' identical chains are resolved once. */
const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-label|aria-controls|aria-describedby)$/.test(k))]))
const memo = new Map<string, unknown>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  const k = `${rules.length}|${key}`
  if (!memo.has(k)) memo.set(k, make())
  return memo.get(k) as T
}

/** The winning declaration of `props` on the element or the nearest ancestor that has one:
 *  lettering inherits, and a face shows through from the first element that paints one. */
function nearest(rules: Rule[], mode: Mode, chain: El[], ...props: string[]) {
  return once(rules, `n|${mode}|${chainKey(chain)}|${props.join()}`, () => {
    for (let i = chain.length; i > 0; i--) {
      const at = chain.slice(0, i)
      const w = winnerAt(rules, mode, at, ...props)
      if (w && !/^(inherit|transparent|none)$/.test(w.value.trim())) return { value: w.value, at }
    }
    return null
  })
}

const colourOf = (rules: Rule[], mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const tokens = once(rules, `t|${mode}|${chainKey(at)}`, () => tokensAt(rules, mode, at))
  const c = toRgb(expandWith(tokens, value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}

/** One word per chain shape: the same shape reads the same in every render. */
const distinct = (words: Word[]) => [...new Map(words.map((w) => [chainKey(w.chain), w])).values()]

/** The page colour under everything: the host's own face, else the theme's page. */
const pageOf = (rules: Rule[], mode: Mode, chain: El[]): Rgb =>
  colourOf(rules, mode, chain.slice(0, 1), nearest(rules, mode, chain.slice(0, 1), 'background', 'background-color')?.value ?? 'var(--bg)', [0, 0, 0])

/** Every word × mode that reads under 4.5:1 on the face it sits on. */
function unreadable(rules: Rule[], words: Word[], modes: readonly Mode[] = MODES): string[] {
  const out: string[] = []
  for (const w of words) {
    for (const mode of modes) {
      const face = nearest(rules, mode, w.chain, 'background', 'background-color')
      const ink = nearest(rules, mode, w.chain, 'color')
      if (!face || !ink) {
        out.push(`${w.what} ${mode}: no ${face ? 'ink' : 'face'} from the sheet`)
        continue
      }
      const bg = colourOf(rules, mode, face.at, face.value, pageOf(rules, mode, w.chain))
      const fg = colourOf(rules, mode, ink.at, ink.value, bg)
      const r = contrast(fg, bg)
      if (r < TEXT_MIN) out.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${r.toFixed(2)}:1`)
    }
  }
  return out
}

afterEach(() => vi.mocked(getWindowBehind).mockReset())

describe('the dashboard bar reads in every theme and mode', () => {
  it('every word clears 4.5:1 on its face, in both hosts, the Stay behind toggle up and pressed', async () => {
    const words: Word[] = []
    for (const host of HOSTS) for (const pressed of [false, true]) words.push(...(await wordsOn(host, pressed)))
    // The census cannot silently empty out: call, grid, two clocks with their labels, six
    // indices with their names and SSN's date, the provenance chip and the toggle, in each of four
    // renders.
    expect(words.length, 'words found').toBe(4 * (2 + 4 + 13 + 1 + 1))
    expect(unreadable(RULES, distinct(words))).toEqual([])
    // Every mode × every shape through the resolver: seconds, not the default five.
  }, 60_000)

  it('the provenance chip carries its warning on an edge that stands 3:1 off the bar, in every mode', async () => {
    const chip = (await wordsOn('app detached', false)).find((w) => w.what.includes('dash-prov'))!
    expect(chip, 'control: the chip is on the bar').toBeTruthy()
    const low: string[] = []
    for (const mode of MODES) {
      const edge = nearest(RULES, mode, chip.chain, 'border', 'border-color')
      const colour = edge?.value.match(/var\(--[\w-]+\)|#[0-9a-fA-F]{3,8}/)?.[0]
      if (!edge || !colour) {
        low.push(`${mode}: no edge colour`)
        continue
      }
      const face = nearest(RULES, mode, chip.chain.slice(0, -1), 'background', 'background-color')!
      const bg = colourOf(RULES, mode, face.at, face.value, pageOf(RULES, mode, chip.chain))
      const fg = colourOf(RULES, mode, edge.at, colour, bg)
      if (contrast(fg, bg) < EDGE_MIN) low.push(`${mode}: ${hex(fg)} on ${hex(bg)} = ${contrast(fg, bg).toFixed(2)}:1`)
    }
    expect(low).toEqual([])
  })

  // A STORM (the operator's pick: "Warning colour on the bar" — amber, never the transmit red). The
  // warning lettering that the FIRES case below catches in the light theme is exactly what a storm
  // mark must not be there: in the light themes the number keeps the ink and the warning is an
  // underline in it, the app's rule for every state word on Connect.
  it('a storm: Kp, X-ray and the wind are amber on a dark bar, inked and underlined in amber on a light one, readable, never the transmit red', async () => {
    const storm = { ...PROP, source: 'live', spaceWx: { ...PROP.spaceWx, kp: 5, xrayClass: 'M1.2-class', solarWind: { bzNt: -6.1, btNt: 9.4, speedKms: 612, density: 7.2 } } } as PropagationSnapshot
    const words: Word[] = []
    for (const host of HOSTS) words.push(...(await wordsOn(host, false, undefined, storm)))
    const marked = words.filter((w) => w.what.endsWith('.dash-index-v') && 'data-warn' in (w.chain[w.chain.length - 2]?.attrs ?? {}))
    expect(marked.length, 'control: Kp, X-ray and SW are marked in both hosts').toBe(6)
    const wrong: string[] = []
    for (const w of distinct(marked))
      for (const mode of MODES) {
        const face = nearest(RULES, mode, w.chain, 'background', 'background-color')!
        const bg = colourOf(RULES, mode, face.at, face.value, pageOf(RULES, mode, w.chain))
        const ink = nearest(RULES, mode, w.chain, 'color')!
        const fg = hex(colourOf(RULES, mode, ink.at, ink.value, bg))
        const tx = hex(colourOf(RULES, mode, w.chain, 'var(--tx)', bg))
        const warning = hex(colourOf(RULES, mode, w.chain, 'var(--alert-warning)', bg))
        const line = winnerAt(RULES, mode, w.chain, 'text-decoration-line')?.value ?? 'none'
        const under = winnerAt(RULES, mode, w.chain, 'text-decoration-color')?.value
        if (fg === tx) wrong.push(`${w.what} ${mode}: lettered in the transmit red`)
        if (baseTheme(mode) === 'dark') {
          if (fg !== warning) wrong.push(`${w.what} ${mode}: ${fg}, not the warning colour ${warning}`)
        } else {
          const plain = hex(colourOf(RULES, mode, w.chain, 'var(--text)', bg))
          if (fg !== plain) wrong.push(`${w.what} ${mode}: ${fg}, not the ink ${plain}`)
          if (line !== 'underline' || !under) {
            wrong.push(`${w.what} ${mode}: no underline (${line})`)
            continue
          }
          const mark = colourOf(RULES, mode, w.chain, under, bg)
          if (hex(mark) !== warning) wrong.push(`${w.what} ${mode}: underlined in ${hex(mark)}, not the warning colour`)
          if (contrast(mark, bg) < EDGE_MIN) wrong.push(`${w.what} ${mode}: the underline ${hex(mark)} on ${hex(bg)} = ${contrast(mark, bg).toFixed(2)}:1`)
        }
      }
    expect(wrong, 'the warning colour, never the transmit red; in the light themes the ink and an underline').toEqual([])
    expect(unreadable(RULES, distinct(marked)), 'every marked number reads at 4.5:1').toEqual([])
    // The quiet bar's numbers are untouched: plain ink, no underline (the census above reads them).
    const quiet = (await wordsOn('app detached', false)).filter((w) => w.what.endsWith('.dash-index-v'))
    expect(quiet.some((w) => 'data-warn' in (w.chain[w.chain.length - 2]?.attrs ?? {})), 'control: nothing is marked on a quiet day').toBe(false)
    for (const mode of MODES) expect(winnerAt(RULES, mode, quiet[0].chain, 'text-decoration-line')?.value ?? 'none', mode).not.toBe('underline')
  }, 60_000)

  it('FIRES: a word lettered in the warning colour is caught on this bar in the light theme', async () => {
    // The planted rule above, owned by this test: the style the bar first reused.
    const words = await wordsOn('app detached', false, <span className="dash-fires-fixture">CACHED 12m</span>)
    const planted = words.filter((w) => w.what === 'app detached .dash-fires-fixture')
    expect(planted, 'control: the planted word is on the bar').toHaveLength(1)
    const failing = MODES.filter((mode) => unreadable([...RULES, ...FIXTURE], planted, [mode]).length > 0)
    // Chrome measured this lettering at 4.23:1 in the standard light theme and 10.69:1 in the standard
    // dark one; the resolver must agree both ways, or it is not measuring what Chrome painted.
    expect(failing, 'the control must fire in the standard light theme').toContain('light')
    expect(failing, 'and not in the standard dark theme').not.toContain('dark')
  })
})
