import { afterEach, expect, it } from 'vitest'
import {
  MIC_LEVEL_DB, MIC_LEVEL_WORKLET_NAME, MIC_METER_FALL, MIC_METER_FLOOR_DB, MIC_PEAK, MicMeter, browserMicLevel, meterDb, micGain,
  micLevel,
} from './mic-level'
// The file exactly as it ships: the build copies it unchanged.
import MIC_LEVEL_SOURCE from './mic-level-processor.js?raw'

// The level and its limiter run in the audio thread, between the microphone and the stream's line. jsdom
// has no AudioWorklet, so the two globals the source needs are supplied and the real `process()` is called
// with real render quanta, as audio-worklet.test.ts does for the receive side.
type Port = { onmessage: ((event: { data: unknown }) => void) | null; postMessage: (value: unknown) => void }
type Processor = { port: Port; process: (i: Float32Array[][], o: Float32Array[][]) => boolean }
type Report = { peak: number; limited: boolean }

const QUANTUM = 128
const RATE = 48000
let registered: string[] = []
afterEach(() => { registered = [] })

function build(options: { gain?: number; every?: number } = {}) {
  let Processor: new (o: unknown) => Processor
  const reports: Report[] = []
  class Base {
    port: Port = { onmessage: null, postMessage: (value: unknown) => { reports.push(value as Report) } }
  }
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  new Function('AudioWorkletProcessor', 'registerProcessor', 'sampleRate', MIC_LEVEL_SOURCE)(
    Base,
    (name: string, klass: new (o: unknown) => Processor) => { registered.push(name); Processor = klass },
    RATE,
  )
  const worklet = new Processor!({ processorOptions: { gain: options.gain ?? 1, peak: MIC_PEAK, every: options.every ?? 2400 } })
  /** What the line carries for `input`, quantum by quantum. */
  const run = (input: Float32Array) => {
    const out: number[] = []
    for (let at = 0; at < input.length; at += QUANTUM) {
      const quantum = new Float32Array(QUANTUM)
      quantum.set(input.subarray(at, at + QUANTUM))
      const channel = new Float32Array(QUANTUM)
      expect(worklet.process([[quantum]], [[channel]])).toBe(true)
      out.push(...channel)
    }
    return out.slice(0, input.length)
  }
  const level = (gain: unknown) => worklet.port.onmessage!({ data: { gain } })
  return { worklet, run, level, reports }
}

/** `n` samples of a `hz` tone at `amplitude`. */
const tone = (n: number, amplitude: number, hz = 1000) => Float32Array.from({ length: n }, (_, i) => amplitude * Math.sin(2 * Math.PI * hz * i / RATE))

it('sends the microphone exactly as it is at the default level: today\'s level, sample for sample', () => {
  expect(MIC_LEVEL_DB.default).toBe(0)
  const { run } = build({ gain: micGain(MIC_LEVEL_DB.default) })
  const voice = tone(4800, 0.5)
  expect(run(voice)).toEqual([...voice])
})

it('lifts the voice by the operator\'s level, exactly, while it stays under the peak', () => {
  const { run } = build({ gain: micGain(12) })
  const quiet = tone(4800, 0.05)
  const sent = run(quiet)
  sent.forEach((sample, i) => expect(sample).toBeCloseTo(quiet[i] * micGain(12), 6))
  // +12 dB is four times the amplitude, near enough: 0.05 becomes 0.199.
  expect(Math.max(...sent)).toBeCloseTo(0.05 * 3.981, 3)
})

it('never sends a sample past the peak, however loud the voice and the level, and lets go once it is quiet again', () => {
  const { run } = build({ gain: micGain(MIC_LEVEL_DB.max) })
  // A full-scale word at the top of the range would be +20 dBFS. It reaches the peak and never passes it.
  const loud = run(tone(9600, 0.99))
  const peak = Math.max(...loud.map(Math.abs))
  expect(peak).toBeLessThanOrEqual(MIC_PEAK)
  expect(peak).toBeGreaterThan(MIC_PEAK * 0.98)
  // A second of quiet speech later the level is the operator's again: 0.01 times ten.
  const after = run(tone(48000, 0.01)).slice(-4800)
  expect(Math.max(...after)).toBeCloseTo(0.01 * micGain(MIC_LEVEL_DB.max), 3)
})

it('takes a new level from the next sample on, and refuses one that is not a gain', () => {
  const { run, level } = build()
  level(2)
  expect(Math.max(...run(tone(4800, 0.1)))).toBeCloseTo(0.2, 4)
  for (const bad of [-1, Number.NaN, '3', null]) level(bad)
  expect(Math.max(...run(tone(4800, 0.1))), 'a bad message changed the level').toBeCloseTo(0.2, 4)
})

