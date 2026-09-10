import type { ReceiptStorage } from './operation-storage'
import {
  manualRecord,
  operationId,
  operationRequest,
  operationResponse,
  type ManualRecord,
  type OperationOutcome,
  type OperationRequest,
  type OperationState
} from './operation-protocol'
export type OperationView = {
  supported: boolean
  state: OperationState | null
  fresh: boolean
  connected: boolean
  busy: boolean
  submitting: boolean
  unresolved: string | null
  pendingDraft: ManualRecord | null
  resolved: OperationOutcome | null
  dismissed: string | null
  error: string | null
}
type Pending = {
  request: OperationRequest
  started: number
  timer: ReturnType<typeof setTimeout>
  resolve: (value: OperationState | OperationOutcome) => void
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
    error: null
  }
  private pending: Pending | null = null
  private timer: ReturnType<typeof setInterval> | undefined
  private stateUntil = 0
  private polledAt = 0
  private finished: OperationOutcome | null = null
  private loggingIntent = false
  constructor(
    private send: (message: string) => void,
    readonly enabled: boolean,
    private now = () => performance.now(),
    private receiptStorage?: ReceiptStorage
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
        this.receiptStorage?.write(value.unresolved ?? null, value.pendingDraft ?? undefined)
      } catch (error) {
        if (value.unresolved) throw error
      }
    }
    this.view = {
      ...this.view,
      ...value,
      ...(value.unresolved === null ? { pendingDraft: null } : {})
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
    const p = this.pending
    this.pending = null
    if (p) {
      clearTimeout(p.timer)
      if (p.request.type === 'logManual') this.update({ unresolved: p.request.requestId })
      p.reject(
        new Error(p.request.type === 'logManual' ? 'operationUnknown' : 'stationUnavailable')
      )
    }
    this.stateUntil = 0
    this.update({ state: null, fresh: false, connected: false, busy: false, submitting: false })
  }
  private tick() {
    const now = this.now(),
      fresh = !!this.view.state && now < this.stateUntil
    if (fresh !== this.view.fresh) this.update({ fresh })
    if (
      !this.view.connected ||
      this.pending ||
      this.loggingIntent ||
      this.view.error === 'stationUnsupported' ||
      now - this.polledAt < 1000
    )
      return
    this.polledAt = now
    const s = this.view.state
    // Hidden pages cannot retain a control lease. Expiry is also enforced by the
    // native monotonic clock when a browser suspends timers altogether.
    if (typeof document !== 'undefined' && document.hidden && s?.phase === 'controlling') {
      void this.release()
      return
    }
    const request: OperationRequest =
      s?.phase === 'controlling' && s.leaseId
        ? { type: 'heartbeat', requestId: crypto.randomUUID(), leaseId: s.leaseId }
        : { type: 'state', requestId: crypto.randomUUID() }
    void this.request(request).catch(() => {})
  }
  private request(request: OperationRequest): Promise<OperationState | OperationOutcome> {
    if (!this.view.connected) return Promise.reject(new Error('stationUnavailable'))
    if (this.pending) return Promise.reject(new Error('remoteBusy'))
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
          const mutation = request.type === 'logManual'
          this.update({
            busy: false,
            submitting: false,
            state: null,
            fresh: false,
            error: mutation ? 'operationUnknown' : 'stationUnavailable',
            ...(mutation ? { unresolved: request.requestId } : {})
          })
          reject(new Error(mutation ? 'operationUnknown' : 'stationUnavailable'))
        }, 7500)
      }
      if (request.type === 'logManual') {
        try {
          this.update({
            unresolved: request.requestId,
            pendingDraft: structuredClone(request.record)
          })
        } catch {
          clearTimeout(p.timer)
          reject(new Error('receiptStorageUnavailable'))
          return
        }
      }
      this.pending = p
      this.update({ busy: true, submitting: request.type === 'logManual', error: null })
      try {
        this.send(JSON.stringify({ type: 'operationRequest', request }))
      } catch {
        clearTimeout(p.timer)
        this.pending = null
        this.update({
          busy: false,
          submitting: false,
          state: null,
          fresh: false,
          error: 'stationUnavailable',
          ...(request.type === 'logManual' ? { unresolved: null } : {})
        })
        reject(new Error('stationUnavailable'))
      }
    })
  }
  receive(raw: unknown) {
    const r = operationResponse(raw),
      p = this.pending
    if (!p || p.request.requestId !== r.requestId) return
    if ('value' in r) {
      const expectsOutcome = p.request.type === 'logManual' || p.request.type === 'result'
      if (expectsOutcome !== 'outcome' in r.value) throw new Error('invalidOperation')
      if (
        'outcome' in r.value &&
        r.value.operationId !==
          (p.request.type === 'result' ? p.request.operationId : p.request.requestId)
      )
        throw new Error('invalidOperation')
    }
    clearTimeout(p.timer)
    this.pending = null
    if ('error' in r) {
      const unknown = p.request.type === 'logManual' && r.error === 'operationUnknown'
      this.update({
        busy: false,
        submitting: false,
        state: null,
        fresh: false,
        error: r.error,
        ...(p.request.type === 'logManual'
          ? { unresolved: unknown ? p.request.requestId : null }
          : {})
      })
      p.reject(new Error(r.error))
      return
    }
    if ('phase' in r.value) {
      this.stateUntil = p.started + Math.min(1200, r.value.leaseRemainingMs ?? 1200)
      this.update({
        supported: true,
        busy: false,
        submitting: false,
        state: r.value,
        fresh: this.now() < this.stateUntil,
        error: null
      })
    } else {
      this.finished = r.value
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
    const leaseId = this.view.state?.leaseId
    this.update({ state: null, fresh: false })
    if (!leaseId) return
    try {
      await this.request({ type: 'release', requestId: crypto.randomUUID(), leaseId })
    } catch {}
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
  async log(record: ManualRecord, onSubmitted?: (id: string) => void): Promise<OperationOutcome> {
    const s = this.view.state,
      until = this.stateUntil,
      draft = structuredClone(manualRecord(record))
    if (this.view.unresolved) throw new Error('operationUnknown')
    if (this.loggingIntent) throw new Error('remoteBusy')
    if (
      !s ||
      !this.view.fresh ||
      s.phase !== 'controlling' ||
      !s.leaseId ||
      !s.commandWindowId ||
      s.nextSequence === null
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
        const requestId = crypto.randomUUID()
        onSubmitted?.(requestId)
        const r = await this.request({
          type: 'logManual',
          requestId,
          ...intent,
          record: draft
        })
        if (!('outcome' in r)) throw new Error('invalidRequest')
        return r
      })
    } finally {
      this.loggingIntent = false
    }
  }
  async resolve(): Promise<OperationOutcome> {
    const id = this.view.unresolved
    if (!id) throw new Error('resultExpired')
    return this.withReceiptLock(async () => {
      if (this.view.unresolved !== id) throw Error('resultExpired')
      const r = await this.request({
        type: 'result',
        requestId: crypto.randomUUID(),
        operationId: id
      })
      if (!('outcome' in r)) throw new Error('invalidRequest')
      return r
    })
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
}
