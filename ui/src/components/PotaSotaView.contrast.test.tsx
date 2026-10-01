// @vitest-environment jsdom
//
// THE POTA / SOTA BOARD'S HUNT BUTTON, PARK REFERENCE, BADGES AND HUNTING LINE READ IN EVERY LIGHT THEME, IN EVERY HOST
// (operator, 2026-09-30: "Separate fix, every host": "A small light-theme fix to the board wherever it shows").
//
// The board letters its HUNT button and each row's park or summit reference in the accent, which was tuned as a mark: as
// lettering in the light themes the HUNT read 3.45:1 on its own accent tint and the reference 4.25:1 on the row (Chrome,
// the POTA view), under the 4.5:1 floor. In the light themes each takes the theme's ink with the accent kept: HUNT on its
// tint and its border, now at full strength, and the reference as its underline. The row badges read under the floor
// too (NEW PARK 3.36:1, the accent on its own tint; BAND OPEN 1.66:1, a fixed green on its own; WORKED TODAY 3.93:1, the
// faint ink on its own): the first two take the ink with their colour on their border, and WORKED TODAY, a fact about the
// log rather than a mark, takes the dim ink. The Hunting line above the list letters the hunted park and call in the
// accent on its accent tint (4.12:1): they take the reference's look.
//
// THEN (the operator, 2026-09-30, "Fix both" and "Measure and fix"): the hunted row's frequency line takes the ink in every
// theme (4.41:1 on the paper theme, 3.64:1 in the dark one); a small dark batch, as N50's: WORKED TODAY takes the dim ink
// in every theme (4.47:1 in the dark one) and the hunted row's HUNT takes the ink with the accent on its border (4.20:1 on
// the Slate theme). Program's Tune and Add buttons share HUNT's look and take its light rule; ADDED keeps its green on its
// border. Everything else in dark is as it was.
//
// THE HOSTS. The board (PotaSotaView) renders in the POTA / SOTA view, in its pop-out, and on the Remote page (observing,
// with HUNT offered). Each is rendered here in its chain, with a hunt target (so the Hunting line shows) and rows in every
// state a row has (plain, the hunted row, a band that is open, a new park, a park worked today), and the cascade is resolved
// with the app's own resolver (cssCascade.ts). Program (RadioProgView, the desktop's Program view) is the fourth host: its
// results with a machine to Tune and Add and one already ADDED. An off-air row (dimmed whole, by opacity) and a digital one
// (its Add disabled) are left out: neither is a word this rule reads.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { ReactNode } from 'react'
import type { AppSnapshot, OtaSpot, RepeaterSearchResult } from '../types'
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
  // Worked today (its badge shows with Hide worked today switched off, which renderWords does).
  spot('W8WKD', 'US-0005', { freqKhz: 3_860, huntedToday: true }),
]
const api = vi.hoisted(() => {
  /** A repeater search with two FM machines (Program's Tune and Add on each; the second is added below, so it reads ADDED). */
  const machine = (callsign: string, outputMhz: number) => ({
    record: {
      source: 'repeaterbook', sourceId: callsign, callsign, outputMhz, inputMhz: outputMhz - 0.6, ctcssEncHz: null, ctcssDecHz: null, dcs: null,
      lat: 39.9, lon: -76.6, city: 'Red Lion', county: 'York', state: 'Pennsylvania', fm: true, dmr: false, dstar: false, fusion: false,
      dmrColorCode: null, bandwidthKhz: null, operational: true, openUse: true, distanceKm: 5, bearingDeg: 90,
    },
    channel: {
      id: callsign.toLowerCase(), name: callsign, rxMhz: outputMhz, duplex: 'minus', offsetMhz: 0.6, toneMode: 'tone', rtoneHz: 100, ctoneHz: 100,
      dtcsCode: 23, mode: 'fm', comment: 'Red Lion', source: { source: 'repeaterbook', sourceId: callsign, callsign },
    },
  })
  const result = { source: 'repeaterbook', fetchedUtc: 1_700_000_000, stale: false, coverageGap: null, missingStates: [],
    rows: [machine('W3ZGD', 146.865), machine('K3RLN', 147.09)] } as unknown as RepeaterSearchResult
  return {
    getOtaSpots: vi.fn(async (): Promise<OtaSpot[]> => []),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    repeaterSearch: vi.fn(async () => result),
    radioprogListProjects: vi.fn(async () => []),
  }
})
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
  selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'
