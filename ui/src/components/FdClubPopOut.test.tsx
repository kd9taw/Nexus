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
import { FdClubSection } from './FieldDayView'
import { openPanelWindow, subscribeSnapshot, getSettings } from '../api'
import type { AppSnapshot, FdClubStatus } from '../types'

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
