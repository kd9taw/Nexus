import type { AppSnapshot } from '../types'
import { bandLabelForMhz } from '../band'
import { clampWheelTarget, stepFrom } from '../wheelTuningPolicy'
import type { ApplicationClient } from './application-client'
import { OperationFailure, type OperationClient } from './operation-client'
import type { StationAction, ControlOutcome, ControlContext } from './station-operation'
import type { Interaction, ResponsivenessProbe } from './responsiveness'

export type WheelSource = { dialMhz: number; sideband: string; owner?: object; context?: ControlContext | null }
type Burst = WheelSource & {
  fromHz: number
  targetHz: number
  radioId: number
  authority: string
  edgeSaid: boolean
  owners: Set<object>
  /** A scope press's sender, prepared at pointer-down so the release cannot borrow a renewed command
   * window. A wheel or digit burst has none: it prepares one when it is sent, from current control. */
  send?: (action: StationAction) => Promise<ControlOutcome>
  /** Waiting out a brief control lapse. Later steps still join the burst; nothing is sent yet. */
  held?: boolean
  /** The responsiveness probe's record of each step in this burst; allocated only while it measures. */
  probes?: Interaction[]
  /** Built on the dial a command already in flight is asking for, not on a station reading — this
   * is the burst QUEUED behind that command, sent the moment it confirms. Refused whole if it does
   * not: nothing else knows where the radio is. */
  queued?: boolean
}
/** THE BACKSTOP on how long a value the operator can see may stand without the station's own
 * reading having shown it. An answer reverts it first — a rejected or unknown outcome puts the
 * digits back at once, said by that failure — so what this clock catches is the case with no answer
 * at all: a lost command, a station gone quiet. Then the digits go back to the station's dial and
 * the operator is told it was not confirmed, because a provisional value is never left standing.
 * A CONFIRMED one is dropped on the same clock and silently: the station's sample (500 ms) has long
 * carried it by then, so the digits do not move. */
const PROVISIONAL_MS = 2500
/** The wheel's coalescing window, and with it the floor on how often a burst may put a CAT write on
 * the rig's port. Pipelining must not lift that floor: on a link whose round trip is shorter than
 * this — a station on the same LAN — a queued burst would otherwise be sent the instant the last
 * one confirmed, at twice the rate the wheel has ever written at. On any real remote link the round
 * trip is already longer than this, so it costs nothing there. */
const BURST_MS = 120

/** One short input burst per browser station, shared by readout digits and
 * scope wheels. The target is ephemeral: submission consumes it, and no input
 * queues behind a radio command or survives authority loss/reconnect.
 *
 * A brief control lapse (a slow heartbeat round trip leaves control stale for a moment every
 * second) keeps a wheel or digit burst, as it keeps a drag (operator decision 2026-09-14): steps
 * made during it are accepted while the latest station state still shows this browser
 * controlling, and the burst is sent as ONE command once control is current again, waiting at most
 * CONTROL_RESUME_MS, or refused as a whole with "Not sent". Nothing is sent on stale control, the
 * dial is re-read before sending (a dial that moved refuses the burst; no old target is replayed),
 * and a scope press still needs current control from press to release.
 *
 * ONE WRITER ON ONE DIAL: the readout digits, the scope wheel and the tuning strip's ◄/► arrows
 * all step the same burst (`nudge`, `nudgeSteps`, `captureTarget`), so two of them can never build
 * on two different dials or put two commands on the wire for one gesture.
 *
 * ONE IN FLIGHT, ONE QUEUED. A step made while a command is in flight is no longer dropped: it
 * joins a burst built on the dial that command is ASKING for, and that burst is sent the moment the
 * command confirms — so the wheel keeps its steps at any round trip, and the operator's correction
 * lands instead of vanishing. If the command does not confirm, the queued burst is refused whole
 * ("Not sent"): nothing else knows where the radio is.
 *
 * THE PROVISIONAL DIAL IS DISPLAY ONLY (`provisionalHz`). It is the dial this browser has asked for
 * and the station has not read back yet, and it is kept HERE, deliberately out of `AppSnapshot`,
 * `OperationState` and every context value, so no privilege, band, mode, sideband, shading-strip,
 * txArmed, S-meter or TX-enable computation can reach it. The only readers are the dial digits.
 * The station's readback remains the only thing that moves the radio or says that it moved. */
