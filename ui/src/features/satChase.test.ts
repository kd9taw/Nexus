import { describe, it, expect, beforeEach, vi } from 'vitest'

// Node test env: in-memory localStorage (the satAlarm.test.ts shim).
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
vi.stubGlobal('window', { localStorage: memStore } as unknown as Window & typeof globalThis)

import { satChasingSet, satChasingNorads, toggleSatChasing } from './satChase'

beforeEach(() => memStore.clear())

describe('satChase NORAD recording (phase 4 rename survival, UI half)', () => {
  it('records the NORAD beside a starred name — additively, never as a key', () => {
    expect(toggleSatChasing('AO-91', 43017)).toBe(true)
    // The chase set stays NAME-keyed (NOT a key migration)…
    expect(satChasingSet().has('AO-91')).toBe(true)
    // …and the catalog number is remembered beside it.
    expect(satChasingNorads()['AO-91']).toBe(43017)
  })

  it('keeps the recorded NORAD across an unstar (additive — knowledge survives)', () => {
    toggleSatChasing('AO-91', 43017)
    expect(toggleSatChasing('AO-91', 43017)).toBe(false) // unstar
    expect(satChasingSet().has('AO-91')).toBe(false)
    expect(satChasingNorads()['AO-91']).toBe(43017)
  })

  it('a norad-less toggle still stars and never invents an entry', () => {
    expect(toggleSatChasing('ISS (ZARYA)')).toBe(true)
    expect(satChasingSet().has('ISS (ZARYA)')).toBe(true)
    expect(satChasingNorads()['ISS (ZARYA)']).toBeUndefined()
  })

  it('corrupt storage reads as empty, never throws', () => {
    memStore.setItem('nexus.sats.chasingNorad', '{not json')
    expect(satChasingNorads()).toEqual({})
    memStore.setItem('nexus.sats.chasingNorad', '["array","not","map"]')
    expect(satChasingNorads()).toEqual({})
    memStore.setItem('nexus.sats.chasingNorad', '{"AO-91":"not-a-number","SO-50":22826}')
    expect(satChasingNorads()).toEqual({ 'SO-50': 22826 })
  })
})
