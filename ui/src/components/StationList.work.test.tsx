// @vitest-environment jsdom
//
// The Stations pane's Work button must hand over WHERE the station was heard.
//
// Field report #183 (1.7.0–1.9.0): clicking Work in the Stations window selected the station
// and started working it, but the RX marker stayed where it was — the operator expected it on
// the frequency of the station's last decode. The same complaint was reported and fixed once
// already for the Roster layout's table (OperateRoster.test.tsx, "works a station where they
// were heard"); the CLASSIC layout's Stations cards are a different component, went through a
// LOSSY adapter, and were missed.
//
// The contract pinned here is the cockpit's SHARED positional signature — the same one Band
// Activity and the roster table use — so the card feeds the work handler directly and there is
// no adapter left in the middle to drop an argument. That is the actual defect: not a missing
// feature, a re-typed callback that silently threw the extra arguments away.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { StationList } from './StationList'
import type { NeedTag, Station } from '../types'

// `../api` is deliberately NOT mocked, exactly as StationList.search.test.tsx leaves it. A
// card's only engine calls are the QRZ opener (click-only, never fired here) and the entity
// centroid fetch behind `useEntityCentroids`, which already catches its own failure. A PARTIAL
// mock is actively harmful: stubbing just `openQrzPage` leaves `getDxccEntityLocations`
// undefined, and the resulting throw inside that effect surfaces as an unhandled React error
// attributed to whatever unrelated file shares the worker (it landed on useScale.popout).

// This project runs vitest WITHOUT auto-cleanup (no setupFiles), so renders otherwise pile up
// in one document and every query finds two of everything.
afterEach(cleanup)

const station = (call: string, extra: Partial<Station> = {}): Station =>
  ({
    call,
    grid: 'JN57',
    snr: -15,
    lastHeardSlot: 10,
    heardCount: 1,
    presence: 'active',
    worked: false,
    ...extra,
  }) as Station

function mount(stations: Station[], onCall: (call: string, ...rest: unknown[]) => void) {
  return render(
    <StationList
      stations={stations}
      myGrid="EN52"
      currentSlot={10}
      activePeer={null}
      unreadByPeer={{}}
      needByCall={new Map<string, NeedTag>()}
      onSelect={() => {}}
      onCall={onCall}
      conversations={[]}
      onArchive={() => {}}
      bandActive={false}
      bandUnread={0}
      onSelectBand={() => {}}
    />,
  )
}

describe('Stations pane works a station where they were heard', () => {
  it('passes the last-heard offset and grid when the Work button is clicked', () => {
    const onCall = vi.fn()
    mount([station('DL3MIB', { freqHz: 983, tier: 'FT8' })], onCall)

    fireEvent.click(screen.getByTitle('Work DL3MIB'))

    // The cockpit's shared order: (call, grid, message, snr, freq, tier). message/snr stay
    // undefined — there is no clicked decode line here, only a roster row.
    expect(onCall).toHaveBeenCalledWith('DL3MIB', 'JN57', undefined, undefined, 983, 'FT8')
  })

  it('passes the offset on the card double-click too — the same gesture', () => {
    const onCall = vi.fn()
    mount([station('DL3MIB', { freqHz: 983, tier: 'FT8' })], onCall)

    fireEvent.doubleClick(screen.getByText('DL3MIB'))

    expect(onCall).toHaveBeenCalledWith('DL3MIB', 'JN57', undefined, undefined, 983, 'FT8')
  })

  it('passes undefined freq for a station heard only by free-text attribution', () => {
    // No decode of its own means no offset to move to — the engine ignores a missing one and
    // leaves the marker alone, which is the honest outcome rather than a guess.
    const onCall = vi.fn()
    mount([station('K2DEF', { tier: 'FT8' })], onCall)

    fireEvent.click(screen.getByTitle('Work K2DEF'))

    expect(onCall).toHaveBeenCalledWith('K2DEF', 'JN57', undefined, undefined, undefined, 'FT8')
  })

  it('passes undefined grid for a station whose grid was never decoded', () => {
    // The grid is what lets the contact log with one even when the DX never sends theirs;
    // a station with no grid must hand over nothing rather than an empty string.
    const onCall = vi.fn()
    mount([station('K2DEF', { grid: null, freqHz: 1200, tier: 'FT8' })], onCall)

    fireEvent.click(screen.getByTitle('Work K2DEF'))

    expect(onCall).toHaveBeenCalledWith('K2DEF', undefined, undefined, undefined, 1200, 'FT8')
  })

  it('still carries the protocol tier, so a Tempo contact opens the chat instead', () => {
    // The tier is what routes a TempoFast/TempoDeep contact to the conversation rather than
    // the FT8 call sequence. Widening this callback must not cost that.
    const onCall = vi.fn()
    mount([station('PD5MVH', { freqHz: 1500, tier: 'TempoFast' })], onCall)

    fireEvent.click(screen.getByTitle('Work PD5MVH'))

    expect(onCall).toHaveBeenCalledWith('PD5MVH', 'JN57', undefined, undefined, 1500, 'TempoFast')
  })
})
