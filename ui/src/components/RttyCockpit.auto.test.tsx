// @vitest-environment jsdom
//
// THE AUTO TOGGLE'S REFUSAL REACHES THE OPERATOR, IN THE ENGINE'S OWN WORDS.
//
// `Engine::rtty_auto_contest_gate` refuses to arm the sequencer in any contest but the two
// Field Day events, and it refuses with a SENTENCE that says what to do instead (send the
// exchange with the macros, log each contact yourself). A toggle that swallowed the detail
// and toasted its own fallback would leave the operator with a button that does nothing and
// no reason — in a contest, mid-run.
//
// ⚠️ THE REAL TOAST BUS, not a mock of it. Every other RTTY suite replaces `../toast` with a
// pass-through, and a pass-through that re-implements `withErrorToast` would be testing the
// copy: the whole question here is whether the error's `message` survives the wrapper the
// cockpit actually calls. So this file mocks the api and the log strip and lets the toast
// module be itself, reading what was pushed off `subscribeToasts` — the same bus the toast
// host renders.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import { subscribeToasts, type Toast } from '../toast'
import * as api from '../api'
import type { AppSnapshot, RttyState } from '../types'

globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(),
  rttyAutoArm: vi.fn(),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttySetAuto: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  haltTx: vi.fn(),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const mocked = <T,>(f: T) => f as unknown as ReturnType<typeof vi.fn>

/** `Engine::rtty_auto_contest_gate`, verbatim — the sentence a Tauri command rejects with. */
const REFUSAL =
  'RTTY Auto works the Field Day exchange only (ARRL Field Day and Winter Field Day). ' +
  'For this contest, send your exchange with the macros and log each contact yourself.'

const IDLE = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: '',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  markHz: 2125,
  spaceHz: 2295,
  sending: false,
  latched: false,
  backend: 'afsk',
  keyerError: null,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
} as unknown as RttyState

const snap = {
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.08, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

let seen: string[] = []
let unsubscribe = () => {}

beforeEach(() => {
  mocked(api.getRttyState).mockReset().mockResolvedValue(IDLE)
  mocked(api.rttyAutoArm).mockReset().mockResolvedValue(IDLE)
  mocked(api.rttySetAuto).mockReset()
  seen = []
  // The bus is app-wide and an error toast lives 12 s, so the previous test's toast is still
  // on it when this one subscribes: count only what is pushed from here on.
  let floor = -1
  unsubscribe = subscribeToasts((ts: Toast[]) => {
    if (floor < 0) floor = ts.reduce((m, t) => Math.max(m, t.id), 0)
    for (const t of ts) if (t.id > floor && !seen.includes(t.message)) seen.push(t.message)
  })
})
afterEach(() => {
  unsubscribe()
  cleanup()
})

const autoBtn = () => screen.getByRole('button', { name: /^Auto( on)?$/ })

describe('the Auto toggle', () => {
  it('says WHY the engine refused to arm — the rule, not just “could not”', async () => {
    mocked(api.rttySetAuto).mockRejectedValue(new Error(REFUSAL))
    render(<RttyCockpit snap={snap} active />)
    await waitFor(() => expect(api.getRttyState).toHaveBeenCalled())
    fireEvent.click(autoBtn())
    await waitFor(() => expect(seen.length).toBe(1))
    expect(seen[0], 'the operator must be told which contests Auto can work').toContain(
      'Field Day exchange only',
    )
    expect(seen[0], 'and what to do instead of it').toContain('send your exchange with the macros')
    // Still off, and the button is still there to try again.
    expect(autoBtn().getAttribute('aria-pressed')).toBe('false')
  })

  it('POSITIVE CONTROL: where Auto can arm, it arms and says nothing', async () => {
    mocked(api.rttySetAuto).mockResolvedValue({ ...IDLE, auto: true })
    render(<RttyCockpit snap={snap} active />)
    await waitFor(() => expect(api.getRttyState).toHaveBeenCalled())
    fireEvent.click(autoBtn())
    await waitFor(() => expect(autoBtn().getAttribute('aria-pressed')).toBe('true'))
    expect(seen, 'an arm that worked is not news').toEqual([])
  })
})
