// @vitest-environment jsdom
// Repeaters made simple to work (2026-10-02). An operator typed "woodstock, il" into the list's text filter, meaning to
// search near it, saw "1 of 25 shown" and took the fetch for broken: "If it was filtering only, what a clunky
// experience". So:
//   (1) ONE place says where to search: My station, or one box that takes a grid or a city (and Route to…). The list's
//       own filter sits with the list, reads as a filter of it, clears with one ✕, and a place typed into it anyway is
//       offered to Near instead of silently emptying the list.
//   (2) The count line says what the filters hide and by which, with one tap that shows everything.
//   (4) A row's actions say what they do, each with its own icon: Tune, Save to Memories, Add to channel list. The ☆ is
//       gone (the saved badge took its place), and the channel list says it is for programming a radio.
// Every case is a PAIR: what the control does, beside the case that must not trigger it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { RepeaterRecord, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'
import { gridToLatLon } from '../grid'
import { miToKm } from '../features/radioprog'
import { emptyBank, memoriesStore } from '../features/memories'

const repeaterSearch = vi.fn()
const geocodeCity = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  geocodeCity: (...a: unknown[]) => geocodeCity(...a),
  radioprogListProjects: vi.fn(async () => []),
  radioprogFileNotice: vi.fn(async () => null),
  radioprogSaveProject: vi.fn(async () => undefined),
}))

import { RadioProgView } from './RadioProgView'

function machine(call: string, mhz: number, over: Partial<RepeaterRecord> = {}): RepeaterSearchRow {
  const record: RepeaterRecord = {
    source: 'hearham', sourceId: call, callsign: call, outputMhz: mhz, inputMhz: mhz - 0.6, ctcssEncHz: 100,
    ctcssDecHz: null, dcs: null, lat: 42.3, lon: -88.4, city: 'Harvard', county: '', state: 'IL', fm: true, dmr: false,
    dstar: false, fusion: false, dmrColorCode: null, bandwidthKhz: null, operational: true, openUse: true, updated: null,
    distanceKm: 10, bearingDeg: 90, ...over,
  }
  return {
    record,
    channel: {
      id: `hh:${call}`, name: call, rxMhz: mhz, duplex: 'minus', offsetMhz: 0.6, toneMode: 'tone', rtoneHz: 100,
      ctoneHz: 100, dtcsCode: 23, mode: 'fm', comment: record.city, source: { source: 'hearham', sourceId: call, callsign: call },
    },
    sources: [{ source: 'hearham', sourceId: call, channelId: `hh:${call}`, updated: null }],
    disagreements: [],
  }
}
/** Four on the air and one off it. */
const ROWS = [
  machine('W9AAA', 146.94),
  machine('W9BBB', 147.18),
  machine('K9CCC', 444.1),
  machine('K9DDD', 443.3),
  machine('W9OFF', 145.21, { operational: false }),
]
const result = (rows: RepeaterSearchRow[]): RepeaterSearchResult => ({
  lists: [{ source: 'hearham', fetchedUtc: Math.floor(Date.now() / 1000) - 3600, stale: false }],
  coverageGap: null, missingStates: [], rsgbUnavailable: false, rsgbBeyond: [], rows,
})
const fetchButton = () => screen.getByRole('button', { name: t('program.fetch.label') })
const placeBox = () => screen.getByRole('textbox', { name: t('program.origin.place.aria') })
const filterBox = () => screen.getByRole('searchbox', { name: t('program.filters.search.aria') })
const count = () => document.querySelector('.rp-count')?.textContent ?? ''
const calls = () => [...document.querySelectorAll('.rp-results .rp-row .rp-call')].map((c) => c.textContent)
const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row')].find((r) => r.querySelector('.rp-call')?.textContent === call)!

async function fetched(rows = ROWS, catOk = false) {
  repeaterSearch.mockResolvedValue(result(rows))
  render(<RadioProgView myGrid="EN52" catOk={catOk} />)
  fireEvent.click(fetchButton())
  await waitFor(() => expect(document.querySelector('.rp-count')).toBeTruthy())
}

