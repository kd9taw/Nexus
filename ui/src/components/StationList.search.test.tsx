// @vitest-environment jsdom
//
// The Stations search box (operator request 2026-08-20: "search a large list of the
// stations, and also use like wildcards, like I want all PA* or ON4* stations").
//
// Rendered, not unit-tested through the matcher — `searchQuery.test.ts` already pins the
// matching rules, and what could still break here is the wiring: the box not reaching the
// filter, the filter not composing with the chips, or the count badge going on claiming a
// number the list does not show.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, fireEvent, within, cleanup } from '@testing-library/react'
import { StationList } from './StationList'
import type { NeedTag, Station } from '../types'

const station = (call: string, extra: Partial<Station> = {}): Station =>
  ({
    call,
    grid: 'JO22',
    snr: -5,
    lastHeardSlot: 10,
    heardCount: 1,
    presence: 'active',
    worked: false,
    ...extra,
  }) as Station

const STATIONS = [
  station('PA0XYZ'),
  station('PD5MVH'),
  station('ON4AOI'),
  station('ON7ZZ'),
  station('EA5ISM'),
  station('W1PA'),
  station('4X4FD'),
]

// This project runs vitest WITHOUT auto-cleanup (no setupFiles), so renders otherwise pile
// up in one document and every query finds two of everything.
afterEach(cleanup)

function mount(stations = STATIONS) {
  return render(
    <StationList
      stations={stations}
      myGrid="EN52"
      currentSlot={10}
      activePeer={null}
      unreadByPeer={{}}
      needByCall={new Map<string, NeedTag>()}
      onSelect={() => {}}
      onCall={() => {}}
      conversations={[]}
      onArchive={() => {}}
      bandActive={false}
      bandUnread={0}
      onSelectBand={() => {}}
    />,
  )
}

/** The callsigns actually on screen, in order. */
function shownCalls(): string[] {
  return screen
    .queryAllByTitle(/^Double-click to work /)
    .map((el) => el.getAttribute('title')!.replace('Double-click to work ', ''))
}

function box(): HTMLInputElement {
  return screen.getByLabelText('Search stations') as HTMLInputElement
}

describe('the Stations search box', () => {
  it('shows every station until something is typed', () => {
    mount()
    expect(shownCalls()).toHaveLength(STATIONS.length)
  })

  it('takes a prefix wildcard, and does not drag in calls that merely contain it', () => {
    mount()
    fireEvent.change(box(), { target: { value: 'PA*' } })
    // W1PA contains "PA" and is deliberately absent: a prefix hunt means the prefix.
    expect(shownCalls()).toEqual(['PA0XYZ'])
  })

  it('takes several terms as alternatives — the request, verbatim', () => {
    mount()
    fireEvent.change(box(), { target: { value: 'PA* ON4*' } })
    expect(shownCalls().sort()).toEqual(['ON4AOI', 'PA0XYZ'])
  })

  it('takes a plain term as a substring, so a partial call still finds it', () => {
    mount()
    fireEvent.change(box(), { target: { value: '4fd' } })
    expect(shownCalls()).toEqual(['4X4FD'])
  })

  it('says so when nothing matches, instead of showing an empty panel', () => {
    mount()
    fireEvent.change(box(), { target: { value: 'ZZ9*' } })
    expect(shownCalls()).toHaveLength(0)
    expect(screen.getByText('No stations match.')).toBeTruthy()
  })

  it('composes with the filter chips rather than replacing them', () => {
    mount([
      station('ON4AOI', { presence: 'active' }),
      station('ON7ZZ', { presence: 'stale' }),
      station('PA0XYZ', { presence: 'stale' }),
    ])
    fireEvent.click(screen.getByRole('tab', { name: 'Heard now' }))
    fireEvent.change(box(), { target: { value: 'ON*' } })
    // ON7ZZ matches the search but is not heard now; PA0XYZ is neither.
    expect(shownCalls()).toEqual(['ON4AOI'])
  })

  it('counts what is on screen, with the total beside it', () => {
    mount()
    const head = document.querySelector('.panel-header') as HTMLElement
    expect(within(head).getByText(String(STATIONS.length))).toBeTruthy()
    fireEvent.change(box(), { target: { value: 'ON*' } })
    expect(within(head).getByText('2')).toBeTruthy()
    expect(within(head).getByText(`of ${STATIONS.length}`)).toBeTruthy()
  })

  it('clears on Escape, and on the ✕', () => {
    mount()
    fireEvent.change(box(), { target: { value: 'PA*' } })
    expect(shownCalls()).toHaveLength(1)
    fireEvent.keyDown(box(), { key: 'Escape' })
    expect(box().value).toBe('')
    expect(shownCalls()).toHaveLength(STATIONS.length)

    fireEvent.change(box(), { target: { value: 'PA*' } })
    fireEvent.click(screen.getByTitle('Clear search'))
    expect(box().value).toBe('')
    expect(shownCalls()).toHaveLength(STATIONS.length)
  })

  it('keeps the chips a tablist and the search OUT of it', () => {
    mount()
    const tablist = screen.getByRole('tablist', { name: 'Station filter' })
    expect(within(tablist).queryByLabelText('Search stations')).toBeNull()
    expect(within(tablist).getAllByRole('tab')).toHaveLength(4)
  })
})
