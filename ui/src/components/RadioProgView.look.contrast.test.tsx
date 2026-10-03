// @vitest-environment jsdom
//
// THE REPEATERS LOOK READS IN EVERY THEME (2026-10-02, the operator's "Conditions' look": "cards, section headers,
// chips and accent colours, readable in every theme").
//
// The view after a fetch, in the state an operator works it: the three cards with their section headers, the count
// line and its buttons, the list stamps as chips, the map's words and a dot's card, a row pointed at (linked) and a row
// selected, a saved machine's ✓ In Memories, a Save to Memories button, an added machine's ✓ Added, a digital-only
// row, and the channel list with its rows and buttons. Every word on it is held to 4.5:1 in every theme, computed with
// the app's own resolver (cssCascade.ts) over the rendered DOM, as the off-air sweep does for its rows.
import { describe, it, expect, vi, beforeAll, afterAll } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { RepeaterSearchResult } from '../types'
import { SKINS } from '../features/skins'
import { emptyBank, memoriesStore } from '../features/memories'
import { t } from '../i18n'
import {
  MODES,
  chainOf,
  compoundMatches,
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
} from '../cssCascade'

const api = vi.hoisted(() => {
  const machine = (callsign: string, outputMhz: number, o: { fm?: boolean; map?: boolean; links?: boolean } = {}) => ({
    record: {
      source: 'hearham', sourceId: callsign, callsign, outputMhz, inputMhz: outputMhz - 0.6, ctcssEncHz: 100, ctcssDecHz: null, dcs: null,
      lat: 41.9, lon: -88.0, city: 'Rockford', county: '', state: 'IL', fm: o.fm ?? true, dmr: !(o.fm ?? true), dstar: false, fusion: false,
      dmrColorCode: o.links ? 1 : null, bandwidthKhz: null, operational: true, openUse: true, distanceKm: 5, bearingDeg: 90,
      links: o.links ? [{ network: 'irlp', node: '3570' }] : [],
    },
    channel: {
      id: callsign.toLowerCase(), name: callsign, rxMhz: outputMhz, duplex: 'minus', offsetMhz: 0.6, toneMode: 'tone', rtoneHz: 100,
      ctoneHz: 100, dtcsCode: 23, mode: o.fm ?? true ? 'fm' : 'dmr', comment: 'Rockford', links: o.links ? ['IRLP 3570'] : [],
      dmrColorCode: o.links ? 1 : null, source: { source: 'hearham', sourceId: callsign, callsign },
    },
    sources: [{ source: 'hearham', sourceId: callsign, channelId: callsign.toLowerCase(), updated: null }],
    disagreements: [],
    ...(o.map ? { map: { lat: 41.9, lon: -88.0, callsign, outputMhz, city: 'Rockford' } } : {}),
  })
  const result = {
    lists: [
      { source: 'repeaterbook', fetchedUtc: 1_700_000_000, stale: false },
      { source: 'hearham', fetchedUtc: 1_700_000_000, stale: false },
    ],
    coverageGap: null, missingStates: [], rsgbUnavailable: false, rsgbBeyond: [],
    rows: [
      machine('W9AAA', 146.94, { map: true, links: true }),
      machine('W9BBB', 147.18, { map: true }),
      machine('W9CCC', 442.725),
      machine('W9DMR', 443.1, { fm: false }),
    ],
  } as unknown as RepeaterSearchResult
  const project = { id: 'working', name: 'My channels', createdUtc: 0, updatedUtc: 0, radiusKm: 0,
    origin: { kind: 'station', grid: 'EN52', label: 'EN52', lat: 0, lon: 0 }, channels: [result.rows[2].channel] }
  return {
    repeaterSearch: vi.fn(async () => result),
    radioprogListProjects: vi.fn(async () => [project]),
    radioprogFileNotice: vi.fn(async () => null),
    radioprogSaveProject: vi.fn(async () => undefined),
  }
})
vi.mock('../api', async (importOriginal) => ({ ...(await importOriginal<typeof import('../api')>()), ...api }))

import { RadioProgView } from './RadioProgView'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

interface Word {
  own: string
  text: string
  chain: El[]
}
const BODY: El = { tag: 'body', classes: [], attrs: {} }
const SKIP = 'option, select, textarea, .sr-only, canvas, script, style'

