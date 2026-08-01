// @vitest-environment node
//
// The ONE reading of "is this bird still worth chasing", shared by every
// surface that names a bird. What is pinned here is the honesty line: the
// helper reports what the catalog SAID, and says nothing at all when the
// catalog never answered — a silent bird and an unknown bird are different
// facts, and guessing either one is how a re-entered bird kept its ★ with no
// mark anywhere.
import { describe, it, expect } from 'vitest'
import { satBirdHealth, satExcludedHealth } from './satHealth'

describe('satBirdHealth (a bird that HAS a row — elements exist)', () => {
  it('an alive bird with a live amateur transmitter is unmarked', () => {
    expect(satBirdHealth('alive', true)).toBeNull()
  })

  it('says NOTHING when the catalog never answered — never a guessed "alive"', () => {
    // Schema-1 manifests and the Celestrak fallback leg carry no catalog, so
    // status is absent for every bird. A chip there would be an invention.
    expect(satBirdHealth(null, undefined)).toBeNull()
    expect(satBirdHealth(undefined, false)).toBeNull()
  })

  it('marks dead and re-entered with the upstream word', () => {
    expect(satBirdHealth('dead', true)?.label).toBe('dead')
    expect(satBirdHealth('dead', true)?.tone).toBe('dead')
    expect(satBirdHealth('re-entered', true)?.label).toBe('re-entered')
    expect(satBirdHealth('re-entered', true)?.tone).toBe('dead')
  })

  it('marks an ALIVE bird whose amateur transmitters have all gone quiet', () => {
    // The catalog's `amateur` flag IS "carries ≥1 live amateur transmitter".
    // Still in orbit, nothing to work — the operator must be told.
    const h = satBirdHealth('alive', false)
    expect(h?.label).toBe('silent')
    expect(h?.tone).toBe('dead')
    expect(h?.title).toMatch(/transmitter/i)
  })

  it('never claims "silent" when the amateur flag is simply not on the wire', () => {
    // Schedule rows (SatPass) carry status but no amateur flag — an
    // `undefined` there means "not asked", not "no transmitters".
    expect(satBirdHealth('alive', undefined)).toBeNull()
  })

  it('degrades an unseen upstream status to a label instead of dropping it', () => {
    const h = satBirdHealth('partially-operational', true)
    expect(h?.label).toBe('partially-operational')
    expect(h?.tone).toBe('stale')
  })

  it('flags a pre-launch bird as not yet workable', () => {
    expect(satBirdHealth('future', true)?.label).toBe('pre-launch')
  })
})

describe('satExcludedHealth (a starred bird the view could not place)', () => {
  it('names each reason distinctly — never one blanket "unavailable"', () => {
    expect(satExcludedHealth('noElements').label).toBe('no elements')
    expect(satExcludedHealth('staleElements').label).toBe('stale elements')
    expect(satExcludedHealth('noPosition').label).toBe('no position')
  })

  it('every reason carries an explanation and the stale tone', () => {
    for (const r of ['noElements', 'staleElements', 'noPosition'] as const) {
      const h = satExcludedHealth(r)
      expect(h.tone).toBe('stale')
      expect(h.title.length).toBeGreaterThan(20)
    }
  })
})
