// @vitest-environment jsdom
//
// A LOG THAT EXISTS IS NEVER REPORTED EMPTY: the Logbook says "No logged contacts yet." only when
// the engine has ANSWERED that the log holds none.
//
// The view asks the engine how many contacts the log holds (`logSize`) and for the first page of
// its list. Until an answer lands (at every open, for a moment) and for as long as the engine
// refuses the questions (a change made before them is still being saved: a slow or a full disk,
// features/notAnswered), the view has no answer at all. It used to show the empty log's answer in
// its place, a count of 0, and with it "No logged contacts yet.", still up a minute later when the
// refusals lasted. It now says it is reading the logbook until it has an answer. Held by value
// against a fake engine, through the real source and the real view; an answered 0 and an answered
// 24 are the controls.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import type { LoggedQso } from '../types'
import type { LogQuestion } from '../features/logAnswers'
import { answerAs } from '../features/logAnswers.testkit'
import { ASK_AGAIN_AFTER_MS, ASK_AGAIN_TIMES, NOT_ANSWERED } from '../features/notAnswered'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import type { QueryPage } from '../remote-web/application-query-protocol'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  askLog: vi.fn(),
}))

import { Logbook } from './Logbook'
import { askLog } from '../api'

const EMPTY = 'No logged contacts yet.'
const READING = 'Reading the logbook…'

const contact = (i: number) =>
  ({
    id: `id-${i}`, call: `K${i}ABC`, grid: 'FN31', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: null,
    country: 'United States', whenUnix: 1_700_000_000 + i * 60, confirmed: false, awardConfirmed: false, qslRcvd: null,
    qslSent: null, ota: null,
  }) as unknown as LoggedQso
const LOG_OF_24 = Array.from({ length: 24 }, (_, k) => contact(k + 1))
/** The 24 as the list shows them: newest first. */
const NEWEST_FIRST = LOG_OF_24.map((q) => q.call).reverse()

/** The fake engine: its log, whether it refuses every question (a change still being saved), and
 *  the kinds of question it keeps waiting. */
const engine = { log: [] as LoggedQso[], refusing: false, waiting: new Set<LogQuestion['kind']>() }

/** What the list area says, top to bottom: its quiet lines. */
const said = () => [...document.querySelectorAll('.log-scroll > p.empty')].map((p) => p.textContent)
/** The calls the list shows, top to bottom. */
const rows = () =>
  [...document.querySelectorAll('.log-rows .logbook-row')].map((r) => r.querySelector('.qrz-link-call')?.textContent ?? '…')
/** The count beside the title, or null when it shows none. */
const badge = () => document.querySelector('.log-title .count-badge')?.textContent ?? null
/** The asks of the log's size. */
const sizeAsks = () => vi.mocked(askLog).mock.calls.filter(([q]) => q.kind === 'logSize').length
const settle = () => act(async () => {})
const aSecond = () => act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))
const open = async () => {
  render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={1} />)
  await settle()
}

