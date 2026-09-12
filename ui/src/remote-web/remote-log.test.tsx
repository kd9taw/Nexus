// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, act } from '@testing-library/react'
import { Logbook } from '../components/Logbook'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import type { RemoteCollections } from './collections'
import type { QueryArgs, QueryPage } from './application-query-protocol'
import * as api from '../api'

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 1000 })
})
afterEach(() => { cleanup(); vi.restoreAllMocks() })
const id = '10000000-0000-4000-8000-000000000001'
const qso = (call: string) => ({ call, band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: 1788940800,
  confirmed: false, awardConfirmed: false, grid: 'FN31', country: 'United States', rstSent: '-10', rstRcvd: '-12' })
const page = (calls: string[], offset = 0, total = 3, snapshotId = id): QueryPage => ({ type: 'applicationPage', requestId: id,
  collection: 'log', snapshotId, offset, total, retained: total, nextCursor: offset + calls.length < total ? `${snapshotId}:${offset + calls.length}` : null,
  ageMs: 0, rows: calls.map(qso), meta: {} })
function mount(source: { page: (args: QueryArgs) => Promise<QueryPage> }) {
  return render(<StationControlContext.Provider value={false}><RemoteCollectionsContext.Provider value={source as RemoteCollections}>
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
  </RemoteCollectionsContext.Provider></StationControlContext.Provider>)
}
it('renders the existing log table with explicit pages and no full-log fetch or write controls', async () => {
  const full = vi.spyOn(api, 'getLog')
  const source = { page: vi.fn(async (args: QueryArgs) => args.cursor ? page(['K1LAST'], 2) : page(['W1AW', 'K1ABC'])) }
  mount(source)
  await screen.findByText('W1AW')
  expect(screen.getByText('Contacts 1–2 · Matches: 3')).toBeTruthy()
  expect(document.querySelectorAll('.log-rowactions button, .log-rowactions select, .log-actions').length).toBe(0)
  expect(full).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Next' }))
  await screen.findByText('K1LAST')
  expect(screen.getByText('Contacts 3–3 · Matches: 3')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Previous' }))
  await screen.findByText('W1AW')
  expect(source.page).toHaveBeenCalledTimes(2)
})
it('a delayed old page cannot replace the results of a new search', async () => {
  let finish!: (value: QueryPage) => void
  const pending = new Promise<QueryPage>(resolve => { finish = resolve })
  const other = '20000000-0000-4000-8000-000000000002'
  mount({ page: async args => args.search ? page(['ZL1OLD'], 0, 1, other) : args.cursor ? pending : page(['W1AW', 'K1ABC']) })
  await screen.findByText('W1AW')
  fireEvent.click(screen.getByRole('button', { name: 'Next' }))
  fireEvent.change(screen.getByPlaceholderText('Search call, country, grid, band or mode'), { target: { value: 'ZL1OLD' } })
  await screen.findByText('ZL1OLD')
  await act(async () => { finish(page(['K1LAST'], 2)); await pending })
  await waitFor(() => expect(screen.queryByText('K1LAST')).toBeNull())
  expect(screen.getByText('Contacts 1–1 · Matches: 1')).toBeTruthy()
})