/** Repeaters fetched and worked, and every distinct word-carrying element on it (one per class chain and text kind). */
async function renderWords(): Promise<{ words: Word[]; kinds: Set<string> }> {
  memoriesStore.set({ ...emptyBank(), memories: [
    { id: 'm', name: 'W9BBB 18', kind: 'repeater', rxMhz: 147.18, mode: 'FM', groups: [], favorite: false, source: 'program', callsign: 'W9BBB' },
  ] })
  const { container } = render(<div className="app"><main className="layout single"><RadioProgView myGrid="EN52" catOk /></main></div>)
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: t('program.fetch.label') }))
  })
  await waitFor(() => expect(container.querySelectorAll('.rp-results .rp-row').length).toBe(3))
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: t('program.filters.digital.label') }))
  })
  await waitFor(() => expect(container.querySelectorAll('.rp-results .rp-row').length).toBe(4))
  const row = (call: string) => [...container.querySelectorAll<HTMLElement>('.rp-results .rp-row')].find((r) => r.querySelector('.rp-call')?.textContent === call)!
  // W9CCC is in the channel list (✓ Added); W9BBB is selected (its dot's card opens); W9AAA is pointed at.
  await act(async () => {
    fireEvent.click(row('W9BBB').querySelector('.rp-call')!)
  })
  await act(async () => {
    fireEvent.pointerEnter(row('W9AAA'))
  })
  await waitFor(() => expect(container.querySelector('.rp-map-card')).toBeTruthy())
  const out: Word[] = []
  const seen = new Set<string>()
  const walker = document.createTreeWalker(container.querySelector('.radioprog')!, NodeFilter.SHOW_TEXT)
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n.parentElement
    const text = (n.textContent ?? '').trim()
    if (!el || el.closest(SKIP) || !/[\p{L}\p{N}]/u.test(text)) continue
    const chain = [BODY, ...chainOf(el)]
    const key = JSON.stringify(chain)
    if (seen.has(key)) continue
    seen.add(key)
    out.push({ own: `.${[...el.classList].join('.')}`, text, chain })
  }
  const kinds = new Set(out.flatMap((w) => w.chain.flatMap((e) => e.classes.map((c) => `.${c}`))))
  return { words: out, kinds }
}

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
const ALL: Mode[] = [...new Set<Mode>([...MODES, ...SKINS.flatMap((s) => skinBaseModes(s.id).map((b): Mode => `${b} skin=${s.id}`))])]
/** Save to Memories, Tune and ＋ Add wear the POTA board's HUNT look, accent lettering on an accent tint, and are held here
 *  like every other word. Held to fewer themes until 2026-10-03, they read under 4.5:1 in dark under both non-default accent
 *  presets and two themes' own accents. The board's own suite (PotaSotaView.contrast.test.tsx) holds the look in every host
 *  and under every preset on the worst-case themes too. */

const memo = new Map<string, unknown>()
function once<T>(key: string, make: () => T): T {
  if (!memo.has(key)) memo.set(key, make())
  return memo.get(key) as T
}
const keyOf = (mode: Mode, at: El[]) => `${mode}|${JSON.stringify(at)}`
/** The compound a rule puts on the element itself, split off its selector the way reachesChain splits it. */
const SUBJECT = new WeakMap<Rule, string | undefined>()
function subjectOf(rule: Rule): string | undefined {
  if (!SUBJECT.has(rule)) SUBJECT.set(rule, rule.selector.replace(/\s*>\s*/g, ' > ').split(/\s+/).filter(Boolean).pop())
  return SUBJECT.get(rule)
}
/** The rules that can win `props` on `el`: those that declare one and whose subject matches `el` (reachesChain gives up on any
 *  other before it reads an ancestor or the theme), cut once per element instead of once per theme, as NativeControls.contrast
 *  does; winnerAt over the cut names the same winner as over every rule. The word-by-word walk over every rule took 107-123 s
 *  of its 120 s budget in the full suite (2026-10-03). */
const cutFor = (el: El, props: string[]) => once(`c|${JSON.stringify(el)}|${props.join()}`, () =>
  RULES.filter((r) => {
    const subject = subjectOf(r)
    return r.decls.some((d) => props.includes(d.prop)) && !!subject && compoundMatches(subject, el)
  }),
)
/** Only a rule that declares a custom property can set one: tokensAt over those alone gives the same tokens. */
const TOKEN_RULES = RULES.filter((r) => r.decls.some((d) => d.prop.startsWith('--')))
const tokens = (mode: Mode, at: El[]) => once(`t|${keyOf(mode, at)}`, () => tokensAt(TOKEN_RULES, mode, at))
const win = (mode: Mode, at: El[], ...props: string[]) =>
  once(`w|${keyOf(mode, at)}|${props.join()}`, () => winnerAt(cutFor(at[at.length - 1], props), mode, at, ...props))
