// @vitest-environment jsdom
// Program's route list (the operator's pick, 2026-09-30: a route list for a trip): "Route to…" at the
// end of the Near row, then from one place to another, given the way the origin is, the machines
// within a corridor (25 mi by default, in the radius chips' style) of the straight line between them,
// in the order the route passes them, added to the channel list in that order and exported as one
// list. On a long route the panel names the RepeaterBook states and RSGB squares the search did not
// ask about.
//
// Every case is a PAIR: the route behaviour appears on a route AND stays away from a radius search,
// so a panel that always shows it, or never does, cannot pass.
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { RepeaterRecord, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'
import { gridToLatLon } from '../grid'
import { miToKm } from '../features/radioprog'
import { setUnitsMirror } from '../units'

const repeaterSearch = vi.fn()
const listProjects = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  radioprogListProjects: (...a: unknown[]) => listProjects(...a),
}))

import { RadioProgView } from './RadioProgView'

// THE BUDGET (2026-10-09). The slowest case here, "asks for the machines within the corridor of the line…", takes
// 0.58 s and 0.49 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const NOW = Math.floor(Date.now() / 1000)
const KM_PER_MI = 1.609344

/** One hearham FM machine `offMi` miles east of the route (or of the origin), `alongMi` miles along
 * it when the search was a route. */
function machine(call: string, mhz: number, id: string, offMi: number, alongMi?: number): RepeaterSearchRow {
  const record: RepeaterRecord = {
    source: 'hearham', sourceId: id, callsign: call, outputMhz: mhz, inputMhz: mhz + 0.6, ctcssEncHz: 103.5,
    ctcssDecHz: null, dcs: null, lat: 42.3, lon: -89.0, city: 'Rockford', county: '', state: 'IL', fm: true,
    dmr: false, dstar: false, fusion: false, dmrColorCode: null, bandwidthKhz: null, operational: true,
    openUse: true, updated: null, distanceKm: offMi * KM_PER_MI, bearingDeg: 90,
  }
  return {
    record,
    channel: {
      id: `hh:${id}`, name: call, rxMhz: mhz, duplex: 'plus', offsetMhz: 0.6, toneMode: 'tone', rtoneHz: 103.5,
      ctoneHz: 103.5, dtcsCode: 23, mode: 'fm', comment: 'Rockford',
      source: { source: 'hearham', sourceId: id, callsign: call },
    },
    sources: [{ source: 'hearham', sourceId: id, channelId: `hh:${id}`, updated: null }],
    disagreements: [],
    ...(alongMi != null ? { alongKm: alongMi * KM_PER_MI } : {}),
  }
}

/** A route result, in the order the station sends it: along the route. Nearest-the-line order
 * would be W9BBB, W9CCC, W9AAA, so a view that re-sorted by distance would show it. */
const routeResult = (over: Partial<RepeaterSearchResult> = {}): RepeaterSearchResult => ({
  route: true,
  lists: [{ source: 'hearham', fetchedUtc: NOW - 86400, stale: false }],
  coverageGap: null,
  missingStates: [],
  rsgbUnavailable: false,
  rsgbBeyond: [],
  rbBeyond: [],
  rows: [machine('W9AAA', 146.94, '1', 20, 12), machine('W9BBB', 147.18, '2', 2, 80), machine('W9CCC', 147.27, '3', 10, 150)],
  ...over,
})
/** The same machines from a radius search: no route anywhere. */
const radiusResult = (over: Partial<RepeaterSearchResult> = {}): RepeaterSearchResult => ({
  lists: [{ source: 'hearham', fetchedUtc: NOW - 86400, stale: false }],
  coverageGap: null,
  missingStates: [],
  rsgbUnavailable: false,
  rsgbBeyond: [],
  rows: [machine('W9BBB', 147.18, '2', 2), machine('W9CCC', 147.27, '3', 10), machine('W9AAA', 146.94, '1', 20)],
  ...over,
})

