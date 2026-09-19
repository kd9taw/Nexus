import type { AppSnapshot } from '../types'
import type { ApplicationClient } from './application-client'
import type { OperationClient } from './operation-client'
import type { WheelTuning } from './wheel-tuning'
import type { SatelliteLive } from './navigation'
import { readBandChoices } from './band-choices'
import { t } from '../i18n'

/** THE MEASURING STICK for Remote responsiveness: a number the operator can produce again on their
 * own link, so "it feels laggy" and "it feels better" are both readings and not impressions.
 *
 * Two clocks, always both, for every interaction:
 *   PERCEIVED  — gesture → the screen visibly responds. In the browser that is the frame after the
 *                first change the interaction's own path puts on the render path (`WheelTuning`
 *                notifies its subscribers, `OperationClient` updates its view); under test, with no
 *                frame to wait for, it is that change itself.
 *   CONFIRMED  — gesture → the station's own reading shows the radio did it (a terminal control
 *                outcome; for Stop, the station's acceptance), and then READOUT — the snapshot the
 *                screen paints from agrees with what was asked (the dial equals the target, the band
 *                label matches, the transmitter reads free).
 * Plus the round trip (request sent → first reply) and a FLICKER count: every time held station
 * control went away by itself (the "buttons on, disappeared, came back" of the operator's report),
 * every refused snapshot read, and every gap in readings long enough to grey the workspace.
 *
 * ⚠️ THE PROBE MUST NOT CHANGE WHAT IT MEASURES. Every hook it installs in `OperationClient`,
 * `WheelTuning` and `ApplicationStreamClient` is a null check while nothing is attached, and while
 * attached it only stamps a clock and pushes to a bounded array on objects nothing renders from: no
 * `update()`, no `notify()`, no state a component subscribes to, no timer, and no work on the wire.
 * The one asynchronous thing it does — `requestAnimationFrame` to read the frame after a response —
 * is scheduled only while a check is running and only observes. Attached at all only while the
 * Responsiveness panel is on screen; an operator who never opens it pays nothing. */
export type InteractionKind = 'tune' | 'band' | 'stop'
export type Interaction = {
  kind: InteractionKind
  /** The gesture, in probe time. Every other stamp is later than this one or absent. */
  made: number
  /** The interaction's own path first changed something on the render path. */
  responded?: number
  /** The frame after `responded` (browser); `responded` itself where there is no frame to wait for. */
  perceived?: number
  /** The request left the browser. Absent: nothing was sent (refused, coalesced away, or cancelled). */
  sent?: number
  /** The first reply to that request — the link's round trip. */
  replied?: number
  /** The terminal outcome landed (applied / rejected / an error reply), or the Stop was accepted. */
  confirmed?: number
  outcome?: string
  /** How the confirmation reached the browser: the station's own push (operation v5) or the
   * browser's poll. Kept apart because "did it get faster" is a question about the push. */
  via?: ConfirmationPath
  /** The first station reading after `sent` that shows the change. */
  readout?: number
  /** Refused at the gesture: nothing was queued, nothing can follow. */
  refused: boolean
  target?: { dialHz?: number; band?: string }
  requestId?: string
}
export type ConfirmationPath = 'pushed' | 'polled'
type FlickerKind = 'controlsOff' | 'readingRefused' | 'readingStale'
type Lane = 'tuning' | 'operations'
const LANE: Record<InteractionKind, Lane> = { tune: 'tuning', band: 'operations', stop: 'operations' }
/** Readings this far apart make the workspace refuse and grey (`APPLICATION_TIMEOUT_MS`). */
const READING_GAP_MS = 3000
/** How far back the idle flicker window reaches, and how long events are kept at all. */
export const IDLE_WINDOW_MS = 60_000
const KEEP_MS = 120_000
export type ProbeHosts = { operations: OperationClient; tuning: WheelTuning; application: ApplicationClient }

