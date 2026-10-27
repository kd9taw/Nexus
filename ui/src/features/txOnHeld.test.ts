// The status lane after a TX On the sign-in hold refused: it says why the latch did not light, in
// the words for where the operator is, for as long as the hold stands, and nothing otherwise.
import { describe, expect, it } from 'vitest'
import { heldLine, txOnHeldLane } from './txOnHeld'
import { EN } from '../i18n/en'

describe('the status lane after a TX On the sign-in hold refused', () => {
  it('says nothing while no TX On is held', () => {
    expect(txOnHeldLane(false, false)).toBeNull()
    expect(txOnHeldLane(undefined, false)).toBeNull()
    expect(txOnHeldLane(false, true)).toBeNull()
  })

  it('at the shack, says a newly signed-in device waits for approval, and where', () => {
    expect(txOnHeldLane(true, false)).toEqual({
      tier: 'warning',
      message: EN['shell.lane.txOnHeld.message'],
      detail: EN['shell.lane.txOnHeld.detail'],
    })
  })

  it('on a computer driving the station, says this device must be approved there', () => {
    expect(txOnHeldLane(true, true)).toEqual({
      tier: 'warning',
      message: EN['shell.lane.txOnHeld.message'],
      detail: EN['client.ptt.refusal.onHold'],
    })
  })
})

describe('the time left on the hold, on a computer driving the station', () => {
  const fill = (text: string, minutes: string) => text.replace('{{minutes}}', minutes)

  it('adds "or about N min" only when the station sent the seconds left', () => {
    expect(txOnHeldLane(true, true, { ownerNotified: true, leftSecs: 125 })?.detail).toBe(
      fill(EN['client.ptt.refusal.onHoldLeft'], '3'),
    )
    expect(txOnHeldLane(true, true, { ownerNotified: false })?.detail).toBe(EN['client.ptt.refusal.onHold'])
    expect(txOnHeldLane(true, true, null)?.detail).toBe(EN['client.ptt.refusal.onHold'])
  })

  it('never at the shack, whose line names Allow to transmit', () => {
    expect(txOnHeldLane(true, false, { ownerNotified: true, leftSecs: 125 })?.detail).toBe(
      EN['shell.lane.txOnHeld.detail'],
    )
    expect(EN['shell.lane.txOnHeld.detail']).toContain(EN['remote.account.allowTransmit'])
  })

  it('a last few seconds still read as a minute, never as none', () => {
    expect(heldLine({ ownerNotified: true, leftSecs: 1 })).toBe(fill(EN['client.ptt.refusal.onHoldLeft'], '1'))
  })
})
