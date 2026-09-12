// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteDxpeditions } from './RemoteDxpeditions'
import { DxpeditionsView } from '../components/DxpeditionsView'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import * as api from '../api'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'
import { parseDxpeditions } from './dxpeditions'
import fixture from './__fixtures__/dxpeditions.json'

const originalScroll = Element.prototype.scrollIntoView
beforeEach(() => { Element.prototype.scrollIntoView = vi.fn() })
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); localStorage.clear(); Element.prototype.scrollIntoView = originalScroll })
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'dxpeditions', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 0, source: structuredClone(fixture) } })
function setup(invoke = vi.fn(async (): Promise<unknown> => page()), supported = true) {
  const client = { invoke, supports: () => supported, getPhase: () => 'ready' } as unknown as ApplicationClient
  const source = new RemoteCollections(client)
  const view = (available = true) => <StationDataContext.Provider value={available}>
    <RemoteCollectionsContext.Provider value={source}><RemoteDxpeditions /></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider>
  return { ...render(view()), view, invoke }
}
it('renders the actual board, calendar and per-band forecast with no native reads or alarm writes', async () => {
  const forbidden = vi.fn(async () => { throw new Error('no native operation') })
  const uninstall = installApplicationTransport({ kind: 'remote', invoke: forbidden })
  const storage = vi.spyOn(Storage.prototype, 'setItem'), open = vi.spyOn(window, 'open').mockReturnValue(null)
  try {
    const test = setup()
    await screen.findByText('Bouvet Island')
    expect(screen.getByText('Station DX board updated 2 s ago.')).toBeTruthy()
    expect(screen.getByText('Station prediction windows computed 1 min ago.')).toBeTruthy()
    expect(test.container.querySelector('.dxped-view .worknow-card')).toBeTruthy()
    expect(test.container.querySelector('.dxped-calendar')).toBeTruthy()
    expect(test.container.querySelector('.wn-live')).toBeTruthy()
    expect(test.container.querySelector('.cp-engine')?.textContent).toBe('P.533')
    expect(test.container.querySelector('.wn-chase, .wn-work, .dx-map-link, .dxped-popout, .dc-alarm, .dc-chase')).toBeNull()
    const website = screen.getAllByTitle(/example.com\/expedition/)[0]
    fireEvent.click(website)
    expect(open).toHaveBeenCalledWith('https://example.com/expedition', '_blank', 'noopener,noreferrer')
    expect(test.invoke).toHaveBeenCalledWith('get_remote_dxpeditions', { collection: 'dxpeditions', cursor: null, search: '', unconfirmed: false, after: null })
    expect(forbidden).not.toHaveBeenCalled(); expect(storage).not.toHaveBeenCalled()
  } finally { uninstall() }
})
it('preserves the native forecast polling and station action controls as a positive control', async () => {
  const value = parseDxpeditions(page()), work = vi.fn(), map = vi.fn()
  const native = vi.spyOn(api, 'getDxpedWindows').mockResolvedValue(value.windows ?? [])
  try {
    const test = render(<DxpeditionsView snap={value} onWorkSpot={work} onShowOnMap={map} />)
    await waitFor(() => expect(native).toHaveBeenCalledWith(7))
    expect(test.container.querySelector('.wn-chase')).toBeTruthy()
    fireEvent.click(test.container.querySelector('.wn-work')!)
    expect(work).toHaveBeenCalledWith({ call: '3Y0TEST', band: '20m', mode: 'CW', freqMhz: null })
    fireEvent.click(test.container.querySelector('.dx-map-link')!)
    expect(map).toHaveBeenCalledWith('3Y0TEST')
  } finally { native.mockRestore() }
})
it('expires the source age, refuses failed refresh and discards replies after loss', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'performance'] })
  const old = page(); (old.meta as { source: { sourceAgeMs: number } }).source.sourceAgeMs = 299_000
  const invoke = vi.fn().mockResolvedValueOnce(old).mockRejectedValue(new Error('applicationUnavailable'))
  const test = setup(invoke)
  await screen.findByText('Bouvet Island')
  act(() => vi.advanceTimersByTime(1000))
  expect(screen.queryByText('Bouvet Island')).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Refresh DXpeditions' }))
  await screen.findByText(/Station data unavailable/)
  let resolve: (v: unknown) => void = () => {}
  invoke.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  fireEvent.click(screen.getByRole('button', { name: 'Refresh DXpeditions' }))
  test.rerender(test.view(false)); await act(async () => resolve(page()))
  expect(screen.queryByText('Bouvet Island')).toBeNull()
  invoke.mockResolvedValue(page()); test.rerender(test.view()); await screen.findByText('Bouvet Island')
})
it('makes no DX request for an older station', () => {
  const test = setup(undefined, false)
  expect(test.invoke).not.toHaveBeenCalled()
  expect(screen.getByRole('button', { name: 'Refresh DXpeditions' }).hasAttribute('disabled')).toBe(true)
})
it('expires prediction windows independently at their TTL or UTC boundary while retaining the board', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'performance'] })
  const p = page(); (p.meta as { source: { windowValidForMs: number } }).source.windowValidForMs = 1000
  const test = setup(vi.fn(async () => p))
  await screen.findByText('Bouvet Island')
  expect(test.container.querySelector('.worknow-card .cp-engine')).toBeTruthy()
  act(() => vi.advanceTimersByTime(1000))
  expect(screen.getByText('Bouvet Island')).toBeTruthy()
  expect(test.container.querySelector('.worknow-card .cp-engine')).toBeNull()
  expect(screen.getByText(/Station prediction windows are unavailable/)).toBeTruthy()
})
