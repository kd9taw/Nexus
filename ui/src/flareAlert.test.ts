import { describe, it, expect, vi, beforeEach } from 'vitest'
import { processFlare, effectiveXray, flareToastGrid, resetFlareAlerts } from './flareAlert'
import { pushToast } from './toast'
import { doubleBeep } from './alerts'
import { flareFootprint, flareRecovery } from './flareNow'
import { flareHafAt, flareHafMhz, flareRecoveryMin } from './mapGeo'
import { t } from './i18n'
import type { DrapGrid } from './types'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
vi.mock('./alerts', () => ({ doubleBeep: vi.fn() }))

const toasts = vi.mocked(pushToast)
const beeps = vi.mocked(doubleBeep)

beforeEach(() => {
  resetFlareAlerts()
  toasts.mockClear()
  beeps.mockClear()
})

describe('processFlare (edge-triggered flare heads-up)', () => {
  it('quiet toast, no beep, for an M1–M4 (tier 1) onset — once per event', () => {
    processFlare(2e-5) // M2
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('M2.0')
    expect(toasts.mock.calls[0][0]).toContain('R1')
    expect(toasts.mock.calls[0][1]).toBe('info')
    expect(toasts.mock.calls[0][3], 'a flare is an alert (#391)').toMatchObject({ alert: true })
    expect(beeps).not.toHaveBeenCalled()
    processFlare(3e-5) // still tier 1 → silent
    processFlare(1.2e-5)
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('prominent toast + beep at M5+ and again on escalation to X', () => {
    processFlare(6e-5) // M6 (tier 2)
    expect(beeps).toHaveBeenCalledTimes(1)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][3]).toMatchObject({ alert: true, prominent: true })
    processFlare(2e-4) // X2 (tier 3) — the escalation re-alert
    expect(toasts).toHaveBeenCalledTimes(2)
    expect(toasts.mock.calls[1][0]).toContain('X2.0')
    expect(toasts.mock.calls[1][0]).toContain('R3')
    processFlare(1.5e-4) // still tier 3 → silent
    expect(toasts).toHaveBeenCalledTimes(2)
  })

  it('does not re-fire during decay; re-arms only after flux holds below C5', () => {
    processFlare(2e-5) // M2 → toast
    processFlare(8e-6) // decayed below M1 but above C5 → not yet re-armed
    processFlare(2e-5) // wobble back up → same event, no second toast
    expect(toasts).toHaveBeenCalledTimes(1)
    processFlare(1e-6) // below C5 → event over, re-armed
    vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 61 * 60_000) // past cooldown
    processFlare(2e-5) // a NEW event → alerts again
    expect(toasts).toHaveBeenCalledTimes(2)
    vi.restoreAllMocks()
  })

  it('a cooldown-suppressed second event stays ARMED and fires once the cooldown lapses', () => {
    // The clustered-flare case (one active region, multiple M/X within an hour):
    // suppression must delay the alert, never permanently drop it.
    const t0 = Date.now()
    const clock = vi.spyOn(Date, 'now')
    clock.mockReturnValue(t0)
    processFlare(6e-5) // M6 fires (tier 2)
    expect(toasts).toHaveBeenCalledTimes(1)
    processFlare(1e-6) // below C5 → event over, re-armed
    clock.mockReturnValue(t0 + 35 * 60_000)
    processFlare(6e-5) // second M6, inside the 60-min cooldown → suppressed for now
    expect(toasts).toHaveBeenCalledTimes(1)
    clock.mockReturnValue(t0 + 61 * 60_000)
    processFlare(6e-5) // still M6, cooldown lapsed → the delayed alert fires
    expect(toasts).toHaveBeenCalledTimes(2)
    processFlare(6e-5) // and only once
    expect(toasts).toHaveBeenCalledTimes(2)
    clock.mockRestore()
  })

  it('includes the D-RAP ceiling and recovery estimate in the copy', () => {
    processFlare(1e-4) // X1 → HAF 25 MHz, recovery ≈58 min
    const msg = toasts.mock.calls[0][0]
    expect(msg).toContain('below ~25 MHz')
    expect(msg).toMatch(/fade ~5[0-9] min/)
  })

  it('ignores null / quiet-sun readings', () => {
    processFlare(null)
    processFlare(1e-7)
    expect(toasts).not.toHaveBeenCalled()
  })
})

describe('effectiveXray (fast lane / snapshot merge)', () => {
  it('prefers the fresher fast-lane reading, falls back to the snapshot', () => {
    expect(effectiveXray(2e-5, 1e-4)).toBe(2e-5)
    expect(effectiveXray(null, 1e-4)).toBe(1e-4)
    expect(effectiveXray(undefined, undefined)).toBeNull()
    expect(effectiveXray(0, 0)).toBeNull()
  })
})

