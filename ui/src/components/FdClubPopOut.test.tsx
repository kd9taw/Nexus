// @vitest-environment jsdom
// THE CLUB BAND BOARD ON ITS OWN MONITOR. The operator asked for the board to
// "be popped out so we can put it on a seperate monitor or area of the monitor
// to keep up" — a multi-station club watches who is on which band continuously,
// and it cannot live behind the dashboard's other five blocks.
//
// The existing `fieldday` pop-out is the SCOREBOARD (operator, tiles, sections
// board); this is a second, different surface, so it needs its own slug. What
// this file covers is the seam: the button opens the right window, the window
// renders the board, and the torn-off copy does not offer the two things that
// only work docked. The registry relations (natural footprint vs the window
// Rust builds, and index.html's pre-paint mirror of it) are machine-checked for
// every openable slug by popout-natural.test.ts and index-preseed.test.ts.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { DetachedPanel } from '../DetachedPanel'
import { FdClubSection, FdBandOccupancy } from './FieldDayView'
import { openPanelWindow, subscribeSnapshot, getSettings } from '../api'
import type { AppSnapshot, FdClubStatus } from '../types'
import { BAND_COLOR } from '../bandColors'

vi.mock('../api', () => ({
  subscribeSnapshot: vi.fn(() => () => {}),
  openPanelWindow: vi.fn(() => Promise.resolve()),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([])),
  getSettings: vi.fn(() => Promise.resolve(null)),
  selectPeer: vi.fn(() => Promise.resolve(null)),
}))

const CLUB: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'CLUB FD 2026',
  hostCall: 'W9ABC',
  score: 420,
  qsos: 210,
  sections: 31,
  skewSecs: 0,
  dupes: [],
  board: [
    {
      posid: 'aaaa1111',
      posName: 'CW tent',
      band: '20m',
      mode: 'CW',
      operator: 'W9AAA',
      qsos: 120,
      rate: 44,
      lastSeenSecs: 2,
    },
    {
      posid: 'bbbb2222',
      posName: 'GOTA tent',
      band: '40m',
      mode: 'PH',
      operator: 'W9BBB',
      qsos: 90,
      rate: 21,
      lastSeenSecs: 3,
    },
  ],
} as unknown as FdClubStatus

const snapWithClub = (club: FdClubStatus | null) =>
  ({ link: { tier: 'FT8' }, radio: {}, fieldDay: club ? { club } : null }) as unknown as AppSnapshot

const mockedOpen = vi.mocked(openPanelWindow)
const mockedSubscribe = vi.mocked(subscribeSnapshot)

beforeEach(() => {
  mockedOpen.mockClear()
  mockedSubscribe.mockImplementation(() => () => {})
})
afterEach(() => cleanup())

