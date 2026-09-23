// @vitest-environment jsdom
//
// #350 on the Classic station list: its "Needed" chip must agree with the Call Roster's
// "Needed only" about a station whose only need is a confirmation.
//
// A fresh contact is unconfirmed, so the backend's Confirm (LoTW) tier tags the station just
// worked — on purpose, and the chip stays. With that station already worked on this band,
// another contact cannot confirm anything the first one will not, so a confirmation and
// nothing else stops counting as "needed" here. A Confirm on a station not worked on this
// band, and any real need beside it, still count.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { StationList } from './StationList'
import type { NeedAlert, NeedTag, Station } from '../types'
import { NEED_TIER } from '../features/needs'

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

const STATIONS = [
  station('JUSTWORKED1', { worked: true, workedBand: true }),
  station('OTHERBAND1', { worked: true, workedBand: false }),
  station('NEVERWORKED1'),
  station('PARKTOO1', { worked: true, workedBand: true }),
  station('PLAIN1'),
]
const ALERTS = new Map<string, NeedAlert[]>([
  ['JUSTWORKED1', [alert('JUSTWORKED1', ['Confirm'])]],
  ['OTHERBAND1', [alert('OTHERBAND1', ['Confirm'])]],
  ['NEVERWORKED1', [alert('NEVERWORKED1', ['Confirm'])]],
  ['PARKTOO1', [alert('PARKTOO1', ['NewPark', 'Confirm', 'Pota'])]],
])

// This project runs vitest WITHOUT auto-cleanup (no setupFiles).
afterEach(cleanup)

function mount() {
  return render(
    <StationList
      stations={STATIONS}
      myGrid="EN52"
      currentSlot={10}
      activePeer={null}
      unreadByPeer={{}}
      needByCall={new Map<string, NeedTag>()}
      needAlertsByCall={ALERTS}
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

/** The callsigns actually on screen. */
function shownCalls(): string[] {
  return screen
    .queryAllByTitle(/^Double-click to work /)
    .map((el) => el.getAttribute('title')!.replace('Double-click to work ', ''))
}

const neededChip = () => screen.getByRole('tab', { name: 'Needed' })

describe('the Classic station list and a need that is only a confirmation (#350)', () => {
  it('Needed drops a station worked on this band whose only need is the confirmation', () => {
    mount()
    fireEvent.click(neededChip())
    expect(shownCalls(), 'the station just worked is still "needed"').not.toContain('JUSTWORKED1')
  })

  it('Needed keeps the confirmation elsewhere, and any real need beside it', () => {
    mount()
    fireEvent.click(neededChip())
    const calls = shownCalls()
    expect(calls, 'worked on another band — a 20 m contact can still confirm').toContain('OTHERBAND1')
    expect(calls, 'never worked').toContain('NEVERWORKED1')
    expect(calls, 'a park not yet worked in this activation').toContain('PARKTOO1')
    // …and the filter still filters: a station with no need at all is not "needed".
    expect(calls).not.toContain('PLAIN1')
  })

  it('without the filter, the station just worked is listed with its LoTW chip', () => {
    mount()
    expect(shownCalls()).toContain('JUSTWORKED1')
    const card = screen.getByTitle('Double-click to work JUSTWORKED1')
    expect(card.querySelector('.need-chip.need-confirm')?.textContent).toBe('LoTW')
  })
})
