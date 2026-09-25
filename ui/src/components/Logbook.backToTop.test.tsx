// @vitest-environment jsdom
//
// BACK TO THE TOP ON A NEW SORT, SEARCH OR FILTER (SPEC-2 v2 §6: "changing sort/filter resets the
// view to the top"; operator 2026-09-24, C17D "All as recommended").
//
// A new sort, search or filter is a NEW list: the rows the operator was looking at are not where
// they were, or not in it at all, so the view goes to the new list's first row — at the top of the
// list, under the search box and the column headers, which stay put (the globe band above them is
// not brought back: the box being typed in would jump). A view that has not scrolled into the list
// is already there and does not move. A newer order of the SAME list — a contact logged, a row
// deleted — is not a new list: it keeps the rows under the operator where they are (R5), as before.
//
// jsdom lays nothing out, so the geometry the list reads is given: the list starts 400 px into the
// scroller, below a 320 px globe band and an 80 px sticky block; rows are 43 px; the view 600.

import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ConfirmHost } from '../confirm'
import { t } from '../i18n'
import { setLogSource } from '../features/logSource'
import { createAskingLogSource } from '../features/askingLogSource'
import { answerFrom, type LogQuestion } from '../features/logAnswers'
import type { LoggedQso } from '../types'

const engine = vi.hoisted(() => ({ log: [] as unknown[], revision: 1 }))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    // The engine answers each question from its log (features/logAnswers.testkit).
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, engine.log as LoggedQso[], engine.revision)),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(), logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(), syncLotwReport: noop(), uploadLotwReport: noop(),
    qrzPushQso: noop(), clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((run: () => Promise<unknown>) => run()) }))

