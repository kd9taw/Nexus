// THE CI TWIN of the Responsiveness check: the same script the operator runs from the panel,
// against a scripted station on a link of an injected round trip, under fake timers. It reports
// today's numbers in the test log and holds two lines:
//
//   · the RATCHET — nothing may get slower than the baseline recorded here (lower these numbers as
//     the responsiveness batches land; never raise one to make a red run green);
//   · the BUDGET — the programme's pass line. Today's build does not meet it, and that is the point
//     of measuring before fixing: the case is `it.fails`, so the day a change meets the budget this
//     file goes red and the case is flipped to `it`.
//
// It also proves the two properties the probe must have: it does not change what it measures (the
// wire and the render-path notifications are identical with the probe attached and without, and
// a probe that DOES touch the wire is caught), and it costs nothing while its panel is closed.
import { afterEach, describe, expect, it } from 'vitest'
import { vi } from 'vitest'
import { scriptedStation } from './__fixtures__/scripted-station'
import { BUDGET, CHECK_STEPS, ResponsivenessProbe, reportText, runResponsivenessCheck, type CheckReport } from './responsiveness'

type Station = ReturnType<typeof scriptedStation>
const open: Station[] = []
afterEach(() => { open.splice(0).forEach(s => s.close()) })
const wait = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))

async function measure(rttMs: number, probe = new ResponsivenessProbe(), attach = true): Promise<{ station: Station; report: CheckReport }> {
  const station = scriptedStation(rttMs)
  open.push(station)
  if (attach) probe.attach(station)
  // Control current and the first snapshot in, as when the operator reaches the panel.
  await vi.advanceTimersByTimeAsync(rttMs + 1500)
  let report: CheckReport | undefined, failure: unknown
  void runResponsivenessCheck({ ...station, probe, wait }).then(r => { report = r }, e => { failure = e })
  await vi.advanceTimersByTimeAsync(45_000)
  if (failure) throw failure
  expect(report, 'the check finished').toBeDefined()
  return { station, report: report! }
}

/** Today's numbers (2026-09-16) plus a tenth for cadence slack, in ms, for the link each describes.
 * The ratchet: a run may come in under these, never over. Measured: at 100 ms — screen 120, tune
 * confirmed 1100, band confirmed 550, readout 1000, 4 of 10 steps sent; at 400 ms — screen 200
 * (a flush that waited on a snapshot read), tune confirmed 1900, band confirmed 1400, readout 1500,
 * 3 of 10 sent. The polled result read is what makes a confirmation seconds instead of a round
 * trip, and refusing steps while one is in flight is what loses most of the ten. */
const BASELINE = {
  100: { screen: 130, tuneConfirmed: 1200, bandConfirmed: 650, readout: 1100, stepsSent: 4 },
  400: { screen: 220, tuneConfirmed: 2050, bandConfirmed: 1550, readout: 1650, stepsSent: 3 }
} as const

describe.each([100, 400] as const)('on a %i ms link', rttMs => {
  it('measures every clock, and today reads as the recorded baseline or better', async () => {
    const { report: r } = await measure(rttMs)
    console.info(reportText(r))
    expect(r.refused).toBeNull()
    expect(r.skipped).toEqual({ tune: null, band: null, stop: null })
    // The measured round trip IS the injected one: the probe's clocks are honest.
    expect(r.rtt!.typical).toBe(rttMs)
    expect(r.tune.made).toBe(CHECK_STEPS)
    expect(r.band.made).toBe(2); expect(r.band.sent).toBe(2); expect(r.band.confirmed!.count).toBe(2)
    expect(r.stop!.accepted).toBe(rttMs)
    expect(r.stop!.readout).not.toBeNull()
    const line = BASELINE[rttMs]
    expect(r.tune.sent).toBeGreaterThanOrEqual(line.stepsSent)
    expect(r.tune.screen!.worst).toBeLessThanOrEqual(line.screen)
    expect(r.tune.confirmed!.worst).toBeLessThanOrEqual(line.tuneConfirmed)
    expect(r.band.confirmed!.worst).toBeLessThanOrEqual(line.bandConfirmed)
    expect(r.tune.readout!.worst).toBeLessThanOrEqual(line.readout)
  })
  it.fails('meets the programme budget (flip to `it` when it does)', async () => {
    const { report: r } = await measure(rttMs)
    expect(r.budget).toEqual({ screen: true, tune: true, band: true, readout: true, flicker: true, steps: true })
    expect(r.tune.screen!.worst).toBeLessThanOrEqual(BUDGET.screenMs)
    expect(r.tune.confirmed!.worst).toBeLessThanOrEqual(rttMs + BUDGET.tuneAfterRttMs)
    expect(r.tune.sent).toBe(CHECK_STEPS)
    expect(r.flicker.run).toBe(0)
  })
})

