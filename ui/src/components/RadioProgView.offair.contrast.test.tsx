// @vitest-environment jsdom
//
// PROGRAM'S OFF-AIR ROW FADES BY COLOUR, NOT BY SEE-THROUGH (2026-10-01).
//
// A machine the source lists as not operational shows when "On-air only" is off, and it was dimmed whole by
// `opacity: 0.55`. That put every word on the row under 4.5:1 (Chrome, the light theme: the call and frequency 3.73,
// the offset, tone and distance 2.56, OFF-AIR 3.33, Tune and Add 2.07; at night every word under 3.6). The row now
// fades the way closed bands do (`.ba-row.is-closed`): its words take the dim ink, Tune and Add with them, and the OFF-AIR badge
// letters in the ink with its warning colour on its border, so the row still reads as off the air at a glance.
//
// Computed with the app's own resolver (cssCascade.ts) over the rendered view: a search with an on-air machine and
// two off-air ones (one of them added, so it shows ADDED), "On-air only" switched off.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { RepeaterSearchResult } from '../types'
import { SKINS } from '../features/skins'
import {
  MODES,
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
} from '../cssCascade'

const api = vi.hoisted(() => {
  const machine = (callsign: string, outputMhz: number, operational: boolean) => ({
    record: {
      source: 'repeaterbook', sourceId: callsign, callsign, outputMhz, inputMhz: outputMhz - 0.6, ctcssEncHz: null, ctcssDecHz: null, dcs: null,
      lat: 39.9, lon: -76.6, city: 'Red Lion', county: 'York', state: 'Pennsylvania', fm: true, dmr: false, dstar: false, fusion: false,
      dmrColorCode: null, bandwidthKhz: null, operational, openUse: true, distanceKm: 5, bearingDeg: 90,
    },
    channel: {
      id: callsign.toLowerCase(), name: callsign, rxMhz: outputMhz, duplex: 'minus', offsetMhz: 0.6, toneMode: 'tone', rtoneHz: 100, ctoneHz: 100,
      dtcsCode: 23, mode: 'fm', comment: 'Red Lion', source: { source: 'repeaterbook', sourceId: callsign, callsign },
    },
  })
  const result = { source: 'repeaterbook', fetchedUtc: 1_700_000_000, stale: false, coverageGap: null, missingStates: [],
    lists: [{ source: 'repeaterbook', fetchedUtc: 1_700_000_000, stale: false }], rsgbUnavailable: false, rsgbBeyond: [],
    rows: [machine('W3ZGD', 146.865, true), machine('N3OFF', 146.94, false), machine('K3DWN', 147.09, false)] } as unknown as RepeaterSearchResult
  return { repeaterSearch: vi.fn(async () => result), radioprogListProjects: vi.fn(async () => []) }
})
vi.mock('../api', async (importOriginal) => ({ ...(await importOriginal<typeof import('../api')>()), ...api }))

import { RadioProgView } from './RadioProgView'

beforeAll(() => {
  ;(Element.prototype as unknown as { scrollIntoView: () => void }).scrollIntoView = () => {}
})
afterEach(cleanup)

interface Word {
  row: 'offair' | 'onair'
  own: string
  text: string
  chain: El[]
}
const BODY: El = { tag: 'body', classes: [], attrs: {} }

/** Program with the search fetched, "On-air only" off and the second off-air machine added; every word on its rows. */
async function renderWords(): Promise<Word[]> {
  const { container } = render(<div className="app"><main className="layout single"><RadioProgView myGrid="FN31" catOk /></main></div>)
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: /fetch/i }))
  })
  await waitFor(() => expect(container.querySelectorAll('.rp-row .rp-add').length).toBe(1))
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: /on-air only/i }))
  })
  await waitFor(() => expect(container.querySelectorAll('.rp-row.offair').length).toBe(2))
  await act(async () => {
    fireEvent.click(container.querySelectorAll<HTMLButtonElement>('.rp-row.offair .rp-add')[1])
  })
  await waitFor(() => expect(container.querySelector('.rp-row.offair .rp-add.added')).toBeTruthy())
  const out: Word[] = []
  const seen = new Set<Element>()
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT)
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n.parentElement
    const row = el?.closest('.rp-results .rp-row')
    if (!el || !row || seen.has(el) || !/[\p{L}\p{N}]/u.test(n.textContent ?? '')) continue
    seen.add(el)
    out.push({ row: row.classList.contains('offair') ? 'offair' : 'onair', own: `.${[...el.classList].join('.')}`, text: (n.textContent ?? '').trim(), chain: [BODY, ...chainOf(el)] })
  }
  return out
}

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

