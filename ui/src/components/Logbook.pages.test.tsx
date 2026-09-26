// @vitest-environment jsdom
//
// THE LOGBOOK LIST, IN PAGES (SPEC-2 v3 C17b, V5; v2 §6).
//
// The list used to be the whole log, filtered and sorted in the view. It is now `total` rows long,
// and each row comes from a page of 128 the view asks `LogSource` for. Pinned here:
//   1. rows past the first page are exactly the rows the view used to show there — today's order
//      (an ORACLE copy of the view's own filter-and-sort) at the same places;
//   2. a row is its ID: the edit mark and an open comment stay on their contact when the list moves
//      under them (v2 §6 (5)). The view used to key both by the row's log POSITION, so a delete
//      above moved them onto the next contact — this half is a FIX, red on the code before it;
//   3. with a source that answers LATER (the engine's, C17a), a row whose page is on its way is a
//      placeholder in its place, and a page cut from an OLDER order than the first page's is never
//      shown beside it (v2 R4) — the rows of one view come from one order.

import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ConfirmHost } from '../confirm'
import { t } from '../i18n'
import { answerFrom, questionKey, type AnswerTo, type LogQuestion } from '../features/logAnswers'
import { DEFAULT_LOG_QUERY } from '../features/logQuery'
import { setLogSource, type LogSource } from '../features/logSource'
import type { LoggedQso } from '../types'

const engine = vi.hoisted(() => ({ log: [] as unknown[], revision: 1 }))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    // The engine answers each question from its log (features/logAnswers.testkit).
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, engine.log as LoggedQso[], engine.revision)),
    deleteQsoById: vi.fn(() => Promise.resolve({})),
    editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(), logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(), syncLotwReport: noop(), uploadLotwReport: noop(),
    qrzPushQso: noop(), clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((run: () => Promise<unknown>) => run()) }))

const ROW_PX = 43 // the list's own row estimate — what jsdom's zero-height rows are placed at
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  // A 600 px viewport, so the virtualizer puts ~14 rows (+ overscan) on screen — and each row
  // measures at the list's own estimate, so a row's place is index × ROW_PX, as it is on screen
  // (the virtualizer measures rows by `offsetHeight`; a blanket 600 would make every row 600 tall).
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      return (this as HTMLElement).classList?.contains('logbook-row') ? ROW_PX : 600
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})
// The list's end-of-scroll report comes 150 ms after the last scroll event, from a timer the list
// does not clear when it unmounts. Let the last test's land while this file's window still exists
// (after it, React has no `window` to read, and the run fails on an error in no test).
afterAll(() => new Promise((resolve) => setTimeout(resolve, 200)))

const contact = (i: number, over: Partial<LoggedQso> = {}): LoggedQso =>
  ({
    id: `id-${i}`,
    call: `K${i}ABC`,
    grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12',
    name: null, qth: null, comment: null, notes: null, country: 'United States',
    // Every third contact shares a second with its neighbour, so ties are everywhere.
    whenUnix: 1_700_000_000 + Math.floor(i / 3) * 60,
    confirmed: false, awardConfirmed: i % 5 === 0, qslRcvd: null, qslSent: null, ota: null,
    ...over,
  }) as unknown as LoggedQso

/** The calls the list shows, top to bottom (placeholders read as '…'). */
const shownCalls = (root: HTMLElement) =>
  [...root.querySelectorAll('.log-rows .logbook-row')].map((r) =>
    r.classList.contains('placeholder') ? '…' : (r.querySelector('.qrz-link-call')?.textContent ?? '?'),
  )
const view = (logTick: number) => (
  <>
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={logTick} />
    <ConfirmHost />
  </>
)

// ---- ORACLE: the view's own filter-and-sort at d5be14ea (the default query: newest first) ----
function oracleNewestFirst(log: LoggedQso[]): LoggedQso[] {
  const out = log.map((q, i) => ({ q, i }))
  out.sort((a, b) => {
    const av = a.q.whenUnix
    const bv = b.q.whenUnix
    const cmp = av < bv ? -1 : av > bv ? 1 : a.q.whenUnix - b.q.whenUnix
    return -cmp
  })
  return out.map(({ q }) => q)
}

