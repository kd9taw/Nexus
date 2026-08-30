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
import { openPanelWindow, subscribeSnapshot } from '../api'
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

  it('says so when there is no club event, instead of an empty board', async () => {
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(snapWithClub(null))
      return () => {}
    })
    render(<DetachedPanel panel="fdclub" />)
    await settle()
    expect(screen.queryByLabelText('Club sync')).toBeNull()
    // Its OWN message, not the router's "panel isn't available" fallback —
    // which happens to contain the slug and would pass a loose match.
    expect(screen.getByText(/no club event/i)).toBeTruthy()
  })
})
