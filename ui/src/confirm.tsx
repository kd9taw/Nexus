/** In-app confirmation for destructive actions.
 *
 * WHY THIS EXISTS. `window.confirm` is INERT in this webview. A JS confirm only appears if the
 * host implements WKWebView's `runJavaScriptConfirmPanel`; wry does not, so `confirm()` returns
 * `false` immediately and shows nothing. Every guard written as
 *
 *     if (!window.confirm('…')) return
 *
 * therefore returns instantly, and the action silently does nothing at all. Reported by the
 * operator on 2026-08-14 for Remove radio, then confirmed on Reset: no dialog, no effect, no
 * error. Fifteen destructive actions were built on it.
 *
 * WHY THE SUITE MISSED IT. Not, as first written here, because tests mocked `window.confirm` to
 * return `true` — nothing in the suite mocks it at all. It is simpler and worse: none of the
 * converted paths had ANY coverage. A guard nothing exercises cannot fail, whatever it returns.
 * `confirm.test.tsx` pins this primitive; `SettingsPanel.removeradio.test.tsx` pins one whole
 * path end to end, which is what would actually have caught it.
 *
 * THE LAST TWO ARE NOW CONVERTED (2026-09-18, operator sign-off: "fix the macOS confirms").
 * SetupHealth's Prove TX and SstvView's ISS 145.800 guard were held back because both are
 * transmit-path and moving them is a TX-behaviour change, not a UI pass. They sat for 32 days,
 * and the cost was exactly as predicted here: both read the inert `false` as "no", so each
 * BLOCKED its action — nothing ever keyed unasked, but on macOS Prove TX was a DEAD BUTTON and
 * SSTV could not be sent on 145.800 at all. Failing closed is what made a 32-day-old shipped
 * defect merely useless instead of dangerous, which is the argument for this file's default.
 * ⛔ NO `window.confirm` REMAINS in `ui/src`. Adding one back reintroduces the whole bug: it
 * shows nothing, returns false, and the action silently does not happen.
 *
 * FAIL CLOSED. If the host is not mounted, `confirmDialog` resolves FALSE — a destructive action
 * must never proceed unconfirmed. The opposite default would turn a missing dialog into a silent
 * deletion, which is the worse half of the same bug.
 */
import { useEffect, useRef, useState } from 'react'
import { Dialog } from './components/ui/Dialog'

export interface ConfirmOptions {
  /** The question, as a title. Say what will happen, not "Are you sure?". */
  title: string
  /** What the operator needs to know before answering — scope, and what is NOT affected. */
  body?: string
  /** Label for the affirmative button. Name the ACT ("Remove radio"), never "OK". */
  confirmLabel?: string
  cancelLabel?: string
  /** Style the affirmative button as destructive. */
  danger?: boolean
}

interface Request extends ConfirmOptions {
  resolve: (ok: boolean) => void
  /** The control the question was asked from (focused when `confirmDialog` was called). */
  opener: HTMLElement | null
}

let present: ((r: Request) => void) | null = null

/** Ask the operator to confirm. Resolves true only on an explicit yes. */
export function confirmDialog(opts: ConfirmOptions): Promise<boolean> {
  if (!present) {
    // No host mounted. Refuse rather than proceed: see FAIL CLOSED above.
    console.error('confirmDialog: no <ConfirmHost/> mounted — refusing the action')
    return Promise.resolve(false)
  }
  const active = document.activeElement
  const opener = active instanceof HTMLElement && active !== document.body ? active : null
  return new Promise<boolean>((resolve) => present?.({ ...opts, resolve, opener }))
}

// THE KEYBOARD GOES BACK TO THE CONTROL THAT ASKED — the standard pattern for a dialog, which this
// one missed: it is opened by code, and Radix returns focus to a Radix trigger only, so every
// question closed with the keyboard on the page itself. A keyboard or screen-reader operator then
// had to find their place again from the top. And if a YES removes that control (a delete), the
// keyboard goes to the same control in the item that takes its place — the next one, else the one
// before — or to the list itself when no item is left. A control the yes only disables (Send while
// sending) hands the keyboard to the group it sits in, never to another control: focus that lands
// on a button is one Enter from pressing it, and a transmit control is not a place to be put.

/** Where the keyboard goes back to, planned as the question closes, before its answer is acted on. */
interface Return {
  opener: HTMLElement
  /** The same control on the other items of the opener's list, in order, the opener among them. */
  peers: HTMLElement[]
  /** The opener's ancestors, nearest first: the list to fall back on when no item is left. */
  ancestors: HTMLElement[]
  /** Did a yes close the question? Only a yes acts, so only a yes can take the opener away. */
  yes: boolean
}

const kindOf = (el: Element) => `${el.tagName}.${[...el.classList].sort().join('.')}`

function planReturn(opener: HTMLElement, yes: boolean): Return {
  const kind = kindOf(opener)
  const ancestors: HTMLElement[] = []
  let peers: HTMLElement[] = []
  for (let a = opener.parentElement; a && a !== document.body; a = a.parentElement) {
    ancestors.push(a)
    if (peers.length === 0) {
      const same = [...a.querySelectorAll<HTMLElement>(opener.tagName)].filter((el) => kindOf(el) === kind)
      if (same.length > 1) peers = same
    }
  }
  return { opener, peers, ancestors, yes }
}

