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
const dispatched = vi.fn()
globalThis.localStorage = memStore
vi.stubGlobal('window', {
  localStorage: memStore,
  dispatchEvent: dispatched,
} as unknown as Window & typeof globalThis)

import {
  satChasingSet,
  satChasingNorads,
  toggleSatChasing,
  satChaseKeys,
  isSatChased,
  filterSatsToChased,
  satFavOnly,
  setSatFavOnly,
  SAT_CHASE_EVENT,
} from './satChase'
import { satAlarmMap, toggleSatAlarm } from './satAlarm'

beforeEach(() => {
  memStore.clear()
  dispatched.mockClear()
})

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

describe('★ filter derivation (the Connect satellite surfaces)', () => {
  const birds = [
    { name: 'RS-44', norad: 44909 },
    { name: 'AO-7', norad: 7530 },
    { name: 'ISS (ZARYA)', norad: 25544 },
  ]

  it('zero stars → every bird shows (a fresh install never renders an empty sky)', () => {
    expect(filterSatsToChased(birds, satChaseKeys())).toEqual(birds)
  })

  it('stars filter by name', () => {
    toggleSatChasing('RS-44', 44909)
    expect(filterSatsToChased(birds, satChaseKeys()).map((b) => b.name)).toEqual(['RS-44'])
  })

  it('a starred bird survives an upstream RENAME via its recorded NORAD', () => {
    toggleSatChasing('AO-91', 43017)
    const renamed = [
      { name: 'AO-91 (FOX-1B)', norad: 43017 },
      { name: 'AO-7', norad: 7530 },
    ]
    expect(filterSatsToChased(renamed, satChaseKeys()).map((b) => b.name)).toEqual([
      'AO-91 (FOX-1B)',
    ])
  })

  it("an UNSTARRED name's surviving NORAD record does not resurrect the bird", () => {
    toggleSatChasing('AO-91', 43017)
    toggleSatChasing('AO-91', 43017) // unstar — the NORAD record deliberately survives
    toggleSatChasing('RS-44', 44909)
    const out = filterSatsToChased(
      [
        { name: 'AO-91', norad: 43017 },
        { name: 'RS-44', norad: 44909 },
      ],
      satChaseKeys(),
    )
    expect(out.map((b) => b.name)).toEqual(['RS-44'])
  })

  it('isSatChased: norad-less birds still match on name (case-insensitive)', () => {
    toggleSatChasing('ISS (ZARYA)')
    expect(isSatChased('ISS (Zarya)', null, satChaseKeys())).toBe(true)
    expect(isSatChased('AO-7', undefined, satChaseKeys())).toBe(false)
  })

  it('unstars a RENAMED bird via its recorded NORAD — no double-star, no stuck ★', () => {
    // isSatChased matches NORAD-first, so the toggle must resolve the current
    // state the same way: after an upstream rename the star lives under the
    // OLD name, and a name-only toggle would ADD a second entry instead of
    // unstarring (the stuck-★ bug — the reviewer's repro).
    toggleSatChasing('AO-91', 43017) // operator stars it
    // CelesTrak renames the bird; the operator clicks ★ to UNSTAR.
    expect(toggleSatChasing('AO-91 (FOX-1B)', 43017)).toBe(false)
    expect(satChasingSet().size).toBe(0)
    expect(isSatChased('AO-91 (FOX-1B)', 43017, satChaseKeys())).toBe(false)
    expect(isSatChased('AO-91', 43017, satChaseKeys())).toBe(false)
  })

  it('a renamed unstar also disarms the OLD name\'s alarm (never orphaned)', () => {
    toggleSatChasing('AO-91', 43017)
    toggleSatAlarm('AO-91')
    toggleSatChasing('AO-91 (FOX-1B)', 43017) // unstar under the new name
    expect(satAlarmMap()['AO-91']).toBeUndefined()
  })

  it('heals a pre-fix DOUBLE-star (both names, one catalog number) in one click', () => {
    memStore.setItem('nexus.sats.chasing', JSON.stringify(['AO-91', 'AO-91 (FOX-1B)']))
    memStore.setItem('nexus.sats.chasingNorad', JSON.stringify({ 'AO-91': 43017 }))
    expect(toggleSatChasing('AO-91 (FOX-1B)', 43017)).toBe(false)
    expect(satChasingSet().size).toBe(0)
  })

  it('distinct birds sharing no NORAD stay independent (a norad-less toggle never cross-matches)', () => {
    toggleSatChasing('AO-91', 43017)
    expect(toggleSatChasing('SO-50')).toBe(true) // stars, does not touch AO-91
    expect(satChasingSet().has('AO-91')).toBe(true)
    expect(satChasingSet().has('SO-50')).toBe(true)
  })

  it('every star/chip mutation announces itself (same-window sync for map + globe + pane)', () => {
    toggleSatChasing('RS-44', 44909)
    setSatFavOnly(false)
    const types = dispatched.mock.calls.map((c) => (c[0] as Event).type)
    expect(types).toEqual([SAT_CHASE_EVENT, SAT_CHASE_EVENT])
  })

  it('★/All choice defaults to ★ (the operator asked Connect to track favorites) and persists', () => {
    expect(satFavOnly()).toBe(true)
    setSatFavOnly(false)
    expect(satFavOnly()).toBe(false)
    expect(memStore.getItem('nexus.sats.favOnly')).toBe('0')
    setSatFavOnly(true)
    expect(satFavOnly()).toBe(true)
  })
})
