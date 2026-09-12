import { ageFrame, FrameOrder, parseFrame, POLL_MS, STALE_MS } from './protocol'
import type { MonitorFrame, SourceKind } from './protocol'

// A view receives one read capability. It cannot name a command or replace an IPC bridge.
export type MonitorSource = {
  readonly id: string
  readonly kind: SourceKind
  read: (signal: AbortSignal) => Promise<unknown>
}
export type MonitorState = {
  status: 'connecting' | 'current' | 'unavailable' | 'invalid'
  frame: MonitorFrame | null
}
export const initialState: MonitorState = { status: 'connecting', frame: null }

export function startMonitor(
  source: MonitorSource,
  publish: (state: MonitorState) => void,
  now: () => number = () => performance.now(),
): () => void {
  const order = new FrameOrder()
  let state = initialState
  let acceptedAt = -Infinity
  let acceptedFrame: MonitorFrame | null = null
  let requestAge = 0
  let stopped = false
  let pending: { abort: AbortController; started: number } | null = null
  const update = (next: MonitorState) => { state = next; publish(next) }
  const tick = () => {
    if (stopped) return
    if (state.status === 'current' && now() - acceptedAt >= STALE_MS) {
      update({ ...state, status: 'unavailable' })
    } else if (state.status === 'current' && acceptedFrame) {
      update({ ...state, frame: ageFrame(acceptedFrame, requestAge + now() - acceptedAt) })
    }
    if (pending) {
      if (now() - pending.started >= STALE_MS && !pending.abort.signal.aborted) {
        pending.abort.abort()
        update({ ...state, status: 'unavailable' })
      }
      // A native invoke may ignore AbortSignal. Keep the single-flight slot until
      // it settles; timeouts must not accumulate a queue of native work.
      return
    }
    const request = { abort: new AbortController(), started: now() }
    pending = request
    void Promise.resolve().then(() => source.read(request.abort.signal)).then((value) => {
      if (stopped || request.abort.signal.aborted) return
      try {
        const next = parseFrame(value, source.kind)
        if (order.accept(next)) {
          acceptedAt = now()
          acceptedFrame = next
          // The whole request duration is a conservative bound on time in transit.
          requestAge = Math.max(0, acceptedAt - request.started)
          update({ status: 'current', frame: ageFrame(next, requestAge) })
        }
      } catch {
        update({ ...state, status: 'invalid' })
      }
    }, () => {
      if (!stopped && !request.abort.signal.aborted) update({ ...state, status: 'unavailable' })
    }).finally(() => { if (pending === request) pending = null })
  }
  publish(initialState)
  tick()
  const timer = setInterval(tick, POLL_MS)
  return () => {
    stopped = true
    clearInterval(timer)
    pending?.abort.abort()
  }
}