const BLANK = /^(inherit|transparent|none|initial|unset)$/i
const colourOf = (mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(tokens(mode, at), value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}
/** What an element sits on: every background from it outward, down to the first opaque one, composited. */
function surfaceOf(mode: Mode, chain: El[]): Rgb {
  const layers: Array<{ value: string; at: El[] }> = []
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = win(mode, at, 'background', 'background-color')
    if (!w || BLANK.test(w.value.trim())) continue
    layers.push({ value: w.value, at })
    if (hex(colourOf(mode, at, w.value, [0, 0, 0])) === hex(colourOf(mode, at, w.value, [255, 255, 255]))) break
  }
  return layers.reduceRight((under, l) => colourOf(mode, l.at, l.value, under), colourOf(mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0]))
}
/** The declaration that paints the word: its own, else the nearest ancestor's (it inherits). */
function inkOf(mode: Mode, chain: El[]) {
  for (let i = chain.length; i > 0; i--) {
    const w = win(mode, chain.slice(0, i), 'color')
    if (w && !BLANK.test(w.value.trim())) return { value: w.value, at: chain.slice(0, i) }
  }
  return { value: 'var(--text)', at: chain.slice(0, 1) }
}
function ratioOf(mode: Mode, w: Word) {
  const bg = surfaceOf(mode, w.chain)
  const ink = inkOf(mode, w.chain)
  const fg = colourOf(mode, ink.at, ink.value, bg)
  return { fg, bg, ratio: contrast(fg, bg) }
}

describe('the Repeaters look: every word reads 4.5:1 in every theme', () => {
  let words: Word[] = []
  let kinds = new Set<string>()
  // jsdom lays nothing out: the map's box is given a size (so its dots are placed and the selected one carries its
  // card) and its canvas a context that draws nothing, as the map's own view test does.
  const box = { w: Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth'), h: Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight') }
  beforeAll(async () => {
    ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
    ;(Element.prototype as unknown as { scrollIntoView: () => void }).scrollIntoView = () => {}
    Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => 800 })
    Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => 600 })
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
    ;({ words, kinds } = await renderWords())
    cleanup()
  }, 30_000)
  afterAll(() => {
    delete (globalThis as unknown as { ResizeObserver?: unknown }).ResizeObserver
    if (box.w) Object.defineProperty(Element.prototype, 'clientWidth', box.w)
    if (box.h) Object.defineProperty(Element.prototype, 'clientHeight', box.h)
    vi.restoreAllMocks()
  })

  it('finds the words of every part the look styles (the census cannot silently empty out)', () => {
    for (const k of ['.rp-card-title', '.rp-count', '.rp-stamp-list', '.rp-save', '.rp-saved-badge', '.rp-add', '.rp-map-note',
      '.rp-map-card', '.rp-row.linked', '.rp-row.selected', '.rp-chan-num', '.rp-deliver', '.rp-lbl', '.filter-chip']) {
      const hit = words.some((w) => w.own.includes(k) || w.chain.some((e) => k.split('.').filter(Boolean).every((c) => e.classes.includes(c))))
      expect(hit, `no word in ${k}: ${[...kinds].filter((c) => c.startsWith('.rp-')).join(' ')}`).toBe(true)
    }
    expect(words.length).toBeGreaterThan(40)
  })

  it('every word is 4.5:1 or better on what it sits on, in every theme', () => {
    const low: string[] = []
    for (const w of words) {
      for (const mode of ALL) {
        const { fg, bg, ratio } = ratioOf(mode, w)
        if (ratio < 4.5) low.push(`${mode}: ${w.own} "${w.text.slice(0, 30)}" ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
      }
    }
    expect(low.slice(0, 16), `${low.length} words under 4.5:1`).toEqual([])
  }, 120_000)

  it('a card is a card in every theme: its face differs from the ground it sits on, and it has an edge', () => {
    const card = words.find((w) => w.chain.some((e) => e.classes.includes('rp-card')))!
    const at = card.chain.slice(0, card.chain.findIndex((e) => e.classes.includes('rp-card')) + 1)
    const flat: string[] = []
    for (const mode of ALL) {
      const face = surfaceOf(mode, at)
      const ground = surfaceOf(mode, at.slice(0, -1))
      const edge = win(mode, at, 'border', 'border-color')
      if (hex(face) === hex(ground) && !edge) flat.push(`${mode}: face ${hex(face)} on ground ${hex(ground)}, no border`)
      if (!edge) flat.push(`${mode}: no border`)
    }
    expect(flat).toEqual([])
  })
})