beforeEach(() => {
  engine.log = LOG_OF_24
  engine.refusing = false
  engine.waiting = new Set()
  vi.mocked(askLog).mockImplementation(async (q: LogQuestion) => {
    if (engine.waiting.has(q.kind)) return new Promise<never>(() => {})
    if (engine.refusing) throw `${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`
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

describe('the Logbook before the engine has answered', () => {
  it(`says "${READING}" while its questions are on their way, never "${EMPTY}"`, async () => {
    engine.waiting = new Set(['logSize', 'page'])
    await open()
    expect(document.body.textContent, 'a log of 24 is never reported empty').not.toContain(EMPTY)
    expect(said()).toEqual([READING])
    expect(badge(), 'no count before there is one').toBeNull()
    expect(rows()).toEqual([])
  })

  it(`refused while a change is being saved: "${READING}" after every ask, and a minute later`, async () => {
    engine.refusing = true
    await open()
    expect(document.body.textContent, 'refused once').not.toContain(EMPTY)
    expect(said()).toEqual([READING])
    for (let i = 0; i < ASK_AGAIN_TIMES; i++) await aSecond()
    expect(sizeAsks(), 'premise: every ask refused, the last one too').toBe(1 + ASK_AGAIN_TIMES)
    expect(document.body.textContent, 'refused every time').not.toContain(EMPTY)
    expect(said()).toEqual([READING])
    await act(() => vi.advanceTimersByTimeAsync(60_000))
    expect(document.body.textContent, 'a minute later').not.toContain(EMPTY)
    expect(said()).toEqual([READING])
    expect(badge()).toBeNull()
    expect(rows()).toEqual([])
  })

  it('once the save lands, the answer takes the line’s place', async () => {
    engine.refusing = true
    await open()
    expect(said(), 'premise: refused').toEqual([READING])
    engine.refusing = false
    await aSecond()
    expect(rows()).toEqual(NEWEST_FIRST)
    expect(said()).toEqual([])
    expect(badge()).toBe('24')
  })

  it(`the count answered before the first page: "${READING}", never "No contacts match"`, async () => {
    engine.waiting = new Set(['page'])
    await open()
    expect(badge(), 'premise: the count is in').toBe('24')
    expect(said()).toEqual([READING])
  })

  it(`the first page answered before the count: its rows, and no "${EMPTY}" beside them`, async () => {
    engine.waiting = new Set(['logSize'])
    await open()
    expect(rows(), 'premise: the page is in').toEqual(NEWEST_FIRST)
    expect(document.body.textContent).not.toContain(EMPTY)
    expect(said()).toEqual([])
    expect(badge()).toBeNull()
  })
})

describe('the controls: an answer is shown as it is', () => {
  it(`an ANSWERED 0 says "${EMPTY}"`, async () => {
    engine.log = []
    await open()
    expect(said()).toEqual([EMPTY])
    expect(badge()).toBe('0')
    expect(rows()).toEqual([])
  })

  it('an answered 24 shows the 24 rows', async () => {
    await open()
    expect(rows()).toEqual(NEWEST_FIRST)
    expect(said()).toEqual([])
    expect(badge()).toBe('24')
  })
})

// The Remote page's Logbook shows the page the station sent (`useRemoteLog`), not the engine's
// answers: "loading" and "unavailable" are its own states, said in its status line.
describe('the Remote page’s Logbook', () => {
  const page = (rows: LoggedQso[]): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), collection: 'log',
    snapshotId: crypto.randomUUID(), offset: 0, total: rows.length, retained: rows.length, nextCursor: null, ageMs: 0,
    rows: rows as unknown as QueryPage['rows'], meta: {} })
  /** The station: its log, searched by call. */
  const station = (log: LoggedQso[], answer: (search: string) => Promise<QueryPage> = async (search) =>
    page(log.filter((q) => q.call.includes(search.toUpperCase())))) => ({
    page: vi.fn(async ({ search }: { search: string }) => answer(search)),
  })
  const observe = async (source: ReturnType<typeof station>) => {
    render(
      <StationControlContext.Provider value={false}>
        <RemoteCollectionsContext.Provider value={source as unknown as RemoteCollections}>
          <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
        </RemoteCollectionsContext.Provider>
      </StationControlContext.Provider>,
    )
    await act(() => vi.advanceTimersByTimeAsync(300))
  }

  it(`while the station's page is on its way, and once the station refuses it: never "${EMPTY}"`, async () => {
    await observe(station(LOG_OF_24, () => new Promise<never>(() => {})))
    expect(document.body.textContent, 'on its way').not.toContain(EMPTY)
    cleanup()
    await observe(station(LOG_OF_24, async () => Promise.reject(new Error('busy'))))
    expect(document.body.textContent, 'refused').not.toContain(EMPTY)
  })

  it(`a search that matches nothing says so, never "${EMPTY}"`, async () => {
    await observe(station(LOG_OF_24))
    expect(rows(), 'premise: the station’s page').toEqual(NEWEST_FIRST)
    fireEvent.change(document.querySelector('input.log-search')!, { target: { value: 'ZZ9' } })
    await act(() => vi.advanceTimersByTimeAsync(300))
    expect(rows(), 'premise: nothing matches').toEqual([])
    expect(document.body.textContent).not.toContain(EMPTY)
    expect(said()).toEqual(['No contacts match “ZZ9”.'])
  })

  it(`the station's log is empty: "${EMPTY}"`, async () => {
    await observe(station([]))
    expect(said()).toEqual([EMPTY])
  })
})
