// @vitest-environment jsdom
//
// #386 — AN OPTIONAL MAXIMUM DISTANCE ON THE CALL ROSTER (a feature request, 2026-09-28): limit the
// roster to about 1000 miles, so that the stations a modest station cannot realistically work
// (Bulgaria from the US Midwest) drop out of the list. Optional, and off by default.
//
// What these tests hold:
//   · The cap is in the operator's OWN units (Settings ▸ Units): 1000 is miles to an imperial
//     operator and kilometres to a metric one. MID1 sits between the two (915 mi, 1473 km from
//     EN52), so a picker that mixed them up cannot pass both halves.
//   · It cuts on the distance the Dist column prints (grid to grid). A station whose distance is not
//     known, because no grid has been heard from it, is KEPT: absence is not a match.
//   · The station being worked, and the selected one, stay whatever their distance, as they do under
//     every other filter on this pane.
//   · The cap is stored in km with the roster's other filters, so it comes back after a restart, and
//     a change of units reads back the same distance rather than a different one.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import { ROSTER_FILTER_KEY, loadRosterFilters } from '../operateFilters'
import { UNITS_KEY } from '../units'
import { t } from '../i18n'
import type { Station } from '../types'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
}))

const SLOT = 100

function station(call: string, grid: string | null, over: Partial<Station> = {}): Station {
  return {
    call,
    grid,
    snr: -10,
    lastHeardSlot: SLOT,
    heardCount: 1,
    presence: 'heard' as Station['presence'],
    worked: false,
    ...over,
  }
}

// Distances from EN52, as grid.ts computes them for the Dist column.
const STATIONS = [
  station('NEAR1', 'EN61'), // 124 mi · 199 km
  station('MID1', 'FN42'), // 915 mi · 1473 km: inside 1000 mi, outside 1000 km
  station('LZ1FAR', 'KN12', { country: 'Bulgaria' }), // 5207 mi · 8379 km: the report's own example
  station('LZ2NOGRID', null, { country: 'Bulgaria' }), // heard only in traffic, so no distance known
]
const ALL = STATIONS.map((s) => s.call)

