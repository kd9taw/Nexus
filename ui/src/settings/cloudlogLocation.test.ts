// A Wavelog station location whose callsign is not the operator's refuses every QSO Nexus sends it
// (HTTP 400, "Differing station callsign … SKIPPED"), and one whose grid is elsewhere files every
// QSO there. Settings warns at the pick; these are the cases that decide whether it does.
import { describe, it, expect } from 'vitest'
import { cloudlogLocationMismatch } from './cloudlogLocation'

const loc = (callsign: string, gridsquare: string) => ({ callsign, gridsquare })

describe('cloudlogLocationMismatch', () => {
  it('flags a location whose callsign is not the one Nexus logs as', () => {
    // The reported shape: a location called PRACTICE, in the next grid square over.
    expect(cloudlogLocationMismatch(loc('PRACTICE', 'DM42'), 'N0CALL', 'DM41')).toEqual({
      callsign: true,
      grid: true,
    })
  })

  it('passes a location that is the operator, whatever the case or spacing', () => {
    // The control: the same comparison must also be able to say "no".
    expect(cloudlogLocationMismatch(loc(' n0call ', 'dm41ab'), 'N0CALL', 'DM41')).toEqual({
      callsign: false,
      grid: false,
    })
  })

  it('compares grids on their first four characters only', () => {
    expect(cloudlogLocationMismatch(loc('N0CALL', 'DM41XY'), 'N0CALL', 'DM41AB').grid).toBe(false)
    expect(cloudlogLocationMismatch(loc('N0CALL', 'DM42AB'), 'N0CALL', 'DM41AB').grid).toBe(true)
  })

  it('a portable or club callsign is a different callsign', () => {
    expect(cloudlogLocationMismatch(loc('N0CALL/P', 'DM41'), 'N0CALL', 'DM41').callsign).toBe(true)
  })

  it('a location listing several grids matches when any of them does', () => {
    // A rover or grid-line location carries a comma-separated list, and Wavelog accepts a QSO
    // from any grid in it.
    expect(cloudlogLocationMismatch(loc('N0CALL', 'DM41,DM42'), 'N0CALL', 'DM42').grid).toBe(false)
    expect(cloudlogLocationMismatch(loc('N0CALL', 'DM41,DM42'), 'N0CALL', 'DM43').grid).toBe(true)
  })

  it('says nothing about a side it cannot read', () => {
    // No callsign or grid set in Nexus, or none on the location: there is nothing to compare,
    // and a warning naming an empty value would only confuse.
    expect(cloudlogLocationMismatch(loc('PRACTICE', 'DM42'), '', '')).toEqual({
      callsign: false,
      grid: false,
    })
    expect(cloudlogLocationMismatch(loc('', 'DM'), 'N0CALL', 'DM41')).toEqual({
      callsign: false,
      grid: false,
    })
  })
})