export class ResponsivenessProbe {
  private interactions: Interaction[] = []
  private awaitingResponse: Interaction[] = []
  private awaitingSend: Interaction[] = []
  private awaitingStop: Interaction | null = null
  private byRequest = new Map<string, Interaction[]>()
  private flicker: { at: number; kind: FlickerKind; back?: number }[] = []
  private held: boolean | null = null
  private lastReading: number | null = null
  private hosts: ProbeHosts | null = null
  private unsubscribe: (() => void) | null = null
  private running = false
  private runStart = 0
  private runEnd = 0
  /** `frame` reads the next frame after a response — `requestAnimationFrame` in the browser; omit
   * where there is none, and `perceived` is the response itself. */
  constructor(readonly now: () => number = () => performance.now(), private frame?: (cb: (at: number) => void) => void) {}
  /** Install the hooks and start counting flicker. Idempotent per host set. */
  attach(hosts: ProbeHosts) {
    this.detach()
    this.hosts = hosts
    this.attachedAt = this.now()
    hosts.operations.probe = this; hosts.tuning.probe = this; hosts.application.attachProbe(this)
    // The flicker predicate is `useStationHeld`'s, computed here from the same view so no hook and
    // no component is touched: held control, gone by itself, is one flicker. It has to be kept in
    // step with that hook by hand — a term dropped there and left here would report a flicker the
    // operator no longer sees, and the reverse would hide one they do. `controlPending` left both
    // together (operator ruling 2026-09-16, a command in flight is not a loss of control).
    const watch = () => {
      const v = hosts.operations.getSnapshot()
      const unknown = v.controlResult?.outcome === 'unknown' && !!v.controlPending
      const held = !!(v.connected && !unknown && (v.requestReady !== false || v.controlPending) && !v.unresolved && v.state?.phase === 'controlling')
      if (this.held === true && !held) this.event('controlsOff')
      if (this.held === false && held) this.heldAgain(this.now())
      this.held = held
    }
    this.unsubscribe = hosts.operations.subscribe(watch)
    watch()
  }
  detach() {
    this.unsubscribe?.(); this.unsubscribe = null
    const hosts = this.hosts
    this.hosts = null
    if (!hosts) return
    if (hosts.operations.probe === this) hosts.operations.probe = null
    if (hosts.tuning.probe === this) hosts.tuning.probe = null
    hosts.application.attachProbe(null)
  }
  attached() { return this.hosts !== null }
  beginRun() {
    this.interactions = []; this.awaitingResponse = []; this.awaitingSend = []; this.awaitingStop = null; this.byRequest.clear()
    this.running = true; this.runStart = this.now(); this.runEnd = 0
  }
  endRun() { this.running = false; this.runEnd = this.now() }
  // ---- hooks: each stamps and returns; none may notify, update or send ----
  /** A gesture. Null outside a run, so a wheel step the operator makes while the panel is merely
   * open records nothing and costs nothing beyond this call. */
  gesture(kind: InteractionKind, target?: Interaction['target']): Interaction | null {
    if (!this.running) return null
    const i: Interaction = { kind, made: this.now(), refused: false, target }
    this.interactions.push(i)
    return i
  }
  refused(i: Interaction | null) { if (i) i.refused = true }
  /** Handed to the transport: the next response on its lane, and the next send on its lane, are its. */
  dispatch(list: Interaction[], target?: Interaction['target']) {
    for (const i of list) {
      if (target) i.target = target
      if (i.responded === undefined) this.awaitingResponse.push(i)
      if (i.sent === undefined) this.awaitingSend.push(i)
    }
  }
  dispatchStop(i: Interaction | null) {
    if (!i) return
    this.awaitingResponse.push(i)
    this.awaitingStop = i
  }
  /** `lane` put something on the render path. */
  responded(lane: Lane) {
    if (!this.awaitingResponse.length) return
    const mine = this.awaitingResponse.filter(i => LANE[i.kind] === lane)
    if (!mine.length) return
    this.awaitingResponse = this.awaitingResponse.filter(i => LANE[i.kind] !== lane)
    this.stamp(mine)
  }
  /** THE OPTIMISTIC DIAL: this gesture's own path has ALREADY changed what is on screen — the
   * digits moved on the step itself, before its burst was coalesced or anything was sent. That is
   * PERCEIVED exactly as defined above (the first change the interaction's own path puts on the
   * render path), and it has to be stamped here because such a step is not yet DISPATCHED, so
   * `responded` — which works off the dispatched list — would not see it and would instead report
   * the flush, up to a whole command later. Stamped once: `dispatch` never re-stamps. */
  shown(i: Interaction | null) {
    if (i && i.responded === undefined) this.stamp([i])
  }
  private stamp(list: Interaction[]) {
    const at = this.now()
    for (const i of list) i.responded = at
    if (this.frame) this.frame(frameAt => { for (const i of list) if (i.perceived === undefined) i.perceived = frameAt })
    else for (const i of list) i.perceived = at
  }
  sent(requestId: string) {
    if (!this.awaitingSend.length) return
    const at = this.now(), mine = this.awaitingSend
    this.awaitingSend = []
    for (const i of mine) { i.sent = at; i.requestId = requestId }
    this.byRequest.set(requestId, mine)
  }
  sentStop(requestId: string) {
    const i = this.awaitingStop
    if (!i) return
    this.awaitingStop = null
    i.sent = this.now(); i.requestId = requestId
    this.byRequest.set(requestId, [i])
  }
  replied(requestId: string) {
    const mine = this.byRequest.get(requestId)
    if (!mine) return
    const at = this.now()
    for (const i of mine) if (i.replied === undefined) i.replied = at
  }
  /** A terminal outcome for `requestId`. `via` says which path carried it: the station's pushed
   * event, or a reply to something the browser asked (the control's own reply, or a result poll).
   * Only the first to arrive counts — a pushed outcome and its fallback poll are one confirmation. */
  confirmed(requestId: string, outcome: string, via: ConfirmationPath = 'polled') {
    const mine = this.byRequest.get(requestId)
    if (!mine) return
    const at = this.now()
    for (const i of mine) if (i.confirmed === undefined) { i.confirmed = at; i.outcome = outcome; i.via = via }
  }
  /** A station snapshot arrived. Every sent interaction still waiting for its readout is checked
   * against it; a gap since the previous reading long enough to grey the workspace is a flicker. */
  reading(snapshot: unknown) {
    const at = this.now()
    if (this.lastReading !== null && at - this.lastReading >= READING_GAP_MS) this.event('readingStale', this.lastReading + READING_GAP_MS)
    this.lastReading = at
    if (!this.running) return
    const radio = (snapshot as Partial<AppSnapshot> | null)?.radio
    if (!radio) return
    for (const i of this.interactions) {
      if (i.sent === undefined || i.readout !== undefined) continue
      const shows = i.kind === 'tune' ? Math.round(radio.dialMhz * 1e6) === i.target?.dialHz
        : i.kind === 'band' ? radio.band === i.target?.band
        : !radio.transmitting && !radio.tuning && !radio.rigKeyed && !radio.txEnabled
      if (shows) i.readout = at
    }
  }
  readingRefused() { this.event('readingRefused') }
  /** Close the open controls-off event, so the report can say how LONG the controls were dark and
   * not merely how often they blinked. The count alone is not a measure of what the operator
   * suffers and it misreported batch 1 to its own author: taking the pending receipt out of the
   * predicate turned eight 550 ms blanks into twenty 100 ms ones on a 100 ms link, which the count
   * read as two and a half times worse and the clock reads as less than half the darkness. */
  private heldAgain(at: number) {
    for (let i = this.flicker.length - 1; i >= 0; i--) {
      if (this.flicker[i].kind !== 'controlsOff') continue
      if (this.flicker[i].back === undefined) this.flicker[i].back = at
      return
    }
  }
  private event(kind: FlickerKind, at = this.now()) {
    this.flicker.push({ at, kind })
    const keep = at - KEEP_MS
    if (this.flicker.length > 64 && this.flicker[0].at < keep) this.flicker = this.flicker.filter(e => e.at >= keep)
  }
  // ---- the report ----
  report(): Measured {
    const run = this.interactions, end = this.runEnd || this.now()
    const during = this.flicker.filter(e => e.at >= this.runStart && e.at <= end).length
    // Still dark when the run ended counts to the end of the run, never past it.
    const darkMs = this.flicker.filter(e => e.kind === 'controlsOff' && e.at >= this.runStart && e.at <= end)
      .reduce((total, e) => total + Math.max(0, Math.min(e.back ?? end, end) - e.at), 0)
    const idleFrom = Math.max(this.runStart - IDLE_WINDOW_MS, this.attachedAt), idle = this.flicker.filter(e => e.at >= idleFrom && e.at < this.runStart).length
    const of = (kind: InteractionKind) => run.filter(i => i.kind === kind)
    const rtt = run.filter(i => i.sent !== undefined && i.replied !== undefined).map(i => i.replied! - i.sent!)
    // Steps coalesced into one command share its stamps, so each reads its own latency from its own gesture.
    const summary = (list: Interaction[]) => ({
      made: list.length,
      sent: list.filter(i => i.sent !== undefined).length,
      refused: list.filter(i => i.refused).length,
      unconfirmed: list.filter(i => i.sent !== undefined && i.confirmed === undefined).length,
      pushed: list.filter(i => i.confirmed !== undefined && i.via === 'pushed').length,
      polled: list.filter(i => i.confirmed !== undefined && i.via === 'polled').length,
      screen: clock(list.filter(i => i.perceived !== undefined).map(i => i.perceived! - i.made)),
      confirmed: clock(list.filter(i => i.confirmed !== undefined).map(i => i.confirmed! - i.made)),
      // From the gesture, like the others: it is what the operator sees. Today the readout lands
      // BEFORE the polled confirmation, so a trail measured from the confirmation would read zero
      // and say nothing; the budget line puts the readout within 300 ms of where the confirmation
      // itself is allowed to be, which is the plan's "trail ≤ 300 ms" once confirmations are pushed.
      readout: clock(list.filter(i => i.readout !== undefined).map(i => i.readout! - i.made))
    })
    const stop = of('stop')[0]
    return {
      rtt: clock(rtt),
      tune: summary(of('tune')),
      band: summary(of('band')),
      stop: stop ? { screen: since(stop, stop.perceived), sent: since(stop, stop.sent), accepted: since(stop, stop.confirmed), readout: since(stop, stop.readout) } : null,
      flicker: { run: during, idle, idleMs: Math.max(0, this.runStart - idleFrom), darkMs },
      interactions: run.map(i => ({ ...i })),
      durationMs: end - this.runStart
    }
  }
  /** When the flicker watch began, so the idle window never claims time it did not see. */
  private attachedAt = -Infinity
}
const since = (i: Interaction, at: number | undefined) => at === undefined ? null : at - i.made
export type Clock = { typical: number; worst: number; count: number } | null
function clock(xs: number[]): Clock {
  if (!xs.length) return null
  const s = [...xs].sort((a, b) => a - b), at = (p: number) => s[Math.min(s.length - 1, Math.max(0, Math.ceil(p * s.length) - 1))]
  return { typical: at(0.5), worst: at(0.95), count: s.length }
}
type Summary = { made: number; sent: number; refused: number; unconfirmed: number; pushed: number; polled: number; screen: Clock; confirmed: Clock; readout: Clock }
export type Measured = {
  rtt: Clock
  tune: Summary
  band: Summary
  stop: { screen: number | null; sent: number | null; accepted: number | null; readout: number | null } | null
  flicker: { run: number; idle: number; idleMs: number; darkMs: number }
  interactions: Interaction[]
  durationMs: number
}

