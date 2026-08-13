import { describe, it, expect, beforeEach } from 'vitest'

// profiles persist through localStorage; this suite runs in the default node
// environment (no jsdom), so install a minimal in-memory localStorage shim.
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
  loadProfiles,
  mergeProfile,
  saveProfile,
  deleteProfile,
  PROFILE_SCHEMA,
  type Profile,
} from './profiles'
import type { Settings } from './types'

// A minimal Settings stub — profiles snapshot the whole object verbatim, so the test
// only needs a couple of distinguishing fields.
const settings = (over: Partial<Settings>): Settings =>
  ({ mycall: 'KD9TAW', serialPort: 'COM1', phoneMode: 'ssb', ...over }) as Settings

describe('config profiles', () => {
  beforeEach(() => memStore.clear())

  it('starts empty and tolerates corrupt storage', () => {
    expect(loadProfiles()).toEqual([])
    localStorage.setItem('nexus.profiles', '{not json')
    expect(loadProfiles()).toEqual([])
    localStorage.setItem('nexus.profiles', '{"not":"an array"}')
    expect(loadProfiles()).toEqual([])
  })

  it('saves and reloads a named snapshot', () => {
    saveProfile('Portable VHF', settings({ serialPort: 'COM7', phoneMode: 'fm' }))
    const list = loadProfiles()
    expect(list).toHaveLength(1)
    expect(list[0].name).toBe('Portable VHF')
    expect(list[0].settings.serialPort).toBe('COM7')
    expect(list[0].settings.phoneMode).toBe('fm')
  })

  it('upserts by name and keeps the list name-sorted', () => {
    saveProfile('Home HF', settings({ serialPort: 'COM3' }))
    saveProfile('Field Day', settings({ serialPort: 'COM5' }))
    saveProfile('Home HF', settings({ serialPort: 'COM9' })) // overwrite, not duplicate
    const list = loadProfiles()
    expect(list.map((p: Profile) => p.name)).toEqual(['Field Day', 'Home HF'])
    expect(list.find((p) => p.name === 'Home HF')!.settings.serialPort).toBe('COM9')
  })

  it('ignores a blank name and deletes by name', () => {
    saveProfile('  ', settings({}))
    expect(loadProfiles()).toEqual([])
    saveProfile('A', settings({}))
    saveProfile('B', settings({}))
    deleteProfile('A')
    expect(loadProfiles().map((p) => p.name)).toEqual(['B'])
  })

  it('stamps the schema on save', () => {
    saveProfile('Home HF', settings({}))
    expect(loadProfiles()[0].schema).toBe(PROFILE_SCHEMA)
  })
})