const ROW_PX = 43
const LIST_AT = 400 // where the rows start in the scroller: globe band + sticky block
const STICKY_PX = 80
const LIST_TOP = LIST_AT - STICKY_PX // the scroll that puts row 0 just under the sticky block

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      const el = this as HTMLElement
      if (el.classList?.contains('logbook-row')) return ROW_PX
      if (el.classList?.contains('log-sticky')) return STICKY_PX
      return 600
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get() {
      return (this as HTMLElement).classList?.contains('log-rows') ? LIST_AT : 0
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
beforeEach(() => {
  engine.log = log(300)
  engine.revision = 1
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

/** `n` contacts, newest last in the log; every third one confirmed (so the filter chip changes
 *  the list). */
const log = (n: number) =>
  Array.from({ length: n }, (_, i) =>
    ({
      id: `id-${i}`,
      call: `K${i}ABC`,
      grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12',
      name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000 + i * 60,
      confirmed: i % 3 === 0, awardConfirmed: i % 3 === 0, qslRcvd: null, qslSent: null, ota: null,
    }) as unknown as LoggedQso,
  )
const view = (logTick: number) => (
  <>
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={logTick} />
    <ConfirmHost />
  </>
)
const scroller = () => document.querySelector('.log-scroll') as HTMLElement
const scrollTo = (top: number) =>
  act(() => {
    scroller().scrollTop = top
    scroller().dispatchEvent(new Event('scroll'))
  })
const drawn = (index: number) => document.querySelector(`.log-rows .logbook-row[data-index="${index}"]`)
/** Deep in the list: row 150 at the top of the view. */
const DEEP = LIST_AT + 150 * ROW_PX - STICKY_PX

async function openDeep() {
  const r = render(view(1))
  await waitFor(() => expect(drawn(0)).not.toBeNull())
  scrollTo(DEEP)
  // Row 150 DRAWN, not only placed: its page is its own answer, and a row whose page is still on
  // its way is a placeholder at the same index.
  await waitFor(() => expect(drawn(150)?.classList.contains('placeholder')).toBe(false))
  return r
}

describe('a new sort, search or filter goes back to the top of the list', () => {
  it('FIX: a new SORT', async () => {
    await openDeep()
    fireEvent.click(screen.getByRole('columnheader', { name: t('logbook.column.call') }))
    await waitFor(() => expect(scroller().scrollTop, 'the view, after the sort').toBe(LIST_TOP))
  })

  it('FIX: a new SEARCH', async () => {
    await openDeep()
    fireEvent.change(document.querySelector('.log-search') as HTMLInputElement, { target: { value: 'ABC' } })
    await waitFor(() => expect(scroller().scrollTop, 'the view, after the search').toBe(LIST_TOP))
  })

  it('FIX: the needs-confirmation FILTER', async () => {
    await openDeep()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.filter.needsConfirmation.label') }))
    await waitFor(() => expect(scroller().scrollTop, 'the view, after the filter').toBe(LIST_TOP))
  })

  it('a view that has not scrolled into the list stays where it is', async () => {
    render(view(1))
    await waitFor(() => expect(drawn(0)).not.toBeNull())
    scrollTo(100) // the globe band partly in view, the list's top below it
    fireEvent.click(screen.getByRole('columnheader', { name: t('logbook.column.call') }))
    await waitFor(() => expect(drawn(0)?.textContent).toContain('K0ABC'))
    expect(scroller().scrollTop).toBe(100)
  })

  it('a contact logged is not a new list: the rows under the operator stay put (R5), and a sort then goes to the top', async () => {
    const { rerender } = await openDeep()
    const looking = drawn(150)!.querySelector('.qrz-link-call')!.textContent
    engine.log = [...(engine.log as LoggedQso[]), { ...(engine.log as LoggedQso[])[0], id: 'id-new', call: 'NEW1', whenUnix: 1_800_000_000 }]
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(drawn(151)?.querySelector('.qrz-link-call')?.textContent).toBe(looking))
    expect(scroller().scrollTop, 'held, one row further down').toBe(DEEP + ROW_PX)

    fireEvent.click(screen.getByRole('columnheader', { name: t('logbook.column.call') }))
    await waitFor(() => expect(scroller().scrollTop, 'the view, after the sort').toBe(LIST_TOP))
  })

  it('a contact logged the moment after a new sort keeps the view at the top', async () => {
    // Before the scroll to the top has been reported (no scroll event here), a newer order arrives:
    // the view shows the new list's first row, so it stays at the top — the rows noted down the
    // OLD list must not pull it back there.
    const { rerender } = await openDeep()
    // The list's own end-of-scroll report of the scroll to row 150 comes 150 ms after it (jsdom has
    // no `scrollend`); a browser has long delivered it, and this test is not about it.
    await act(() => new Promise((resolve) => setTimeout(resolve, 200)))
    fireEvent.click(screen.getByRole('columnheader', { name: t('logbook.column.call') }))
    await waitFor(() => expect(scroller().scrollTop).toBe(LIST_TOP))
    engine.log = [...(engine.log as LoggedQso[]), { ...(engine.log as LoggedQso[])[0], id: 'id-new', call: 'AAA1', whenUnix: 1_800_000_000 }]
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(document.querySelector('.log-rows')?.getAttribute('style')).toContain(`${301 * ROW_PX}px`))
    expect(scroller().scrollTop, 'still at the top').toBe(LIST_TOP)
  })
})

// A NEW LIST STAYS OFF SCREEN UNTIL ITS FIRST PAGE IS HERE. A source that answers one question at a
// time (C17a's) had the list EMPTY between a new sort, search or filter and that answer: a blank frame
// or more, the "no contacts match" line with it — and, in a browser, the scroll pulled back up to the
// globe band, the search box dropping down the screen under the operator's typing. Now the list on
// screen stays until the new one can be drawn, as a newer order of one list already did (the swap).
// The whole-log source answers in the same render, so there nothing waits and nothing changes.
describe('a new sort, search or filter keeps the list on screen until its first page is here', () => {
  /** An asking source; the first page of every query but the first is held until released. */
  function askingWithHeldFirstPages() {
    const rows = log(300)
    const out: (() => void)[] = []
    let first: string | null = null
    setLogSource(
      createAskingLogSource(async (q) => {
        if (q.kind === 'page' && q.offset === 0) {
          const key = JSON.stringify(q.query)
          first ??= key
          if (key !== first) await new Promise<void>((resolve) => out.push(resolve))
        }
        return answerFrom(rows, q, 1)
      }),
    )
    return { release: () => act(async () => { for (const r of out.splice(0)) r() }), held: () => out.length }
  }
  const calls = () =>
    [...document.querySelectorAll('.log-rows .logbook-row:not(.placeholder)')].map((r) => r.querySelector('.qrz-link-call')?.textContent)

  for (const [name, change] of [
    ['SORT', () => fireEvent.click(screen.getByRole('columnheader', { name: t('logbook.column.call') }))],
    ['SEARCH', () => fireEvent.change(document.querySelector('.log-search') as HTMLInputElement, { target: { value: 'K1' } })],
    ['FILTER', () => fireEvent.click(screen.getByRole('button', { name: t('logbook.filter.needsConfirmation.label') }))],
  ] as const) {
    it(`FIX: a new ${name} on an asking source — the old list until the new one is here, then its top`, async () => {
      const source = askingWithHeldFirstPages()
      render(view(1))
      await waitFor(() => expect(drawn(150) ?? drawn(0)).not.toBeNull())
      scrollTo(DEEP)
      await waitFor(() => expect(drawn(150)?.classList.contains('placeholder')).toBe(false))
      const before = calls()
      change()
      await waitFor(() => expect(source.held(), 'the new list’s first page asked for').toBeGreaterThan(0))
      expect(document.querySelector('.log-rows'), 'the list, while the new one is on its way').not.toBeNull()
      expect(calls(), 'the rows on screen, while the new list is on its way').toEqual(before)
      expect(document.querySelector('.log-scroll > .empty'), 'no "no match" line meanwhile').toBeNull()
      expect(scroller().scrollTop, 'the view, meanwhile').toBe(DEEP)

      await source.release()
      await waitFor(() => expect(scroller().scrollTop, 'the new list, at its top').toBe(LIST_TOP))
      act(() => {
        scroller().dispatchEvent(new Event('scroll')) // as a browser reports the list's own scroll
      })
      await waitFor(() => expect(drawn(0)?.classList.contains('placeholder')).toBe(false))
      expect(calls()).not.toEqual(before)
    })
  }

  it('the whole-log source: the list is never taken off screen by a new search (as always)', async () => {
    await openDeep()
    const removed: string[] = []
    const watch = new MutationObserver((ms) => {
      for (const m of ms) for (const n of m.removedNodes) if ((n as Element).classList?.contains('log-rows')) removed.push('log-rows')
    })
    watch.observe(scroller(), { childList: true })
    fireEvent.change(document.querySelector('.log-search') as HTMLInputElement, { target: { value: 'K1' } })
    await waitFor(() => expect(scroller().scrollTop).toBe(LIST_TOP))
    watch.disconnect()
    expect(removed).toEqual([])
  })
})
