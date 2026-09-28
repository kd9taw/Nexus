// THE SETTINGS PATCH SEAM (patch.ts): a surface's save carries its own fields over the settings the
// backend holds at save time, and every other field comes from that read. The live settings here
// always DISAGREE with the copy a surface would have held, so a save built on the copy fails.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { getSettings, setSettings } from '../api'
import { patchSettings } from './patch'
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
})