/** Every mode the sweep covers (base modes, colour-role sets, the worst-case themes), and every theme in its own modes. */
const ALL: Mode[] = [...new Set<Mode>([...MODES, ...SKINS.flatMap((s) => skinBaseModes(s.id).map((b): Mode => `${b} skin=${s.id}`))])]

const memo = new Map<string, unknown>()
function once<T>(key: string, make: () => T): T {
  if (!memo.has(key)) memo.set(key, make())
  return memo.get(key) as T
}
const keyOf = (mode: Mode, at: El[]) => `${mode}|${JSON.stringify(at)}`
const tokens = (mode: Mode, at: El[]) => once(`t|${keyOf(mode, at)}`, () => tokensAt(RULES, mode, at))
const win = (mode: Mode, at: El[], ...props: string[]) => once(`w|${keyOf(mode, at)}|${props.join()}`, () => winnerAt(RULES, mode, at, ...props))
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
  throw new Error('nothing paints it')
}
function wordOf(mode: Mode, w: Word) {
  const bg = surfaceOf(mode, w.chain)
  const ink = inkOf(mode, w.chain)
  const fg = colourOf(mode, ink.at, ink.value, bg)
  return { fg, bg, ratio: contrast(fg, bg) }
}

describe("Program's off-air row fades by colour and every word on it reads, in every theme", () => {
  let words: Word[] = []
  beforeAll(async () => {
    words = await renderWords()
    cleanup()
  }, 30_000)
  const offair = () => words.filter((w) => w.row === 'offair')

  it('finds every word an off-air row letters, on both off-air rows (the census cannot silently empty out)', () => {
    const kinds = new Set(offair().map((w) => w.own))
    for (const k of ['.rp-call.mono', '.rp-freq.mono', '.rp-off.mono', '.rp-tone.mono', '.rp-dist.mono', '.pota-badge.rp-offair-badge', '.pota-hunt-btn.rp-tune', '.pota-hunt-btn.rp-add', '.pota-hunt-btn.rp-add.added']) {
      expect(kinds.has(k), `no ${k} on an off-air row: ${[...kinds].join(' ')}`).toBe(true)
    }
    expect(offair().length).toBe(16)
  })

  it('nothing dims an off-air row by opacity: the row and everything on it composites at full strength', () => {
    const dimmed: string[] = []
    for (const w of offair()) {
      for (let i = 2; i <= w.chain.length; i++) {
        const o = win('dark', w.chain.slice(0, i), 'opacity')
        if (o && parseFloat(o.value) < 1) dimmed.push(`${w.own} "${w.text}": \`${o.rule.selector} { opacity: ${o.value} }\``)
      }
    }
    expect([...new Set(dimmed)], 'see-through puts every word on the row under its own contrast').toEqual([])
  })

  it('every word on an off-air row reads 4.5:1 in every theme', () => {
    const low: string[] = []
    for (const mode of ALL) {
      for (const w of offair()) {
        const { fg, bg, ratio } = wordOf(mode, w)
        if (ratio < 4.5) low.push(`${mode}: ${w.own} "${w.text}" ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
      }
    }
    expect(low.slice(0, 12), `${low.length} words under 4.5:1`).toEqual([])
  }, 60_000)

  it('it still reads as off the air: the call and frequency in the dim ink, the badge with its warning colour on its border', () => {
    const on = words.find((w) => w.row === 'onair' && w.own === '.rp-call.mono')!
    const off = offair().find((w) => w.own === '.rp-call.mono')!
    const badge = offair().find((w) => w.own === '.pota-badge.rp-offair-badge')!
    for (const mode of ALL) {
      const { fg, bg } = wordOf(mode, off)
      const dim = colourOf(mode, off.chain, 'var(--text-dim)', bg)
      expect(hex(fg), `${mode}: the off-air call is not in the dim ink`).toBe(hex(dim))
      // A theme whose dim ink IS its ink (none today) fades nothing by colour; the badge is then the whole signal.
      if (hex(dim) !== hex(colourOf(mode, off.chain, 'var(--text)', bg)))
        expect(hex(fg), `${mode}: the off-air call reads exactly like an on-air one`).not.toBe(hex(wordOf(mode, on).fg))
      const edge = win(mode, badge.chain, 'border', 'border-color')
      expect(edge?.value, `${mode}: OFF-AIR carries no warning colour on its border`).toMatch(/var\(--state-weak\)/)
      expect(edge?.value, `${mode}: OFF-AIR's border is drawn at less than full strength or not at all`).toMatch(/^1px solid var\(--state-weak\)$/)
    }
  }, 60_000)
})