const usable = (el: HTMLElement) => el.isConnected && !el.matches(':disabled') && !el.closest('[inert]')

/** Focus `el` without the browser panning every scroller above it (useRovingList's reason). */
function tryFocus(el: HTMLElement): boolean {
  if (!usable(el)) return false
  el.focus({ preventScroll: true })
  if (document.activeElement !== el) return false
  el.scrollIntoView?.({ block: 'nearest' })
  return true
}

/** The opener is gone, or cannot take the keyboard: the place that takes its place. */
function handOn(plan: Return) {
  if (!plan.opener.isConnected) {
    const at = plan.peers.indexOf(plan.opener)
    for (const el of [...plan.peers.slice(at + 1), ...plan.peers.slice(0, Math.max(0, at)).reverse()])
      if (tryFocus(el)) return
  }
  for (const a of plan.ancestors) {
    if (!a.isConnected) continue
    // A place to be, not a stop: focusable for this moment only, as it was once the keyboard leaves.
    const lent = !a.hasAttribute('tabindex')
    if (lent) a.setAttribute('tabindex', '-1')
    if (tryFocus(a)) {
      if (lent) a.addEventListener('blur', () => a.removeAttribute('tabindex'), { once: true })
      return
    }
    if (lent) a.removeAttribute('tabindex')
  }
}

/** Once a yes has closed the question: should acting on it remove or disable the opener while it
 *  still has the keyboard, hand it on. Watches until the keyboard moves anywhere else, or 30 s. */
function followAnswer(plan: Return) {
  let done = false
  const stop = () => {
    done = true
    watch.disconnect()
    clearTimeout(timer)
    document.removeEventListener('focusin', moved, true)
  }
  const check = () => {
    if (done || usable(plan.opener)) return
    const active = document.activeElement
    if (active && active !== document.body && active !== plan.opener) return stop() // someone moved it on
    stop()
    handOn(plan)
  }
  const moved = (e: FocusEvent) => {
    if (e.target !== plan.opener) stop()
  }
  const watch = new MutationObserver(check)
  watch.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['disabled'] })
  document.addEventListener('focusin', moved, true)
  const timer = setTimeout(stop, 30_000)
}

function giveBack(plan: Return) {
  if (tryFocus(plan.opener)) {
    if (plan.yes) followAnswer(plan)
  } else handOn(plan)
}

/** Mount ONCE, near the app root. */
export function ConfirmHost() {
  const [req, setReq] = useState<Request | null>(null)
  // The same request the state holds, kept where a callback can settle it without reading
  // through a render. Resolving inside a `setReq` updater would be a side effect in a function
  // React is free to call twice (StrictMode), and `close` reading `req` would answer whichever
  // request that closure captured rather than the one on screen.
  const pending = useRef<Request | null>(null)
  /** Planned by `close`, carried out once the dialog has let go of the keyboard. */
  const returning = useRef<Return | null>(null)

  useEffect(() => {
    present = (next) => {
      // A second question arriving while one is open REPLACES it, so the one being replaced must
      // be answered on its way out or its `await` never settles and the caller waits for the
      // lifetime of the window. Answered NO: the operator never saw it, and an unseen
      // destructive question is a refusal. Reachable from the fire-and-forget call sites in
      // RadioProgView, which do not await one another.
      // …and the keyboard goes back to where the FIRST was asked: the operator never left it.
      const first = pending.current
      first?.resolve(false)
      pending.current = first ? { ...next, opener: first.opener } : next
      setReq(pending.current)
    }
    return () => {
      present = null
    }
  }, [])

  const close = (ok: boolean) => {
    const opener = pending.current?.opener
    // Planned now, while the page is as the question found it — before the answer is acted on.
    returning.current = opener ? planReturn(opener, ok) : null
    pending.current?.resolve(ok)
    pending.current = null
    setReq(null)
  }

  return (
    <Dialog
      open={req !== null}
      // ESC and overlay-click land here: anything that is not an explicit yes is a no.
      onOpenChange={(open) => {
        if (!open) close(false)
      }}
      title={req?.title ?? ''}
      description={req?.body}
      onCloseAutoFocus={(e) => {
        // Radix would focus a trigger this dialog does not have — which is to say, nothing.
        e.preventDefault()
        const plan = returning.current
        returning.current = null
        if (plan) giveBack(plan)
      }}
    >
      <div className="confirm-actions">
        <button type="button" className="settings-refresh" onClick={() => close(false)} autoFocus>
          {req?.cancelLabel ?? 'Cancel'}
        </button>
        <button
          type="button"
          className={req?.danger ? 'settings-save danger' : 'settings-save'}
          onClick={() => close(true)}
        >
          {req?.confirmLabel ?? 'Confirm'}
        </button>
      </div>
    </Dialog>
  )
}
