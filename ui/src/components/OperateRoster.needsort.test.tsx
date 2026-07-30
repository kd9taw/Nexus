// @vitest-environment jsdom
//
// "Sort by need has no discernible order — a new band-mode sits three quarters of the way
// down the list" (operator report). The comparator itself is unit-tested in
// features/needs.test.ts; this file pins the ROSTER's rendered row order, because the defect
// lived in which need each row was ranked from, not in the ladder.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import type { NeedAlert, NeedTag, Station } from '../types'

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
    presence: 'heard' as Station['presence'],
    worked: false,
    ...over,
  }
}

function alert(call: string, tags: NeedTag[], priority: number, band = '20m'): NeedAlert {
  return {
    call,
    entity: 'Test',
    band,
    zone: 14,
    tags,
    priority,
    headline: 'Need',
    mode: 'Digital',
    freqMhz: null,
  }
}

function mount(
  stations: Station[],
  needByCall: Map<string, NeedTag>,
  needAlertsByCall?: Map<string, NeedAlert[]>,
) {
  return render(
    <OperateRoster
      stations={stations}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={needByCall}
      needAlertsByCall={needAlertsByCall}
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
}

/** Calls in rendered order. Row aria-labels lead with the callsign; the header has none. */
function order(): string[] {
  return screen
    .getAllByRole('row')
    .map((r) => r.getAttribute('aria-label'))
    .filter((l): l is string => l != null)
    .map((l) => l.split(',')[0])
}

beforeEach(() => localStorage.clear()) // the filter checkboxes now persist — start clean
afterEach(cleanup)

describe('roster "sort by need" ranks by chase importance', () => {
  // One station per rank, all at the same SNR so the tiebreak cannot decide the order.
  const STATIONS = [
    station('PLAIN'),
    station('CONFIRM'),
    station('ENTITY'),
    station('MODE'),
    station('BAND'),
    station('WANTED'),
  ]
  const NEEDS = new Map<string, NeedTag>([
    ['CONFIRM', 'Confirm'],
    ['ENTITY', 'NewEntity'],
    ['MODE', 'NewMode'],
    ['BAND', 'NewBand'],
    ['WANTED', 'Wanted'],
  ])
  const ALERTS = new Map<string, NeedAlert[]>([
    ['CONFIRM', [alert('CONFIRM', ['Confirm'], 10)]],
    ['ENTITY', [alert('ENTITY', ['NewEntity'], 100)]],
    ['MODE', [alert('MODE', ['NewMode'], 30)]],
    ['BAND', [alert('BAND', ['NewBand'], 50)]],
    ['WANTED', [alert('WANTED', ['Wanted', 'NewEntity'], 120)]],
  ])

  it('puts the most valuable station first and the merely workable one last', () => {
    mount(STATIONS, NEEDS, ALERTS)
    expect(order()).toEqual(['WANTED', 'ENTITY', 'BAND', 'MODE', 'CONFIRM', 'PLAIN'])
  })

  it("pins the operator's complaint: the new-mode row is NOT near the bottom", () => {
    mount(STATIONS, NEEDS, ALERTS)
    const rows = order()
    // Above every merely-workable row, below new band and new entity — and specifically not
    // three quarters of the way down a six-row list.
    expect(rows.indexOf('MODE')).toBeLessThan(rows.indexOf('PLAIN'))
    expect(rows.indexOf('MODE')).toBeGreaterThan(rows.indexOf('BAND'))
    expect(rows.indexOf('MODE')).toBeGreaterThan(rows.indexOf('ENTITY'))
  })

  it('keeps the same order for a host that passes no alert map (tag ladder fallback)', () => {
    mount(STATIONS, NEEDS)
    expect(order()).toEqual(['WANTED', 'ENTITY', 'BAND', 'MODE', 'CONFIRM', 'PLAIN'])
  })
})

describe('the multi-band station — the defect behind the report', () => {
  it('ranks a call by its strongest need even when needByCall names its weakest', () => {
    // Exactly what App.tsx produces: needByCall writes one tag per call with no guard, so a
    // station heard on two bands keeps the LAST alert's tag, and the backend orders them
    // priority-descending — so the tag left behind is the WEAKEST. MULTI is a new entity on
    // 20 m that also needs a confirm on 40 m; it used to sort as the confirm.
    const stations = [station('MULTI'), station('BAND'), station('PLAIN')]
    const needByCall = new Map<string, NeedTag>([
      ['MULTI', 'Confirm'], // the weak leftover
      ['BAND', 'NewBand'],
    ])
    const alerts = new Map<string, NeedAlert[]>([
      ['MULTI', [alert('MULTI', ['NewEntity'], 100, '20m'), alert('MULTI', ['Confirm'], 10, '40m')]],
      ['BAND', [alert('BAND', ['NewBand'], 50)]],
    ])
    mount(stations, needByCall, alerts)
    expect(order()).toEqual(['MULTI', 'BAND', 'PLAIN'])
  })

  it('labels the row with the strongest need too, so colour and rank agree', () => {
    const stations = [station('MULTI')]
    const needByCall = new Map<string, NeedTag>([['MULTI', 'Confirm']])
    const alerts = new Map<string, NeedAlert[]>([
      ['MULTI', [alert('MULTI', ['NewEntity'], 100, '20m'), alert('MULTI', ['Confirm'], 10, '40m')]],
    ])
    mount(stations, needByCall, alerts)
    // A row sorted to the top while coloured "confirm" would read as a fresh bug.
    expect(screen.getByRole('row', { name: /MULTI/ }).getAttribute('aria-label')).toContain(
      'needed NewEntity',
    )
  })
})

describe('within one rank', () => {
  it('puts the stronger signal first', () => {
    // The roster's existing tiebreak for every sort key, and a sensible chase order: of two
    // equally-needed stations, work the louder one.
    const stations = [
      station('WEAK', { snr: -20 }),
      station('LOUD', { snr: -3 }),
      station('MID', { snr: -12 }),
    ]
    const needByCall = new Map<string, NeedTag>([
      ['WEAK', 'NewBand'],
      ['LOUD', 'NewBand'],
      ['MID', 'NewBand'],
    ])
    const alerts = new Map<string, NeedAlert[]>([
      ['WEAK', [alert('WEAK', ['NewBand'], 50)]],
      ['LOUD', [alert('LOUD', ['NewBand'], 50)]],
      ['MID', [alert('MID', ['NewBand'], 50)]],
    ])
    mount(stations, needByCall, alerts)
    expect(order()).toEqual(['LOUD', 'MID', 'WEAK'])
  })

  it('still puts the stronger signal first when the column is flipped to ascending', () => {
    // The tiebreak must NOT be multiplied by the sort direction. It was, so every descending
    // sort — the default need view among them — ordered equal rows weakest-first.
    const stations = [
      station('WEAK', { snr: -20 }),
      station('LOUD', { snr: -3 }),
      station('LOWRANK', { snr: -25 }),
    ]
    const needByCall = new Map<string, NeedTag>([
      ['WEAK', 'NewBand'],
      ['LOUD', 'NewBand'],
      ['LOWRANK', 'Confirm'],
    ])
    const alerts = new Map<string, NeedAlert[]>([
      ['WEAK', [alert('WEAK', ['NewBand'], 50)]],
      ['LOUD', [alert('LOUD', ['NewBand'], 50)]],
      ['LOWRANK', [alert('LOWRANK', ['Confirm'], 10)]],
    ])
    mount(stations, needByCall, alerts)
    expect(order()).toEqual(['LOUD', 'WEAK', 'LOWRANK'])
    // Clicking the active Need header flips it to ascending: the ranks reverse, the
    // signal tiebreak does not.
    fireEvent.click(screen.getByTitle('Sort by Need'))
    expect(order()).toEqual(['LOWRANK', 'LOUD', 'WEAK'])
  })
})