import { RadioProgView } from './RadioProgView'

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
/** Each host, and what it needs done once it has rendered (Program's search is fetched, and its second machine added). */
const HOSTS: Array<[string, () => ReactNode, ((c: HTMLElement) => Promise<void>)?]> = [
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
  ['the Program view', () => <div className="app"><main className="layout single"><RadioProgView myGrid="FN31" catOk /></main></div>, async (c) => {
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /fetch/i }))
    })
    await waitFor(() => expect(c.querySelectorAll('.rp-row .rp-add').length).toBe(2))
    await act(async () => {
      fireEvent.click(c.querySelectorAll<HTMLButtonElement>('.rp-row .rp-add')[1])
    })
    await waitFor(() => expect(c.querySelector('.rp-row .rp-add.added')).toBeTruthy())
  }],
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
  for (const [host, make, prepare] of HOSTS) {
    api.getOtaSpots.mockResolvedValue(SPOTS)
    localStorage.setItem('nexus.ota.hideWorked', '0')
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<>{make()}</>)
    })
    for (let k = 0; k < 4; k++)
      await act(async () => {
        await new Promise((res) => setTimeout(res, 0))
      })
    await prepare?.(r.container)
    const seen = new Set<Element>()
    const walker = document.createTreeWalker(r.container, NodeFilter.SHOW_TEXT)
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const el = n.parentElement
      if (!el || seen.has(el) || !/[\p{L}\p{N}]/u.test(n.textContent ?? '') || el.closest('[hidden]') || !el.closest('li.pota-spot, .pota-hunt-banner, .rp-row')) continue
      // A disabled control need not read (a digital machine's Add, for one).
      if (el.closest('button:disabled')) continue
      seen.add(el)
      // A word with no class of its own is named by its parent's (the Hunting line's park and call are its <strong>s).
      const own = el.classList.length ? `.${[...el.classList].join('.')}` : `.${[...el.parentElement!.classList].join('.')} ${el.tagName.toLowerCase()}`
      const row = (el.closest('li.pota-spot') ?? el.closest('.pota-hunt-banner') ?? el.closest('.rp-row'))!
      out.push({ host, own, what: `${host} ${[...row.classList].join('.')} ${own} "${(n.textContent ?? '').trim().slice(0, 16)}"`, chain: [BODY, ...chainOf(el)] })
    }
    cleanup()
  }
  return out
}

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** This change's rules: the light ones on the HUNT look (the board's and Program's), the reference, the badges, the Hunting
 *  line and ADDED; and, in every theme, the hunted row's HUNT and frequency line and WORKED TODAY's dim ink. */
const OURS = /^\[data-theme='light'\] (\.pota-view )?\.pota-(hunt-btn|hunt-text|spot-ref|badge-(new|open|worked))\b|^\[data-theme='light'\] \.rp-add\.added$|^\.pota-view \.pota-(spot-v2\.selected \.pota-(hunt-btn|spot-meta)|badge-worked)$/
/** The board and Program's buttons as they shipped: this change's rules removed. */
const SHIPPED = RULES.filter((r) => !OURS.test(r.selector))
/** The colour a kind's own base rule letters it in (the last such rule wins its ties). */
const baseInk = (selector: string) => [...RULES].reverse().find((r) => r.selector === selector && r.decls.some((d) => d.prop === 'color'))!.decls.find((d) => d.prop === 'color')!.value

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

/** The words this is about, by the classes that name them: how each keeps its colour in the light themes (on its border
 *  at full strength, or as its underline; WORKED TODAY is a fact about the log, not a mark, and only has to read), and the
 *  base rule that letters it, which in dark must still be what paints it. */
