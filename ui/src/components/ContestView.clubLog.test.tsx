// @vitest-environment jsdom
//
// THE CLUB LOG ON THE HOST'S OWN SCREEN. The operator, benching two laptops: "I can see the number on
// the board on the host computer here but it doesnt show in the host master log." So the host's club
// block lists every position's contacts as the host merged them (`fdClubLogNewest`, the engine's
// `fd_club_log_newest`), newest first, each with the position that logged it, and marks a duplicate
// exactly as this station's own log marks one: the same row, its `dupe` class and its title.
//
// The host's alone: a joined laptop holds the club's counts and keys, never its rows, so its club
// block lists nothing and never asks; and the Remote page's contest view (an observation) neither
// shows the list nor asks for it. That the list IS the club's files is the engine's property test
// (`engine::club_log_tests`); here the screen renders what the engine answered, as it answered it.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, act, within } from '@testing-library/react'
import { ContestView, FdClubSection } from './ContestView'
import { fdClubLogNewest } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FdClubLog, FdClubLogRow, FdClubStatus, FieldDayStatus } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdClubLogNewest: vi.fn(async () => null),
  fdClubGivePosition: vi.fn(async () => ({ outcome: 'given' })),
  fdClubRemovePosition: vi.fn(async () => ({ outcome: 'done' })),
  fdClubAllowBack: vi.fn(async () => ({ outcome: 'done' })),
  fdClubImport: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  saveTextToDownloads: vi.fn(async () => '/tmp/x'),
}))

const readLog = vi.mocked(fdClubLogNewest)

/** 1800Z, Saturday 27 June 2026. */
const T0 = 1_782_583_200

const HOST: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9ABC Field Day',
  hostCall: 'W9ABC',
  score: 14,
  qsos: 5,
  sections: 4,
  skewSecs: 0,
  dupes: [],
  board: [
    { posid: 'aaaa0001', posName: 'HQ', band: '20m', mode: 'DIG', operator: 'W9ABC', qsos: 1, rate: 1, lastSeenSecs: 1, link: 'host' },
    { posid: 'bbbb0002', posName: 'CW tent', band: '15m', mode: 'CW', operator: 'KD9AAA', qsos: 2, rate: 2, lastSeenSecs: 2, link: 'connected' },
    { posid: 'cccc0003', posName: 'SSB tent', band: '20m', mode: 'PH', operator: 'KD9BBB', qsos: 2, rate: 2, lastSeenSecs: 2, link: 'removed' },
    { posid: 'dddd0004', posName: '', band: '40m', mode: 'CW', operator: 'N9CCC', qsos: 0, rate: 0, lastSeenSecs: 3, link: 'connected' },
  ],
}

const row = (r: Partial<FdClubLogRow> & Pick<FdClubLogRow, 'call' | 'whenUnix' | 'posid' | 'seq'>): FdClubLogRow => ({
  class: '',
  section: '',
  band: '20m',
  mode: 'CW',
  submode: '',
  posName: '',
  ...r,
})

/** The engine's answer for the property test's own club: two laptops besides the host's, the SSB
 *  tent's repeat of the CW tent's K1ABC (marked), and the SSB tent since removed. Newest first. */
const LOG: FdClubLog = {
  total: 5,
  rows: [
    row({ call: 'W2DEF', class: '1E', section: 'ENY', band: '15m', whenUnix: T0 + 301, posid: 'bbbb0002', seq: 2, posName: 'CW tent' }),
    row({ call: 'K1ABC', class: '2A', section: 'EMA', band: '40m', whenUnix: T0 + 247, posid: 'cccc0003', seq: 2, posName: 'SSB tent', dupe: true }),
    row({ call: 'N0XYZ', class: '3A', section: 'MN', mode: 'PH', whenUnix: T0 + 190, posid: 'cccc0003', seq: 1, posName: 'SSB tent' }),
    row({ call: 'K1ABC', class: '2A', section: 'EMA', band: '40m', whenUnix: T0 + 125, posid: 'bbbb0002', seq: 1, posName: 'CW tent' }),
    row({ call: 'W1AW', class: '1D', section: 'CT', mode: 'DIG', submode: 'FT8', whenUnix: T0 + 60, posid: 'aaaa0001', seq: 1, posName: 'HQ' }),
  ],
}

