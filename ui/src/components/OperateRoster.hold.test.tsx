// @vitest-environment jsdom
//
// #510 — "rows shift during double-click". The roster re-sorts on every decode batch, and two
// equally-needed stations swap on their live SNR, so a different station could slide under the
// pointer between the two clicks of a double-click. A pointerdown on the list holds the displayed
// ORDER for about a second (the data in each row keeps updating), long enough for the second click
// of a double-click; then the list re-sorts even with the pointer parked on it, so a newly needed
// station still rises to the top (operator decision: a bounded hold, not hover-for-ever).
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { OperateRoster, ROSTER_HOLD_MS } from './OperateRoster'
import type { Station } from '../types'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
}))

const SLOT = 100

function station(call: string, snr: number, over: Partial<Station> = {}): Station {
  return {
    call,
    grid: 'EN52',
    snr,
    lastHeardSlot: SLOT,
    heardCount: 1,
    presence: 'heard' as Station['presence'],
    worked: false,
    ...over,
  }
}

function roster(stations: Station[], onCall = vi.fn()) {
  return (
    <OperateRoster
      stations={stations}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={() => {}}
      onCall={onCall}
    />
  )
}

function dataRows(): HTMLElement[] {
  return screen.getAllByRole('row').filter((r) => r.getAttribute('aria-label') != null)
}

function order(): string[] {
  return dataRows().map((r) => r.getAttribute('aria-label')!.split(',')[0])
}

beforeEach(() => {
  localStorage.clear()
  vi.useFakeTimers()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const grid = () => screen.getByRole('grid')
const after = (ms: number) => act(() => void vi.advanceTimersByTime(ms))

describe('#510 a pointerdown holds the roster order for about a second', () => {
  it('within the window a batch that would swap two rows does not move them; newcomers join at the bottom', () => {
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)]))
    expect(order()).toEqual(['K1AAA', 'K2BBB'])
    fireEvent.pointerDown(dataRows()[0])
    // The next batch: K2BBB is now the louder one, which ranks it first on the SNR tiebreak.
    rerender(roster([station('K1AAA', -15), station('K2BBB', -5), station('K3CCC', 0)]))
    expect(order()).toEqual(['K1AAA', 'K2BBB', 'K3CCC'])
    // The data still updates in place: K2BBB's row reads its new SNR.
    expect(dataRows()[1].textContent).toContain('-5')
  })

  it('after the window it re-sorts, with the pointer still parked over the list', () => {
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)]))
    fireEvent.pointerEnter(grid())
    fireEvent.pointerDown(dataRows()[0])
    rerender(roster([station('K1AAA', -15), station('K2BBB', -5), station('K3CCC', 0)]))
    after(ROSTER_HOLD_MS - 50)
    expect(order(), 'released early').toEqual(['K1AAA', 'K2BBB', 'K3CCC'])
    after(100)
    // No pointerleave: the pointer never left. A newly heard loud station is on top.
    expect(order()).toEqual(['K3CCC', 'K2BBB', 'K1AAA'])
  })

  it('merely hovering holds nothing: the list keeps sorting under a parked pointer', () => {
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)]))
    fireEvent.pointerEnter(grid())
    rerender(roster([station('K1AAA', -15), station('K2BBB', -5)]))
    expect(order()).toEqual(['K2BBB', 'K1AAA'])
  })

  it('the row double-clicked is the row worked, after a batch arrived between the clicks', () => {
    const onCall = vi.fn()
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)], onCall))
    fireEvent.pointerDown(dataRows()[0])
    fireEvent.click(dataRows()[0])
    after(200)
    rerender(roster([station('K1AAA', -15), station('K2BBB', -5)], onCall))
    after(200)
    // The second click lands on the same place on screen: the first row.
    fireEvent.pointerDown(dataRows()[0])
    fireEvent.doubleClick(dataRows()[0])
    expect(onCall).toHaveBeenCalledTimes(1)
    expect(onCall.mock.calls[0][0]).toBe('K1AAA')
  })

  it('the second click restarts the window, so the hold covers the whole double-click', () => {
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)]))
    fireEvent.pointerDown(dataRows()[0])
    after(ROSTER_HOLD_MS - 100)
    fireEvent.pointerDown(dataRows()[0])
    after(ROSTER_HOLD_MS - 100)
    rerender(roster([station('K1AAA', -15), station('K2BBB', -5)]))
    expect(order()).toEqual(['K1AAA', 'K2BBB'])
  })

  it('a column the operator sorts by still re-sorts the list during a hold', () => {
    render(roster([station('K2BBB', -5), station('K1AAA', -15)]))
    expect(order()).toEqual(['K2BBB', 'K1AAA'])
    const call = screen.getByRole('button', { name: /^Call$/ })
    fireEvent.pointerDown(call)
    fireEvent.click(call)
    expect(order()).toEqual(['K1AAA', 'K2BBB'])
  })

  it('a station that drops off the list leaves it, held or not', () => {
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)]))
    fireEvent.pointerDown(dataRows()[0])
    rerender(roster([station('K2BBB', -15)]))
    expect(order()).toEqual(['K2BBB'])
  })
})
