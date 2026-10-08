// @vitest-environment jsdom
//
// The club-sync block on ContestView: the honesty chip (derived state, queue
// in the label), the band board with its 15 s stale marks, the >30 s clock-skew
// warning, and the host-only club export buttons. The whole section is gated on
// `fieldDay.club` — a solo Field Day renders none of it (the control).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, act, within } from '@testing-library/react'
import { ContestView } from './ContestView'
import { getSettings } from '../api'
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
      posid: 'aaaa1111',
      posName: 'CW tent',
      band: '20m',
      mode: 'CW',
      operator: 'KD9TAW',
      qsos: 57,
      rate: 23,
      lastSeenSecs: 2,
    },
    {
      posid: 'bbbb2222',
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
  composing: [
    { key: 'CLASS', raw: '3A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: false,
  state: 'Listening',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  club,
})

afterEach(() => cleanup())

describe('ContestView club sync section', () => {
  it('renders nothing club-related for a solo Field Day (no club block)', () => {
    render(<ContestView fieldDay={fd(undefined)} onSetMode={() => {}} />)
    expect(screen.queryByLabelText('Club sync')).toBeNull()
  })

  it('shows the synced chip, counters, host line and the band board with stale marks', () => {
    render(<ContestView fieldDay={fd(CLUB)} onSetMode={() => {}} />)
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
      <ContestView
        fieldDay={fd({ ...CLUB, syncState: 'behind', queued: 3 })}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Behind — 3 to send')).toBeTruthy()
    cleanup()
    render(
      <ContestView
        fieldDay={fd({ ...CLUB, syncState: 'offline', queued: 7, offlineSinceUnix: 1 })}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Offline — 7 queued here')).toBeTruthy()
    expect(screen.queryByText('Synced')).toBeNull()
  })

  it('warns past 30 s of clock skew and surfaces a host error verbatim', () => {
    render(
      <ContestView
        fieldDay={fd({ ...CLUB, skewSecs: -45, lastError: 'update the host' })}
        onSetMode={() => {}}
      />,
    )
    expect(
      screen.getByText("This PC's clock differs from the host's by 45 s — check this PC's clock"),
    ).toBeTruthy()
    expect(screen.getByText('Host: update the host')).toBeTruthy()
  })

  it('counts a party club by its score and QSOs — a party has no sections to count', () => {
    render(
      <ContestView fieldDay={{ ...fd({ ...CLUB, hosting: true }), event: 'ilqp' }} onSetMode={() => {}} />,
    )
    const club = within(screen.getByLabelText('Club sync'))
    expect(club.getByText('Club: 1234 pts · 312 QSOs')).toBeTruthy()
    expect(club.queryByText(/sections/)).toBeNull()
    // The party keeps only the earliest of a repeat, as Field Day does.
    expect(screen.getByText('Club Cabrillo').getAttribute('title')).toMatch(/earliest contact wins/)
  })

  it('says a club file keeps every repeat for a contest that wants them reported', () => {
    render(
      <ContestView
        fieldDay={{
          ...fd({ ...CLUB, hosting: true }),
          event: 'nyqp',
          dupeRule: {
            byCall: true,
            byBand: true,
            byModeClass: true,
            byFields: ['QTH'],
            bySentFields: ['QTH'],
            modeClassGroups: [],
            logDupes: true,
          },
        }}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Club Cabrillo').getAttribute('title')).toMatch(/every contact stays in/)
    expect(screen.getByText('Club ADIF').getAttribute('title')).toMatch(/every contact stays in/)
  })

  it('offers the club exports only in the host role', () => {
    render(<ContestView fieldDay={fd({ ...CLUB, hosting: true })} onSetMode={() => {}} />)
    expect(screen.getByText('Club Cabrillo')).toBeTruthy()
    expect(screen.getByText('Club ADIF')).toBeTruthy()
  })
})

// ⭐ CLUB SYNC SWITCHED ON FOR A CONTEST IT CANNOT RUN. The club log runs the picked contest's
// own rules and the engine refuses only a contest whose merged log would be wrong whoever built
// it (a serial-number exchange, a template with a transmitter column), so no club block arrives —
// and the screen says why instead of going quiet about a switch the operator turned on. The view
// reads its settings once, on mount; every case below waits the same two microtasks for them, so
// an absence is measured at the moment the presence case finds its text.
describe('club sync switched on for a contest the club log cannot run', () => {
  async function renderWith(settings: Record<string, unknown>) {
    vi.mocked(getSettings).mockResolvedValueOnce({ ...defaultSettings, ...settings } as never)
    render(<ContestView fieldDay={fd(undefined)} onSetMode={() => {}} />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
  }

  it('says it is not syncing, and why, where the club block would be', async () => {
    await renderWith({ fdEvent: 'cqww_cw', fdHostEnable: true })
    expect(screen.getByText('Not syncing')).toBeTruthy()
    expect(
      screen.getByText(/Club sync does not run CQ World-Wide DX Contest \(CW\): its log must say which transmitter/),
    ).toBeTruthy()
  })

  it('says the same for a join address, with a serial-number contest\'s own reason', async () => {
    await renderWith({ fdEvent: 'arrlss_cw', fdHostEnable: false, fdJoinAddr: '192.168.1.10:42073' })
    expect(screen.getByText('Not syncing')).toBeTruthy()
    expect(screen.getByText(/its serial numbers must run in one sequence/)).toBeTruthy()
  })

  it('POSITIVE CONTROL: the Illinois QSO Party, both Field Days and the default say nothing of it', async () => {
    for (const fdEvent of ['ilqp', 'nyqp', 'arrlfd', 'wfd', '']) {
      await renderWith({ fdEvent, fdHostEnable: true })
      expect(screen.queryByText('Not syncing')).toBeNull()
      cleanup()
    }
  })

  it('POSITIVE CONTROL: a refused contest with club sync never switched on says nothing either', async () => {
    await renderWith({ fdEvent: 'cqww_cw', fdHostEnable: false, fdJoinAddr: '' })
    expect(screen.queryByText('Not syncing')).toBeNull()
  })
})