describe('rows past the first page', () => {
  it('are the rows the view used to show at those places', async () => {
    engine.log = Array.from({ length: 300 }, (_, i) => contact(i))
    const { container } = render(view(1))
    await waitFor(() => expect(shownCalls(container).length).toBeGreaterThan(0))
    const scroller = container.querySelector('.log-scroll') as HTMLElement
    // Scroll into the THIRD page (rows 256…) and let the virtualizer take the new window.
    act(() => {
      scroller.scrollTop = 262 * ROW_PX
      scroller.dispatchEvent(new Event('scroll'))
    })
    await waitFor(() => expect(container.querySelector('.logbook-row[data-index="262"]')).not.toBeNull())
    // Every row in the window DRAWN: a row whose page is still on its way is a placeholder at its
    // index, and the pages are answers of their own.
    await waitFor(() => expect(shownCalls(container), 'a page of the window never arrived').not.toContain('…'))
    const want = oracleNewestFirst(engine.log as LoggedQso[])
    const rendered = [...container.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[]
    expect(rendered.some((r) => Number(r.dataset.index) >= 256), 'the window never reached page 3').toBe(true)
    expect(rendered.some((r) => Number(r.dataset.index) < 256), 'the window must straddle two pages').toBe(true)
    for (const r of rendered) {
      const i = Number(r.dataset.index)
      expect(r.querySelector('.qrz-link-call')?.textContent, `row ${i}`).toBe(want[i].call)
    }
  })
})

describe('a row is its id', () => {
  it('FIX: the edit mark and an open comment stay on their contact when a delete above moves the list', async () => {
    const rows = [
      contact(1, { call: 'W1AW', whenUnix: 1_700_000_300 }),
      contact(2, { call: 'K1ABC', whenUnix: 1_700_000_200, comment: 'rag chew about antennas' }),
      contact(3, { call: 'N2XYZ', whenUnix: 1_700_000_100 }),
      contact(4, { call: 'G4ABC', whenUnix: 1_700_000_000 }),
    ]
    engine.log = rows
    const { container, rerender } = render(view(1))
    await waitFor(() => expect(shownCalls(container)).toEqual(['W1AW', 'K1ABC', 'N2XYZ', 'G4ABC']))
    fireEvent.click(container.querySelector('button[aria-label="Edit K1ABC"]') as HTMLButtonElement)
    fireEvent.click(container.querySelector('.log-note-text') as HTMLButtonElement) // open K1ABC's comment
    const marked = () => container.querySelector('.logbook-row.editing .qrz-link-call')?.textContent
    const openRow = () => container.querySelector('.log-note.expanded')?.closest('.logbook-row')?.querySelector('.qrz-link-call')?.textContent
    expect(marked()).toBe('K1ABC')
    expect(openRow()).toBe('K1ABC')

    // Another writer deletes W1AW — the contact ABOVE, lower in the log — and the tick moves.
    engine.log = rows.slice(1)
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(shownCalls(container)).toEqual(['K1ABC', 'N2XYZ', 'G4ABC']))
    expect(marked(), 'the edit mark moved to another contact').toBe('K1ABC')
    expect(openRow(), 'the open comment moved to another contact').toBe('K1ABC')
  })
})

