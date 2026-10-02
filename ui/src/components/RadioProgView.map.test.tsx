// @vitest-environment jsdom
// Program's map (the operator's pick, 2026-09-30: "hearham-only map now"): a Map chip beside the
// count puts the shown machines on a map in the list's place. It plots only the machines with a
// hearham row, each by that row's own position, callsign and town (`row.map`), never a
// RepeaterBook row (their terms forbid a map) and, for now, not a machine only the RSGB list has;
// the words under it say what it shows and what it leaves off. A click on a machine adds it to the
// channel list or takes it off, as the row's ＋ does.
//
// Every case is a PAIR: what the map does is shown beside what the list does, or beside the case
// that must not trigger it, so a view that always or never does it cannot pass.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { RepeaterMapPoint, RepeaterRecord, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'
import { gridToLatLon } from '../grid'
import { setUnitsMirror } from '../units'

const repeaterSearch = vi.fn()
const listProjects = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  radioprogListProjects: (...a: unknown[]) => listProjects(...a),
}))

import { RadioProgView } from './RadioProgView'

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
const mapChip = () => screen.getByRole('button', { name: t('program.view.map') })
const listChip = () => screen.getByRole('button', { name: t('program.view.list') })
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

  it('plots the machines hearham lists, by hearham’s own call and town, in the list’s place', async () => {
    await fetched(result([HH, BOTH, RB_ONLY], ['repeaterbook', 'hearham']))
    // CONTROL: the list shows all three, RepeaterBook's name for the shared one.
    expect(listCalls()).toEqual(['W9AAA', 'W9BBB', 'W9CCC'])
    expect(document.querySelector('.rp-map')).toBeNull()

    fireEvent.click(mapChip())
    expect(mapChip().getAttribute('aria-pressed')).toBe('true')
    expect(document.querySelector('.rp-results')).toBeNull()
    expect(screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })).toBeTruthy()
    // hearham's own rows: its callsign, output and town for the shared machine, never
    // RepeaterBook's; the machine only RepeaterBook lists is not there at all.
    expect(plotted()).toEqual(['W9AAA 146.94 · Rockford', 'W9BBB-R 147.1815 · Belvidere'])
    expect(document.querySelector('.rp-map')?.textContent).not.toMatch(/W9CCC|Hilltop|Freeport/)

    fireEvent.click(listChip())
    expect(document.querySelector('.rp-map')).toBeNull()
    expect(listCalls()).toEqual(['W9AAA', 'W9BBB', 'W9CCC'])
  })

  it('says under the map what it shows and what it leaves off', async () => {
    await fetched(result([HH, BOTH, RB_ONLY], ['repeaterbook', 'hearham']))
    fireEvent.click(mapChip())
    expect(note()).toContain(t('program.map.shows', { count: 2, hearham: 'hearham' }))
    expect(note()).toContain(t('program.map.rb', { count: 1, rb: 'RepeaterBook' }))
    expect(note()).toContain(t('program.map.hint'))
    expect(note()).not.toContain(t('program.map.rsgb.none', { rsgb: 'RSGB', hearham: 'hearham' }))
    cleanup()

    // RepeaterBook answered, but every machine shown is hearham's too: its listings are still
    // never mapped, and the map says so instead of a count.
    await fetched(result([HH, BOTH], ['repeaterbook', 'hearham']))
    fireEvent.click(mapChip())
    expect(note()).toContain(t('program.map.rb.none', { rb: 'RepeaterBook', hearham: 'hearham' }))
    expect(note()).not.toContain(t('program.map.rb', { count: 1, rb: 'RepeaterBook' }))
    cleanup()

    // CONTROL: hearham's list alone has nothing left off and names no other directory.
    await fetched(result([HH], ['hearham']))
    fireEvent.click(mapChip())
    expect(note()).toContain(t('program.map.shows', { count: 1, hearham: 'hearham' }))
    expect(note()).not.toMatch(/RepeaterBook|RSGB/)
    cleanup()

    // A UK list: a machine only the RSGB list has is left off for now, and counted.
    const uk = machine('GB3AA', 145.6, ['rsgb', 'hearham'], 'Bolton', {
      lat: HOME.lat, lon: HOME.lon, callsign: 'GB3AA', outputMhz: 145.6, city: 'Bolton',
    })
    const rsgbOnly = machine('GB3BB', 433.1, ['rsgb'], 'Wigan')
    await fetched(result([uk, rsgbOnly], ['rsgb', 'hearham']))
    fireEvent.click(mapChip())
    expect(plotted()).toEqual(['GB3AA 145.6 · Bolton'])
    expect(note()).toContain(t('program.map.rsgb', { count: 1, rsgb: 'RSGB' }))
    expect(note()).not.toMatch(/RepeaterBook/)
  })

  it('adds a machine to the channel list from the map, and takes it off again', async () => {
    await fetched(result([HH, BOTH], ['hearham']))
    fireEvent.click(mapChip())
    const canvas = screen.getByRole('img', { name: t('program.map.aria', { count: 2 }) })
    const chanRows = () => document.querySelectorAll('.rp-chan-row').length
    // CONTROL: a click on empty map, a corner far from every machine, adds nothing.
    fireEvent.click(canvas, { clientX: 5, clientY: 5 })
    expect(chanRows()).toBe(0)
    // W9AAA sits at the station's grid, the map's centre.
    fireEvent.click(canvas, { clientX: W / 2, clientY: H / 2 })
    await waitFor(() => expect(chanRows()).toBe(1))
    fireEvent.click(canvas, { clientX: W / 2, clientY: H / 2 })
    await waitFor(() => expect(chanRows()).toBe(0))
  })

  it('gives way to the list’s own words when nothing is shown', async () => {
    await fetched(result([HH, BOTH], ['hearham']))
    fireEvent.click(mapChip())
    expect(document.querySelector('.rp-map')).toBeTruthy()
    // A text filter that matches nothing: the empty words (and their way out) stand, not a map.
    fireEvent.change(screen.getByRole('searchbox', { name: t('program.filters.search.aria') }), {
      target: { value: 'ZZZ' },
    })
    expect(document.querySelector('.rp-map')).toBeNull()
    expect(document.querySelector('.rp-results .aw-empty')).toBeTruthy()
  })
})
