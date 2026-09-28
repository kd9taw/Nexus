// THE SETTINGS PATCH SEAM (patch.ts): a surface's save carries its own fields over the settings the
// backend holds at save time, and every other field comes from that read. The live settings here
// always DISAGREE with the copy a surface would have held, so a save built on the copy fails.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { getSettings, setSettings } from '../api'
import { changedSince, patchSettings } from './patch'
import defaultSettings from '../components/__fixtures__/defaultSettings.json'
import type { Settings } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(async () => ({})),
}))

/** The settings a surface read when it opened… */
const held = { ...defaultSettings, simultaneousRadios: true, dialMhz: 14.025, fdBonuses: [] } as unknown as Settings
/** …and what the backend holds by the time it saves. */
const live = { ...held, simultaneousRadios: false, dialMhz: 7.03, fdBonuses: ['youth'] } as Settings

const saved = () => vi.mocked(setSettings).mock.calls[0][0] as unknown as Record<string, unknown>

beforeEach(() => {
  vi.mocked(getSettings).mockReset()
  vi.mocked(setSettings).mockClear()
})

describe('patchSettings', () => {
  it('saves the edit over the live settings, every other field included', async () => {
    vi.mocked(getSettings).mockResolvedValue(live)
    await patchSettings(() => ({ fdPowerMult: 5 }))
    expect(saved()).toEqual({ ...live, fdPowerMult: 5 })
    expect(saved().simultaneousRadios).toBe(false)
    expect(saved().dialMhz).toBe(7.03)
    // A whole struct, the shape `set_settings` has always received: a missing key would
    // deserialise to its default.
    expect(Object.keys(saved()).sort()).toEqual(Object.keys(live).sort())
  })

  it('hands the edit the live settings, so a change to the current value starts from it', async () => {
    vi.mocked(getSettings).mockResolvedValue(live)
    await patchSettings((s) => ({ fdBonuses: [...(s.fdBonuses ?? []), 'safety-officer'] }))
    expect(saved().fdBonuses).toEqual(['youth', 'safety-officer'])
  })

  it('writes nothing when the settings cannot be read', async () => {
    vi.mocked(getSettings).mockRejectedValue(new Error('engine busy'))
    await expect(patchSettings(() => ({ fdPowerMult: 5 }))).rejects.toThrow('engine busy')
    expect(setSettings).not.toHaveBeenCalled()
  })

  it('runs back-to-back patches one after another, so the second reads what the first wrote', async () => {
    // The backend's write lands a moment after it is sent, as a real save does.
    let stored = { ...live, fdBonuses: [] } as Settings
    vi.mocked(getSettings).mockImplementation(async () => stored)
    const landsLater = async (s: Settings) => {
      await new Promise((r) => setTimeout(r, 5))
      stored = s
      return {} as never
    }
    vi.mocked(setSettings).mockImplementationOnce(landsLater).mockImplementationOnce(landsLater)
    await Promise.all([
      patchSettings((s) => ({ fdBonuses: [...(s.fdBonuses ?? []), 'youth'] })),
      patchSettings((s) => ({ fdBonuses: [...(s.fdBonuses ?? []), 'safety-officer'] })),
    ])
    expect(stored.fdBonuses).toEqual(['youth', 'safety-officer'])
  })

  it('a failed patch does not hold up the next one', async () => {
    vi.mocked(getSettings).mockRejectedValueOnce(new Error('engine busy')).mockResolvedValue(live)
    await expect(patchSettings(() => ({ fdPowerMult: 5 }))).rejects.toThrow('engine busy')
    await patchSettings(() => ({ fdPowerMult: 1 }))
    expect(saved().fdPowerMult).toBe(1)
  })
})

// THE SETTINGS FORM'S SIDE (`changedSince`): its Save sends what the form changed since it loaded,
// so a field the form writes and this misses is an operator's edit that never reaches the station.
describe('changedSince', () => {
  const loaded = { ...defaultSettings, fdOperator: 'K1ABC' } as unknown as Settings

  /** A value of the same kind that serialises differently. */
  const other = (v: unknown): unknown =>
    typeof v === 'boolean' ? !v
      : typeof v === 'number' ? v + 1
        : typeof v === 'string' ? `${v}~`
          : Array.isArray(v) ? [...v, 'probe']
            : v && typeof v === 'object' ? { ...v, probe: 1 }
              : 'probe'

  it('is what the form changed and nothing else', () => {
    expect(changedSince(loaded, { ...loaded, fdOperator: 'N0AAA' })).toEqual({ fdOperator: 'N0AAA' })
  })

  it('counts a change to every field the settings carry, whatever its kind', () => {
    // Every field of the settings fixture: booleans, numbers, strings, lists, objects and nulls.
    // `changedSince` never names a field, so its kinds are what there is to cover.
    const keys = Object.keys(loaded)
    expect(keys.length).toBeGreaterThan(200)
    const was = loaded as unknown as Record<string, unknown>
    const missed = keys.filter((key) => {
      const form = { ...was, [key]: other(was[key]) } as unknown as Settings
      const changes = changedSince(loaded, form) as Record<string, unknown>
      return JSON.stringify(changes[key]) !== JSON.stringify(other(was[key]))
    })
    expect(missed).toEqual([])
  })

  it('counts a field the form has and the loaded settings lacked', () => {
    const form = { ...loaded, fieldFromANewerForm: 1 } as unknown as Settings
    expect(changedSince(loaded, form)).toEqual({ fieldFromANewerForm: 1 })
  })

  it('moves the tune as one: a dial changed in the form saves its band and sideband with it', () => {
    const tuned = { ...loaded, dialMhz: 14.074, band: '20m', sideband: 'USB' } as Settings
    expect(changedSince(tuned, { ...tuned, dialMhz: 14.2 })).toEqual({ dialMhz: 14.2, band: '20m', sideband: 'USB' })
  })

  it('a value rebuilt equal is not a change', () => {
    expect(changedSince(loaded, structuredClone(loaded))).toEqual({})
  })

  it('with nothing loaded to compare against, is the whole form', () => {
    expect(changedSince(null, loaded)).toBe(loaded)
  })
})
