// @vitest-environment jsdom
//
// The Logbook's field filters, saved filters and multi-key sort, from the operator's side: what a
// field typed, a filter recalled or a Shift-click on a header puts in the question the engine is
// asked. The engine answers the filter (src-tauri `log_queries` tests); here it is enough that the
// window asks for it — and never filters a row itself.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, screen, cleanup, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 120 })
})

const engineLog = vi.hoisted(() => vi.fn())
const settings = vi.hoisted(() => ({ value: {} as Record<string, unknown> }))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    getSettings: vi.fn(async () => settings.value),
    setLogbookFilters: vi.fn(async () => {}),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(() => Promise.resolve({ kind: 'applied' })),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(() => Promise.resolve({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

const CONTACT = {
  id: 'id-1', call: 'DL1ABC', grid: 'JO31', band: '20m', freqMhz: 14.074, mode: 'FT8',
  rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
  country: 'Fed. Rep. of Germany', whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false,
}

async function renderLog() {
  engineLog.mockResolvedValue([CONTACT])
  const utils = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() => expect(utils.container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
  return utils
}

/** The query of the latest page the window asked for. */
const lastPageQuery = () => {
  const pages = vi.mocked(api.askLog).mock.calls.map(([q]) => q).filter((q) => q.kind === 'page')
  const last = pages[pages.length - 1]
  return last?.kind === 'page' ? last.query : undefined
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  settings.value = {}
})

describe('field filters reach the engine', () => {
  it('no filter asks exactly the query it always did', async () => {
    await renderLog()
    expect(lastPageQuery()).toEqual({ sort: 'time', asc: false, search: '', needsConfirmOnly: false })
  })

  it('each field set is in the question, and clearing them takes it out again', async () => {
    await renderLog()
    fireEvent.click(screen.getByRole('button', { name: 'Filters' }))
    fireEvent.change(screen.getByLabelText('Call'), { target: { value: 'DL*' } })
    fireEvent.change(screen.getByLabelText('Band'), { target: { value: '20m' } })
    fireEvent.change(screen.getByLabelText('Award need'), { target: { value: 'dxcc' } })
    fireEvent.change(screen.getByLabelText('LoTW'), { target: { value: 'notSent' } })
    fireEvent.change(screen.getByLabelText('Comment / name / QTH'), { target: { value: 'Berlin' } })
    await waitFor(() =>
      expect(lastPageQuery()?.filter).toEqual({ call: 'DL*', band: '20m', need: 'dxcc', qsl: { lotw: 'notSent' }, text: 'Berlin' }),
    )
    expect(screen.getByRole('button', { name: 'Filters (5)' })).toBeTruthy()
    fireEvent.click(within(screen.getByRole('group', { name: 'Log filters' })).getByRole('button', { name: 'Clear all' }))
    // Back to the unfiltered list, which the window already holds an answer for.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Filters' })).toBeTruthy())
  })

  it('the window never drops a row the engine sent: filtering is the engine’s', async () => {
    const { container } = await renderLog()
    fireEvent.click(screen.getByRole('button', { name: 'Filters' }))
    fireEvent.change(screen.getByLabelText('Band'), { target: { value: '40m' } })
    await waitFor(() => expect(lastPageQuery()?.filter).toEqual({ band: '40m' }))
    // The test engine ignores the filter and sends the 20 m row: it is shown.
    expect(container.querySelectorAll('.logbook-row:not(.head):not(.placeholder)')).toHaveLength(1)
  })
})

describe('saved filters', () => {
  it('saves under a name through the one writer, and recalls a saved one', async () => {
    settings.value = { logbookFilters: [{ name: 'Parks', filter: { reference: 'K-' } }] }
    await renderLog()
    fireEvent.click(screen.getByRole('button', { name: 'Filters' }))
    const saved = screen.getByLabelText('Saved filters') as HTMLSelectElement
    await waitFor(() => expect(saved.options).toHaveLength(2))
    fireEvent.change(saved, { target: { value: 'Parks' } })
    await waitFor(() => expect(lastPageQuery()?.filter).toEqual({ reference: 'K-' }))

    fireEvent.change(screen.getByLabelText('Band'), { target: { value: '20m' } })
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Parks on 20' } })
    fireEvent.click(within(screen.getByRole('group', { name: 'Log filters' })).getByRole('button', { name: 'Save' }))
    expect(api.setLogbookFilters).toHaveBeenLastCalledWith([
      { name: 'Parks', filter: { reference: 'K-' } },
      { name: 'Parks on 20', filter: { reference: 'K-', band: '20m' } },
    ])
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Twenty' } })
    fireEvent.click(within(screen.getByRole('group', { name: 'Log filters' })).getByRole('button', { name: 'Rename' }))
    expect(api.setLogbookFilters).toHaveBeenLastCalledWith([
      { name: 'Parks', filter: { reference: 'K-' } },
      { name: 'Twenty', filter: { reference: 'K-', band: '20m' } },
    ])
    fireEvent.click(within(screen.getByRole('group', { name: 'Log filters' })).getByRole('button', { name: 'Delete' }))
    expect(api.setLogbookFilters).toHaveBeenLastCalledWith([{ name: 'Parks', filter: { reference: 'K-' } }])
  })
})

describe('multi-key sort', () => {
  it('Shift-click on headers adds keys after the first, in order', async () => {
    await renderLog()
    const header = (name: string) => screen.getAllByRole('columnheader').find((h) => h.firstChild?.textContent?.trim() === name)!
    fireEvent.click(header('Band'))
    fireEvent.click(header('Mode'), { shiftKey: true })
    fireEvent.click(header('Call'), { shiftKey: true })
    await waitFor(() =>
      expect(lastPageQuery()).toMatchObject({
        sort: 'band',
        asc: false,
        then: [
          { sort: 'mode', asc: true },
          { sort: 'call', asc: true },
        ],
      }),
    )
    expect(header('Band').getAttribute('aria-sort')).toBe('descending')
    expect(header('Call').querySelector('.log-th-place')?.textContent).toBe('3')
    // A plain click orders by one column again.
    fireEvent.click(header('Call'))
    await waitFor(() => expect(lastPageQuery()?.then).toBeUndefined())
  })
})
