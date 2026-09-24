// @vitest-environment jsdom
//
// THE LOGBOOK ON THE ASKING SOURCE — the engine's shape (SPEC-2 v3 C17a).
//
// With the whole-log source every question about a newer order is answered in the same turn, so a
// list that swapped to the new order without waiting for its open rows' new places could not be
// told from one that waited (C17b's e1ad18c1; mutation M30). The engine answers each question on
// its own, in whatever order the answers land. Here the window's own source
// (`createAskingLogSource`) asks, and the open row's new place is the LAST answer to land: until it
// has, the list keeps the order it shows, laid out as its rows are; then it swaps, the open row's
// height moved with it.
//
// jsdom lays nothing out, so this reads what the list itself decides, as
// Logbook.openRowHeights.test.tsx does: each drawn row's place against the height the row really
// has. Rows are 43 px; a row whose note is open is 120.

import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ConfirmHost } from '../confirm'
import { createAskingLogSource, type LogTransport } from '../features/askingLogSource'
import { answerFrom, type AnswerTo, type LogQuestion } from '../features/logAnswers'
import { setLogSource } from '../features/logSource'
import type { LoggedQso } from '../types'

vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    getLogDelta: vi.fn(async () => {
      throw new Error('the asking source never reads the whole log')
    }),
    deleteQso: noop(), editQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTag: vi.fn(async () => ({})),
    saveTextToDownloads: noop(), logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: noop(), markQslCard: noop(), syncLotwReport: noop(), uploadLotwReport: noop(),
    qrzPushQso: noop(), clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((run: () => Promise<unknown>) => run()) }))

const ROW_PX = 43
const OPEN_PX = 120

/** The list's ResizeObservers, driven by the test: `fire()` delivers what a browser would — a size
 *  for every element each one observes (as on mounting, or on a row growing when its note opens). */
const observers = new Set<{ cb: (entries: { target: Element }[]) => void; els: Set<Element> }>()
const fire = () =>
  act(() => {
    for (const o of observers) o.cb([...o.els].filter((el) => el.isConnected).map((target) => ({ target })))
  })