/** A session's observable behaviour: every message that left the browser, when, and how often
 * anything on the render path was told. Times are relative to the first message. */
const behaviour = (s: Station) => ({ wire: s.wire.map(w => ({ type: w.type, at: w.at - s.wire[0].at })), notifications: { ...s.notifications } })

it('the probe does not change what it measures: attached or not, the wire and the render-path notifications are the same', async () => {
  const measured = await measure(100)
  const bare = await measure(100, new ResponsivenessProbe(), false)
  expect(behaviour(measured.station)).toEqual(behaviour(bare.station))
  expect(measured.station.wire.length).toBeGreaterThan(30)
  // Control: the comparison can fail. A different link differs...
  const slower = await measure(400)
  expect(behaviour(slower.station)).not.toEqual(behaviour(measured.station))
  // ...and a probe that touches the wire — one extra request on a reply — is caught by it.
  class Touching extends ResponsivenessProbe {
    station: Station | null = null
    override replied(requestId: string) { super.replied(requestId); void this.station?.operations.stopTransmit().catch(() => {}) }
  }
  const touching = new Touching()
  const perturbed = scriptedStation(100)
  open.push(perturbed)
  touching.station = perturbed
  touching.attach(perturbed)
  await vi.advanceTimersByTimeAsync(1600)
  void runResponsivenessCheck({ ...perturbed, probe: touching, wait }).catch(() => {})
  await vi.advanceTimersByTimeAsync(45_000)
  expect(behaviour(perturbed)).not.toEqual(behaviour(measured.station))
})

it('costs nothing while its panel is closed: no hook target, no listener, no timer', async () => {
  const station = scriptedStation(100)
  open.push(station)
  await vi.advanceTimersByTimeAsync(1600)
  const subscribed = vi.spyOn(station.operations, 'subscribe')
  const timers = vi.getTimerCount()
  const probe = new ResponsivenessProbe()
  // Built but not attached — what an operator who never opens the panel gets.
  expect(station.operations.probe).toBeNull(); expect(station.tuning.probe).toBeNull()
  expect(probe.gesture('tune'), 'nothing is recorded outside a run').toBeNull()
  expect(subscribed).not.toHaveBeenCalled()
  expect(vi.getTimerCount()).toBe(timers)
  // Control: attaching is exactly one listener and the three hook targets, and detaching undoes it
  // and leaves no timer behind.
  probe.attach(station)
  expect(subscribed).toHaveBeenCalledTimes(1)
  expect(station.operations.probe).toBe(probe); expect(station.tuning.probe).toBe(probe)
  expect(vi.getTimerCount()).toBe(timers)
  probe.detach()
  expect(station.operations.probe).toBeNull(); expect(station.tuning.probe).toBeNull()
  expect(vi.getTimerCount()).toBe(timers)
})

it('refuses rather than runs when this browser is not controlling or the radio is not idle', async () => {
  const station = scriptedStation(100)
  open.push(station)
  await vi.advanceTimersByTimeAsync(1600)
  const probe = new ResponsivenessProbe()
  probe.attach(station)
  station.radio.transmitting = true
  await vi.advanceTimersByTimeAsync(1100)
  let report: CheckReport | undefined
  void runResponsivenessCheck({ ...station, probe, wait }).then(r => { report = r })
  await vi.advanceTimersByTimeAsync(2000)
  expect(report?.refused).toBe('needIdle')
  expect(station.wire.filter(w => w.type === 'stationControl' || w.type === 'stopTransmit'), 'nothing was sent').toHaveLength(0)
  expect(reportText(report!)).toContain('Not run')
})
