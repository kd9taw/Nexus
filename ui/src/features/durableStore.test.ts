// @vitest-environment jsdom
// The durable store (#28): operator data must survive what `localStorage` does not.
//
// Two things are worth testing here and they are different in kind:
//   - the CLASSIFICATION (which keys are durable) — a list, checked against the other list that
//     classifies the same keys, so the two cannot drift apart silently;
//   - the BEHAVIOUR (migration, read precedence, write-through) — where the failure that matters
//     is silent data loss, so each case is paired with the state that must NOT happen.
import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import {
  loadDurable,
  durableGet,
  durableSet,
  durableRemove,
  flushDurable,
  __resetDurableForTest,
} from './durableStore'

const mockLoad = vi.fn<() => Promise<Record<string, string>>>()
const mockSave = vi.fn<(s: Record<string, string>) => Promise<boolean>>()
vi.mock('../api', () => ({
  uiStateLoad: () => mockLoad(),
  uiStateSave: (s: Record<string, string>) => mockSave(s),
}))

beforeEach(() => {
  __resetDurableForTest()
  window.localStorage.clear()
  mockLoad.mockReset().mockResolvedValue({})
  mockSave.mockReset().mockResolvedValue(true)
})
afterEach(() => __resetDurableForTest())

describe('a write that lands before the load (#205)', () => {
  it('survives the load instead of being shadowed by the stale file copy', async () => {
    // The waterfall pop-out bug, distilled: boot hygiene re-docked the stale popped-out
    // pane BEFORE ui-state.json was read, the corrective write reached localStorage only
    // (the cache was still null), and the loaded file's stale 'popped' then shadowed it
    // for the whole session — served by durableGet and re-flushed forever, so the pane
    // came up popped-out with no window on every launch.
    const key = 'nexus.panels.ft8.main'
    mockLoad.mockResolvedValue({ [key]: 'popped', 'nexus.watchlist': '["G0ABC"]' })
    durableSet(key, 'docked') // before loadDurable — the #205 ordering
    await loadDurable()
    expect(durableGet(key)).toBe('docked')
    // …and the correction is what flushes, not the stale value resurrected.
    await flushDurable()
    expect(mockSave.mock.calls[mockSave.mock.calls.length - 1][0][key]).toBe('docked')
    // Positive control: a key with no pre-load write still comes from the file, so this
    // cannot pass by the load being ignored wholesale.
    expect(durableGet('nexus.watchlist')).toBe('["G0ABC"]')
  })

  it('a pre-load remove sticks too', async () => {
    mockLoad.mockResolvedValue({ 'nexus.watchlist': '["G0ABC"]' })
    durableRemove('nexus.watchlist')
    await loadDurable()
    expect(durableGet('nexus.watchlist')).toBeNull()
  })
})

describe('migration off localStorage', () => {
  it('adopts a key that exists only in localStorage', async () => {
    window.localStorage.setItem('nexus.watchlist', '["G0ABC"]')
    await loadDurable()
    expect(durableGet('nexus.watchlist')).toBe('["G0ABC"]')
    // The migration flush is debounced, so drive it rather than race the timer.
    await flushDurable()
    expect(mockSave).toHaveBeenCalled()
    expect(mockSave.mock.calls[0][0]['nexus.watchlist']).toBe('["G0ABC"]')
  })

  it('does NOT resurrect a key the operator deleted', async () => {
    // The failure this prevents: treating an empty store as "not migrated yet" would re-adopt
    // from localStorage on every launch, so a deleted watchlist would keep coming back. The
    // test is absence FROM THE STORE, not emptiness OF the store.
    window.localStorage.setItem('nexus.watchlist', '["STALE"]')
    mockLoad.mockResolvedValue({ 'nexus.watchlist': '[]' })
    await loadDurable()
    expect(durableGet('nexus.watchlist')).toBe('[]')
  })

  it('leaves the localStorage copy in place as a rollback', async () => {
    window.localStorage.setItem('nexus.profiles', '["home"]')
    await loadDurable()
    expect(window.localStorage.getItem('nexus.profiles')).toBe('["home"]')
  })

  it('migrates nothing when there is nothing to migrate', async () => {
    await loadDurable()
    expect(mockSave, 'an empty first run must not write a file').not.toHaveBeenCalled()
  })
})

describe('reads and writes', () => {
  it('prefers the durable copy over localStorage', async () => {
    window.localStorage.setItem('nexus.navOrder', 'OLD')
    mockLoad.mockResolvedValue({ 'nexus.navOrder': 'NEW' })
    await loadDurable()
    expect(durableGet('nexus.navOrder')).toBe('NEW')
  })

  it('writes through to BOTH stores', async () => {
    await loadDurable()
    durableSet('nexus.watchlist', '["M7XYZ"]')
    // localStorage is updated synchronously so it never becomes the stale copy...
    expect(window.localStorage.getItem('nexus.watchlist')).toBe('["M7XYZ"]')
    // ...and the durable flush carries the same value.
    await flushDurable()
    expect(mockSave.mock.calls[mockSave.mock.calls.length - 1][0]['nexus.watchlist']).toBe('["M7XYZ"]')
  })

  it('a delete sticks in both stores', async () => {
    mockLoad.mockResolvedValue({ 'nexus.watchlist': '["G0ABC"]' })
    await loadDurable()
    durableRemove('nexus.watchlist')
    expect(durableGet('nexus.watchlist')).toBeNull()
    await flushDurable()
    expect('nexus.watchlist' in mockSave.mock.calls[mockSave.mock.calls.length - 1][0]).toBe(false)
  })

  it('falls back to localStorage when the bridge is not there', async () => {
    // A test, a plain browser, or a bridge that failed. This must degrade to exactly the
    // pre-#28 behaviour rather than losing the read.
    mockLoad.mockRejectedValue(new Error('no bridge'))
    window.localStorage.setItem('nexus.watchlist', '["FALLBACK"]')
    await loadDurable()
    expect(durableGet('nexus.watchlist')).toBe('["FALLBACK"]')
  })

  it('a failed flush does not lose the value', async () => {
    mockSave.mockRejectedValue(new Error('disk full'))
    await loadDurable()
    durableSet('nexus.watchlist', '["KEPT"]')
    await expect(flushDurable()).resolves.toBe(false)
    expect(window.localStorage.getItem('nexus.watchlist')).toBe('["KEPT"]')
  })
})