const fetchButton = () => screen.getByRole('button', { name: t('program.fetch.label') })
const calls = () =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row .rp-call')].map((c) => c.textContent)
const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row')].find(
    (r) => r.querySelector('.rp-call')?.textContent === call,
  )!
const notes = () => [...document.querySelectorAll<HTMLElement>('.rp-note')].map((n) => n.textContent ?? '')
const plain = (s: string) => s.replace(/<\/?b>/g, '')

/** Program on a route from the station (EN52) to the grid EN53 (from "Route to…" when it is not one
 * yet). */
function routeToEN53() {
  const add = screen.queryByRole('button', { name: t('program.route.add') })
  if (add) fireEvent.click(add)
  const to = screen.getByRole('group', { name: t('program.route.to.aria') })
  // The route's end is one box that takes a grid or a city (2026-10-02): a locator is the place as typed.
  fireEvent.change(within(to).getByRole('textbox', { name: t('program.route.to.place.aria') }), {
    target: { value: 'EN53' },
  })
}

async function fetched(res: RepeaterSearchResult, route = true) {
  repeaterSearch.mockResolvedValue(res)
  render(<RadioProgView myGrid="EN52" />)
  if (route) routeToEN53()
  fireEvent.click(fetchButton())
  await waitFor(() => expect(document.querySelector('.rp-count, .rp-results .aw-empty')).toBeTruthy())
}