describe('the rows under the operator stay put (v2 §6 R5)', () => {
  /** The list's scroller, a row's index and its top edge in the viewport (index × row − scroll). */
  const scroller = (root: HTMLElement) => root.querySelector('.log-scroll') as HTMLElement
  /** A drawn contact's place in the list, or null. */
  const indexOf = (root: HTMLElement, call: string) => {
    const row = [...root.querySelectorAll('.log-rows .logbook-row')].find((r) => r.querySelector('.qrz-link-call')?.textContent === call) as HTMLElement | undefined
    return row ? Number(row.dataset.index) : null
  }
  const topOf = (root: HTMLElement, call: string) => {
    const row = [...root.querySelectorAll('.log-rows .logbook-row')].find((r) => r.querySelector('.qrz-link-call')?.textContent === call) as HTMLElement | undefined
    return row ? Number(row.dataset.index) * ROW_PX - scroller(root).scrollTop : null
  }
  const scrollToRow = (root: HTMLElement, index: number) =>
    act(() => {
      scroller(root).scrollTop = index * ROW_PX
      scroller(root).dispatchEvent(new Event('scroll'))
    })
  const firstVisible = (root: HTMLElement) => {
    const top = scroller(root).scrollTop
    const rows = [...root.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[]
    const at = rows.map((r) => Number(r.dataset.index)).filter((i) => i * ROW_PX >= top).sort((a, b) => a - b)[0]
    return rows.find((r) => Number(r.dataset.index) === at)!.querySelector('.qrz-link-call')!.textContent!
  }

  it('FIX: a contact logged while the operator is scrolled down does not move the rows they are looking at', async () => {
    engine.log = Array.from({ length: 300 }, (_, i) => contact(i))
    engine.revision = 1
    const { container, rerender } = render(view(1))
    await waitFor(() => expect(shownCalls(container).length).toBeGreaterThan(0))
    scrollToRow(container, 150)
    await waitFor(() => expect(container.querySelector('.logbook-row[data-index="150"]:not(.placeholder)')).not.toBeNull())
    const looking = firstVisible(container)
    const before = topOf(container, looking)
    const at = indexOf(container, looking)!

    // The sequencer logs a contact: newest first, it lands ABOVE everything on screen.
    engine.log = [...(engine.log as LoggedQso[]), contact(1000, { call: 'NEW1', whenUnix: 1_800_000_000 })]
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(container.querySelector('.count-badge')?.textContent).toBe('301'))
    // The NEW order on screen — the row one place further down the list — before its top is
    // compared: while the old page is still shown, nothing has moved and the check proves nothing.
    await waitFor(() => expect(indexOf(container, looking)).toBe(at + 1))
    expect(topOf(container, looking), `${looking} moved under the operator`).toBe(before)
  })

  it('a delete above keeps them put too; the anchor row itself deleted, the next one holds its place (R5 (3))', async () => {
    const rows = Array.from({ length: 300 }, (_, i) => contact(i))
    engine.log = rows
    engine.revision = 1
    const { container, rerender } = render(view(1))
    await waitFor(() => expect(shownCalls(container).length).toBeGreaterThan(0))
    scrollToRow(container, 120)
    await waitFor(() => expect(container.querySelector('.logbook-row[data-index="120"]')).not.toBeNull())
    const looking = firstVisible(container)
    const next = (() => {
      const i = Number(([...container.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[]).find((r) => r.querySelector('.qrz-link-call')?.textContent === looking)!.dataset.index)
      return (container.querySelector(`.logbook-row[data-index="${i + 1}"] .qrz-link-call`) as HTMLElement).textContent!
    })()
    const nextBefore = topOf(container, next)!
    const nextAt = indexOf(container, next)!
    // Another writer deletes the anchor row itself AND one far above it.
    engine.log = rows.filter((q) => q.call !== looking && q.call !== 'K299ABC')
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(container.querySelector('.count-badge')?.textContent).toBe('298'))
    // The NEW order on screen (two rows gone above it) before its top is compared — see above.
    await waitFor(() => expect(indexOf(container, next)).toBe(nextAt - 2))
    // The row the operator was looking at is gone; the next surviving one holds exactly where it
    // was (v2 §6 R5 (3)) — nothing still on screen moves, the gap closes from above the view.
    expect(topOf(container, next)).toBe(nextBefore)
  })

  it('at the top of the list, a new contact appears at the top (the list is not held down)', async () => {
    engine.log = Array.from({ length: 300 }, (_, i) => contact(i))
    engine.revision = 1
    const { container, rerender } = render(view(1))
    await waitFor(() => expect(shownCalls(container).length).toBeGreaterThan(0))
    expect(scroller(container).scrollTop).toBe(0)
    engine.log = [...(engine.log as LoggedQso[]), contact(1000, { call: 'NEW1', whenUnix: 1_800_000_000 })]
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(shownCalls(container)[0]).toBe('NEW1'))
    expect(scroller(container).scrollTop).toBe(0)
  })
})