beforeAll(() => {
  globalThis.ResizeObserver = class {
    private o: { cb: (entries: { target: Element }[]) => void; els: Set<Element> }
    constructor(cb: (entries: { target: Element }[]) => void) {
      this.o = { cb, els: new Set() }
      observers.add(this.o)
    }
    observe(el: Element) {
      this.o.els.add(el)
    }
    unobserve(el: Element) {
      this.o.els.delete(el)
    }
    disconnect() {
      this.o.els.clear()
    }
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      const el = this as HTMLElement
      if (el.classList?.contains('logbook-row')) return el.querySelector('.log-note.expanded') ? OPEN_PX : ROW_PX
      return 600
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
beforeEach(() => {
  observers.clear()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

/** `n` contacts, newest last in the log (so newest-first puts contact n-1 on top); `withNote` get a
 *  private note, whose 📝 opens the row. */
const log = (n: number, withNote: number[]) =>
  Array.from({ length: n }, (_, i) =>
    ({
      id: `id-${i}`,
      call: `K${i}ABC`,
      grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12',
      name: null, qth: null, comment: null,
      notes: withNote.includes(i) ? 'Worked him on the club net.\nQSL via bureau.' : null,
      country: 'United States', whenUnix: 1_700_000_000 + i * 60,
      confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null,
    }) as unknown as LoggedQso,
  )
const newContact = (i: number) =>
  ({ ...log(i + 1, [])[i], id: `id-new-${i}`, call: `NEW${i}`, whenUnix: 1_800_000_000 + i }) as unknown as LoggedQso

/** The engine's transport, held: each question waits until the test answers it. The answers are
 *  today's (`answerFrom`, which the engine's are held to by the goldens), over a log at a chosen
 *  revision — a page's `orderRev` is how the list learns an order is newer. */
function heldTransport() {
  const waiting: { q: LogQuestion; answer: (a: unknown) => void }[] = []
  const transport: LogTransport = <Q extends LogQuestion>(q: Q) =>
    new Promise<AnswerTo<Q>>((resolve) => waiting.push({ q, answer: resolve as (a: unknown) => void }))
  /** Let the window ask: the source re-asks a stale question on the turn after a change. */
  const settle = () =>
    act(async () => {
      await new Promise((r) => setTimeout(r, 0))
    })
  /** Answer every question asked so far that `only` picks, and every one those answers bring on. */
  const deliver = async (from: LoggedQso[], revision: number, only: (q: LogQuestion) => boolean = () => true) => {
    for (let round = 0; round < 50; round++) {
      await settle()
      const now = waiting.filter((w) => only(w.q))
      if (now.length === 0) return
      for (const w of now) {
        waiting.splice(waiting.indexOf(w), 1)
        w.answer(answerFrom(from, w.q, revision))
      }
    }
    throw new Error('the questions never stopped coming')
  }
  const asked = (pick: (q: LogQuestion) => boolean) => waiting.some((w) => pick(w.q))
  return { transport, deliver, asked }
}

const view = (logTick: number) => (
  <>
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={logTick} />
    <ConfirmHost />
  </>
)
const rowY = (el: HTMLElement) => Number(/translateY\((-?[\d.]+)px\)/.exec(el.style.transform)?.[1] ?? NaN)
/** Every pair of drawn rows whose places disagree with the first one's height: the gap (+) or the
 *  overlap (−) between them, by list index. Empty when the list is laid out as the rows really are. */
function misfits(root: HTMLElement) {
  const rows = ([...root.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[]).sort(
    (a, b) => Number(a.dataset.index) - Number(b.dataset.index),
  )
  const out: string[] = []
  for (let k = 1; k < rows.length; k++) {
    const [a, b] = [rows[k - 1], rows[k]]
    if (Number(b.dataset.index) !== Number(a.dataset.index) + 1) continue
    const off = rowY(b) - rowY(a) - a.offsetHeight
    if (off !== 0) out.push(`${a.dataset.index}:${off > 0 ? '+' : ''}${off}`)
  }
  return out
}
const rowOf = (root: HTMLElement, call: string) =>
  ([...root.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[]).find(
    (r) => r.querySelector('.qrz-link-call')?.textContent === call,
  )
const indexOf = (root: HTMLElement, call: string) => Number(rowOf(root, call)?.dataset.index)
const openNoteOf = (root: HTMLElement, call: string) =>
  fireEvent.click(rowOf(root, call)!.querySelector('.log-note-flag') as HTMLElement)

/** The open row's new place, asked while the list waits to swap. */
const placeOfRow = (id: string) => (q: LogQuestion) => q.kind === 'locate' && q.id === id

describe('the Logbook on the asking source: an open row’s place is the last answer to land', () => {
  it('the list at the top: it keeps its order until the open row’s place is here, then swaps with its height', async () => {
    const rows = log(60, [55]) // K55ABC is 5th from the top (index 4)
    const engine = heldTransport()
    setLogSource(createAskingLogSource(engine.transport))
    const { container, rerender } = render(view(1))
    await engine.deliver(rows, 1)
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(4))
    fire() // every row measured as it mounts
    openNoteOf(container, 'K55ABC')
    fire() // the open row grows, and is measured
    expect(misfits(container), 'the open row laid out before anything moves').toEqual([])

    // A contact is logged: the tick moves, and the answers of the newer order come back one by one.
    const newer = [...rows, newContact(60)]
    rerender(view(2))
    await engine.deliver(newer, 2, (q) => !placeOfRow('id-55')(q))
    expect(engine.asked(placeOfRow('id-55')), 'premise: the list asked where the open row went').toBe(true)
    expect(indexOf(container, 'K55ABC'), 'the list still shows the order it had').toBe(4)
    expect(misfits(container), 'and it is laid out as its rows are').toEqual([])

    await engine.deliver(newer, 2)
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(5))
    expect(misfits(container), 'swapped, with the open row’s height moved').toEqual([])
  })

  it('the list held still: the anchor’s place alone does not swap it while the open row’s is on its way', async () => {
    const rows = log(300, [199]) // K199ABC is index 100 in the newest-first list
    const engine = heldTransport()
    setLogSource(createAskingLogSource(engine.transport))
    const { container, rerender } = render(view(1))
    await engine.deliver(rows, 1)
    await waitFor(() => expect(container.querySelector('.log-rows .logbook-row')).not.toBeNull())
    const sc = container.querySelector('.log-scroll') as HTMLElement
    const scrollTo = async (index: number) => {
      act(() => {
        sc.scrollTop = index * ROW_PX
        sc.dispatchEvent(new Event('scroll'))
      })
      await engine.deliver(rows, 1) // the pages of the rows now on screen
    }
    await scrollTo(96)
    await waitFor(() => expect(indexOf(container, 'K199ABC')).toBe(100))
    fire()
    openNoteOf(container, 'K199ABC')
    fire()
    // Down a few rows: the open row is above the view, still drawn (the list draws 12 beyond it).
    await scrollTo(106)
    await waitFor(() => expect(container.querySelector('.logbook-row[data-index="106"]')).not.toBeNull())
    expect(indexOf(container, 'K199ABC'), 'premise: the open row is drawn, above the view').toBe(100)
    expect(misfits(container)).toEqual([])

    const newer = [...rows, newContact(300)]
    rerender(view(2))
    await engine.deliver(newer, 2, (q) => !placeOfRow('id-199')(q))
    expect(engine.asked(placeOfRow('id-199')), 'premise: the list asked where the open row went').toBe(true)
    expect(indexOf(container, 'K199ABC'), 'the list still shows the order it had').toBe(100)
    expect(misfits(container), 'and it is laid out as its rows are').toEqual([])

    await engine.deliver(newer, 2)
    await waitFor(() => expect(indexOf(container, 'K199ABC')).toBe(101))
    expect(misfits(container), 'swapped, with the open row’s height moved').toEqual([])
  })
})
