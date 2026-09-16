import type { ReceiptStorage } from './operation-storage'
import type { ControlStorage, PendingControl } from './control-storage'
import { actionCapability, controlContext, stationAction, type StationAction, type ControlOutcome, type ControlContext } from './station-operation'
import { controlVersion, type OperationVersion } from './operation-version'
import { OPERATION_RATE_LIMIT, OPERATION_RATE_WINDOW_MS } from './operation-limits'
import {
  OPERATION_EXPORT_RESPONSE_BYTES,
  OPERATION_RESPONSE_BYTES,
  activationSelection,
  logChange,
  logChangeCapabilities,
  manualRecord,
  operationId,
  operationRequest,
  operationResponse,
  type ActivationExportValue,
  type ActivationSelection,
  type LogChange,
  type LogChangeOutcome,
  type ManualRecord,
  type OperationOutcome,
  type OperationRequest,
  type OperationState,
  type OperationValue,
  type ProgramExportFormat,
  type ProgramExportValue
} from './operation-protocol'
/** A station command or manual log that failed. `sent` records whether its request actually left
 * this browser: false means nothing reached the station, so nothing there changed. `busy` records
 * that the station answered stationBusy, which it does before changing anything (its Engine was
 * held). The view says "not sent" or "busy" only then; a request that left and got no confirmation
 * stays "not confirmed". */
export class OperationFailure extends Error {
  constructor(message: string, readonly sent: boolean, readonly busy = false) {
    super(message)
  }
}
export type ControlFailure = { code: string; sent: boolean; busy: boolean }
/** Only a reply that came back from the station can say it was busy. */
const stationWasBusy = (code: string, sent: boolean) => sent && code === 'stationBusy'
/** How long a gesture committed during a brief control lapse waits for control to be current again
 * before it is refused as not sent. The client heartbeats once a second on a 250 ms tick and a reply
 * is current for 1200 ms from its request, so a lapse ends when the next reply lands: at most 50 ms
 * after the window closes plus that reply's round trip. 1500 ms allows a round trip of about 1.4 s;
 * a longer silence is not a brief lapse. The freshness window and heartbeat are unchanged. */
export const CONTROL_RESUME_MS = 1500
export type OperationView = {
  supported: boolean
  state: OperationState | null
  fresh: boolean
  connected: boolean
  busy: boolean
  submitting: boolean
  unresolved: string | null
  pendingDraft: ManualRecord | null
  resolved: OperationOutcome | LogChangeOutcome | null
  dismissed: string | null
  error: string | null
  controlPending: PendingControl | null
  controlSending: boolean
  controlResult: ControlOutcome | null
  controlError: ControlFailure | null
  controlRefreshing?: boolean
  stopSending?: boolean
  stopAvailable?: boolean
  stopError?: string | null
  /** The station ACCEPTED a Stop — it is not a claim that RF stopped, and must never be shown as
   * one. `stop_transmit` returns on acceptance, and when the station's Engine is held the halt runs
   * afterwards on its own thread, so between this and the transmitter actually going free the rig
   * is still on the air. The browser says "stop sent" here and "stopped" only once the station's
   * own reading shows the transmitter free (`useStationStopProgress`). Cleared when a new Stop is
   * sent, when one fails, and on disconnect. */
  stopAccepted?: boolean
  requestReady?: boolean
  /** DISPLAY ONLY. The last state the station sent, kept while a command outcome's re-read is
   * pending so the banner and panels stay mounted (shown disabled) instead of unmounting until the
   * next heartbeat. Cleared whenever authority ends or becomes unknown: disconnect, an error reply,
   * a timed-out or unsendable request, release. No command gate reads it; they read `state` and
   * `fresh`, which a command outcome still clears exactly as before. */
  retainedState?: OperationState | null
  /** DISPLAY ONLY. The station reported this browser's lease as running until at least now,
   * measured from the request's start. Lets the authority label stay steady through the short gap
   * between heartbeats; commands still require `fresh`. */
  leaseHeld?: boolean
}
type CapturedControl = { state: OperationState; until: number }
type Pending = {
  request: OperationRequest
  started: number
  timer: ReturnType<typeof setTimeout>
  resolve: (value: OperationValue) => void
  reject: (error: Error) => void
}
/** One explicit operation at a time. Neither a reconnect nor a timeout can
 * acquire control or resubmit a QSO. Native receipts confirm outcomes; an operator can explicitly dismiss a pending check. */
