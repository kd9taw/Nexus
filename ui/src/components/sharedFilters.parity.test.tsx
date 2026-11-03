// @vitest-environment jsdom
//
// MATCHING FILTERS ON THE TWO BOARDS (operator: "Can we have matching filters between Needed and
// Spots in the cockpits? Spots has much better filter options … that are missing on Needed").
//
// One table of stations, written once as Spots rows and once as Needed rows, and one table of
// filter actions. Every action is clicked on BOTH boards and must leave exactly the same calls on
// screen — the proof that the Needed board's new chips are the Spots board's rules
// (`spotFilters.ts`) and not a second copy that can drift.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SpotsPanel } from './SpotsPanel'
import { NeededPanel } from './NeededPanel'
import type { NeedAlert, SpotRow } from '../types'

afterEach(() => {
  cleanup()
  sessionStorage.clear()
  localStorage.clear()
})

interface Station {
  call: string
  entity: string
  band: string
  freqMhz: number
  /** The Spots row's class; the need's mode label is the same word. */
  mode: string
  /** `undefined` = a station that never judged it (an older backend): kept by "My privileges". */
  licensed: boolean | undefined
  state: string | null
}

const STATIONS: Station[] = [
  // 7.140 SSB, Extra-only: a General may not transmit to it.
  { call: 'W1AAA', entity: 'United States', band: '40m', freqMhz: 7.14, mode: 'Phone', licensed: false, state: 'VT' },
  { call: 'K2BBB', entity: 'United States', band: '20m', freqMhz: 14.025, mode: 'CW', licensed: true, state: 'NY' },
  { call: 'JA1CCC', entity: 'Japan', band: '20m', freqMhz: 14.074, mode: 'Digital', licensed: undefined, state: null },
  { call: 'VE3DDD', entity: 'Canada', band: '15m', freqMhz: 21.3, mode: 'Phone', licensed: true, state: null },
]

const asSpot = (s: Station): SpotRow =>
  ({
    call: s.call,
    entity: s.entity,
    zone: 5,
    state: s.state,
    band: s.band,
    freqMhz: s.freqMhz,
    mode: s.mode,
    submode: null,
    spotter: 'W3LPL',
    corroborators: [],
    ageSecs: 60,
    comment: '',
    licensed: s.licensed,
    spotterLocal: true,
  }) as unknown as SpotRow

const asNeed = (s: Station): NeedAlert =>
  ({
    call: s.call,
    entity: s.entity,
    band: s.band,
    zone: 5,
    tags: ['NewBand'],
    priority: 50,
    headline: 'New band',
    mode: s.mode,
    freqMhz: s.freqMhz,
    ...(s.licensed === undefined ? {} : { licensed: s.licensed }),
    state: s.state,
  }) as NeedAlert

function renderSpots() {
  render(
    <SpotsPanel spots={STATIONS.map(asSpot)} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} />,
  )
}
function renderNeeded() {
  render(
    <NeededPanel
      alerts={STATIONS.map(asNeed)}
      bandPlan={[]}
      selectedCall={null}
      onQsy={() => {}}
      onSelect={() => {}}
      onWork={() => {}}
    />,
  )
}

/** The calls on screen, in either board (each row's call button). */
function shownCalls(): string[] {
  return [...document.querySelectorAll('.np-call .qrz-link-call')].map((b) => b.textContent ?? '').sort()
}

/** Open the filter bar when it is not already open. */
function openBar() {
  if (!document.querySelector('.np-filters')) fireEvent.click(document.querySelector('.np-filter-toggle')!)
}
const chip = (name: string) => {
  openBar()
  fireEvent.click(screen.getByRole('button', { name }))
}
const search = (q: string) => fireEvent.change(screen.getByRole('searchbox'), { target: { value: q } })

const CASES: { name: string; act: () => void; shown: string[] }[] = [
  { name: 'nothing picked', act: () => {}, shown: ['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'] },
  // An explicit "may not" goes; a station that never judged it stays (fail open).
  { name: 'My privileges', act: () => chip('My privileges'), shown: ['JA1CCC', 'K2BBB', 'VE3DDD'] },
  // A picked state keeps only that state; an unknown state matches nothing.
  { name: 'a state chip', act: () => chip('VT'), shown: ['W1AAA'] },
  { name: 'two state chips', act: () => (chip('VT'), chip('NY')), shown: ['K2BBB', 'W1AAA'] },
  { name: 'a band chip', act: () => chip('20m'), shown: ['JA1CCC', 'K2BBB'] },
  { name: 'privileges and a band', act: () => (chip('My privileges'), chip('40m')), shown: [] },
  { name: 'a search term', act: () => search('k2'), shown: ['K2BBB'] },
  { name: 'search terms AND together', act: () => search('20m japan'), shown: ['JA1CCC'] },
  { name: 'a wildcard term is a whole-word prefix', act: () => search('VE3*'), shown: ['VE3DDD'] },
  { name: 'search by frequency', act: () => search('7.14'), shown: ['W1AAA'] },
]

