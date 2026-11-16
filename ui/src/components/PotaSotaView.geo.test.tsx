// @vitest-environment jsdom
//
// WHERE THE ACTIVATOR IS — the POTA/SOTA board's continent chips and country picker, so a
// low-power station can list only the activations it can reach.
//
// The station places each spot (`country`, `continent`: cty.dat's entity and continent, null when
// nothing places it); these tests take that as given. Both are pickers in the Sort row, not a row of
// their own: a row costs the feed its height at a narrow window. The board's job: offer only the continents
// and countries on the list now, in a fixed continent order and an alphabetical country order,
// default to All, remember the choice per surface, show an unplaced spot under All only, and never
// let a remembered place that is on nobody's list now hide the board without saying so and
// offering the way back.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import type { AppSnapshot, OtaSpot } from '../types'
import { t } from '../i18n'
import { continentName } from './CountryExclude'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
  selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'

const spot = (
  activator: string,
  reference: string,
  country: string | null,
  continent: string | null,
): OtaSpot => ({
  program: 'POTA',
  reference,
  name: 'Test park',
  activator,
  freqKhz: 14_285,
  mode: 'SSB',
  spotter: null,
  comment: null,
  grid: null,
  newPark: false,
  bandOpen: false,
  huntedToday: false,
  country,
  continent,
})
const snap = { hunt: null, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot

// Listed out of continent order on purpose: the chips must not follow the feed's order.
const FEED = [
  spot('DL1AAA', 'DE-0001', 'Fed. Rep. of Germany', 'EU'),
  spot('KD9TAW', 'US-0001', 'United States', 'NA'),
  spot('VE3/KD9TAW', 'CA-0001', 'Canada', 'NA'),
  spot('G1AAA', 'GB-0001', 'England', 'EU'),
  spot('X0NONE', 'XX-0001', null, null),
]

const continentPick = () => screen.getByRole('combobox', { name: t('ota.filter.continent.aria') }) as HTMLSelectElement
const countryPick = () => screen.getByRole('combobox', { name: t('ota.filter.country.aria') }) as HTMLSelectElement
const pickContinent = (code: string) => fireEvent.change(continentPick(), { target: { value: code } })
const optionLabels = (sel: HTMLSelectElement) => [...sel.options].map((o) => o.textContent)

beforeEach(() => {
  api.getOtaSpots.mockReset()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  window.history.replaceState(null, '', '/')
})

describe('Continent and country filters', () => {
  it('default to All and hide nothing, an unplaced spot included', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    for (const call of ['KD9TAW', 'VE3/KD9TAW', 'G1AAA', 'X0NONE']) expect(screen.getByText(call)).toBeTruthy()
    expect(continentPick().value).toBe('All')
    expect(countryPick().value).toBe('All')
  })

  it('offers only the continents on the list, in a fixed order', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    expect(optionLabels(continentPick())).toEqual([t('ota.filter.all'), continentName('NA'), continentName('EU')])
    expect([...continentPick().options].map((o) => o.value), 'the stored values are the codes').toEqual(['All', 'NA', 'EU'])
  })

  it('lists only the countries on the list, alphabetically, narrowed to the chosen continent', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    expect(optionLabels(countryPick())).toEqual([
      t('ota.filter.all'),
      'Canada',
      'England',
      'Fed. Rep. of Germany',
      'United States',
    ])
    pickContinent('EU')
    expect(optionLabels(countryPick())).toEqual([t('ota.filter.all'), 'England', 'Fed. Rep. of Germany'])
  })

  it('a continent shows only its spots; an unplaced spot shows under All only', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    pickContinent('NA')
    expect(screen.getByText('KD9TAW')).toBeTruthy()
    expect(screen.getByText('VE3/KD9TAW')).toBeTruthy()
    expect(screen.queryByText('DL1AAA')).toBeNull()
    expect(screen.queryByText('X0NONE')).toBeNull()
  })

  it('a country shows only its spots: the portable call is where the activator is', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    fireEvent.change(countryPick(), { target: { value: 'Canada' } })
    expect(screen.getByText('VE3/KD9TAW')).toBeTruthy()
    expect(screen.queryByText('KD9TAW')).toBeNull()
    expect(screen.queryByText('X0NONE')).toBeNull()
  })

  it('both are remembered between sessions, and the Connect box keeps its own', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    const first = render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    pickContinent('EU')
    fireEvent.change(countryPick(), { target: { value: 'England' } })
    expect(localStorage.getItem('nexus.ota.continentFilter')).toBe('EU')
    expect(localStorage.getItem('nexus.ota.countryFilter')).toBe('England')
    expect(localStorage.getItem('nexus.connect.ota.continentFilter'), 'the box is not moved').toBeNull()
    expect(localStorage.getItem('nexus.connect.ota.countryFilter'), 'the box is not moved').toBeNull()
    first.unmount()

    render(<PotaSotaView snap={snap} />)
    await screen.findByText('G1AAA')
    expect(screen.queryByText('DL1AAA')).toBeNull()
    expect(countryPick().value).toBe('England')
  })

  it('a remembered country on nobody’s list now says so and offers one click back', async () => {
    localStorage.setItem('nexus.ota.countryFilter', 'Japan')
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    expect(await screen.findByText(t('ota.empty.place', { place: 'Japan' }))).toBeTruthy()
    // The picker still shows what is picked, so the operator can see why the board is empty.
    expect(countryPick().value).toBe('Japan')
    fireEvent.click(screen.getByRole('button', { name: t('ota.empty.place.reset') }))
    expect(screen.getByText('DL1AAA')).toBeTruthy()
    expect(screen.getByText('X0NONE')).toBeTruthy()
    expect(localStorage.getItem('nexus.ota.countryFilter')).toBe('All')
  })

  it('a remembered continent on nobody’s list now says so, keeps its option picked, and offers one click back', async () => {
    localStorage.setItem('nexus.ota.continentFilter', 'OC')
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    expect(await screen.findByText(t('ota.empty.place', { place: continentName('OC') }))).toBeTruthy()
    expect(continentPick().value).toBe('OC')
    expect(optionLabels(continentPick())).toContain(continentName('OC'))
    fireEvent.click(screen.getByRole('button', { name: t('ota.empty.place.reset') }))
    expect(screen.getByText('DL1AAA')).toBeTruthy()
    expect(localStorage.getItem('nexus.ota.continentFilter')).toBe('All')
  })

  it('a hand-edited continent code that names no continent is All', async () => {
    localStorage.setItem('nexus.ota.continentFilter', 'XX')
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    expect(screen.getByText('X0NONE')).toBeTruthy()
  })

  it('a row from a station that sends no place shows under All, and no picker hides it', async () => {
    const legacy = spot('W1AW', 'US-0002', null, null)
    delete (legacy as Partial<OtaSpot>).country
    delete (legacy as Partial<OtaSpot>).continent
    api.getOtaSpots.mockResolvedValue([legacy])
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('W1AW')
    expect(screen.queryByRole('combobox', { name: t('ota.filter.continent.aria') })).toBeNull()
    expect(screen.queryByRole('combobox', { name: t('ota.filter.country.aria') })).toBeNull()
  })

  it('both pickers sit in the Sort row: the place filters add no row of their own', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('DL1AAA')
    // The row's group is named for both jobs it does, so a screen reader does not announce the
    // place filters as sort choices.
    const sortRow = screen.getByRole('group', { name: t('ota.sortPlace.aria') })
    expect(screen.queryByRole('group', { name: t('ota.sort.aria') })).toBeNull()
    expect(within(sortRow).getByRole('combobox', { name: t('ota.filter.continent.aria') })).toBeTruthy()
    expect(within(sortRow).getByRole('combobox', { name: t('ota.filter.country.aria') })).toBeTruthy()
  })

  it('with no place to pick from, the row is the Sort row alone and is named so', async () => {
    const legacy = spot('W1AW', 'US-0002', null, null)
    delete (legacy as Partial<OtaSpot>).country
    delete (legacy as Partial<OtaSpot>).continent
    api.getOtaSpots.mockResolvedValue([legacy])
    render(<PotaSotaView snap={snap} />)
    await screen.findByText('W1AW')
    expect(screen.getByRole('group', { name: t('ota.sort.aria') })).toBeTruthy()
    expect(screen.queryByRole('group', { name: t('ota.sortPlace.aria') })).toBeNull()
  })

  it('a place on the list with a band that empties it says the filters, not the place', async () => {
    // England is on the list (on 20 m); 40 m is what hides it.
    localStorage.setItem('nexus.ota.countryFilter', 'England')
    localStorage.setItem('nexus.ota.bandFilter', JSON.stringify(['40m']))
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    expect(await screen.findByText(t('ota.empty.filtered'))).toBeTruthy()
    expect(screen.queryByText(t('ota.empty.place', { place: 'England' }))).toBeNull()
    expect(screen.queryByRole('button', { name: t('ota.empty.place.reset') })).toBeNull()
  })

  it('a place with a mode that empties it says the filters too', async () => {
    localStorage.setItem('nexus.ota.continentFilter', 'EU')
    localStorage.setItem('nexus.ota.modeFilter', 'CW')
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} />)
    expect(await screen.findByText(t('ota.empty.filtered'))).toBeTruthy()
  })

  it('the Connect box’s Filter button says Filtered from a place alone', async () => {
    localStorage.setItem('nexus.connect.ota.continentFilter', 'EU')
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} pane />)
    await screen.findByText('DL1AAA')
    const toggle = screen.getByRole('button', { name: t('spots.filter.toggle.active') })
    expect(toggle.className).toContain('active')
    expect(screen.queryByText('KD9TAW'), 'the place filter applies with the rows closed').toBeNull()
  })

  it('the Connect box’s Filter button is idle with every filter at All', async () => {
    api.getOtaSpots.mockResolvedValue(FEED)
    render(<PotaSotaView snap={snap} pane />)
    await screen.findByText('DL1AAA')
    expect(screen.getByRole('button', { name: t('spots.filter.toggle.idle') })).toBeTruthy()
  })
})
