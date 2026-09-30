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
}))

import { getWindowBehind } from '../api'
import { DashboardBar, StayBehindToggle } from './DashboardBar'

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

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

/** Every lettered element on the bar (`.prop-prov` is the panes' chip, for the control below). */
const LETTERED =
  '.dash-call, .dash-grid, .dash-time-v, .dash-time-k, .dash-index-k, .dash-index-v, .dash-prov, .prop-prov, .dash-behind'

interface Word {
  what: string
  chain: El[]
}

/** Render the bar in `host` (its toggle pressed or not, `extra` inside it) and read every word. */
async function wordsOn(host: string, pressed: boolean, extra?: React.ReactNode): Promise<Word[]> {
  vi.mocked(getWindowBehind).mockResolvedValue({ supported: true, on: pressed })
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(
      <div className={host}>
        <DashboardBar call="KD9TAW" grid="EN52" prop={PROP}>
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
    // The census cannot silently empty out: call, grid, two clocks with their labels, five
    // indices with their names, the provenance chip and the toggle, in each of four renders.
    expect(words.length, 'words found').toBe(4 * (2 + 4 + 10 + 1 + 1))
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

  it('FIRES: the panes’ chip, lettered in the warning colour, is caught on this bar in the light theme', async () => {
    // The shipped style this bar first reused, and the one the measurement found under the floor.
    const words = await wordsOn('app detached', false, <span className="prop-prov prov-cached">CACHED 12m</span>)
    const panes = words.filter((w) => w.what === 'app detached .prop-prov.prov-cached')
    expect(panes, 'control: the panes’ chip is on the bar').toHaveLength(1)
    const failing = MODES.filter((mode) => unreadable(RULES, panes, [mode]).length > 0)
    // Chrome measured this chip at 4.23:1 in the standard light theme and 10.69:1 in the standard
    // dark one; the resolver must agree both ways, or it is not measuring what Chrome painted.
    expect(failing, 'the control must fire in the standard light theme').toContain('light')
    expect(failing, 'and not in the standard dark theme').not.toContain('dark')
  })
})
