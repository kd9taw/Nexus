// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { RemoteInsights } from './RemoteInsights'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { StationDataContext } from '../stationAccess'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/insights.json'

afterEach(() => { cleanup(); localStorage.clear() })
const page = (collection: string, source: unknown): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: collection as QueryPage['collection'], offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 0, source: source as QueryPage['meta'] } })
const awards = () => page('awards', { logCount: 2301, ...structuredClone(fixture) })
const diagnostics = { diagnoses: [{ index: 0, award: 'DXCC/WAS', status: 'needsAction', reasons: [{ code: 'r3', confidence: 'confident',
  explanation: 'W1AW is confirmed on a non-award source only.', action: { kind: 'uploadToLotw' } }] }],
  buckets: [{ kind: 'Upload to LoTW', count: 1, qsoIndices: [] }], oneAway: [], waitingOnPartner: 0, pendingLag: 0, logCount: 2301 }
function view(commands: string[], confirmations: () => Promise<unknown>) {
  const invoke = vi.fn(async (_command: string, args?: Record<string, unknown>): Promise<unknown> => args?.collection === 'confirmations' ? confirmations() : awards())
  const client = { invoke, supports: (c: string) => commands.includes(c), getPhase: () => 'ready' } as unknown as ApplicationClient
  const source = new RemoteCollections(client)
  const result = render(<StationDataContext.Provider value={true}><RemoteCollectionsContext.Provider value={source}>
    <RemoteInsights kind="awards" />
  </RemoteCollectionsContext.Provider></StationDataContext.Provider>)
  return { ...result, invoke }
}

it('shows the station diagnostics under the award summary on a v15 station, read only', async () => {
  const test = view(['get_remote_insights', 'get_remote_confirmations'], async () => page('confirmations', diagnostics))
  await screen.findByText('W1AW is confirmed on a non-award source only.')
  expect(test.container.querySelector('.conf-panel button')).toBeNull()
  expect(screen.getByText('Journey and uploads are not available remotely yet. Confirmation diagnostics come from the station.')).toBeTruthy()
  expect(test.invoke.mock.calls.map(([command, args]) => `${command} ${args?.collection}`).sort())
    .toEqual(['get_remote_confirmations confirmations', 'get_remote_insights awards'])
})

it('keeps the award summary on a v14 station, says diagnostics are not available, and never asks for them', async () => {
  const confirmations = vi.fn(async () => page('confirmations', diagnostics))
  const test = view(['get_remote_insights'], confirmations)
  await screen.findByText('Japan')
  expect(screen.getByText('Journey, confirmation diagnostics and uploads are not available remotely yet.')).toBeTruthy()
  expect(test.invoke.mock.calls.map(([command]) => command)).toEqual(['get_remote_insights'])
  expect(confirmations).not.toHaveBeenCalled()
  expect(test.container.querySelector('.conf-panel')).toBeNull()
})

it('still shows the award summary when the diagnosis read fails or arrives malformed', async () => {
  for (const failing of [async () => { throw new Error('applicationUnavailable') }, async () => page('confirmations', { ...diagnostics, account: 'W1AW' })]) {
    const test = view(['get_remote_insights', 'get_remote_confirmations'], failing)
    await screen.findByText('Japan')
    await waitFor(() => expect(test.invoke).toHaveBeenCalledTimes(2))
    expect(test.container.querySelector('.conf-panel')).toBeNull()
    cleanup()
  }
})
