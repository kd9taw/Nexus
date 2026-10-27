// The shared cty.dat continent vocabulary (#174), one list for the Spots chips, Band Activity's
// hide-by-continent and the alert area filter's picker.
import { describe, it, expect } from 'vitest'
import { CONTINENT_CODES } from './dxccGeo'

describe('the continent vocabulary', () => {
  it('the six cty.dat codes, in picker order', () => {
    expect(CONTINENT_CODES).toEqual(['NA', 'SA', 'EU', 'AF', 'AS', 'OC'])
  })
})
