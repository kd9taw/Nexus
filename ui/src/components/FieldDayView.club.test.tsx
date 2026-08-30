// @vitest-environment jsdom
//
// The club-sync block on FieldDayView: the honesty chip (derived state, queue
// in the label), the band board with its 15 s stale marks, the >30 s clock-skew
// warning, and the host-only club export buttons. The whole section is gated on
// `fieldDay.club` — a solo Field Day renders none of it (the control).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { FieldDayView } from './FieldDayView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FdClubStatus, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  saveTextToDownloads: vi.fn(async () => '/tmp/x'),
}))

const CLUB: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: false,
  event: 'W9ABC Field Day',
  hostCall: 'W9ABC',
  score: 1234,
  qsos: 312,
  sections: 41,
  skewSecs: 2,
  dupes: [],
  board: [
    {
      posName: 'CW tent',
      band: '20m',
      mode: 'CW',
      operator: 'KD9TAW',
      qsos: 57,
      rate: 23,
      lastSeenSecs: 2,
    },
    {
      posName: 'SSB tent',
      band: '40m',
      mode: 'PH',
      operator: 'W1ABC',
      qsos: 31,
      rate: 9,
      lastSeenSecs: 44, // past the 15 s dead-man → stale-marked
    },
  ],
}

const fd = (club?: FdClubStatus): FieldDayStatus => ({
  myClass: '3A',
  mySection: 'WI',
  running: false,
  state: 'Listening',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  club,
})

afterEach(() => cleanup())

describe('FieldDayView club sync section', () => {
  it('renders nothing club-related for a solo Field Day (no club block)', () => {
    render(<FieldDayView fieldDay={fd(undefined)} onSetMode={() => {}} />)
    expect(screen.queryByLabelText('Club sync')).toBeNull()
  })

  it('shows the synced chip, counters, host line and the band board with stale marks', () => {
    render(<FieldDayView fieldDay={fd(CLUB)} onSetMode={() => {}} />)
    expect(screen.getByText('Synced')).toBeTruthy()
    expect(screen.getByText('Club: 1234 pts · 312 QSOs · 41 sections')).toBeTruthy()
    expect(screen.getByText('W9ABC Field Day · host W9ABC')).toBeTruthy()
    // Both positions on the board; only the silent one is stale-marked, and
    // the mark carries WHEN it was last heard (never silently stale).
    expect(screen.getByText('CW tent')).toBeTruthy()
    const stale = screen.getByTitle('Last heard 44 s ago')
    expect(stale.textContent).toContain('SSB tent')
    expect(screen.getByTitle('Last heard 44 s ago')).toBeTruthy()
    expect(screen.queryByTitle('Last heard 2 s ago')).toBeNull()
    // Non-host: no club export buttons.
    expect(screen.queryByText('Club Cabrillo')).toBeNull()
    // Skew of 2 s: no clock warning.
    expect(screen.queryByText(/check this PC's clock/)).toBeNull()
  })

  it('keeps the queue in the label — behind and offline can never read as synced', () => {
    render(
      <FieldDayView
        fieldDay={fd({ ...CLUB, syncState: 'behind', queued: 3 })}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Behind — 3 to send')).toBeTruthy()
    cleanup()
    render(
      <FieldDayView
        fieldDay={fd({ ...CLUB, syncState: 'offline', queued: 7, offlineSinceUnix: 1 })}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Offline — 7 queued here')).toBeTruthy()
    expect(screen.queryByText('Synced')).toBeNull()
  })

  it('warns past 30 s of clock skew and surfaces a host error verbatim', () => {
    render(
      <FieldDayView
        fieldDay={fd({ ...CLUB, skewSecs: -45, lastError: 'update the host' })}
        onSetMode={() => {}}
      />,
    )
    expect(
      screen.getByText("This PC's clock differs from the host's by 45 s — check this PC's clock"),
    ).toBeTruthy()
    expect(screen.getByText('Host: update the host')).toBeTruthy()
  })

  it('offers the club exports only in the host role', () => {
    render(<FieldDayView fieldDay={fd({ ...CLUB, hosting: true })} onSetMode={() => {}} />)
    expect(screen.getByText('Club Cabrillo')).toBeTruthy()
    expect(screen.getByText('Club ADIF')).toBeTruthy()
  })
})
