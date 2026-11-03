// Toast rendering via Radix Toast (accessible live-region + dismissal), wired to
// the existing dependency-free toast.ts bus and the --alert-* tokens. The bus
// owns TTL/auto-dismiss; Radix duration is Infinity so it doesn't double-expire.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). A toast's MESSAGE
// and its action label arrive already translated from whoever raised it; what lives here is
// the chrome — the default action word and the close button's name.
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type RefObject } from 'react'
import * as RToast from '@radix-ui/react-toast'
import { t } from '../i18n'
import { dismissToast, keepToast, subscribePopups, type Toast, type ToastKind } from '../toast'
import { getPopupCorner, subscribePopupCorner, type PopupCorner } from '../features/popupCorner'
import { TOAST_MIN_ROOM, toastRoom, type Box } from '../features/toastRoom'
import { TX_AVOID } from '../features/txSurfaces'

/** #445: a toast swipes away toward the side of the window its corner is on. (The body is
 *  click-through, styles.css CLICK-THROUGH, so it is a touch of the toast's own controls' area at most;
 *  it must still never point back across the window.) */
const SWIPE: Record<PopupCorner, 'left' | 'right'> = {
  'bottom-right': 'right',
  'top-right': 'right',
  'bottom-left': 'left',
  'top-left': 'left',
}

/** What the column keeps off (features/toastRoom.ts): every transmit surface the call card keeps
 *  off, and the update banner, which stands in the bottom-right corner above everything. */
const TOAST_AVOID = `${TX_AVOID}, .update-banner`
/** The column's distance from its corner's side edge, and from each surface: `--space-4`. */
const GAP_PX = 16

/** #445: measure the window's transmit surfaces and give the column the room of its corner that
 *  touches none of them, as `--toast-inset` and `--toast-max-h` on the column (styles.css reads
 *  both, in every corner). In the column's own CSS px: `.app` is zoomed, and a fixed box's insets
 *  are scaled by that zoom, so window px are divided by it (read off the column itself), and the
 *  least room a toast needs is scaled up by it.
 *
 *  Again whenever the surfaces may have changed, only while a toast is up:
 *  · a window resize, and a scroll anywhere (a sticky strip moves);
 *  · a size change of the column or of ANY surface now in the window — every measure observes the
 *    surfaces it finds (observing is idempotent), so a strip mounted later is watched too;
 *  · any change in the app's tree (a screen's first visit mounting its TX strip after the toasts, a
 *    notice that moves a strip without resizing it, a late update banner): a MutationObserver on
 *    `.app`, coalesced to one measure per frame. The column's own subtree is ignored, and a measure
 *    writes only what changed, so measuring never sets itself off again. */
function useToastRoom(viewport: RefObject<HTMLOListElement | null>, corner: PopupCorner, shown: boolean) {
  useLayoutEffect(() => {
    const vp = viewport.current
    if (!vp || !shown) return
    const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(() => place()) : null
    const write = (name: string, value: string | null) => {
      if (value === null) {
        if (vp.style.getPropertyValue(name) !== '') vp.style.removeProperty(name)
      } else if (vp.style.getPropertyValue(name) !== value) vp.style.setProperty(name, value)
    }
    const place = () => {
      const r = vp.getBoundingClientRect()
      const zoom = vp.offsetWidth > 0 && r.width > 0 ? r.width / vp.offsetWidth : 1
      const avoid: Box[] = []
      for (const el of document.querySelectorAll(TOAST_AVOID)) {
        if (vp.contains(el)) continue
        ro?.observe(el)
        const b = el.getBoundingClientRect()
        if (b.width > 0 && b.height > 0) avoid.push({ left: b.left, top: b.top, right: b.right, bottom: b.bottom })
      }
      const room =
        r.width > 0
          ? toastRoom(corner, { w: window.innerWidth, h: window.innerHeight }, r.width, avoid, GAP_PX * zoom, TOAST_MIN_ROOM * zoom)
          : null
      write('--toast-inset', room ? `${room.inset / zoom}px` : null)
      write('--toast-max-h', room ? `${room.maxHeight / zoom}px` : null)
    }
    let frame = 0
    const later = () => {
      if (frame) return
      frame = window.requestAnimationFrame(() => {
        frame = 0
        place()
      })
    }
    place()
    ro?.observe(vp)
    const mo =
      typeof MutationObserver === 'function'
        ? new MutationObserver((records) => {
            if (records.some((m) => !vp.contains(m.target))) later()
          })
        : null
    mo?.observe(vp.closest('.app') ?? document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['hidden', 'style', 'class'],
    })
    window.addEventListener('resize', place)
    window.addEventListener('scroll', place, true)
    return () => {
      ro?.disconnect()
      mo?.disconnect()
      if (frame) window.cancelAnimationFrame(frame)
      window.removeEventListener('resize', place)
      window.removeEventListener('scroll', place, true)
    }
  })
}

const KIND_CLASS: Record<ToastKind, string> = {
  error: 'kind-error',
  info: 'kind-info',
  success: 'kind-success',
}