beforeEach(() => {
  cleanup()
  localStorage.clear()
  memoriesStore.set(emptyBank())
  repeaterSearch.mockReset()
  geocodeCity.mockReset()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  ;(Element.prototype as unknown as { scrollIntoView: () => void }).scrollIntoView = () => {}
})
afterEach(cleanup)

describe('(1) one place says where; the list filter is a filter of the list', () => {
  it('the Near row is My station or one box that takes a grid: no Grid and City chips', async () => {
    repeaterSearch.mockResolvedValue(result(ROWS))
    render(<RadioProgView myGrid="EN52" />)
    const near = screen.getByRole('group', { name: t('program.origin.aria') })
    expect(within(near).queryByRole('button', { name: /^Grid$|^City$/ })).toBeNull()
    expect(within(near).getAllByRole('textbox')).toHaveLength(1)
    // CONTROL: with the box empty, the search is around the station.
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(1))
    const station = gridToLatLon('EN52')!
    expect(repeaterSearch.mock.calls[0].slice(0, 2)).toEqual([station.lat, station.lon])
    // A locator typed into the box is the place, as typed: no lookup.
    fireEvent.change(placeBox(), { target: { value: 'fn31' } })
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(2))
    const fn31 = gridToLatLon('FN31')!
    expect(repeaterSearch.mock.calls[1]).toEqual([fn31.lat, fn31.lon, miToKm(50)])
    expect(geocodeCity).not.toHaveBeenCalled()
  })

  it('a town typed into the same box is looked up, and the search is around what it found', async () => {
    geocodeCity.mockResolvedValue([{ displayName: 'Gatlinburg, Tennessee, United States', lat: 35.71, lon: -83.51 }])
    repeaterSearch.mockResolvedValue(result(ROWS))
    render(<RadioProgView myGrid="EN52" />)
    fireEvent.change(placeBox(), { target: { value: 'Gatlinburg TN' } })
    // Not a place yet: nothing to fetch until the town is found.
    expect(fetchButton()).toHaveProperty('disabled', true)
    fireEvent.keyDown(placeBox(), { key: 'Enter' })
    await waitFor(() => expect(geocodeCity).toHaveBeenCalledWith('Gatlinburg TN'))
    await waitFor(() => expect(fetchButton()).toHaveProperty('disabled', false))
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(1))
    expect(repeaterSearch.mock.calls[0].slice(0, 2)).toEqual([35.71, -83.51])
  })

  it('the filter sits with the list, says it filters it, and one ✕ clears it', async () => {
    await fetched()
    const box = filterBox()
    expect(box.getAttribute('placeholder')).toBe(t('program.filters.search.placeholder'))
    // Not with the place: outside the Search card, right above the list.
    expect(box.closest('.rp-query')).toBeNull()
    expect(box.closest('.rp-list-tools')?.nextElementSibling?.classList.contains('rp-results')).toBe(true)
    // CONTROL: nothing typed, nothing to clear.
    expect(screen.queryByRole('button', { name: t('program.filters.search.clear') })).toBeNull()
    fireEvent.change(box, { target: { value: 'W9B' } })
    expect(calls()).toEqual(['W9BBB'])
    fireEvent.click(screen.getByRole('button', { name: t('program.filters.search.clear') }))
    expect(filterBox()).toHaveProperty('value', '')
    expect(calls()).toEqual(['W9AAA', 'W9BBB', 'K9CCC', 'K9DDD'])
  })

  it('a place typed into the filter is offered to Near, which looks it up; a callsign is not', async () => {
    geocodeCity.mockResolvedValue([])
    await fetched()
    const offer = (place: string) => screen.queryByRole('button', { name: t('program.filters.place.offer', { place }) })
    // CONTROL: a callsign fragment filters, and nothing is offered.
    fireEvent.change(filterBox(), { target: { value: 'W9' } })
    expect(offer('W9')).toBeNull()
    fireEvent.change(filterBox(), { target: { value: 'woodstock, il' } })
    expect(calls()).toEqual([])
    fireEvent.click(offer('woodstock, il')!)
    await waitFor(() => expect(geocodeCity).toHaveBeenCalledWith('woodstock, il'))
    expect(placeBox()).toHaveProperty('value', 'woodstock, il')
    // The filter is cleared: the list is whole again, and nothing was fetched behind the operator's back.
    expect(filterBox()).toHaveProperty('value', '')
    expect(calls()).toEqual(['W9AAA', 'W9BBB', 'K9CCC', 'K9DDD'])
    expect(repeaterSearch).toHaveBeenCalledTimes(1)
  })
})

