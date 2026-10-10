// @vitest-environment jsdom
//
// The host's Give button on its club block: a laptop turned away because another laptop holds
// its position gets one, on the host's own contest screen. A press asks first, showing the club
// code of the laptop the position would go to, and only the answer that the codes match sends
// that entry's handle. Nowhere else: not on an entry turned away for another reason, not on
// Remote (an observed contest screen), not in the pop-out, not on a position's screen, and a
// press that comes through a stream gives nothing, at either step. Each answer is said on the
// block. Each laptop's club code is on its own club line, and each turned-away laptop's on its
// entry, on the host's own screen alone.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { ContestView, FdClubSection } from './ContestView'
import { fdClubGivePosition } from '../api'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FdClubStatus, FieldDayStatus } from '../types'

// THE BUDGET: the house budget for a real-render file (each case here renders the contest
// screen once); a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdClubGivePosition: vi.fn(async () => ({ outcome: 'given' })),
  openPanelWindow: vi.fn(async () => {}),
  saveTextToDownloads: vi.fn(async () => '/tmp/x'),
}))
const give = vi.mocked(fdClubGivePosition)

const HELD = 'this Nexus joined as a club position that another laptop holds at this event'
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
  board: [],
  refused: [
    { posName: 'CW tent', call: 'W9ABC', reason: HELD, handle: 42, clubCode: '7KQ2-M9XD' },
    { posName: 'SSB tent', call: 'W9ABC', reason: 'this club is running the IL QSO Party', clubCode: '0B4N-VW8T' },
  ],
  clubCode: '3HV8-ZQ1P',
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
const LABEL = 'Give this laptop its position'
const YES = 'The codes match: give it the position'
/** Press Give on an entry, then answer that the codes match. */
async function giveAndConfirm(button: HTMLElement) {
  fireEvent.click(button)
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: YES })) })
}

afterEach(() => {
  cleanup()
  give.mockClear()
})

describe('the host gives a held position from its own contest screen', () => {
  it('puts the button on the entry held out of its position alone, and sends that entry’s handle', async () => {
    render(<ContestView fieldDay={fd(HOST)} onSetMode={() => {}} />)
    const buttons = screen.getAllByRole('button', { name: LABEL })
    expect(buttons).toHaveLength(1)
    expect(buttons[0].closest('[role="alert"]')?.textContent).toMatch(/Turned away CW tent \(W9ABC\)/)
    expect(buttons[0].getAttribute('title')).toMatch(/turned away/)
    fireEvent.click(buttons[0])
    expect(give, 'a press asks first').not.toHaveBeenCalled()
    const asked = screen.getByRole('group', { name: 'Give this laptop its position?' })
    expect(asked.querySelector('[data-club-code]')?.textContent).toBe('7KQ2-M9XD')
    expect(asked.textContent).toMatch(/only if the code there is this one/)
    expect(asked.textContent).toMatch(/It says it is CW tent \(W9ABC\), which any laptop can say/)
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: YES })) })
    expect(give).toHaveBeenCalledTimes(1)
    expect(give).toHaveBeenCalledWith(42)
    expect(screen.getByText(/CW tent gets its position on its next try/).getAttribute('role')).toBe('status')
  })

  it('says nothing changed when the entry is gone by the press, or this Nexus is not hosting', async () => {
    for (const [refusal, said] of [
      ['stale', /no longer on this list/],
      ['notHosting', /not hosting a club event/],
    ] as const) {
      give.mockResolvedValueOnce({ outcome: 'refused', refusal })
      render(<FdClubSection club={HOST} onGivePosition={give} />)
      await giveAndConfirm(screen.getByRole('button', { name: LABEL }))
      expect(screen.getAllByRole('alert').map((a) => a.textContent).join(' ')).toMatch(said)
      cleanup()
    }
  })

  it('says so when the press did not reach the host', async () => {
    give.mockRejectedValueOnce('unavailable')
    render(<FdClubSection club={HOST} onGivePosition={give} />)
    await giveAndConfirm(screen.getByRole('button', { name: LABEL }))
    expect(screen.getAllByRole('alert').map((a) => a.textContent).join(' ')).toMatch(/could not give the position/)
  })
})

describe('the button is the host screen’s alone', () => {
  it('is not on Remote: an observed contest screen shows the entry with no button, whatever the data holds', () => {
    const observation = { fdOperator: '', fdPowerMult: 1, fdBonuses: [], fdBonusesPlanned: [] }
    render(<ContestView fieldDay={fd(HOST)} observation={observation} />)
    expect(screen.getByText(/Turned away CW tent/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: LABEL })).toBeNull()
  })

  it('is not in the pop-out, which shows the entry', () => {
    render(<FdClubSection club={HOST} detached />)
    expect(screen.getByText(/Turned away CW tent/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: LABEL })).toBeNull()
  })

  it('is not on a position’s screen, nor on an entry with no handle', () => {
    render(<FdClubSection club={{ ...HOST, hosting: false }} onGivePosition={give} />)
    expect(screen.queryByRole('button', { name: LABEL })).toBeNull()
    cleanup()
    render(<FdClubSection club={{ ...HOST, refused: [HOST.refused![1]] }} onGivePosition={give} />)
    expect(screen.getByText(/Turned away SSB tent/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: LABEL })).toBeNull()
  })
})