// The three-week window that made merge-loading real: profiles saved
// 2026-06-30…07-22 predate the per-mode power ceilings, and the raw replay let
// serde's absent-key-takes-default turn "no key" into "NO CAP" at 100% digital
// duty — the one audit finding with a hardware-damage path.
describe('mergeProfile — the profile-load contract', () => {
  const current = {
    mycall: 'KD9TAW',
    licenseClass: 'general',
    maxPowerDigital: 0.25,
    maxPowerPhone: 0.8,
    band: '20m',
    dialMhz: 14.074,
    activeRadio: 1,
    radios: [{ id: 0 }, { id: 1 }],
    qrzLastSyncUnix: 1_753_000_000,
    lotwLastQsl: '2025-01-15',
  } as unknown as Settings

  it('a key the profile predates keeps the CURRENT value — never the default', () => {
    // An old (pre-ceiling) profile: no maxPower* keys at all.
    const old = { band: '80m', dialMhz: 3.573 } as unknown as Settings
    const merged = mergeProfile(current, old) as unknown as Record<string, unknown>
    expect(merged.maxPowerDigital).toBe(0.25) // the safety cap SURVIVES the load
    expect(merged.maxPowerPhone).toBe(0.8)
    expect(merged.band).toBe('80m') // what the profile does carry applies
    expect(merged.dialMhz).toBe(3.573)
  })

  it('identity, license class, the radio roster and sync cursors never import', () => {
    const foreign = {
      mycall: 'N0CALL',
      licenseClass: 'open',
      radios: [{ id: 9 }],
      activeRadio: 9,
      qrzLastSyncUnix: 0,
      band: '40m',
    } as unknown as Settings
    const merged = mergeProfile(current, foreign) as unknown as Record<string, unknown>
    expect(merged.mycall).toBe('KD9TAW')
    expect(merged.licenseClass).toBe('general') // the TX-safety gate stays yours
    expect(merged.activeRadio).toBe(1)
    expect((merged.radios as { id: number }[])[0].id).toBe(0)
    expect(merged.qrzLastSyncUnix).toBe(1_753_000_000)
    expect(merged.band).toBe('40m')
  })

  it('the LoTW download cursor never imports either', () => {
    // Same class as `qrzLastSyncUnix` and `eqslLastSync`, and it was the one missing from the
    // list. A cursor says "I have already fetched everything up to here" — true of the machine
    // that wrote the profile, not of this one. Importing a NEWER stamp makes Nexus skip the
    // confirmations between the two, and they never come back on their own: the next sync asks
    // only for records after the borrowed date. Silent, and it costs award credit the operator
    // has already earned.
    const foreign = { lotwLastQsl: '2026-08-01', band: '40m' } as unknown as Settings
    const merged = mergeProfile(current, foreign) as unknown as Record<string, unknown>
    expect(merged.lotwLastQsl).toBe('2025-01-15')
    expect(merged.band).toBe('40m') // control: the profile's real settings still apply
  })

  it('a profile value the operator set DOES override the current one', () => {
    const p = { maxPowerDigital: 0.5 } as unknown as Settings
    const merged = mergeProfile(current, p) as unknown as Record<string, unknown>
    expect(merged.maxPowerDigital).toBe(0.5)
  })
})

// Round 2, defect 3. `confirmSatUplink` (features/satVfo.ts) is THE
// uplink-mapping writer — the retire-on-change rule lives there, in both
// languages. `mergeProfile` was a third writer: a profile saved by 0.25.0
// carries `satVfoMap` and no `satUplinkRadios`, so loading it re-pointed the
// mapping while every standing confirmation survived — a radio left listed as
// confirmed for a layout it never confirmed, the exact state the rule exists
// to make unreachable. And a post-0.26 profile carries `satUplinkRadios` (raw
// `RadioProfile.id`s) while `radios`/`activeRadio` — the ids' meaning — are
// declared machine-local and refused. The pair is machine wiring, exactly like
// the roster it describes: neither key rides a profile.
describe('mergeProfile — the satellite uplink pair never imports', () => {
  const current = {
    mycall: 'KD9TAW',
    satVfoMap: 'main-down-sub-up',
    satUplinkRadios: [0],
    band: '20m',
  } as unknown as Settings

  it('a pre-0.26 profile cannot re-point the mapping under standing confirmations', () => {
    const old = { satVfoMap: 'a-up-b-down', band: '70cm' } as unknown as Settings
    const merged = mergeProfile(current, old) as unknown as Record<string, unknown>
    expect(merged.satVfoMap).toBe('main-down-sub-up')
    expect(merged.satUplinkRadios).toEqual([0])
    expect(merged.band).toBe('70cm') // the rest of the profile still applies
  })

  it('a post-0.26 profile cannot import consent ids for a roster it did not bring', () => {
    const foreign = {
      satVfoMap: 'main-down-sub-up',
      satUplinkRadios: [2], // id 2 on the SAVING machine — a different rig here
    } as unknown as Settings
    const merged = mergeProfile(current, foreign) as unknown as Record<string, unknown>
    expect(merged.satUplinkRadios).toEqual([0])
  })
})
