// @vitest-environment jsdom
// Repeaters made simple to work (2026-10-02). An operator typed "woodstock, il" into the list's text filter, meaning to
// search near it, saw "1 of 25 shown" and took the fetch for broken: "If it was filtering only, what a clunky
// experience". So:
//   (1) ONE place says where to search: My station, or one box that takes a grid or a city (and Route to…). The list's
//       own filter sits with the list, reads as a filter of it, clears with one ✕, and a place typed into it anyway is
//       offered to Near instead of silently emptying the list. The offer is the search (2026-10-03): its tap looks the
//       place up and fetches around it, as Fetch would.
//   (2) The count line says what the filters hide and by which, with one tap that shows everything.
//   (4) A row's actions say what they do, each with its own icon: Tune, Save to Memories, Add to channel list. The ☆ is
//       gone (the saved badge took its place), and the channel list says it is for programming a radio. Its own save
//       writes into the same Memories, so it says so: Save list to Memories, not "Memory Bank" (2026-10-03).
// Every case is a PAIR: what the control does, beside the case that must not trigger it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { RepeaterRecord, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'
import { gridToLatLon } from '../grid'
import { miToKm } from '../features/radioprog'
import { emptyBank, memoriesStore } from '../features/memories'
import { subscribeToasts } from '../toast'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { EN } from '../i18n/en'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'

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