// ---- the check: one fixed script, the same in the browser and under test ----
export const CHECK_STEPS = 10
export const CHECK_STEP_HZ = 10
export const CHECK_STEP_MS = 300
/** The budget (the programme's §1, in the plan's own numbers): perceived within a frame or two;
 * confirmed within the link's round trip plus the radio's own time; the readout no more than
 * `readoutMs` behind where the confirmation is allowed to be; nothing you could press going away by
 * itself; no wheel step lost. */
export const BUDGET = { screenMs: 50, tuneAfterRttMs: 300, bandAfterRttMs: 1500, readoutMs: 300 }
export type CheckRefusal = 'needControl' | 'needIdle' | 'unsupported'
export type CheckSkip = 'mode' | 'digital' | 'noBand' | 'unsupported' | 'stopUnavailable' | 'satellite'
export type CheckPhase = 'tuning' | 'band' | 'stop' | 'settling'
export type CheckReport = Measured & {
  refused: CheckRefusal | null
  at: number
  skipped: { tune: CheckSkip | null; band: CheckSkip | null; stop: CheckSkip | null }
  /** Per line of the budget: true = within it, false = outside it, null = not measured. */
  budget: { screen: boolean | null; tune: boolean | null; band: boolean | null; readout: boolean | null; flicker: boolean; steps: boolean | null }
}
export type CheckDeps = ProbeHosts & { probe: ResponsivenessProbe; wait: (ms: number) => Promise<void> }
const WHEEL_SIDEBANDS = ['USB', 'LSB', 'AM', 'FM']

