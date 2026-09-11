import { describe, expect, it } from 'vitest'
import { actionCapability, controlContext, controlOutcome, stationAction } from './station-operation'

describe('closed station operating requests', () => {
  it('accepts only exact, changed decoder choices and requires their own capability', () => {
    const choices = [
      { action: 'decoder.js8Speed', expectedSpeed: 1, speed: 3 },
      { action: 'decoder.msk144Period', expectedPeriodSecs: 15, periodSecs: 5 }
    ]
    for (const choice of choices) {
      expect(stationAction(choice)).toEqual(choice)
      expect(actionCapability(stationAction(choice))).toBe('decoderSettings')
      for (const extra of ['settings', 'command', 'txEnabled', 'radioId']) expect(() => stationAction({ ...choice, [extra]: true })).toThrow()
    }
    for (const speed of [-1, 1, 4, 2.5, '3', null, NaN, Infinity]) expect(() => stationAction({ ...choices[0], speed })).toThrow()
    for (const expectedSpeed of [-1, 3, 4, '1', null]) expect(() => stationAction({ ...choices[0], expectedSpeed })).toThrow()
    for (const periodSecs of [0, 14, 15, 60, '5', null, NaN]) expect(() => stationAction({ ...choices[1], periodSecs })).toThrow()
    for (const expectedPeriodSecs of [0, 5, 14, '15', null]) expect(() => stationAction({ ...choices[1], expectedPeriodSecs })).toThrow()
  })
  it('admits complete workspace intents without accepting embedded commands, settings or transmit choices', () => {
    for (const workspace of ['ft', 'tempo', 'js8']) {
      const action = stationAction({ action: 'radio.workspace', workspace })
      expect(action).toEqual({ action: 'radio.workspace', workspace })
      expect(actionCapability(action)).toBe('workspace')
    }
    for (const action of [
      { action: 'radio.workspace', workspace: 'FT' }, { action: 'radio.workspace', workspace: 'cw' },
      { action: 'radio.workspace', workspace: 'js8', txEnabled: true },
      { action: 'radio.workspace', workspace: 'ft', tier: 'WSPR' },
      { action: 'radio.workspace', workspace: 'tempo', settings: {} },
      { action: 'radio.workspace', workspace: 'js8', command: 'js8_enter' }
    ]) expect(() => stationAction(action)).toThrow('invalidOperation')
  })
  it('accepts only a changed follow choice with the displayed profile and exact Settings revision', () => {
    const intent = { action: 'amplifier.followBand', radioId: 3, expectedSettingsRevision: 'a'.repeat(64), expectedFollow: false, follow: true }
    expect(stationAction(intent)).toEqual(intent)
    for (const bad of [
      { ...intent, radioId: -1 }, { ...intent, radioId: '3' }, { ...intent, expectedSettingsRevision: 'A'.repeat(64) },
      { ...intent, expectedSettingsRevision: 'a'.repeat(63) }, { ...intent, follow: false },
      { ...intent, follow: 1 }, { ...intent, settingsPath: '/untrusted' }, { ...intent, settings: {} }
    ]) expect(() => stationAction(bad)).toThrow('invalidOperation')
    const base = { operation: 'stationControl', operationId: crypto.randomUUID() }
    expect(controlOutcome({ ...base, outcome: 'applied', evidence: 'settingsSaved' })).toMatchObject({ evidence: 'settingsSaved' })
    expect(controlOutcome({ ...base, outcome: 'rejected', reason: 'persistenceFailed' })).toMatchObject({ reason: 'persistenceFailed' })
  })
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
