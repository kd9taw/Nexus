// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { RemoteMemories } from './RemoteMemories'
import { MemoriesView } from '../components/MemoriesView'
import { memoriesStore, coerceBank } from '../features/memories'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import { t } from '../i18n'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/memories.json'

afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); localStorage.clear() })
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'memories', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 0, source: { bank: structuredClone(fixture), sourceAgeMs: 250 } } as unknown as QueryPage['meta'] })
function setup(invoke = vi.fn(async (): Promise<unknown> => page()), supported = true) {
  const client = { invoke, supports: () => supported, getPhase: () => 'ready' } as unknown as ApplicationClient
  const source = new RemoteCollections(client)
  const view = (available = true) => <StationDataContext.Provider value={available}>
    <RemoteCollectionsContext.Provider value={source}><RemoteMemories myGrid="FN31" /></RemoteCollectionsContext.Provider>
  </StationDataContext.Provider>
  return { ...render(view()), view, invoke }
}
it('uses the real Memories list/grid, groups, favorites and search without reading or writing a browser bank or recalling', async () => {
  const forbidden = vi.fn(async () => { throw new Error('no native operation') })
  const uninstall = installApplicationTransport({ kind: 'remote', invoke: forbidden })
  const get = vi.spyOn(memoriesStore, 'get'), update = vi.spyOn(memoriesStore, 'update'), stored = vi.spyOn(Storage.prototype, 'setItem')
  try {
    const test = setup()
    await screen.findByText('Evening test net')
    expect(test.container.querySelector('.memories-view .mv-list')).toBeTruthy()
    expect(screen.getByText('Check in after the opening call')).toBeTruthy()
    expect(test.container.querySelector('.mv-row-tune, .mv-row-edit, .mv-row-del, .mv-row-move, .mv-side-add, input[type=file]')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Local repeaters.*2/ }))
    expect(screen.queryByText('Evening test net')).toBeNull()
    expect(screen.getByText('Imported odd split')).toBeTruthy()
    fireEvent.change(test.container.querySelector('.mv-search')!, { target: { value: 'P25' } })
    expect(screen.queryByText('Hilltop repeater')).toBeNull()
    fireEvent.click(screen.getByTitle(t('memories.toolbar.grid.title')))
    expect(test.container.querySelector('.mv-grid')?.textContent).toContain('P25')
    const name = test.container.querySelector<HTMLInputElement>('.mv-grid input.mv-cell')!
    expect(name.readOnly).toBe(true)
    fireEvent.change(name, { target: { value: 'Attempted edit' } }); fireEvent.blur(name)
    fireEvent.change(test.container.querySelector('.mv-search')!, { target: { value: '' } })
    fireEvent.click(screen.getByTitle(t('memories.toolbar.list.title')))
    fireEvent.click(screen.getByRole('button', { name: /Favorites.*2/ }))
    expect(screen.getByText('Evening test net')).toBeTruthy()
    expect(screen.queryByText('Imported odd split')).toBeNull()
    fireEvent.click(test.container.querySelector('.mv-star')!)
    fireEvent.click(test.container.querySelector('.mv-row-main')!)
    expect(test.invoke).toHaveBeenCalledTimes(1)
    expect(test.invoke).toHaveBeenCalledWith('get_remote_memories', { collection: 'memories', cursor: null, search: '', unconfirmed: false, after: null })
    expect(get).not.toHaveBeenCalled(); expect(update).not.toHaveBeenCalled()
    expect(stored).not.toHaveBeenCalled(); expect(forbidden).not.toHaveBeenCalled()
  } finally { uninstall() }
})
it('keeps the native bank, favorite write and recall as positive controls', () => {
  memoriesStore.set(coerceBank(fixture))
  const recall = vi.fn(), test = render(<MemoriesView dialMhz={14.06} dialMode="CW" onRecall={recall} />)
  fireEvent.click(test.container.querySelector('.mv-row-tune')!)
  expect(recall.mock.calls[0][0].id).toBe('m-net')
  fireEvent.click(test.container.querySelector('.mv-star')!)
  expect(memoriesStore.get().memories[0].favorite).toBe(false)
  expect(test.container.querySelector('.mv-side-add, .mv-row-edit')).toBeTruthy()
})
it('expires source age, clears failures and late replies, and refreshes after recovery', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'performance'] })
  const old = page(); (old.meta as { source: { sourceAgeMs: number } }).source.sourceAgeMs = 59_000
  const invoke = vi.fn().mockResolvedValueOnce(old).mockRejectedValue(new Error('applicationUnavailable'))
  const test = setup(invoke)
  await screen.findByText('Evening test net')
  act(() => vi.advanceTimersByTime(1000))
  expect(screen.queryByText('Evening test net')).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Refresh memories' }))
  await screen.findByText(/Station data unavailable/)
  let resolve: (value: unknown) => void = () => {}
  invoke.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  fireEvent.click(screen.getByRole('button', { name: 'Refresh memories' }))
  test.rerender(test.view(false)); await act(async () => resolve(page()))
  expect(screen.queryByText('Evening test net')).toBeNull()
  invoke.mockResolvedValue(page()); test.rerender(test.view()); await screen.findByText('Evening test net')
})
it('does not request Memories from an older station', () => {
  const test = setup(undefined, false)
  expect(test.invoke).not.toHaveBeenCalled()
  expect(screen.getByRole('button', { name: 'Refresh memories' }).hasAttribute('disabled')).toBe(true)
})
