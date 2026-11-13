// @vitest-environment jsdom
//
// #510 — "rows shift during double-click". The roster re-sorts on every decode batch, and two
// equally-needed stations swap on their live SNR, so a different station could slide under the
// pointer between the two clicks of a double-click. While the pointer is over the list the
// displayed ORDER holds still (the data in each row keeps updating); it re-sorts on leave.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
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

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('#510 the roster holds its order under the pointer', () => {
  it('a decode batch that would swap two rows does not move them while the pointer is over the list', () => {
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)]))
    expect(order()).toEqual(['K1AAA', 'K2BBB'])
    fireEvent.pointerEnter(screen.getByRole('grid'))
    // The next batch: K2BBB is now the louder one, which ranks it first on the SNR tiebreak.
    rerender(roster([station('K1AAA', -15), station('K2BBB', -5), station('K3CCC', 0)]))
    // Held: the two rows stay where they were; the newcomer joins at the bottom.
    expect(order()).toEqual(['K1AAA', 'K2BBB', 'K3CCC'])
    // The data still updates in place: K2BBB's row reads its new SNR.
    expect(dataRows()[1].textContent).toContain('-5')
    fireEvent.pointerLeave(screen.getByRole('grid'))
    expect(order()).toEqual(['K3CCC', 'K2BBB', 'K1AAA'])
  })

  it('the row double-clicked is the row worked, after a batch arrived between the clicks', () => {
    const onCall = vi.fn()
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)], onCall))
    fireEvent.pointerEnter(screen.getByRole('grid'))
    fireEvent.click(dataRows()[0])
    rerender(roster([station('K1AAA', -15), station('K2BBB', -5)], onCall))
    // The second click lands on the same place on screen: the first row.
    fireEvent.doubleClick(dataRows()[0])
    expect(onCall).toHaveBeenCalledTimes(1)
    expect(onCall.mock.calls[0][0]).toBe('K1AAA')
  })

  it('a column the operator sorts by still re-sorts the list under the pointer', () => {
    render(roster([station('K2BBB', -5), station('K1AAA', -15)]))
    expect(order()).toEqual(['K2BBB', 'K1AAA'])
    fireEvent.pointerEnter(screen.getByRole('grid'))
    fireEvent.click(screen.getByRole('button', { name: /^Call$/ }))
    expect(order()).toEqual(['K1AAA', 'K2BBB'])
  })

  it('a station that drops off the list leaves it, held or not', () => {
    const { rerender } = render(roster([station('K1AAA', -5), station('K2BBB', -15)]))
    fireEvent.pointerEnter(screen.getByRole('grid'))
    rerender(roster([station('K2BBB', -15)]))
    expect(order()).toEqual(['K2BBB'])
  })
})
