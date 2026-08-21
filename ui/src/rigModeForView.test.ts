// #143 — switching CW → FT left the rig on the CW frequency.
//
// The bug was a SEQUENCE, which is why it survived: no single view transition is wrong, and
// nothing that looked at one transition at a time could see it. These walk routes.
import { describe, it, expect } from 'vitest'
import { rigModeTransition, RIG_MODE_BY_VIEW, type RigMode } from './rigModeForView'

/** Walk a route and return the views that re-homed the dial. */
function homesAlong(views: string[], start: RigMode = 'digital'): string[] {
  let homed = start
  const out: string[] = []
  for (const v of views) {
    const t = rigModeTransition(v, homed)
    if (t.followFreq) out.push(v)
    homed = t.nextHomed
  }
  return out
}

describe('#143 — the route that broke it', () => {
  it('re-homes on the FT screen even after Tempo asserted the digital mode', () => {
    // ve3wej's route, in his words: "tabbing through each" — and his tab list has tempo in it.
    // Tempo maps to the digital rig mode but owns no frequency, so it asserts without homing.
    // Before the fix it still advanced the guard, and the FT screen then saw "no change" and
    // left FT8 sitting on the CW dial.
    expect(homesAlong(['cw', 'chat', 'operate'])).toEqual(['cw', 'operate'])
  })

  it('still re-homes on the direct route, which never broke', () => {
    expect(homesAlong(['cw', 'operate'])).toEqual(['cw', 'operate'])
  })

  it('re-homes going the other way too — FT then CW', () => {
    // START FROM PHONE, and that is not incidental. The guard's initial value is 'digital'
    // (what the app boots holding), so entering Operate from a cold start correctly does NOT
    // home — launch is a read-only act and the rig is where the operator left it. The first
    // version of this test started there and expected a home, which was the test being wrong
    // rather than the code. Coming from a genuinely different mode is the case it means.
    expect(homesAlong(['operate', 'chat', 'cw'], 'phone')).toEqual(['operate', 'cw'])
  })
})

describe('what must NOT change', () => {
  it('returning to a mode you are already in never yanks the VFO', () => {
    // The rule the guard exists for in the first place.
    expect(homesAlong(['cw', 'operate', 'operate'])).toEqual(['cw', 'operate'])
  })

  it('a glance at a non-operating view touches nothing and advances nothing', () => {
    // Map/Logbook/Settings mid-QSO: no mode, no home, and crucially the guard is left alone
    // so a LATER operate click still homes.
    expect(homesAlong(['cw', 'map', 'logbook', 'operate'])).toEqual(['cw', 'operate'])
    expect(rigModeTransition('map', 'cw')).toEqual({ followFreq: false, nextHomed: 'cw' })
  })

  it('Tempo asserts the digital mode but never homes', () => {
    const t = rigModeTransition('chat', 'cw')
    expect(t.mode, 'Tempo is a digital mode and must still assert it').toBe('digital')
    expect(t.followFreq, 'but it keeps its own band picker frequency').toBe(false)
    expect(t.nextHomed, 'and must not claim a home it did not do').toBe('cw')
  })

  it('a view with no rig mode asserts nothing — the #80 rule', () => {
    // POTA/SOTA carries a `workspace` for layout reasons and is not a mode. An earlier
    // fallthrough flipped an FT-991A from USB to D-U on opening it.
    expect(rigModeTransition('pota', 'phone').mode).toBeUndefined()
    expect(RIG_MODE_BY_VIEW.pota).toBeUndefined()
  })

  it('every cockpit that owns a frequency still homes from a different mode', () => {
    for (const v of ['operate', 'cw', 'phone', 'rtty', 'psk']) {
      const from: RigMode = v === 'cw' ? 'digital' : 'cw'
      expect(rigModeTransition(v, from).followFreq, `${v} must re-home`).toBe(true)
    }
  })

  it('control: the map is not empty and the helper can say no', () => {
    // A "nothing found" guard is worthless if the map it reads is empty.
    expect(Object.keys(RIG_MODE_BY_VIEW).length).toBeGreaterThan(4)
    expect(rigModeTransition('operate', 'digital').followFreq).toBe(false)
  })
})
