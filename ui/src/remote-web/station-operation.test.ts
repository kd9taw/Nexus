import { describe, expect, it } from 'vitest'
import { actionCapability, controlContext, controlOutcome, stationAction } from './station-operation'

describe('closed station operating requests', () => {
  it('admits only receiver function names, boolean targets and native AGC choices', () => {
    for (const mode of ['cw', 'phone']) {
      for (const func of ['nb', 'nr', 'notch', 'manualNotch']) {
        const action = { action: 'radio.function', mode, func, expectedOn: false, on: true }
        expect(stationAction(action)).toEqual(action); expect(actionCapability(stationAction(action))).toBe('receiverDsp')
        for (const func of ['vox', 'comp', 'tuner', 'NB', 'nb\nT 1', '', null]) expect(() => stationAction({ ...action, func })).toThrow()
        for (const value of [0, 1, 'true', null]) for (const field of ['expectedOn', 'on']) expect(() => stationAction({ ...action, [field]: value })).toThrow()
        for (const extra of ['dialMhz', 'sideband', 'settings', 'command', 'tx', 'radioId']) expect(() => stationAction({ ...action, [extra]: true })).toThrow()
        for (const mode of ['digital', 'CW', 'ssb', null]) expect(() => stationAction({ ...action, mode })).toThrow()
      }
      for (const speed of ['auto', 'fast', 'mid', 'slow', 'off']) {
        const action = { action: 'radio.agc', mode, expectedSpeed: 'fast', speed }
        expect(stationAction(action)).toEqual(action); expect(actionCapability(stationAction(action))).toBe('receiverDsp')
        for (const value of ['FAST', 'user', 2, 6, null, 'slow\nT 1']) for (const field of ['expectedSpeed', 'speed']) expect(() => stationAction({ ...action, [field]: value })).toThrow()
        for (const extra of ['settings', 'command', 'txEnabled', 'radioId']) expect(() => stationAction({ ...action, [extra]: true })).toThrow()
      }
    }
  })
  it('binds native filter width to the displayed cockpit and prior width under its own capability', () => {
    for (const [mode, expectedHz, hz] of [['cw', 500, 550], ['phone', 2400, 2300]] as const) {
      const a = { action: 'radio.filterWidth', mode, expectedHz, hz }
      expect(stationAction(a)).toEqual(a)
      expect(actionCapability(stationAction(a))).toBe('receiverFilter')
      for (const extra of ['dialMhz', 'sideband', 'settings', 'command', 'tx', 'radioId']) expect(() => stationAction({ ...a, [extra]: true })).toThrow()
      for (const expectedHz of [0, -1, 0x100000000, 500.5, NaN, Infinity, '500', null]) expect(() => stationAction({ ...a, expectedHz })).toThrow()
      for (const hz of [0, 49, 4001, NaN, Infinity, 550.5, '550', null]) expect(() => stationAction({ ...a, hz })).toThrow()
      for (const mode of ['digital', 'CW', 'ssb', null]) expect(() => stationAction({ ...a, mode })).toThrow()
    }
    expect(() => stationAction({ action: 'radio.filterWidth', mode: 'cw', expectedHz: 500, hz: 2001 })).toThrow()
    expect(() => stationAction({ action: 'radio.filterWidth', mode: 'phone', expectedHz: 2400, hz: 299 })).toThrow()
    // A native value outside the UI range can be narrowed back into it; the
    // expected readback is not itself a request to write an invalid width.
    expect(stationAction({ action: 'radio.filterWidth', mode: 'phone', expectedHz: 4500, hz: 4000 })).toMatchObject({ expectedHz: 4500 })
  })
  it('names a band and its cockpit mode without accepting a browser frequency policy', () => {
    for (const mode of ['cw', 'phone']) {
      const a = { action: 'radio.band', band: '40m', mode }
      expect(actionCapability(stationAction(a))).toBe('bandSelection')
      for (const extra of ['dialMhz', 'sideband', 'settings', 'command', 'radioId']) expect(() => stationAction({ ...a, [extra]: 1 })).toThrow()
      for (const band of ['', null, '40m-call', '20m;T 1']) expect(() => stationAction({ ...a, band })).toThrow()
    }
    for (const mode of ['digital', 'CW', null, ['cw']]) expect(() => stationAction({ action: 'radio.band', band: '40m', mode })).toThrow()
  })

  it('accepts a bandless receive dial through the existing frequency intent', () => {
    const action = { action: 'radio.frequency', dialMhz: 10, band: '', sideband: 'USB' }
    expect(stationAction(action)).toEqual(action)
    expect(actionCapability(stationAction(action))).toBe('frequency')
    for (const band of [null, 0, 'WWV', ' ', '20m;T 1']) expect(() => stationAction({ ...action, band })).toThrow()
    for (const extra of ['tx', 'radioId', 'command', 'settings']) expect(() => stationAction({ ...action, [extra]: true })).toThrow()
  })
  it('binds gain to the saved active radio and prior value under its own capability', () => {
    const choice = { action: 'receiver.rxGain', radioId: 3, expectedSettingsRevision: 'a'.repeat(64), expectedGain: 1, gain: 2.5 }
    expect(stationAction(choice)).toEqual(choice)
    expect(actionCapability(stationAction(choice))).toBe('receiverGain')
    for (const extra of ['settings', 'command', 'txLevel', 'path', 'expectedTier']) expect(() => stationAction({ ...choice, [extra]: true })).toThrow()
    for (const gain of [NaN, Infinity, -Infinity, 0.9, 8.1, 1, '2', null]) expect(() => stationAction({ ...choice, gain })).toThrow()
    for (const expectedGain of [NaN, Infinity, '1', null, 2.5]) expect(() => stationAction({ ...choice, expectedGain })).toThrow()
    for (const radioId of [-1, 0x100000000, '3', 2.5]) expect(() => stationAction({ ...choice, radioId })).toThrow()
    for (const expectedSettingsRevision of ['A'.repeat(64), 'a'.repeat(63), '', null]) expect(() => stationAction({ ...choice, expectedSettingsRevision })).toThrow()
    expect(stationAction({ ...choice, expectedGain: 0.5 })).toMatchObject({ expectedGain: 0.5 })
  })
  it('binds receive choices to the displayed tier and prior value without accepting TX or form fields', () => {
    const depth = { action: 'decoder.depth', expectedTier: 'FT8', expectedDepth: 3, depth: 1 }
    const rx = { action: 'receiver.rxOffset', expectedTier: 'JS8', expectedHz: 1500, hz: 725.25 }
    for (const choice of [depth, rx]) {
      expect(stationAction(choice)).toEqual(choice)
      expect(actionCapability(stationAction(choice))).toBe('receiverSettings')
      for (const extra of ['settings', 'command', 'txOffsetHz', 'radioId', 'target']) expect(() => stationAction({ ...choice, [extra]: true })).toThrow()
      for (const expectedTier of [null, 'unknown', 'cw', 1]) expect(() => stationAction({ ...choice, expectedTier })).toThrow()
    }
    for (const value of [0, 3, 4, 2.5, '2', null, NaN]) expect(() => stationAction({ ...depth, depth: value })).toThrow()
    for (const value of [0, 1, 4, 255, '3', NaN]) expect(() => stationAction({ ...depth, expectedDepth: value })).toThrow()
    for (const value of [NaN, Infinity, -Infinity, 199, 4001, 1500, '725', null]) expect(() => stationAction({ ...rx, hz: value })).toThrow()
    for (const value of [NaN, Infinity, '1500', null, 725.25]) expect(() => stationAction({ ...rx, expectedHz: value })).toThrow()
    expect(stationAction({ ...depth, expectedDepth: 2 })).toMatchObject({ expectedDepth: 2 })
    expect(stationAction({ ...rx, expectedHz: 50 })).toMatchObject({ expectedHz: 50 })
  })
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