export async function runResponsivenessCheck(deps: CheckDeps, progress?: (phase: CheckPhase) => void): Promise<CheckReport> {
  const { operations, tuning, application, probe, wait } = deps
  const empty = probe.report()
  const refusal = (refused: CheckRefusal): CheckReport => ({ ...empty, refused, at: Date.now(), skipped: { tune: null, band: null, stop: null },
    budget: { screen: null, tune: null, band: null, readout: null, flicker: true, steps: null } })
  const view = operations.getSnapshot(), state = view.state
  if (!view.connected || state?.phase !== 'controlling' || !state.controls) return refusal('needControl')
  if (operations.operationVersion < 3 || !state.controls.capabilities.includes('frequency')) return refusal('unsupported')
  let snapshot: AppSnapshot
  try { snapshot = await application.invoke<AppSnapshot>('get_snapshot') } catch { return refusal('needIdle') }
  const radio = snapshot.radio
  if (radio.txEnabled || radio.transmitting || radio.rigKeyed || radio.tuning || radio.txBusyReason || radio.catOk !== true) return refusal('needIdle')
  const startBand = radio.band, sideband = radio.sideband || 'USB', mode = radio.operatingMode
  const skipped: CheckReport['skipped'] = { tune: null, band: null, stop: null }
  const context = () => operations.getSnapshot().state?.controls?.context ?? null
  const controlReady = () => { const v = operations.getSnapshot(); return v.fresh && v.state?.phase === 'controlling' && !v.controlPending && !tuning.getPending() }
  // The script sequences on the CLIENTS' own state, never on the probe's stamps: the probe only
  // watches, so a check runs the same with it attached or not (the test proves that).
  const settle = async (ready: () => boolean | Promise<boolean>, maxMs: number) => {
    const until = probe.now() + maxMs
    while (!(await ready()) && probe.now() < until) await wait(50)
  }
  const reading = async (shows: (radio: AppSnapshot['radio']) => boolean) => {
    try { return shows((await application.invoke<AppSnapshot>('get_snapshot')).radio) } catch { return false }
  }
  probe.beginRun()
  // 1. Ten wheel steps, as the readout digits send them: five up, five down, so the dial ends where it began.
  if (!WHEEL_SIDEBANDS.includes(sideband)) skipped.tune = 'mode'
  else {
    progress?.('tuning')
    for (let step = 0; step < CHECK_STEPS; step++) {
      let dialMhz = radio.dialMhz
      try { dialMhz = (await application.invoke<AppSnapshot>('get_snapshot')).radio.dialMhz } catch {}
      tuning.nudge(step < CHECK_STEPS / 2 ? CHECK_STEP_HZ : -CHECK_STEP_HZ, { dialMhz, sideband, context: context() })
      await wait(CHECK_STEP_MS)
    }
    progress?.('settling')
    await settle(controlReady, 8000)
    // One more station reading, so the last step's readout has had its chance to land.
    await wait(700)
  }
  // 2. One band change to the next licensed band, and one back.
  if (mode !== 'cw' && mode !== 'phone') skipped.band = 'digital'
  else if (!operations.getSnapshot().state?.controls?.capabilities.includes('bandSelection')) skipped.band = 'unsupported'
  else {
    let choices: { band: string }[] = []
    try { choices = readBandChoices(await application.invoke<unknown>('get_settings'), mode) } catch {}
    const index = choices.findIndex(c => c.band === startBand)
    const other = index < 0 ? null : choices[index + 1] ?? choices[index - 1] ?? null
    if (!other) skipped.band = 'noBand'
    else {
      progress?.('band')
      for (const band of [other.band, startBand]) {
        await settle(controlReady, 5000)
        const i = probe.gesture('band', { band })
        if (i) probe.dispatch([i])
        let applied = false
        try { applied = (await operations.control({ action: 'radio.band', band, mode })).outcome === 'applied' } catch {}
        if (applied) await settle(() => reading(radio => radio.band === band), 3000)
      }
    }
  }
  // 3. One Stop while idle. Stop is the safe direction — an unkey — but at the station it also ends
  // a satellite track, so it is only sent when the station can say there is none.
  await settle(controlReady, 5000)
  if (!operations.getSnapshot().stopAvailable) skipped.stop = 'stopUnavailable'
  else {
    let tracking = true
    if (application.supports('get_remote_satellite_state')) {
      try { tracking = (await application.invoke<SatelliteLive>('get_remote_satellite_state')).track !== null } catch {}
    }
    if (tracking) skipped.stop = 'satellite'
    else {
      progress?.('stop')
      probe.dispatchStop(probe.gesture('stop'))
      let accepted = false
      try { await operations.stopTransmit(); accepted = true } catch {}
      // The transmitter was already free; the next station reading is the one that says so.
      if (accepted) await wait(700)
    }
  }
  probe.endRun()
  const m = probe.report()
  const rtt = m.rtt?.worst ?? null
  return { ...m, refused: null, at: Date.now(), skipped, budget: {
    screen: m.tune.screen ? m.tune.screen.worst <= BUDGET.screenMs : null,
    tune: m.tune.confirmed && rtt !== null ? m.tune.confirmed.worst <= rtt + BUDGET.tuneAfterRttMs : null,
    band: m.band.confirmed && rtt !== null ? m.band.confirmed.worst <= rtt + BUDGET.bandAfterRttMs : null,
    readout: m.tune.readout && rtt !== null ? m.tune.readout.worst <= rtt + BUDGET.tuneAfterRttMs + BUDGET.readoutMs : null,
    flicker: m.flicker.run === 0 && m.flicker.idle === 0,
    steps: skipped.tune ? null : m.tune.sent === m.tune.made && m.tune.unconfirmed === 0
  } }
}

