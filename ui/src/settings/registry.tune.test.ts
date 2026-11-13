// The Tune timeout was unfindable. It lived in Settings ▸ Digital ▸ "Transmit & Sequencing" and
// the word "tune" appeared nowhere in the registry, so settings search returned nothing for it
// and no deep link could name it. The control that auto-releases a key-down carrier is not one
// to make an operator hunt for.
//
// #487 then MOVED it to Settings ▸ Radio ▸ Transmit limits & sharing (`transmit-limits`), beside
// the high-SWR cutoff: it applies to Tune on every screen, not only FT8. Its words moved with it
// — a keyword that lands the operator on a section no longer holding the control is worse than
// none. No new section: `transmit-limits` was already declared.
//
// POSITIVE CONTROL below: the same search for a word that was already there must hit, so a
// green run cannot mean the search itself is broken.
import { describe, it, expect } from 'vitest'
import { resolveTarget, searchSettings } from './registry'

describe('the Tune timeout can be found', () => {
  it('searching "tune" reaches the section it lives in', () => {
    const hits = searchSettings('tune')
    expect(hits.map((h) => h.section.id)).toContain('transmit-limits')
  })

  it('and "tune timeout", the way an operator would type it, lands there FIRST', () => {
    expect(searchSettings('tune timeout')[0]?.section.id).toBe('transmit-limits')
  })

  it('a deep link by the control\'s own name lands on the Radio tab, in that section', () => {
    expect(resolveTarget('Settings ▸ Radio ▸ Transmit limits & sharing')).toEqual({ tab: 'radio', section: 'transmit-limits' })
  })

  // #487: the reporter looked for it as the TUNER's time limit, and "tuner" is not a substring of
  // "tune", so the search found nothing. The operator's own words must land on it.
  it.each(['tuner', 'tuner timeout', 'tuner time limit', 'atu time'])('and so does %j (#487)', (q) => {
    expect(searchSettings(q)[0]?.section.id).toBe('transmit-limits')
  })

  it('positive control — a keyword that was already registered still hits', () => {
    expect(searchSettings('watchdog').map((h) => h.section.id)).toContain('digital-ft8-ft4')
  })

  it('negative control — a word in no section returns nothing', () => {
    expect(searchSettings('zzzznotasetting')).toEqual([])
  })
})
