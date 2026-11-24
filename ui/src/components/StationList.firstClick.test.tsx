// @vitest-environment jsdom
//
// The Classic layout's Stations cards: a double-click works the card its FIRST click landed on.
//
// The operator's report (1.25.0) was on the Call Roster, where the grid moved a row between the two
// clicks of a double-click and the station that slid under the pointer was worked. The cards have the
// same gesture and the same exposure: the selected card grows a path line (and the card it left
// shrinks), and the list re-sorts as stations are heard, so another card can slide under the pointer
// between the clicks. A double-click is one gesture aimed at the card its first click landed on; that
// card is the one worked, the second click does not re-select, and if that station has left the list
// in between, the double-click does nothing.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { StationList } from './StationList'
import type { NeedTag, Station } from '../types'

afterEach(cleanup)

const station = (call: string, extra: Partial<Station> = {}): Station =>
  ({ call, grid: 'JN57', snr: -15, lastHeardSlot: 10, heardCount: 1, presence: 'active', worked: false, ...extra }) as Station

function mount(stations: Station[]) {
  const onCall = vi.fn(), onSelect = vi.fn()
  const el = (list: Station[]) => (
    <StationList stations={list} myGrid="EN52" currentSlot={10} activePeer={null} unreadByPeer={{}}
      needByCall={new Map<string, NeedTag>()} onSelect={onSelect} onCall={onCall} conversations={[]}
      onArchive={() => {}} bandActive={false} bandUnread={0} onSelectBand={() => {}} />
  )
  const view = render(el(stations))
  const open = (call: string) => {
    const b = [...view.container.querySelectorAll<HTMLElement>('.station-open')].find((x) => x.querySelector('.station-call')?.textContent === call)
    expect(b, `no card for ${call}`).toBeDefined()
    return b!
  }
  return { onCall, onSelect, open, rerender: (list: Station[]) => view.rerender(el(list)) }
}

describe('a Stations card double-click works the card its first click landed on', () => {
  it('the cards moved between the clicks: the first card is worked, not the one under the second click', () => {
    const { onCall, onSelect, open } = mount([station('KB2DJJ', { freqHz: 1200 }), station('KM7CAB', { freqHz: 900, tier: 'FT8' })])
    fireEvent.click(open('KM7CAB'), { detail: 1 })
    fireEvent.click(open('KB2DJJ'), { detail: 2 })
    fireEvent.doubleClick(open('KB2DJJ'), { detail: 2 })
    expect(onSelect.mock.calls.map((c) => c[0])).toEqual(['KM7CAB'])
    expect(onCall).toHaveBeenCalledTimes(1)
    expect(onCall).toHaveBeenCalledWith('KM7CAB', 'JN57', undefined, undefined, 900, 'FT8')
  })

  it('the first station left the list in between: nothing is worked', () => {
    const { onCall, open, rerender } = mount([station('KB2DJJ'), station('KM7CAB')])
    fireEvent.click(open('KM7CAB'), { detail: 1 })
    rerender([station('KB2DJJ')])
    fireEvent.click(open('KB2DJJ'), { detail: 2 })
    fireEvent.doubleClick(open('KB2DJJ'), { detail: 2 })
    expect(onCall).not.toHaveBeenCalled()
  })

  it('a still double-click works its own card, and the next gesture starts afresh', () => {
    const { onCall, open } = mount([station('KB2DJJ'), station('KM7CAB')])
    fireEvent.click(open('KM7CAB'), { detail: 1 })
    fireEvent.click(open('KM7CAB'), { detail: 2 })
    fireEvent.doubleClick(open('KM7CAB'), { detail: 2 })
    fireEvent.click(open('KB2DJJ'), { detail: 1 })
    fireEvent.click(open('KB2DJJ'), { detail: 2 })
    fireEvent.doubleClick(open('KB2DJJ'), { detail: 2 })
    expect(onCall.mock.calls.map((c) => c[0])).toEqual(['KM7CAB', 'KB2DJJ'])
  })
})
