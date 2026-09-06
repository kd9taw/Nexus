// JS8 ships STAGED (spec B8: release with defaultOff: true; flip after bench sign-off). This
// pins the staging flags so a later "let's turn it on" is a deliberate edit here, not a drift.
import { describe, expect, it } from 'vitest'
import { featureById } from './registry'
import { resolveEnabled } from './profiles'

describe('the JS8 feature is a staged Operate section', () => {
  it('is a non-core, defaultOff section whose view is itself', () => {
    const f = featureById('js8')
    expect(f).toBeDefined()
    expect(f!.kind).toBe('section')
    expect(f!.category).toBe('Operate')
    expect(f!.core).toBe(false)
    expect(f!.defaultOff).toBe(true)
    expect(f!.view).toBe('js8')
    expect(f!.dependsOn).toEqual([])
    expect(f!.intents).toEqual([])
    expect(f!.label).toBe('JS8') // the mode's own name — an invariant token, never translated
    expect(f!.oneLine.length).toBeGreaterThan(20)
  })

  it("stays OFF even under the 'everything' profile (the defaultOff contract)", () => {
    expect(resolveEnabled('everything').js8).toBe(false)
  })
})