export function Toasts() {
  const [toasts, setToasts] = useState<Toast[]>([])
  // What pops up, not every toast raised: with pop-ups off (#391) the bus keeps the rest.
  useEffect(() => subscribePopups(setToasts), [])
  // ⚠️ IDS WHOSE ACTION IS IN FLIGHT OR HAS FAILED (R3). Radix closes a Root when its Action is
  // pressed — right for a one-shot action, wrong for one that can FAIL, and it is the second
  // half of the bug: even once the click handler stopped dismissing unconditionally, Radix's
  // own close still retired the toast through `onOpenChange`. An id parked here opts out of
  // that one path (never out of ✕ or swipe) until its action either succeeds — it is dismissed
  // explicitly then — or fails, when the toast must stay so the operator can act again.
  const held = useRef(new Set<number>())
  // `held` changed: its buttons are drawn disabled while their action is in flight.
  const [, redraw] = useState(0)
  // #445: the corner the column stands in (Settings ▸ Spots & Alerts ▸ Alerts ▸ Pop-up corner).
  const corner = useSyncExternalStore(subscribePopupCorner, getPopupCorner)
  const viewport = useRef<HTMLOListElement>(null)
  useToastRoom(viewport, corner, toasts.length > 0)

  return (
    <RToast.Provider swipeDirection={SWIPE[corner]} duration={Infinity}>
      {/* The bound toast is `toast`, not `t` — `t` is the translator here. */}
      {toasts.map((toast) => (
        <RToast.Root
          key={toast.id}
          // Screen-reader severity: errors + prominent alerts ("W1AW is calling
          // you") interrupt (foreground = assertive); routine notes (QSY, saved-
          // file confirmations) queue politely (background) so they never talk
          // over the operator mid-exchange.
          type={toast.kind === 'error' || toast.prominent ? 'foreground' : 'background'}
          className={`ui-toast ${KIND_CLASS[toast.kind]}${toast.prominent ? ' prominent' : ''}`}
          open
          onOpenChange={(o) => {
            if (!o && !held.current.has(toast.id)) dismissToast(toast.id)
          }}
        >
          <RToast.Description className="ui-toast-msg">
            {toast.message}
            {/* Why its action did not happen: the toast stays so the operator can act and press
                again, and this says what to do first. Announced as it appears. */}
            {toast.note && <span role="alert" className="ui-toast-note">{toast.note}</span>}
          </RToast.Description>
          {toast.action && (
            <RToast.Action asChild altText={toast.actionLabel ?? t('toast.action.default')}>
              <button
                type="button"
                className="ui-toast-action"
                // In flight, it cannot be sent again (a second Undo of a large purge, pressed by an
                // operator who saw nothing happen for the seconds it takes).
                disabled={held.current.has(toast.id)}
                // ⚠️ AN ACTION THAT FAILS MUST NOT RETIRE ITS TOAST (R3). This used to run the
                // action and dismiss unconditionally and synchronously, so the update toast's
                // Download button closed the notice whether or not a browser ever opened — and
                // the updater, seeing the same false success, recorded the version as dismissed
                // for good. An action that returns a promise now decides: resolve = done, close
                // it; reject = it did not happen, leave the toast so the operator can act again.
                // Sync actions (every other toast in the app — Work, Answer, QSY) are untouched.
                onClick={() => {
                  // Two presses before the redraw that disables it: the second sends nothing.
                  if (held.current.has(toast.id)) return
                  const result: unknown = toast.action?.()
                  if (typeof (result as PromiseLike<unknown> | undefined)?.then !== 'function') {
                    dismissToast(toast.id)
                    return
                  }
                  // Runs BEFORE Radix's own close handler (Slot composes the child's onClick
                  // first), so the id is parked by the time `onOpenChange` asks. Its expiry is
                  // called off too: an action can outlast it, and one that fails must not take
                  // its toast with it (an Undo refused while transmitting is pressed again later).
                  held.current.add(toast.id)
                  redraw((n) => n + 1)
                  keepToast(toast.id)
                  void Promise.resolve(result).then(
                    () => {
                      held.current.delete(toast.id)
                      dismissToast(toast.id)
                    },
                    (err: unknown) => {
                      // The action reported its own failure. Release the hold so ✕ works again,
                      // and the button, and leave the toast up, saying why.
                      held.current.delete(toast.id)
                      redraw((n) => n + 1)
                      const why = err instanceof Error ? err.message : typeof err === 'string' ? err : ''
                      keepToast(toast.id, why || undefined)
                    },
                  )
                }}
              >
                {toast.actionLabel ?? t('toast.action.default')} →
              </button>
            </RToast.Action>
          )}
          {toast.secondaryAction && toast.secondaryLabel && (
            <RToast.Action asChild altText={toast.secondaryLabel}>
              <button
                type="button"
                className="ui-toast-action ui-toast-secondary"
                onClick={() => {
                  toast.secondaryAction?.()
                  dismissToast(toast.id)
                }}
              >
                {toast.secondaryLabel}
              </button>
            </RToast.Action>
          )}
          <RToast.Close className="ui-toast-close" aria-label={t('toast.dismiss')}>
            ×
          </RToast.Close>
        </RToast.Root>
      ))}
      {/* Where it stands is styles.css's, by `data-corner`: the column keeps its zoom-corrected
          caps (`--vh-eff`, `--vw-eff`) in every corner. */}
      <RToast.Viewport ref={viewport} className="ui-toast-viewport" data-corner={corner} />
    </RToast.Provider>
  )
}
