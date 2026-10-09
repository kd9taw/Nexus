// @vitest-environment jsdom
//
// THE LOGBOOK SHOWS THE CONTACT JUST LOGGED ON A SLOW DISK: its questions ask again after a refusal.
//
// The Logbook asks the engine for its page on every change to the log (`logTick`). The tick moves
// when a contact is logged, before it is saved, so on a slow disk the engine refuses the page
// (features/notAnswered) rather than answer without the contact. The list used to keep its old page
// until the next change, the contact missing. The window's log source now asks a refused question
// again a second later, three times at most. Held by value against a fake engine, through the real
// source and the real view: the list shows the contact once the save lands, and after the last
// refusal it keeps its old rows and asks no more.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import type { LoggedQso } from '../types'
import type { LogQuestion } from '../features/logAnswers'
import { answerAs } from '../features/logAnswers.testkit'
import { ASK_AGAIN_AFTER_MS, ASK_AGAIN_TIMES, NOT_ANSWERED } from '../features/notAnswered'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  askLog: vi.fn(),
}))

import { Logbook } from './Logbook'
import { askLog } from '../api'

/** The engine's log, and whether the change made last has reached the logbook database yet. */
const engine = { log: [] as LoggedQso[], saved: true }

const contact = (i: number, call: string, whenUnix: number) =>
  ({
    id: `id-${i}`, call, grid: 'FN31', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: null,
    country: 'United States', whenUnix, confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null,
  }) as unknown as LoggedQso
const BEFORE = [contact(1, 'W1AW', 1_700_000_000), contact(2, 'K1ABC', 1_700_000_060), contact(3, 'N2XYZ', 1_700_000_120)]
const ZD7AA = contact(4, 'ZD7AA', 1_700_000_180)

/** The calls the list shows, top to bottom. */
const rows = () =>
  [...document.querySelectorAll('.log-rows .logbook-row')].map((r) => r.querySelector('.qrz-link-call')?.textContent ?? '…')
/** The asks for the list's first page. */
const pageAsks = () =>
  vi.mocked(askLog).mock.calls.filter(([q]) => q.kind === 'page' && q.offset === 0).length
const settle = () => act(async () => {})
const aSecond = () => act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))

beforeEach(() => {
  engine.log = BEFORE
  engine.saved = true
  // Every question is refused while the change is on its way, as the engine refuses it.
  vi.mocked(askLog).mockImplementation(async (q: LogQuestion) => {
    if (!engine.saved) throw `${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`
    return answerAs(q, engine.log)
  })
  // A 2000 px viewport and 43 px rows (the list measures rows by `offsetHeight`), so every row fits.
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      return (this as HTMLElement).classList?.contains('logbook-row') ? 43 : 2000
    },
  })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.mocked(askLog).mockReset()
})

/** The Logbook open on the log as it was, then ZD7AA logged: the tick moves, the save is still on
 *  its way, and the engine refuses the list's questions. */
async function loggedOnASlowDisk() {
  const view = <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={1} />
  const { rerender } = render(view)
  await settle()
  expect(rows(), 'premise: the list shows the log').toEqual(['N2XYZ', 'K1ABC', 'W1AW'])
  engine.log = [...BEFORE, ZD7AA]
  engine.saved = false
  rerender(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={2} />)
  await settle()
  expect(rows(), 'the old page stays up meanwhile').toEqual(['N2XYZ', 'K1ABC', 'W1AW'])
}

describe('the Logbook on a slow disk', () => {
  it('asks again after a refusal, and shows the contact once the save lands', async () => {
    await loggedOnASlowDisk()
    const refused = pageAsks()
    engine.saved = true
    await aSecond()
    expect(rows()).toEqual(['ZD7AA', 'N2XYZ', 'K1ABC', 'W1AW'])
    expect(pageAsks()).toBe(refused + 1)
  })

  it(`refused ${1 + ASK_AGAIN_TIMES} times, it keeps its old rows and asks no more`, async () => {
    await loggedOnASlowDisk()
    const refused = pageAsks()
    for (let i = 0; i < ASK_AGAIN_TIMES; i++) await aSecond()
    expect(pageAsks()).toBe(refused + ASK_AGAIN_TIMES)
    expect(rows()).toEqual(['N2XYZ', 'K1ABC', 'W1AW'])
    await act(() => vi.advanceTimersByTimeAsync(60_000))
    expect(pageAsks(), 'never past its limit').toBe(refused + ASK_AGAIN_TIMES)
  })
})
