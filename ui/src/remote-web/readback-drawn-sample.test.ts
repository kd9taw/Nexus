// THE DIAL A PAGE DRAWS TRAILS THE STATION. The cockpit draws a station sample a poll after the
// stream holds it, so right after a readback the stream can already carry a sample taken AFTER the
// readback while the controls are still drawn on the one from BEFORE the command. A notch or press
// in that poll was built on the dial the command had moved away from, and the pre-dispatch re-read
// refused it as though the radio had moved: "Not sent", nothing on the wire, the digits jumping
// back to a dial the station had already left.
//
// The station, the stream and the receipt lock here are the shipped clients (`scripted-station`);
// the drawn sample is the `dialMhz` a step or press is given, exactly what `useWheelTune` hands
// `WheelTuning` from the render. Each fix case has a control that must still refuse: once the
// station's newest sample shows the radio somewhere else, nothing is built on the readback.
import { afterEach, expect, it, vi } from 'vitest'
import { scriptedStation } from './__fixtures__/scripted-station'
import type { AppSnapshot } from '../types'

type Station = ReturnType<typeof scriptedStation>
const open: Station[] = []
afterEach(() => { open.splice(0).forEach(s => s.close()) })

async function until(ready: () => boolean, maxMs: number) {
  for (let waited = 0; !ready(); waited += 50) {
    if (waited >= maxMs) return false
    await vi.advanceTimersByTimeAsync(50)
  }
  return true
}
/** A station sample TAKEN after `at` is the newest one the page holds. */
const sampledAfter = (station: Station, at: number) => performance.now() - station.application.age('get_snapshot') > at

async function tunedOnce() {
  const station = scriptedStation(100, 4)
  open.push(station)
  await vi.advanceTimersByTimeAsync(1600)
  const first = station.application.invoke<AppSnapshot>('get_snapshot')
  await vi.advanceTimersByTimeAsync(700)
  await first
  const context = station.operations.getSnapshot().state!.controls!.context
  const source = (dialMhz: number) => ({ dialMhz, sideband: 'USB', context })
  // The page's render of the dial: one poll behind the stream cache.
  const rendered = station.radio.dialMhz
  expect(station.tuning.nudge(1000, source(rendered))).toBe(true)
  expect(await until(() => station.wire.some(w => w.type === 'stationControl') && !station.tuning.getPending(), 5000)).toBe(true)
  expect(station.radio.dialMhz).toBe(14.201)
  const controls = () => station.wire.filter(w => w.type === 'stationControl').length
  return { station, source, rendered, confirmedAt: performance.now(), controls }
}

it('render one sample behind the readback: the next step still goes out', async () => {
  const { station, source, rendered, confirmedAt, controls } = await tunedOnce()
  expect(await until(() => sampledAfter(station, confirmedAt), 3000)).toBe(true)
  expect(station.tuning.nudge(100, source(rendered))).toBe(true)
  await vi.advanceTimersByTimeAsync(3000)
  expect(station.failures).toEqual([])
  expect(controls()).toBe(2)
  expect(station.radio.dialMhz).toBe(14.2011)
}, 60_000)

it('control: the radio moved after the readback and the page has not shown it — refused', async () => {
  const { station, source, rendered, controls } = await tunedOnce()
  station.radio.dialMhz = 14.25
  const movedAt = performance.now()
  expect(await until(() => sampledAfter(station, movedAt), 3000)).toBe(true)
  for (const shown of [rendered, 14.201]) {
    expect(station.tuning.nudge(100, source(shown))).toBe(true)
    await vi.advanceTimersByTimeAsync(3000)
    expect(station.failures.pop()).toBe('staleContext')
    expect(controls()).toBe(1)
    expect(station.radio.dialMhz).toBe(14.25)
  }
}, 60_000)

it('control: the radio moved after the readback and the page shows it — the step builds on it', async () => {
  const { station, source, controls } = await tunedOnce()
  station.radio.dialMhz = 14.25
  const movedAt = performance.now()
  expect(await until(() => sampledAfter(station, movedAt), 3000)).toBe(true)
  expect(station.tuning.nudge(100, source(14.25))).toBe(true)
  await vi.advanceTimersByTimeAsync(3000)
  expect(station.failures).toEqual([])
  expect(controls()).toBe(2)
  expect(station.radio.dialMhz).toBe(14.2501)
}, 60_000)

it('render one sample behind the readback: a scope press still goes out', async () => {
  const { station, source, rendered, confirmedAt, controls } = await tunedOnce()
  expect(await until(() => sampledAfter(station, confirmedAt) && station.tuning.ready(), 3000)).toBe(true)
  const press = station.tuning.captureTarget(source(rendered))
  expect(press).not.toBeNull()
  expect(press!(14_203_000)).toBe(true)
  await vi.advanceTimersByTimeAsync(3000)
  expect(station.failures).toEqual([])
  expect(controls()).toBe(2)
  expect(station.radio.dialMhz).toBe(14.203)
}, 60_000)

it('control: a scope press after the radio moved, which the page has not shown, is refused', async () => {
  const { station, source, rendered, controls } = await tunedOnce()
  station.radio.dialMhz = 14.25
  const movedAt = performance.now()
  expect(await until(() => sampledAfter(station, movedAt) && station.tuning.ready(), 3000)).toBe(true)
  const press = station.tuning.captureTarget(source(rendered))
  expect(press!(14_203_000)).toBe(true)
  await vi.advanceTimersByTimeAsync(3000)
  expect(station.failures).toEqual(['staleContext'])
  expect(controls()).toBe(1)
  expect(station.radio.dialMhz).toBe(14.25)
}, 60_000)
