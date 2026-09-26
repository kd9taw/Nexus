// @vitest-environment jsdom
//
// THE REMOTE OBSERVER'S LIST: an open row across a contact the station logged (a bug older than
// C17b's list, fixed here).
//
// A Remote browser's Logbook shows the page of rows the station sent, keyed by id, with no swap. A
// contact logged at the station reaches it when the page is fetched again — after this browser's
// own log, edit or delete, or on Refresh — and the list empties while the page loads, so every row
// mounts again. But a row mounting at a place the old page measured takes that place's height until
// the browser reports its own, a frame later: the row now at the open row's old place was drawn at
// the open row's height, and the open row, a place further down, over the rows below it. Real
// Chrome: one frame of a 139 px gap and overlap at 1024×768 (109 px at 1920×1080), in 3 runs of 3
// at each size. The fix measures the new page afresh while a row is open, as it does a new sort or
// search on the desktop.
//
// This drives that path through the desktop's check (Logbook.openRowHeights.test.tsx): each drawn
// row's place (its `translateY`) against its real height, as first drawn — before the browser's
// report — and after it. Rows are 43 px; a row whose comment is open is 120. The ResizeObservers
// are the test's, as there.

import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { waitOutTheListsScrollTimer } from './Logbook.testkit'
import { ConfirmHost } from '../confirm'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import type { QueryPage } from '../remote-web/application-query-protocol'
import { t } from '../i18n'

const ROW_PX = 43
const OPEN_PX = 120

/** The list's ResizeObservers, as a browser's: `fire()` reports an element when it is first
 *  observed and whenever its size has changed since — never a row that only moved. */
const observers = new Set<{ cb: (entries: { target: Element }[]) => void; els: Map<Element, number | undefined> }>()
const fire = () =>
  act(() => {
    for (const o of observers) {
      const changed = [...o.els].filter(([el, last]) => el.isConnected && (el as HTMLElement).offsetHeight !== last)
      for (const [el] of changed) o.els.set(el, (el as HTMLElement).offsetHeight)
      if (changed.length) o.cb(changed.map(([target]) => ({ target })))
    }
  })

beforeAll(() => {
  globalThis.ResizeObserver = class {
    private o: { cb: (entries: { target: Element }[]) => void; els: Map<Element, number | undefined> }
    constructor(cb: (entries: { target: Element }[]) => void) {
      this.o = { cb, els: new Map() }
      observers.add(this.o)
    }
    observe(el: Element) {
      this.o.els.set(el, undefined)
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
  vi.restoreAllMocks()
})
afterAll(waitOutTheListsScrollTimer)

/** The station's rows, newest first as it sends them: contact `i` is `K{i}ABC`, id `id-{i}`;
 *  `withComment` carry a comment, whose text opens the row. */
const contact = (i: number, comment: boolean) => ({
  id: `id-${i}`, call: `K${i}ABC`, band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: 1_700_000_000 + i * 60,
  confirmed: false, awardConfirmed: false, grid: 'FN31', country: 'United States', rstSent: '-10', rstRcvd: '-12',
  comment: comment ? 'Worked him on the club net, then again on 40 m the same evening, QSL via bureau.' : null,
})
const stationLog = (n: number, withComment: number[]) =>
  Array.from({ length: n }, (_, k) => n - 1 - k).map((i) => contact(i, withComment.includes(i)))
const page = (rows: object[]): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), collection: 'log',
  snapshotId: crypto.randomUUID(), offset: 0, total: rows.length, retained: rows.length, nextCursor: null, ageMs: 0,
  rows: rows as QueryPage['rows'], meta: {} })

function observe(rows: object[]) {
  const source = { page: vi.fn(async () => page(rows)) }
  const view = render(
    <StationControlContext.Provider value={false}>
      <RemoteCollectionsContext.Provider value={source as unknown as RemoteCollections}>
        <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
      </RemoteCollectionsContext.Provider>
      <ConfirmHost />
    </StationControlContext.Provider>,
  )
  return { source, container: view.container }
}

const rowY = (el: HTMLElement) => Number(/translateY\((-?[\d.]+)px\)/.exec(el.style.transform)?.[1] ?? NaN)
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
const openCommentOf = (root: HTMLElement, call: string) =>
  fireEvent.click(rowOf(root, call)!.querySelector('.log-note-text') as HTMLElement)
/** The station logs a contact, and this browser fetches the page again (its Refresh button — the
 *  same `refresh` its own log, edit and delete use). */
const refreshWith = (source: { page: ReturnType<typeof vi.fn> }, rows: object[]) => {
  source.page.mockImplementation(async () => page(rows))
  fireEvent.click(screen.getByRole('button', { name: t('remote.refreshCollection') }))
}

describe('the Remote observer: an open row across a contact the station logged', () => {
  it('FIX: an open row on screen keeps its height when the page comes back with a contact above it', async () => {
    const rows = stationLog(60, [55]) // K55ABC is 5th from the top (index 4)
    const { source, container } = observe(rows)
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(4))
    fire()
    openCommentOf(container, 'K55ABC')
    fire()
    expect(misfits(container), 'laid out before anything moves').toEqual([])

    refreshWith(source, [contact(60, false), ...rows])
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(5), { timeout: 4000 })
    expect(rowOf(container, 'K55ABC')!.querySelector('.log-note.expanded'), 'still open').not.toBeNull()
    expect(misfits(container), 'the page as first drawn, one contact above the open row').toEqual([])
    fire() // the rows that mounted are observed, as a browser does next
    expect(misfits(container), 'once the rows that mounted are measured').toEqual([])
  })

  it('FIX: an open row just above the view keeps its height too', async () => {
    const rows = stationLog(300, [199]) // K199ABC is index 100
    const { source, container } = observe(rows)
    await waitFor(() => expect(container.querySelector('.log-rows .logbook-row')).not.toBeNull())
    const sc = container.querySelector('.log-scroll') as HTMLElement
    const scrollTo = (index: number) =>
      act(() => {
        sc.scrollTop = index * ROW_PX
        sc.dispatchEvent(new Event('scroll'))
      })
    scrollTo(96)
    await waitFor(() => expect(indexOf(container, 'K199ABC')).toBe(100))
    fire()
    openCommentOf(container, 'K199ABC')
    fire()
    scrollTo(106)
    await waitFor(() => expect(container.querySelector('.logbook-row[data-index="106"]')).not.toBeNull())
    expect(indexOf(container, 'K199ABC'), 'drawn, above the view').toBe(100)
    expect(misfits(container)).toEqual([])

    refreshWith(source, [contact(300, false), ...rows])
    await waitFor(() => expect(indexOf(container, 'K199ABC')).toBe(101), { timeout: 4000 })
    expect(misfits(container), 'the page as first drawn, one contact above the open row').toEqual([])
    fire()
    expect(misfits(container), 'once the rows that mounted are measured').toEqual([])
  })

  it('the check sees a misfit on this list when there is one (its positive control)', async () => {
    const { container } = observe(stationLog(60, [55]))
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(4))
    fire()
    // The comment opens but the list is never told the row grew.
    openCommentOf(container, 'K55ABC')
    expect(misfits(container)).toEqual([`4:-${OPEN_PX - ROW_PX}`])
  })
})
