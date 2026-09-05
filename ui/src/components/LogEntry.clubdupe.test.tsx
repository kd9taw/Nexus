// @vitest-environment jsdom
//
// The while-typing FD dupe verdict in the log strip — zero IPC, computed from
// data already on the snapshot: the OWN log (the hard block fdLogManual will
// refuse) and the club-sync block's club-ONLY keys (N3FJP semantics: a warning,
// never a lock). The two must be told apart, and a clean call shows neither.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

const fieldDay = {
  myClass: '3A',
  mySection: 'WI',
  running: true,
  state: '',
  qsoCount: 1,
  sections: 1,
  points: 1,
  // W1AW is in THIS position's log on 20m PH → the hard-block verdict.
  log: [
    { call: 'W1AW', class: '1D', section: 'CT', band: '20m', mode: 'PH', submode: '' },
  ],
  club: {
    syncState: 'synced',
    queued: 0,
    offlineSinceUnix: 0,
    hosting: false,
    event: 'FD',
    hostCall: 'W9ABC',
    score: 0,
    qsos: 2,
    sections: 1,
    skewSecs: 0,
    // Club-only keys: K1ABC worked by ANOTHER position on 20m PH (warn), and
    // one on 40m (a different band — must NOT warn on the 20m dial).
    dupes: [
      ['K1ABC', '20m', 'PH'],
      ['N0XYZ', '40m', 'PH'],
    ],
    board: [],
  },
} as unknown as FieldDayStatus

function typeCall(call: string) {
  render(
    <LogEntry
      snap={snap}
      mode="PH"
      defaultRst="59"
      exchange="terrestrial"
      fieldDay={fieldDay}
      fdMode="PH"
    />,
  )
  fireEvent.change(screen.getByPlaceholderText('W1AW'), { target: { value: call } })
}

afterEach(() => cleanup())

describe('FD while-typing dupe verdicts', () => {
  it('flags an OWN-log dupe as the hard block', () => {
    typeCall('w1aw')
    expect(
      screen.getByText("Dupe: W1AW is already in this position's log on 20m PH"),
    ).toBeTruthy()
    expect(screen.queryByText(/Club dupe/)).toBeNull()
  })

  it('flags a club-only key as a WARNING (logging allowed), never the hard block', () => {
    typeCall('k1abc')
    expect(
      screen.getByText(
        'Club dupe: another position already worked K1ABC on 20m PH — logging is allowed but adds no points',
      ),
    ).toBeTruthy()
    expect(screen.queryByText(/^Dupe:/)).toBeNull()
  })

  it('stays quiet for a clean call, and for a club key on ANOTHER band', () => {
    typeCall('n0xyz') // club-worked on 40m only; the dial is 20m
    expect(screen.queryByText(/dupe/i)).toBeNull()
  })
})
