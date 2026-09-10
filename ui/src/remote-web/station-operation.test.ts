import { describe, expect, it } from 'vitest'
import { controlContext, controlOutcome, stationAction } from './station-operation'

describe('closed station operating requests', () => {
  it('admits explicit receiver and amplifier gestures without accepting a generic invoke', () => {
    for (const receiver of ['rtty', 'psk', 'sstv', 'aprs']) {
      const action = { action: 'decoder.arm', receiver, on: true }
      expect(stationAction(action)).toEqual(action)
    }
    const amp = { action: 'amplifier.operate', expectedOperate: false, operate: true }
    expect(stationAction(amp)).toEqual(amp)
    for (const action of [
      { action: 'invoke', command: 'set_ptt', args: { on: true } },
      { action: 'radio.disarm', on: true },
      { ...amp, expectedOperate: true },
      { ...amp, command: 'powerOff' },
      { action: 'amplifier.band', expectedBand: '40m', direction: '1' }
    ]) expect(() => stationAction(action)).toThrow('invalidOperation')
  })
  it('rejects nonfinite inputs and unknown radios, modes and band names', () => {
    const good = { action: 'radio.frequency', dialMhz: 14.074, band: '20m', sideband: 'USB' }
    expect(stationAction(good)).toEqual(good)
    for (const dialMhz of [NaN, Infinity, -1, 0, 250001]) expect(() => stationAction({ ...good, dialMhz })).toThrow()
    expect(() => stationAction({ ...good, sideband: 'garbled' })).toThrow()
    expect(() => stationAction({ action: 'radio.select', radioId: -1 })).toThrow()
    expect(() => stationAction({ action: 'radio.tier', tier: 'futureMode' })).toThrow()
    expect(() => stationAction({ action: 'decoder.arm', receiver: 'ft8', on: true })).toThrow()
  })
  it('keeps hardware binding and completion evidence explicit', () => {
    const context = { radioId: 3, radioConnection: 2, ampConnection: 5, ampReadSequence: 10 }
    expect(controlContext(context)).toEqual(context)
    expect(() => controlContext({ ...context, ampConnection: null })).toThrow()
    const base = { operation: 'stationControl', operationId: crypto.randomUUID() }
    for (const result of [
      { ...base, outcome: 'pending' },
      { ...base, outcome: 'applied', evidence: 'amplifierReadback' },
      { ...base, outcome: 'unknown', reason: 'hardwareUnconfirmed' }
    ]) expect(controlOutcome(result)).toEqual(result)
    expect(() => controlOutcome({ ...base, outcome: 'applied', evidence: 'queueAccepted' })).toThrow()
    expect(() => controlOutcome({ ...base, outcome: 'applied', evidence: 'rfOff' })).toThrow()
  })
})
