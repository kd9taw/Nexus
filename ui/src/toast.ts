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
  /** An ALERT about the world — a decode, a spot, a band opening, space weather — that the app
   *  raised on its own. With pop-ups off it does not pop up (see `popsUpWhenOff`). */
  alert?: boolean
}

export interface ToastOptions {
  prominent?: boolean
  action?: () => void
  actionLabel?: string
  alert?: boolean
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

/**
 * #391: WHAT STILL POPS UP WITH POP-UPS OFF. InsaneSplash, 2026-09-29: "how do I turn off the popup
 * notifications in the bottom right of the application?" Off takes two kinds of toast out of the
 * corner:
 *  · a CONFIRMATION: a `success` toast with no button that is not prominent ("Logged", "Saved",
 *    "Uploaded to QRZ", a QSY). It confirms what the operator just did; had that failed, the toast
 *    would have been an error.
 *  · an ALERT (`alert`): a decode, a spot, a band opening, space weather, the app telling the
 *    operator about the world. Each also beeps or shows where it happened, and each has its own
 *    switch in Spots & Alerts. An alert drawn in red for loudness (a storm) is still an alert.
 * Everything else still pops up, each on the safe side of a call:
 *  · every ERROR, whatever it is about: the transmit path, the rig, the log or anything else;
 *  · every NOTICE (`info`), because a notice is how a refusal or a change on the transmit path, the
 *    rig or the log is said: "TX locked", "Nothing to log", "TX was turned back on", the voice keyer
 *    stopping an over, an export that left contacts out;
 *  · a PROMINENT toast that is not an alert: the ISS auto-arm retuning the rig, the armed pass
 *    rising, a contest start about to send the wrong exchange;
 *  · a toast with a BUTTON that is not an alert's, because that button is a control: Stop a looping
 *    alarm, Undo a delete, Tune to a net the operator asked to be reminded of, Download an update.
 */
export function popsUpWhenOff(toast: Toast): boolean {
  if (toast.alert) return false
  if (toast.kind !== 'success') return true
  return toast.prominent === true || toast.action != null
}

/** Whether the corner stack shows every toast (the default, today's behaviour) or only what
 *  `popsUpWhenOff` keeps. Each window sets its own from its settings (`setPopupNotifications`). */
let popupsOn = true
const popupListeners = new Set<Listener>()
const popups = (): Toast[] => (popupsOn ? toasts : toasts.filter(popsUpWhenOff))

function emit(): void {
  for (const fn of listeners) fn(toasts)
  const shown = popups()
  for (const fn of popupListeners) fn(shown)
}

/** Every toast raised, whether or not it pops up. */
export function subscribeToasts(fn: Listener): () => void {
  listeners.add(fn)
  fn(toasts)
  return () => {
    listeners.delete(fn)
  }
}

/** The toasts the corner stack shows: every one, or with pop-ups off only what `popsUpWhenOff`
 *  keeps. */
export function subscribePopups(fn: Listener): () => void {
  popupListeners.add(fn)
  fn(popups())
  return () => {
    popupListeners.delete(fn)
  }
}

/** Pop-ups on or off (Settings ▸ Spots & Alerts ▸ Alerts). A toast already up that the switch
 *  hides leaves the stack at once; one that must still pop up stays where it is. */
export function setPopupNotifications(on: boolean): void {
  if (on === popupsOn) return
  popupsOn = on
  const shown = popups()
  for (const fn of popupListeners) fn(shown)
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
