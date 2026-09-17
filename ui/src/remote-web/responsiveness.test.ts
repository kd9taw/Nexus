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
 * PUSH-COMPLETION (a v5 station naming `outcomePush` — 2026-09-17): the confirmation is the
 * station's own word, so a band change confirms in a round trip plus the CAT time: at 100 ms —
 * tune confirmed 750, band confirmed 300, 5 of 10 steps sent; at 400 ms — tune confirmed 1100,
 * band confirmed 550, 4 of 10 sent. The screen and readout clocks do not move with it: the
 * wheel's debounce and the 500 ms snapshot cadence are batches 1 and 3.
 *
 * As first merged (int-all at 684bfb70) it read 3 and 2 of 10 sent — FEWER than v4 — because the
 * pushed outcome resolved the control but did not clear its receipt on a real page (the receipt's
 * write needs the storage lock, which only the polled path held), so the controls stayed refused
 * until the fallback poll a second on. The cases under "a pushed outcome and the pending-control
 * receipt" hold the fix, and "push-completion against polling" holds the gesture count.
 *
 * ── THE OPTIMISTIC DIAL (2026-09-17) ──────────────────────────────────────────────────────────
 * `stepsSent` is **10 of 10 on every link and both versions**, up from 4/3/5/4: a step made while
 * a command is in flight now joins the next burst instead of being refused. `screen` is **0 ms**
 * everywhere, down from 120-200: the digits move on the step itself, so PERCEIVED is no longer
 * "when the burst was flushed". Those are the batch.
 *
 * ⚠️ AND `tuneConfirmed` AND `readout` WENT UP — 1100→1550, 1900→2950, 750→800, 1100→1550 — WHICH
 * IS NOT A REGRESSION, AND HERE IS HOW TO CHECK THAT RATHER THAN BELIEVE IT. Both are measured
 * FROM THE GESTURE, over the gestures that got confirmed at all. Before, six of the operator's ten
 * notches were refused and contributed nothing to either clock; the four that survived were each
 * the head of their own burst. Now all ten are served, and the later ones necessarily queue behind
 * the earlier ones on one CAT link — so the population grew by the slowest members it used to
 * throw away. The clock that is INDEPENDENT of how many gestures rode on a command is
 * `commandConfirmed` (confirmed − sent, per command), and it is unchanged: 550 / 1450 / 250 / 550,
 * the same numbers to the millisecond as `bandConfirmed`, which is one command per gesture by
 * construction and still passes its own untouched ratchet. The link and the radio cost exactly
 * what they cost before. If a future change makes a COMMAND slower, `commandConfirmed` and
 * `bandConfirmed` go red together and no amount of gesture accounting hides it.
 *
 * Flicker is the honest cost, measured against this same twin run on the batch's own base: 6→8,
 * 5→6, 7→12 and 6→8 controls-off events per run. Serving ten notches instead of four means ten
 * `controlPending` windows instead of four, and each one is the §3(b) grey-out that batch 1
 * (lapse-not-loss, "stay lit, show it is confirming") removes. Batch 3 adds no new mechanism; it
 * exercises the existing one more often, and that is a thing the operator can see until batch 1
 * lands. There is no ratchet line for it here because the budget already demands zero.
 *
 * ── THE FLICKER (batch 1, 2026-09-17) ─────────────────────────────────────────────────────────
 * `flicker` and `dark` are the batch, and they are TWO lines because the count alone misreported
 * this work to its own author. Taking the pending receipt out of `useStationHeld` turned eight
 * 550 ms blanks into twenty 100 ms ones on a 100 ms polled link: the count read two and a half
 * times WORSE while the darkness halved. Whatever the operator suffers is the time, so `darkMs`
 * is the line that matters and the count rides beside it to catch a change that trades one for
 * the other. Same twin run, old predicate against new: **8 events / 4.8 s dark → 1 / 0.4 s**
 * (v4 100 ms), **6 / 8.7 s → 0 / 0** (v4 400 ms), **12 / 3.2 s → 4 / 0.2 s** (v5 100 ms),
 * **8 / 4.4 s → 0 / 0** (v5 400 ms). On the slow link the controls had been dark for 64% of the
 * run. What is LEFT is the four-per-second request budget with no command of ours out — the
 * rate-limit entries outliving the command that filled them — and it only appears where the
 * operator drives faster than the budget, which is why both 400 ms lines are zero.
 *
 * Budget lines met, per link: 1 of 6 before, 3 of 6 after — screen and steps joined band. */