/** A source that answers only when the test says so — the shape of the engine's (C17a). Answers
 *  are today's, from `answerFrom` over the log at a chosen revision. */
function laterSource() {
  const held = new Map<string, unknown>()
  const listeners = new Set<() => void>()
  const wanted = new Map<string, LogQuestion>()
  const asks: { q: LogQuestion; resolve: (a: unknown) => void }[] = []
  const source: LogSource = {
    peek: <Q extends LogQuestion>(q: Q) => held.get(questionKey(q)) as AnswerTo<Q> | undefined,
    // Answered at the next delivery, like every other question.
    ask: <Q extends LogQuestion>(q: Q) => new Promise<AnswerTo<Q>>((resolve) => asks.push({ q, resolve: resolve as (a: unknown) => void })),
    want: (q) => {
      wanted.set(questionKey(q), q)
      return () => wanted.delete(questionKey(q))
    },
    follow: () => {},
    subscribe: (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    refresh: () => {},
  }
  /** Answer every question wanted so far (or those `only` picks) from `log` at `revision`. */
  const deliver = (log: LoggedQso[], revision: number, only: (q: LogQuestion) => boolean = () => true) =>
    act(async () => {
      for (const [key, q] of wanted) if (only(q)) held.set(key, answerFrom(log, q, revision))
      for (const a of asks.splice(0)) {
        if (only(a.q)) a.resolve(answerFrom(log, a.q, revision))
        else asks.push(a)
      }
      for (const l of listeners) l()
    })
  return { source, deliver, wanted }
}

describe('with a source that answers later (the engine’s shape)', () => {
  it('draws placeholders in place, then the rows, and never mixes a page from an older order', async () => {
    const log = Array.from({ length: 300 }, (_, i) => contact(i))
    const later = laterSource()
    setLogSource(later.source)
    const { container } = render(view(1))
    // Nothing answered yet: the view knows no rows, so it shows none (and asks for the first page).
    expect(shownCalls(container)).toEqual([])
    expect([...later.wanted.values()].some((q) => q.kind === 'page' && q.offset === 0)).toBe(true)

    // The first page and the count arrive: the list is 300 long and its top rows are real.
    await later.deliver(log, 1)
    await waitFor(() => expect(shownCalls(container)[0]).toBe(oracleNewestFirst(log)[0].call))

    // Scroll to the page boundary: page 3 is wanted but not answered — its rows are placeholders.
    const scroller = container.querySelector('.log-scroll') as HTMLElement
    act(() => {
      scroller.scrollTop = 250 * ROW_PX
      scroller.dispatchEvent(new Event('scroll'))
    })
    await waitFor(() => expect(container.querySelector('.logbook-row[data-index="258"]')).not.toBeNull())
    const placeholder = container.querySelector('.logbook-row[data-index="258"]') as HTMLElement
    expect(placeholder.classList.contains('placeholder'), 'a row of an unanswered page must hold its place').toBe(true)
    expect(placeholder.getAttribute('aria-busy')).toBe('true')
    expect(placeholder.textContent).toBe(t('logbook.rows.loading'))

    // Page 3 arrives from a NEWER order (a contact logged since) while page 1 is still the old
    // order: the two cannot share a view, so page 3's rows stay placeholders…
    const newer = [...log, contact(999, { call: 'NEW1', whenUnix: 1_800_000_000 })]
    await later.deliver(newer, 2, (q) => q.kind === 'page' && q.offset === 256)
    expect(container.querySelector('.logbook-row[data-index="258"]')?.classList.contains('placeholder')).toBe(true)
    // …until the first page is of that order too — then every row is from the newer order.
    await later.deliver(newer, 2)
    await waitFor(() => expect(container.querySelector('.logbook-row.placeholder')).toBeNull())
    const want = oracleNewestFirst(newer)
    for (const r of [...container.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[]) {
      const i = Number(r.dataset.index)
      expect(r.querySelector('.qrz-link-call')?.textContent, `row ${i}`).toBe(want[i].call)
    }
  })

  // The answers of a newer order land one at a time, in either order: the other pages before the
  // anchor's new place, or the place before the pages it shows. Both must hold the list still.
  const ORDERS: [string, (q: LogQuestion) => boolean, (q: LogQuestion) => boolean][] = [
    ['pages, then the anchor’s place', (q) => q.kind === 'page', (q) => q.kind === 'locate'],
    ['the anchor’s place, then the pages', (q) => q.kind === 'locate', (q) => q.kind === 'page'],
  ]
  for (const [name, second, third] of ORDERS)
    it(`FIX: a newer order is swapped in WHOLE — the row under the operator never moves (${name})`, async () => {
      // A source that answers one question at a time (the engine's, C17a): after a contact is
      // logged, the first page, the other pages and the anchor's new place land separately. The
      // list used to swap to the new order on the first of them, drawing it at the old scroll until
      // the anchor's place arrived — the row under the pointer sat 43 px off, and before its page
      // came it was not drawn at all. It must hold still through every partial delivery.
      const log = Array.from({ length: 300 }, (_, i) => contact(i))
      const later = laterSource()
      setLogSource(later.source)
      const { container, rerender } = render(view(1))
      await later.deliver(log, 1)
      await waitFor(() => expect(shownCalls(container).length).toBeGreaterThan(0))
      const sc = container.querySelector('.log-scroll') as HTMLElement
      act(() => {
        sc.scrollTop = 150 * ROW_PX
        sc.dispatchEvent(new Event('scroll'))
      })
      await later.deliver(log, 1) // the pages of the rows now on screen
      await waitFor(() => expect(container.querySelector('.logbook-row[data-index="150"] .qrz-link-call')).not.toBeNull())
      const looking = (container.querySelector('.logbook-row[data-index="150"] .qrz-link-call') as HTMLElement).textContent!
      const topOfLooking = () => {
        const row = [...container.querySelectorAll('.log-rows .logbook-row')].find(
          (r) => r.querySelector('.qrz-link-call')?.textContent === looking,
        ) as HTMLElement | undefined
        return row ? Number(row.dataset.index) * ROW_PX - sc.scrollTop : null
      }
      const before = topOfLooking()
      expect(before).toBe(0)

      // The sequencer logs a contact; the snapshot's tick moves; the answers come back one by one.
      const newer = [...log, contact(1000, { call: 'NEW1', whenUnix: 1_800_000_000 })]
      rerender(view(2))
      await later.deliver(newer, 2, (q) => q.kind === 'page' && q.offset === 0)
      expect(topOfLooking(), 'after the new first page alone').toBe(before)
      await later.deliver(newer, 2, second)
      expect(topOfLooking(), `after the ${name.split(',')[0]}`).toBe(before)
      await later.deliver(newer, 2, third)
      await later.deliver(newer, 2) // anything the placed view still asks
      await waitFor(() => expect(container.querySelector('.count-badge')?.textContent).toBe('301'))
      expect(topOfLooking(), 'after the swap').toBe(before)
      // …and the list IS the new order now: the row sits one place further down it.
      expect(container.querySelector('.logbook-row[data-index="151"] .qrz-link-call')?.textContent).toBe(looking)
    })

  it('an answer to another sort never lands in this one (the page question carries its query)', () => {
    const page = (sort: 'time' | 'call') =>
      questionKey({ kind: 'page', query: { ...DEFAULT_LOG_QUERY, sort }, offset: 0, limit: 128 })
    expect(page('time')).not.toBe(page('call'))
  })
})
