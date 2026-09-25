// @vitest-environment jsdom
//
// A station on the operator's WATCH LIST stands out on the Call Roster (maintainer, 2026-09-23).
//
// The report behind it: the watch list gives one loud alert, and after that nothing on the lists
// marks the station. So a matching row carries a WATCH tile — computed from the list the operator
// already keeps, with no extra setup — and a watch match counts as a need, the way a new park
// does: it stays under Needed only and under Hide worked, even after the station has been worked.
//
// The list is set up exactly where the app keeps it (`nexus.watchlist`), and the live-edit cases
// drive the REAL Settings manager (`WatchlistPanel`), so these tests cover the path an operator
// takes rather than a prop nobody passes.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import { WatchlistPanel } from './WatchlistPanel'
import { ROSTER_FILTER_KEY } from '../operateFilters'
import { saveWatchlist, type WatchFilter } from '../watchlist'
import type { NeedAlert, NeedTag, Station } from '../types'
import { NEED_TIER } from '../features/needs'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
}))

const SLOT = 100

function station(call: string, over: Partial<Station> = {}): Station {
  return {
    call,
    grid: 'EN52',
    snr: -10,
    lastHeardSlot: SLOT,
    heardCount: 1,
    presence: 'active',
    worked: false,
    ...over,
  }
}

const alert = (call: string, tags: NeedTag[]): NeedAlert => ({
  call,
  entity: 'United States',
  band: '20m',
  zone: 4,
  tags,
  priority: NEED_TIER[tags[0]],
  headline: '',
  mode: 'FT8',
  freqMhz: 14.074,
})

// One entry per kind the list supports. The call entry is CQ-only, and its station is busy
// working somebody: the tile marks WHO is on the list, and CQ-only only decides when to alert.
const VP8: WatchFilter = { id: 'w-vp8', kind: 'call', value: 'VP8*', cqOnly: true }
const K1ABC: WatchFilter = { id: 'w-k1', kind: 'call', value: 'K1ABC', label: 'Club station' }
const BOUVET: WatchFilter = { id: 'w-3y', kind: 'dxcc', value: 'Bouvet' }
const FN3: WatchFilter = { id: 'w-fn', kind: 'grid', value: 'FN3*' }

const STATIONS = [
  station('VP8PJ', { calling: 'W9XYZ', country: 'Falkland Islands', grid: null }),
  station('K1ABC', { country: 'United States' }),
  station('3Y0J', { country: 'Bouvet', grid: null }),
  station('W1AW', { country: 'United States', grid: 'FN31' }),
  // The CONTROL: nothing on the list names it, and it has no need of its own.
  station('PLAIN1', { country: 'United States' }),
  // Worked on this band, nothing to gain, not watched — the filter must still hide it.
  station('WORKED1', { worked: true, workedBand: true, country: 'United States' }),
]

