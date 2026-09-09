// JS8 ships ON (operator ruling 2026-09: "new releases should have js8call on by default, so users
// don't have to turn it on"). Spec B8's `defaultOff` staging flag is gone, so this file no longer
// pins the staged state — it pins the NEW intent, which has three halves a half-flip would break:
// JS8 resolves ON for a fresh install AND for an upgrading operator who never saw the toggle; it is
// a sibling of the other digital modes, not a goal (so a goal profile still leaves it to the
// operator, exactly as it leaves RTTY); and VISIBILITY is the only gate that opened — the two-act
// transmit arm is untouched and pinned in Rust (`js8_enter_keys_nothing_and_lands_on_the_watering_hole`,
// `js8_enter_keys_nothing_even_with_tx_enabled`, `js8_autoreply_never_keys_at_launch`).
import { describe, expect, it } from 'vitest'
import { featureById } from './registry'
import { PROFILES, resolveEnabled, type ProfileId } from './profiles'
import { coerceEnabled, defaultState } from './state'

describe('the JS8 feature is a normal Operate section', () => {
  it('is a non-core section, no longer staged, whose view is itself', () => {
    const f = featureById('js8')
    expect(f).toBeDefined()
    expect(f!.kind).toBe('section')
    expect(f!.category).toBe('Operate')
    expect(f!.core).toBe(false)
    expect(f!.defaultOff, 'the staging flag is gone — JS8 is on by default').toBeUndefined()
    expect(f!.view).toBe('js8')
    expect(f!.dependsOn).toEqual([])
    expect(f!.intents).toEqual([])
    expect(f!.label).toBe('JS8') // the mode's own name — an invariant token, never translated
    expect(f!.oneLine.length).toBeGreaterThan(20)
  })

  it("is ON under the 'everything' profile", () => {
    expect(resolveEnabled('everything').js8).toBe(true)
  })

  it('is ON for a fresh install and for an upgrading operator who never saw the toggle', () => {
    // The first-run set (everything-except-Field-Day).
    expect(defaultState().enabled.js8).toBe(true)
    // A state persisted before the flip carries no `js8` key at all: the missing-key default is
    // what turns it on for an upgrade, and it is `!defaultOff`. This is the half a flip that only
    // touched 'everything' would miss.
    expect(coerceEnabled({ operate: true }).js8).toBe(true)
  })

  it('still respects an operator who turned it off', () => {
    expect(coerceEnabled({ js8: false }).js8).toBe(false)
  })

  it('is a mode, not a goal: every profile treats it exactly as it treats RTTY', () => {
    // Empty `intents`, like CW/Phone/RTTY/PSK/SSTV/APRS — a goal profile never auto-enables a
    // mode. Pinned as PARITY with a sibling rather than as a hard-coded false, so the day RTTY
    // joins a goal profile this test asks whether JS8 should too instead of silently disagreeing.
    for (const id of Object.keys(PROFILES) as ProfileId[]) {
      const en = resolveEnabled(id)
      expect(en.js8, id).toBe(en.rtty)
    }
  })
})