describe('Program, along a route', () => {
  beforeEach(() => {
    cleanup()
    localStorage.clear()
    setUnitsMirror('imperial')
    repeaterSearch.mockReset()
    listProjects.mockReset()
    listProjects.mockResolvedValue([])
  })

  it('asks for the machines within the corridor of the line from the origin to a second place', async () => {
    repeaterSearch.mockResolvedValue(radiusResult())
    render(<RadioProgView myGrid="EN52" />)
    const station = gridToLatLon('EN52')!
    // CONTROL: around a place, the search has no other end.
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(1))
    expect(repeaterSearch.mock.calls[0]).toEqual([station.lat, station.lon, miToKm(50)])
    expect(screen.queryByRole('group', { name: t('program.route.to.aria') })).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: t('program.route.add') }))
    expect(screen.queryByRole('button', { name: t('program.route.add') })).toBeNull()
    const origin = screen.getByRole('group', { name: t('program.origin.aria') })
    expect(origin.querySelector('.rp-lbl')?.textContent).toBe(t('program.route.from'))
    // The corridor replaces the radius: 10, 25 and 50 mi, 25 to start with.
    expect(screen.queryByRole('group', { name: t('program.radius.aria') })).toBeNull()
    const corridor = screen.getByRole('group', { name: t('program.corridor.aria') })
    const chips = within(corridor).getAllByRole('button')
    expect(chips.map((c) => c.textContent)).toEqual(['10 mi', '25 mi', '50 mi'])
    expect(chips.map((c) => c.classList.contains('active'))).toEqual([false, true, false])
    // Nothing to fetch until the other end is a place.
    expect(fetchButton()).toHaveProperty('disabled', true)

    routeToEN53()
    expect(fetchButton()).toHaveProperty('disabled', false)
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(2))
    expect(repeaterSearch.mock.calls[1]).toEqual([station.lat, station.lon, miToKm(25), gridToLatLon('EN53')])
    fireEvent.click(chips[2])
    await waitFor(() => expect(fetchButton()).toHaveProperty('disabled', false))
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(3))
    expect(repeaterSearch.mock.calls[2][2]).toBe(miToKm(50))
    // ✕ goes back to the search around a place.
    fireEvent.click(screen.getByRole('button', { name: t('program.route.remove.title') }))
    expect(screen.queryByRole('group', { name: t('program.route.to.aria') })).toBeNull()
    expect(screen.getByRole('group', { name: t('program.radius.aria') })).toBeTruthy()
    expect(origin.querySelector('.rp-lbl')?.textContent).toBe(t('program.origin.label'))
  })

  it('lists the machines in route order with where each is, and adds them to the list in that order', async () => {
    await fetched(routeResult())
    expect(calls()).toEqual(['W9AAA', 'W9BBB', 'W9CCC'])
    expect(document.querySelector('.rp-count')?.textContent).toContain(t('program.count.route', { shown: 3, total: 3 }))
    // How far along the route in the cell; how far off it leads the line under the row.
    const dist = rowOf('W9AAA').querySelector('.rp-dist')!
    expect(dist.textContent).toBe('12 mi')
    expect(rowOf('W9AAA').querySelector('.rp-src')?.textContent).toBe(
      `${t('program.row.route.off', { off: '20 mi', dir: 'E' })} · ${t('program.row.source.noDate', { sources: 'hearham', age: t('program.age.hours', { hours: 24 }) })}`,
    )
    expect(dist.getAttribute('title')).toBe(
      `${t('program.row.route.title', { along: '12 mi', off: '20 mi', dir: 'E' })} · Rockford, IL`,
    )
    fireEvent.click(screen.getByRole('button', { name: t('program.addAll.label') }))
    const names = [...document.querySelectorAll<HTMLInputElement>('.rp-chan-rows .rp-chan-name')].map((n) => n.value)
    expect(names).toEqual(['W9AAA', 'W9BBB', 'W9CCC'])
  })

  it('keeps a radius search as it was: nearest first, distance from the origin', async () => {
    await fetched(radiusResult(), false)
    expect(calls()).toEqual(['W9BBB', 'W9CCC', 'W9AAA'])
    expect(document.querySelector('.rp-count')?.textContent).toContain(t('program.count', { shown: 3, total: 3 }))
    const dist = rowOf('W9AAA').querySelector('.rp-dist')!
    expect(dist.textContent).toBe('20 mi E')
    expect(dist.getAttribute('title')).toBe('Rockford, IL')
    expect(rowOf('W9AAA').querySelector('.rp-src')?.textContent).toBe(
      t('program.row.source.noDate', { sources: 'hearham', age: t('program.age.hours', { hours: 24 }) }),
    )
  })

  it('names the states and squares a long route did not ask about, in words for a route', async () => {
    await fetched(routeResult({ rbBeyond: ['CO', 'NM', 'CA'], rsgbBeyond: ['IO95', 'IO86'] }))
    expect(notes()).toContain(
      plain(t('program.route.rbBeyond', { rb: 'RepeaterBook', hearham: 'hearham', states: 'CO, NM, CA' })),
    )
    expect(notes()).toContain(
      plain(t('program.rsgb.beyond.route', { rsgb: 'RSGB', hearham: 'hearham', squares: 'IO95, IO86' })),
    )
    cleanup()
    // CONTROL: a radius search's squares in the radius words, and no states note without states.
    await fetched(radiusResult({ rsgbBeyond: ['IO95'] }), false)
    expect(notes()).toEqual([plain(t('program.rsgb.beyond', { rsgb: 'RSGB', hearham: 'hearham', squares: 'IO95' }))])
  })

  it('says an empty route in route words, and offers the next corridor up', async () => {
    await fetched(routeResult({ rows: [] }))
    const empty = () => document.querySelector('.rp-results .aw-empty')!
    expect(empty().textContent).toContain(t('program.results.none.fm.route', { radius: '25 mi' }))
    fireEvent.click(within(empty() as HTMLElement).getByRole('button', { name: t('program.results.tryWider', { radius: '50 mi' }) }))
    const corridor = screen.getByRole('group', { name: t('program.corridor.aria') })
    expect(within(corridor).getAllByRole('button').map((c) => c.classList.contains('active'))).toEqual([false, false, true])
    // From the widest corridor there is nothing wider to offer.
    expect(within(empty() as HTMLElement).queryByRole('button', { name: /50 mi/ })).toBeNull()
    cleanup()
    // CONTROL: an empty radius search keeps its own words.
    await fetched(radiusResult({ rows: [] }), false)
    expect(empty().textContent).toContain(t('program.results.none.fm', { radius: '50 mi' }))
  })
})