describe('the Needed board and the Spots board filter the same rows the same way', () => {
  for (const c of CASES) {
    it(c.name, () => {
      renderSpots()
      c.act()
      const spots = shownCalls()
      cleanup()
      renderNeeded()
      c.act()
      const needed = shownCalls()
      expect(spots, 'Spots').toEqual(c.shown)
      expect(needed, 'Needed').toEqual(c.shown)
    })
  }
})

describe('the Needed board keeps its new filters as it keeps the others', () => {
  const KEY = 'neededFilters'

  it('an existing record opens with every new filter off, so nothing is suddenly hidden', () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({ needTypes: [], bands: [], modes: { Digital: true, CW: true, Phone: true } }),
    )
    renderNeeded()
    expect(shownCalls()).toEqual(['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'])
    openBar()
    expect(screen.getByRole('button', { name: 'My privileges' }).getAttribute('aria-pressed')).toBe('false')
  })

  it('a picked state, the privilege chip and the search survive the board closing', () => {
    renderNeeded()
    chip('My privileges')
    chip('NY')
    search('k2')
    cleanup()
    renderNeeded()
    expect(shownCalls()).toEqual(['K2BBB'])
    expect((screen.getByRole('searchbox') as HTMLInputElement).value).toBe('k2')
    expect(screen.getByRole('button', { name: 'My privileges' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('the search lives for the session only, as on Spots: never in the saved record', () => {
    renderNeeded()
    chip('NY')
    search('k2')
    expect(localStorage.getItem(KEY) ?? '', 'the saved record').not.toContain('k2')
    cleanup()
    // A new launch: the session is gone, the saved record stays.
    sessionStorage.clear()
    renderNeeded()
    expect((screen.getByRole('searchbox') as HTMLInputElement).value).toBe('')
    expect(shownCalls(), 'the state chip is saved; the search is not').toEqual(['K2BBB'])
  })

  it('a pane keeps its own search, apart from the board', () => {
    render(
      <NeededPanel
        alerts={STATIONS.map(asNeed)}
        bandPlan={[]}
        selectedCall={null}
        onQsy={() => {}}
        onSelect={() => {}}
        onWork={() => {}}
        pane={{ filterKey: 'nexus.cw.neededFilters' }}
      />,
    )
    search('k2')
    cleanup()
    renderNeeded()
    expect((screen.getByRole('searchbox') as HTMLInputElement).value).toBe('')
  })

  it('a stale or hand-edited record cannot empty the board', () => {
    localStorage.setItem(KEY, JSON.stringify({ licensedOnly: 'yes', states: [3, null], query: 7 }))
    renderNeeded()
    expect(shownCalls()).toEqual(['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'])
  })

  it('Clear takes the new chips off too', () => {
    renderNeeded()
    chip('My privileges')
    chip('VT')
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    expect(shownCalls()).toEqual(['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'])
  })

  it('a cockpit pane keeps its own record, so its chips never move the board', () => {
    render(
      <NeededPanel
        alerts={STATIONS.map(asNeed)}
        bandPlan={[]}
        selectedCall={null}
        onQsy={() => {}}
        onSelect={() => {}}
        onWork={() => {}}
        pane={{ filterKey: 'nexus.cw.neededFilters' }}
      />,
    )
    chip('My privileges')
    cleanup()
    renderNeeded()
    expect(shownCalls()).toEqual(['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'])
  })
})

describe('"Heard on my continent" on the Needed board', () => {
  it('is shown as a fixed rule, not a switch, and its reason is reachable by keyboard', () => {
    // The board's spotted rows were already put through the continent rule before they reach any
    // window (the same list feeds the roster chips, the map and the alerts), so a chip that could
    // turn it off would be a lie. It says so instead, and does not look like a lit toggle.
    renderNeeded()
    openBar()
    expect(screen.queryByRole('button', { name: /heard on my continent/i })).toBeNull()
    const fixed = screen.getByText('Heard on my continent')
    expect(fixed.classList.contains('active'), 'not drawn as a lit toggle').toBe(false)
    expect(fixed.getAttribute('tabindex'), 'focusable, so its tooltip is reachable').toBe('0')
    const title = fixed.getAttribute('title') ?? ''
    expect(title).toMatch(/only lists spots heard on your continent/i)
    // The callsign fail-open is said, not hidden.
    expect(title).toMatch(/callsign/i)
    fireEvent.click(fixed)
    expect(shownCalls()).toEqual(['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'])
  })
})

describe('a picked state that is no longer on the board keeps its chip, on both boards', () => {
  // A pick outlives the rows that offered it (the feed moves on; Needed keeps its picks across
  // launches). While it still filters, its chip must be there to untick.
  it('Needed', () => {
    localStorage.setItem('neededFilters', JSON.stringify({ needTypes: [], bands: [], modes: { Digital: true, CW: true, Phone: true }, states: ['AK'] }))
    renderNeeded()
    expect(shownCalls(), 'the stale pick still filters').toEqual([])
    chip('AK')
    expect(shownCalls()).toEqual(['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'])
  })

  it('Spots', () => {
    sessionStorage.setItem('nexus.spots.states', JSON.stringify(['AK']))
    renderSpots()
    expect(shownCalls()).toEqual([])
    chip('AK')
    expect(shownCalls()).toEqual(['JA1CCC', 'K2BBB', 'VE3DDD', 'W1AAA'])
  })
})
