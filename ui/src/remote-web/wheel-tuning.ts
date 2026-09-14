import type { AppSnapshot } from '../types'
import { bandLabelForMhz } from '../band'
import { clampWheelTarget } from '../wheelTuningPolicy'
import type { ApplicationClient } from './application-client'
import { OperationFailure, type OperationClient } from './operation-client'
import type { StationAction, ControlOutcome, ControlContext } from './station-operation'

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
}

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
 * and a scope press still needs current control from press to release. */
export class WheelTuning {
  private burst: Burst | null = null
  private reading: Burst | null = null
  private timer: ReturnType<typeof setTimeout> | undefined
  private unsubscribe: (() => void) | undefined
  private generation = 0
  private inputEpoch = 0
  private live = false
  private sending = false
  private listeners = new Set<() => void>()
  constructor(private operations: OperationClient, private application: ApplicationClient, private failed: (error: OperationFailure) => void) {}
  subscribe = (f: () => void) => { this.listeners.add(f); return () => { this.listeners.delete(f) } }
  getPending = () => this.sending
  private notify() { for (const f of this.listeners) f() }
  activate() {
    this.live = true
    this.unsubscribe?.()
    let context = this.context()
    this.unsubscribe = this.operations.subscribe(() => {
      const next = this.context()
      // Also invalidate sub-notch input still held by a mounted wheel listener.
      // A disconnect/reopen can occur in one React render with the same lease.
      // A freshness lapse alone keeps held input (see the class comment); losing authority does not.
      if (!this.inputReady() || context !== next) this.cancel()
      context = next
    })
  }
  dispose() {
    this.live = false; this.generation++; this.cancel(); this.unsubscribe?.(); this.unsubscribe = undefined
    this.sending = false; this.notify()
  }
  private context(): string {
    const s = this.operations.getSnapshot().state, c = s?.controls?.context
    return JSON.stringify([s?.stationBootId, s?.leaseId, s?.revision, c?.radioId, c?.radioConnection, c?.ampConnection])
  }
  ready(): boolean { return !this.sending && this.authorityReady() }
  inputContext(): string { return JSON.stringify([this.inputEpoch, this.context()]) }
  private authorityReady(): boolean { return this.inputReady() && this.operations.getSnapshot().fresh }
  /** Authority to accept a wheel or digit step: everything authorityReady asks except the freshness
   * window. The send itself still waits for current control. */
  private inputReady(): boolean {
    const v = this.operations.getSnapshot(), s = v.state
    return !!(this.live && this.operations.operationVersion >= 3 && v.connected &&
      !v.unresolved && !v.controlPending && !v.submitting && s?.phase === 'controlling' && !s.txArmed &&
      s.controls?.capabilities.includes('frequency') && s.controls.context.radioConnection !== null)
  }
  private matchesSource(source: WheelSource): boolean {
    const displayed = source.context, current = this.operations.getSnapshot().state?.controls?.context
    return !!(displayed && current && displayed.radioId === current.radioId &&
      displayed.radioConnection === current.radioConnection && displayed.ampConnection === current.ampConnection)
  }
  cancel(owner?: object) {
    if (owner && !this.burst?.owners.has(owner) && !this.reading?.owners.has(owner)) return
    // A burst already waiting out a lapse is a gesture the operator made and saw accepted:
    // discarding it unsent is said ("Not sent"), never silent.
    const held = this.live && !!this.burst?.held && this.burst.targetHz !== this.burst.fromHz
    this.inputEpoch++
    clearTimeout(this.timer); this.timer = undefined; this.burst = null
    if (this.reading) { this.reading = null; this.generation++; this.sending = false; this.notify() }
    if (held) this.failed(new OperationFailure('notController', false))
  }
  /** Capture at pointer-down; a later release may submit one absolute native
   * scope target. It cannot borrow authority renewed during the gesture. */
  captureTarget(source: WheelSource): ((dialHz: number) => boolean) | null {
    if (!this.ready() || !this.matchesSource(source) || this.burst || !Number.isFinite(source.dialMhz) || source.dialMhz <= 0 || source.dialMhz > 250000 ||
      !['USB', 'LSB', 'AM', 'FM'].includes(source.sideband)) return null
    const fromHz = Math.round(source.dialMhz * 1e6), context = this.context(), input = this.inputContext()
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
      void this.flush()
      return true
    }
  }
  nudge(deltaHz: number, source: WheelSource, onEdge?: (mhz: number) => void): boolean {
    if (this.sending || !this.inputReady() || !this.matchesSource(source) || !Number.isFinite(deltaHz) || !deltaHz || !Number.isFinite(source.dialMhz) || source.dialMhz <= 0 || source.dialMhz > 250000 ||
      !['USB', 'LSB', 'AM', 'FM'].includes(source.sideband)) return false
    const fromHz = Math.round(source.dialMhz * 1e6), context = this.context()
    // A new event may begin a fresh burst; it cannot carry the old target into
    // that new context. Its authority (lease, revision, radio) is checked again when it is sent.
    if (this.burst && (this.burst.authority !== context || this.burst.fromHz !== fromHz || this.burst.sideband !== source.sideband)) this.cancel()
    if (!this.burst) {
      const state = this.operations.getSnapshot().state!
      this.burst = { ...source, fromHz, targetHz: fromHz, authority: context, radioId: state.controls!.context.radioId,
        edgeSaid: false, owners: new Set() }
    }
    const b = this.burst
    if (source.owner) b.owners.add(source.owner)
    if (b.owners.size > 32) { this.cancel(); return false }
    const old = b.targetHz, next = clampWheelTarget(old + deltaHz, old, b.fromHz)
    if (!Number.isSafeInteger(Math.round(next.hz)) || next.hz < 1 || next.hz > 250000e6) return false
    b.targetHz = Math.round(next.hz)
    if (next.hitEdge && b.targetHz !== old && !b.edgeSaid) { b.edgeSaid = true; onEdge?.(b.targetHz / 1e6) }
    if (!this.timer) this.timer = setTimeout(() => { void this.flush() }, 120)
    return true
  }
  private async flush() {
    const b = this.burst, generation = this.generation
    this.timer = undefined
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
    if (b.targetHz === b.fromHz) return
    this.reading = b; this.sending = true; this.notify()
    let submitted = false
    try {
      // Re-read the shared station stream before dispatch. A local dial change,
      // unavailable sample or another radio's snapshot cancels the entire burst.
      const s = await this.application.invoke<AppSnapshot>('get_snapshot'), radio = s?.radio
      if (!this.live || generation !== this.generation) return
      if (b.authority !== this.context() || this.application.age('get_snapshot') >= 1200 || s.activeRadioId !== b.radioId ||
        !radio || radio.source !== 'native' || radio.catOk !== true || radio.txEnabled || radio.transmitting || radio.rigKeyed !== false || radio.tuning || radio.txBusyReason ||
        Math.round(radio.dialMhz * 1e6) !== b.fromHz || radio.sideband !== b.sideband) throw Error('staleContext')
      const dialMhz = b.targetHz / 1e6
      this.reading = null
      // A scope press brought its sender from pointer-down. A wheel or digit burst is sent from the
      // control current now; prepareControl refuses, unsent, if it is not.
      const send = b.send ?? this.operations.prepareControl(b.context!)
      submitted = true
      const result = await send({ action: 'radio.frequency', dialMhz, band: bandLabelForMhz(dialMhz), sideband: b.sideband as 'USB' | 'LSB' | 'AM' | 'FM' })
      if (result.outcome !== 'applied' || result.evidence !== 'radioReadback') throw Error('operationUnconfirmed')
    } catch (error) {
      if (this.live && generation === this.generation)
        this.failed(error instanceof OperationFailure ? error : new OperationFailure(error instanceof Error ? error.message : 'operationUnconfirmed', submitted))
    }
    finally { if (generation === this.generation) { this.reading = null; this.sending = false; this.notify() } }
  }
}
