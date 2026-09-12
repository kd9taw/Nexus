// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RemoteOta } from './RemoteOta'
import { PotaSotaView } from '../components/PotaSotaView'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import type { ApplicationClient } from './application-client'
import type { AppSnapshot } from '../types'
import type { QueryPage } from './application-query-protocol'
import { t } from '../i18n'
import fixture from './__fixtures__/ota.json'

const snap = { hunt: null } as AppSnapshot
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'ota', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { capturedAgeMs: 0, source: structuredClone(fixture) } })
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); localStorage.clear() })
function setup(invoke = vi.fn(async (): Promise<unknown> => page())) {
  const client = { invoke, supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient
  const source = new RemoteCollections(client)
  const view = (available = true) => <StationDataContext.Provider value={available}><RemoteCollectionsContext.Provider value={source}>
    <RemoteOta snap={snap} /></RemoteCollectionsContext.Provider></StationDataContext.Provider>
  return { ...render(view()), invoke, view }
}
it('uses the real hunter cards and filters with no station action or native loader', async () => {
  const forbidden = vi.fn(async () => { throw new Error('unexpected native operation') })
  const uninstall = installApplicationTransport({ kind: 'remote', invoke: forbidden })
  try {
    const test = setup()
    await screen.findByText('Logged park')
    expect(test.container.querySelector('.pota-view .pota-spot-list')).toBeTruthy()
    expect(test.container.querySelector('.pota-activation')?.textContent).toContain('2301')
    expect(test.container.querySelector('.pota-hunt-banner')?.textContent).toContain('US-0004')
    expect(test.container.querySelector('.pota-hunt-btn, .pota-hunt-clear, .pota-act-start, .pota-popout, input[type=file]')).toBeNull()
    expect(test.container.querySelector('.pota-source-hint')).toBeNull()
    expect(screen.queryByText('Test summit')).toBeNull()
    fireEvent.click(screen.getByRole('tab', { name: t('ota.program.both') }))
    expect(screen.getByText('Test summit')).toBeTruthy()
    const modes = within(screen.getByRole('group', { name: t('ota.filter.mode.aria') }))
    fireEvent.click(modes.getByRole('button', { name: 'CW' }))
    expect(screen.getByText('Imported park')).toBeTruthy(); expect(screen.queryByText('Logged park')).toBeNull()
    fireEvent.click(modes.getByRole('button', { name: t('ota.filter.all') }))
    const bands = within(screen.getByRole('group', { name: t('ota.filter.band.aria') }))
    fireEvent.click(bands.getByRole('button', { name: '20m' }))
    expect(screen.queryByText('Imported park')).toBeNull(); expect(screen.getByText('Logged park')).toBeTruthy()
    fireEvent.change(test.container.querySelector('.pota-sort-pick')!, { target: { value: 'reference' } })
    fireEvent.click(screen.getByRole('button', { name: t('remote.otaRefresh') }))
    await screen.findByText('Logged park')
    expect(test.container.querySelector<HTMLSelectElement>('.pota-sort-pick')?.value).toBe('reference')
    expect(screen.getByRole('tab', { name: t('ota.program.both') }).getAttribute('aria-selected')).toBe('true')
    expect(screen.queryByText('Imported park')).toBeNull()
    expect(test.invoke).toHaveBeenCalledTimes(2)
    expect(test.invoke).toHaveBeenLastCalledWith('get_remote_ota', { collection: 'ota', cursor: null, search: '', unconfirmed: false, after: null })
    expect(forbidden).not.toHaveBeenCalled()
  } finally { uninstall() }
})
it('clears lost station values and retains the program choice through recovery', async () => {
  const test = setup()
  await screen.findByText('Logged park')
  fireEvent.click(screen.getByRole('tab', { name: 'SOTA' }))
  expect(screen.getByText('Test summit')).toBeTruthy()
  test.rerender(test.view(false))
  expect(test.container.textContent).not.toContain('Test summit')
  expect(test.container.textContent).not.toContain('US-0001')
  test.rerender(test.view())
  await screen.findByText('Test summit')
  expect(screen.getByRole('tab', { name: 'SOTA' }).getAttribute('aria-selected')).toBe('true')
  expect(screen.queryByText('Logged park')).toBeNull()
})
it('expires one feed independently, then clears expired capture context and rejected refreshes', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'performance'] })
  const initial = page()
  ;(initial.meta as { source: typeof fixture }).source.feeds[0].sourceAgeMs = 899_000
  const invoke = vi.fn(async (): Promise<unknown> => initial)
  const test = setup(invoke)
  await act(async () => { await Promise.resolve() })
  fireEvent.click(screen.getByRole('tab', { name: t('ota.program.both') }))
  expect(screen.getByText('Logged park')).toBeTruthy()
  await act(async () => { vi.advanceTimersByTime(2000) })
  expect(screen.queryByText('Logged park')).toBeNull()
  expect(screen.getByText('Test summit')).toBeTruthy()
  expect(screen.getByText(t('remote.otaFeedExpired'), { exact: false })).toBeTruthy()
  await act(async () => { vi.advanceTimersByTime(60_000) })
  expect(test.container.textContent).not.toContain('Test summit')
  expect(test.container.textContent).not.toContain('US-0001')
  invoke.mockRejectedValueOnce(new Error('applicationUnavailable'))
  fireEvent.click(screen.getByRole('button', { name: t('remote.otaRefresh') }))
  await act(async () => { await Promise.resolve() })
  expect(test.container.textContent).not.toContain('Test summit')
  expect(test.invoke).toHaveBeenCalledTimes(2)
})
it('retains native Hunt, activation and import behavior as positive controls', async () => {
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    if (command === 'get_ota_spots') return fixture.feeds.find(f => f.program === args?.program)?.spots ?? []
    if (command === 'get_activation' || command === 'clear_activation') return { program: null, reference: null, qsoCount: 0 }
    if (command === 'set_activation') return { ...args, qsoCount: 0 }
    if (command === 'set_hunt_target' || command === 'clear_hunt_target') return snap
    if (command === 'parks_count') return 42
    if (command === 'hunted_parks_count') return 7
    if (command === 'import_hunted_parks_csv') return 77
    throw new Error(`unexpected command ${command}`)
  })
  const previous = window.__TAURI_INTERNALS__
  window.__TAURI_INTERNALS__ = { invoke: invoke as never }
  try {
    const onHunt = vi.fn(), onSnap = vi.fn()
    const test = render(<PotaSotaView snap={snap} onHunt={onHunt} onSnap={onSnap} />)
    await screen.findByText('Logged park')
    fireEvent.click(test.container.querySelector('.pota-hunt-btn')!)
    expect(test.container.querySelector('.pota-source-hint')).toBeTruthy()
    await waitFor(() => expect(onHunt).toHaveBeenCalledTimes(1))
    expect(invoke).toHaveBeenCalledWith('set_hunt_target', { call: 'W1AW', program: 'POTA', reference: 'US-0002' })
    expect(onSnap).toHaveBeenCalledWith(snap)
    fireEvent.change(test.container.querySelector('.pota-act-ref')!, { target: { value: 'US-0005' } })
    fireEvent.click(screen.getByRole('button', { name: t('ota.activation.start') }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('set_activation', { program: 'POTA', reference: 'US-0005' }))
    await screen.findByRole('button', { name: t('ota.activation.stop.label') })
    fireEvent.click(screen.getByRole('button', { name: t('ota.activation.stop.label') }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('clear_activation', undefined))
    const files = test.container.querySelectorAll('input[type=file]')
    fireEvent.change(files[1], { target: { files: [{ text: async () => 'Reference\nUS-0006' }] } })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_hunted_parks_csv', { csv: 'Reference\nUS-0006' }))
    await screen.findByText(t('ota.hunted.have', { formatted: '77' }))
  } finally { window.__TAURI_INTERNALS__ = previous }
})
