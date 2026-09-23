// @vitest-environment jsdom
//
// The Classic station list marks a station on the operator's WATCH LIST with the same WATCH tile
// the Call Roster and Spots use, and a watched station counts toward its "Needed" chip — the
// maintainer's ruling (2026-09-23) that a watch match is a need, the way a new park is.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { StationList } from './StationList'
import { saveWatchlist, type WatchFilter } from '../watchlist'
import type { NeedTag, Station } from '../types'

const station = (call: string, extra: Partial<Station> = {}): Station =>
  ({
    call,
    grid: 'EN52',
    snr: -5,
    lastHeardSlot: 10,
    heardCount: 1,
    presence: 'active',
    worked: false,
    ...extra,
  }) as Station

const VP8: WatchFilter = { id: 'w-vp8', kind: 'call', value: 'VP8*' }
const BOUVET: WatchFilter = { id: 'w-3y', kind: 'dxcc', value: 'Bouvet' }

const STATIONS = [
  station('VP8PJ', { country: 'Falkland Islands' }),
  station('3Y0J', { country: 'Bouvet', grid: null }),
  // The CONTROL: not on the list, nothing needed.
  station('PLAIN1', { country: 'United States' }),
]

function mount() {
  return render(
    <StationList
      stations={STATIONS}
      myGrid="EN52"
      currentSlot={10}
      activePeer={null}
      unreadByPeer={{}}
      needByCall={new Map<string, NeedTag>()}
      band="20m"
      feedMode="FT8"
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

// Both return NULL for "not there" — never undefined. An optional chain yields undefined, and
// `expect(undefined).not.toBeNull()` PASSES: a filtered-out card would read as "kept".
const cardOf = (call: string): HTMLElement | null => screen.queryByTitle(`Double-click to work ${call}`)
const tileOf = (call: string): HTMLElement | null =>
  (cardOf(call)?.querySelector('.need-chip.need-watch') as HTMLElement | null | undefined) ?? null

beforeEach(() => {
  localStorage.clear()
  saveWatchlist([VP8, BOUVET])
})
afterEach(cleanup)

describe('the WATCH tile on the Classic station list', () => {
  it('marks a card the list names — by call and by entity — with the roster’s tile', () => {
    mount()
    for (const call of ['VP8PJ', '3Y0J']) {
      expect(tileOf(call), `${call} carries no WATCH tile`).not.toBeNull()
      expect(tileOf(call)!.textContent).toBe('WATCH')
    }
    expect(tileOf('VP8PJ')!.getAttribute('title')).toBe('On your watch list: VP8*')
    expect(tileOf('3Y0J')!.getAttribute('title')).toBe('On your watch list: Bouvet')
  })

  it('leaves a card the list does not name alone', () => {
    mount()
    expect(cardOf('PLAIN1'), 'the control card is listed').not.toBeNull()
    expect(tileOf('PLAIN1')).toBeNull()
  })

  it('the Needed chip keeps a watched station that has no other need', () => {
    mount()
    fireEvent.click(screen.getByRole('tab', { name: 'Needed' }))
    expect(cardOf('VP8PJ'), 'watched by prefix').not.toBeNull()
    expect(cardOf('3Y0J'), 'watched by entity').not.toBeNull()
    // The filter still filters.
    expect(cardOf('PLAIN1'), 'the unwatched control').toBeNull()
  })
})
