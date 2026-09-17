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
import { SCRIPTED_CAT_MS, scriptedStation } from './__fixtures__/scripted-station'
import { BUDGET, CHECK_STEPS, ResponsivenessProbe, reportText, runResponsivenessCheck, type CheckReport } from './responsiveness'

type Station = ReturnType<typeof scriptedStation>
const open: Station[] = []
afterEach(() => { open.splice(0).forEach(s => s.close()) })
const wait = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))

async function measure(rttMs: number, version: 4 | 5 = 4, probe = new ResponsivenessProbe(), attach = true): Promise<{ station: Station; report: CheckReport }> {
  const station = scriptedStation(rttMs, version)
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

/** Today's numbers plus a tenth for cadence slack, in ms, for the link each describes. The
 * ratchet: a run may come in under these, never over. Lower a line when a batch lands; never raise
 * one to make a red run green — a red here is the instrument doing its job.
 *
 * PRE-PUSH-COMPLETION (a v4 station, or a v5 page on one — 2026-09-16): at 100 ms — screen 120,
 * tune confirmed 1100, band confirmed 550, readout 1000, 4 of 10 steps sent; at 400 ms — screen
 * 200 (a flush that waited on a snapshot read), tune confirmed 1900, band confirmed 1400, readout
 * 1500, 3 of 10 sent. The polled result read is what makes a confirmation seconds instead of a
 * round trip, and refusing steps while one is in flight is what loses most of the ten.
 *
 * PUSH-COMPLETION (a v5 station naming `outcomePush` — 2026-09-17, int-all at 684bfb70): the
 * confirmation is the station's own word, so a band change confirms in a round trip plus the CAT
 * time: at 100 ms — tune confirmed 750, band confirmed 300, 3 of 10 steps sent; at 400 ms — tune
 * confirmed 1100, band confirmed 550, 2 of 10 sent. The screen and readout clocks do not move
 * with it (the wheel's debounce and the 500 ms snapshot cadence are batches 1 and 3), and STEPS
 * SENT WENT DOWN, 4→3 and 3→2: the pushed outcome resolves the control but does not clear its
 * receipt on a real page (the receipt's write needs the storage lock, which only the polled path
 * holds), so the controls stay refused until the fallback poll a second on — longer than the v4
 * re-read took. The cases under "push-completion against polling" hold that finding. */
const BASELINE = {
  'v4/100': { screen: 130, tuneConfirmed: 1200, bandConfirmed: 650, readout: 1100, stepsSent: 4 },
  'v4/400': { screen: 220, tuneConfirmed: 2050, bandConfirmed: 1550, readout: 1650, stepsSent: 3 },
  'v5/100': { screen: 130, tuneConfirmed: 850, bandConfirmed: 350, readout: 1100, stepsSent: 3 },
  'v5/400': { screen: 220, tuneConfirmed: 1250, bandConfirmed: 650, readout: 1650, stepsSent: 2 }
} as const

describe.each([
  { rttMs: 100, version: 4 }, { rttMs: 400, version: 4 }, { rttMs: 100, version: 5 }, { rttMs: 400, version: 5 }
] as const)('on a $rttMs ms link at operation v$version', ({ rttMs, version }) => {
  it('measures every clock, and today reads as the recorded baseline or better', async () => {
    const { report: r } = await measure(rttMs, version)
    console.info(`operation v${version}\n` + reportText(r))
    expect(r.refused).toBeNull()
    expect(r.skipped).toEqual({ tune: null, band: null, stop: null })
    // The measured round trip IS the injected one: the probe's clocks are honest.
    expect(r.rtt!.typical).toBe(rttMs)
    expect(r.tune.made).toBe(CHECK_STEPS)
    expect(r.band.made).toBe(2); expect(r.band.sent).toBe(2); expect(r.band.confirmed!.count).toBe(2)
    expect(r.stop!.accepted).toBe(rttMs)
    expect(r.stop!.readout).not.toBeNull()
    // Every confirmation names its path, and a station's version decides it: a v4 station's are
    // all polled, a v5 station's all pushed. A run that mixed the two would answer "did it get
    // faster" badly, so the report keeps them apart.
    for (const part of [r.tune, r.band]) {
      expect(part.pushed + part.polled).toBe(part.confirmed!.count)
      expect(version === 5 ? part.polled : part.pushed).toBe(0)
    }
    expect(reportText(r)).toContain(version === 5 ? 'by polling: 0' : "by the station's push: 0")
    const line = BASELINE[`v${version}/${rttMs}`]
    expect(r.tune.sent).toBeGreaterThanOrEqual(line.stepsSent)
    expect(r.tune.screen!.worst).toBeLessThanOrEqual(line.screen)
    expect(r.tune.confirmed!.worst).toBeLessThanOrEqual(line.tuneConfirmed)
    expect(r.band.confirmed!.worst).toBeLessThanOrEqual(line.bandConfirmed)
    expect(r.tune.readout!.worst).toBeLessThanOrEqual(line.readout)
  })
  it.fails('meets the programme budget (flip to `it` when it does)', async () => {
    const { report: r } = await measure(rttMs, version)
    expect(r.budget).toEqual({ screen: true, tune: true, band: true, readout: true, flicker: true, steps: true })
    expect(r.tune.screen!.worst).toBeLessThanOrEqual(BUDGET.screenMs)
    expect(r.tune.confirmed!.worst).toBeLessThanOrEqual(rttMs + BUDGET.tuneAfterRttMs)
    expect(r.tune.sent).toBe(CHECK_STEPS)
    expect(r.flicker.run).toBe(0)
  })
})

// BEFORE AND AFTER, side by side: the same script on the same link against a station that polls
// (v4) and one that pushes (v5), so the improvement push-completion buys is a printed, asserted
// number and not a belief — and so is what it has not bought yet.
describe.each([100, 400] as const)('push-completion against polling on a %i ms link', rttMs => {
  it('confirms every control sooner, by the station\'s push instead of a poll', async () => {
    const before = (await measure(rttMs, 4)).report, after = (await measure(rttMs, 5)).report
    console.info(`${rttMs} ms link, polled (v4) → pushed (v5): tune confirmed ${before.tune.confirmed!.worst} → ${after.tune.confirmed!.worst} ms, ` +
      `band confirmed ${before.band.confirmed!.worst} → ${after.band.confirmed!.worst} ms, steps sent ${before.tune.sent} → ${after.tune.sent} of ${CHECK_STEPS}, ` +
      `controls went off ${before.flicker.run} → ${after.flicker.run} times`)
    expect(after.tune.confirmed!.worst).toBeLessThan(before.tune.confirmed!.worst)
    expect(after.band.confirmed!.worst).toBeLessThan(before.band.confirmed!.worst)
    expect(before.tune.pushed + before.band.pushed).toBe(0)
    expect(after.tune.polled + after.band.polled).toBe(0)
  })
  it.fails('loses no wheel step to the confirmation, and needs no fallback poll (flip to `it` when a pushed outcome clears the receipt)', async () => {
    // Today a v5 page accepts FEWER steps than a v4 page: the event resolves the control but its
    // receipt survives it, so the wheel is refused until the fallback poll clears it a second on.
    const before = (await measure(rttMs, 4)).report, after = (await measure(rttMs, 5)).report
    expect(after.tune.sent).toBeGreaterThanOrEqual(before.tune.sent)
    expect(after.flicker.run).toBeLessThan(before.flicker.run)
  })
})

// THE RECEIPT AND THE PUSHED OUTCOME. `pendingControlStorage.write` refuses outside `exclusive()`;
// the polled path always clears the receipt inside it, the event path never holds it. Two cases
// hold the consequence, both expected to fail until `receiveEvent` clears the receipt under the
// lock; the day it does, both go red and are flipped to `it`.
describe('a pushed outcome and the pending-control receipt', () => {
  it.fails('clears the receipt at once, so nothing is pending a round trip and a CAT time after the send (flip to `it` when fixed)', async () => {
    const station = scriptedStation(100, 5)
    open.push(station)
    await vi.advanceTimersByTimeAsync(1600)
    let settled = false
    void station.operations.control({ action: 'radio.band', band: '17m', mode: 'phone' }).then(() => { settled = true }, () => {})
    await vi.advanceTimersByTimeAsync(100 + SCRIPTED_CAT_MS + 60)
    expect(settled, 'the pushed outcome resolved the control').toBe(true)
    expect(station.operations.getSnapshot().controlPending, 'and cleared its receipt with it').toBeNull()
    await vi.advanceTimersByTimeAsync(1500)
    expect(station.wire.filter(w => w.type === 'result'), 'so no fallback poll was needed').toHaveLength(0)
  })
  it.fails('landing while the fallback poll is in flight, it does not make the poll\'s reply a crossed one (flip to `it` when fixed: today `invalidOperation`, which closes the socket)', async () => {
    // A radio slow enough that the event lands during the poll's round trip: a band change with a
    // mode-before-dial window. The poll holds the lock, so THIS time the event's clear succeeds —
    // and the poll's own reply then fails the crossed-reply check.
    const station = scriptedStation(100, 5, { catMs: 1050 })
    open.push(station)
    const thrown: string[] = []
    const receive = station.operations.receive.bind(station.operations)
    station.operations.receive = (raw: unknown, bytes?: number) => { try { receive(raw, bytes) } catch (error) { thrown.push((error as Error).message) } }
    await vi.advanceTimersByTimeAsync(1600)
    void station.operations.control({ action: 'radio.band', band: '17m', mode: 'phone' }).catch(() => {})
    await vi.advanceTimersByTimeAsync(3000)
    expect(station.wire.filter(w => w.type === 'result').length, 'the fallback poll went out').toBeGreaterThan(0)
    expect(thrown).toEqual([])
  })
})

/** A session's observable behaviour: every message that left the browser, when, and how often
 * anything on the render path was told. Times are relative to the first message. */
const behaviour = (s: Station) => ({ wire: s.wire.map(w => ({ type: w.type, at: w.at - s.wire[0].at })), notifications: { ...s.notifications } })

it('the probe does not change what it measures: attached or not, the wire and the render-path notifications are the same', async () => {
  const measured = await measure(100)
  const bare = await measure(100, 4, new ResponsivenessProbe(), false)
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

// THE PUSHED PATH IS NOT BLIND. Push-completion (operation v5) delivers a control's outcome as an
// operationEvent, a different code path from the polled reply; a probe that stamped only the polled
// path would report nothing for the one path it was built to show working, and the operator would
// read the polled numbers as "the fix did nothing". So the event path is held to a stamp of its
// own, isolated: the station's result poll is scripted never to settle, leaving the event as the
// only possible source of a confirmation.
describe('a pushed confirmation is stamped by the event path itself', () => {
  async function bandChange(version: 4 | 5, options: { push?: boolean; pollSettles?: boolean }) {
    const station = scriptedStation(100, version, options)
    open.push(station)
    const probe = new ResponsivenessProbe()
    probe.attach(station)
    await vi.advanceTimersByTimeAsync(1600)
    probe.beginRun()
    const gesture = probe.gesture('band', { band: '17m' })!
    probe.dispatch([gesture])
    void station.operations.control({ action: 'radio.band', band: '17m', mode: 'phone' }).catch(() => {})
    await vi.advanceTimersByTimeAsync(3000)
    return { station, gesture }
  }
  it('stamps CONFIRMED from the pushed event alone, a round trip plus the CAT time after the send', async () => {
    const { gesture } = await bandChange(5, { push: true, pollSettles: false })
    expect(gesture.sent).toBeDefined()
    expect(gesture.confirmed).toBeDefined()
    expect(gesture.via).toBe('pushed')
    expect(gesture.outcome).toBe('applied')
    expect(gesture.confirmed! - gesture.sent!).toBe(100 + SCRIPTED_CAT_MS)
  })
  it('control: with no event and a poll that never settles, nothing stamps — so the assertion above is live against the stamp being removed from the event path', async () => {
    const { gesture, station } = await bandChange(5, { push: false, pollSettles: false })
    expect(gesture.sent).toBeDefined()
    expect(station.wire.filter(w => w.type === 'result').length, 'the poll did run, and answered pending').toBeGreaterThan(0)
    expect(gesture.confirmed).toBeUndefined()
    expect(gesture.via).toBeUndefined()
  })
  it('control: the polled path stamps its own way, and says so', async () => {
    const { gesture } = await bandChange(4, {})
    expect(gesture.confirmed).toBeDefined()
    expect(gesture.via).toBe('polled')
  })
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
