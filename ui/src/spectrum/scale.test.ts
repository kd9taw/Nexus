import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { SpectrumFrameWire, SpectrumScale } from '../types'
import { aggregateRow, strengthIn } from './aggregate'
import {
  axisOf,
  axisTicks,
  axisUnit,
  dbAtY,
  dbToValue,
  refLabel,
  rendererFrame,
  valueToDb,
  type ScaleAxis,
} from './scale'
import { LogRecursiveAverager } from './scaleAverage'
import { autoRange, displayRange } from './scaleRange'
import type { DisplayRange } from './types'

/** The backend's own wire goldens for a frame (dto.rs, the frame serialisation test), read from the Rust
 *  source so a change to the wire on that side reaches this file. */
function rustGoldens(): SpectrumFrameWire[] {
  const dto = readFileSync(fileURLToPath(new URL('../../../crates/tempo-app/src/dto.rs', import.meta.url)), 'utf8')
  return [...dto.matchAll(/r#"(\{"seq":[^#]*\})"#/g)].map((m) => JSON.parse(m[1]) as SpectrumFrameWire)
}

/** tempo_core::spectrum `power_to_display`: a power ratio against full scale, onto the audio feed's
 *  0..1. The producer's half, pinned in Rust by `the_absolute_db_axis_reads_the_same_at_every_window_length`
 *  (a tone of amplitude 0.1 reads −20 dBFS at every window length). */
function powerToDisplay(p: number): number {
  const db = 10 * Math.log10(Math.max(p, 1e-30))
  return Math.min(1, Math.max(0, (db + 120) / 120))
}

/** mulberry32: the same noise on every run. */
function prng(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** A 512-bin audio row, 0–4000 Hz: noise around `noiseDb` dBFS and a sine of amplitude `amp` in bin 192
 *  (1500 Hz), each encoded as the producer encodes a power. */
function audioRow(noiseDb: number, amp: number, seed: number): number[] {
  const r = prng(seed)
  const row = Array.from({ length: 512 }, () => powerToDisplay(10 ** ((noiseDb + 5 * (r() + r() + r() - 1.5)) / 10)))
  row[192] = powerToDisplay(amp * amp)
  return row
}

describe('the axis a frame is on', () => {
  it("reads the backend's own frames: both wire goldens, taken from dto.rs", () => {
    const goldens = rustGoldens()
    // Parser sanity: a pattern that found nothing would let every assertion below pass on nothing.
    expect(goldens.map((g) => g.source).sort()).toEqual(['audio', 'civ'])
    const audio = goldens.find((g) => g.source === 'audio')!
    const civ = goldens.find((g) => g.source === 'civ')!
    expect(axisOf(audio.scale, audio.source)).toEqual({ kind: 'dbfs', topDb: 0, dbPerUnit: 120, graduated: true })
    expect(axisOf(civ.scale, civ.source)).toEqual({ kind: 'relative', topDb: 0, dbPerUnit: 80, graduated: true })
    expect(rendererFrame(audio)).toEqual({ seq: 8, tMs: 1700000000143, loHz: 0, hiHz: 4000, bins: [0.25], dbPerUnit: 120 })
    expect(rendererFrame(civ).dbPerUnit).toBe(80)
    expect(axisUnit(axisOf(audio.scale, audio.source))).toBe('dBFS')
    expect(axisUnit(axisOf(civ.scale, civ.source))).toBe('dB rel')
  })

  it('never claims an axis it cannot back: no dBm, and nothing absolute from a scale it does not understand', () => {
    const dbfs: SpectrumScale = { kind: 'dbfs', loDb: -120, hiDb: 0 }
    const broken: SpectrumScale[] = [
      { kind: 'dbfs', loDb: 0, hiDb: 0 },
      { kind: 'dbfs', loDb: -120, hiDb: Number.NaN },
      { kind: 'dbfs', loDb: 0, hiDb: -120 },
      // A kind a newer backend might send (a calibrated source): this build reads it as relative.
      { kind: 'dbm', loDbm: -140, hiDbm: -40 } as unknown as SpectrumScale,
    ]
    for (const source of ['audio', 'civ', 'flex', 'yaesu', 'kiwi']) {
      for (const scale of [dbfs, { kind: 'relative' } as SpectrumScale, null, undefined, ...broken]) {
        const axis = axisOf(scale, source)
        expect(axisUnit(axis), `${source} ${JSON.stringify(scale)}`).not.toMatch(/dBm/)
        expect(axis.kind === 'dbfs', `${source} ${JSON.stringify(scale)}`).toBe(scale === dbfs)
      }
    }
    // The radios with no published dB per unit print no numbers, whatever the range.
    for (const source of ['flex', 'yaesu', 'kiwi']) {
      const axis = axisOf({ kind: 'relative' }, source)
      expect(axis).toEqual({ kind: 'relative', topDb: 0, dbPerUnit: 120, graduated: false })
      expect(axisUnit(axis)).toBe('rel')
      expect(axisTicks(axis, { floor: 0.2, ceil: 0.8 }, 200)).toEqual([])
    }
    // Control: the same range on the one graduated radio does get numbers, so the empty lists above are
    // the rule talking and not a tick function that returns nothing.
    expect(axisTicks(axisOf({ kind: 'relative' }, 'civ'), { floor: 0.2, ceil: 0.8 }, 200).length).toBeGreaterThan(3)
  })
})

describe('one conversion between the scale and the renderer’s dB per unit, both ways', () => {
  const axes: [string, ScaleAxis][] = [
    ['audio', axisOf({ kind: 'dbfs', loDb: -120, hiDb: 0 }, 'audio')],
    ['civ', axisOf({ kind: 'relative' }, 'civ')],
    ['an offset dBFS scale', axisOf({ kind: 'dbfs', loDb: -150, hiDb: -10 }, 'audio')],
  ]

  it('value → dB → value and dB → value → dB are the identity', () => {
    for (const [name, axis] of axes) {
      for (const v of [0, 0.125, 0.5, 0.8333, 1]) expect(dbToValue(axis, valueToDb(axis, v)), name).toBeCloseTo(v, 12)
      for (const db of [-130, -95, -20, 0]) expect(valueToDb(axis, dbToValue(axis, db)), name).toBeCloseTo(db, 9)
    }
    // The fixed points that make it an axis: full scale and the floor of the audio feed.
    expect(valueToDb(axes[0][1], 1)).toBe(0)
    expect(valueToDb(axes[0][1], 0)).toBe(-120)
    expect(valueToDb(axes[2][1], 0)).toBe(-150)
  })

  it("the renderer's average detector, fed the frame's dbPerUnit, is the power mean in the axis's dB", () => {
    // The renderer averages POWER through `dbPerUnit`; the axis turns values into dB. If the two
    // disagreed, the average detector would draw a level the axis labels wrongly.
    for (const [name, axis] of axes) {
      const values = [dbToValue(axis, -30), dbToValue(axis, -40), dbToValue(axis, -60), dbToValue(axis, -33)]
      const out = new Float32Array(1)
      aggregateRow(values, 0, 4, 0, 4, 0, 4, out, 'average', axis.dbPerUnit)
      const meanPower = values.reduce((s, v) => s + 10 ** (valueToDb(axis, v) / 10), 0) / values.length
      expect(valueToDb(axis, out[0]), name).toBeCloseTo(10 * Math.log10(meanPower), 3)
    }
  })
})

/**
 * Where the axis says a value is: the dB of the tick that sits at the trace's peak, read back off the axis
 * at that height. The trace's peak is the y the renderer draws the row's loudest pixel at: `aggregateRow`
 * through the peak detector, then canvas2d.ts `drawTrace`'s own `yFor`, written out here rather than
 * taken from scale.ts, so an axis that drifted from the renderer's mapping would fail this.
 */
function readAtPeak(axis: ScaleAxis, bins: ArrayLike<number>, range: DisplayRange, h: number) {
  const px = new Float32Array(320)
  aggregateRow(bins, 0, bins.length, 0, 4000, 0, 4000, px, 'peak', axis.dbPerUnit)
  const peakY = h - strengthIn(Math.max(...px), range.floor, range.ceil) * (h - 1)
  const tick = axisTicks(axis, range, h).find((t) => t.db === -20)
  return { peakY, tickY: tick?.y ?? Number.NaN, reading: dbAtY(axis, range, h, peakY) }
}

describe('a −20 dBFS tone reads −20 dBFS on the axis', () => {
  const H = 120
  // The noise floors and the operator's G and Z the tone is checked under: the auto range moves with the
  // noise, G widens or narrows it, Z trims its black point, and the tick must follow the tone through all.
  const cases: [number, number, number][] = [
    [-60, 0, 0],
    [-65, 0, 0],
    [-60, -1, 0],
    [-60, 0.2, 0],
    [-60, 0, 1],
    [-60, -0.5, -1],
  ]

  it('on the frame as the backend sends it, with its own range, at every G and Z', () => {
    const golden = rustGoldens().find((g) => g.source === 'audio')!
    for (const [noise, gain, zero] of cases) {
      // amplitude 0.1 = −20 dBFS, encoded exactly as the producer encodes it.
      const wire = { ...golden, bins: audioRow(noise, 0.1, 7) }
      const axis = axisOf(wire.scale, wire.source)
      const frame = rendererFrame(wire)
      const range = displayRange(autoRange(frame.bins, frame.dbPerUnit), gain, zero, frame.dbPerUnit)
      const { peakY, tickY, reading } = readAtPeak(axis, frame.bins, range, H)
      const at = `noise ${noise} dBFS, G ${gain}, Z ${zero}`
      expect(Number.isNaN(tickY), `${at}: the −20 tick is on the axis`).toBe(false)
      expect(Math.abs(tickY - peakY), `${at}: the −20 tick sits on the tone`).toBeLessThan(0.01)
      // To float32's grain: the renderer's pixel values are Float32 (2.4e-6 dB here).
      expect(reading, at).toBeCloseTo(-20, 4)
    }
  })

  it('CONTROL: an axis pinned to the wrong reference fails the same check', () => {
    // The −100 dBFS floor of an older axis, or the CI-V figure handed to an audio frame: each must put the
    // −20 tick off the tone by more than a pixel and misread it by more than a dB, or the check above
    // could not fail.
    const golden = rustGoldens().find((g) => g.source === 'audio')!
    const wrong: [string, ScaleAxis][] = [
      ['a −100 dBFS floor', axisOf({ kind: 'dbfs', loDb: -100, hiDb: 0 }, 'audio')],
      ["the CI-V scope's 80 dB", { ...axisOf({ kind: 'relative' }, 'civ'), kind: 'dbfs' }],
    ]
    for (const [name, axis] of wrong) {
      for (const [noise, gain, zero] of cases) {
        const frame = rendererFrame({ ...golden, bins: audioRow(noise, 0.1, 7) })
        const range = displayRange(autoRange(frame.bins, frame.dbPerUnit), gain, zero, frame.dbPerUnit)
        const { peakY, tickY, reading } = readAtPeak(axis, frame.bins, range, H)
        const at = `${name}: noise ${noise}, G ${gain}, Z ${zero}`
        expect(Number.isNaN(tickY) || Math.abs(tickY - peakY) > 1, at).toBe(true)
        expect(Math.abs(reading + 20), at).toBeGreaterThan(1)
      }
    }
  })

  it('still reads −20 dBFS through two seconds of 250 ms averaging over a moving noise floor', () => {
    // A steady carrier keeps its level through the log-recursive average while the noise is smoothed.
    const golden = rustGoldens().find((g) => g.source === 'audio')!
    const avg = new LogRecursiveAverager(250)
    let out = rendererFrame(golden)
    for (let i = 0; i < 100; i++) out = avg.push(rendererFrame({ ...golden, bins: audioRow(-62, 0.1, 100 + i) }), i * 20)
    const axis = axisOf(golden.scale, golden.source)
    const range = displayRange(autoRange(out.bins, out.dbPerUnit), 0, 0, out.dbPerUnit)
    const { peakY, tickY, reading } = readAtPeak(axis, out.bins, range, H)
    expect(Math.abs(tickY - peakY)).toBeLessThan(0.01)
    expect(reading).toBeCloseTo(-20, 4)
  })
})

describe('the graduations', () => {
  const audio = axisOf({ kind: 'dbfs', loDb: -120, hiDb: 0 }, 'audio')
  const civ = axisOf({ kind: 'relative' }, 'civ')

  it('are whole steps inside the range, top first, at least the minimum gap apart', () => {
    for (const [axis, range, h] of [
      [audio, { floor: dbToValue(audio, -100), ceil: dbToValue(audio, -45) }, 120],
      [audio, { floor: dbToValue(audio, -100), ceil: dbToValue(audio, -45) }, 600],
      [audio, { floor: dbToValue(audio, -130), ceil: dbToValue(audio, 5) }, 90],
      [civ, { floor: 0.1, ceil: 0.9 }, 240],
    ] as [ScaleAxis, DisplayRange, number][]) {
      const ticks = axisTicks(axis, range, h)
      expect(ticks.length).toBeGreaterThan(1)
      const step = ticks[0].db - ticks[1].db
      expect([1, 2, 5, 10, 20, 50]).toContain(step)
      for (let i = 0; i < ticks.length; i++) {
        const { db, y } = ticks[i]
        expect(Math.abs(db % step)).toBe(0)
        expect(db).toBeLessThanOrEqual(valueToDb(axis, range.ceil) + 1e-9)
        expect(db).toBeGreaterThanOrEqual(valueToDb(axis, range.floor) - 1e-9)
        expect(dbAtY(axis, range, h, y)).toBeCloseTo(db, 9)
        if (i > 0) expect(y - ticks[i - 1].y).toBeGreaterThanOrEqual(16 - 1e-9)
      }
    }
    // Full scale is labelled 0, never −0.
    expect(Object.is(axisTicks(audio, { floor: 0.5, ceil: 1 }, 200)[0].db, 0)).toBe(true)
    // A relative axis reads dB below the display's top: never above it.
    expect(axisTicks(civ, { floor: 0, ceil: 1 }, 240).every((t) => t.db <= 0)).toBe(true)
  })

  it('label the REF beside a relative axis only when it is known', () => {
    expect(refLabel(2.5)).toBe('REF +2.5 dB')
    expect(refLabel(-3)).toBe('REF −3.0 dB')
    expect(refLabel(0)).toBe('REF +0.0 dB')
    expect(refLabel(null)).toBeNull()
    expect(refLabel(undefined)).toBeNull()
    expect(refLabel(Number.NaN)).toBeNull()
  })
})