async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe('the club band board pops out', () => {
  it('the docked board offers a pop-out, and it opens the club-board window', () => {
    render(<FdClubSection club={CLUB} onExport={() => {}} busy={false} />)
    fireEvent.click(screen.getByTitle(/own window/i))
    // The SCOREBOARD pop-out already owns `fieldday`; a second surface in that
    // window would just replace the one the operator already tore off.
    expect(mockedOpen).toHaveBeenCalledTimes(1)
    expect(mockedOpen).toHaveBeenCalledWith('fdclub')
  })

  it('the torn-off window shows who is on what band', async () => {
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(CLUB))
      return () => {}
    })
    const { container } = render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect((container.firstElementChild as HTMLElement).className).toBe('app detached')
    const board = screen.getByLabelText('Club sync')
    expect(board.textContent).toContain('CW tent')
    expect(board.textContent).toContain('20m')
    expect(board.textContent).toContain('GOTA tent')
    expect(board.textContent).toContain('40m')
  })

  it('does not offer to pop itself out again', async () => {
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(CLUB))
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.queryByTitle(/own window/i)).toBeNull()
    expect(mockedOpen).not.toHaveBeenCalled()
  })

  it('does not offer the club exports, which need the window that can report them', async () => {
    // The export buttons write a file and then TOAST the path. A detached
    // window hosts no toasts (the documented pattern in DetachedPanel), so a
    // failed export there would be silent — and the dashboard, one click away,
    // does it properly.
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(CLUB))
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.queryByText('Club Cabrillo')).toBeNull()
    expect(screen.queryByText('Club ADIF')).toBeNull()
    // POSITIVE CONTROL: the docked board, hosting, does offer them — the
    // absence above is the detached arm, not a broken `hosting` read.
    cleanup()
    render(<FdClubSection club={CLUB} onExport={() => {}} busy={false} />)
    expect(screen.getByText('Club Cabrillo')).toBeTruthy()
  })

  it('with sync off it names the exact route to turn it on, instead of an empty box', async () => {
    // THE FINDABILITY DEFECT. The rail button opens this window whenever Field Day
    // is on, so most operators will meet it BEFORE club sync exists — an empty
    // board there is the same dead end that hid the feature in the first place.
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(null))
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.queryByLabelText('Club sync')).toBeNull()
    // Its OWN message, not the router's "panel isn't available" fallback —
    // which happens to contain the slug and would pass a loose match.
    expect(screen.getByText(/club sync is off/i)).toBeTruthy()
    // The route, in the words the operator will read off the Settings tabs.
    expect(screen.getByText(/Settings ▸ Contesting ▸ Field Day Club Sync/i)).toBeTruthy()
    expect(screen.getByText(/Host a club event/i)).toBeTruthy()
  })

  it('with sync on but nobody else on the air, it says it is waiting — not that sync is off', async () => {
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub({ ...CLUB, board: [] } as FdClubStatus))
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.getByLabelText('Club sync')).toBeTruthy()
    expect(screen.getByText(/no positions heard yet/i)).toBeTruthy()
    // Sending an operator to Settings when the setting is already right is the
    // worse of the two wrong messages: it reads as "this is broken".
    expect(screen.queryByText(/Settings ▸ Contesting/i)).toBeNull()
  })

  it('before the first snapshot it says connecting — never that sync is off', async () => {
    // No snapshot yet is not the same claim as no club event, and the window
    // opens before the first 300 ms tick every single time.
    mockedSubscribe.mockImplementation(() => () => {})
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.getByText(/connecting/i)).toBeTruthy()
    expect(screen.queryByText(/club sync is off/i)).toBeNull()
  })

  it('the torn-off board is set in bigger type than the docked one — it is watched, not read', async () => {
    // The one surface where bigger is correct: a band board across the room is
    // glanced at from the operating position, and this window exists to be
    // parked on a second monitor and left alone.
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(CLUB))
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    const torn = Number(
      /(\d+)/.exec(
        (screen.getByLabelText('Club sync').querySelector('[data-club-board]') as HTMLElement).style
          .fontSize,
      )![1],
    )
    cleanup()
    render(<FdClubSection club={CLUB} onExport={() => {}} busy={false} />)
    const docked = Number(
      /(\d+)/.exec(
        (screen.getByLabelText('Club sync').querySelector('[data-club-board]') as HTMLElement).style
          .fontSize,
      )![1],
    )
    expect(torn).toBeGreaterThan(docked)
  })

  it('⭐ does not claim sync is off when the host merely stepped out of Field Day', async () => {
    // The window exists for one job: a host watches it on a second monitor to see which
    // bands are busy. The whole `fieldDay` block is built only inside the engine's Field Day
    // mode, so it vanishes the moment the operator clicks into any other section — one click
    // on the rail. Reading that absence as "sync is off" turned a live board into the words
    // "Club sync is off" plus instructions to switch on hosting that was already on, while
    // the host was still collecting contacts.
    vi.mocked(getSettings).mockResolvedValue({ fdHostEnable: true } as never)
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb({ ...snapWithClub(null), fieldDay: null })
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.queryByText(/Club sync is off/i)).toBeNull()
    expect(screen.getByText(/nothing has stopped/i)).toBeTruthy()
  })

  it('POSITIVE CONTROL: a station that really has not configured sync is still told how', async () => {
    vi.mocked(getSettings).mockResolvedValue({ fdHostEnable: false, fdJoinAddr: '' } as never)
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb({ ...snapWithClub(null), fieldDay: null })
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.getByText(/Club sync is off/i)).toBeTruthy()
  })
})
describe('the band board answers "where can I move?"', () => {
  // ⚠️ THE INVERSION IS THE FEATURE. The club board lists POSITIONS with a band column, which
  // answers "what is the CW tent doing?". The operator asked four separate times for the other
  // question — which band is free — and could not find it, because it did not exist: to get it
  // from a position list you invert the whole thing in your head, at 2 AM, while somebody
  // waits. Here an EMPTY ROW is the answer.
  const twoUp: FdClubStatus = {
    ...CLUB,
    board: [
      { posid: 'a1', posName: 'CW tent', band: '40m', mode: 'CW', operator: 'KD9TAW', qsos: 12, rate: 20, lastSeenSecs: 2 },
      { posid: 'b2', posName: 'SSB tent', band: '20m', mode: 'PH', operator: 'W1ABC', qsos: 30, rate: 25, lastSeenSecs: 2 },
    ],
  }

  it('shows every band, marks the busy ones, and calls the rest free', async () => {
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(twoUp))
      return () => {}
    })
    render(<DetachedPanel panel="fieldday" />)
    await settle()
    const board = document.querySelector('[data-band-occupancy]') as HTMLElement
    expect(board, 'the band board is in the pop-out beside the operator box').toBeTruthy()
    const text = board.textContent ?? ''
    // The two that are busy say who, and in which mode.
    expect(text).toContain('CW tent')
    expect(text).toContain('SSB tent')
    // 15m is on nobody, and that is the whole point of the view.
    expect(text).toContain('free')
    // Every worked band is listed even when nobody is on it — the operator scans for a gap.
    for (const b of ['160m', '80m', '40m', '20m', '15m', '10m']) {
      expect(text, `${b} must be listed`).toContain(b)
    }
    // The barred bands are NOT offered as somewhere to go. Checked against the band CELLS,
    // not the concatenated text — "160m" contains "60m", and a substring assertion here
    // fails on a perfectly correct board.
    const bandCells = [...board.querySelectorAll('span')]
      .map((e) => (e.textContent ?? '').trim())
      .filter((tx) => /^\d+(\.\d+)?(m|cm)$/.test(tx))
    for (const b of ['60m', '30m', '17m', '12m']) {
      expect(bandCells, `${b} is barred at Field Day and must not read as free`).not.toContain(b)
    }
    expect(bandCells, 'the standard Field Day bands are all listed').toContain('160m')
  })

  it('shows BOTH stations when two land on the same band', async () => {
    // At a multi-transmitter club this is either a mistake about to cost a contact or a
    // deliberate CW/phone split. Hiding one would make the collision invisible.
    const clash: FdClubStatus = {
      ...CLUB,
      board: [
        { posid: 'a1', posName: 'CW tent', band: '20m', mode: 'CW', operator: 'KD9TAW', qsos: 12, rate: 20, lastSeenSecs: 2 },
        { posid: 'b2', posName: 'GOTA', band: '20m', mode: 'PH', operator: 'W1ABC', qsos: 3, rate: 4, lastSeenSecs: 2 },
      ],
    }
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(clash))
      return () => {}
    })
    render(<DetachedPanel panel="fieldday" />)
    await settle()
    const text = (document.querySelector('[data-band-occupancy]') as HTMLElement).textContent ?? ''
    expect(text).toContain('CW tent')
    expect(text).toContain('GOTA')
  })
})

