// @vitest-environment jsdom
//
// THE OLD HIDDEN "WANTED" LIST JOINS THE WATCH LIST, ONCE (operator, 2026-09-24: "One list").
//
// `settings.wantedCalls` has had no editor since the watch list replaced it (2026-07-10), yet the
// Needed board went on tagging its entries — a list the operator could neither see nor change. On
// the first launch after the upgrade its entries are added to the watch list (Settings ▸ Spots &
// Alerts) and the old list is emptied through its one writer. Exactly once: the old list is
// emptied only after the watch list holding its entries is on disk, and a second launch finds
// nothing to add — so an entry the operator then removes from the watch list stays removed.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getSettings, retireWantedCalls, uiStateLoad, uiStateSave } from '../api'
import { pushToast } from '../toast'
import { __resetDurableForTest, flushDurable, loadDurable } from './durableStore'
import { __resetWatchlistFoldForTest, foldRetiredWantedList } from './watchlistFold'
import { loadWatchlist, newWatchFilter, saveWatchlist, WATCHLIST_CHANGED, type WatchFilter } from '../watchlist'

/** The station: its settings.json and its ui-state.json, as the backend holds them. */
const station = vi.hoisted(() => ({
  wantedCalls: [] as string[],
  uiState: {} as Record<string, string>,
  /** The order things reached the disk. */
  writes: [] as string[],
  uiStateSaveWorks: true,
}))
vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ wantedCalls: [...station.wantedCalls] })),
  retireWantedCalls: vi.fn(async () => {
    station.wantedCalls = []
    station.writes.push('retired')
  }),
  uiStateLoad: vi.fn(async () => ({ ...station.uiState })),
  uiStateSave: vi.fn(async (state: Record<string, string>) => {
    if (!station.uiStateSaveWorks) return false
    station.uiState = { ...state }
    station.writes.push(`ui-state:${state['nexus.watchlist'] ?? ''}`)
    return true
  }),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

/** A launch of the app: localStorage survives, the in-memory store does not. */
async function launch(): Promise<void> {
  __resetDurableForTest()
  __resetWatchlistFoldForTest()
  await loadDurable()
  await foldRetiredWantedList()
}
const calls = (list: WatchFilter[]) => list.filter((f) => f.kind === 'call').map((f) => f.value)

beforeEach(() => {
  station.wantedCalls = []
  station.uiState = {}
  station.writes = []
  station.uiStateSaveWorks = true
  localStorage.clear()
  vi.clearAllMocks()
})
afterEach(() => {
  __resetDurableForTest()
})

describe('the old wanted list folds into the watch list, once', () => {
  it('FIX: an operator with old entries sees them in their watch list after the upgrade', async () => {
    station.wantedCalls = ['VP8*', ' 3y0j ', 'FT*']
    // …beside what the watch list already held.
    saveWatchlist([newWatchFilter('dxcc', 'Bouvet')])
    const told = vi.fn()
    window.addEventListener(WATCHLIST_CHANGED, told)
    await launch()
    window.removeEventListener(WATCHLIST_CHANGED, told)
    const list = loadWatchlist()
    expect(calls(list)).toEqual(['VP8*', '3Y0J', 'FT*'])
    expect(list.filter((f) => f.kind === 'dxcc').map((f) => f.value), 'what the list held is kept').toEqual(['Bouvet'])
    // Durable: the watch list holding them is in ui-state.json, and only THEN is the old list gone.
    expect(station.writes).toEqual([expect.stringContaining('VP8*'), 'retired'])
    expect(station.wantedCalls).toEqual([])
    // The operator is told once, where the entries came from.
    expect(pushToast).toHaveBeenCalledTimes(1)
    // …and every view of the list open now re-reads it: the Settings editor, the WATCH tiles, the
    // board's chip.
    expect(told).toHaveBeenCalledTimes(1)
  })

  it('FIX: a second launch adds nothing', async () => {
    station.wantedCalls = ['VP8*', '3Y0J']
    await launch()
    const first = loadWatchlist()
    expect(calls(first), 'the first launch folded them (the control)').toEqual(['VP8*', '3Y0J'])
    vi.clearAllMocks()
    station.writes = []
    await launch()
    expect(loadWatchlist()).toEqual(first)
    expect(retireWantedCalls, 'nothing left to retire').not.toHaveBeenCalled()
    expect(pushToast).not.toHaveBeenCalled()
    expect(station.writes).toEqual([])
  })

  it('FIX: an entry the operator removes after the fold stays removed', async () => {
    station.wantedCalls = ['VP8*', '3Y0J']
    await launch()
    saveWatchlist(loadWatchlist().filter((f) => f.value !== '3Y0J'))
    await flushDurable() // the store writes an edit to disk a moment after it is made
    await launch()
    expect(calls(loadWatchlist())).toEqual(['VP8*'])
  })

  it('the old list is NOT emptied when the watch list could not be written — the next launch folds it', async () => {
    station.wantedCalls = ['VP8*']
    station.uiStateSaveWorks = false
    await launch()
    expect(retireWantedCalls, 'emptied before its entries were safe').not.toHaveBeenCalled()
    expect(station.wantedCalls).toEqual(['VP8*'])
    // Next launch the disk works: nothing is added twice (localStorage kept the entry), and the
    // old list goes.
    station.uiStateSaveWorks = true
    await launch()
    expect(calls(loadWatchlist())).toEqual(['VP8*'])
    expect(station.wantedCalls).toEqual([])
  })

  it('an entry already on the watch list is not added twice (case and spaces aside)', async () => {
    saveWatchlist([newWatchFilter('call', 'vp8*')])
    station.wantedCalls = ['VP8*', 'VP8* ', '3Y0J']
    await launch()
    expect(calls(loadWatchlist())).toEqual(['vp8*', '3Y0J'])
  })

  it('an old entry that named no station is dropped, not widened into one that names every station', async () => {
    // The old list matched a call exactly, or a TRAILING * as a prefix; "*" alone, "**" and a
    // star anywhere else matched nothing. The watch list's star is a glob — "*" there is every
    // station — so carrying these over would turn a dead entry into a siren.
    station.wantedCalls = ['*', '**', ' ', 'A*B', '*ABC', 'VK9*X*', 'K1ABC']
    await launch()
    expect(calls(loadWatchlist())).toEqual(['K1ABC'])
    expect(station.wantedCalls, 'the dead entries go with the rest').toEqual([])
  })

  it('does nothing without a station to ask (a browser preview, a test), and does not throw', async () => {
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('no bridge'))
    await expect(launch()).resolves.toBeUndefined()
    expect(loadWatchlist()).toEqual([])
    expect(retireWantedCalls).not.toHaveBeenCalled()
  })

  it('runs once however many callers ask at once (React runs a mount effect twice in development)', async () => {
    station.wantedCalls = ['VP8*']
    __resetDurableForTest()
    __resetWatchlistFoldForTest()
    await loadDurable()
    await Promise.all([foldRetiredWantedList(), foldRetiredWantedList()])
    expect(calls(loadWatchlist())).toEqual(['VP8*'])
    expect(retireWantedCalls).toHaveBeenCalledTimes(1)
  })

  it('an older station with no old list at all (the key absent) is left alone', async () => {
    vi.mocked(getSettings).mockResolvedValueOnce({} as never)
    await launch()
    expect(retireWantedCalls).not.toHaveBeenCalled()
    expect(uiStateLoad).toHaveBeenCalled()
    expect(uiStateSave).not.toHaveBeenCalled()
  })
})
