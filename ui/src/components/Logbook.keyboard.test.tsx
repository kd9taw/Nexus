// @vitest-environment jsdom
//
// THE LOGBOOK KEYBOARD GRID (SPEC-2 v2 §6; operator 2026-09-24, C17D "All as recommended").
//
// The list is a grid with ONE Tab stop — a row — and the keys move it, grid-local (no global
// shortcuts): ↑/↓ a row, Page Up/Down a visible page, Home/End the first/last contact (scrolled in,
// its page fetched, then focused); Enter edits the contact, Delete asks the existing delete question,
// Escape closes the form (the question closes itself). A key on a row's own control (its QSL menu,
// its buttons) stays that control's. The row that has the stop is held by id, so a contact logged
// above it does not move the operator's place.
//
// jsdom lays nothing out, so the geometry the list reads is given: rows 43 px, the list 400 px into
// a 600 px scroller below an 80 px sticky block — twelve rows to a page.

import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { ConfirmHost } from '../confirm'
import { deleteQsoById, type RowRef } from '../api'
import { t } from '../i18n'
import { setLogSource } from '../features/logSource'
import { createAskingLogSource } from '../features/askingLogSource'
import { answerFrom, type LogQuestion } from '../features/logAnswers'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import type { QueryPage } from '../remote-web/application-query-protocol'
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
const LIST_AT = 400
const STICKY_PX = 80
const N = 300

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
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get() {
      return (this as HTMLElement).classList?.contains('log-rows') ? LIST_AT : 0
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
beforeEach(() => {
  engine.log = log(N)
  engine.revision = 1
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})
// The list's end-of-scroll report fires 150 ms after a scroll, from a timer it does not clear on
// unmount; let the last one land while this file's window exists.
afterAll(() => new Promise((resolve) => setTimeout(resolve, 200)))

/** `n` contacts, newest last in the log: newest-first, index k is K{n-1-k}ABC. */
const log = (n: number) =>
  Array.from({ length: n }, (_, i) =>
    ({
      id: `id-${i}`,
      call: `K${i}ABC`,
      grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12',
      name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000 + i * 60,
      confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null,
    }) as unknown as LoggedQso,
  )
const callAt = (index: number) => `K${N - 1 - index}ABC`
const view = (logTick: number) => (
  <>
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={logTick} />
    <ConfirmHost />
  </>
)
const rowAt = (index: number) =>
  document.querySelector(`.log-rows .logbook-row[data-index="${index}"]`) as HTMLElement | null
const focusedIndex = () => {
  const el = document.activeElement as HTMLElement | null
  return el?.classList.contains('logbook-row') ? Number(el.dataset.index) : null
}
const press = (key: string, on: Element = document.activeElement ?? document.body) => fireEvent.keyDown(on, { key })

async function openAt(index: number) {
  const r = render(view(1))
  await waitFor(() => expect(rowAt(0)).not.toBeNull())
  act(() => rowAt(index)!.focus())
  return r
}

describe('the Logbook is a keyboard grid', () => {
  it('a grid with one Tab stop, its rows numbered, and its keys described', async () => {
    render(view(1))
    await waitFor(() => expect(rowAt(0)).not.toBeNull())
    const grid = screen.getByRole('grid')
    expect(grid.getAttribute('aria-rowcount')).toBe(String(N + 1))
    const stops = [...document.querySelectorAll('.log-rows .logbook-row')].filter((r) => (r as HTMLElement).tabIndex === 0)
    expect(stops.map((r) => (r as HTMLElement).dataset.index), 'the one Tab stop').toEqual(['0'])
    expect(rowAt(5)!.getAttribute('aria-rowindex')).toBe('7') // the header row is row 1
    const help = document.getElementById(grid.getAttribute('aria-describedby')!)!.textContent
    for (const key of ['logbook.keys.move', 'logbook.keys.edit', 'logbook.keys.delete', 'logbook.keys.close'] as const)
      expect(help).toContain(t(key))
  })

  it('↓ and ↑ move a row at a time, and stop at the ends', async () => {
    await openAt(0)
    press('ArrowDown')
    await waitFor(() => expect(focusedIndex()).toBe(1))
    press('ArrowDown')
    await waitFor(() => expect(focusedIndex()).toBe(2))
    press('ArrowUp')
    await waitFor(() => expect(focusedIndex()).toBe(1))
    press('ArrowUp')
    press('ArrowUp')
    await waitFor(() => expect(focusedIndex()).toBe(0))
    expect(rowAt(0)!.tabIndex, 'the Tab stop moved with it').toBe(0)
  })

  it('Page Down / Page Up move one visible page', async () => {
    await openAt(0)
    press('PageDown')
    await waitFor(() => expect(focusedIndex()).toBe(12))
    press('PageUp')
    await waitFor(() => expect(focusedIndex()).toBe(0))
  })

  it('End and Home jump to the last and the first contact, scrolled in and drawn', async () => {
    await openAt(3)
    press('End')
    await waitFor(() => expect(focusedIndex()).toBe(N - 1))
    // DRAWN: the focus lands on the last row's place at once, and that place is a placeholder
    // until its page's answer is in — so wait for the contact, not only the focus.
    await waitFor(() => expect(document.activeElement?.textContent).toContain(callAt(N - 1)))
    press('Home')
    await waitFor(() => expect(focusedIndex()).toBe(0))
  })

  it('Enter edits the contact, in the form; Escape there closes it and gives the row back', async () => {
    await openAt(3)
    press('Enter')
    await waitFor(() => expect(document.querySelector('.logbook-form')).not.toBeNull())
    const form = document.querySelector('.logbook-form') as HTMLElement
    expect((form.querySelector('input') as HTMLInputElement).value).toBe(callAt(3))
    expect(form.contains(document.activeElement), 'the keyboard is in the form').toBe(true)
    press('Escape')
    await waitFor(() => expect(document.querySelector('.logbook-form')).toBeNull())
    await waitFor(() => expect(focusedIndex()).toBe(3))
  })

  it('Delete asks the existing delete question — and deletes nothing unless answered yes', async () => {
    await openAt(3)
    press('Delete')
    const dialog = await screen.findByRole('dialog')
    expect(dialog.textContent).toContain(t('logbook.delete.heading', { call: callAt(3), band: '20m' }))
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(deleteQsoById).not.toHaveBeenCalled()
    await waitFor(() => expect(focusedIndex(), 'the keyboard back on the contact').toBe(3))
  })

  it('Delete answered yes: the keyboard lands on the contact that takes its place', async () => {
    vi.mocked(deleteQsoById).mockImplementation(async (target: RowRef) => {
      engine.log = (engine.log as LoggedQso[]).filter((r) => r.id !== target.id)
      engine.revision += 1
      return { kind: 'deleted' }
    })
    await openAt(3)
    press('Delete')
    await screen.findByRole('dialog')
    fireEvent.click(screen.getByRole('button', { name: t('logbook.delete.confirm') }))
    await waitFor(() => expect(deleteQsoById).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(document.activeElement?.textContent).toContain(callAt(4)))
    expect(focusedIndex(), 'the same place in the list').toBe(3)
  })

  it('the row’s ✕ answered yes: the keyboard goes to the ✕ of the contact that takes its place', async () => {
    vi.mocked(deleteQsoById).mockImplementation(async (target: RowRef) => {
      engine.log = (engine.log as LoggedQso[]).filter((r) => r.id !== target.id)
      engine.revision += 1
      return { kind: 'deleted' }
    })
    await openAt(3)
    const del = screen.getByRole('button', { name: t('logbook.row.delete', { call: callAt(3) }) })
    act(() => del.focus())
    fireEvent.click(del)
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.delete.confirm') }))
    await waitFor(() => expect(del.isConnected).toBe(false))
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('button', { name: t('logbook.row.delete', { call: callAt(4) }) })),
    )
  })

  it('a row opened in the detail view (double-click) has the keyboard again when the view closes', async () => {
    await openAt(3)
    fireEvent.doubleClick(rowAt(3)!)
    await screen.findByRole('dialog', { name: new RegExp(callAt(3)) })
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(focusedIndex()).toBe(3))
  })

  it('Escape on the grid closes the edit form', async () => {
    await openAt(2)
    fireEvent.click(screen.getByRole('button', { name: t('logbook.row.edit', { call: callAt(2) }) }))
    await waitFor(() => expect(document.querySelector('.logbook-form')).not.toBeNull())
    act(() => rowAt(2)!.focus())
    press('Escape')
    await waitFor(() => expect(document.querySelector('.logbook-form')).toBeNull())
  })

  it('a key on a row’s own control stays that control’s; a key outside the grid is not the grid’s', async () => {
    await openAt(2)
    const edit = screen.getByRole('button', { name: t('logbook.row.edit', { call: callAt(2) }) })
    act(() => edit.focus())
    press('ArrowDown', edit)
    press('Delete', edit)
    expect(document.activeElement, 'focus stays on the row’s button').toBe(edit)
    expect(screen.queryByRole('dialog')).toBeNull()

    const search = document.querySelector('.log-search') as HTMLInputElement
    act(() => search.focus())
    press('ArrowDown', search)
    press('Enter', search)
    press('Delete', search)
    expect(document.activeElement).toBe(search)
    expect(document.querySelector('.logbook-form')).toBeNull()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('the row with the Tab stop is held by id: a contact logged above does not move the operator’s place', async () => {
    const { rerender } = await openAt(5)
    const here = document.activeElement as HTMLElement
    engine.log = [...(engine.log as LoggedQso[]), { ...(engine.log as LoggedQso[])[0], id: 'id-new', call: 'NEW1', whenUnix: 1_800_000_000 }]
    engine.revision = 2
    rerender(view(2))
    await waitFor(() => expect(here.dataset.index).toBe('6'))
    expect(document.activeElement, 'still the same contact').toBe(here)
    expect(here.tabIndex).toBe(0)
    press('ArrowDown')
    await waitFor(() => expect(focusedIndex()).toBe(7))
  })

  it('a row whose page has not landed holds the focus as a placeholder, then the row takes it', async () => {
    // An asking source (C17a's shape) whose later pages are held back.
    const rows = log(N)
    let release = () => {}
    const held = new Promise<void>((resolve) => (release = resolve))
    setLogSource(
      createAskingLogSource(async (q) => {
        if (q.kind === 'page' && q.offset > 0) await held
        return answerFrom(rows, q, 1)
      }),
    )
    render(view(1))
    await waitFor(() => expect(rowAt(0)?.classList.contains('placeholder')).toBe(false))
    act(() => rowAt(0)!.focus())
    press('End')
    await waitFor(() => expect((document.activeElement as HTMLElement).dataset.index).toBe(String(N - 1)))
    expect(document.activeElement?.classList.contains('placeholder'), 'its page is on its way').toBe(true)
    await act(async () => release())
    await waitFor(() => expect(focusedIndex()).toBe(N - 1))
    expect(document.activeElement?.classList.contains('placeholder')).toBe(false)
    expect(document.activeElement?.textContent).toContain(callAt(N - 1))
  })

  it('a Remote browser without the station’s edit gets the moves, and no edit or delete', async () => {
    const page = (): QueryPage => ({ type: 'applicationPage', requestId: 'r', collection: 'log', snapshotId: 's', offset: 0,
      total: 20, retained: 20, nextCursor: null, ageMs: 0, meta: {},
      rows: [...log(20)].reverse() as unknown as QueryPage['rows'] })
    render(
      <StationControlContext.Provider value={false}>
        <RemoteCollectionsContext.Provider value={{ page: async () => page() } as unknown as RemoteCollections}>
          <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
        </RemoteCollectionsContext.Provider>
        <ConfirmHost />
      </StationControlContext.Provider>,
    )
    await waitFor(() => expect(rowAt(0)).not.toBeNull(), { timeout: 3000 })
    act(() => rowAt(0)!.focus())
    press('ArrowDown')
    await waitFor(() => expect(focusedIndex()).toBe(1))
    press('Enter')
    press('Delete')
    expect(document.querySelector('.logbook-form')).toBeNull()
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
