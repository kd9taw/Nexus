// @vitest-environment jsdom
//
// THE CONTEST SCREEN'S EXPORT FILES ARE NAMED FOR THE CONTEST. All six were named for Field Day
// whatever the contest was: an Illinois QSO Party log went to Downloads as `fd-log-<date>.cbr`,
// its club log as `fd-club-log-<date>.cbr`, its summary and dupe sheet as `fd-summary-<date>.txt`
// and `fd-dupesheet-<date>.txt`. Each name now starts with the contest's rules-file id, the id
// the picker and the rules data share, so an ARRL Field Day log is `arrlfd-log-<date>.cbr`.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { ContestView } from './ContestView'
import { saveTextToDownloads } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FdClubStatus, FieldDayStatus } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  // The host's club log under its board: none listed here.
  fdClubLogNewest: vi.fn(async () => null),
  fdClubGivePosition: vi.fn(async () => ({ outcome: 'given' })),
  fdClubRemovePosition: vi.fn(async () => ({ outcome: 'done' })),
  fdClubAllowBack: vi.fn(async () => ({ outcome: 'done' })),
  fdClubImport: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  saveTextToDownloads: vi.fn(async () => '/tmp/x'),
}))

// Hosting, so the club's two exports are on the screen beside the station log's four.
const CLUB: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9ABC Illinois QSO Party',
  hostCall: 'W9ABC',
  score: 0,
  qsos: 0,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board: [],
}

const session = (event: string | undefined): FieldDayStatus =>
  ({
    event,
    composing: [],
    running: false,
    state: 'Listening',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    club: CLUB,
  }) as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

/** Press one export button and read back the file name it wrote to Downloads. */
const savedAs = async (label: string) => {
  vi.mocked(saveTextToDownloads).mockClear()
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: label }))
  })
  await settle()
  return vi.mocked(saveTextToDownloads).mock.calls.map((c) => c[0])
}

beforeEach(() => {
  // The date in each name is today's, in UTC: fixed here so every name is read whole.
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date('2026-10-18T18:30:00Z'))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('the export files are named for the contest', () => {
  it.each([
    ['Export Cabrillo', 'ilqp-log-2026-10-18.cbr'],
    ['Export ADIF', 'ilqp-log-2026-10-18.adi'],
    ['Summary', 'ilqp-summary-2026-10-18.txt'],
    ['Dupe sheet', 'ilqp-dupesheet-2026-10-18.txt'],
    ['Club Cabrillo', 'ilqp-club-log-2026-10-18.cbr'],
    ['Club ADIF', 'ilqp-club-log-2026-10-18.adi'],
  ])('an Illinois QSO Party: %s writes %s', async (label, file) => {
    render(<ContestView fieldDay={session('ilqp')} onSetMode={() => {}} />)
    await settle()
    expect(await savedAs(label)).toEqual([file])
  })

  it.each([
    ['ARRL Field Day', 'arrlfd', 'arrlfd-log-2026-10-18.cbr'],
    // A session with no contest id is ARRL Field Day, as it is everywhere else on the screen.
    ['a session with no contest id', '', 'arrlfd-log-2026-10-18.cbr'],
    ['a session with no contest id', undefined, 'arrlfd-log-2026-10-18.cbr'],
    ['Winter Field Day', 'wfd', 'wfd-log-2026-10-18.cbr'],
    ['CQ WW CW', 'cqww_cw', 'cqww_cw-log-2026-10-18.cbr'],
    // A contest a downloaded rules file added, under an id with characters a file name cannot
    // carry: the slash would make the name a path, and Downloads keeps only its last part.
    ['a rules-file id no file name can carry', 'my contest/2027', 'my-contest-2027-log-2026-10-18.cbr'],
  ])('%s (%j): Export Cabrillo writes %s', async (_what, event, file) => {
    render(<ContestView fieldDay={session(event)} onSetMode={() => {}} />)
    await settle()
    expect(await savedAs('Export Cabrillo')).toEqual([file])
  })
})
