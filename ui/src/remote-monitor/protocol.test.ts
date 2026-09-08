import { describe, expect, it } from 'vitest'
import fixtures from './fixtures.v2.json'
import { ageFrame, FrameOrder, MAX_FRAME_BYTES, parseFrame } from './protocol'

describe('Rust v2 observer boundary', () => {
  it('preserves the integer wire schema across fractional browser clock readings', () => {
    const original = parseFrame(structuredClone(fixtures.spe), 'fixture')
    const aged = ageFrame(original, 0.125)
    expect(() => parseFrame(aged, 'fixture')).not.toThrow()
    expect(aged.station.radio.readings.dial!.ageMs).toBe(original.station.radio.readings.dial!.ageMs + 1)
  })
  it('requires attributable readings and rejects mixed radio connections', () => {
    const missing = structuredClone(fixtures.spe)
    missing.station.radio.readings.cat = null as never
    expect(() => parseFrame(missing, 'fixture')).toThrow()
    const mixed = structuredClone(fixtures.spe)
    mixed.station.radio.readings.ptt!.connectionGeneration++
    expect(() => parseFrame(mixed, 'fixture')).toThrow()
    const legacyAmp = structuredClone(fixtures.spe)
    legacyAmp.station.amplifier!.reading = null as never
    expect(() => parseFrame(legacyAmp, 'fixture')).toThrow()
    const negative = structuredClone(fixtures.spe)
    negative.station.radio.readings.cat!.ageMs = -1
    expect(() => parseFrame(negative, 'fixture')).toThrow()
  })
  it('expires measurements without changing the publication or refreshing other fields', () => {
    const original = parseFrame(structuredClone(fixtures.spe), 'fixture')
    original.station.radio.readings.ptt!.ageMs = 4900
    original.station.amplifier!.reading!.ageMs = 4700
    const first = ageFrame(original, 200)
    expect(first.station.radio.rigKeyed).toBeNull()
    expect(first.station.radio.catConnected).toBe(true)
    expect(first.station.amplifier!.outputWatts).toBe(0)
    const later = ageFrame(original, 400)
    expect(later.station.amplifier!.outputWatts).toBeNull()
    expect(later.station.amplifier!.operate).toBeNull()
    expect(later.station.amplifier!.reading).toBeNull()
    expect(later.sequence).toBe(original.sequence)
    expect(later.generatedAtMs).toBe(original.generatedAtMs)
    expect(original.station.radio.rigKeyed).toBe(false)
  })
  it('consumes every Rust-serialized fixture, retaining missing readings and family semantics', () => {
    expect(Object.keys(fixtures)).toHaveLength(9)
    for (const value of Object.values(fixtures)) {
      expect(parseFrame(value, 'fixture')).toEqual(value)
      expect(new TextEncoder().encode(JSON.stringify(value)).length).toBeLessThan(MAX_FRAME_BYTES)
    }
    expect(fixtures.firstMiss.station.amplifier).toMatchObject({ linked: true, outputWatts: null, operate: null, reason: 'noAnswer' })
    expect(fixtures.kpa.station.amplifier).toMatchObject({ tempCelsius: true, transmitting: null })
    expect(fixtures.spe.station.amplifier).toMatchObject({ tempCelsius: false })
    expect(fixtures.noAmp.station.amplifier).toBeNull()
  })
  it('rejects versions, wrong sources, missing/extra fields and unbounded values', () => {
    for (const patch of [{ version: 1 }, { source: 'native' }, { sequence: 0 },
      { sequence: Number.MAX_SAFE_INTEGER + 1 }, { epoch: 'x'.repeat(65) }, { settings: {} }]) {
      expect(() => parseFrame({ ...fixtures.spe, ...patch }, 'fixture')).toThrow()
    }
    const bad = structuredClone(fixtures.spe)
    bad.station.radio.dialMhz = Infinity
    expect(() => parseFrame(bad, 'fixture')).toThrow()
    expect(() => parseFrame({ ...fixtures.spe, station: { call: 'N0CALL' } }, 'fixture')).toThrow()
  })
  it('never regresses within an epoch or resurrects an old process after restart', () => {
    const order = new FrameOrder()
    const a = parseFrame(fixtures.spe, 'fixture')
    expect(order.accept(a)).toBe(true)
    expect(order.accept(a)).toBe(false)
    expect(order.accept({ ...a, sequence: 4 })).toBe(true)
    expect(order.accept({ ...a, sequence: 3 })).toBe(false)
    expect(order.accept({ ...a, epoch: 'new-process' })).toBe(true)
    expect(order.accept({ ...a, sequence: 99 })).toBe(false)
  })
})
