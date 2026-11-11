// A typed IOTA reference in the log's one spelling: the obvious shapes an operator types become
// `EU-005`; anything else is no reference (a form then says so and does not log it as nothing).
import { describe, expect, it } from 'vitest'
import { normalizeIota } from './iotaGroups'

describe('normalizeIota', () => {
  it('takes the obvious spellings to the one the log holds', () => {
    for (const [typed, want] of [
      ['eu-5', 'EU-005'],
      ['EU 5', 'EU-005'],
      ['EU-05', 'EU-005'],
      ['eu005', 'EU-005'],
      [' na-131 ', 'NA-131'],
      ['OC-146', 'OC-146'],
    ]) {
      expect(normalizeIota(typed), typed).toBe(want)
    }
  })

  it('refuses anything else', () => {
    for (const junk of ['', 'EU', 'EU-1234', 'XX-001', 'E-5', 'EU--5', '5', 'EU-5A', 'IOTA EU-5']) {
      expect(normalizeIota(junk), junk).toBeNull()
    }
  })
})