export class OperationClient {
  private listeners = new Set<() => void>()
  private view: OperationView = {
    supported: false,
    state: null,
    fresh: false,
    connected: false,
    busy: false,
    submitting: false,
    unresolved: null,
    pendingDraft: null,
    resolved: null,
    dismissed: null,
    error: null,
    controlPending: null,
    controlSending: false,
    controlResult: null,
    controlError: null,
    retainedState: null,
    leaseHeld: false
  }
  private pending: Pending | null = null
  private pendingStop: Pending | null = null
  private stopTarget: { stationBootId: string; leaseId: string; transmitEpoch: string } | null = null
  /** The lease this browser's stop token was first issued under. Remembered separately because it
   * outlives both the lease itself (the station keeps matching a Stop against it after the lease
   * runs out — expired-lease stop, 2026-09-15) and `stopTarget`, which a successful Stop clears,
   * its epoch having been retired. Cleared only on disconnect. */
  private stopLeaseId: string | null = null
  // A successful mutation invalidates command context, not the controller's
  // lease. Refresh with a heartbeat so continuous use cannot starve renewal.
  // This token permits only renewal/release; actions still require fresh state.
  private heartbeatLeaseId: string | null = null
  private timer: ReturnType<typeof setInterval> | undefined
  private stateUntil = 0
  private leaseUntil = 0
  private controlRefreshUntil = 0
  private polledAt = 0
  private finished: OperationOutcome | null = null
  private loggingIntent = false
  private controlIntent = false
  private resultIntent: object | null = null
  private controlPolledAt = -Infinity
  private requestBudget: { requestId: string; until: number }[] = []
  constructor(
    private send: (message: string) => void,
    readonly enabled: boolean,
    private now = () => performance.now(),
    private receiptStorage?: ReceiptStorage,
    readonly operationVersion: OperationVersion = 1,
    private controlStorage?: ControlStorage
  ) {
    try {
      const id = receiptStorage?.read()
      if (operationId(id))
        this.view = {
          ...this.view,
          unresolved: id,
          pendingDraft: receiptStorage?.readDraft?.() ?? null
        }
    } catch {
      this.view = { ...this.view, error: 'receiptStorageUnavailable' }
    }
    try { this.view = { ...this.view, controlPending: controlStorage?.read() ?? null } }
    catch { this.view = { ...this.view, error: 'receiptStorageUnavailable' } }
  }
  subscribe = (f: () => void) => {
    this.listeners.add(f)
    return () => {
      this.listeners.delete(f)
    }
  }
  getSnapshot = () => this.view
  private update(value: Partial<OperationView>) {
    if ('unresolved' in value && value.unresolved !== this.view.unresolved) {
      try {
        this.receiptStorage?.write(value.unresolved ?? null, 'pendingDraft' in value ? value.pendingDraft : undefined)
      } catch (error) {
        if (value.unresolved) throw error
      }
    }
    this.view = {
      ...this.view,
      ...value,
      requestReady: (value.connected ?? this.view.connected) && this.requestCount() < OPERATION_RATE_LIMIT,
      controlSending: this.pending?.request.type === 'stationControl',
      stopSending: !!this.pendingStop,
      stopAvailable: this.operationVersion >= 4 && !!this.stopTarget && (value.connected ?? this.view.connected),
      ...((value.error || value.connected === false || value.state) ? { controlRefreshing: false } : {}),
      ...(value.unresolved === null ? { pendingDraft: null } : {}),
      ...(value.state ? { retainedState: value.state } : {}),
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
    this.resultIntent = null
    this.stopTarget = null
    this.stopLeaseId = null
    this.heartbeatLeaseId = null
    const stop = this.pendingStop
    this.pendingStop = null
    this.update({ stopAccepted: false })
    if (stop) { clearTimeout(stop.timer); stop.reject(new Error('operationUnknown')) }
    const p = this.pending
    this.pending = null
    if (p) {
      clearTimeout(p.timer)
      this.finishBudget(p.request.requestId)
      const write = p.request.type === 'logManual' || p.request.type === 'logChange'
      if (write) this.update({ unresolved: p.request.requestId })
      p.reject(new Error(write ? 'operationUnknown' : 'stationUnavailable'))
    }
    this.stateUntil = 0
    this.leaseUntil = 0
    this.update({ state: null, retainedState: null, fresh: false, connected: false, busy: false, submitting: false })
  }
  private tick() {
    const now = this.now(),
      fresh = !!this.view.state && now < this.stateUntil
    if (this.view.controlRefreshing && now >= this.controlRefreshUntil) this.update({ controlRefreshing: false })
    if (fresh !== this.view.fresh) this.update({ fresh })
    if ((now < this.leaseUntil) !== this.view.leaseHeld) this.update({})
    if ((this.view.connected && this.requestCount() < OPERATION_RATE_LIMIT) !== this.view.requestReady) this.update({})
    if (
      !this.view.connected ||
      this.pending ||
      this.loggingIntent ||
      (this.controlIntent && !this.view.controlPending) ||
      this.resultIntent ||
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
    const readLimit = this.view.controlPending || this.view.unresolved ? OPERATION_RATE_LIMIT : OPERATION_RATE_LIMIT - 1
    if (this.requestCount() >= readLimit) return
    if (now - this.polledAt < 1000) {
      if (this.view.controlPending && this.view.controlResult?.outcome !== 'unknown' && now - this.controlPolledAt >= 1000) {
        this.controlPolledAt = now
        void this.refreshControl().catch(() => {})
      }
      return
    }
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
  private requireRequestCapacity() {
    if (this.requestCount() >= OPERATION_RATE_LIMIT) throw Error('remoteBusy')
  }
  private finishBudget(requestId: string) {
    const entry = this.requestBudget.find(entry => entry.requestId === requestId)
    // Receipt time bounds the relay's earlier arrival without comparing clocks.
    // Keeping a slot through the reply also handles delayed/clustered delivery.
    if (entry) entry.until = this.now() + OPERATION_RATE_WINDOW_MS
  }
  private request(request: OperationRequest, onSent?: () => void): Promise<OperationValue> {
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
          const mutation = request.type === 'logManual' || request.type === 'stationControl' || request.type === 'logChange'
          this.update({
            busy: false,
            submitting: false,
            state: null,
            retainedState: null,
            fresh: false,
            error: mutation ? 'operationUnknown' : 'stationUnavailable',
            ...(request.type === 'logManual' || request.type === 'logChange' ? { unresolved: request.requestId } : {})
          })
          reject(new Error(mutation ? 'operationUnknown' : 'stationUnavailable'))
        }, 7500)
      }
      if (request.type === 'logManual' || request.type === 'logChange') {
        try {
          // A change has no draft to show again; its receipt alone blocks the next write.
          this.update({
            unresolved: request.requestId,
            pendingDraft: request.type === 'logManual' ? structuredClone(request.record) : null
          })
        } catch {
          clearTimeout(p.timer)
          reject(new Error('receiptStorageUnavailable'))
          return
        }
      }
      this.pending = p
      this.requestBudget.push({ requestId: request.requestId, until: Infinity })
      this.update({ busy: true, submitting: request.type === 'logManual', error: null })
      try {
        this.send(JSON.stringify({ type: 'operationRequest', ...(this.operationVersion >= 2 ? { operationVersion: this.operationVersion } : {}), request }))
        onSent?.()
      } catch {
        clearTimeout(p.timer)
        this.pending = null
        this.heartbeatLeaseId = null
        this.leaseUntil = 0
        this.finishBudget(request.requestId)
        this.update({
          busy: false,
          submitting: false,
          state: null,
          retainedState: null,
          fresh: false,
          error: 'stationUnavailable',
          ...(request.type === 'logManual' || request.type === 'logChange' ? { unresolved: null } : {})
        })
        reject(new Error('stationUnavailable'))
      }
    })
  }
  /** Stop uses the last station-issued owner token, even while an ordinary
   * request or receipt is unresolved. Native authority alone decides if it is
   * still valid. Acceptance is revocation, not confirmation that RF stopped. */
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
  /** `bytes` is the reply's size on the wire. Only a chunk of the file this browser asked for —
   * an activation ADIF or the programming CHIRP/CSV — may exceed OPERATION_RESPONSE_BYTES. */
  receive(raw: unknown, bytes = 0) {
    if (bytes > OPERATION_EXPORT_RESPONSE_BYTES) throw Error('invalidOperation')
    const r = operationResponse(raw)
    const stop = this.pendingStop
    if (stop?.request.requestId === r.requestId) {
      if (bytes > OPERATION_RESPONSE_BYTES || ('value' in r && !('stop' in r.value))) throw Error('invalidOperation')
      clearTimeout(stop.timer)
      this.pendingStop = null
      if ('error' in r) {
        this.update({ stopError: r.error, stopAccepted: false })
        stop.reject(new Error(r.error))
      } else {
        // ACCEPTANCE, never a claim that RF stopped — see `stopAccepted`.
        this.stopTarget = null
        this.polledAt = -Infinity
        this.update({ stopError: null, stopAccepted: true })
        stop.resolve(r.value)
      }
      return
    }
    const p = this.pending
    if (!p || p.request.requestId !== r.requestId) return
    // Either export answers with a chunk that may be over the ordinary ceiling, and each must come
    // back to the request that asked for THAT one — a programming CSV arriving for an activation
    // read is a crossed reply, not a large one.
    const exportOperation = 'value' in r && 'operation' in r.value &&
      (r.value.operation === 'activationExport' || r.value.operation === 'programExport')
      ? r.value.operation : null
    const exported = exportOperation !== null
    if (bytes > OPERATION_RESPONSE_BYTES && !exported) throw Error('invalidOperation')
    if ('value' in r) {
      if ('stop' in r.value) throw Error('invalidOperation')
      const asked = p.request.type === 'activationExport' || p.request.type === 'programExport'
      if (exported !== asked || (exported && exportOperation !== p.request.type)) throw Error('invalidOperation')
      const expectsOutcome = p.request.type === 'logManual' || p.request.type === 'stationControl' || p.request.type === 'logChange' || p.request.type === 'result'
      if (expectsOutcome !== 'outcome' in r.value) throw new Error('invalidOperation')
      if (
        'outcome' in r.value &&
        r.value.operationId !==
          (p.request.type === 'result' ? p.request.operationId : p.request.requestId)
      )
        throw new Error('invalidOperation')
      if ('outcome' in r.value) {
        const control = p.request.type === 'stationControl' ||
          (p.request.type === 'result' && p.request.operationId === this.view.controlPending?.operationId)
        if (control !== ('operation' in r.value && r.value.operation === 'stationControl')) throw Error('invalidOperation')
        const change = 'operation' in r.value && r.value.operation === 'logChange'
        if ((p.request.type === 'logChange' && !change) || (p.request.type === 'logManual' && 'operation' in r.value)) throw Error('invalidOperation')
      }
    }
    clearTimeout(p.timer)
    this.pending = null
    this.finishBudget(p.request.requestId)
    if (p.request.type === 'activationExport' || p.request.type === 'programExport') {
      // A read changed nothing at the station, so a refusal (busy, not the controller) costs neither
      // this browser's lease token nor its station state; the next heartbeat still decides those.
      this.update({ busy: false })
      if ('error' in r) p.reject(new Error(r.error))
      else p.resolve(r.value)
      return
    }
    if ('error' in r) {
      this.heartbeatLeaseId = null
      this.leaseUntil = 0
      const write = p.request.type === 'logManual' || p.request.type === 'logChange'
      const unknown = write && r.error === 'operationUnknown'
      if (p.request.type === 'stationControl' && r.error !== 'operationUnknown') {
        try { this.controlStorage?.write(null); this.update({ controlPending: null }) } catch {}
      }
      this.update({
        busy: false,
        submitting: false,
        state: null,
        retainedState: null,
        fresh: false,
        error: r.error,
        ...(write
          ? { unresolved: unknown ? p.request.requestId : null }
          : {})
      })
      p.reject(new Error(r.error))
      return
    }
    if ('stop' in r.value) throw Error('invalidOperation')
    if ('phase' in r.value) {
      this.heartbeatLeaseId = r.value.phase === 'controlling' ? r.value.leaseId : null
      // Stop OUTLIVES the lease (operator ruling, 2026-09-15): an unnecessary unkey is a smaller
      // harm than a keyed rig and a Stop button that reported a refusal. The STATION decides — it
      // keeps issuing a current `transmitEpoch` to a browser that held station control after its
      // lease runs out, and stops the moment the grant goes, another browser takes over or the
      // station reboots. So the token is the whole permission here and the lease id it was first
      // issued under is simply carried. Nothing this keeps alive can START anything: every command
      // still requires `phase === 'controlling'` and a live command window.
      if (r.value.phase === 'controlling' && r.value.leaseId) this.stopLeaseId = r.value.leaseId
      this.stopTarget = this.operationVersion >= 4 && this.stopLeaseId && r.value.transmitEpoch
        ? { stationBootId: r.value.stationBootId, leaseId: this.stopLeaseId, transmitEpoch: r.value.transmitEpoch } : null
      this.stateUntil = p.started + Math.min(1200, r.value.leaseRemainingMs ?? 1200)
      this.leaseUntil = r.value.phase === 'controlling' && r.value.leaseRemainingMs != null ? p.started + r.value.leaseRemainingMs : 0
      this.update({
        supported: true,
        busy: false,
        submitting: false,
        state: r.value,
        fresh: this.now() < this.stateUntil,
        error: null
      })
    } else if ('operation' in r.value && r.value.operation === 'logChange') {
      // A change has no editable copy to keep, so only an unknown outcome holds the receipt.
      this.update({
        busy: false,
        submitting: false,
        state: null,
        fresh: false,
        error: null,
        unresolved: r.value.outcome === 'unknown' ? r.value.operationId : null,
        ...(p.request.type === 'result' ? { resolved: r.value } : {})
      })
      this.polledAt = -Infinity
    } else if ('operation' in r.value) {
      // An export reply returned above, so what reaches here is a station control outcome.
      const result = r.value as ControlOutcome
      if (p.request.type === 'logManual') throw Error('invalidOperation')
      const terminal = result.outcome === 'applied' || result.outcome === 'rejected'
      let cleared = false
      if (terminal) {
        try { this.controlStorage?.write(null); cleared = true } catch {}
      }
      const refreshing = result.outcome === 'applied' && (p.request.type === 'stationControl' || !this.view.state)
      if (refreshing) this.controlRefreshUntil = this.now() + 1200
      this.update({ busy: false, submitting: false, controlResult: result, controlError: null,
        controlRefreshing: refreshing,
        ...(cleared ? { controlPending: null } : {}),
        ...(p.request.type === 'stationControl' ? { state: null, fresh: false } : {}), error: null })
      if (p.request.type === 'stationControl') this.polledAt = -Infinity
    } else {
      if (p.request.type === 'stationControl' || p.request.type === 'logChange') throw Error('invalidOperation')
      this.finished = r.value
      const retain =
        r.value.outcome === 'unknown' ||
        (p.request.type === 'result' && r.value.outcome === 'rejected')
      this.update({
        busy: false,
        submitting: false,
        state: null,
        fresh: false,
        error: null,
        // A reopened form has no editable copy of a refused submission. Keep
        // those fields until the operator explicitly checks the station log.
        unresolved: retain ? r.value.operationId : null,
        ...(p.request.type === 'result' ? { resolved: r.value } : {})
      })
      this.polledAt = -Infinity
    }
    p.resolve(r.value)
  }
  async acquire() {
    const s = this.view.state
    if (!s || !this.view.fresh || !s.allowed || s.phase !== 'available')
      throw new Error('localPermissionRequired')
    await this.request({
      type: 'acquire',
      requestId: crypto.randomUUID(),
      stationBootId: s.stationBootId
    })
  }
  async release() {
    const leaseId = this.heartbeatLeaseId
    this.heartbeatLeaseId = null
    this.leaseUntil = 0
    this.update({ state: null, retainedState: null, fresh: false })
    if (!leaseId) return
    try {
      await this.request({ type: 'release', requestId: crypto.randomUUID(), leaseId })
    } catch {}
  }
  /** Control held (connected, the latest state shows this browser controlling) but past its freshness
   * window: the brief lapse a command may wait out. Anything else keeps its own refusal. */
  private lapsed(): boolean {
    return !this.view.fresh && this.view.connected && this.view.state?.phase === 'controlling'
  }
  /** A gesture committed during a brief control lapse is sent only once control is current again.
   * Resolves at once while current. Otherwise waits, at most CONTROL_RESUME_MS, for a later state
   * reply to make it current, and refuses as not sent (nothing left the browser) if control is still
   * stale then, or the lease or connection ends first. The command path still requires fresh state. */
  awaitCurrent(maxMs = CONTROL_RESUME_MS): Promise<void> {
    const current = () => this.view.connected && this.view.fresh && this.view.state?.phase === 'controlling'
    const held = () => this.view.connected && this.view.state?.phase === 'controlling'
    if (current()) return Promise.resolve()
    if (!held()) return Promise.reject(new OperationFailure('notController', false))
    return new Promise((resolve, reject) => {
      const finish = () => {
        clearTimeout(timer)
        unsubscribe()
        if (current()) resolve()
        else reject(new OperationFailure('notController', false))
      }
      const timer = setTimeout(finish, maxMs)
      const unsubscribe = this.subscribe(() => { if (current() || !held()) finish() })
    })
  }
  private waitForHeartbeat(until: number): Promise<void> {
    if (!this.pending) return Promise.resolve()
    if (this.pending.request.type !== 'heartbeat') return Promise.reject(new Error('remoteBusy'))
    return new Promise((resolve, reject) => {
      const finish = (error?: string) => {
        clearTimeout(timer)
        unsubscribe()
        error ? reject(new Error(error)) : resolve()
      }
      const timer = setTimeout(
        () => finish('windowExpired'),
        Math.max(0, Math.min(500, until - this.now()))
      )
      const unsubscribe = this.subscribe(() => {
        if (!this.view.connected) finish('stationUnavailable')
        else if (!this.pending) finish()
      })
    })
  }
  private withReceiptLock<T>(action: () => T | Promise<T>): Promise<T> {
    return this.receiptStorage?.exclusive
      ? this.receiptStorage.exclusive(action)
      : Promise.resolve(action())
  }
  /** Reserve the request slot before an asynchronous browser receipt lock.
   * Background status polling must not consume the operator's result gesture.
   * This admits only a receipt read; it never replays a station action. */
  private async withResultIntent<T>(read: (current: () => void) => Promise<T>): Promise<T> {
    if (this.pending || this.resultIntent) throw Error('remoteBusy')
    if (!this.view.connected) throw Error('stationUnavailable')
    const intent = {}
    this.resultIntent = intent
    this.update({ busy: true })
    try {
      return await read(() => {
        if (this.resultIntent !== intent || !this.view.connected) throw Error('stationUnavailable')
      })
    } finally {
      if (this.resultIntent === intent) {
        this.resultIntent = null
        this.update({ busy: !!this.pending })
      }
    }
  }
  async log(record: ManualRecord, onSubmitted?: (id: string) => void): Promise<OperationOutcome> {
    const attempt = { sent: false }
    try {
      // The lapse rule of controlFrom: the entry as submitted, sent once control is current again.
      const entry = structuredClone(record)
      if (this.lapsed()) await this.awaitCurrent()
      return await this.submitLog(entry, onSubmitted, attempt)
    } catch (error) {
      const code = error instanceof Error ? error.message : 'stationUnavailable'
      throw new OperationFailure(code, attempt.sent, stationWasBusy(code, attempt.sent))
    }
  }
  private async submitLog(record: ManualRecord, onSubmitted: ((id: string) => void) | undefined, attempt: { sent: boolean }): Promise<OperationOutcome> {
    const draft = structuredClone(manualRecord(record))
    const r = await this.submitWrite(s => s.actions.includes('log.manual'),
      (intent, requestId) => ({ type: 'logManual', requestId, ...intent, record: draft }), onSubmitted, attempt)
    if (!('outcome' in r) || 'operation' in r) throw new Error('invalidRequest')
    return r
  }
  /** Change an existing log row, or the station's log context, under the same lease, window,
   * logging permission and receipt rules as a manual entry. */
  async change(change: LogChange): Promise<LogChangeOutcome> {
    const attempt = { sent: false }
    try {
      const intent = structuredClone(logChange(change))
      if (this.operationVersion < 4) throw Error('stationUnsupported')
      const r = await this.submitWrite(s => logChangeCapabilities(intent).every(c => !!s.controls?.capabilities.includes(c)),
        (base, requestId) => ({ type: 'logChange', requestId, ...base, change: intent }), undefined, attempt)
      if (!('operation' in r) || r.operation !== 'logChange') throw Error('invalidRequest')
      return r
    } catch (error) {
      const code = error instanceof Error ? error.message : 'stationUnavailable'
      throw new OperationFailure(code, attempt.sent, stationWasBusy(code, attempt.sent))
    }
  }
  /** Read the station's activations (no selection) or one chunk of one activation file, under this
   * browser's logging lease. A read: it spends no command sequence and changes nothing at the station.
   * Sent only to a station that offers it, because an older desktop cannot parse the request at all.
   * The offer is read from the last station state, kept through a command's re-read; the station
   * decides each read against its live grant and lease. */
  async activationExport(selection: ActivationSelection | null, index = 0): Promise<ActivationExportValue> {
    const attempt = { sent: false }
    try {
      if (this.operationVersion < 4) throw Error('stationUnsupported')
      const shown = this.view.state ?? this.view.retainedState
      if (!this.view.connected || !this.heartbeatLeaseId || shown?.phase !== 'controlling' ||
        !shown.controls?.capabilities.includes('activationExport')) throw Error('notController')
      const value = await this.request({ type: 'activationExport', requestId: crypto.randomUUID(), stationBootId: shown.stationBootId,
        leaseId: this.heartbeatLeaseId, selection: selection && structuredClone(activationSelection(selection)), index },
      () => { attempt.sent = true })
      if (!('operation' in value) || value.operation !== 'activationExport') throw Error('invalidOperation')
      return value
    } catch (error) {
      const code = error instanceof Error ? error.message : 'stationUnavailable'
      throw new OperationFailure(code, attempt.sent, stationWasBusy(code, attempt.sent))
    }
  }
  /** Read one chunk of the station's working channel list as a CHIRP or spreadsheet CSV, under
   * station control and this browser's lease. A read, exactly like `activationExport`: no command
   * sequence, nothing written, and only to a station that advertises the hint. */
  async programExport(format: ProgramExportFormat, nameCap: number, index = 0): Promise<ProgramExportValue> {
    const attempt = { sent: false }
    try {
      if (this.operationVersion < 4) throw Error('stationUnsupported')
      const shown = this.view.state ?? this.view.retainedState
      if (!this.view.connected || !this.heartbeatLeaseId || shown?.phase !== 'controlling' ||
        !shown.controls?.capabilities.includes('programExport')) throw Error('notController')
      const value = await this.request({ type: 'programExport', requestId: crypto.randomUUID(), stationBootId: shown.stationBootId,
        leaseId: this.heartbeatLeaseId, format, nameCap, index },
      () => { attempt.sent = true })
      if (!('operation' in value) || value.operation !== 'programExport') throw Error('invalidOperation')
      return value
    } catch (error) {
      const code = error instanceof Error ? error.message : 'stationUnavailable'
      throw new OperationFailure(code, attempt.sent, stationWasBusy(code, attempt.sent))
    }
  }
  private async submitWrite(
    allowed: (s: OperationState) => boolean,
    build: (intent: { stationBootId: string; leaseId: string; commandWindowId: string; expectedRevision: number; clientSequence: number }, requestId: string) => OperationRequest,
    onSubmitted: ((id: string) => void) | undefined,
    attempt: { sent: boolean }
  ): Promise<OperationValue> {
    const s = this.view.state,
      until = this.stateUntil
    if (this.view.unresolved) throw new Error('operationUnknown')
    if (this.view.controlPending || this.controlIntent) throw new Error('operationUnknown')
    if (this.loggingIntent) throw new Error('remoteBusy')
    if (
      !s ||
      !this.view.fresh ||
      s.phase !== 'controlling' ||
      !s.leaseId ||
      !s.commandWindowId ||
      s.nextSequence === null || !allowed(s)
    )
      throw new Error('notController')
    const intent = {
      stationBootId: s.stationBootId,
      leaseId: s.leaseId,
      commandWindowId: s.commandWindowId,
      expectedRevision: s.revision,
      clientSequence: s.nextSequence
    }
    this.loggingIntent = true
    try {
      return await this.withReceiptLock(async () => {
        // An automatic heartbeat can begin between pointer-down and click. Wait at
        // most half a second for that READ; retain the click's original context,
        // window and payload. Never let the gesture migrate to a new station state.
        if (this.pending) await this.waitForHeartbeat(until)
        if (this.now() >= until) throw new Error('windowExpired')
        if (this.view.state?.leaseId !== s.leaseId || this.view.state.revision !== s.revision)
          throw new Error('staleContext')
        this.requireRequestCapacity()
        const requestId = crypto.randomUUID()
        onSubmitted?.(requestId)
        return this.request(build(intent, requestId), () => { attempt.sent = true })
      })
    } finally {
      this.loggingIntent = false
    }
  }
  async resolve(): Promise<OperationOutcome | LogChangeOutcome> {
    const id = this.view.unresolved
    if (!id) throw new Error('resultExpired')
    return this.withResultIntent(current => this.withReceiptLock(async () => {
      current()
      if (this.view.unresolved !== id) throw Error('resultExpired')
      const r = await this.request({
        type: 'result',
        requestId: crypto.randomUUID(),
        operationId: id
      })
      if (!('outcome' in r) || ('operation' in r && r.operation !== 'logChange')) throw new Error('invalidRequest')
      return r
    }))
  }
  async acknowledgeAfterCheckingLog() {
    const id = this.view.unresolved
    if (this.view.busy || !id) return
    return this.withReceiptLock(() => {
      if (this.view.busy || this.view.unresolved !== id) return
      this.update({ dismissed: id, unresolved: null, error: null })
    })
  }
  getLastOutcome() {
    return this.finished
  }
  /** A coalesced gesture keeps its original authority while its target is being
   * formed. The returned sender cannot borrow a newer heartbeat or lease. */
  prepareControl(displayed?: ControlContext): (action: StationAction) => Promise<ControlOutcome> {
    if (!this.view.state || !this.view.fresh) throw new OperationFailure('notController', false)
    const captured = { state: structuredClone(this.view.state), until: this.stateUntil }
    const context = displayed && structuredClone(displayed)
    return action => this.controlFrom(action, context, captured)
  }
  async control(action: StationAction, displayed?: ControlContext): Promise<ControlOutcome> {
    return this.controlFrom(action, displayed)
  }
  private async controlFrom(action: StationAction, displayed?: ControlContext, captured?: CapturedControl): Promise<ControlOutcome> {
    const attempt = { sent: false }
    this.update({ controlError: null })
    try {
      // A command made during a brief control lapse waits for current control and is sent once, or is
      // refused as not sent (operator decision 2026-09-14); executeControl still requires fresh state.
      // Never a captured gesture (it keeps the window it was prepared with) and never a transmit action
      // (it carries a transmit epoch): those are refused at once, exactly as before.
      if (!captured && !('transmitEpoch' in action) && this.lapsed()) await this.awaitCurrent()
      return await this.executeControl(action, displayed, captured, attempt)
    }
    catch (error) {
      const code = error instanceof Error ? error.message : 'stationUnavailable'
      const busy = stationWasBusy(code, attempt.sent)
      this.update({ controlError: { code, sent: attempt.sent, busy } })
      throw new OperationFailure(code, attempt.sent, busy)
    }
  }
  private async executeControl(action: StationAction, displayed: ControlContext | undefined, captured: CapturedControl | undefined, attempt: { sent: boolean }): Promise<ControlOutcome> {
    const s = captured?.state ?? this.view.state, until = captured?.until ?? this.stateUntil, intent = structuredClone(stationAction(action))
    if (this.operationVersion < controlVersion(intent)) throw Error('stationUnsupported')
    if (!this.controlStorage) throw Error('receiptStorageUnavailable')
    if (this.view.unresolved || this.view.controlPending || this.controlIntent || this.loggingIntent) throw Error('operationUnknown')
    const capability = actionCapability(intent)
    if ('transmitEpoch' in intent && intent.transmitEpoch !== s?.transmitEpoch) throw Error('staleContext')
    if (!s || !this.view.fresh || s.phase !== 'controlling' || !s.leaseId || !s.commandWindowId || s.nextSequence === null || !s.controls?.capabilities.includes(capability)) throw Error('notController')
    const context = structuredClone(controlContext(displayed ?? s.controls.context))
    const sameConnection = (current: ControlContext | undefined) => !!current &&
      context.radioId === current.radioId && context.radioConnection === current.radioConnection && context.ampConnection === current.ampConnection
    if (!sameConnection(s.controls.context)) throw Error('staleContext')
    const request: OperationRequest = { type: 'stationControl', requestId: crypto.randomUUID(), stationBootId: s.stationBootId,
      leaseId: s.leaseId, expectedRevision: s.revision, commandWindowId: s.commandWindowId, clientSequence: s.nextSequence,
      context, action: intent }
    this.controlIntent = true
    try {
      const first = await this.controlStorage.exclusive(async () => {
        if (this.pending) await this.waitForHeartbeat(until)
        if (this.now() >= until) throw Error('windowExpired')
        if (this.view.state?.leaseId !== s.leaseId || this.view.state.revision !== s.revision) throw Error('staleContext')
        if (!sameConnection(this.view.state.controls?.context)) throw Error('staleContext')
        if ('transmitEpoch' in intent && (intent.transmitEpoch !== this.view.state.transmitEpoch || intent.transmitEpoch !== this.stopTarget?.transmitEpoch)) throw Error('staleContext')
        this.requireRequestCapacity()
        const saved = { operationId: request.requestId, action: intent }
        this.controlStorage!.write(saved)
        this.update({ controlPending: saved, controlResult: null })
        const result = await this.request(request, () => { attempt.sent = true })
        if (!('operation' in result) || result.operation !== 'stationControl') throw Error('invalidOperation')
        return result
      })
      if (first.outcome !== 'pending') return first
      return await new Promise<ControlOutcome>((resolve, reject) => {
        const finish = (result?: ControlOutcome) => { clearTimeout(timer); unsubscribe(); result ? resolve(result) : reject(Error('operationUnknown')) }
        const timer = setTimeout(() => finish(), 7500)
        const check = () => {
          const r = this.view.controlResult
          if (r?.operationId === request.requestId && r.outcome !== 'pending') finish(r)
          else if (!this.view.connected) finish()
        }
        const unsubscribe = this.subscribe(check)
        check()
      })
    } finally { this.controlIntent = false }
  }
  async refreshControl(): Promise<ControlOutcome> {
    const entry = this.view.controlPending
    if (!entry || !this.controlStorage) throw Error('resultExpired')
    return this.withResultIntent(current => this.controlStorage!.exclusive(async () => {
      current()
      if (this.view.controlPending?.operationId !== entry.operationId) throw Error('resultExpired')
      const result = await this.request({ type: 'result', requestId: crypto.randomUUID(), operationId: entry.operationId })
      if (!('operation' in result) || result.operation !== 'stationControl') throw Error('invalidOperation')
      return result
    }))
  }
  async acknowledgeControl() {
    if (this.view.busy || !this.view.controlPending || !this.controlStorage) return
    await this.controlStorage.exclusive(() => {
      if (this.view.busy) throw Error('remoteBusy')
      this.controlStorage!.write(null)
      this.update({ controlPending: null, controlResult: null, controlError: null, error: null })
    })
  }
}