function mount(opts: { myGrid?: string; stations?: Station[]; selectedCall?: string | null; workingCall?: string | null } = {}) {
  return render(
    <OperateRoster
      stations={opts.stations ?? STATIONS}
      myGrid={opts.myGrid ?? 'EN52'}
      currentSlot={SLOT}
      needByCall={new Map()}
      selectedCall={opts.selectedCall ?? null}
      workingCall={opts.workingCall ?? null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
}

const picker = () => screen.getByRole('combobox', { name: t('operate.roster.filter.distance.aria') }) as HTMLSelectElement
const chosen = () => picker().selectedOptions[0]?.textContent
/** A boolean, never an element: `expect(undefined).not.toBeNull()` passes, so a query that could
 *  come back undefined would read a filtered-out row as kept. */
const shown = (call: string) => screen.queryByText(call) != null
const units = (u: 'imperial' | 'metric') => localStorage.setItem(UNITS_KEY, u)
function pick(label: string) {
  const option = [...picker().options].find((o) => o.textContent === label)
  expect(option, `the picker offers "${label}"`).toBeDefined()
  fireEvent.change(picker(), { target: { value: option!.value } })
}
const storeCap = (km: number) =>
  localStorage.setItem(
    ROSTER_FILTER_KEY,
    JSON.stringify({ neededOnly: false, hideWorked: false, hideBlocked: false, maxDistanceKm: km }),
  )

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('#386 the Call Roster distance cap: off by default', () => {
  it('shows every station, and the picker says Any distance', () => {
    units('imperial')
    mount()
    for (const call of ALL) expect(shown(call), call).toBe(true)
    expect(chosen()).toBe('Any distance')
  })
})

describe('#386 the cap is in the operator’s own units', () => {
  it('offers round numbers of miles to an imperial operator', () => {
    units('imperial')
    mount()
    expect([...picker().options].map((o) => o.textContent)).toEqual([
      'Any distance',
      'Within 250 mi',
      'Within 500 mi',
      'Within 1000 mi',
      'Within 1500 mi',
      'Within 2000 mi',
      'Within 3000 mi',
      'Within 5000 mi',
    ])
  })

  it('and round numbers of kilometres to a metric one', () => {
    units('metric')
    mount()
    expect([...picker().options].map((o) => o.textContent)).toEqual([
      'Any distance',
      'Within 250 km',
      'Within 500 km',
      'Within 1000 km',
      'Within 1500 km',
      'Within 2000 km',
      'Within 3000 km',
      'Within 5000 km',
    ])
  })

  it('Within 1000 mi hides Bulgaria and keeps a station 915 mi away', () => {
    units('imperial')
    mount()
    pick('Within 1000 mi')
    expect(shown('LZ1FAR'), 'Bulgaria, 5207 mi').toBe(false)
    expect(shown('MID1'), 'FN42, 915 mi').toBe(true)
    expect(shown('NEAR1'), 'EN61, 124 mi').toBe(true)
  })

  it('Within 1000 km hides that same station, which is 1473 km away', () => {
    units('metric')
    mount()
    pick('Within 1000 km')
    expect(shown('MID1'), 'FN42, 1473 km').toBe(false)
    expect(shown('LZ1FAR'), 'Bulgaria, 8379 km').toBe(false)
    expect(shown('NEAR1'), 'EN61, 199 km').toBe(true)
  })
})

describe('#386 what the cap never hides', () => {
  it('a station whose distance is not known: no grid heard from it yet', () => {
    units('imperial')
    mount()
    pick('Within 250 mi')
    expect(shown('MID1'), 'the control: the cap is cutting').toBe(false)
    expect(shown('LZ2NOGRID'), 'no grid heard, so no distance to cut on').toBe(true)
  })

  it('anything, while this station has no grid of its own', () => {
    units('imperial')
    mount({ myGrid: '' })
    pick('Within 250 mi')
    for (const call of ALL) expect(shown(call), call).toBe(true)
  })

  it('the station being worked, however far away it is', () => {
    units('imperial')
    mount({ workingCall: 'LZ1FAR' })
    pick('Within 250 mi')
    expect(shown('LZ1FAR'), 'the station being worked, 5207 mi away').toBe(true)
    expect(shown('MID1'), 'the control: a station not being worked, 915 mi away').toBe(false)
  })

  it('the selected station, however far away it is', () => {
    units('imperial')
    const stations = [...STATIONS, station('DL1SEL', 'JO31', { country: 'Fed. Rep. of Germany' })] // 4233 mi
    mount({ stations, selectedCall: 'DL1SEL' })
    pick('Within 1000 mi')
    expect(shown('DL1SEL'), 'the selected station').toBe(true)
    expect(shown('LZ1FAR'), 'the control: an unselected station as far away').toBe(false)
  })
})

describe('#386 the cap is kept with the roster’s other filters', () => {
  it('a cap stored by an earlier session applies from the first render', () => {
    units('imperial')
    storeCap(1000 * 1.609344)
    mount()
    expect(shown('LZ1FAR'), 'Bulgaria is outside the stored cap').toBe(false)
    expect(shown('MID1')).toBe(true)
    expect(chosen()).toBe('Within 1000 mi')
  })

  it('is stored in km, without touching the checkboxes, and survives a restart', () => {
    units('imperial')
    mount()
    pick('Within 1000 mi')
    const stored = loadRosterFilters()
    expect(stored.maxDistanceKm).toBeCloseTo(1609.344, 6)
    expect(stored).toMatchObject({ neededOnly: false, hideWorked: false, hideBlocked: false })
    cleanup()
    mount() // a fresh component, as after a restart
    expect(chosen()).toBe('Within 1000 mi')
    expect(shown('LZ1FAR')).toBe(false)
  })

  it('reads a stored cap back in the units in use now: the same distance, not a new one', () => {
    storeCap(1000 * 1.609344) // picked as 1000 mi
    units('metric')
    mount()
    expect(chosen()).toBe('Within 1609 km')
    expect(shown('MID1'), '1473 km is still inside the cap the operator set').toBe(true)
    expect(shown('LZ1FAR')).toBe(false)
  })

  it('Any distance takes the cap off, and out of storage', () => {
    units('imperial')
    storeCap(1000 * 1.609344)
    mount()
    pick('Any distance')
    for (const call of ALL) expect(shown(call), call).toBe(true)
    expect(localStorage.getItem(ROSTER_FILTER_KEY)).toBe('{"neededOnly":false,"hideWorked":false,"hideBlocked":false}')
  })
})
