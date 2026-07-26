import { describe, it, expect, beforeEach } from 'vitest'

// connectConfig persists through localStorage; this suite runs in the default node
// environment (no jsdom dependency), so install a minimal in-memory localStorage
// shim — enough for the migrateLegacyMode / load / save paths under test.
class MemoryStorage {
  private m = new Map<string, string>()
  get length() { return this.m.size }
  clear() { this.m.clear() }
  getItem(k: string) { return this.m.has(k) ? (this.m.get(k) as string) : null }
  key(i: number) { return [...this.m.keys()][i] ?? null }
  removeItem(k: string) { this.m.delete(k) }
  setItem(k: string, v: string) { this.m.set(k, String(v)) }
}
const memStore = new MemoryStorage() as unknown as Storage
globalThis.localStorage = memStore
;(globalThis as { window?: { localStorage: Storage } }).window = { localStorage: memStore }
import {
  normalizeConfig,
  defaultConnectConfig,
  isPaneId,
  DEFAULT_SLOTS,
  SLOT_IDS,
  PANE_IDS,
} from './connectConfig'

describe('connectConfig', () => {
  beforeEach(() => {
    try {
      localStorage.clear()
    } catch {
      /* ignore */
    }
  })

  it('defaults to a complete slot record', () => {
    const c = defaultConnectConfig()
    expect(Object.keys(c.slots).sort()).toEqual([...SLOT_IDS].sort())
  })

  it('normalize keeps valid overrides, drops unknown pane ids, fills every slot', () => {
    const c = normalizeConfig({ slots: { left1: 'spacewx', left2: 'bogus' } })
    expect(c.slots.left1).toBe('spacewx') // valid override kept
    expect(c.slots.left2).toBe(DEFAULT_SLOTS.left2) // unknown id → default
    expect(Object.keys(c.slots).sort()).toEqual([...SLOT_IDS].sort()) // complete record
  })

  it('normalize repairs junk to a usable config', () => {
    expect(normalizeConfig(null).slots).toEqual(DEFAULT_SLOTS)
    expect(normalizeConfig('garbage').slots).toEqual(DEFAULT_SLOTS)
    expect(normalizeConfig(42).slots).toEqual(DEFAULT_SLOTS)
  })

  // The Basic/Expert detail toggle was removed 2026-07-26 (operator): every pane renders in
  // full. A stored `mode` from an older install must be IGNORED, not choke normalize — there is
  // no setting left for it to migrate into.
  it('ignores a stored mode from before the toggle was removed', () => {
    localStorage.setItem('nexus.connect.mode', 'expert')
    const c = normalizeConfig({ mode: 'basic', slots: { left1: 'spacewx' } })
    expect('mode' in c).toBe(false)
    expect(c.slots.left1).toBe('spacewx') // the rest of the stored config still applies
  })

  it('isPaneId accepts valid ids and rejects junk', () => {
    expect(isPaneId('spacewx')).toBe(true)
    expect(isPaneId('nope')).toBe(false)
    expect(isPaneId(3)).toBe(false)
  })

  it('DEFAULT_SLOTS references only valid pane ids', () => {
    for (const s of SLOT_IDS) expect((PANE_IDS as readonly string[]).includes(DEFAULT_SLOTS[s])).toBe(true)
  })
})