function mount(opts: { neededOnly?: boolean; hideWorked?: boolean; stations?: Station[]; alerts?: Map<string, NeedAlert[]> } = {}) {
  localStorage.setItem(
    ROSTER_FILTER_KEY,
    JSON.stringify({ neededOnly: !!opts.neededOnly, hideWorked: !!opts.hideWorked, hideBlocked: false }),
  )
  return render(
    <OperateRoster
      stations={opts.stations ?? STATIONS}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={new Map()}
      needAlertsByCall={opts.alerts ?? new Map()}
      band="20m"
      feedMode="FT8"
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
}

// Both return NULL for "not there" — never undefined. An optional chain yields undefined, and
// `expect(undefined).not.toBeNull()` PASSES: a filtered-out row would read as "kept".
const rowOf = (call: string): HTMLElement | null =>
  (screen.queryByText(call)?.closest('[role="row"]') as HTMLElement | null | undefined) ?? null
const tileOf = (call: string): HTMLElement | null =>
  (rowOf(call)?.querySelector('.need-chip.need-watch') as HTMLElement | null | undefined) ?? null

beforeEach(() => {
  localStorage.clear()
  saveWatchlist([VP8, K1ABC, BOUVET, FN3])
})
afterEach(cleanup)

describe('the WATCH tile on the Call Roster', () => {
  it('marks a row the list names by call, prefix wildcard, entity or grid', () => {
    mount()
    for (const call of ['VP8PJ', 'K1ABC', '3Y0J', 'W1AW']) {
      expect(tileOf(call), `${call} carries no WATCH tile`).not.toBeNull()
      expect(tileOf(call)!.textContent).toBe('WATCH')
    }
  })

  it('leaves a row the list does not name alone', () => {
    mount()
    expect(rowOf('PLAIN1'), 'the control row is on the roster').not.toBeNull()
    expect(tileOf('PLAIN1')).toBeNull()
  })

  it('names the entry that matched in the tooltip', () => {
    mount()
    expect(tileOf('VP8PJ')!.getAttribute('title')).toBe('On your watch list: VP8*')
    expect(tileOf('3Y0J')!.getAttribute('title')).toBe('On your watch list: Bouvet')
    expect(tileOf('W1AW')!.getAttribute('title')).toBe('On your watch list: grid FN3*')
    // An entry the operator gave a name is called by that name.
    expect(tileOf('K1ABC')!.getAttribute('title')).toBe('On your watch list: Club station')
  })

  it('leads the Need cell, so a crowded cell clips a need chip and never the tile', () => {
    mount({ alerts: new Map([['VP8PJ', [alert('VP8PJ', ['NewEntity', 'NewZone'])]]]) })
    const cell = rowOf('VP8PJ')!.querySelector('.or-need')!
    expect(cell.firstElementChild?.classList.contains('need-watch')).toBe(true)
    // …and the needs are all still there beside it.
    expect(cell.querySelectorAll('.need-chip:not(.need-watch)')).toHaveLength(2)
  })

  it('a station the STATION marks watched shows one mark — the tile — not a second WATCH beside it', () => {
    // The station puts a watched station first on the Needed board with the `Wanted` need, and the
    // roster reads the same alerts (operator 2026-09-24: "watched counts as needed").
    mount({
      alerts: new Map([
        ['VP8PJ', [alert('VP8PJ', ['Wanted', 'NewEntity'])]],
        ['PLAIN1', [alert('PLAIN1', ['Wanted'])]],
      ]),
    })
    const cell = rowOf('VP8PJ')!.querySelector('.or-need')!
    const marks = cell.querySelectorAll('.need-watch')
    expect(marks, 'the same fact twice').toHaveLength(1)
    expect(marks[0].getAttribute('title'), 'the one mark is the tile, naming the entry').toBe('On your watch list: VP8*')
    expect(cell.querySelectorAll('.need-chip:not(.need-watch)'), 'the new one still rides').toHaveLength(1)
    // Where this window's list draws no tile (a Remote browser keeps a list of its own), the
    // station's mark is the only one, and it stays — in the tile's own look.
    const mark = rowOf('PLAIN1')!.querySelector('.or-need .need-chip.need-watch')
    expect(mark, "the station's mark is gone").not.toBeNull()
    expect(mark!.textContent).toBe('WATCH')
  })

  it('says so in the row’s accessible name, which is what a screen reader reads for the row', () => {
    mount()
    expect(rowOf('VP8PJ')!.getAttribute('aria-label')).toContain('on your watch list: VP8*')
    expect(rowOf('PLAIN1')!.getAttribute('aria-label')).not.toContain('watch list')
  })
})

describe('a watch match counts as a need under the roster filters', () => {
  it('Needed only keeps a watched station that has no other need', () => {
    mount({ neededOnly: true })
    expect(rowOf('VP8PJ'), 'watched, no need of its own').not.toBeNull()
    expect(rowOf('3Y0J'), 'watched by entity').not.toBeNull()
    // The filter still filters.
    expect(rowOf('PLAIN1'), 'the unwatched control').toBeNull()
  })

  it('Hide worked keeps a watched station that has been worked on this band', () => {
    const stations = [
      ...STATIONS.filter((s) => s.call !== 'K1ABC'),
      station('K1ABC', { worked: true, workedBand: true, country: 'United States' }),
    ]
    mount({ hideWorked: true, stations })
    expect(rowOf('K1ABC'), 'watched and worked on 20 m').not.toBeNull()
    expect(tileOf('K1ABC')).not.toBeNull()
    expect(rowOf('WORKED1'), 'the unwatched worked control').toBeNull()
  })

  it('keeps it even when all the backend still says is "confirm it" (#350 would drop that)', () => {
    // The station just worked: worked on this band, and its only need is the confirmation the
    // first contact is waiting on. #350 hides exactly this row — unless it is watched.
    const stations = [
      station('K1ABC', { worked: true, workedBand: true, country: 'United States' }),
      station('JUSTWORKED1', { worked: true, workedBand: true, country: 'United States' }),
    ]
    const alerts = new Map([
      ['K1ABC', [alert('K1ABC', ['Confirm'])]],
      ['JUSTWORKED1', [alert('JUSTWORKED1', ['Confirm'])]],
    ])
    for (const filters of [{ neededOnly: true }, { hideWorked: true }]) {
      mount({ ...filters, stations, alerts })
      expect(rowOf('K1ABC'), `watched, ${JSON.stringify(filters)}`).not.toBeNull()
      expect(rowOf('JUSTWORKED1'), `#350 control, ${JSON.stringify(filters)}`).toBeNull()
      cleanup()
    }
  })
})

describe('the tile follows the list live', () => {
  it('removing the entry in Settings removes the tile — and its place under Needed only', () => {
    render(<WatchlistPanel />)
    mount({ neededOnly: true })
    expect(tileOf('VP8PJ')).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Remove VP8*' }))
    // Needed only was holding the row on the watch match alone, so it leaves the list entirely.
    expect(rowOf('VP8PJ')).toBeNull()
    // The other entries are untouched.
    expect(tileOf('3Y0J')).not.toBeNull()
  })

  it('removing it with the filters off takes the tile off and leaves the row', () => {
    render(<WatchlistPanel />)
    mount()
    expect(tileOf('3Y0J'), 'the tile is there to remove').not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Remove Bouvet' }))
    expect(rowOf('3Y0J'), 'the row stays').not.toBeNull()
    expect(tileOf('3Y0J'), 'the tile goes').toBeNull()
  })

  it('adding an entry in Settings marks the row without a remount', () => {
    render(<WatchlistPanel />)
    mount()
    expect(tileOf('PLAIN1')).toBeNull()
    fireEvent.change(screen.getByRole('textbox', { name: 'Watch value' }), { target: { value: 'PLAIN*' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    expect(tileOf('PLAIN1')).not.toBeNull()
  })
})
