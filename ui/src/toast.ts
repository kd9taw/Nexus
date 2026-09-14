// Tiny app-wide toast bus.
//
// Dependency-free: components subscribe to receive the current toast list and
// any code path (e.g. an api-call wrapper) can `pushToast(...)` a brief message.
// Used to surface friendly errors instead of failing silently.

export type ToastKind = 'error' | 'info' | 'success'

export interface Toast {
  id: number
  kind: ToastKind
  message: string
  /** Loud/attention styling (filled background + pulse) for the things worth chasing —
   * "someone is calling you", a new DXCC. Routine toasts (QSY, errors) leave it off. */
  prominent?: boolean
  /** Optional one-click action — e.g. "Work" the station this alert is about. When set,
   * the toast shows an action button that runs this then dismisses the toast. */
  action?: () => void
  /** Button label for `action` (default "Work"). */
  actionLabel?: string
}

export interface ToastOptions {
  prominent?: boolean
  action?: () => void
  actionLabel?: string
}

type Listener = (toasts: Toast[]) => void

const DEFAULT_TTL_MS = 4000

/** D#19: an error toast stays at least this long (or until dismissed). At the 4 s default a
 * failure was gone before an operator looking at the rig got back to the screen, and several
 * call sites asked for less than that. A call site may ask for LONGER; `0` still means sticky.
 * Info and success toasts keep whatever ttl they are given. */
export const ERROR_MIN_TTL_MS = 12_000

let nextId = 1
let toasts: Toast[] = []
const listeners = new Set<Listener>()

function emit(): void {
  for (const fn of listeners) fn(toasts)
}

export function subscribeToasts(fn: Listener): () => void {
  listeners.add(fn)
  fn(toasts)
  return () => {
    listeners.delete(fn)
  }
}

export function pushToast(
  message: string,
  kind: ToastKind = 'error',
  ttlMs = DEFAULT_TTL_MS,
  opts: ToastOptions = {},
): number {
  const id = nextId++
  toasts = [...toasts, { id, kind, message, ...opts }]
  emit()
  if (ttlMs > 0) {
    const ttl = kind === 'error' ? Math.max(ttlMs, ERROR_MIN_TTL_MS) : ttlMs
    window.setTimeout(() => dismissToast(id), ttl)
  }
  return id
}

export function dismissToast(id: number): void {
  const next = toasts.filter((t) => t.id !== id)
  if (next.length !== toasts.length) {
    toasts = next
    emit()
  }
}

/**
 * Run an async action and surface a friendly toast if it rejects. Returns the
 * resolved value, or null on failure (so callers can branch without throwing).
 */
export async function withErrorToast<T>(
  action: () => Promise<T>,
  fallbackMessage: string,
): Promise<T | null> {
  try {
    return await action()
  } catch (err) {
    const detail = err instanceof Error ? err.message : typeof err === 'string' ? err : ''
    pushToast(detail ? `${fallbackMessage}: ${detail}` : fallbackMessage, 'error')
    return null
  }
}