// ---- the operator's copy ----
/** Invariant units: a duration never goes through a locale formatter (see i18n/index.ts). */
export function duration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return '—'
  return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`
}
const refusedText = (r: CheckRefusal) => r === 'needControl' ? t('remote.responsiveness.refused.needControl')
  : r === 'needIdle' ? t('remote.responsiveness.refused.needIdle') : t('remote.responsiveness.refused.unsupported')
const skipText = (s: CheckSkip) => s === 'mode' ? t('remote.responsiveness.skip.mode') : s === 'digital' ? t('remote.responsiveness.skip.digital')
  : s === 'noBand' ? t('remote.responsiveness.skip.noBand') : s === 'unsupported' ? t('remote.responsiveness.skip.unsupported')
  : s === 'satellite' ? t('remote.responsiveness.skip.satellite') : t('remote.responsiveness.skip.stopUnavailable')
export function reportText(r: CheckReport): string {
  const when = new Date(r.at).toISOString().replace('T', ' ').slice(0, 16) + ' UTC'
  const lines = [t('remote.responsiveness.report.title', { when })]
  if (r.refused) { lines.push(refusedText(r.refused)); return lines.join('\n') }
  const rtt = r.rtt?.worst ?? null
  const clockLine = (key: 'screen' | 'confirmed' | 'readout', c: Clock, target: number | null) => {
    if (!c) return t('remote.responsiveness.report.none')
    const values = { typical: duration(c.typical), worst: duration(c.worst), target: duration(target) }
    return key === 'screen' ? t('remote.responsiveness.report.screen', values)
      : key === 'confirmed' ? t('remote.responsiveness.report.confirmed', values) : t('remote.responsiveness.report.readout', values)
  }
  lines.push(r.rtt ? t('remote.responsiveness.report.link', { typical: duration(r.rtt.typical), worst: duration(r.rtt.worst) }) : t('remote.responsiveness.report.linkUnknown'))
  if (r.skipped.tune) lines.push(t('remote.responsiveness.report.tuningSkipped', { reason: skipText(r.skipped.tune) }))
  else {
    lines.push(t('remote.responsiveness.report.tuning', { made: r.tune.made, sent: r.tune.sent, refused: r.tune.made - r.tune.sent }))
    lines.push(clockLine('screen', r.tune.screen, BUDGET.screenMs), clockLine('confirmed', r.tune.confirmed, rtt === null ? null : rtt + BUDGET.tuneAfterRttMs),
      clockLine('readout', r.tune.readout, rtt === null ? null : rtt + BUDGET.tuneAfterRttMs + BUDGET.readoutMs))
    if (r.tune.confirmed) lines.push(t('remote.responsiveness.report.via', { pushed: r.tune.pushed, polled: r.tune.polled }))
    if (r.tune.unconfirmed) lines.push(t('remote.responsiveness.report.unconfirmed', { count: r.tune.unconfirmed }))
  }
  if (r.skipped.band) lines.push(t('remote.responsiveness.report.bandSkipped', { reason: skipText(r.skipped.band) }))
  else {
    lines.push(t('remote.responsiveness.report.band', { made: r.band.made }))
    lines.push(clockLine('screen', r.band.screen, BUDGET.screenMs), clockLine('confirmed', r.band.confirmed, rtt === null ? null : rtt + BUDGET.bandAfterRttMs),
      clockLine('readout', r.band.readout, rtt === null ? null : rtt + BUDGET.bandAfterRttMs + BUDGET.readoutMs))
    if (r.band.confirmed) lines.push(t('remote.responsiveness.report.via', { pushed: r.band.pushed, polled: r.band.polled }))
  }
  if (r.skipped.stop || !r.stop) lines.push(t('remote.responsiveness.report.stopSkipped', { reason: skipText(r.skipped.stop ?? 'stopUnavailable') }))
  else lines.push(t('remote.responsiveness.report.stop', { screen: duration(r.stop.screen), sent: duration(r.stop.sent), accepted: duration(r.stop.accepted), readout: duration(r.stop.readout) }))
  lines.push(t('remote.responsiveness.report.flicker', { run: r.flicker.run, idle: r.flicker.idle, seconds: Math.round(r.flicker.idleMs / 1000) }))
  const checks = Object.values(r.budget).filter(v => v !== null)
  lines.push(t('remote.responsiveness.report.verdict', { passed: checks.filter(Boolean).length, total: checks.length }))
  return lines.join('\n')
}
