// What each answer to the first-run question changes, and that "not asked" changes nothing.
import { describe, it, expect } from 'vitest'
import { featurePreset, finishDefaults, foldsSettings, startsFolded, EXPERIENCE_LEVELS } from './experience'
import { applyProfiles } from '../features/state'
import { SETTINGS_SECTIONS } from './registry'

const enabledFor = (level: Parameters<typeof featurePreset>[0]) => {
  const p = featurePreset(level)
  return applyProfiles(p.profiles, [], p.extraOn).enabled
}

describe('the experience answer', () => {
  it('offers the three answers, in order', () => {
    expect(EXPERIENCE_LEVELS.map((l) => l.value)).toEqual(['newToHobby', 'newToDigital', 'experienced'])
  })

  it('starts a new ham on the starter set: no Phone, CW or contest screens', () => {
    const on = enabledFor('newToHobby')
    expect(on.operate).toBe(true)
    expect(on.logbook).toBe(true)
    expect(on.phone).toBe(false)
    expect(on.cw).toBe(false)
    expect(on.rtty).toBe(false)
  })

  it('keeps Phone and CW on for someone new only to digital', () => {
    const on = enabledFor('newToDigital')
    expect(on.phone).toBe(true)
    expect(on.cw).toBe(true)
    // …and is otherwise the starter set.
    expect(on.rtty).toBe(false)
    expect(on.awards).toBe(false)
  })

  it('turns everything on for an experienced operator, and when nothing was asked', () => {
    for (const level of ['experienced', undefined, null] as const) {
      expect(featurePreset(level)).toEqual({ profiles: ['everything'], extraOn: [] })
    }
    const on = enabledFor('experienced')
    expect(on.rtty && on.phone && on.cw && on.awards).toBe(true)
  })

  it("sets the wizard's last step per answer: packs, guide and walk", () => {
    expect(finishDefaults('newToHobby')).toEqual({ packs: true, guide: true, walk: true })
    expect(finishDefaults('newToDigital')).toEqual({ packs: true, guide: false, walk: false })
    expect(finishDefaults('experienced')).toEqual({ packs: false, guide: false, walk: false })
    // Not asked: the start every earlier build had.
    expect(finishDefaults(undefined)).toEqual({ packs: true, guide: false, walk: false })
  })

  it('folds Settings to the first-hour sections for the two new answers only', () => {
    expect(foldsSettings('newToHobby')).toBe(true)
    expect(foldsSettings('newToDigital')).toBe(true)
    expect(foldsSettings('experienced')).toBe(false)
    expect(foldsSettings(undefined)).toBe(false)
  })

  it('never folds a section a first-hour operator needs, and folds the rest', () => {
    const hourOne = SETTINGS_SECTIONS.filter((s) => s.neededInHourOne)
    expect(hourOne.length).toBeGreaterThan(5)
    for (const s of hourOne) expect(startsFolded(s.id, 'newToHobby'), s.id).toBe(false)
    // Positive control: a section nobody needs in hour one does fold.
    expect(startsFolded('theme', 'newToHobby')).toBe(true)
    expect(startsFolded('theme', 'experienced')).toBe(false)
    expect(startsFolded('theme', undefined)).toBe(false)
    // A nested section follows its parent rather than folding by itself.
    const nested = SETTINGS_SECTIONS.find((s) => s.parent)
    expect(nested).toBeTruthy()
    expect(startsFolded(nested!.id, 'newToHobby')).toBe(false)
    expect(startsFolded('no-such-section', 'newToHobby')).toBe(false)
  })
})
