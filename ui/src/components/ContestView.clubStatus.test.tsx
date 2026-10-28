// @vitest-environment jsdom
//
// The host's club board says where each position's link stands, in words, with when the host
// last heard it: Connected, Reconnecting, Lost or Removed, and "This laptop" for its own. The
// host's own contest screen can Remove a position, after a question that names it and says its
// contacts stay, and let a removed one back in. Nowhere else: not on the host's own row, not in
// the pop-out, not on Remote, not on a position's screen, and a press through a stream removes
// nothing. A board without the host's link words (a position's, Remote's) has no Status column.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent, within } from '@testing-library/react'
import { ContestView, FdClubSection } from './ContestView'
import { fdClubAllowBack, fdClubRemovePosition } from '../api'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FdClubBoardRow, FdClubStatus, FieldDayStatus } from '../types'

// THE BUDGET: the house budget for a real-render file; a test that hangs still fails, after 15 s.
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
const remove = vi.mocked(fdClubRemovePosition)
const allowBack = vi.mocked(fdClubAllowBack)

const row = (posid: string, posName: string, extra: Partial<FdClubBoardRow>): FdClubBoardRow => ({
  posid,
  posName,
  band: '20m',
  mode: 'CW',
  operator: 'W9ABC',
  qsos: 10,
  rate: 4,
  lastSeenSecs: 1,
  ...extra,
})
const BOARD: FdClubBoardRow[] = [
  row('aaaa0001', 'Host table', { link: 'host' }),
  row('bbbb0002', 'CW tent', { link: 'connected', heardSecs: 3 }),
  row('cccc0003', 'SSB tent', { link: 'reconnecting', heardSecs: 12 }),
  row('dddd0004', 'GOTA', { link: 'lost', heardSecs: 4 * 60 + 10 }),
  row('eeee0005', 'VHF tent', { link: 'removed', heardSecs: 2 * 3600 + 1 }),
  row('ffff0006', 'Satellite', { link: 'lost' }),
]
const HOST: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9ABC Field Day',
  hostCall: 'W9ABC',
  score: 0,
  qsos: 0,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board: BOARD,
}
const fd = (club: FdClubStatus): FieldDayStatus => ({
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

/** The board's cells in reading order, one array per row, header first. */
function grid(): string[][] {
  const board = document.querySelector('[data-club-board]') as HTMLElement
  const cells = [...board.children].map((c) => c.textContent ?? '')
  const heads = [...board.children].filter((c) => (c as HTMLElement).dataset.head !== undefined).length
  expect(heads, 'the board marks its column heads').toBeGreaterThan(0)
  const rows: string[][] = []
  for (let i = 0; i < cells.length; i += heads) rows.push(cells.slice(i, i + heads))
  return rows
}
/** The cells of the board's row for the position named `name` (keyed by its id). */
function rowOf(name: string): HTMLElement[] {
  const posid = BOARD.find((r) => r.posName === name)?.posid
  const board = document.querySelector('[data-club-board]') as HTMLElement
  return [...board.querySelectorAll<HTMLElement>(`[data-club-row="${posid}"]`)]
}

afterEach(() => {
  cleanup()
  remove.mockClear()
  allowBack.mockClear()
})

describe("the host's board says where each position's link stands", () => {
  it('in words, with when the host last heard it, and its own line as this laptop', () => {
    render(<FdClubSection club={HOST} />)
    const rows = grid()
    expect(rows[0].slice(0, 2)).toEqual(['Position', 'Status'])
    expect(rows.slice(1).map((r) => [r[0], r[1]])).toEqual([
      ['Host table', 'This laptop'],
      ['CW tent', 'Connected (heard 3 s ago)'],
      ['SSB tent', 'Reconnecting (heard 12 s ago)'],
      ['GOTA', 'Lost (heard 4 min ago)'],
      ['VHF tent', 'Removed (heard 2 h ago)'],
      ['Satellite', 'Lost (not heard since this Nexus started)'],
    ])
  })

  it('dims a lost and a removed position, and no other, whatever its presence age says', () => {
    render(<FdClubSection club={{ ...HOST, board: BOARD.map((r) => ({ ...r, lastSeenSecs: 99 })) }} />)
    const dimmed = (name: string) => rowOf(name).some((c) => c.style.opacity !== '')
    expect(['Host table', 'CW tent', 'SSB tent', 'GOTA', 'VHF tent', 'Satellite'].map(dimmed)).toEqual([
      false,
      false,
      false,
      true,
      true,
      true,
    ])
  })

  it('in the pop-out too, without a button', () => {
    render(<FdClubSection club={HOST} detached />)
    expect(grid()[2][1]).toBe('Connected (heard 3 s ago)')
    expect(screen.queryByRole('button', { name: 'Remove' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Allow back' })).toBeNull()
  })

  it("CONTROL: a board without the host's words (a position's, Remote's copy) has no Status column", () => {
    const plain = BOARD.map(({ link: _link, heardSecs: _heard, ...r }) => r)
    render(<FdClubSection club={{ ...HOST, board: plain }} />)
    expect(grid()[0]).not.toContain('Status')
    expect(screen.queryByText(/heard 3 s ago/)).toBeNull()
  })
})

describe('a position’s board names the position its host removed', () => {
  // A position's copy of the board carries no link words but this one, which its host's line
  // sends for a position its operator removed: the laptops learn it with the next club line, where
  // they used to learn only from the line going quiet, a ⚠ 15 s and a heartbeat later.
  const POSITION: FdClubStatus = {
    ...HOST,
    hosting: false,
    board: [
      row('aaaa0001', 'Host table', {}),
      row('bbbb0002', 'CW tent', {}),
      row('eeee0005', 'VHF tent', { link: 'removed' }),
    ],
  }

  it('after its name, its line dimmed and kept, with no Status column and no ⚠', () => {
    render(<FdClubSection club={POSITION} />)
    const rows = grid()
    expect(rows[0]).not.toContain('Status')
    expect(rows.slice(1).map((r) => r[0])).toEqual(['Host table', 'CW tent', 'VHF tent · Removed'])
    expect(rowOf('VHF tent').every((c) => c.style.opacity === '0.45')).toBe(true)
    expect(rowOf('VHF tent').map((c) => c.textContent)).toContain('10')
    expect(rowOf('CW tent').some((c) => c.style.opacity !== '')).toBe(false)
    expect(rowOf('VHF tent').some((c) => (c.textContent ?? '').includes('⚠'))).toBe(false)
  })

  it('leaves it out of the band × mode grid, as the host’s own grid does', () => {
    render(<FdClubSection club={POSITION} />)
    const names = [...document.querySelectorAll('.club-grid-name')].map((n) => n.textContent)
    expect(names).toContain('CW tent')
    expect(names).not.toContain('VHF tent')
  })
})

describe('the host removes a position from its own contest screen, and lets it back', () => {
  const host = () => (
    <FdClubSection club={HOST} onRemovePosition={remove} onAllowBack={allowBack} />
  )

  it('puts Remove on every other position, Allow back on a removed one, and nothing on its own', () => {
    render(host())
    const buttons = (name: string) =>
      rowOf(name)
        .flatMap((c) => [...c.querySelectorAll('button')])
        .map((b) => b.textContent)
    expect(buttons('Host table')).toEqual([])
    expect(buttons('CW tent')).toEqual(['Remove'])
    expect(buttons('SSB tent')).toEqual(['Remove'])
    expect(buttons('GOTA')).toEqual(['Remove'])
    expect(buttons('Satellite')).toEqual(['Remove'])
    expect(buttons('VHF tent')).toEqual(['Allow back'])
  })

  it('asks first, naming the position and saying its contacts stay, and sends its id only on yes', async () => {
    render(host())
    const button = rowOf('SSB tent').flatMap((c) => [...c.querySelectorAll('button')])[0]
    expect(button.getAttribute('title')).toMatch(/contacts it already sent stay/)
    fireEvent.click(button)
    expect(remove, 'a press asks first').not.toHaveBeenCalled()
    const asked = screen.getByRole('group', { name: 'Remove this position?' })
    expect(within(asked).getByText('Remove SSB tent from the club event?')).toBeTruthy()
    expect(asked.textContent).toMatch(/turned away when it tries again, until you press Allow back/)
    expect(asked.textContent).toMatch(/The contacts it already sent stay in the club's log, its score and the club's files/)
    await act(async () => {
      fireEvent.click(within(asked).getByRole('button', { name: 'Remove it' }))
    })
    expect(remove).toHaveBeenCalledTimes(1)
    expect(remove).toHaveBeenCalledWith('cccc0003')
    expect(screen.queryByRole('group', { name: 'Remove this position?' })).toBeNull()
    expect(screen.getByText(/^SSB tent is removed:/).getAttribute('role')).toBe('status')
  })

  it('sends nothing on Cancel', () => {
    render(host())
    fireEvent.click(rowOf('CW tent').flatMap((c) => [...c.querySelectorAll('button')])[0])
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByRole('group', { name: 'Remove this position?' })).toBeNull()
    expect(remove).not.toHaveBeenCalled()
  })

  it('lets a removed position back with one press, and says when it rejoins', async () => {
    render(host())
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Allow back' }))
    })
    expect(allowBack).toHaveBeenCalledWith('eeee0005')
    expect(remove).not.toHaveBeenCalled()
    expect(screen.getByText(/^VHF tent can rejoin:/).getAttribute('role')).toBe('status')
  })

  it('says nothing changed, and why, when the host refuses or the press does not reach it', async () => {
    for (const [answer, said] of [
      [{ outcome: 'refused', refusal: 'stale' }, /no longer on this board/],
      [{ outcome: 'refused', refusal: 'notHosting' }, /not hosting a club event/],
      [{ outcome: 'refused', refusal: 'own' }, /this laptop's own position/],
    ] as const) {
      remove.mockResolvedValueOnce(answer)
      render(host())
      fireEvent.click(rowOf('GOTA').flatMap((c) => [...c.querySelectorAll('button')])[0])
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Remove it' }))
      })
      expect(screen.getAllByRole('alert').map((a) => a.textContent).join(' ')).toMatch(said)
      cleanup()
    }
    allowBack.mockRejectedValueOnce('unavailable')
    render(host())
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Allow back' }))
    })
    expect(screen.getAllByRole('alert').map((a) => a.textContent).join(' ')).toMatch(/could not change the position/)
  })

  it('is on the host’s own contest screen, and not on Remote or on a position’s screen', () => {
    render(<ContestView fieldDay={fd(HOST)} onSetMode={() => {}} />)
    expect(screen.getAllByRole('button', { name: 'Remove' })).toHaveLength(4)
    expect(screen.getAllByRole('button', { name: 'Allow back' })).toHaveLength(1)
    cleanup()
    render(<ContestView fieldDay={fd(HOST)} observation={defaultSettings as never} onSetMode={() => {}} />)
    expect(screen.queryByRole('button', { name: 'Remove' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Allow back' })).toBeNull()
    cleanup()
    render(<ContestView fieldDay={fd({ ...HOST, hosting: false })} onSetMode={() => {}} />)
    expect(screen.queryByRole('button', { name: 'Remove' })).toBeNull()
    cleanup()
    // A read-only block shows no button, whatever it is handed.
    render(<FdClubSection club={HOST} onRemovePosition={remove} onAllowBack={allowBack} readOnly />)
    expect(screen.queryByRole('button', { name: 'Remove' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Allow back' })).toBeNull()
    expect(grid()[2][1], 'the words stay').toBe('Connected (heard 3 s ago)')
  })

  describe('through a stream', () => {
    let under: Element | null = null
    let stream: StreamInputDispatcher | null = null
    beforeEach(() => {
      under = null
      Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
      stream = new StreamInputDispatcher(window)
    })
    afterEach(() => {
      stream?.dispose()
      stream = null
      delete (document as { elementFromPoint?: unknown }).elementFromPoint
    })
    const press = async (el: Element) => {
      under = el
      const pointer = (action: 'down' | 'up') =>
        ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
          modifiers: 0, pointerType: 'mouse', clicks: 1 }) as const
      act(() => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
      await act(async () => { await Promise.resolve() })
    }
    const removeOf = (name: string) => rowOf(name).flatMap((c) => [...c.querySelectorAll('button')])[0]

    it('removes nothing and lets nothing back, at either step; CONTROL: the same presses at the host do', async () => {
      render(host())
      // The question, pressed through the stream…
      await press(removeOf('CW tent'))
      expect(screen.queryByRole('group', { name: 'Remove this position?' }), 'nothing asked').toBeNull()
      expect(screen.getAllByRole('alert').map((a) => a.textContent).join(' ')).toMatch(/Only at the host/)
      // …the answer, to a question asked at the host…
      fireEvent.click(removeOf('CW tent'))
      await press(screen.getByRole('button', { name: 'Remove it' }))
      // …and Allow back.
      await press(screen.getByRole('button', { name: 'Allow back' }))
      expect(remove).not.toHaveBeenCalled()
      expect(allowBack).not.toHaveBeenCalled()
      // CONTROL: answered and pressed at the host.
      await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Remove it' })) })
      await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Allow back' })) })
      expect(remove).toHaveBeenCalledWith('bbbb0002')
      expect(allowBack).toHaveBeenCalledWith('eeee0005')
    })
  })
})