// jsdom never lays out: `elementFromPoint` does not exist. Each press says what is under it.
describe('a press through a stream gives no position', () => {
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
  const pointer = (action: 'down' | 'up') =>
    ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
      modifiers: 0, pointerType: 'mouse', clicks: 1 })

  it('refuses it and says why; CONTROL: the same button pressed at the host gives it', async () => {
    render(<FdClubSection club={HOST} onGivePosition={give} />)
    under = screen.getByRole('button', { name: LABEL })
    act(() => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
    await act(async () => { await Promise.resolve() })
    expect(give).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: YES }), 'nothing asked').toBeNull()
    expect(screen.getAllByRole('alert').map((a) => a.textContent).join(' ')).toMatch(/Only at the host/)
    await giveAndConfirm(screen.getByRole('button', { name: LABEL }))
    expect(give).toHaveBeenCalledWith(42)
  })

  it('refuses the answer too, when only that comes through the stream; CONTROL: answered at the host', async () => {
    render(<FdClubSection club={HOST} onGivePosition={give} />)
    fireEvent.click(screen.getByRole('button', { name: LABEL }))
    under = screen.getByRole('button', { name: YES })
    act(() => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
    await act(async () => { await Promise.resolve() })
    expect(give).not.toHaveBeenCalled()
    expect(screen.getAllByRole('alert').map((a) => a.textContent).join(' ')).toMatch(/Only at the host/)
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: YES })) })
    expect(give).toHaveBeenCalledWith(42)
  })
})

describe('the club codes tell laptops apart, on the host’s screen alone', () => {
  const code = (el: Element | null | undefined) => el?.querySelector('[data-club-code]')?.textContent ?? null

  it('shows this laptop’s own code on its club line, and each turned-away laptop’s on its entry', () => {
    render(<FdClubSection club={HOST} onGivePosition={give} />)
    const codes = [...document.querySelectorAll('[data-club-code]')].map((c) => c.textContent)
    expect(codes).toEqual(['Club code 3HV8-ZQ1P', 'Club code 7KQ2-M9XD', 'Club code 0B4N-VW8T'])
    expect(screen.getByText('Club code 3HV8-ZQ1P').getAttribute('title')).toMatch(/checks the code/)
  })

  it('tells two entries that read alike apart, and asks about the one pressed', async () => {
    const twins: FdClubStatus = {
      ...HOST,
      refused: [
        { posName: 'CW tent', call: 'W9ABC', reason: HELD, handle: 42, clubCode: '7KQ2-M9XD' },
        { posName: 'CW tent', call: 'W9ABC', reason: HELD, handle: 43, clubCode: 'X4RM-2Q8P' },
      ],
    }
    render(<FdClubSection club={twins} onGivePosition={give} />)
    const entries = screen.getAllByRole('button', { name: LABEL }).map((b) => b.closest('[role="alert"]'))
    expect(entries.map(code)).toEqual(['Club code 7KQ2-M9XD', 'Club code X4RM-2Q8P'])
    fireEvent.click(screen.getAllByRole('button', { name: LABEL })[1])
    expect(code(screen.getByRole('group', { name: 'Give this laptop its position?' }))).toBe('X4RM-2Q8P')
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: YES })) })
    expect(give).toHaveBeenCalledWith(43)
  })

  it('gives nothing on Cancel, and asks no more once the entry has gone from the list', () => {
    const { rerender } = render(<FdClubSection club={HOST} onGivePosition={give} />)
    fireEvent.click(screen.getByRole('button', { name: LABEL }))
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByRole('button', { name: YES })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: LABEL }))
    rerender(<FdClubSection club={{ ...HOST, refused: [HOST.refused![1]] }} onGivePosition={give} />)
    expect(screen.queryByRole('button', { name: YES })).toBeNull()
    expect(give).not.toHaveBeenCalled()
  })

  it('shows no code in the pop-out or on Remote, whatever the data holds', () => {
    render(<FdClubSection club={HOST} detached />)
    expect(document.querySelector('[data-club-code]')).toBeNull()
    expect(screen.getByText(/Turned away CW tent/)).toBeTruthy()
    cleanup()
    const observation = { fdOperator: '', fdPowerMult: 1, fdBonuses: [], fdBonusesPlanned: [] }
    render(<ContestView fieldDay={fd(HOST)} observation={observation} />)
    expect(screen.getByText(/Turned away CW tent/)).toBeTruthy()
    expect(document.querySelector('[data-club-code]')).toBeNull()
  })
})
