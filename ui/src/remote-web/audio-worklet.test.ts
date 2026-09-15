import { expect, it, vi } from 'vitest'
import { AUDIO_WORKLET_NAME, AUDIO_WORKLET_SOURCE, AUDIO_BED_LEVEL } from './audio-worklet'

// The worklet source is evaluated and driven here rather than approximated, because the
// rules it holds - never time-stretch, a gap is audible - live nowhere else. jsdom has no
// AudioWorklet at all, so the two globals the source needs are supplied and the real
// `process()` is called with real render quanta.
type Port = { onmessage: ((event: { data: unknown }) => void) | null; postMessage: (value: unknown) => void }
type Processor = { port: Port; process: (i: Float32Array[][], o: Float32Array[][]) => boolean }
type Report = { held: number; underruns: number; dropped: number; playing: boolean }

const QUANTUM = 128
function build(options: { capacity: number; prefill: number; ceiling: number }) {
  let Processor: new (o: unknown) => Processor
  const reports: Report[] = []
  class Base {
    port: Port = {
      onmessage: null,
      postMessage: (value: unknown) => { reports.push(value as Report) },
    }
  }
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  new Function('AudioWorkletProcessor', 'registerProcessor', AUDIO_WORKLET_SOURCE)(
    Base,
    (name: string, klass: new (o: unknown) => Processor) => {
      expect(name).toBe(AUDIO_WORKLET_NAME)
      Processor = klass
    },
  )
  const worklet = new Processor!({ processorOptions: { ...options, level: AUDIO_BED_LEVEL } })
  const render = (quanta: number) => {
    const out: number[] = []
    for (let q = 0; q < quanta; q++) {
      const channel = new Float32Array(QUANTUM)
      expect(worklet.process([], [[channel]])).toBe(true)
      out.push(...channel)
    }
    return out
  }
  const push = (samples: Float32Array) => worklet.port.onmessage!({ data: { pcm: samples.buffer } })
  const reset = () => worklet.port.onmessage!({ data: { reset: true } })
  return { worklet, render, push, reset, reports }
}

/** A ramp, so every sample is identifiable and a reorder or a stretch is visible. */
const ramp = (n: number, from = 0) => Float32Array.from({ length: n }, (_, i) => (from + i) / 100000)

it('holds silence until prefill is met, then plays every sample exactly once in order', () => {
  const { render, push } = build({ capacity: 4800, prefill: 512, ceiling: 2048 })
  // Below prefill: nothing plays, and it is SILENCE, not the bed. A page that starts
  // hissing the moment it opens is worse than one that says "connecting".
  push(ramp(256))
  expect(render(2).every(sample => sample === 0)).toBe(true)
  push(ramp(512, 256))
  const played = render(6).slice(0, 768)
  // Exactly the samples that were pushed, in order, at rate. Any resampling or
  // stretching to manage depth would show up here as a value that was never pushed.
  expect(played).toEqual([...ramp(768)])
})

it('drops the oldest samples rather than speeding up when the buffer runs deep', () => {
  const { render, push, reports } = build({ capacity: 8192, prefill: 256, ceiling: 1024 })
  push(ramp(4096))
  const played = render(4)
  // Resynced to prefill, so playback resumes near the END of what arrived - the band as
  // it is now, not a recording of the band as it was.
  expect(played[0]).toBeCloseTo(ramp(4096)[4096 - 256], 6)
  expect(played.slice(0, 256)).toEqual([...ramp(4096).slice(4096 - 256)])
  render(60)
  expect(reports[reports.length - 1].dropped).toBe(4096 - 256)
})

it('makes a gap audible as a bed instead of letting it pass as a quiet band', () => {
  const { render, push, reports } = build({ capacity: 4800, prefill: 256, ceiling: 2048 })
  push(ramp(256))
  render(2)
  // Starved. What comes out must not be silence, or an operator cannot tell a dead link
  // from a dead band - which is the whole failure this control exists to prevent.
  const starved = render(40)
  expect(starved.some(sample => sample !== 0)).toBe(true)
  // And it must be far below any real signal, so it can never be mistaken for one.
  expect(Math.max(...starved.map(Math.abs))).toBeLessThan(0.05)
  expect(reports[reports.length - 1].underruns).toBeGreaterThan(0)
  expect(reports[reports.length - 1].playing).toBe(false)

  // The control, on the same instrument: real audio at the same moment is NOT the bed.
  push(ramp(1024, 10000))
  const resumed = render(4)
  expect(Math.max(...resumed.map(Math.abs))).toBeGreaterThan(0.05)
})

it('throws the buffer away on a real end rather than playing out a receiver that is gone', () => {
  const { render, push, reset } = build({ capacity: 4800, prefill: 256, ceiling: 2048 })
  push(ramp(2048, 50000))
  render(2)
  reset()
  const after = render(4)
  // Nothing from before the reset survives. The bed is allowed; the old samples are not.
  expect(after.every(sample => Math.abs(sample) < 0.05)).toBe(true)
  // Control: a fresh push after the reset plays normally, so the emptiness above is the
  // reset and not a worklet that has stopped working.
  push(ramp(1024, 90000))
  expect(Math.max(...render(4).map(Math.abs))).toBeGreaterThan(0.05)
})

it('never lets the ring exceed the capacity it was built with', () => {
  const { push, render, reports } = build({ capacity: 2048, prefill: 256, ceiling: 1024 })
  // Ten times the capacity, all at once, which is what a burst after a stall looks like.
  for (let i = 0; i < 10; i++) push(ramp(2048, i * 2048))
  render(40)
  expect(reports.every(report => report.held <= 2048)).toBe(true)
})

it('keeps rendering after an error in the page thread rather than killing the graph', () => {
  const { worklet, render } = build({ capacity: 4800, prefill: 256, ceiling: 2048 })
  // A message the page should never send. `process` must still return true: a worklet
  // that returns false is removed from the graph and audio never comes back.
  worklet.port.onmessage!({ data: null })
  worklet.port.onmessage!({ data: { nonsense: 1 } })
  expect(render(1)).toHaveLength(QUANTUM)
})

it('is safe to mount with no output connected', () => {
  const { worklet } = build({ capacity: 4800, prefill: 256, ceiling: 2048 })
  expect(worklet.process([], [[]])).toBe(true)
  expect(worklet.process([], [])).toBe(true)
})

it('reports depth often enough to drive a state line and rarely enough to stay cheap', () => {
  const { push, render, reports } = build({ capacity: 16384, prefill: 256, ceiling: 12288 })
  push(ramp(12288))
  render(64)
  expect(reports.length).toBe(2)
  expect(reports[0]).toMatchObject({ playing: true, underruns: 0 })
})

it('reports nothing at all while it is doing nothing', () => {
  const { render, reports } = build({ capacity: 4800, prefill: 256, ceiling: 2048 })
  render(31)
  expect(reports).toHaveLength(0)
})

it('was actually exercised - the harness really evaluated the shipped source', () => {
  // The positive control for every case above: if `new Function` had silently produced a
  // processor that never ran, they would all pass on empty output.
  const registered = vi.fn()
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  new Function('AudioWorkletProcessor', 'registerProcessor', AUDIO_WORKLET_SOURCE)(class {}, registered)
  expect(registered).toHaveBeenCalledOnce()
  expect(registered.mock.calls[0][0]).toBe(AUDIO_WORKLET_NAME)
})