const fd = (club: FdClubStatus | undefined, extra: Partial<FieldDayStatus> = {}): FieldDayStatus => ({
  composing: [
    { key: 'CLASS', raw: '3A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: false,
  state: 'Listening',
  qsoCount: 1,
  sections: 1,
  points: 0,
  // This station's own log holds one dupe of its own: the mark the club log must match.
  log: [{ call: 'K9OWN', class: '1D', section: 'WI', band: '20m', mode: 'CW', whenUnix: T0, dupe: true }],
  club,
  ...extra,
})

beforeEach(() => {
  readLog.mockReset()
  readLog.mockResolvedValue(LOG)
})
afterEach(() => cleanup())

/** Let the read the list makes on mount answer. */
async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

const clubLog = () => screen.queryByRole('region', { name: 'Club log' })
const cells = (r: Element, col: string) => r.querySelector(`.fd-col.${col}`)?.textContent ?? ''

describe("the club log on the host's own screen", () => {
  it('lists every position\'s contacts newest first, each with its position, and marks the repeat as the own log marks a dupe', async () => {
    render(<ContestView fieldDay={fd(HOST)} onSetMode={() => {}} />)
    await settle()
    const list = clubLog()
    expect(list).not.toBeNull()
    expect(readLog).toHaveBeenCalled()
    const rows = Array.from(list!.querySelectorAll('.fd-log-row'))
    expect(rows.map((r) => cells(r, 'time'))).toEqual(['18:05', '18:04', '18:03', '18:02', '18:01'])
    expect(rows.map((r) => cells(r, 'call'))).toEqual(['W2DEF', 'K1ABC', 'N0XYZ', 'K1ABC', 'W1AW'])
    expect(rows.map((r) => `${cells(r, 'cls')} ${cells(r, 'sec')}`)).toEqual(['1E ENY', '2A EMA', '3A MN', '2A EMA', '1D CT'])
    expect(rows.map((r) => cells(r, 'band'))).toEqual(['15m', '40m', '20m', '40m', '20m'])
    expect(rows.map((r) => cells(r, 'mode'))).toEqual(['CW', 'CW', 'PH', 'CW', 'DIG'])
    expect(rows.map((r) => cells(r, 'pos'))).toEqual(['CW tent', 'SSB tent', 'SSB tent', 'CW tent', 'HQ'])
    expect(within(list as HTMLElement).getByText('Position')).toBeTruthy()
    // The repeat, and only the repeat, carries the own log's dupe mark: its class and its title.
    const own = document.querySelector('section.fieldday > .fd-log .fd-log-row.dupe')
    expect(own).not.toBeNull()
    expect(list!.contains(own)).toBe(false)
    expect(rows.map((r) => r.classList.contains('dupe'))).toEqual([false, true, false, false, false])
    expect(rows[1].getAttribute('title')).toBe(own!.getAttribute('title'))
    expect(rows[1].getAttribute('title')).toBe('Duplicate — kept in the log and worth no points')
    // How many the club holds: all of them are listed here.
    expect(list!.textContent).toContain('5 contacts')
  })

  it('says how many the club holds when it lists only the newest of them', async () => {
    const newest = Array.from({ length: 100 }, (_, i) =>
      row({ call: `W${i % 10}AA${String.fromCharCode(65 + (i % 26))}`, whenUnix: T0 + 1000 - i, posid: 'bbbb0002', seq: 100 - i, posName: 'CW tent' }),
    )
    readLog.mockResolvedValue({ total: 1234, rows: newest })
    render(<ContestView fieldDay={fd({ ...HOST, qsos: 1234 })} onSetMode={() => {}} />)
    await settle()
    const list = clubLog()!
    expect(list.querySelectorAll('.fd-log-row')).toHaveLength(100)
    expect(list.textContent).toContain('the newest 100 of 1234 contacts')
  })

  it("names a position as its line on the host's board does, and one the board does not know yet by its own name", async () => {
    readLog.mockResolvedValue({
      total: 3,
      rows: [
        // Unnamed: the board names it by who sits at it.
        row({ call: 'K9AAA', whenUnix: T0 + 30, posid: 'dddd0004', seq: 1, posName: '' }),
        // Not on this snapshot's board yet.
        row({ call: 'K9BBB', whenUnix: T0 + 20, posid: 'eeee0005', seq: 1, posName: 'Rover' }),
        row({ call: 'K9CCC', whenUnix: T0 + 10, posid: 'ffff0006', seq: 1, posName: '' }),
      ],
    })
    render(<ContestView fieldDay={fd(HOST)} onSetMode={() => {}} />)
    await settle()
    const rows = Array.from(clubLog()!.querySelectorAll('.fd-log-row'))
    expect(rows.map((r) => cells(r, 'pos'))).toEqual(['N9CCC', 'Rover', 'Unnamed position'])
  })

  it('draws a contest\'s own received columns, as the screen\'s own log does', async () => {
    readLog.mockResolvedValue({
      total: 1,
      rows: [row({ call: 'K9AAA', rcvd: ['599', 'COOK'], whenUnix: T0, posid: 'bbbb0002', seq: 1, posName: 'CW tent' })],
    })
    const party = fd(HOST, {
      event: 'ilqp',
      role: 'in_state',
      log: [],
      receives: [
        { key: 'RST', kind: 'rst', required: true },
        { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
      ],
    } as Partial<FieldDayStatus>)
    render(<ContestView fieldDay={party} onSetMode={() => {}} />)
    await settle()
    const list = clubLog()!
    const r = list.querySelector('.fd-log-row')!
    expect(Array.from(r.querySelectorAll('.fd-col.slot')).map((c) => c.textContent)).toEqual(['599', 'COOK'])
    expect(cells(r, 'pos')).toBe('CW tent')
    expect(list.querySelector('.fd-log-head')!.getAttribute('data-slots')).toBe('2')
  })

  it('says so when the club log holds nothing yet', async () => {
    readLog.mockResolvedValue({ total: 0, rows: [] })
    render(<ContestView fieldDay={fd({ ...HOST, qsos: 0 })} onSetMode={() => {}} />)
    await settle()
    expect(clubLog()!.textContent).toContain('No contacts in the club log yet.')
  })

  it("reads the club log again the moment the club's count moves", async () => {
    const { rerender } = render(<ContestView fieldDay={fd(HOST)} onSetMode={() => {}} />)
    await settle()
    const before = readLog.mock.calls.length
    expect(before).toBeGreaterThan(0)
    rerender(<ContestView fieldDay={fd({ ...HOST, qsos: 6 })} onSetMode={() => {}} />)
    await settle()
    expect(readLog.mock.calls.length).toBeGreaterThan(before)
  })

  it("is the host's alone: a joined laptop's club block lists nothing and never asks", async () => {
    render(<ContestView fieldDay={fd({ ...HOST, hosting: false })} onSetMode={() => {}} />)
    await settle()
    expect(screen.getByLabelText('Club sync')).toBeTruthy()
    expect(clubLog()).toBeNull()
    expect(readLog).not.toHaveBeenCalled()
  })

  it("is not on Remote: the Remote page's contest view neither shows the club log nor asks for it", async () => {
    render(<ContestView fieldDay={fd(HOST)} observation={defaultSettings as never} onSetMode={() => {}} />)
    await settle()
    expect(screen.getByLabelText('Club sync')).toBeTruthy()
    expect(clubLog()).toBeNull()
    expect(readLog).not.toHaveBeenCalled()
  })

  it('is drawn only where a host passes its columns: the club block alone, as other screens draw it, lists nothing', async () => {
    render(<FdClubSection club={HOST} />)
    await settle()
    expect(clubLog()).toBeNull()
    expect(readLog).not.toHaveBeenCalled()
  })
})