const BASELINE = {
  'v4/100': { screen: 50, tuneConfirmed: 1700, commandConfirmed: 650, bandConfirmed: 650, readout: 1650, stepsSent: 10, flicker: 1, dark: 450 },
  'v4/400': { screen: 50, tuneConfirmed: 3250, commandConfirmed: 1600, bandConfirmed: 1550, readout: 2750, stepsSent: 10, flicker: 0, dark: 0 },
  'v5/100': { screen: 50, tuneConfirmed: 900, commandConfirmed: 350, bandConfirmed: 350, readout: 1100, stepsSent: 10, flicker: 4, dark: 250 },
  'v5/400': { screen: 50, tuneConfirmed: 1700, commandConfirmed: 650, bandConfirmed: 650, readout: 2200, stepsSent: 10, flicker: 0, dark: 0 }
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
    // Not one of them left hanging: every step that went out came back with an outcome.
    expect(r.tune.unconfirmed).toBe(0)
    expect(r.tune.screen!.worst).toBeLessThanOrEqual(line.screen)
    expect(r.tune.confirmed!.worst).toBeLessThanOrEqual(line.tuneConfirmed)
    // THE GESTURE-COUNT-FREE CLOCK — what one command costs on this link, whoever rode on it. The
    // line above moves whenever the number of gestures a run serves changes; this one does not, so
    // it is the one that can say a COMMAND got slower. See the baseline's note.
    const perCommand = r.interactions.filter(i => i.kind === 'tune' && i.confirmed !== undefined).map(i => i.confirmed! - i.sent!)
    expect(Math.max(...perCommand)).toBeLessThanOrEqual(line.commandConfirmed)
    expect(r.band.confirmed!.worst).toBeLessThanOrEqual(line.bandConfirmed)
    expect(r.tune.readout!.worst).toBeLessThanOrEqual(line.readout)
    // BATCH 1'S OWN CLOCK. Every other line here is a duration; this one counts how many times the
    // station controls went off under the operator while they were using them, and the ONLY number
    // that can be right is zero (operator ruling 2026-09-16, "a knob on a radio does not stop
    // existing after you turn it"). It reads as a ratchet like the rest so a later change that
    // reintroduces a blanking term has to move this line to land.
    console.info(`FLICKER v${version}/${rttMs}: ${r.flicker.run} events, ${r.flicker.darkMs} ms dark of ${r.durationMs} ms`)
    expect({ during: r.flicker.run, idle: r.flicker.idle }).toEqual({ during: line.flicker, idle: 0 })
    expect(r.flicker.darkMs).toBeLessThanOrEqual(line.dark)
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
  it('loses no wheel step to the confirmation the poll used to take — the gesture count, which is what caught the receipt bug', async () => {
    // Before the receipt was cleared under the lock, a v5 page accepted FEWER steps than a v4
    // page (3 vs 4 at 100 ms, 2 vs 3 at 400 ms): the event resolved the control but its receipt
    // survived it, so the wheel was refused until the fallback poll cleared it a second on. A
    // faster path losing a step is never noise.
    const before = (await measure(rttMs, 4)).report, after = (await measure(rttMs, 5)).report
    expect(after.tune.sent).toBeGreaterThanOrEqual(before.tune.sent)
    expect(after.band.sent).toBe(before.band.sent)
  })
})

