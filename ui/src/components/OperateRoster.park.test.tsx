// @vitest-environment jsdom
//
// #357 — THE CALL ROSTER'S PARK COLUMN. A POTA hunter asked for a column of its own for the park,
// sortable so the parks stay at the top of the list while hunting.
//
// The roster knows a station is out at a park two ways, and the column reads both:
//   · a POTA/SOTA spot names the activation (the need alert's `park`, the reference itself);
//   · the station's own frame says so (`CQ POTA`, `CQ SOTA`: the CQ modifier), with no reference.
// A spot's reference wins over the bare word. Everything else is a blank, and blanks sort LAST in
// both directions: the direction orders the parks among themselves, it never sinks them.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
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
    country: 'United States',
    ...over,
  }
}

const parkAlert = (call: string, reference: string, program = 'POTA', tags: NeedTag[] = ['Pota']): NeedAlert => ({
  call,
  entity: 'United States',
  band: '20m',
  zone: 4,
  tags,
  priority: NEED_TIER[tags[0]] ?? 0,
  headline: '',
  mode: 'Digital',
  freqMhz: 14.074,
  park: { program, reference },
})

const STATIONS = [
  station('W1LOUD', { snr: 5 }), // no park, loudest: the default sort would put it high
  station('K2PARK', { snr: -15 }), // spotted at US-1234
  station('N3CQ', { snr: -12, cqDir: 'POTA' }), // CQ POTA, never spotted
  station('W4SOTA', { snr: -18 }), // spotted on a summit
  station('KA5ABC', { snr: 0 }), // no park
]
const ALERTS = new Map<string, NeedAlert[]>([
  ['K2PARK', [parkAlert('K2PARK', 'US-1234')]],
  ['W4SOTA', [parkAlert('W4SOTA', 'W4C/CM-001', 'SOTA', ['Sota'])]],
])

function mount() {
  return render(
    <OperateRoster
      stations={STATIONS}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={new Map()}
      needAlertsByCall={ALERTS}
      band="20m"
      feedMode="FT8"
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
}

const rowOf = (call: string): HTMLElement | null =>
  (screen.queryByText(call)?.closest('[role="row"]') as HTMLElement | null | undefined) ?? null
const parkCell = (call: string) => rowOf(call)?.querySelector('.or-park')?.textContent ?? null
/** The data rows' calls, top to bottom. */
const order = () =>
  [...document.querySelectorAll('.or-row:not(.or-header) .or-call')].map((c) => c.firstChild?.textContent)

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('#357 the Call Roster Park column', () => {
  it('has a Park heading the operator can sort by', () => {
    mount()
    expect(screen.getByRole('button', { name: /^Park/ })).not.toBeNull()
  })

  it('shows the spotted reference, the bare program for a CQ POTA, and a dash for the rest', () => {
    mount()
    expect(parkCell('K2PARK')).toBe('US-1234')
    expect(parkCell('W4SOTA')).toBe('W4C/CM-001')
    expect(parkCell('N3CQ')).toBe('POTA')
    expect(parkCell('W1LOUD')).toBe('—')
    expect(parkCell('KA5ABC')).toBe('—')
  })

  it('sorting by Park puts every park on top, and they STAY on top when the direction flips', () => {
    mount()
    const head = screen.getByRole('button', { name: /^Park/ })
    fireEvent.click(head)
    const parks = new Set(['K2PARK', 'N3CQ', 'W4SOTA'])
    const first = order()
    expect(new Set(first.slice(0, 3)), `parks not on top: ${first.join(' ')}`).toEqual(parks)
    fireEvent.click(head) // the other direction
    const second = order()
    expect(new Set(second.slice(0, 3)), `parks sank on the flip: ${second.join(' ')}`).toEqual(parks)
    // …and the direction did reorder the parks among themselves.
    expect(second.slice(0, 3)).toEqual([...first.slice(0, 3)].reverse())
  })

  it('the control: the default Need sort does not group the parks', () => {
    mount()
    expect(order()[0]).toBe('W1LOUD')
  })
})
