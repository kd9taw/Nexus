// THE KEYBOARD GOES BACK TO THE CONTROL A DIALOG WAS OPENED FROM — the standard pattern, which the
// app's dialogs missed: they are opened by code, and Radix returns focus to a Radix TRIGGER only, so
// each one closed with the keyboard on the page itself, and a keyboard or screen-reader operator had
// to find their place again from the top. The confirm question (confirm.tsx) and every dialog on
// ui/Dialog (`useFocusReturn`, below) give it back.
//
// Where it goes: the control that had the keyboard when the dialog opened — for an item of a menu,
// the menu's own button, since the item goes with the menu. If that control is gone (a delete, a
// list refreshed under it), to the same control on the item that takes its place — the next one,
// else the one before — or to the group it sat in (focusable for that moment only: the tabindex is
// lent, and taken back on blur); and, when nothing of where it was is left, to the view itself
// (`main`) — never the page. A control that is only DISABLED hands the keyboard to its group, never
// to another control: a focused button is one Enter from being pressed, and a transmit control is
// not a place to be put.
//
// Nothing here waits or repeats unless asked: only the confirm question watches, after a yes, for
// the yes removing the control that asked (30 s at most, and not once the keyboard has moved on).
// A dialog's own return is one synchronous focus, inside the close Radix already schedules.
import { useCallback, useLayoutEffect, useRef, useState } from 'react'

/** Where the keyboard goes back to, planned as a dialog closes (before its answer is acted on). */
export interface FocusReturn {
  opener: HTMLElement
  /** The same control on the other items of the opener's list, in order, the opener among them. */
  peers: HTMLElement[]
  /** The opener's ancestors, nearest first: the group to fall back on. */
  ancestors: HTMLElement[]
  /** Watch, after the return, for acting on the answer taking the opener away (the confirm's yes). */
  follow: boolean
}

/** The control the keyboard is on now, or null — the page itself is no control. */
export function focusedControl(): HTMLElement | null {
  const active = document.activeElement
  if (!(active instanceof HTMLElement) || active === document.body) return null
  // An item of a menu goes with the menu: the control is the menu's own button, which the menu
  // names as its label.
  const menu = active.closest('[role="menu"]')
  const button = menu ? document.getElementById(menu.getAttribute('aria-labelledby') ?? '') : null
  return button instanceof HTMLElement ? button : active
}

const kindOf = (el: Element) => `${el.tagName}.${[...el.classList].sort().join('.')}`

export function planReturn(opener: HTMLElement, follow: boolean): FocusReturn {
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
  return { opener, peers, ancestors, follow }
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

/** A place to be, not a stop: a group made focusable for this moment only. */
function lend(el: HTMLElement): boolean {
  const lent = !el.hasAttribute('tabindex')
  if (lent) el.setAttribute('tabindex', '-1')
  if (tryFocus(el)) {
    if (lent) el.addEventListener('blur', () => el.removeAttribute('tabindex'), { once: true })
    return true
  }
  if (lent) el.removeAttribute('tabindex')
  return false
}

/** The opener is gone, or cannot take the keyboard: the place that takes its place. */
function handOn(plan: FocusReturn) {
  if (!plan.opener.isConnected) {
    const at = plan.peers.indexOf(plan.opener)
    for (const el of [...plan.peers.slice(at + 1), ...plan.peers.slice(0, Math.max(0, at)).reverse()])
      if (tryFocus(el)) return
  }
  for (const a of plan.ancestors) if (a.isConnected && lend(a)) return
  // Nothing of where it was is left (the view changed under it): the view itself.
  const view = document.querySelector('main')
  if (view instanceof HTMLElement && !plan.ancestors.includes(view)) lend(view)
}

/** Once a yes has closed the question: should acting on it remove or disable the opener while it
 *  still has the keyboard, hand it on. Watches until the keyboard moves anywhere else, or 30 s. */
function followAnswer(plan: FocusReturn) {
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

export function giveBack(plan: FocusReturn) {
  if (tryFocus(plan.opener)) {
    if (plan.follow) followAnswer(plan)
  } else handOn(plan)
}

/**
 * A dialog on ui/Dialog: pass the result as its `onCloseAutoFocus`, and the keyboard goes back to
 * the control it was opened from. That control is noted in the render that opens the dialog, before
 * anything inside it — an autoFocus, or Radix itself — can have taken the keyboard. (Today Radix's
 * portal mounts the dialog a render later, so an effect would still see it; the render is the place
 * that does not depend on that.) A dialog opened with the keyboard nowhere closes with it nowhere,
 * as it always did.
 */
export function useFocusReturn(open: boolean): (event: Event) => void {
  const [noted, setNoted] = useState(() => ({ open, opener: open ? focusedControl() : null }))
  if (noted.open !== open) setNoted({ open, opener: open ? focusedControl() : noted.opener })
  const { opener } = noted
  // Its neighbours, planned as the dialog opens: should the control go while the dialog is up (a
  // list refreshed under it), they are still known when it closes.
  const planned = useRef<FocusReturn | null>(null)
  useLayoutEffect(() => {
    if (open && opener) planned.current = planReturn(opener, false)
  }, [open, opener])
  return useCallback(
    (event: Event) => {
      event.preventDefault() // Radix would focus a trigger these dialogs do not have: nothing
      const plan = opener?.isConnected ? planReturn(opener, false) : planned.current
      if (plan) giveBack(plan)
    },
    [opener],
  )
}
