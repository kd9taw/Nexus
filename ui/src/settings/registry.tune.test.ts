// The Tune timeout was unfindable. It lives in Settings ▸ Digital ▸ "Transmit & Sequencing" —
// a sub-group of the `digital-ft8-ft4` section — and the word "tune" appeared nowhere in the
// registry, so settings search returned nothing for it and no deep link could name it. The
// control that auto-releases a key-down carrier is not one to make an operator hunt for.
//
// A SECTION is the registry's unit, and `digital-ft8-ft4` was already declared; what was
// missing is the vocabulary, which is exactly what `keywords` exists for ("the searchable
// vocabulary lives in hint text, not labels"). No new section — the Radio tab is at 10 of a
// hard ceiling of 12 and this belongs on Digital regardless.
//
// POSITIVE CONTROL below: the same search for a word that was already there must hit, so a
// green run cannot mean the search itself is broken.
import { describe, it, expect } from 'vitest'
import { searchSettings } from './registry'

describe('the Tune timeout can be found', () => {
  it('searching "tune" reaches the section it lives in', () => {
    const hits = searchSettings('tune')
    expect(hits.map((h) => h.section.id)).toContain('digital-ft8-ft4')
  })

  it('and so does "tune timeout", the way an operator would type it', () => {
    expect(searchSettings('tune timeout').map((h) => h.section.id)).toContain('digital-ft8-ft4')
  })

  it('positive control — a keyword that was already registered still hits', () => {
    expect(searchSettings('watchdog').map((h) => h.section.id)).toContain('digital-ft8-ft4')
  })

  it('negative control — a word in no section returns nothing', () => {
    expect(searchSettings('zzzznotasetting')).toEqual([])
  })
})