// THE BUDGET (2026-10-09). The slowest case here, "the channel list saves into Memories by that name…", takes 1.45 s
// and 0.33 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
    // The words the operator was given (2026-10-02), read as written: a placeholder compared with its own
    // catalog entry would pass whatever the entry said. It never asks for a place.
    expect(box.getAttribute('placeholder')).toBe('Filter: call or MHz')
    expect(box.getAttribute('placeholder')).not.toMatch(/city|town|place|near/i)
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

  const offer = (place: string) => screen.queryByRole('button', { name: t('program.filters.place.offer', { place }) })

  it('a place typed into the filter is offered, and the tap is the search: around it, as Fetch would, the filter cleared', async () => {
    geocodeCity.mockResolvedValue([{ displayName: 'Woodstock, McHenry County, Illinois, United States', lat: 42.31, lon: -88.45 }])
    await fetched()
    // CONTROL: a callsign fragment filters, and nothing is offered.
    fireEvent.change(filterBox(), { target: { value: 'W9' } })
    expect(offer('W9')).toBeNull()
    // The bands shown set the reach: 70cm alone reaches 25 mi, where 2m reached 50.
    fireEvent.click(within(screen.getByRole('group', { name: t('program.filters.bands.label') })).getByRole('button', { name: '2m' }))
    fireEvent.change(filterBox(), { target: { value: 'woodstock, il' } })
    expect(calls()).toEqual([])
    repeaterSearch.mockResolvedValue(result([machine('W9WDS', 442.5)]))
    fireEvent.click(offer('woodstock, il')!)
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(2))
    expect(geocodeCity).toHaveBeenCalledWith('woodstock, il')
    expect(repeaterSearch.mock.calls[1]).toEqual([42.31, -88.45, miToKm(25)])
    expect(placeBox()).toHaveProperty('value', 'woodstock, il')
    // The filter is cleared, and the list is the new search's.
    expect(filterBox()).toHaveProperty('value', '')
    await waitFor(() => expect(calls()).toEqual(['W9WDS']))
    // Exactly what Fetch sends from here.
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(3))
    expect(repeaterSearch.mock.calls[2]).toEqual(repeaterSearch.mock.calls[1])
  })

  it('words that match no place say so where the offer was, and nothing is fetched', async () => {
    geocodeCity.mockResolvedValue([])
    await fetched()
    const toasts: string[] = []
    const off = subscribeToasts((all) => toasts.push(...all.map((x) => x.message)))
    fireEvent.change(filterBox(), { target: { value: 'nowhere, zz' } })
    fireEvent.click(offer('nowhere, zz')!)
    await waitFor(() => expect(geocodeCity).toHaveBeenCalledWith('nowhere, zz'))
    const tools = document.querySelector<HTMLElement>('.rp-list-tools')!
    await waitFor(() => expect(within(tools).getByText(t('program.city.noMatch'))).toBeTruthy())
    off()
    // Said there, not in a corner as well.
    expect(toasts).not.toContain(t('program.city.noMatch'))
    expect(offer('nowhere, zz')).toBeNull()
    expect(repeaterSearch).toHaveBeenCalledTimes(1)
    // The words stay in the filter as typed; new words take the message away and are offered in their turn.
    expect(filterBox()).toHaveProperty('value', 'nowhere, zz')
    fireEvent.change(filterBox(), { target: { value: 'woodstock, il' } })
    expect(within(tools).queryByText(t('program.city.noMatch'))).toBeNull()
    expect(offer('woodstock, il')).toBeTruthy()
  })

  it('a name that is several places waits for the pick, and the pick is the search', async () => {
    geocodeCity.mockResolvedValue([
      { displayName: 'Springfield, Illinois, United States', lat: 39.8, lon: -89.65 },
      { displayName: 'Springfield, Missouri, United States', lat: 37.21, lon: -93.29 },
    ])
    await fetched()
    fireEvent.change(filterBox(), { target: { value: 'springfield' } })
    fireEvent.click(offer('springfield')!)
    const picks = await screen.findByRole('listbox', { name: t('program.city.matches.aria') })
    // CONTROL: nothing is fetched while the place is not one yet.
    expect(repeaterSearch).toHaveBeenCalledTimes(1)
    fireEvent.click(within(picks).getByRole('button', { name: 'Springfield, Missouri, United States' }))
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(2))
    expect(repeaterSearch.mock.calls[1].slice(0, 2)).toEqual([37.21, -93.29])
    expect(filterBox()).toHaveProperty('value', '')
  })

  it('a lookup that fails drops the request: the offer stands, and Near’s own Search later fetches nothing', async () => {
    geocodeCity.mockRejectedValueOnce(new Error('offline'))
    await fetched()
    fireEvent.change(filterBox(), { target: { value: 'woodstock, il' } })
    fireEvent.click(offer('woodstock, il')!)
    await waitFor(() => expect(offer('woodstock, il')).toHaveProperty('disabled', false))
    expect(repeaterSearch).toHaveBeenCalledTimes(1)
    // The same words looked up from Near, by its own Search: the town is found, and Fetch stays the operator's.
    geocodeCity.mockResolvedValue([{ displayName: 'Woodstock, McHenry County, Illinois, United States', lat: 42.31, lon: -88.45 }])
    fireEvent.click(within(screen.getByRole('group', { name: t('program.origin.aria') })).getByRole('button', { name: t('program.city.search') }))
    await waitFor(() => expect(fetchButton()).toHaveProperty('disabled', false))
    expect(repeaterSearch).toHaveBeenCalledTimes(1)
    expect(filterBox()).toHaveProperty('value', 'woodstock, il')
  })

  it('Near changed by hand while the tapped place waits is the operator’s own: nothing is fetched until Fetch', async () => {
    geocodeCity.mockResolvedValue([
      { displayName: 'Springfield, Illinois, United States', lat: 39.8, lon: -89.65 },
      { displayName: 'Springfield, Missouri, United States', lat: 37.21, lon: -93.29 },
    ])
    await fetched()
    fireEvent.change(filterBox(), { target: { value: 'springfield' } })
    fireEvent.click(offer('springfield')!)
    await screen.findByRole('listbox', { name: t('program.city.matches.aria') })
    fireEvent.change(placeBox(), { target: { value: 'FN31' } })
    expect(repeaterSearch).toHaveBeenCalledTimes(1)
    expect(filterBox()).toHaveProperty('value', 'springfield')
    // CONTROL: Fetch searches there.
    fireEvent.click(fetchButton())
    await waitFor(() => expect(repeaterSearch).toHaveBeenCalledTimes(2))
    const fn31 = gridToLatLon('FN31')!
    expect(repeaterSearch.mock.calls[1].slice(0, 2)).toEqual([fn31.lat, fn31.lon])
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

  it('the channel list saves into Memories by that name, apart from a row’s Save to Memories, in every language and the kit', async () => {
    await fetched(ROWS, true)
    fireEvent.click(within(rowOf('W9AAA')).getByRole('button', { name: t('program.row.add.label') }))
    // It writes the list into the Memories a row's Save to Memories writes to: the words as written.
    const save = within(document.querySelector<HTMLElement>('.rp-deliver')!).getByRole('button', { name: 'Save list to Memories' })
    fireEvent.click(save)
    expect(memoriesStore.get().memories.map((m) => m.rxMhz)).toEqual([146.94])
    // Each language names it with its own name for Memories (the nav's), says it is the list, and never calls it a bank;
    // a row's Save to Memories is another button, so it is another name.
    const BANK: Record<string, RegExp> = { en: /bank/i, de: /bank/i, es: /banco/i, fr: /banque/i, ja: /バンク/ }
    const LIST: Record<string, RegExp> = { en: /\blist\b/i, de: /Liste/, es: /\blista\b/i, fr: /\bliste\b/i, ja: /リスト/ }
    const wrong: string[] = []
    for (const [loc, cat] of Object.entries({ en: EN, de: DE, es: ES, fr: FR, ja: JA } as Record<string, Record<string, unknown>>)) {
      const label = String(cat['program.deliver.saveBank.label'])
      const title = String(cat['program.deliver.saveBank.title'])
      if (!label.includes(String(cat['nav.memories.label'])) || !LIST[loc].test(label)) wrong.push(`${loc}: "${label}"`)
      if (label === cat['program.row.save.label']) wrong.push(`${loc}: the row's own name`)
      if (BANK[loc].test(label) || BANK[loc].test(title)) wrong.push(`${loc}: a bank in "${label}" / "${title}"`)
    }
    expect(wrong).toEqual([])
    // The pt-BR kit carries the English word for word.
    const kit = readFileSync(resolve(process.cwd(), '../translations/pt-BR/nexus-ptbr-translation.csv'), 'utf8')
    const english = (key: string) => kit.match(new RegExp(`^"\\d","${key.replace(/\./g, '\\.')}","((?:[^"]|"")*)"`, 'm'))?.[1].replace(/""/g, '"')
    expect(english('program.deliver.saveBank.label')).toBe(EN['program.deliver.saveBank.label'])
    expect(english('program.deliver.saveBank.title')).toBe(EN['program.deliver.saveBank.title'])
  })
})
