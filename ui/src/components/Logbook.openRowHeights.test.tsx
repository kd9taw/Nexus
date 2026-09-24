// @vitest-environment jsdom
//
// AN OPEN ROW'S HEIGHT GOES WITH IT WHEN THE LIST MOVES (a bug older than C17b's list, fixed here).
//
// The Logbook's list (TanStack Virtual) keeps each row's measured height by the row's PLACE, and
// measures a row only when it mounts or resizes. A row that stays mounted while its place changes —
// every row on screen, when a contact is logged above them — is never measured again. Closed rows
// are all one height, so that costs nothing. An OPEN row (its comment or note opened to full length,
// taller) did cost: after the list moved, its height stayed at its old place and the row was drawn
// into a closed row's height at its new place. Real Chrome measured it (1024×768): a 132 px gap
// where the open row had been, and a −132 px overlap covering its note where it went, until the rows
// remounted — on today's code (d5be14ea) as on the new list. The fix moves the open rows' heights to
// their new places when the list swaps to the new order.
//
// jsdom lays nothing out, so this reads what the list itself decides: each drawn row's place (its
// `translateY`) against the height the row really has. Rows are 43 px; a row whose note is open is
// 120. The list learns heights the way it does in a browser — from its ResizeObserver, which this
// test drives (a row mounting, or opening, is observed and measured).

import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ConfirmHost } from '../confirm'
import type { LoggedQso } from '../types'

const engine = vi.hoisted(() => ({ log: [] as unknown[], revision: 1 }))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    getLogDelta: vi.fn(async () => ({ revision: engine.revision, full: true, rows: engine.log })),
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

/** The list's ResizeObservers (it keeps one on the scroller and one on the rows), driven by the
 *  test: `fire()` delivers what a browser would — a size for every element each one observes (as
 *  on mounting, or on a row growing when its note opens). */
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
  // A row is 43 px; a row whose note (or comment) is open is 120. The scroller is 600 px tall.
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
  ({ ...log(i + 1, [])[i], call: `NEW${i}`, whenUnix: 1_800_000_000 + i }) as unknown as LoggedQso

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
const openNoteOf = (root: HTMLElement, call: string) =>
  fireEvent.click(
    ([...root.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[])
      .find((r) => r.querySelector('.qrz-link-call')?.textContent === call)!
      .querySelector('.log-note-flag') as HTMLElement,
  )
const indexOf = (root: HTMLElement, call: string) =>
  Number(
    ([...root.querySelectorAll('.log-rows .logbook-row')] as HTMLElement[]).find(
      (r) => r.querySelector('.qrz-link-call')?.textContent === call,
    )?.dataset.index,
  )

describe('an open row’s height goes with it when the list moves', () => {
  it('FIX: a contact logged while an open row is ON SCREEN (the list at the top)', async () => {
    const rows = log(60, [55]) // K55ABC is 5th from the top (index 4)
    engine.log = rows
    engine.revision = 1
    const { container, rerender } = render(view(1))
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(4))
    fire() // every row measured as it mounts
    openNoteOf(container, 'K55ABC')
    fire() // the open row grows, and is measured
    expect(misfits(container), 'the list must lay the open row out before anything moves').toEqual([])

    engine.log = [...rows, newContact(60)]
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(5))
    expect(misfits(container), 'after one contact logged above the open row').toEqual([])

    // …and again: the height follows the row through every move, not only the first.
    engine.log = [...(engine.log as LoggedQso[]), newContact(61)]
    engine.revision = 3
    rerender(view(3))
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(6))
    expect(misfits(container), 'after a second contact').toEqual([])
  })

  it('FIX: a contact logged while an open row is just ABOVE the view (the list held still)', async () => {
    const rows = log(300, [199]) // K199ABC is index 100 in the newest-first list
    engine.log = rows
    engine.revision = 1
    const { container, rerender } = render(view(1))
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
    openNoteOf(container, 'K199ABC')
    fire()
    // Down a few rows: the open row is above the view, still drawn (the list draws 12 beyond it).
    scrollTo(106)
    await waitFor(() => expect(container.querySelector('.logbook-row[data-index="106"]')).not.toBeNull())
    expect(indexOf(container, 'K199ABC'), 'the open row must still be drawn, above the view').toBe(100)
    expect(misfits(container)).toEqual([])

    engine.log = [...rows, newContact(300)]
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(indexOf(container, 'K199ABC')).toBe(101))
    expect(misfits(container), 'after a contact logged above the open row').toEqual([])
  })

  it('FIX: a contact deleted above an open row on screen', async () => {
    const rows = log(60, [55])
    engine.log = rows
    engine.revision = 1
    const { container, rerender } = render(view(1))
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(4))
    fire()
    openNoteOf(container, 'K55ABC')
    fire()
    engine.log = rows.filter((q) => q.call !== 'K58ABC') // index 1, above the open row
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(3))
    expect(misfits(container), 'after a contact deleted above the open row').toEqual([])
  })

  it('the check sees a misfit when there is one (its positive control)', async () => {
    const rows = log(60, [55])
    engine.log = rows
    engine.revision = 1
    const { container } = render(view(1))
    await waitFor(() => expect(indexOf(container, 'K55ABC')).toBe(4))
    fire()
    // The note opens but the list is never told the row grew: the one misfit is that row's.
    openNoteOf(container, 'K55ABC')
    expect(misfits(container)).toEqual([`4:-${OPEN_PX - ROW_PX}`])
  })
})