describe('(2) the count line says what is hidden, with one tap to show it all', () => {
  it('names On-air only when it hides a machine, and Show all brings everything back', async () => {
    await fetched()
    const hidden = t('program.count.hidden', { count: 1, why: t('program.filters.onAir.label') })
    expect(count()).toContain(t('program.count', { shown: 4, total: 5 }))
    expect(count()).toContain(hidden)
    fireEvent.click(screen.getByRole('button', { name: t('program.count.showAll') }))
    expect(count()).toContain(t('program.count', { shown: 5, total: 5 }))
    expect(document.querySelector('.rp-hidden')).toBeNull()
    expect(calls()).toHaveLength(5)
  })

  it('names the text filter and the bands with the words the operator typed and chose', async () => {
    await fetched()
    fireEvent.change(filterBox(), { target: { value: 'W9' } })
    // W9AAA and W9BBB are shown; K9CCC and K9DDD fail the filter, W9OFF the on-air chip.
    // The language's own list ("On-air only and the filter “W9”"), as the view joins it.
    const LF = (Intl as unknown as { ListFormat: new (l: string, o: object) => { format(x: string[]): string } }).ListFormat
    const why = new LF('en', { style: 'long', type: 'conjunction' }).format([
      t('program.filters.onAir.label'),
      t('program.count.why.text', { text: 'W9' }),
    ])
    expect(count()).toContain(t('program.count.hidden', { count: 3, why }))
    cleanup()
    // CONTROL: nothing hidden, nothing said.
    await fetched(ROWS.slice(0, 2))
    expect(document.querySelector('.rp-hidden')).toBeNull()
    expect(screen.queryByRole('button', { name: t('program.count.showAll') })).toBeNull()
  })
})

describe('(4) a row says what each action does, and Memories and the channel list read apart', () => {
  it('Tune, Save to Memories and Add to channel list, each with its own icon, and no ☆', async () => {
    await fetched(ROWS, true)
    const row = rowOf('W9AAA')
    const names = within(row).getAllByRole('button').map((b) => b.textContent)
    expect(names).toEqual([t('program.row.tune.label'), t('program.row.save.label'), t('program.row.add.label')])
    expect(document.querySelector('.rp-results')!.textContent).not.toMatch(/[☆★]/)
    const icon = (name: string) => within(row).getByRole('button', { name }).querySelector('svg')?.getAttribute('class') ?? ''
    expect(icon(t('program.row.save.label'))).toMatch(/bookmark/)
    expect(icon(t('program.row.add.label'))).toMatch(/list/)
    expect(icon(t('program.row.tune.label'))).toMatch(/radio/)
    // The two destinations never share an icon.
    expect(icon(t('program.row.save.label'))).not.toBe(icon(t('program.row.add.label')))
  })

  it('saving shows the badge in the button’s place, adding names the channel list, and the list says what it is for', async () => {
    await fetched(ROWS, true)
    const row = () => rowOf('W9AAA')
    fireEvent.click(within(row()).getByRole('button', { name: t('program.row.save.label') }))
    await waitFor(() => expect(row().querySelector('.rp-saved-badge')?.textContent).toBe(t('program.row.saved.label')))
    expect(within(row()).queryByRole('button', { name: t('program.row.save.label') })).toBeNull()
    fireEvent.click(within(row()).getByRole('button', { name: t('program.row.add.label') }))
    await waitFor(() => expect(within(row()).getByRole('button', { name: t('program.row.added.label') })).toBeTruthy())
    expect(document.querySelector('.rp-builder-head')?.textContent).toContain(t('program.builder.sub'))
    // CONTROL: the other rows are untouched.
    expect(within(rowOf('W9BBB')).getByRole('button', { name: t('program.row.save.label') })).toBeTruthy()
    expect(within(rowOf('W9BBB')).getByRole('button', { name: t('program.row.add.label') })).toBeTruthy()
  })
})