// THE RECEIPT AND THE PUSHED OUTCOME. `pendingControlStorage.write` refuses outside `exclusive()`,
// and the browser's lock (`browserReceiptLock`, `ifAvailable`) refuses rather than queues when it is
// held. The polled path always clears the receipt inside the lock; the event path had cleared it
// bare, the refusal swallowed, and the receipt outlived every pushed outcome by a second — the
// wheel refused, the fallback poll still sent, and, when the event landed during that poll's
// flight, the poll's reply failed the crossed-reply check and closed the socket. Every case here
// went red against that code before `receiveEvent` cleared the receipt under the lock.
describe('a pushed outcome and the pending-control receipt', () => {
  const capturing = (station: Station) => {
    const thrown: string[] = []
    const receive = station.operations.receive.bind(station.operations)
    station.operations.receive = (raw: unknown, bytes?: number) => { try { receive(raw, bytes) } catch (error) { thrown.push((error as Error).message) } }
    return thrown
  }
  it('clears the receipt at once, so nothing is pending a round trip and a CAT time after the send, and no fallback poll goes out', async () => {
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
  it('landing while the fallback poll is in flight, it leaves the clear to the poll, whose reply is then not a crossed one', async () => {
    // A radio slow enough that the event lands during the poll's round trip: a band change with a
    // mode-before-dial window. The poll holds the lock, the event's clear is refused, and the
    // poll's own terminal reply clears the receipt it still finds — instead of `invalidOperation`.
    const station = scriptedStation(100, 5, { catMs: 1050 })
    open.push(station)
    const thrown = capturing(station)
    await vi.advanceTimersByTimeAsync(1600)
    let outcome = ''
    void station.operations.control({ action: 'radio.band', band: '17m', mode: 'phone' }).then(r => { outcome = r.outcome }, e => { outcome = `refused:${e.message}` })
    await vi.advanceTimersByTimeAsync(3000)
    expect(station.wire.filter(w => w.type === 'result').length, 'the fallback poll went out').toBeGreaterThan(0)
    expect(thrown).toEqual([])
    expect(outcome).toBe('applied')
    expect(station.operations.getSnapshot().controlPending).toBeNull()
  })
  it('outrunning the control\'s own reply, it leaves the clear to that reply, which holds the lock', async () => {
    const station = scriptedStation(100, 5, { replyDelayMs: 400 })
    open.push(station)
    const thrown = capturing(station)
    await vi.advanceTimersByTimeAsync(1600)
    let outcome = ''
    void station.operations.control({ action: 'radio.band', band: '17m', mode: 'phone' }).then(r => { outcome = r.outcome }, e => { outcome = `refused:${e.message}` })
    // The event (a round trip and a CAT time) lands before the reply (a round trip and the delay).
    await vi.advanceTimersByTimeAsync(100 + SCRIPTED_CAT_MS + 20)
    expect(station.operations.getSnapshot().controlResult?.outcome, 'the event has been read').toBe('applied')
    expect(station.operations.getSnapshot().controlPending, 'the receipt waits for the lock holder').not.toBeNull()
    await vi.advanceTimersByTimeAsync(400)
    expect(outcome).toBe('applied')
    expect(station.operations.getSnapshot().controlPending, 'the reply cleared it').toBeNull()
    await vi.advanceTimersByTimeAsync(1500)
    expect(station.wire.filter(w => w.type === 'result')).toHaveLength(0)
    expect(thrown).toEqual([])
  })
  it('control: an UNKNOWN outcome keeps the receipt for the operator to acknowledge', async () => {
    const station = scriptedStation(100, 5, { eventOutcome: 'unknown' })
    open.push(station)
    await vi.advanceTimersByTimeAsync(1600)
    let outcome = ''
    void station.operations.control({ action: 'radio.band', band: '17m', mode: 'phone' }).then(r => { outcome = r.outcome }, e => { outcome = `refused:${e.message}` })
    await vi.advanceTimersByTimeAsync(3000)
    expect(outcome).toBe('unknown')
    expect(station.operations.getSnapshot().controlPending).not.toBeNull()
  })
  it('control: a genuinely crossed reply — a control\'s result read answered as a log outcome — is still refused', async () => {
    // The strictness the clear-under-lock preserves: nothing was widened at the crossed-reply check.
    const station = scriptedStation(100, 5, { push: false, crossPoll: true })
    open.push(station)
    const thrown = capturing(station)
    await vi.advanceTimersByTimeAsync(1600)
    void station.operations.control({ action: 'radio.band', band: '17m', mode: 'phone' }).catch(() => {})
    await vi.advanceTimersByTimeAsync(3000)
    expect(station.wire.filter(w => w.type === 'result').length).toBeGreaterThan(0)
    expect(thrown).toContain('invalidOperation')
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
