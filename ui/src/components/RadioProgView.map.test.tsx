// @vitest-environment jsdom
// Repeaters' map (the operator's picks: "hearham-only map now", 2026-09-30; "Map first, list below", 2026-10-02):
// after a fetch the shown machines are on a map above the list. It plots only the machines with a hearham row, each
// by that row's own position, callsign and town (`row.map`), never a RepeaterBook row (their terms forbid a map) and,
// for now, not a machine only the RSGB list has; the words under it say what it shows and what it leaves off. A dot
// and its row are linked: pointing at one lights the other, and a click on a dot selects it, brings its row into view
// and opens a card with the row's Save to Memories and ＋ Add.
//
// Every case is a PAIR: what the map does is shown beside what the list does, or beside the case
// that must not trigger it, so a view that always or never does it cannot pass.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { RepeaterMapPoint, RepeaterRecord, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'
import { gridToLatLon } from '../grid'
import { setUnitsMirror } from '../units'
import { emptyBank, memoriesStore } from '../features/memories'

const repeaterSearch = vi.fn()
const listProjects = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  radioprogListProjects: (...a: unknown[]) => listProjects(...a),
}))

import { RadioProgView } from './RadioProgView'
import { SITE_SPREAD_PX } from './RepeaterMap'
import { markerScaleFor } from './MapView'

// THE BUDGET (2026-10-09). The slowest case here, "a dot and its row are linked: pointing at one lights…", takes
// 1.12 s and 0.52 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const NOW = Math.floor(Date.now() / 1000)
const HOME = gridToLatLon('EN52')!

type Src = 'hearham' | 'repeaterbook' | 'rsgb'

/** One FM machine. `srcs` are the directories behind it, the top one first; `map` is its hearham
 *  point when one of them is hearham. */
function machine(call: string, mhz: number, srcs: Src[], city: string, map?: RepeaterMapPoint): RepeaterSearchRow {
  const top = srcs[0]
  const record: RepeaterRecord = {
    source: top, sourceId: `${top}-${call}`, callsign: call, outputMhz: mhz, inputMhz: mhz + 0.6,
    ctcssEncHz: 103.5, ctcssDecHz: null, dcs: null, lat: 42.3, lon: -89.0, city, county: '', state: 'IL',
    fm: true, dmr: false, dstar: false, fusion: false, dmrColorCode: null, bandwidthKhz: null,
    operational: true, openUse: true, updated: null, distanceKm: 10, bearingDeg: 90,
  }
  return {
    record,
    channel: {
      id: `${top}:${call}`, name: call, rxMhz: mhz, duplex: 'plus', offsetMhz: 0.6, toneMode: 'tone',
      rtoneHz: 103.5, ctoneHz: 103.5, dtcsCode: 23, mode: 'fm', comment: city,
      source: { source: top, sourceId: `${top}-${call}`, callsign: call },
    },
    sources: srcs.map((s) => ({ source: s, sourceId: `${s}-${call}`, channelId: `${s}:${call}`, updated: null })),
    disagreements: [],
    ...(map ? { map } : {}),
  }
}

/** hearham's alone, placed exactly at the station's grid, the map's centre (a click there hits it). */
const HH = machine('W9AAA', 146.94, ['hearham'], 'Rockford', {
  lat: HOME.lat, lon: HOME.lon, callsign: 'W9AAA', outputMhz: 146.94, city: 'Rockford',
})
/** RepeaterBook's and hearham's: the list shows RepeaterBook's name and town, the map hearham's. */
const BOTH = machine('W9BBB', 147.18, ['repeaterbook', 'hearham'], 'Rockford, Hilltop', {
  lat: HOME.lat + 0.2, lon: HOME.lon + 0.2, callsign: 'W9BBB-R', outputMhz: 147.1815, city: 'Belvidere',
})
/** RepeaterBook's alone: never on the map. */
const RB_ONLY = machine('W9CCC', 147.27, ['repeaterbook'], 'Freeport')

const result = (rows: RepeaterSearchRow[], lists: Src[]): RepeaterSearchResult => ({
  lists: lists.map((source) => ({ source, fetchedUtc: NOW - 3600, stale: false })),
  coverageGap: null,
  missingStates: [],
  rsgbUnavailable: false,
  rsgbBeyond: [],
  rows,
})

const fetchButton = () => screen.getByRole('button', { name: t('program.fetch.label') })
const card = () => document.querySelector<HTMLElement>('.rp-map-card')
const cardButton = (name: string) => [...(card()?.querySelectorAll('button') ?? [])].find((b) => b.textContent === name)
const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row')].find((r) => r.querySelector('.rp-call')?.textContent === call)!
const listCalls = () =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row .rp-call')].map((c) => c.textContent)
/** What the map names, for a screen reader: one line per machine it plots. */
const plotted = () => [...document.querySelectorAll<HTMLElement>('.rp-map .sr-only li')].map((l) => l.textContent)
const note = () => document.querySelector('.rp-map-note')?.textContent ?? ''