// ONE CEILING EVERYWHERE: the toast quotes the map's footprint — NOAA's D-RAP grid while it is
// current, else the GOES estimate — and says which, so it never contradicts the flare chip.
describe('the toast quotes the map’s ceiling, with its source', () => {
  const M5 = 5e-5
  /** A grid in NOAA's layout, 30 % hotter than the GOES estimate, valid `ageS` seconds ago. */
  function hotGrid(nowMs: number, ageS: number): DrapGrid {
    const lats = Array.from({ length: 90 }, (_, i) => 89 - 2 * i)
    const lons = Array.from({ length: 90 }, (_, i) => -178 + 4 * i)
    const haf: number[] = []
    for (const la of lats) for (const lo of lons) haf.push(flareHafAt(la, lo, nowMs, M5) * 1.3)
    return { validAt: nowMs / 1000 - ageS, recoveryAt: null, lats, lons, haf }
  }

  it("quotes NOAA's grid while it is current, and says so", () => {
    const now = Date.now()
    const drap = hotGrid(now, 60)
    const foot = flareFootprint(now, M5, drap)!
    expect(foot.source, 'CONTROL: the grid is the footprint').toBe('noaa')
    expect(Math.round(foot.hafSub), 'CONTROL: NOAA differs from GOES').not.toBe(Math.round(flareHafMhz(M5)))
    processFlare(M5, drap)
    expect(toasts.mock.calls[0][0]).toBe(
      t('prop.flareAlert.noaa.fade', {
        cls: 'M5.0',
        r: 2,
        mhz: Math.round(foot.hafSub),
        mins: Math.round(flareRecoveryMin(M5)!),
      }),
    )
  })

  it('says "estimated from GOES" beside a stale grid or none', () => {
    const now = Date.now()
    for (const drap of [hotGrid(now, 10 * 60), null]) {
      resetFlareAlerts()
      toasts.mockClear()
      processFlare(M5, drap)
      expect(toasts.mock.calls[0][0]).toBe(
        t('prop.flareAlert.fade', {
          cls: 'M5.0',
          r: 2,
          mhz: Math.round(flareHafMhz(M5)),
          mins: Math.round(flareRecoveryMin(M5)!),
        }),
      )
      expect(toasts.mock.calls[0][0]).not.toContain('NOAA D-RAP')
    }
  })
})

// ONE RECOVERY TIME EVERYWHERE: the toast's fade is the chip's (flareRecovery), and says which it
// is: NOAA's own recovery time from a current grid, else NOAA's rule of thumb from the X-ray level.
describe("the toast's fade, with its source", () => {
  const M5 = 5e-5
  function noaaGrid(nowMs: number, ageS: number, recoveryAt: number | null): DrapGrid {
    const lats = Array.from({ length: 90 }, (_, i) => 89 - 2 * i)
    const lons = Array.from({ length: 90 }, (_, i) => -178 + 4 * i)
    const haf: number[] = []
    for (const la of lats) for (const lo of lons) haf.push(flareHafAt(la, lo, nowMs, M5))
    return { validAt: nowMs / 1000 - ageS, recoveryAt, lats, lons, haf }
  }

  it("quotes NOAA's own recovery time from a current grid, as the chip does, and says so", () => {
    const now = Date.now()
    const drap = noaaGrid(now, 60, now / 1000 + 38 * 60)
    expect(flareRecovery(drap, M5, now), "CONTROL: the chip's source").toEqual({ mins: 38, noaa: true })
    processFlare(M5, drap)
    const msg = toasts.mock.calls[0][0]
    expect(msg).toBe(
      t('prop.flareAlert.noaa.fadeOwn', { cls: 'M5.0', r: 2, mhz: Math.round(flareFootprint(now, M5, drap)!.hafSub), mins: 38 }),
    )
    expect(msg).toContain("fade ~38 min (NOAA's own estimate)")
    expect(msg).not.toContain('rule of thumb')
  })

  it("CONTROL: without NOAA's time it quotes the rule of thumb and says so", () => {
    const now = Date.now()
    const fit = Math.round(flareRecoveryMin(M5)!)
    // No grid; a stale grid that carries a time; a current grid without one.
    for (const drap of [null, noaaGrid(now, 10 * 60, now / 1000 + 38 * 60), noaaGrid(now, 60, null)]) {
      resetFlareAlerts()
      toasts.mockClear()
      expect(flareRecovery(drap, M5, now), "CONTROL: the chip's source").toEqual({ mins: fit, noaa: false })
      processFlare(M5, drap)
      const msg = toasts.mock.calls[0][0]
      expect(msg).toContain(`fade ~${fit} min (NOAA's rule of thumb from the X-ray level)`)
      expect(msg).not.toContain("NOAA's own estimate")
    }
  })
})

// The toast's grid is NOAA's only beside NOAA's own fresh flux: a simulated flux (the dev
// override) or an old GOES sample toasted as "(NOAA D-RAP)" would put NOAA's name on our numbers.
describe('flareToastGrid', () => {
  const now = Date.now()
  const drap = { validAt: now / 1000 - 60, recoveryAt: null, lats: [10, 0], lons: [0, 10], haf: [5, 5, 5, 5] }
  const lane = { flux: 5e-5, asOf: now / 1000 - 60, drap }

  it("POSITIVE CONTROL: a fresh lane's grid", () => {
    expect(flareToastGrid(lane, now)).toBe(drap)
  })

  it('none from a stale lane, an absent lane or a lane without one', () => {
    expect(flareToastGrid({ ...lane, asOf: now / 1000 - 11 * 60 }, now)).toBeNull()
    expect(flareToastGrid(null, now)).toBeNull()
    expect(flareToastGrid({ ...lane, drap: undefined }, now)).toBeNull()
  })

  it("none beside the dev override's simulated flux, even with a current grid", () => {
    localStorage.setItem('nexus.dev.xray', String(1e-4))
    try {
      expect(flareToastGrid(lane, now)).toBeNull()
    } finally {
      localStorage.removeItem('nexus.dev.xray')
    }
    expect(flareToastGrid(lane, now), 'CONTROL: the override is gone').toBe(drap)
  })
})
