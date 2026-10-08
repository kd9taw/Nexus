// @vitest-environment jsdom
//
// ⭐ **THE CLUB WARNING AT A QSO PARTY** — the second position to work a station hears about it
// before it logs. A party keys a station on its county as well (a mobile in a new county is a
// new station), so the strip's verdict needs the county being typed and the one this station
// is sending; with them it compares the key the engine is about to build against the club's.
// Without them it said nothing at all, at every party, for every club.
//
// Rendered with the props `PhoneCockpit` passes and a `fieldDay` of the shape the DTO really
// produces (`LogEntry.countyTypeahead.test.tsx`'s). jsdom lays nothing out; not one assertion
// here is about geometry.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  contestZoneHint: vi.fn(() => Promise.resolve(null)),
}))

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

/** An Illinois QSO Party position in McLean county, in a club whose host already holds K9BBB
 *  worked from Kane on 20 m CW by another position — and K9CCC worked here, from Cook. */
const party = (): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    event: 'ilqp',
    qsoCount: 1,
    sections: 0,
    points: 2,
    role: 'in_state',
    log: [{ call: 'K9CCC', band: '20m', mode: 'CW', dkey: ['K9CCC', '20M', 'CW', 'COOK', 'MCLN'] }],
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'MCLN', domain: 'il_counties' },
    ],
    dupeModeGroups: [['CW', 'DIG']],
    dupeRule: {
      byCall: true,
      byBand: true,
      byModeClass: true,
      byFields: ['QTH'],
      bySentFields: ['QTH'],
      modeClassGroups: [['CW', 'DIG']],
      logDupes: false,
    },
    club: {
      syncState: 'synced',
      queued: 0,
      offlineSinceUnix: 0,
      hosting: false,
      event: 'W9XYZ IL QSO Party',
      hostCall: 'W9XYZ',
      score: 2,
      qsos: 2,
      sections: 0,
      skewSecs: 0,
      dupes: [],
      dkeys: [['K9BBB', '20M', 'CW', 'KANE', 'MCLN']],
      board: [],
    },
  }) as unknown as FieldDayStatus

function typeContact(call: string, county: string, fdMode: 'CW' | 'DIG' | 'PH' = 'CW') {
  render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={party()}
      fdMode={fdMode}
    />,
  )
  fireEvent.change(screen.getByPlaceholderText('W1AW'), { target: { value: call } })
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find((n) => n.textContent === 'QTH')
  if (!cap) throw new Error('no received QTH box')
  const qth = cap.closest('label')!.querySelector('input')! as HTMLInputElement
  fireEvent.change(qth, { target: { value: county } })
}

afterEach(() => cleanup())

describe('the club warning at a QSO party', () => {
  it('warns the second position about a station the club worked from that county', () => {
    typeContact('k9bbb', 'KANE')
    expect(screen.getByText(/^Club dupe: another position already worked K9BBB on 20m CW/)).toBeTruthy()
  })

  it('…and on RTTY too, which this party counts as the same mode as CW', () => {
    typeContact('k9bbb', 'kane', 'DIG')
    expect(screen.getByText(/^Club dupe: another position already worked K9BBB/)).toBeTruthy()
  })

  it('stays quiet for the same station from another county — a mobile that moved', () => {
    typeContact('k9bbb', 'DUPG')
    expect(screen.queryByText(/dupe/i)).toBeNull()
  })

  it('stays quiet until the county is typed', () => {
    typeContact('k9bbb', '')
    expect(screen.queryByText(/dupe/i)).toBeNull()
  })

  it("names this position's own repeat as its own, not the club's", () => {
    typeContact('k9ccc', 'COOK')
    expect(screen.getByText(/^Dupe: K9CCC is already in this position's log/)).toBeTruthy()
    expect(screen.queryByText(/Club dupe/)).toBeNull()
  })
})