it('reports the loudest sample it sent, and whether the limiter held one down, every 50 ms', () => {
  const { run, reports } = build({ gain: 1, every: RATE * 50 / 1000 })
  run(tone(4800, 0.3))
  expect(reports).toHaveLength(2)
  expect(reports[0].peak).toBeCloseTo(0.3, 2)
  expect(reports[0].limited).toBe(false)
  run(tone(2400, 0.99))
  expect(reports[2]).toMatchObject({ limited: true })
  expect(reports[2].peak).toBeLessThanOrEqual(MIC_PEAK)
})

it('sends silence, never noise, with nothing connected to it', () => {
  const { worklet } = build({ gain: micGain(MIC_LEVEL_DB.max) })
  const channel = new Float32Array(QUANTUM).fill(1)
  expect(worklet.process([[]], [[channel]])).toBe(true)
  expect([...channel].every(sample => sample === 0)).toBe(true)
  expect(worklet.process([], [[channel]])).toBe(true)
})

it('was actually exercised: the harness evaluated the shipped source, and it registers the page\'s name', () => {
  build()
  expect(registered).toEqual([MIC_LEVEL_WORKLET_NAME])
})

it('the level in whole dB inside its range; anything unreadable is today\'s level, never silence or a blast', () => {
  for (const unreadable of [null, undefined, '', 'loud', Number.NaN, Number.POSITIVE_INFINITY, {}, []]) expect(micLevel(unreadable)).toBe(MIC_LEVEL_DB.default)
  expect(micLevel('6')).toBe(6)
  expect(micLevel(6.4)).toBe(6)
  expect(micLevel(-3)).toBe(-3)
  expect(micLevel(30), 'past the ceiling').toBe(MIC_LEVEL_DB.max)
  expect(micLevel(-40)).toBe(MIC_LEVEL_DB.min)
  expect(MIC_LEVEL_DB).toEqual({ min: -12, max: 20, default: 0 })
})

it('dB to the multiplier the level applies; the peak leaves the codec 3 dB', () => {
  expect(micGain(0)).toBe(1)
  expect(20 * Math.log10(MIC_PEAK)).toBeCloseTo(-3, 9)
  expect(micGain(20)).toBeCloseTo(10, 9)
  expect(micGain(6)).toBeCloseTo(1.9953, 4)
  expect(micGain(-12)).toBeCloseTo(0.2512, 4)
})

it('the meter: live only while the level runs, falls 12 dB a second after a peak, and holds a limited one for half a second', () => {
  const meter = new MicMeter()
  let told = 0
  meter.subscribe(() => { told++ })
  expect(meter.getSnapshot()).toEqual({ live: false, peak: 0, limited: false })
  meter.report(0.5, false)
  expect(meter.getSnapshot().live, 'a report before the level runs counts').toBe(false)
  meter.start()
  meter.report(0.8, false)
  expect(meter.getSnapshot()).toEqual({ live: true, peak: 0.8, limited: false })
  meter.report(0, false)
  expect(meter.getSnapshot().peak).toBeCloseTo(0.8 * MIC_METER_FALL, 9)
  expect(20 * Math.log10(MIC_METER_FALL)).toBeCloseTo(-0.6, 9)
  for (let i = 0; i < 50; i++) meter.report(0, false)
  expect(meter.getSnapshot().peak).toBeLessThan(0.8 * 10 ** (-30 / 20))
  meter.report(0.9, true)
  for (let i = 0; i < 9; i++) meter.report(0.1, false)
  expect(meter.getSnapshot().limited, 'the limiter\'s mark went before half a second').toBe(true)
  meter.report(0.1, false)
  expect(meter.getSnapshot().limited).toBe(false)
  meter.stop()
  meter.report(0.7, true)
  expect(meter.getSnapshot()).toEqual({ live: false, peak: 0, limited: false })
  expect(told).toBeGreaterThan(0)
})

it('the meter reads in dBFS from its floor to full scale', () => {
  expect(meterDb(1)).toBe(0)
  expect(meterDb(0.5)).toBeCloseTo(-6.02, 2)
  expect(meterDb(0)).toBe(MIC_METER_FLOOR_DB)
  expect(meterDb(1e-9)).toBe(MIC_METER_FLOOR_DB)
})

it('the browser keeps the operator\'s level in this browser', () => {
  localStorage.removeItem('nexus.remote.micLevel')
  const env = browserMicLevel()
  expect(env.store!.load()).toBeNull()
  env.store!.save(6)
  expect(localStorage.getItem('nexus.remote.micLevel')).toBe('6')
  expect(micLevel(env.store!.load())).toBe(6)
  localStorage.removeItem('nexus.remote.micLevel')
})