export class WheelTuning {
  private burst: Burst | null = null
  private reading: Burst | null = null
  private timer: ReturnType<typeof setTimeout> | undefined
  private unsubscribe: (() => void) | undefined
  private generation = 0
  private inputEpoch = 0
  private live = false
  private sending = false
  /** The dial the command in flight is asking for, while one is. A step made now is built on it. */
  private inFlight: number | null = null
  /** When the last command left the browser, so a queued burst keeps the BURST_MS write floor. */
  private sentAt = -Infinity
  /** The dial the station last READ BACK for a command of ours, and when that word landed. A station
   * sample taken before it is older news than the station's own readback and cannot be the "the dial
   * moved under us" the pre-dispatch re-read is for; a sample taken after it still refuses. */
  private confirmed: { hz: number; at: number } | null = null
  /** See the class comment: display only, never shared state. */
  private provisional: { hz: number; confirmed: boolean } | null = null
  private settle: ReturnType<typeof setTimeout> | undefined
  private listeners = new Set<() => void>()
  /** The responsiveness probe, attached only while its panel is open (see responsiveness.ts). */
  probe: ResponsivenessProbe | null = null
  constructor(private operations: OperationClient, private application: ApplicationClient, private failed: (error: OperationFailure) => void) {}
  subscribe = (f: () => void) => { this.listeners.add(f); return () => { this.listeners.delete(f) } }
  getPending = () => this.sending
  /** DISPLAY ONLY — the dial this browser asked for, until the station's own reading shows it. See
   * the class comment for why it lives here and nowhere else. */
  getProvisionalHz = () => this.provisional?.hz ?? null
  private notify() { for (const f of this.listeners) f(); this.probe?.responded('tuning') }
  activate() {
    this.live = true
    this.unsubscribe?.()
    let context = this.context()
    this.unsubscribe = this.operations.subscribe(() => {
      const next = this.context()
      // Also invalidate sub-notch input still held by a mounted wheel listener.
      // A disconnect/reopen can occur in one React render with the same lease.
      // A freshness lapse alone keeps held input (see the class comment); losing authority does not.
      // A readback is about the radio it came from, so authority ends it too.
      if (context !== next) this.confirmed = null
      if (!this.inputReady() || context !== next) this.cancel()
      context = next
    })
  }
  dispose() {
    this.live = false; this.generation++; this.cancel(); this.unsubscribe?.(); this.unsubscribe = undefined
    this.sending = false; this.inFlight = null; this.confirmed = null; this.notify()
  }
  /** WHO the input belongs to: this station boot, this lease, this radio and its connections. The
   * command WINDOW (the revision) is deliberately NOT part of it. A burst queued behind a command
   * in flight exists precisely to be sent on the window AFTER that command consumed its own, so
   * folding the revision in here would discard every queued burst the moment the command it waits
   * for is answered. The window is checked where a command is actually built — `prepareControl`
   * captures the state and `executeControl` refuses a spent revision, window or lease — so nothing
   * leaves the browser on a stale window either way. */
  private context(): string {
    const s = this.operations.getSnapshot().state, c = s?.controls?.context
    return JSON.stringify([s?.stationBootId, s?.leaseId, c?.radioId, c?.radioConnection, c?.ampConnection])
  }
  ready(): boolean { return !this.sending && this.authorityReady() }
  inputContext(): string { return JSON.stringify([this.inputEpoch, this.context()]) }
  private authorityReady(): boolean { return this.inputReady() && this.operations.getSnapshot().fresh }
  /** Authority to accept a wheel or digit step: everything authorityReady asks except the freshness
   * window. The send itself still waits for current control.
   *
   * A pending control receipt while THIS controller is sending is that send's own: `flush` only
   * sets `sending` from a state with no receipt at all, and `executeControl` refuses to start a
   * second command while one is pending. So steps made during it join the next burst rather than
   * being lost, which is the whole of "never drop wheel input". Any OTHER pending control still
   * refuses, as does an armed transmitter, at every round trip.
   *
   * The one window where the receipt could be someone else's is between `sending` going true and
   * this burst's own request leaving — another control started from the UI in that gap. It costs
   * nothing: that same receipt makes OUR send throw, the burst fails, and the one queued behind it
   * is refused whole and said. Input can be accepted and then refused here; it can never be sent
   * against a state that is not its own. */
  private inputReady(): boolean {
    const v = this.operations.getSnapshot(), s = v.state
    return !!(this.live && this.operations.operationVersion >= 3 && v.connected &&
      !v.unresolved && (!v.controlPending || this.sending) && !v.submitting && s?.phase === 'controlling' && !s.txArmed &&
      s.controls?.capabilities.includes('frequency') && s.controls.context.radioConnection !== null)
  }
  private matchesSource(source: WheelSource): boolean {
    const displayed = source.context, current = this.operations.getSnapshot().state?.controls?.context
    return !!(displayed && current && displayed.radioId === current.radioId &&
      displayed.radioConnection === current.radioConnection && displayed.ampConnection === current.ampConnection)
  }
  cancel(owner?: object) {
    if (owner && !this.burst?.owners.has(owner) && !this.reading?.owners.has(owner)) return
    // A burst waiting out a lapse, or queued behind a command in flight, is a gesture the operator
    // made and SAW ACCEPTED on the digits: discarding it unsent is said ("Not sent"), never silent.
    const said = this.live && !!this.burst && !!(this.burst.held || this.burst.queued) && this.burst.targetHz !== this.burst.fromHz
    this.inputEpoch++
    clearTimeout(this.timer); this.timer = undefined; this.burst = null
    this.revert()
    if (this.reading) { this.reading = null; this.generation++; this.sending = false; this.inFlight = null; this.notify() }
    if (said) this.failed(new OperationFailure('notController', false))
  }
  /** Show a dial the operator has asked for but the station has not read back. Re-arms the clock:
   * whatever is standing is reverted at PROVISIONAL_MS, and an unconfirmed one is said. */
  private show(hz: number, confirmed = false) {
    this.provisional = { hz, confirmed }
    clearTimeout(this.settle)
    this.settle = setTimeout(() => {
      const standing = this.provisional
      if (!standing) return
      this.provisional = null
      this.notify()
      // A CONFIRMED value is the radio's own: the station's sample has carried it long since, so
      // dropping it moves nothing and says nothing. An unconfirmed one is a claim the station never
      // made — the digits go back to its dial, and the operator is told.
      if (!standing.confirmed && this.live) this.failed(new OperationFailure('operationUnconfirmed', true))
    }, PROVISIONAL_MS)
  }
  /** Drop it without a word: the gesture that made it was refused, and its refusal is already said. */
  private revert() {
    clearTimeout(this.settle); this.settle = undefined
    if (this.provisional) { this.provisional = null; this.notify() }
  }
  /** Capture at pointer-down; a later release may submit one absolute native
   * scope target. It cannot borrow authority renewed during the gesture. */
  captureTarget(source: WheelSource): ((dialHz: number) => boolean) | null {
    if (!this.ready() || !this.matchesSource(source) || this.burst || !Number.isFinite(source.dialMhz) || source.dialMhz <= 0 || source.dialMhz > 250000 ||
      !['USB', 'LSB', 'AM', 'FM'].includes(source.sideband)) return null
    const fromHz = this.dialNow(source.dialMhz), context = this.context(), input = this.inputContext()
    const state = this.operations.getSnapshot().state!
    const b: Burst = { ...source, fromHz, targetHz: fromHz, authority: context, radioId: state.controls!.context.radioId,
      edgeSaid: false, owners: new Set(source.owner ? [source.owner] : []), send: this.operations.prepareControl(source.context!) }
    let consumed = false
    return (dialHz: number) => {
      if (consumed) return false
      consumed = true
      if (!this.ready() || this.burst || input !== this.inputContext() || !Number.isFinite(dialHz) ||
        !Number.isSafeInteger(Math.round(dialHz)) || dialHz < 1 || dialHz > 250000e6) return false
      // An absolute click uses native signal/sideband math from PhoneScope. It
      // has no wheel band-edge clamp and never leaves a trailing drag timer.
      this.cancel()
      b.targetHz = Math.round(dialHz); this.burst = b
      if (b.targetHz !== b.fromHz) { this.show(b.targetHz); this.notify() }
      void this.flush()
      return true
    }
  }
  nudge(deltaHz: number, source: WheelSource, onEdge?: (mhz: number) => void): boolean {
    if (!Number.isFinite(deltaHz) || !deltaHz) return false
    return this.measured(() => this.step(old => old + deltaHz, source, onEdge))
  }
  /** The tuning strip's ◄/► : `steps` whole steps of `stepHz`, rounding to the step grid first as a
   * rig's VFO does (#273, `stepFrom`). It goes through the same burst as the wheel and the digits
   * ON PURPOSE. The strip used to command an absolute dial of its own, read off the sample it
   * draws: while a wheel command was in flight that dial was the one the radio had just left, so a
   * press either landed a step behind or was refused as a second command — and the digits, already
   * showing where the wheel was going, said neither. One writer, one dial. */
  nudgeSteps(steps: number, stepHz: number, source: WheelSource, onEdge?: (mhz: number) => void): boolean {
    if (!Number.isSafeInteger(steps) || !steps || !Number.isSafeInteger(stepHz) || stepHz <= 0) return false
    return this.measured(() => this.step(old => stepFrom(old, steps, stepHz), source, onEdge))
  }
  /** Measured: the step is a gesture; refused, or joined to the burst it will be sent with. */
  private measured(apply: () => boolean): boolean {
    const probe = this.probe
    if (!probe) return apply()
    const gesture = probe.gesture('tune'), before = this.getProvisionalHz()
    const accepted = apply()
    if (!gesture) return accepted
    if (accepted && this.burst) {
      (this.burst.probes ??= []).push(gesture)
      // The digits moved on this step. PERCEIVED stops here — not when the burst is flushed a
      // debounce and a snapshot read later, and not when the command it joins is answered.
      if (this.getProvisionalHz() !== before) probe.shown(gesture)
    }
    else probe.refused(gesture)
    return accepted
  }
  /** `move` takes the burst's current target to the next one: the wheel adds its delta, the strip's
   * arrows step the grid. Everything else about a step — the dial it builds on, the burst it joins,
   * the band-edge stop, the digits — is the same for both, which is the point. */
  private step(move: (old: number) => number, source: WheelSource, onEdge?: (mhz: number) => void): boolean {
    if (!this.inputReady() || !this.matchesSource(source) || !Number.isFinite(source.dialMhz) || source.dialMhz <= 0 || source.dialMhz > 250000 ||
      !['USB', 'LSB', 'AM', 'FM'].includes(source.sideband)) return false
    // A step made while a command is in flight builds on the dial THAT command is asking for, not
    // on the station's reading — which still shows where the radio was, and would send the whole
    // correction to the wrong place. It joins the queued burst instead, sent when that command
    // confirms. `this.inFlight` is null only when nothing is sending, so the `fromHz` below is
    // always a dial someone has committed to.
    const queued = this.sending
    const fromHz = queued ? this.inFlight : this.dialNow(source.dialMhz)
    if (fromHz === null) return false
    const context = this.context()
    // A new event may begin a fresh burst; it cannot carry the old target into
    // that new context. Its authority (lease, radio) is checked again when it is sent, and the
    // command window it goes out on is the one current at that moment.
    if (this.burst && (this.burst.authority !== context || this.burst.fromHz !== fromHz || this.burst.sideband !== source.sideband)) this.cancel()
    if (!this.burst) {
      const state = this.operations.getSnapshot().state!
      this.burst = { ...source, fromHz, targetHz: fromHz, authority: context, radioId: state.controls!.context.radioId,
        edgeSaid: false, owners: new Set(), queued }
    }
    const b = this.burst
    if (source.owner) b.owners.add(source.owner)
    if (b.owners.size > 32) { this.cancel(); return false }
    const old = b.targetHz, next = clampWheelTarget(move(old), old, b.fromHz)
    if (!Number.isSafeInteger(Math.round(next.hz)) || next.hz < 1 || next.hz > 250000e6) return false
    b.targetHz = Math.round(next.hz)
    if (next.hitEdge && b.targetHz !== old && !b.edgeSaid) { b.edgeSaid = true; onEdge?.(b.targetHz / 1e6) }
    // The digits follow the wheel at frame rate: this is the only change a step puts on the render
    // path, and it is a number nothing but the readout can read.
    if (b.targetHz !== old) { this.show(b.targetHz); this.notify() }
    // A queued burst has no timer of its own: the command it waits behind flushes it when it lands.
    if (!queued && !this.timer) this.timer = setTimeout(() => { void this.flush() }, BURST_MS)
    return true
  }
  private async flush() {
    const b = this.burst, generation = this.generation
    this.timer = undefined
    // A burst queued behind a command in flight is not late, it is waiting: the settle below sends
    // it. Nothing is refused here for being queued.
    if (b?.queued && this.sending) return
    if (!b || !this.inputReady() || this.sending || b.authority !== this.context() || (b.send && !this.ready())) { this.cancel(); return }
    if (b.held) return
    if (!b.send && !this.operations.getSnapshot().fresh) {
      // A brief control lapse: hold the burst (later steps still join it) and send it whole once control
      // is current again. A lapse longer than CONTROL_RESUME_MS, or authority lost meanwhile, refuses it
      // whole as not sent (cancel says so). Nothing is sent while control is stale.
      b.held = true
      try { await this.operations.awaitCurrent() } catch { if (this.burst === b) this.cancel(); return }
      if (this.burst !== b) return
      if (!this.inputReady() || this.sending || b.authority !== this.context()) { this.cancel(); return }
      b.held = false
      clearTimeout(this.timer); this.timer = undefined
    }
    this.burst = null
    if (b.targetHz === b.fromHz) { this.revert(); return }
    if (b.probes) this.probe?.dispatch(b.probes, { dialHz: b.targetHz })
    this.reading = b; this.sending = true; this.inFlight = b.targetHz; this.notify()
    let submitted = false
    try {
      // Re-read the shared station stream before dispatch. A local dial change,
      // unavailable sample or another radio's snapshot cancels the entire burst.
      const s = await this.application.invoke<AppSnapshot>('get_snapshot'), radio = s?.radio
      if (!this.live || generation !== this.generation) return
      if (b.authority !== this.context() || this.application.age('get_snapshot') >= 1200 || s.activeRadioId !== b.radioId ||
        !radio || radio.source !== 'native' || radio.catOk !== true || radio.txEnabled || radio.transmitting || radio.rigKeyed !== false || radio.tuning || radio.txBusyReason ||
        this.dialNow(radio.dialMhz) !== b.fromHz || radio.sideband !== b.sideband) throw Error('staleContext')
      const dialMhz = b.targetHz / 1e6
      this.reading = null
      // A scope press brought its sender from pointer-down. A wheel or digit burst is sent from the
      // control current now; prepareControl refuses, unsent, if it is not.
      const send = b.send ?? this.operations.prepareControl(b.context!)
      submitted = true; this.sentAt = performance.now()
      const result = await send({ action: 'radio.frequency', dialMhz, band: bandLabelForMhz(dialMhz), sideband: b.sideband as 'USB' | 'LSB' | 'AM' | 'FM' })
      if (result.outcome !== 'applied' || result.evidence !== 'radioReadback') throw Error('operationUnconfirmed')
      // The station read the radio back at the target: the dial IS there, whatever its next sample
      // still says. That word is what the burst queued behind this one is built on.
      this.confirmed = { hz: b.targetHz, at: performance.now() }
      if (this.provisional?.hz === b.targetHz) this.show(b.targetHz, true)
    } catch (error) {
      // REJECTED, UNKNOWN, OR REFUSED BEFORE IT LEFT: the digits go back to the station's dial NOW,
      // not at the PROVISIONAL_MS deadline. Standing for another two seconds would show a dial the
      // station has just said the radio is not on, and would say so twice — the failure below is
      // the one message this gesture gets.
      if (generation === this.generation) { this.confirmed = null; this.revert() }
      if (this.live && generation === this.generation)
        this.failed(error instanceof OperationFailure ? error : new OperationFailure(error instanceof Error ? error.message : 'operationUnconfirmed', submitted))
    }
    finally {
      if (generation === this.generation) {
        this.reading = null; this.sending = false; this.inFlight = null; this.notify()
        // The burst made while this command was in flight: sent now from the dial the station read
        // back, or refused whole if it read back nothing — a target built on a dial that never
        // happened is exactly what the pre-dispatch re-read exists to stop.
        const queued = this.queuedBurst()
        if (queued && this.confirmed?.hz === queued.fromHz) {
          queued.queued = false
          clearTimeout(this.timer); this.timer = undefined
          // Sent now unless that would break the write floor, which only a round trip shorter than
          // BURST_MS can do. Waiting through a timer where none is needed would also defer the send
          // by a macrotask, which on a polled (v4) station can miss a result poll and cost a whole
          // poll cycle — measured, and the reason this is a branch and not an unconditional timer.
          const wait = BURST_MS - (performance.now() - this.sentAt)
          if (wait > 0) this.timer = setTimeout(() => { void this.flush() }, wait)
          else void this.flush()
        } else if (queued) this.cancel()
      }
    }
  }
  /** The burst built while a command was in flight, if the operator made one. Read through a call
   * because the steps that build it arrive while `flush` is awaiting, which narrowing cannot see. */
  private queuedBurst(): Burst | null { return this.burst?.queued ? this.burst : null }
  /** Where the radio IS, for a step to build on and for the pre-dispatch re-read to check against.
   * The station's own sample is the ordinary answer. A sample it TOOK before it told us a command
   * of ours had read back has simply not caught up — it is older news than that readback, not
   * evidence the dial moved under us. Refusing on it would drop every burst made while the previous
   * one was in flight, which is the whole of what this batch fixes; accepting a sample taken AFTER
   * the readback is what still catches a dial that really did move.
   *
   * A step's sample is the page's RENDER, and `age` does not time a render: it times the newest
   * sample the stream holds, which the render trails by up to a poll. Taken after the readback by
   * that clock, the render could still show the dial the command moved away from, and a step built
   * on it was refused by the re-read as if the radio had moved — the notch after a readback lost
   * whenever it landed in that poll. So the question is put to the newest sample itself: while it
   * shows the readback, the radio is where the readback put it and a render that disagrees is older
   * news; once it shows another dial, the radio moved, and the step keeps the render's dial for the
   * re-read to refuse unless the page already shows that move. */
  private dialNow(sampleDialMhz: number): number {
    const c = this.confirmed, sample = Math.round(sampleDialMhz * 1e6)
    if (!c) return sample
    if (performance.now() - this.application.age('get_snapshot') < c.at) return c.hz
    const held = (this.application.held('get_snapshot') as AppSnapshot | undefined)?.radio?.dialMhz
    return held !== undefined && Math.round(held * 1e6) === c.hz ? c.hz : sample
  }
}
