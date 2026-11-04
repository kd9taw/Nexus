import { describe, it, expect, beforeEach } from 'vitest'

// Node test env: in-memory localStorage (the satChase.test.ts shim).
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

import {
  satTpPrefs,
  rememberSatTp,
  toggleSatTpFavorite,
  resolveSatTp,
  isWorkableTp,
} from './satTransponderPref'
import { isDurable, STATION_KEYS } from './durableStore'
import type { SatTransmitter } from '../types'

const KEY = 'nexus.sats.transponders'

const tx = (over: Partial<SatTransmitter>): SatTransmitter => ({
  description: 'x',
  alive: true,
  mode: 'FM',
  uplinkLowHz: null,
  downlinkLowHz: null,
  invert: false,
  uplinkHighHz: null,
  downlinkHighHz: null,
  uplinkMode: null,
  downlinkMode: null,
  kind: 'Transceiver',
  ...over,
})

/** The ISS as SatNOGS lists it, near enough: a dead entry first (the raw-index trap), an APRS
 *  digipeater, two beacons, then the FM voice repeater the operator actually works. */
const ISS = (): SatTransmitter[] => [
  tx({ description: 'Old packet', alive: false, uplinkLowHz: 145_990_000, downlinkLowHz: 437_550_000 }),
  tx({ description: 'APRS digipeater', mode: 'AFSK', uplinkLowHz: 145_825_000, downlinkLowHz: 145_825_000 }),
  tx({ description: 'SSTV', kind: 'Transmitter', downlinkLowHz: 145_800_000 }),
  tx({ description: 'Telemetry', kind: 'Transmitter', mode: 'BPSK', downlinkLowHz: 437_800_000 }),
  tx({ description: 'FM voice repeater', uplinkLowHz: 145_990_000, downlinkLowHz: 437_800_000 }),
]

beforeEach(() => memStore.clear())

describe('the transponder choice is the station’s own data', () => {
  it('is a durable station key, so backups and a computer driving the station carry it', () => {
    expect(isDurable(KEY)).toBe(true)
    expect(STATION_KEYS).toContain(KEY)
  })
})

describe('remember the last pick', () => {
  it('round trips: the row picked last is the one resolved next time', () => {
    rememberSatTp('iss', ISS()[4])
    // Keyed by the upper-case bird name, the ★ set's identity.
    expect(Object.keys(satTpPrefs())).toEqual(['ISS'])
    expect(resolveSatTp('ISS', ISS(), satTpPrefs())).toEqual({ index: 4, source: 'last', gone: null })
  })

  it('is identified by frequencies and mode, never by its place in the list', () => {
    rememberSatTp('ISS', ISS()[4])
    // SatNOGS reorders the list and adds an entry: the same repeater is found at its new place.
    const reordered = [ISS()[4], tx({ description: 'New beacon', kind: 'Transmitter' }), ...ISS().slice(0, 4)]
    expect(resolveSatTp('ISS', reordered, satTpPrefs()).index).toBe(0)
    // A different transmitter at the OLD index is never taken for it.
    const swapped = ISS()
    swapped[4] = tx({ description: 'FM voice repeater', uplinkLowHz: 145_990_000, downlinkLowHz: 437_550_000 })
    expect(resolveSatTp('ISS', swapped, satTpPrefs()).index).toBeNull()
  })

  it('never remembers a beacon or a dead entry: there is nothing to work there', () => {
    rememberSatTp('ISS', ISS()[2])
    rememberSatTp('ISS', ISS()[0])
    expect(satTpPrefs()).toEqual({})
  })

  it('is per bird', () => {
    rememberSatTp('ISS', ISS()[4])
    expect(resolveSatTp('SO-50', ISS(), satTpPrefs())).toEqual({ index: null, source: null, gone: null })
  })
})