const KINDS: Record<string, { mark: 'border' | 'underline' | 'none'; colour?: string; base: string; dark?: string }> = {
  '.pota-hunt-btn': { mark: 'border', colour: 'var(--accent)', base: '.pota-hunt-btn' },
  '.pota-spot-ref': { mark: 'underline', colour: 'var(--accent)', base: '.pota-spot-ref' },
  '.pota-badge.pota-badge-new': { mark: 'border', colour: 'var(--accent)', base: '.pota-badge-new' },
  '.pota-badge.pota-badge-open': { mark: 'border', colour: 'var(--band-open)', base: '.pota-badge-open' },
  '.pota-badge.pota-badge-worked': { mark: 'none', base: '.pota-badge-worked', dark: 'var(--text-dim)' },
  '.pota-hunt-text strong': { mark: 'underline', colour: 'var(--accent)', base: '.pota-hunt-text strong' },
  '.pota-spot-meta': { mark: 'none', base: '.pota-spot-meta' },
  '.pota-hunt-btn.rp-tune': { mark: 'border', colour: 'var(--accent)', base: '.pota-hunt-btn' },
  '.pota-hunt-btn.rp-add': { mark: 'border', colour: 'var(--accent)', base: '.pota-hunt-btn' },
  '.pota-hunt-btn.rp-add.added': { mark: 'border', colour: 'var(--state-good)', base: '.rp-add.added' },
}
/** The hunted row, where HUNT and the frequency line take the ink in every theme (HUNT with the accent on its border). */
const onHunted = (w: Word) => w.what.includes(' pota-spot.pota-spot-v2.selected ')
const huntedInk = (w: Word) => onHunted(w) && (w.own === '.pota-hunt-btn' || w.own === '.pota-spot-meta')
/** What letters a word in the dark themes: the ink on the hunted row, else the dark batch's colour for its kind, else its own
 *  base rule. */
const darkInk = (w: Word) => (huntedInk(w) ? 'var(--text)' : (KINDS[w.own].dark ?? baseInk(KINDS[w.own].base)))
const BOARD_HOSTS = HOSTS.filter(([h]) => h !== 'the Program view')
/** A border declaration's colour: the value itself, or a `border` shorthand without its width and style. */
const borderColour = (v: string) => v.replace(/^\s*[\d.]+(px|em|rem)\s+/, '').replace(/^(solid|dashed|dotted|double)\s+/, '').trim()

