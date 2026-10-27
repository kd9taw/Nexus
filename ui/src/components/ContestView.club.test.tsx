// @vitest-environment jsdom
//
// The club-sync block on ContestView: the honesty chip (derived state, queue
// in the label), the band board with its 15 s stale marks and clock column, the
// clock line from 2 s and its >30 s warning, and the host-only club export buttons. The whole section is gated on
// `fieldDay.club` — a solo Field Day renders none of it (the control).
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, act, within, fireEvent } from '@testing-library/react'
import { ContestView, FdClubSection } from './ContestView'
import { exportLog, fdClubExport, getSettings, saveTextToDownloads } from '../api'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { ClubLink, FdClubStatus, FieldDayStatus } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "POSITIVE CONTROL: the Illinois QSO Party, both Field…", takes
// 0.26 s and 0.19 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  // The contest screen's Removed list, read on mount: none removed.
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
    const table = within(document.querySelector<HTMLElement>('[data-club-board]')!)
    expect(table.getByText('CW tent')).toBeTruthy()
    const stale = table.getByTitle('Last heard 44 s ago')
    expect(stale.textContent).toContain('SSB tent')
    expect(table.getByTitle('Last heard 44 s ago')).toBeTruthy()
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

  // This PC's clock against the host's (whole seconds, measured over the club link): said
  // from 2 s, in its direction, and past 30 s the warning the block always had. By value at
  // every edge, both ways.
  it('says the clock difference from 2 s, ahead or behind, and warns past 30 s', () => {
    const line = (skewSecs: number) => {
      render(<ContestView fieldDay={fd({ ...CLUB, skewSecs })} onSetMode={() => {}} />)
      const club = within(screen.getByLabelText('Club sync'))
      const said = {
        note: club.queryByText(/^This PC's clock is \d+ s (ahead of|behind) the host's$/)?.textContent ?? null,
        warning: club.queryByRole('alert')?.textContent ?? null,
      }
      cleanup()
      return said
    }
    expect(line(0)).toEqual({ note: null, warning: null })
    expect(line(1)).toEqual({ note: null, warning: null })
    expect(line(-1)).toEqual({ note: null, warning: null })
    expect(line(2)).toEqual({ note: "This PC's clock is 2 s ahead of the host's", warning: null })
    expect(line(-3)).toEqual({ note: "This PC's clock is 3 s behind the host's", warning: null })
    expect(line(30)).toEqual({ note: "This PC's clock is 30 s ahead of the host's", warning: null })
    expect(line(31)).toEqual({
      note: null,
      warning: "This PC's clock differs from the host's by 31 s — check this PC's clock",
    })
    expect(line(-45)).toEqual({
      note: null,
      warning: "This PC's clock differs from the host's by 45 s — check this PC's clock",
    })
  })

  // The host's board lists every position's clock, in the club line's own terms and with
  // its rounding (half away from zero, so a row and that position's own line agree). A
  // position's board has no such column: the clock reached the host, not the other tents.
  it('gives the host\'s board a Clock column: in step, ahead or behind, warned past 30 s, a dash unmeasured', () => {
    const row = (posid: string, posName: string, clockMs?: number | null) => ({
      ...CLUB.board[0],
      posid,
      posName,
      lastSeenSecs: 2,
      ...(clockMs === undefined ? {} : { clockMs }),
    })
    render(
      <ContestView
        fieldDay={fd({
          ...CLUB,
          hosting: true,
          board: [
            row('a', 'A tent', 0),
            row('b', 'B tent', 1_499),
            row('c', 'C tent', -1_500),
            row('d', 'D tent', 3_400),
            row('e', 'E tent', -45_600),
            row('f', 'F tent', null),
            row('g', 'G tent'), // a station older than the field sends no key at all
          ],
        })}
        onSetMode={() => {}}
      />,
    )
    const board = screen.getByLabelText('Club sync').querySelector('[data-club-board]') as HTMLElement
    expect(within(board).getByText('Clock').getAttribute('title')).toMatch(/never changes a clock/)
    const cells = Array.from(board.children) as HTMLElement[]
    const clockOf = (name: string) => cells[cells.findIndex(c => c.textContent === name) + 6]
    expect(clockOf('A tent').textContent).toBe('in step')
    expect(clockOf('B tent').textContent).toBe('in step')
    expect(clockOf('C tent').textContent).toBe('2 s behind')
    expect(clockOf('D tent').textContent).toBe('3 s ahead')
    expect(clockOf('E tent').textContent).toBe('46 s behind')
    expect(clockOf('F tent').textContent).toBe('—')
    expect(clockOf('G tent').textContent).toBe('—')
    expect(clockOf('E tent').style.color).toContain('--status-new-entity')
    expect(clockOf('D tent').style.color).toBe('')
    expect(clockOf('F tent').getAttribute('title')).toMatch(/^Not measured/)
    expect(clockOf('A tent').getAttribute('title')).toBeNull()
    cleanup()
    // A position's board: the same rows, no Clock column.
    render(<ContestView fieldDay={fd({ ...CLUB, board: [row('a', 'A tent', 0)] })} onSetMode={() => {}} />)
    const theirs = screen.getByLabelText('Club sync').querySelector('[data-club-board]') as HTMLElement
    expect(within(theirs).queryByText('Clock')).toBeNull()
    expect(within(theirs).queryByText('in step')).toBeNull()
    expect(theirs.children.length).toBe(12) // six heads and one row of six
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

// Every export on the screen is written to the station's disk: the station log's own four and,
// while hosting, the club's two. So a press that comes through the Remote stream writes none of
// them. jsdom never lays out: `elementFromPoint` does not exist, so each streamed press says what
// is under it.
describe('a press through the Remote stream exports no file', () => {
  let under: Element | null = null
  let stream: StreamInputDispatcher | null = null
  beforeEach(() => {
    under = null
    Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
    stream = new StreamInputDispatcher(window)
    for (const f of [exportLog, fdClubExport, saveTextToDownloads]) vi.mocked(f).mockClear()
  })
  afterEach(() => {
    stream?.dispose()
    stream = null
    delete (document as { elementFromPoint?: unknown }).elementFromPoint
  })
  const settle = async () => {
    await act(async () => {
      for (let i = 0; i < 8; i++) await Promise.resolve()
    })
  }
  // Two ways through the stream: a click on the button, or Enter and then Space on it focused
  // (each presses a button, as a browser does).
  const through = {
    pressed: async (el: HTMLElement) => {
      under = el
      for (const action of ['down', 'up']) {
        await act(async () => {
          stream!.handle({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
            modifiers: 0, pointerType: 'mouse', clicks: 1 })
        })
      }
      await settle()
    },
    keyed: async (el: HTMLElement) => {
      el.focus()
      for (const [key, code] of [['Enter', 'Enter'], [' ', 'Space']]) {
        await act(async () => {
          stream!.handle({ type: 'key', action: 'down', key, code, modifiers: 0, repeat: false })
          stream!.handle({ type: 'key', action: 'up', key, code, modifiers: 0, repeat: false })
        })
      }
      await settle()
    },
  }
  const exportAlert = () => document.querySelector('.fd-export [role="alert"]')?.textContent ?? null
  const host = async () => {
    render(<ContestView fieldDay={fd({ ...CLUB, hosting: true })} onSetMode={() => {}} />)
    await settle()
  }

  it.each([
    ['Club Cabrillo', 'pressed', 'cabrillo', 'cbr'],
    ['Club Cabrillo', 'keyed', 'cabrillo', 'cbr'],
    ['Club ADIF', 'pressed', 'adif', 'adi'],
    ['Club ADIF', 'keyed', 'adif', 'adi'],
  ] as const)('refuses %s %s through the stream, and says why; CONTROL: at the station it exports', async (label, way, format, ext) => {
    await host()
    const button = () => screen.getByRole('button', { name: label })
    await through[way](button())
    expect(fdClubExport).not.toHaveBeenCalled()
    expect(exportAlert()).toBe('Only at the station: a press through Remote exports no club log.')
    expect(saveTextToDownloads, 'no file written').not.toHaveBeenCalled()
    expect((button() as HTMLButtonElement).disabled, 'nothing started').toBe(false)
    await act(async () => { fireEvent.click(button()) })
    await settle()
    expect(fdClubExport).toHaveBeenCalledWith(format)
    expect(saveTextToDownloads).toHaveBeenCalledWith(expect.stringMatching(new RegExp(`^arrlfd-club-log-\\d{4}-\\d{2}-\\d{2}\\.${ext}$`)), '')
    expect(exportAlert(), 'the export clears the refusal').toBeNull()
  })

  // The station log's own four, on a station with no club: Cabrillo and ADIF come from the
  // backend, Summary and Dupe sheet are built here, and all four are written to Downloads.
  it.each([
    ['Export Cabrillo', 'pressed', [['cabrillo']], /^arrlfd-log-\d{4}-\d{2}-\d{2}\.cbr$/],
    ['Export Cabrillo', 'keyed', [['cabrillo']], /^arrlfd-log-\d{4}-\d{2}-\d{2}\.cbr$/],
    ['Export ADIF', 'pressed', [['adif']], /^arrlfd-log-\d{4}-\d{2}-\d{2}\.adi$/],
    ['Export ADIF', 'keyed', [['adif']], /^arrlfd-log-\d{4}-\d{2}-\d{2}\.adi$/],
    ['Summary', 'pressed', [], /^arrlfd-summary-\d{4}-\d{2}-\d{2}\.txt$/],
    ['Summary', 'keyed', [], /^arrlfd-summary-\d{4}-\d{2}-\d{2}\.txt$/],
    ['Dupe sheet', 'pressed', [], /^arrlfd-dupesheet-\d{4}-\d{2}-\d{2}\.txt$/],
    ['Dupe sheet', 'keyed', [], /^arrlfd-dupesheet-\d{4}-\d{2}-\d{2}\.txt$/],
  ] as const)('refuses the station log’s own %s %s through the stream, and says why; CONTROL: at the station it exports', async (label, way, backend, file) => {
    render(<ContestView fieldDay={fd()} onSetMode={() => {}} />)
    await settle()
    const button = () => screen.getByRole('button', { name: label })
    await through[way](button())
    expect(saveTextToDownloads, 'no file written').not.toHaveBeenCalled()
    expect(exportLog).not.toHaveBeenCalled()
    expect(exportAlert()).toBe('Only at the station: a press through Remote exports no log.')
    expect((button() as HTMLButtonElement).disabled, 'nothing started').toBe(false)
    await act(async () => { fireEvent.click(button()) })
    await settle()
    expect(vi.mocked(exportLog).mock.calls).toEqual(backend)
    expect(saveTextToDownloads).toHaveBeenCalledWith(expect.stringMatching(file), expect.any(String))
    expect(exportAlert(), 'the export clears the refusal').toBeNull()
  })

  it('refuses the station log’s own Export Cabrillo pressed through the stream while hosting, in the station’s words', async () => {
    await host()
    const button = () => screen.getByRole('button', { name: 'Export Cabrillo' })
    await through.pressed(button())
    expect(exportLog).not.toHaveBeenCalled()
    expect(saveTextToDownloads, 'no file written').not.toHaveBeenCalled()
    expect(fdClubExport).not.toHaveBeenCalled()
    expect(exportAlert()).toBe('Only at the station: a press through Remote exports no log.')
    await act(async () => { fireEvent.click(button()) })
    await settle()
    expect(vi.mocked(exportLog).mock.calls).toEqual([['cabrillo']])
    expect(fdClubExport, 'the station’s own export reads no club log').not.toHaveBeenCalled()
    expect(exportAlert(), 'the export clears the refusal').toBeNull()
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

  // A rules file the station downloaded can drop a contest, and the engine refuses club sync
  // for it (`ClubRefusal::NoRuleset`): the station's preview of the pick resolves to the ARRL
  // Field Day that mode entry falls back to, and the screen says why by the sponsor's name.
  async function renderPreviewed(fdEvent: string, previewEvent: string) {
    vi.mocked(getSettings).mockResolvedValueOnce({ ...defaultSettings, fdEvent, fdHostEnable: true } as never)
    render(
      <ContestView
        fieldDay={fd(undefined)}
        onSetMode={() => {}}
        fdRuleset={{
          event: previewEvent,
          rulesYear: 2026,
          bannedModes: [],
          spottingAllowed: true,
          clusterAllowed: true,
          enforcement: 'warn',
          role: '',
          exchange: [],
          problem: '',
        } as never}
      />,
    )
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
  }

  it('says a contest the loaded rules do not carry is not syncing, and why', async () => {
    await renderPreviewed('ilqp', 'arrlfd')
    expect(screen.getByText('Not syncing')).toBeTruthy()
    expect(
      screen.getByText(/Club sync does not run Illinois QSO Party: the contest rules this Nexus loaded do not include it/),
    ).toBeTruthy()
  })

  it('POSITIVE CONTROL: the same pick, carried by the loaded rules, says nothing of it', async () => {
    await renderPreviewed('ilqp', 'ilqp')
    expect(screen.queryByText('Not syncing')).toBeNull()
  })
})

// ⭐ WHAT THE JOURNAL RESTORE KEPT OUT. The contest journal is one file per position whatever
// contest it last ran; entering a contest loads only its own rows from this running of it, and
// the screen says how many it kept out and why — a rehearsal's, another contest's — and that
// nothing was deleted.
describe('the contest screen says what the journal restore kept out', () => {
  async function renderWith(status: Partial<FieldDayStatus>) {
    vi.mocked(getSettings).mockResolvedValueOnce({ ...defaultSettings, fdEvent: 'ilqp' } as never)
    render(<ContestView fieldDay={{ ...fd(undefined), ...status }} onSetMode={() => {}} />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
  }

  it('names each reason with its count, and says nothing was deleted', async () => {
    await renderWith({ event: 'ilqp', keptOut: { otherContest: 1, otherRunning: 3 } })
    expect(screen.getByText(/3 contacts in this computer's contest journal were logged in another running of Illinois QSO Party/)).toBeTruthy()
    expect(screen.getByText(/1 contact in this computer's contest journal belongs to another contest/)).toBeTruthy()
    expect(screen.getByText(/Nothing was deleted/)).toBeTruthy()
  })

  it('says only the reason that applies', async () => {
    await renderWith({ event: 'ilqp', keptOut: { otherContest: 0, otherRunning: 1 } })
    expect(screen.getByText(/1 contact in this computer's contest journal was logged in another running/)).toBeTruthy()
    expect(screen.queryByText(/belongs to another contest/)).toBeNull()
  })

  it('POSITIVE CONTROL: a session that kept nothing out says nothing of it', async () => {
    await renderWith({ event: 'ilqp' })
    expect(screen.queryByText(/Nothing was deleted/)).toBeNull()
  })
})

// ⭐ THE HOST'S SIDE OF A REFUSAL, AND OF A FULL BOARD. A position the host turns away is told
// why on its own screen; the host's club block says so too — who, and what they were told — and
// warns before its board outgrows the club line every position is sent, naming the count.
describe('the host\'s club block names who it turned away and how full its board is', () => {
  const HOST: FdClubStatus = { ...CLUB, hosting: true }

  it('names a refused position and quotes what it was told', () => {
    render(
      <FdClubSection
        club={{
          ...HOST,
          refused: [{ posName: 'SSB tent', call: 'W9XYZ', reason: 'this club sends the in-state IL QSO Party exchange (its county)' }],
        }}
      />,
    )
    expect(screen.getByText(/Turned away SSB tent \(W9XYZ\)/)).toBeTruthy()
    expect(screen.getByText(/this club sends the in-state IL QSO Party exchange \(its county\)/)).toBeTruthy()
  })

  it('names an unnamed refused position by its call', () => {
    render(<FdClubSection club={{ ...HOST, refused: [{ posName: '', call: 'K9ABC', reason: 'x' }] }} />)
    expect(screen.getByText(/Turned away K9ABC/)).toBeTruthy()
  })

  it('warns, naming the count, before the next position might not fit on the board', () => {
    render(<FdClubSection club={{ ...HOST, boardFull: { positions: 58, shown: 58 } }} />)
    expect(screen.getByText(/This club has 58 positions, as many as each position's club board has room for/)).toBeTruthy()
  })

  it('says how many the positions see once the board is cut', () => {
    render(<FdClubSection club={{ ...HOST, boardFull: { positions: 70, shown: 59 } }} />)
    expect(screen.getByText(/Each position's club board shows 59 of this club's 70 positions/)).toBeTruthy()
  })

  // ⭐ THE HOST'S LASTING LIST of contacts it kept out of the club's log: who sent each and
  // why, for the rest of the event, not a minute's note that goes when that position rejoins.
  it('lists the contacts it kept out of the club\'s log, and who sent each', () => {
    render(
      <FdClubSection
        club={{
          ...HOST,
          keptOut: {
            total: 2,
            latest: [
              {
                posName: 'SSB tent',
                call: 'W9XYZ',
                reason: "its contact with K1ABC is not in the club's log: its band holds what the club's log does not take.",
              },
              { posName: '', call: 'K9GOT', reason: "a contact it sent is not in the club's log." },
            ],
          },
        }}
      />,
    )
    expect(screen.getByText(/2 contacts the positions sent are not in the club's log/)).toBeTruthy()
    expect(screen.getByText(/From SSB tent \(W9XYZ\): its contact with K1ABC is not in the club's log/)).toBeTruthy()
    expect(screen.getByText(/From K9GOT: a contact it sent is not in the club's log/)).toBeTruthy()
    expect(screen.queryByText(/more, not listed/)).toBeNull()
  })

  it('says how many more it keeps than it lists', () => {
    render(
      <FdClubSection
        club={{ ...HOST, keptOut: { total: 20, latest: [{ posName: 'SSB tent', call: 'W9XYZ', reason: 'x' }] } }}
      />,
    )
    expect(screen.getByText(/20 contacts the positions sent are not in the club's log/)).toBeTruthy()
    expect(screen.getByText(/19 more, not listed here/)).toBeTruthy()
  })

  // ⭐ A LAPTOP SET UP ON ANOTHER COUNTY sends it on the air under the club's call, a busted
  // exchange for every station it works. The host names it, with what it sends and what the club
  // sends, so it can be put right on that laptop; its contacts are still taken. An unnamed
  // position reads as its line on the board does: who is at it, else the placeholder.
  it('names each position sending another county than the club, with both', () => {
    render(
      <FdClubSection
        club={{
          ...HOST,
          otherLocation: [
            { posName: 'CW tent', operator: 'K9OPB', sent: 'KANE', club: 'MCLN' },
            { posName: '', operator: 'K9OPD', sent: 'WILL', club: 'MCLN' },
            { posName: '', operator: '', sent: 'LAKE', club: 'MCLN' },
          ],
        }}
      />,
    )
    expect(screen.getByText(/^CW tent sends KANE, and this club sends MCLN\./)).toBeTruthy()
    expect(screen.getByText(/^K9OPD sends WILL, and this club sends MCLN\./)).toBeTruthy()
    expect(screen.getByText(/^Unnamed position sends LAKE, and this club sends MCLN\./)).toBeTruthy()
    expect(screen.getAllByText(/set that laptop to MCLN so the next ones match/)).toHaveLength(3)
  })

  it('POSITIVE CONTROL: a club with room and no refusals says neither', () => {
    render(<FdClubSection club={HOST} />)
    expect(screen.queryByText(/Turned away/)).toBeNull()
    expect(screen.queryByText(/club board shows|as many as each position/)).toBeNull()
    expect(screen.queryByText(/not in the club's log/)).toBeNull()
    expect(screen.queryByText(/and this club sends/)).toBeNull()
  })
})

// ⭐ WHERE THIS LAPTOP'S CLUB LINK STANDS (2026-10-10). A laptop that typed the host's address said
// nothing about whether it was connected, and with the Field Day mode off there is no club block
// to say it in. So the line is said with the mode off, where the block would be, and beside the
// block only what the block does not already say.
describe("where this laptop's club link stands", () => {
  const LINK: ClubLink = {
    role: 'position',
    addr: '192.168.1.10:42073',
    state: 'unreachable',
    why: 'noAnswer',
    downSinceUnix: 0,
    event: '',
    hostCall: '',
    position: 'CW tent',
    clubCode: 'ABCD-EFGH',
  }
  const HOST_LINK: ClubLink = {
    ...LINK,
    role: 'host',
    addr: '127.0.0.1:42073',
    state: 'joined',
    why: undefined,
    hosting: {
      up: true,
      port: 42073,
      addrs: [{ addr: '192.168.1.10:42073', adapter: 'Wi-Fi' }],
      connected: 1,
    },
  }

  it('says it with the Field Day mode off, where the club block would be', () => {
    render(<ContestView fieldDay={null} clubLink={LINK} onSetMode={() => {}} />)
    const block = screen.getByLabelText('Club sync')
    expect(within(block).getByText('Cannot reach the host at 192.168.1.10:42073. Trying again.')).toBeTruthy()
    expect(
      within(block).getByText(/press Allow Nexus through Windows Firewall in Settings ▸ Contesting on the host\./),
    ).toBeTruthy()
  })

  it('names the event, the host, this laptop’s position and its code once in, with the mode off', () => {
    render(
      <ContestView
        fieldDay={null}
        clubLink={{ ...LINK, state: 'joined', why: undefined, event: 'ILQP rehearsal', hostCall: 'W9AAA' }}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Joined ILQP rehearsal, hosted by W9AAA, as CW tent.')).toBeTruthy()
    expect(screen.getByText('Club code ABCD-EFGH')).toBeTruthy()
  })

  it('beside the club block, says only what the block does not', () => {
    render(
      <ContestView
        fieldDay={fd({ ...CLUB, lastError: 'this club is on the air as W9AAA' })}
        clubLink={{ ...LINK, state: 'turnedAway', why: undefined, told: 'this club is on the air as W9AAA' }}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Host: this club is on the air as W9AAA')).toBeTruthy()
    expect(screen.queryByText(/Turned away by the host/)).toBeNull()
    cleanup()
    render(
      <ContestView
        fieldDay={fd(CLUB)}
        clubLink={{ ...LINK, state: 'lost', why: 'refused', event: 'W9ABC Field Day' }}
        onSetMode={() => {}}
      />,
    )
    expect(screen.getByText('Lost the host at 192.168.1.10:42073. Trying again.')).toBeTruthy()
  })

  it('gives the host the addresses a joining laptop types, and who is in', () => {
    render(<ContestView fieldDay={fd({ ...CLUB, hosting: true })} clubLink={HOST_LINK} onSetMode={() => {}} />)
    expect(screen.getByText(/^Hosting on port 42073\. Positions join at:/)).toBeTruthy()
    expect(screen.getByText('192.168.1.10:42073 (Wi-Fi)')).toBeTruthy()
    expect(screen.getByText('1 other position connected.')).toBeTruthy()
  })

  it('POSITIVE CONTROL: no club link, no line', () => {
    render(<ContestView fieldDay={null} onSetMode={() => {}} />)
    expect(screen.queryByLabelText('Club sync')).toBeNull()
    expect(screen.queryByText(/the host at/)).toBeNull()
  })

  it('says none of it to a Remote observer', () => {
    render(
      <ContestView
        fieldDay={fd(CLUB)}
        clubLink={LINK}
        observation={defaultSettings as never}
        onSetMode={() => {}}
      />,
    )
    expect(screen.queryByText(/the host at/)).toBeNull()
  })
})

// ⭐ THE CLUB FILE'S TOOLTIP SAYS WHAT THE CLUB FILE DOES. Every contest logs its dupes now
// (`DupeRule::log_dupes` is true for all of them), but a Field Day club still writes its
// earliest-wins unique rows, the file 1.x wrote (`fdevent.rs` `export_indices`), so its export
// buttons go on saying so. A party club keeps every contact and says that.
describe('the club export buttons say what the club file keeps', () => {
  const HOST: FdClubStatus = { ...CLUB, hosting: true }
  const titles = () =>
    ['Club Cabrillo', 'Club ADIF'].map((label) => screen.getByText(label).getAttribute('title') ?? '')

  it('a Field Day club keeps the earliest contact, though the contest logs its dupes', () => {
    render(<FdClubSection club={HOST} onExport={() => {}} fieldDay keepsDupes />)
    for (const title of titles()) expect(title).toMatch(/deduped — the earliest contact wins/)
  })

  it('CONTROL — a party club keeps every contact, and says so', () => {
    render(<FdClubSection club={HOST} onExport={() => {}} fieldDay={false} keepsDupes />)
    for (const title of titles()) expect(title).toMatch(/every contact stays in/)
  })
})