describe('the ★ favorite', () => {
  it('beats the last pick', () => {
    toggleSatTpFavorite('ISS', ISS()[4])
    rememberSatTp('ISS', ISS()[1])
    expect(resolveSatTp('ISS', ISS(), satTpPrefs())).toEqual({ index: 4, source: 'favorite', gone: null })
  })

  it('toggles off on the same row, and the last pick takes over again', () => {
    rememberSatTp('ISS', ISS()[1])
    expect(toggleSatTpFavorite('ISS', ISS()[4])).toBe(true)
    expect(toggleSatTpFavorite('ISS', ISS()[4])).toBe(false)
    expect(resolveSatTp('ISS', ISS(), satTpPrefs())).toEqual({ index: 1, source: 'last', gone: null })
  })

  it('moves to another row in one click', () => {
    toggleSatTpFavorite('ISS', ISS()[1])
    expect(toggleSatTpFavorite('ISS', ISS()[4])).toBe(true)
    expect(resolveSatTp('ISS', ISS(), satTpPrefs()).index).toBe(4)
  })

  it('cannot be a beacon', () => {
    expect(toggleSatTpFavorite('ISS', ISS()[2])).toBe(false)
    expect(satTpPrefs()).toEqual({})
  })
})

describe('a saved transponder SatNOGS no longer lists', () => {
  it('falls back to nothing chosen and names the one that is gone', () => {
    toggleSatTpFavorite('ISS', ISS()[4])
    const without = ISS().slice(0, 4)
    expect(resolveSatTp('ISS', without, satTpPrefs())).toEqual({
      index: null,
      source: null,
      gone: 'FM voice repeater',
    })
  })

  it('never resolves to it once it is marked dead', () => {
    toggleSatTpFavorite('ISS', ISS()[4])
    const dead = ISS()
    dead[4] = { ...dead[4], alive: false }
    const r = resolveSatTp('ISS', dead, satTpPrefs())
    expect(r.index).toBeNull()
    expect(r.gone).toBe('FM voice repeater')
  })

  it('a gone favorite still lets the last pick stand, and says the favorite is gone', () => {
    toggleSatTpFavorite('ISS', ISS()[4])
    rememberSatTp('ISS', ISS()[1])
    const without = ISS().slice(0, 4)
    expect(resolveSatTp('ISS', without, satTpPrefs())).toEqual({
      index: 1,
      source: 'last',
      gone: 'FM voice repeater',
    })
  })

  it('an empty list is not knowledge: nothing is called gone', () => {
    toggleSatTpFavorite('ISS', ISS()[4])
    expect(resolveSatTp('ISS', [], satTpPrefs())).toEqual({ index: null, source: null, gone: null })
  })

  it('is kept, so it is found again if SatNOGS lists it again', () => {
    toggleSatTpFavorite('ISS', ISS()[4])
    resolveSatTp('ISS', ISS().slice(0, 4), satTpPrefs())
    expect(resolveSatTp('ISS', ISS(), satTpPrefs()).index).toBe(4)
  })
})

describe('load validation', () => {
  it('reads blocked, corrupt or wrong-shaped storage as nothing saved', () => {
    for (const raw of ['{not json', '[]', '"x"', 'null', '42']) {
      memStore.setItem(KEY, raw)
      expect(satTpPrefs(), raw).toEqual({})
    }
  })

  it('drops entries that cannot name a transponder, and keeps the good ones beside them', () => {
    memStore.setItem(
      KEY,
      JSON.stringify({
        iss: {
          fav: { up: 145_990_000, down: 437_800_000, mode: 'FM', label: 'FM voice repeater' },
          last: { up: 'soon', down: -5, mode: 3, label: 'junk' },
        },
        'SO-50': { fav: { up: null, down: null, mode: 'FM', label: 'nothing to match' } },
        'AO-91': 'not an object',
        'AO-7': { last: { up: 1e400, down: Number.NaN, mode: null, label: 'x' } },
      }),
    )
    expect(satTpPrefs()).toEqual({
      ISS: { fav: { up: 145_990_000, down: 437_800_000, mode: 'FM', label: 'FM voice repeater' } },
    })
  })

  it('caps a label that is too long to be one', () => {
    memStore.setItem(
      KEY,
      JSON.stringify({ ISS: { fav: { up: 145_990_000, down: 437_800_000, mode: 'FM', label: 'y'.repeat(5000) } } }),
    )
    expect(satTpPrefs().ISS.fav!.label.length).toBeLessThanOrEqual(200)
  })

  it('isWorkableTp: an alive transponder or repeater, nothing else', () => {
    expect(ISS().map(isWorkableTp)).toEqual([false, true, false, false, true])
  })
})
