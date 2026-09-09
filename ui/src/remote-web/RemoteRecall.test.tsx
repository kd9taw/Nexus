// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteRecall } from './RemoteRecall'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { StationDataContext } from '../stationAccess'
import type { ApplicationClient } from './application-client'
import type { AppSnapshot } from '../types'

afterEach(cleanup)
const snap = { mygrid: 'FN31', radio: { band: '20m', dialMhz: 14.074 }, stations: [], b4MatchMode: true } as unknown as AppSnapshot
export function page(call = 'W1AW') {
  return { type: 'applicationPage', collection: 'recall', offset: 0, total: 2030, retained: 1, nextCursor: null,
    rows: [{ call, whenUnix: 1750000000, band: '40m', mode: 'FT8', freqMhz: 7.074, confirmed: false, comment: 'previous contact' }],
    meta: { capturedAgeMs: 0, source: { call, entity: 'United States',
      history: { count: 2030, workedBefore: true, lastUnix: 1750000000, confirmedCount: 10, bands: ['40m', '20m'], modes: ['FT8', 'CW'] },
      workedBandModes: [['40m', 'FT8'], ['20m', 'CW']], latestNote: 'note from an older contact',
      slots: { workedEver: true, bandUnknown: false, bandsWorked: ['40M', '20M'], modesWorked: ['FT8', 'CW'] } } } }
}
function setup(invoke = vi.fn(async (_command: string, _args?: Record<string, unknown>): Promise<unknown> => page())) {
  const client = { invoke, supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient
  const source = new RemoteCollections(client)
  const open = vi.fn()
  const view = (call = 'W1AW', available = true) => <StationDataContext.Provider value={available}>
    <RemoteCollectionsContext.Provider value={source}><RemoteRecall snap={snap} call={call} mode="FT8" onOpenLog={open} /></RemoteCollectionsContext.Provider></StationDataContext.Provider>
  return { ...render(view()), view, invoke, open }
}
it('uses complete station summaries, the real recall card, and browser-local log navigation', async () => {
  const test = setup()
  await waitFor(() => expect(screen.getByText('previous contact')).toBeTruthy())
  expect(screen.getByText(/note from an older contact/)).toBeTruthy()
  expect(screen.getByText(/Showing 1 of 2030/)).toBeTruthy()
  expect(screen.queryByText(/NEW ONE|NEW BAND|NEW MODE|Dupe/)).toBeNull()
  fireEvent.click(screen.getByRole('listitem'))
  expect(test.open).toHaveBeenCalledWith('W1AW')
  expect(test.invoke).toHaveBeenCalledWith('get_remote_recall', expect.objectContaining({ collection: 'recall', search: 'W1AW' }))
  expect(test.invoke.mock.calls.every(([command]) => command === 'get_remote_recall')).toBe(true)
})
it('hides old recall immediately on call changes or station loss, and ignores late results', async () => {
  let resolve: (value: unknown) => void = () => {}
  const invoke = vi.fn().mockResolvedValueOnce(page()).mockImplementationOnce(() => new Promise(r => { resolve = r }))
  const test = setup(invoke)
  await waitFor(() => expect(screen.getByText('previous contact')).toBeTruthy())
  test.rerender(test.view('K1ABC'))
  expect(screen.queryByText('previous contact')).toBeNull()
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2))
  test.rerender(test.view('W1AW', false))
  resolve(page('K1ABC'))
  await waitFor(() => expect(screen.getByText(/Station data unavailable/)).toBeTruthy())
  expect(screen.queryByText('previous contact')).toBeNull()
})
it('a malformed or mismatched summary is unavailable, never a new-station claim', async () => {
  setup(vi.fn().mockResolvedValue(page('K1ABC')))
  await waitFor(() => expect(screen.getByText(/Station data unavailable/)).toBeTruthy())
  expect(screen.queryByRole('listitem')).toBeNull()
  expect(screen.queryByText(/NEW ONE|First contact/)).toBeNull()
})