describe("the POTA / SOTA board's words and Program's HUNT-look buttons read in every theme, in every host", () => {
  let all: Word[] = []
  let words: Word[] = []
  beforeAll(async () => {
    all = await renderWords()
    words = all.filter((w) => w.own in KINDS)
  }, 60_000)

  // THE INVENTORY: each host, and each word in each row state the data gives it: HUNT, the reference and the frequency line
  // on every row, NEW PARK on the new park, BAND OPEN on the open band, WORKED TODAY on the worked row, and the Hunting line's
  // park and call; Program's Tune, Add and ADDED. Exact, so a host that stops rendering the board, or a row that stops
  // lettering one, cannot leave the sweep silently.
  const ROWS = ['pota-spot.pota-spot-v2', 'pota-spot.pota-spot-v2.pota-spot-new', 'pota-spot.pota-spot-v2.pota-spot-open', 'pota-spot.pota-spot-v2.selected']
  const BADGE_ROWS: Record<string, string> = {
    '.pota-badge.pota-badge-new': 'pota-spot.pota-spot-v2.pota-spot-new',
    '.pota-badge.pota-badge-open': 'pota-spot.pota-spot-v2.pota-spot-open',
    '.pota-badge.pota-badge-worked': 'pota-spot.pota-spot-v2',
  }
  it('finds HUNT, the reference and the frequency line on every row, each badge on its own and the Hunting line, in every host, and Program\'s buttons (the census cannot silently empty out)', () => {
    const seen = [...new Set(words.map((w) => w.what.replace(/ "[^"]*"$/, '')))].sort()
    const want = [
      ...BOARD_HOSTS.flatMap(([h]) => [
        ...ROWS.flatMap((r) => ['.pota-hunt-btn', '.pota-spot-ref', '.pota-spot-meta'].map((k) => `${h} ${r} ${k}`)),
        ...Object.entries(BADGE_ROWS).map(([k, r]) => `${h} ${r} ${k}`),
        `${h} pota-hunt-banner .pota-hunt-text strong`,
      ]),
      ...['.pota-hunt-btn.rp-tune', '.pota-hunt-btn.rp-add', '.pota-hunt-btn.rp-add.added'].map((k) => `the Program view rp-row ${k}`),
    ].sort()
    expect(seen).toEqual(want)
  })

  it('in every light theme and under every accent each reads 4.5:1, in the ink with its colour kept on its border or as its underline, 3:1', () => {
    const low: string[] = []
    for (const w of words)
      for (const mode of LIGHT) {
        const { fg, bg, ratio } = wordOf(RULES, mode, w)
        if (ratio < 4.5) low.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
        const kind = KINDS[w.own]
        if (kind.mark === 'underline') {
          const line = win(RULES, mode, w.chain, 'text-decoration-line')?.value
          const mark = line === 'underline' ? win(RULES, mode, w.chain, 'text-decoration-color')?.value : null
          if (mark !== kind.colour) {
            low.push(`${w.what} ${mode}: ${kind.colour} is not its underline (${line} ${mark})`)
            continue
          }
          const c = colourOf(RULES, mode, w.chain, mark!, bg)
          if (contrast(c, bg) < 3) low.push(`${w.what} ${mode}: the underline ${hex(c)} on ${hex(bg)} = ${contrast(c, bg).toFixed(2)}:1`)
        } else if (kind.mark === 'border') {
          const v = win(RULES, mode, w.chain, 'border-top-color', 'border-color', 'border')?.value ?? ''
          if (borderColour(v) !== kind.colour) {
            low.push(`${w.what} ${mode}: its border is ${v}, not ${kind.colour} at full strength`)
            continue
          }
          const under = surfaceOf(RULES, mode, w.chain.slice(0, -1))
          const c = colourOf(RULES, mode, w.chain, kind.colour!, under)
          if (contrast(c, under) < 3) low.push(`${w.what} ${mode}: the border ${hex(c)} on ${hex(under)} = ${contrast(c, under).toFixed(2)}:1`)
        }
      }
    expect(low).toEqual([])
  }, 120_000)

  // THE DARK THEMES. Every word reads 4.5:1 in every dark theme: the dark batch took the three that did not (WORKED TODAY 4.47:1,
  // the hunted row's HUNT 4.20:1 on Slate and its frequency line 3.64:1).
  it('in every dark theme each reads 4.5:1', () => {
    const low: string[] = []
    for (const w of words)
      for (const mode of DARK) {
        const { fg, bg, ratio } = wordOf(RULES, mode, w)
        if (ratio < 4.5) low.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
      }
    expect(low).toEqual([])
  }, 120_000)

  // And dark keeps its look apart from that batch: each word is lettered by its own base rule, or the batch's ink where it set
  // one (so a light rule that escaped its theme would show here); the reference has no underline; a bordered word keeps its
  // mixed border, but the hunted row's HUNT, which keeps the accent at full strength on its border.
  it("in every dark theme each is lettered by its own rule or the dark batch's ink, the reference with no underline, the rest with their borders", () => {
    const moved: string[] = []
    for (const w of words)
      for (const mode of DARK) {
        const kind = KINDS[w.own]
        const { fg, bg } = wordOf(RULES, mode, w)
        const want = colourOf(RULES, mode, w.chain, darkInk(w), bg)
        if (hex(fg) !== hex(want)) moved.push(`${w.what} ${mode}: lettered in ${hex(fg)}, not ${darkInk(w)} ${hex(want)}`)
        if (kind.mark === 'underline') {
          const line = win(RULES, mode, w.chain, 'text-decoration-line', 'text-decoration')?.value
          if (line && line !== 'none') moved.push(`${w.what} ${mode}: underlined in dark`)
        } else if (kind.mark === 'border') {
          const v = win(RULES, mode, w.chain, 'border-top-color', 'border-color', 'border')?.value ?? ''
          if (huntedInk(w) ? borderColour(v) !== 'var(--accent)' : !v.includes('color-mix(')) moved.push(`${w.what} ${mode}: its border is ${v}`)
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
    for (const [h] of BOARD_HOSTS) {
      expect(has(new RegExp(`^${h} pota-spot\\.pota-spot-v2 \\.pota-hunt-btn "HUNT": #0174ab on #c1d7e5 = 3\\.45:1$`)), `${h}: HUNT`).toBe(true)
      expect(has(new RegExp(`^${h} pota-spot\\.pota-spot-v2 \\.pota-spot-ref "US-0001": #0174ab on #e5eaf0 = 4\\.25:1$`)), `${h}: the reference`).toBe(true)
      // The Hunting line's park and call, on its accent tint (Chrome: 4.12:1, the same pair).
      expect(has(new RegExp(`^${h} pota-hunt-banner \\.pota-hunt-text strong "US-0002": #0174ab on #d8e9f2 = 4\\.12:1$`)), `${h}: the Hunting line`).toBe(true)
    }
    // The hunted row, on its accent tint, is lower still (the same pair in Chrome, 3.20:1 from its unrounded colours).
    expect(has(/ pota-spot\.pota-spot-v2\.selected \.pota-hunt-btn "HUNT": #0174ab on #add2e4 = 3\.2[01]:1$/), 'HUNT on the hunted row').toBe(true)
    // The badges, on their own tints (Chrome: NEW PARK 3.36, BAND OPEN 1.66 and WORKED TODAY 3.93, the same pairs).
    expect(has(/ \.pota-badge\.pota-badge-new "NEW PARK": #0174ab on #bcd5e4 = 3\.3[67]:1$/), 'NEW PARK').toBe(true)
    expect(has(/ \.pota-badge\.pota-badge-open "BAND OPEN": #22c55e on #c2e3d6 = 1\.6[56]:1$/), 'BAND OPEN').toBe(true)
    expect(has(/ \.pota-badge\.pota-badge-worked "WORKED TODAY": #57647a on #cbd2db = 3\.9[34]:1$/), 'WORKED TODAY').toBe(true)
    // Program's buttons, the HUNT look on their tint, and ADDED's green on it (Chrome: 4.01:1 each, the same pairs).
    expect(has(/^the Program view rp-row \.pota-hunt-btn\.rp-tune "Tune": #0174ab on #d3e6f1 = 4\.0[01]:1$/), 'Tune').toBe(true)
    expect(has(/^the Program view rp-row \.pota-hunt-btn\.rp-add "＋ Add": #0174ab on #d3e6f1 = 4\.0[01]:1$/), 'Add').toBe(true)
    expect(has(/^the Program view rp-row \.pota-hunt-btn\.rp-add\.added "✓ Added": #007f35 on #d3e6f1 = 4\.0[01]:1$/), 'ADDED').toBe(true)
  }, 60_000)

  // The dark batch's three, and the paper theme's frequency line, as they shipped (Chrome, the same pairs; a ratio a unit
  // or two apart where the page's mix rounds).
  it('FIRES: the dark batch and the paper frequency line are caught as they shipped, at the ratios Chrome measured', () => {
    const at = (mode: Mode, re: RegExp) =>
      words.some((w) => {
        const { fg, bg, ratio } = wordOf(SHIPPED, mode, w)
        return re.test(`${w.what}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
      })
    expect(at('dark', /^the POTA \/ SOTA view pota-spot\.pota-spot-v2 \.pota-badge\.pota-badge-worked "WORKED TODAY": #7c8ca3 on #1f2630 = 4\.4[5-7]:1$/), 'WORKED TODAY, dark').toBe(true)
    expect(at('dark', /^the POTA \/ SOTA view pota-spot\.pota-spot-v2\.selected \.pota-spot-meta "7\.1850 MHz": #7c8ca3 on #1c3747 = 3\.64:1$/), 'the hunted frequency line, dark').toBe(true)
    expect(at('dark skin=slate' as Mode, /^the POTA \/ SOTA view pota-spot\.pota-spot-v2\.selected \.pota-hunt-btn "HUNT": #88c4d6 on #3d535f = 4\.20:1$/), 'the hunted HUNT, Slate').toBe(true)
    expect(at('light skin=paper' as Mode, /^the POTA \/ SOTA view pota-spot\.pota-spot-v2\.selected \.pota-spot-meta "7\.1850 MHz": #6a6353 on #d5dfe6 = 4\.4[01]:1$/), 'the hunted frequency line, paper').toBe(true)
  }, 60_000)
})
