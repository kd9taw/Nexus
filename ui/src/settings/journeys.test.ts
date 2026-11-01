// The first-run journeys' two rules, by value: which answer shows where contacts go, and which
// answer hands the rig step on to the radio setup.
import { describe, it, expect } from 'vitest'
import { handsOffToRadioSetup, RADIO_COUNTS, showsServices } from './journeys'

describe('the first-run journeys', () => {
  it('only an operator switching programs (Experienced) is shown where contacts go', () => {
    expect([null, undefined, 'newToHobby', 'newToDigital', 'experienced'].map((l) => showsServices(l as never))).toEqual([
      false,
      false,
      false,
      false,
      true,
    ])
  })

  it('only "Two or more" hands the rig step on to the radio setup; unanswered is today\'s wizard', () => {
    expect([null, 'one', 'several'].map((r) => handsOffToRadioSetup(r as never))).toEqual([false, false, true])
  })

  it('offers one radio, then two or more, each with words', () => {
    expect(RADIO_COUNTS.map((r) => r.value)).toEqual(['one', 'several'])
    for (const r of RADIO_COUNTS) {
      expect(r.label.length).toBeGreaterThan(0)
      expect(r.blurb.length).toBeGreaterThan(r.label.length)
    }
  })
})