describe('the band board colours itself, and shouts when two positions collide', () => {
  // THE OPERATOR'S ASK: "how can we colorize the Who is There and when we have overlap when
  // someone is trying to operate the same mode on the same band and how will we alert on that?"
  //
  // ⚠️ WHAT THE WIRE ACTUALLY CARRIES decides the comparison. `fd_position_report` sends the FD
  // SCORING CLASS — "CW" | "PH" | "DIG" — never a submode, so FT8 and RTTY reach this board as
  // the same three letters and cannot be told apart here even in principle. That is also the
  // right granularity: ARRL FD scores one QSO per band per mode CLASS and permits one signal per
  // band/mode, so two digital positions on 20m are splitting one pool of contacts (and Class A
  // is over its signal limit), while 20m CW + 20m phone is ordinary multi-transmitter operating.
  const row = (
    posid: string,
    posName: string,
    band: string,
    mode: string,
    lastSeenSecs = 2,
  ) => ({ posid, posName, band, mode, operator: 'W9XYZ', qsos: 5, rate: 9, lastSeenSecs })
  const boardOf = (...rows: ReturnType<typeof row>[]) =>
    ({ ...CLUB, board: rows }) as unknown as FdClubStatus

  /** The band cell for `band`, and the "who" cell beside it. */
  function cells(container: HTMLElement, band: string) {
    const board = container.querySelector('[data-band-occupancy]') as HTMLElement
    const bandCell = [...board.querySelectorAll('span')].find(
      (e) => (e.textContent ?? '').trim() === band,
    ) as HTMLElement
    expect(bandCell, `${band} must have a row`).toBeTruthy()
    return { band: bandCell, who: bandCell.nextElementSibling as HTMLElement }
  }
  /** jsdom normalises a hex colour to rgb(); compare through a probe, not by string. */
  function asRendered(color: string) {
    const probe = document.createElement('span')
    probe.style.color = color
    return probe.style.color
  }

  it('a busy band reads in its own map colour, and a free one stays dim', () => {
    const { container } = render(
      <FdBandOccupancy club={boardOf(row('a1', 'CW tent', '20m', 'CW'))} />,
    )
    const busy = cells(container, '20m')
    expect(busy.band.style.color, '20m must look like 20m everywhere').toBe(
      asRendered(BAND_COLOR['20m']),
    )
    // A different band is a different colour — proves it is the palette, not one hard-coded ink.
    cleanup()
    const other = render(<FdBandOccupancy club={boardOf(row('a1', 'CW tent', '15m', 'CW'))} />)
    expect(cells(other.container, '15m').band.style.color).toBe(asRendered(BAND_COLOR['15m']))
    // The gap the eye is hunting for is NOT painted — a free band stays dim and uncoloured.
    const free = cells(other.container, '10m')
    expect(free.who.textContent).toContain('free')
    expect(free.band.style.color).toBe('')
    expect(Number(free.band.style.opacity)).toBeLessThan(1)
  })

  it('⭐ two positions on the same band in the same mode class are flagged', () => {
    const { container } = render(
      <FdBandOccupancy
        club={boardOf(row('a1', 'FT8 tent', '20m', 'DIG'), row('b2', 'Trailer', '20m', 'DIG'))}
      />,
    )
    const c = cells(container, '20m')
    expect(c.who.getAttribute('data-band-clash')).toBe('20m')
    // Colour is never the only signal: the marker is a WORD.
    expect(c.who.textContent).toContain('CLASH')
    expect(c.band.style.color).toContain('--alert-critical')
    // And the row says WHY, for a reader who cannot see any of it.
    expect(c.who.textContent).toMatch(/split the run/i)
  })

  it('⭐ FT8 and RTTY on one band are the SAME class — the wire sends DIG for both', () => {
    // A peer on an older build reports the raw on-air mode instead of the scoring code. It is
    // still one FD mode class, one dupe pool, one permitted signal — flag it.
    const { container } = render(
      <FdBandOccupancy
        club={boardOf(row('a1', 'FT8 tent', '40m', 'FT8'), row('b2', 'RTTY tent', '40m', 'RTTY'))}
      />,
    )
    expect(cells(container, '40m').who.getAttribute('data-band-clash')).toBe('40m')
  })

  it('⭐ CW and phone on the same band are NOT flagged — that is legitimate 2A operating', () => {
    // THE LIAR TEST. Flagging this makes every multi-transmitter club ignore the alert, and
    // then the real collision at 2 AM goes unread.
    const { container } = render(
      <FdBandOccupancy
        club={boardOf(row('a1', 'CW tent', '20m', 'CW'), row('b2', 'SSB tent', '20m', 'PH'))}
      />,
    )
    const c = cells(container, '20m')
    expect(c.who.getAttribute('data-band-clash')).toBeNull()
    expect(c.who.textContent).not.toContain('CLASH')
    // Both stations still show — the operator sees the split, it is just not an alarm.
    expect(c.who.textContent).toContain('CW tent')
    expect(c.who.textContent).toContain('SSB tent')
  })

  it('a free band is neither coloured nor flagged', () => {
    const { container } = render(
      <FdBandOccupancy club={boardOf(row('a1', 'CW tent', '20m', 'CW'))} />,
    )
    const c = cells(container, '15m')
    expect(c.who.getAttribute('data-band-clash')).toBeNull()
    expect(c.who.textContent).toContain('free')
  })

  it('⭐ a stale position never raises a collision — we have not heard it, it may have moved', () => {
    // NO FALSE ALARMS. Past the 15 s dead-man the reading is a memory, not a fact: the tent may
    // have QSY'd and the report simply has not arrived. An alarm that cries wolf at 2 AM is one
    // nobody reads at 3.
    const { container } = render(
      <FdBandOccupancy
        club={boardOf(row('a1', 'CW tent', '20m', 'CW'), row('b2', 'Trailer', '20m', 'CW', 44))}
      />,
    )
    const c = cells(container, '20m')
    expect(c.who.getAttribute('data-band-clash')).toBeNull()
    // It is not hidden either — it still lists, still stale-marked.
    expect(c.who.textContent).toContain('Trailer')
    expect(c.who.textContent).toContain('⚠')
    // POSITIVE CONTROL: the identical pair, both live, DOES flag — the silence above is the
    // staleness rule, not a broken detector.
    cleanup()
    const live = render(
      <FdBandOccupancy
        club={boardOf(row('a1', 'CW tent', '20m', 'CW'), row('b2', 'Trailer', '20m', 'CW'))}
      />,
    )
    expect(cells(live.container, '20m').who.getAttribute('data-band-clash')).toBe('20m')
  })

  it('⭐ does not cry wolf at three laptops opened before the rigs are plugged in', async () => {
    // ⚠️ THE FALSE ALARM THAT WOULD HAVE KILLED THE FEATURE. Presence carries DEFAULTS — a
    // freshly launched Nexus reports band "20m" and mode DIG before a rig is connected, and
    // the sync pump sends that first report the moment it joins. So the ordinary Friday
    // afternoon at a club (three laptops up, nothing hooked up yet, plus a logging-only seat)
    // put every position on 20m/DIG simultaneously and would have painted ⛔ CLASH across the
    // board before anyone keyed a transmitter. An alarm that fires at setup is ignored by
    // 2 AM, which is exactly when a real collision costs contacts.
    const idle: FdClubStatus = {
      ...CLUB,
      board: [
        { posid: 'a1', posName: 'tent 1', band: '20m', mode: 'DIG', operator: 'KD9TAW', qsos: 0, rate: 0, lastSeenSecs: 1 },
        { posid: 'b2', posName: 'tent 2', band: '20m', mode: 'DIG', operator: 'W1ABC', qsos: 0, rate: 0, lastSeenSecs: 1 },
        { posid: 'c3', posName: 'logger', band: '20m', mode: 'DIG', operator: 'W2DEF', qsos: 0, rate: 0, lastSeenSecs: 1 },
      ],
    }
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(idle))
      return () => {}
    })
    render(<DetachedPanel panel="fieldday" />)
    await settle()
    expect(
      document.querySelector('[data-band-clash]'),
      'nobody has worked anybody — this is three idle laptops, not a collision',
    ).toBeNull()
  })

  it('POSITIVE CONTROL: the same two positions DO clash once they are working', async () => {
    // Otherwise the guard above would pass against a build where the alert never fires at all.
    const working: FdClubStatus = {
      ...CLUB,
      board: [
        { posid: 'a1', posName: 'tent 1', band: '20m', mode: 'DIG', operator: 'KD9TAW', qsos: 4, rate: 6, lastSeenSecs: 1 },
        { posid: 'b2', posName: 'tent 2', band: '20m', mode: 'DIG', operator: 'W1ABC', qsos: 9, rate: 8, lastSeenSecs: 1 },
      ],
    }
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(working))
      return () => {}
    })
    render(<DetachedPanel panel="fieldday" />)
    await settle()
    expect(document.querySelector('[data-band-clash]')?.getAttribute('data-band-clash')).toBe('20m')
  })
})
