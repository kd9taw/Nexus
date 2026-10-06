// @vitest-environment jsdom
//
// THE STATE AN ACTIVATOR IS IN, ON THE POTA/SOTA BOARD (operator, 2026-10-05: "State on rows, needed lit").
//
// Each row shows the state its activator is in: a park's from pota.app (each of them, for a park on a state line), a
// summit's from its SOTA association. A state the log still needs for Worked All States lights in the WAS colour, by the
// station's need scorer (`neededStates`). Which states a row carries and which are needed are the station's answers,
// which these tests take as given; the board's job is to show them, light them, name them in words, and say what a
// park on a state line means for the contact.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen, within } from '@testing-library/react'
import type { AppSnapshot, OtaSpot } from '../types'
import { t } from '../i18n'
import { placeLabel } from '../features/otaStates'

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

const spot = (activator: string, reference: string, over: Partial<OtaSpot> = {}): OtaSpot => ({
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
  states: [],
  neededStates: [],
  ...over,
})
const snap = { hunt: null, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot

afterEach(() => {
  cleanup()
  localStorage.clear()
})

/** The board on `program`, showing `spots`, once its first fetch has landed. */
async function board(spots: OtaSpot[], program: 'POTA' | 'SOTA' | 'Both' = 'POTA') {
  localStorage.setItem('nexus.ota.program', program)
  api.getOtaSpots.mockResolvedValue(spots)
  render(<PotaSotaView snap={snap} />)
  await screen.findByText(spots[0].activator)
}

/** The row whose activator is `call`. */
const row = (call: string) => screen.getByText(call).closest('li') as HTMLElement
/** The state label a row shows, or null. */
const stateOf = (call: string) => row(call).querySelector<HTMLElement>('.pota-spot-state')

describe("the state on the POTA/SOTA board's rows", () => {
  it('a POTA row shows the state its park is in, and names it in words', async () => {
    await board([spot('K0ABC', 'US-0700', { states: ['US-ND'] })])
    const label = stateOf('K0ABC')
    expect(label?.querySelector('[aria-hidden="true"]')?.textContent).toBe('ND')
    // In words, not only the code: the row's accessible name and what a reader reads in the label.
    expect(screen.getByRole('listitem', { name: new RegExp(t('place.us.nd')) })).toBe(row('K0ABC'))
    expect(within(label!).getByText(t('place.us.nd'))).toBeTruthy()
  })

  it('a park on a state line shows each state, and says which one the contact counts for', async () => {
    await board([spot('N7ABC', 'US-4567', { states: ['US-MT', 'US-ND'] })])
    expect(stateOf('N7ABC')?.querySelector('[aria-hidden="true"]')?.textContent).toBe('MT·ND')
    const named = row('N7ABC').getAttribute('title') ?? ''
    expect(named).toContain(
      t('ota.spot.state.spans', { states: `${t('place.us.mt')}, ${t('place.us.nd')}` }),
    )
  })

  it("a SOTA row shows its association's state, or nothing", async () => {
    await board(
      [
        spot('K7ABC', 'W7M/MT-001', { program: 'SOTA', states: ['US-MT'] }),
        spot('G0ABC', 'G/LD-001', { program: 'SOTA', states: [] }),
      ],
      'SOTA',
    )
    expect(stateOf('K7ABC')?.querySelector('[aria-hidden="true"]')?.textContent).toBe('MT')
    expect(stateOf('G0ABC'), 'a summit with no state shows none').toBeNull()
  })

  it('shows nothing where the station sends no states (an older station, an observed Remote feed)', async () => {
    const legacy = spot('K1ABC', 'US-0001')
    delete (legacy as Partial<OtaSpot>).states
    delete (legacy as Partial<OtaSpot>).neededStates
    await board([legacy, spot('K0ABC', 'US-0700', { states: ['US-ND'] })])
    expect(stateOf('K1ABC')).toBeNull()
    // The control: the same board shows a state where one is sent.
    expect(stateOf('K0ABC')).not.toBeNull()
  })

  it('a needed state is lit in the WAS colour, a worked one is not, and the need is said in words', async () => {
    await board([
      spot('K0ABC', 'US-0700', { states: ['US-ND'], neededStates: ['US-ND'] }),
      spot('W7ABC', 'US-0800', { states: ['US-MT'], neededStates: [] }),
    ])
    expect(stateOf('K0ABC')?.classList.contains('need-state')).toBe(true)
    expect(stateOf('W7ABC')?.classList.contains('need-state')).toBe(false)
    // Not colour alone: the lit label and the row say it.
    expect(within(stateOf('K0ABC')!).getByText(t('ota.spot.state.needed.sr', { state: t('place.us.nd') }))).toBeTruthy()
    expect(row('K0ABC').getAttribute('title')).toContain(
      t('ota.spot.state.needed', { state: t('place.us.nd'), band: '20m' }),
    )
    expect(row('W7ABC').getAttribute('title')).not.toContain(t('ota.spot.state.needed', { state: t('place.us.mt'), band: '20m' }))
  })

  it('a park on a state line lights when either state is needed, and names the one that is', async () => {
    await board([spot('N7ABC', 'US-4567', { states: ['US-MT', 'US-ND'], neededStates: ['US-ND'] })])
    expect(stateOf('N7ABC')?.classList.contains('need-state')).toBe(true)
    const named = row('N7ABC').getAttribute('title') ?? ''
    expect(named).toContain(t('ota.spot.state.needed', { state: t('place.us.nd'), band: '20m' }))
    expect(named).not.toContain(t('ota.spot.state.needed', { state: t('place.us.mt'), band: '20m' }))
  })
})

describe('the state label stays short', () => {
  it('spells out up to three states, then two and how many more, the needed ones first', () => {
    expect(placeLabel(['US-ND'])).toBe('ND')
    expect(placeLabel(['US-ID', 'US-MT', 'US-WY'])).toBe('ID·MT·WY')
    const trail = ['US-ID', 'US-IA', 'US-KS', 'US-MO', 'US-MT', 'US-NE', 'US-ND', 'US-OR', 'US-SD', 'US-WA', 'US-IL']
    expect(placeLabel(trail)).toBe('ID·IA +9')
    expect(placeLabel(trail, ['US-ND', 'US-MT'])).toBe('MT·ND +9')
  })
})
