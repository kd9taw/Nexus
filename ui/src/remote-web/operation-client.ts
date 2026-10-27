import type { OperationVersion } from './operation-version'
import { OPERATION_RATE_LIMIT, OPERATION_RATE_WINDOW_MS } from './operation-limits'
import {
  OPERATION_EXPORT_RESPONSE_BYTES,
  OPERATION_RESPONSE_BYTES,
  operationRequest,
  operationResponse,
  type OperationRequest,
  type OperationState,
  type OperationValue
} from './operation-protocol'
export type OperationView = {
  supported: boolean
  state: OperationState | null
  fresh: boolean
  connected: boolean
  busy: boolean
  error: string | null
  /** An automatic read (heartbeat/state) is the request in flight — Release and acquire wait it out. */
  reading: boolean
  stopSending?: boolean
  stopAvailable?: boolean
  stopError?: string | null
  /** The station ACCEPTED a Stop — it is not a claim that RF stopped, and must never be shown as
   * one. `stop_transmit` returns on acceptance, and when the station's Engine is held the halt runs
   * afterwards on its own thread, so between this and the transmitter actually going free the rig
   * is still on the air. The browser says "stop sent" here and "stopped" only once the station's
   * own reading shows the transmitter free. Cleared when a new Stop is
   * sent, when one fails, and on disconnect; the relay's limit inside the second after an acceptance
   * is not a failure, and puts it back (`stopAcceptedAt`). */
  stopAccepted?: boolean
  requestReady?: boolean
  /** DISPLAY ONLY. The station reported this browser's lease as running until at least now,
   * measured from the request's start. Lets the authority label stay steady through the short gap
   * between heartbeats; commands still require `fresh`. */
  leaseHeld?: boolean
}
type Pending = {
  request: OperationRequest
  started: number
  timer: ReturnType<typeof setTimeout>
  resolve: (value: OperationValue) => void
  reject: (error: Error) => void
}
/** One explicit operation at a time. Neither a reconnect nor a timeout can acquire control. */
export class OperationClient {
  private listeners = new Set<() => void>()
  private view: OperationView = {
    supported: false,
    state: null,
    fresh: false,
    connected: false,
    busy: false,
    error: null,
    reading: false,
    leaseHeld: false
  }
  private pending: Pending | null = null
  private pendingStop: Pending | null = null
  private stopTarget: { stationBootId: string; leaseId: string; transmitEpoch: string } | null = null
  /** The lease this browser's stop token was first issued under. Remembered separately because it
   * outlives both the lease itself (the station keeps matching a Stop against it after the lease
   * runs out — expired-lease stop, 2026-09-15) and any one `stopTarget`. Cleared only on
   * disconnect. */
  private stopLeaseId: string | null = null
  /** When this browser's last Stop was accepted. The relay holds a session to two Stops a second, its
   * own limit, so a `remoteBusy` for a press sent within that second of an acceptance is the limit, not
   * a refusal to stop: the station is already stopping, and that acceptance stands (2026-10-03). */
  private stopAcceptedAt = -Infinity
  // A successful mutation invalidates command context, not the controller's
  // lease. Refresh with a heartbeat so continuous use cannot starve renewal.
  // This token permits only renewal/release; actions still require fresh state.
  private heartbeatLeaseId: string | null = null
  private timer: ReturnType<typeof setInterval> | undefined
  private stateUntil = 0
  private leaseUntil = 0
  private polledAt = 0
  private requestBudget: { requestId: string; until: number }[] = []
  constructor(
    private send: (message: string) => void,
    readonly enabled: boolean,
    private now = () => performance.now(),
    readonly operationVersion: OperationVersion = 1
  ) {}
  subscribe = (f: () => void) => {
    this.listeners.add(f)
    return () => {
      this.listeners.delete(f)
    }
  }
  getSnapshot = () => this.view
  private update(value: Partial<OperationView>) {
    this.view = {
      ...this.view,
      ...value,
      requestReady: (value.connected ?? this.view.connected) && this.requestCount() < OPERATION_RATE_LIMIT,
      reading: this.automaticReadInFlight(),
      stopSending: !!this.pendingStop,
      stopAvailable: this.operationVersion >= 4 && !!this.stopTarget && (value.connected ?? this.view.connected),
      leaseHeld: this.now() < this.leaseUntil
    }
    for (const f of this.listeners) f()
  }
  open() {
    if (!this.enabled) return
    this.disconnected()
    this.update({ connected: true, error: null })
    this.tick()
    this.timer = setInterval(() => this.tick(), 250)
  }
  disconnected() {
    clearInterval(this.timer)
    this.timer = undefined
    this.stopTarget = null
    this.stopLeaseId = null
    this.stopAcceptedAt = -Infinity
    this.heartbeatLeaseId = null
    const stop = this.pendingStop
    this.pendingStop = null
    this.update({ stopAccepted: false, stopError: null })
    if (stop) { clearTimeout(stop.timer); stop.reject(new Error('operationUnknown')) }
    const p = this.pending
    this.pending = null
    if (p) {
      clearTimeout(p.timer)
      this.finishBudget(p.request.requestId)
      p.reject(new Error('stationUnavailable'))
    }
    this.stateUntil = 0
    this.leaseUntil = 0
    this.update({ state: null, fresh: false, connected: false, busy: false })
  }
  private tick() {
    const now = this.now(),
      fresh = !!this.view.state && now < this.stateUntil
    if (fresh !== this.view.fresh) this.update({ fresh })
    if ((now < this.leaseUntil) !== this.view.leaseHeld) this.update({})
    if ((this.view.connected && this.requestCount() < OPERATION_RATE_LIMIT) !== this.view.requestReady) this.update({})
    if (
      !this.view.connected ||
      this.pending ||
      this.view.error === 'stationUnsupported'
    )
      return
    // Hidden pages cannot retain a control lease. Expiry is also enforced by the
    // native monotonic clock when a browser suspends timers altogether.
    if (typeof document !== 'undefined' && document.hidden && this.heartbeatLeaseId) {
      void this.release()
      return
    }
    // Automatic reads leave one ordinary slot for an explicit operator action.
    // After a mutation, controls remain unavailable until a fresh state can be
    // read within this budget. Stop never enters the ordinary budget.
    if (this.requestCount() >= OPERATION_RATE_LIMIT - 1) return
    if (now - this.polledAt < 1000) return
    this.polledAt = now
    const request: OperationRequest =
      this.heartbeatLeaseId
        ? { type: 'heartbeat', requestId: crypto.randomUUID(), leaseId: this.heartbeatLeaseId }
        : { type: 'state', requestId: crypto.randomUUID() }
    void this.request(request).catch(() => {})
  }
  private requestCount(): number {
    this.requestBudget = this.requestBudget.filter(entry => this.now() < entry.until)
    return this.requestBudget.length
  }
  private finishBudget(requestId: string) {
    const entry = this.requestBudget.find(entry => entry.requestId === requestId)
    // Receipt time bounds the relay's earlier arrival without comparing clocks.
    // Keeping a slot through the reply also handles delayed/clustered delivery.
    if (entry) entry.until = this.now() + OPERATION_RATE_WINDOW_MS
  }
  private request(request: OperationRequest): Promise<OperationValue> {
    if (!this.view.connected) return Promise.reject(new Error('stationUnavailable'))
    if (this.pending) return Promise.reject(new Error('remoteBusy'))
    if (this.requestCount() >= OPERATION_RATE_LIMIT) return Promise.reject(new Error('remoteBusy'))
    operationRequest(request)
    return new Promise((resolve, reject) => {
      const p: Pending = {
        request,
        started: this.now(),
        resolve,
        reject,
        timer: setTimeout(() => {
          if (this.pending !== p) return
          this.pending = null
          this.heartbeatLeaseId = null
          this.leaseUntil = 0
          this.finishBudget(request.requestId)
          this.update({ busy: false, state: null, fresh: false, error: 'stationUnavailable' })
          reject(new Error('stationUnavailable'))
        }, 7500)
      }
      this.pending = p
      this.requestBudget.push({ requestId: request.requestId, until: Infinity })
      this.update({ busy: true, error: null })
      try {
        this.send(JSON.stringify({ type: 'operationRequest', ...(this.operationVersion >= 2 ? { operationVersion: this.operationVersion } : {}), request }))
      } catch {
        clearTimeout(p.timer)
        this.pending = null
        this.heartbeatLeaseId = null
        this.leaseUntil = 0
        this.finishBudget(request.requestId)
        this.update({ busy: false, state: null, fresh: false, error: 'stationUnavailable' })
        reject(new Error('stationUnavailable'))
      }
    })
  }
  /** Stop uses the last station-issued owner token, even while an ordinary
   * request or receipt is unresolved. Native authority alone decides if it is
   * still valid. Acceptance is revocation, not confirmation that RF stopped.
   * An accepted Stop keeps the token: the next press goes on it at once. */
  stopTransmit(): Promise<OperationValue> {
    if (this.operationVersion < 4) return Promise.reject(new Error('stationUnsupported'))
    if (!this.view.connected) return Promise.reject(new Error('stationUnavailable'))
    if (this.pendingStop) return Promise.reject(new Error('remoteBusy'))
    if (!this.stopTarget) return Promise.reject(new Error('localPermissionRequired'))
    const request: OperationRequest = { type: 'stopTransmit', requestId: crypto.randomUUID(), ...this.stopTarget }
    operationRequest(request)
    return new Promise((resolve, reject) => {
      const p: Pending = { request, started: this.now(), resolve, reject, timer: setTimeout(() => {
        if (this.pendingStop !== p) return
        this.pendingStop = null
        this.update({ stopError: 'operationUnknown', stopAccepted: false })
        reject(new Error('operationUnknown'))
      }, 7500) }
      this.pendingStop = p
      this.update({ stopError: null, stopAccepted: false })
      try { this.send(JSON.stringify({ type: 'operationRequest', operationVersion: 4, request })) }
      catch {
        clearTimeout(p.timer)
        this.pendingStop = null
        this.update({ stopError: 'operationUnknown', stopAccepted: false })
        reject(new Error('operationUnknown'))
      }
    })
  }
  /** `bytes` is the reply's size on the wire. This page asks for no export, so no reply it is
   * waiting for may exceed OPERATION_RESPONSE_BYTES. */
  receive(raw: unknown, bytes = 0) {
    if (bytes > OPERATION_EXPORT_RESPONSE_BYTES) throw Error('invalidOperation')
    const r = operationResponse(raw)
    const stop = this.pendingStop
    if (stop?.request.requestId === r.requestId) {
      if (bytes > OPERATION_RESPONSE_BYTES || ('value' in r && !('stop' in r.value))) throw Error('invalidOperation')
      clearTimeout(stop.timer)
      this.pendingStop = null
      if ('error' in r) {
        // The relay's two-a-second limit met inside the second after an accepted Stop: no failure to
        // show, because a Stop is already under way. The acceptance stands, and the press resolves on
        // it. Every other refusal, and this one without that acceptance, is still a failure.
        if (r.error === 'remoteBusy' && stop.started - this.stopAcceptedAt < OPERATION_RATE_WINDOW_MS) {
          this.update({ stopError: null, stopAccepted: true })
          stop.resolve({ stop: 'accepted' })
          return
        }
        this.update({ stopError: r.error, stopAccepted: false })
        stop.reject(new Error(r.error))
      } else {
        // ACCEPTANCE, never a claim that RF stopped — see `stopAccepted`.
        this.stopAcceptedAt = this.now()
        this.polledAt = -Infinity
        this.update({ stopError: null, stopAccepted: true })
        stop.resolve(r.value)
      }
      return
    }
    const p = this.pending
    if (!p || p.request.requestId !== r.requestId) return
    if (bytes > OPERATION_RESPONSE_BYTES) throw Error('invalidOperation')
    // This page sends only reads, acquire and release, and each is answered with a station state:
    // a Stop, an outcome or an export answering one of them is a crossed reply.
    if ('value' in r && !('phase' in r.value)) throw Error('invalidOperation')
    clearTimeout(p.timer)
    this.pending = null
    this.finishBudget(p.request.requestId)
    if ('error' in r) {
      // An automatic read the station was merely too busy to answer (`stationBusy`: one of its own
      // operation tasks in flight; `remoteBusy`: its authority lock held) is contention, not a
      // refusal, and it decided nothing about this browser's authority. The station itself keeps
      // feeding audio through it on purpose. Keep the lease and the last state; the freshness
      // window still runs out on its own, so commands wait for a current state, and the next
      // heartbeat - one second on - re-asks under the same lease. Treating it as the end of
      // authority blanked every control and released the audio each time the station was busy.
      const automatic = p.request.type === 'heartbeat' || p.request.type === 'state'
      if (automatic && (r.error === 'stationBusy' || r.error === 'remoteBusy')) {
        this.update({ busy: false })
        p.reject(new Error(r.error))
        return
      }
      this.heartbeatLeaseId = null
      this.leaseUntil = 0
      this.update({ busy: false, state: null, fresh: false, error: r.error })
      p.reject(new Error(r.error))
      return
    }
    // A station state: anything else was refused above, before the request was settled.
    if (!this.installState(r.value as OperationState, p.started)) this.update({ busy: false, error: null })
    p.resolve(r.value)
  }
  /** A state from a station reply. The station's revision only grows, so a state arriving with a
   * LOWER revision than the one held for the same boot was computed earlier and is discarded;
   * equal or higher is installed. Returns whether it installed. `started` is when the state was
   * asked for: freshness is measured from there. */
  private installState(value: OperationState, started: number): boolean {
    const held = this.view.state
    if (held && held.stationBootId === value.stationBootId && value.revision < held.revision) return false
    this.heartbeatLeaseId = value.phase === 'controlling' ? value.leaseId : null
    // Stop OUTLIVES the lease (operator ruling, 2026-09-15): an unnecessary unkey is a smaller
    // harm than a keyed rig and a Stop button that reported a refusal. The STATION decides — it
    // keeps issuing a current `transmitEpoch` to a browser that held station control after its
    // lease runs out, and stops the moment the grant goes, another browser takes over or the
    // station reboots. So the token is the whole permission here and the lease id it was first
    // issued under is simply carried. Nothing this keeps alive can START anything: every command
    // still requires `phase === 'controlling'` and a live command window.
    if (value.phase === 'controlling' && value.leaseId) this.stopLeaseId = value.leaseId
    this.stopTarget = this.operationVersion >= 4 && this.stopLeaseId && value.transmitEpoch
      ? { stationBootId: value.stationBootId, leaseId: this.stopLeaseId, transmitEpoch: value.transmitEpoch } : null
    this.stateUntil = started + Math.min(1200, value.leaseRemainingMs ?? 1200)
    this.leaseUntil = value.phase === 'controlling' && value.leaseRemainingMs != null ? started + value.leaseRemainingMs : 0
    this.update({
      supported: true,
      busy: false,
      state: value,
      fresh: this.now() < this.stateUntil,
      error: null
    })
    return true
  }
  async acquire() {
    // Waits out an automatic read first, as `release()` does and for the same reason: `request()`
    // refuses anything while another request is in flight, so an acquire made mid-read was refused
    // `remoteBusy` and never left. Only when a read IS in flight; with none this stays synchronous
    // up to `request()`.
    if (this.automaticReadInFlight()) await this.settleAutomaticRead()
    const s = this.view.state
    // Refused here, unsent, in the words the station uses for the same two refusals, so a page can
    // say why: another browser holds control, or this one may not take it. Anything else is a
    // state not current enough to act on, which the next read answers.
    if (!s || !this.view.fresh || !s.allowed || s.phase !== 'available')
      throw new Error(s?.phase === 'occupied' ? 'controllerBusy' : s && !s.allowed ? 'localPermissionRequired' : 'stationUnavailable')
    await this.request({
      type: 'acquire',
      requestId: crypto.randomUUID(),
      stationBootId: s.stationBootId
    })
  }
  private automaticReadInFlight(): boolean {
    const type = this.pending?.request.type
    return type === 'heartbeat' || type === 'state'
  }
  /** Resolves once no automatic read is in flight. Bounded: a reply, an error, the request's own
   * timeout or a disconnect all clear `pending` and notify, exactly as `waitForHeartbeat` relies on. */
  private settleAutomaticRead(): Promise<void> {
    if (!this.automaticReadInFlight()) return Promise.resolve()
    return new Promise(resolve => {
      const unsubscribe = this.subscribe(() => {
        if (!this.automaticReadInFlight() || !this.view.connected) {
          unsubscribe()
          resolve()
        }
      })
    })
  }
  async release() {
    // ⚠️ WAIT OUT AN AUTOMATIC READ FIRST. `request()` refuses anything while another request is in
    // flight, and the refusal is swallowed below — so a Release clicked mid-heartbeat used to clear
    // the lease locally, send NOTHING, and let the heartbeat's reply re-arm the lease: the operator
    // believed they had released the station and still held control. That is why the button greyed
    // on every heartbeat (the flicker). Now it waits for the read to settle, reads the lease the
    // reply may just have re-armed, and sends once. A second click finds the lease already taken.
    // Only when a read IS in flight: with none, this stays synchronous up to `request()`, so the
    // release leaves on the click exactly as before (an unconditional await deferred it a microtask).
    if (this.automaticReadInFlight()) await this.settleAutomaticRead()
    const leaseId = this.heartbeatLeaseId
    this.heartbeatLeaseId = null
    this.leaseUntil = 0
    this.update({ state: null, fresh: false })
    if (!leaseId) return
    try {
      await this.request({ type: 'release', requestId: crypto.randomUUID(), leaseId })
    } catch {}
  }
}