async function fetched(res: RepeaterSearchResult) {
  repeaterSearch.mockResolvedValue(res)
  render(<RadioProgView myGrid="EN52" />)
  fireEvent.click(fetchButton())
  await waitFor(() => expect(document.querySelector('.rp-count')).toBeTruthy())
}

// jsdom lays nothing out and has no 2-D canvas: the map's box is given a size and its context a
// stand-in that answers every call (MapView.ota-doubleclick.test.tsx explains the pattern), so the
// real projection places the machines and a click is hit-tested exactly as in a browser.
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
function fakeCtx(): CanvasRenderingContext2D {
  const self: object = new Proxy(function fakeCtxTarget() {}, {
    get(_t, prop) {
      if (prop === 'measureText') return () => ({ width: 10 })
      return self
    },
    set() {
      return true
    },
    apply() {
      return self
    },
  })
  return self as CanvasRenderingContext2D
}
const W = 800
const H = 600
const scrolled = vi.fn(function (this: Element, _o?: unknown) {
  void _o
  return this
})
let widthDesc: PropertyDescriptor | undefined
let heightDesc: PropertyDescriptor | undefined

describe("Program's map: hearham's listings only", () => {
  beforeEach(() => {
    cleanup()
    localStorage.clear()
    setUnitsMirror('imperial')
    repeaterSearch.mockReset()
    listProjects.mockReset()
    listProjects.mockResolvedValue([])
    ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
    memoriesStore.set(emptyBank())
    ;(Element.prototype as unknown as { scrollIntoView: unknown }).scrollIntoView = scrolled
    scrolled.mockClear()
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(fakeCtx())
    widthDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
    heightDesc = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
    Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => W })
    Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => H })
  })
  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
    if (widthDesc) Object.defineProperty(Element.prototype, 'clientWidth', widthDesc)
    if (heightDesc) Object.defineProperty(Element.prototype, 'clientHeight', heightDesc)
  })

  it('plots the machines hearham lists, by hearham’s own call and town, above the list of every machine', async () => {
    // CONTROL: before a fetch there is no map, only the list's prompt.
    repeaterSearch.mockResolvedValue(result([HH, BOTH, RB_ONLY], ['repeaterbook', 'hearham']))
    render(<RadioProgView myGrid="EN52" />)
    expect(document.querySelector('.rp-map')).toBeNull()
    expect(document.querySelector('.rp-results .aw-empty')).toBeTruthy()
    fireEvent.click(fetchButton())
    await waitFor(() => expect(document.querySelector('.rp-count')).toBeTruthy())

    // The list shows all three, RepeaterBook's name for the shared one, under the map.
    expect(listCalls()).toEqual(['W9AAA', 'W9BBB', 'W9CCC'])
    const map = document.querySelector('.rp-map')!
    const list = document.querySelector('.rp-results')!
    expect(map).toBeTruthy()
    expect(map.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })).toBeTruthy()
    // hearham's own rows: its callsign, output and town for the shared machine, never
    // RepeaterBook's; the machine only RepeaterBook lists is not there at all.
    expect(plotted()).toEqual(['W9AAA 146.94 · Rockford', 'W9BBB-R 147.1815 · Belvidere'])
    expect(map.textContent).not.toMatch(/W9CCC|Hilltop|Freeport/)
    // There is nothing to switch: no List or Map chips.
    expect(document.querySelector('.rp-view')).toBeNull()
  })

  it('says under the map what it shows and what it leaves off', async () => {
    await fetched(result([HH, BOTH, RB_ONLY], ['repeaterbook', 'hearham']))
    expect(note()).toContain(t('program.map.shows', { count: 2, hearham: 'hearham' }))
    expect(note()).toContain(t('program.map.rb', { count: 1, rb: 'RepeaterBook' }))
    expect(note()).toContain(t('program.map.hint'))
    expect(note()).not.toContain(t('program.map.rsgb.none', { rsgb: 'RSGB', hearham: 'hearham' }))
    cleanup()

    // RepeaterBook answered, but every machine shown is hearham's too: its listings are still
    // never mapped, and the map says so instead of a count.
    await fetched(result([HH, BOTH], ['repeaterbook', 'hearham']))
    expect(note()).toContain(t('program.map.rb.none', { rb: 'RepeaterBook', hearham: 'hearham' }))
    expect(note()).not.toContain(t('program.map.rb', { count: 1, rb: 'RepeaterBook' }))
    cleanup()

    // CONTROL: hearham's list alone has nothing left off and names no other directory.
    await fetched(result([HH], ['hearham']))
    expect(note()).toContain(t('program.map.shows', { count: 1, hearham: 'hearham' }))
    expect(note()).not.toMatch(/RepeaterBook|RSGB/)
    cleanup()

    // A UK list: a machine only the RSGB list has is left off for now, and counted.
    const uk = machine('GB3AA', 145.6, ['rsgb', 'hearham'], 'Bolton', {
      lat: HOME.lat, lon: HOME.lon, callsign: 'GB3AA', outputMhz: 145.6, city: 'Bolton',
    })
    const rsgbOnly = machine('GB3BB', 433.1, ['rsgb'], 'Wigan')
    await fetched(result([uk, rsgbOnly], ['rsgb', 'hearham']))
    expect(plotted()).toEqual(['GB3AA 145.6 · Bolton'])
    expect(note()).toContain(t('program.map.rsgb', { count: 1, rsgb: 'RSGB' }))
    expect(note()).not.toMatch(/RepeaterBook/)
  })

  it("a dot's card adds the machine to the channel list, and takes it off again", async () => {
    await fetched(result([HH, BOTH], ['hearham']))
    const canvas = screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })
    const chanRows = () => document.querySelectorAll('.rp-chan-row').length
    // CONTROL: a click on empty map, a corner far from every machine, opens nothing and adds nothing.
    fireEvent.click(canvas, { clientX: 5, clientY: 5 })
    expect(card()).toBeNull()
    expect(chanRows()).toBe(0)
    // W9AAA sits at the station's grid, the map's centre: its card, in hearham's words.
    fireEvent.click(canvas, { clientX: W / 2, clientY: H / 2 })
    await waitFor(() => expect(card()).toBeTruthy())
    expect(card()!.textContent).toContain('W9AAA 146.94 · Rockford')
    // The click selects; it adds nothing by itself.
    expect(chanRows()).toBe(0)
    fireEvent.click(cardButton(t('program.row.add.label'))!)
    await waitFor(() => expect(chanRows()).toBe(1))
    fireEvent.click(cardButton(t('program.row.added.label'))!)
    await waitFor(() => expect(chanRows()).toBe(0))
    // ✕ closes it, and so does a click on empty map.
    fireEvent.click(card()!.querySelector('.rp-map-card-close')!)
    expect(card()).toBeNull()
  })

  it('spreads the machines on one site, so each is a click of its own', async () => {
    // A second machine on W9AAA's tower, at the station's grid: the map's centre.
    const twin = machine('W9DDD', 443.5, ['hearham'], 'Rockford', {
      lat: HOME.lat, lon: HOME.lon, callsign: 'W9DDD', outputMhz: 443.5, city: 'Rockford',
    })
    await fetched(result([HH, twin], ['hearham']))
    const canvas = screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })
    const chanRows = () => document.querySelectorAll('.rp-chan-row').length
    const d = SITE_SPREAD_PX * markerScaleFor(W, H)
    // The first stands above the site and the second below it; stacked, both clicks would reach
    // the first, and its card would open twice.
    fireEvent.click(canvas, { clientX: W / 2, clientY: H / 2 - d })
    await waitFor(() => expect(card()?.textContent).toContain('W9AAA'))
    fireEvent.click(cardButton(t('program.row.add.label'))!)
    await waitFor(() => expect(chanRows()).toBe(1))
    fireEvent.click(canvas, { clientX: W / 2, clientY: H / 2 + d })
    await waitFor(() => expect(card()?.textContent).toContain('W9DDD'))
    fireEvent.click(cardButton(t('program.row.add.label'))!)
    await waitFor(() => expect(chanRows()).toBe(2))
  })

  it('gives way to the list’s own words when nothing is shown', async () => {
    await fetched(result([HH, BOTH], ['hearham']))
    expect(document.querySelector('.rp-map')).toBeTruthy()
    // A text filter that matches nothing: the empty words (and their way out) stand, not a map.
    fireEvent.change(screen.getByRole('searchbox', { name: t('program.filters.search.aria') }), {
      target: { value: 'ZZZ' },
    })
    expect(document.querySelector('.rp-map')).toBeNull()
    expect(document.querySelector('.rp-results .aw-empty')).toBeTruthy()
  })

  it('a dot and its row are linked: pointing at one lights the other, and a click on a dot brings its row into view', async () => {
    await fetched(result([HH, BOTH], ['hearham']))
    const canvas = screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })
    const stage = () => document.querySelector<HTMLElement>('.rp-map-stage')!
    // CONTROL: nothing pointed at, nothing lit.
    expect(stage().dataset.linked).toBeUndefined()
    expect(document.querySelectorAll('.rp-row.linked, .rp-row.selected')).toHaveLength(0)
    // The pointer on W9AAA's dot (the map's centre) lights W9AAA's row, and only it.
    fireEvent.pointerMove(canvas, { clientX: W / 2, clientY: H / 2 })
    await waitFor(() => expect(rowOf('W9AAA').classList.contains('linked')).toBe(true))
    expect(rowOf('W9BBB').classList.contains('linked')).toBe(false)
    // Off the dot, the row goes back.
    fireEvent.pointerMove(canvas, { clientX: 5, clientY: 5 })
    await waitFor(() => expect(rowOf('W9AAA').classList.contains('linked')).toBe(false))
    // The pointer on W9BBB's row rings W9BBB's dot.
    fireEvent.pointerEnter(rowOf('W9BBB'))
    expect(stage().dataset.linked).toBe(BOTH.channel.id)
    fireEvent.pointerLeave(rowOf('W9BBB'))
    expect(stage().dataset.linked).toBeUndefined()
    // A click on W9AAA's dot selects its row and scrolls it into view (nearest, never jumping the page).
    expect(scrolled).not.toHaveBeenCalled()
    fireEvent.click(canvas, { clientX: W / 2, clientY: H / 2 })
    await waitFor(() => expect(rowOf('W9AAA').classList.contains('selected')).toBe(true))
    expect(scrolled).toHaveBeenCalledTimes(1)
    expect(scrolled.mock.contexts[0]).toBe(rowOf('W9AAA'))
    expect(scrolled.mock.calls[0][0]).toEqual({ block: 'nearest' })
    // A click on the other ROW selects it instead: its dot carries the card now, in hearham's words.
    fireEvent.click(rowOf('W9BBB').querySelector('.rp-call')!)
    await waitFor(() => expect(rowOf('W9BBB').classList.contains('selected')).toBe(true))
    expect(rowOf('W9AAA').classList.contains('selected')).toBe(false)
    expect(card()?.textContent).toContain('W9BBB-R 147.1815 · Belvidere')
    // CONTROL: a click on a row's own button acts on the machine and selects nothing.
    fireEvent.click(rowOf('W9AAA').querySelector('.rp-add')!)
    expect(rowOf('W9AAA').classList.contains('selected')).toBe(false)
  })

  it('under the UI zoom, the dot under the pointer is the one it names, lights and selects', async () => {
    // THE DEFECT (Chrome, 2026-10-03): `.app { zoom: var(--ui-zoom) }` makes the pointer's clientX and the canvas's rect
    // VISUAL px while the dots are placed in LAYOUT px (the box's clientWidth), and the hit test compared the two
    // unscaled. At 85 %, the default on 1366×768 and 1024×768, a pointer on the station's own dot lit another machine's
    // row, and a click there opened that machine's card, Save to Memories and Add with it. MapView's canvasXY undoes the
    // zoom by the rect's ratio; this is the same. The cases above are the unzoomed control (jsdom's rect is 0 × 0).
    await fetched(result([HH, BOTH], ['hearham']))
    const canvas = screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })
    const Z = 0.85
    vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue({
      x: 10, y: 20, left: 10, top: 20, width: W * Z, height: H * Z, right: 10 + W * Z, bottom: 20 + H * Z, toJSON: () => ({}),
    } as DOMRect)
    // W9AAA's dot is the map's centre: on screen, the rect's corner plus half its zoomed size.
    const at = { clientX: 10 + (W / 2) * Z, clientY: 20 + (H / 2) * Z }
    fireEvent.pointerMove(canvas, at)
    await waitFor(() => expect(rowOf('W9AAA').classList.contains('linked')).toBe(true))
    expect(rowOf('W9BBB').classList.contains('linked')).toBe(false)
    fireEvent.click(canvas, at)
    await waitFor(() => expect(rowOf('W9AAA').classList.contains('selected')).toBe(true))
    expect(card()?.textContent).toContain('W9AAA')
  })

  it("a dot's card saves the machine to Memories, then shows the row's badge", async () => {
    await fetched(result([HH, BOTH], ['hearham']))
    const canvas = screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })
    fireEvent.click(canvas, { clientX: W / 2, clientY: H / 2 })
    await waitFor(() => expect(card()).toBeTruthy())
    expect(card()!.querySelector('.rp-saved-badge')).toBeNull()
    fireEvent.click(cardButton(t('program.row.save.label'))!)
    expect(memoriesStore.get().memories.map((m) => m.callsign)).toEqual(['W9AAA'])
    await waitFor(() => expect(card()!.querySelector('.rp-saved-badge')?.textContent).toBe(t('program.row.saved.label')))
    expect(cardButton(t('program.row.save.label'))).toBeUndefined()
    // The row says so too: one machine, one state.
    expect(rowOf('W9AAA').querySelector('.rp-saved-badge')).toBeTruthy()
    expect(rowOf('W9BBB').querySelector('.rp-saved-badge')).toBeNull()
  })
})
