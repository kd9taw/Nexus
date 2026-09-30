import { describe, it, expect } from 'vitest'
import { follow, pending, STALLS_TO_DROP } from './rotorTargets'

/** Walk a target through a run of readings. */
const through = (deg: number, readings: (number | null)[], wraps: boolean) =>
  readings.reduce<ReturnType<typeof follow>>((p, r) => follow(p, r, wraps), pending(deg))

describe('a pending target lives exactly as long as the slew to it', () => {
  it('stays while the mast closes on it, and goes when it arrives', () => {
    // A G-5500's elevation climbing to 30° at ~2.7°/s, read every 2 s.
    expect(through(30, [0, 5, 11, 16], false)).toMatchObject({ deg: 30, stalls: 0 })
    expect(through(30, [0, 5, 11, 16, 22, 28.5], false)).toBeNull()
  })

  it('goes when the mast stops short: STOP, a jam, the controller’s own buttons', () => {
    const stuck = [0, 6, 12, 12, 12]
    expect(through(30, stuck.slice(0, 4), false)).not.toBeNull()
    expect(through(30, stuck, false)).toBeNull()
    expect(STALLS_TO_DROP).toBe(2)
  })

  it('goes when something else sends the mast the other way', () => {
    // ↗ from another screen retargets the azimuth: the mast turns away from 200°.
    expect(through(200, [150, 160, 150, 140], true)).toBeNull()
  })

  it('measures an azimuth the short way round', () => {
    // 350° → 10° is 20° of travel through north, not 340°.
    expect(through(10, [350, 358], true)).toMatchObject({ gap: 12 })
    expect(through(10, [350, 358, 9], true)).toBeNull()
  })

  it('is kept when the rotator reports nothing: it is all that is known', () => {
    expect(through(45, [null, null, null], true)).toMatchObject({ deg: 45 })
  })
})
