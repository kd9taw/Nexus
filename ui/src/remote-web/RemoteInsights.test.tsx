// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteInsights } from './RemoteInsights'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import type { ApplicationClient } from './application-client'
import type { InsightCollection, QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/insights.json'

afterEach(() => { cleanup(); vi.useRealTimers(); localStorage.clear() })
const page = (kind = 'statistics'): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: kind as InsightCollection, offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 0, source: { logCount: 2301, ...structuredClone(fixture) } } })
function setup(invoke = vi.fn(async (_command: string, args?: Record<string, unknown>): Promise<unknown> => page(String(args?.collection)))) {
  const client = { invoke, supports: (c: string) => c === 'get_remote_insights', getPhase: () => 'ready' } as unknown as ApplicationClient
  const source = new RemoteCollections(client)
  const view = (kind: InsightCollection = 'statistics', available = true) => <StationDataContext.Provider value={available}>
    <RemoteCollectionsContext.Provider value={source}><RemoteInsights kind={kind} /></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider>
  return { ...render(view()), view, invoke }
}
it('uses the actual Statistics and Awards views with only explicit summary reads', async () => {
  const forbidden = vi.fn(async () => { throw new Error('no generic desktop reads') })
  const uninstall = installApplicationTransport({ kind: 'remote', invoke: forbidden })
  try {
    const test = setup()
    await screen.findByText('2012')
    expect(screen.getByText(/All 2301 station contacts\. Summary captured \d+ s ago\./)).toBeTruthy()
    expect(test.container.querySelector('.stats-view')).toBeTruthy()
    test.rerender(test.view('awards'))
    await screen.findByText('Japan')
    expect(test.container.querySelector('.awards-journey .awards')).toBeTruthy()
    expect(test.invoke.mock.calls.map(([command, args]) => [command, args?.collection])).toEqual([
      ['get_remote_insights', 'statistics'], ['get_remote_insights', 'awards']])
    expect(forbidden).not.toHaveBeenCalled()
  } finally { uninstall() }
})
it('clears on station loss, discards late navigation replies, and reads again on recovery', async () => {
  let resolve: (v: unknown) => void = () => {}
  const invoke = vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise(r => { resolve = r })).mockResolvedValue(page())
  const test = setup(invoke)
  await screen.findByText('2012')
  test.rerender(test.view('awards'))
  expect(screen.queryByText('2012')).toBeNull()
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2))
  test.rerender(test.view('statistics', false))
  await act(async () => resolve(page('awards')))
  expect(screen.queryByText('Japan')).toBeNull()
  expect(screen.queryByText('2012')).toBeNull()
  test.rerender(test.view())
  await screen.findByText('2012')
  expect(invoke).toHaveBeenCalledTimes(3)
})
it('expires displayed summaries and a failed refresh never leaves old totals visible', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'performance'] })
  const test = setup()
  await screen.findByText('2012')
  act(() => vi.advanceTimersByTime(60_000))
  expect(screen.queryByText('2012')).toBeNull()
  expect(screen.getByText(/summary has expired/)).toBeTruthy()
  test.invoke.mockRejectedValue(new Error('applicationBusy'))
  fireEvent.click(screen.getByRole('button', { name: 'Refresh summary' }))
  await waitFor(() => expect(screen.getByText(/Station data unavailable/)).toBeTruthy())
  expect(screen.queryByText('2012')).toBeNull()
})
