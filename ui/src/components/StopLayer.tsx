// STOP TX IS ALWAYS CLICKABLE, ON TOP OF WHATEVER IS OPEN (the operator's ruling: "always clickable on top").
//
// A dialog, a confirmation, a full-window scrim or a menu drawn over the top bar used to stand between the operator and
// Stop TX: the Radix dialogs lock the page's pointer events and dim it, the hand-rolled scrims (the prompt to log, the
// Logbook's confirmations, the launch radio picker) cover the window, and at a narrow window the band pill's menu is
// drawn over the top bar's Stop TX. In each case one click on Stop TX stopped nothing.
//
// THE RULE, once, for every overlay present and future: while any overlay is open (anything drawn with role dialog,
// alertdialog or menu, anything aria-modal, or a page whose pointer events are locked), every stop control
// on screen that the overlay keeps from being clicked gets a MIRROR in this layer — the one layer above every other
// (`.stop-layer`, the top z-index of the app; `stop-layer.test.ts` computes that it wins over every overlay's). The
// mirror is a copy of the button, drawn at the button's own place and size (Stop TX does not move), and a click on it
// is the button's own click: `original.click()`, so the stop runs through exactly the handler and the wire it always
// did (stop-control-wiring.test.tsx).
//
// A STOP CONTROL is an element carrying `data-stop-control`: Stop TX wherever it is drawn (the top bar, the TX strip,
// FT's strip, Roam), and the docks' stop keys (RTTY's, PSK's and JTTY's Esc Stop, SSTV's Stop). stop-line.test.tsx
// holds every Stop TX and dock stop of every cockpit to carrying it. Tune, PTT and the TX latch are not on it: the
// ruling is Stop TX and the docks' stops, and the latch's other click turns transmit ON.
//
// WHAT A PRESS ON A MIRROR DOES, AND DOES NOT:
//   · It stops: the original's click handler runs once.
//   · The overlay stays open: the press stops at this layer, so no overlay's "press outside" listener (Radix's are on
//     the document, in the bubble phase) hears it, and a hand-rolled backdrop's own click never gets it.
//   · The keyboard stays where it was (in the dialog): the mirror is not focusable (tabIndex -1, aria-hidden) and its
//     mousedown is prevented, so the press moves no focus and a dialog's focus trap is not touched.
//   · A disabled stop stays disabled: the mirror copies the button's `disabled`.
// A stop control the overlay does NOT cover keeps no mirror and is clicked as it is (a menu beside Stop TX closes and
// lets the click through, MenuParts.tsx). With no overlay open there is no mirror at all, so nothing here can hide a
// layout fault the shell probes exist to catch.
//
// Esc is untouched: it was already a stop through any overlay (useEscStop listens on window in the capture phase).
import { useEffect } from 'react'

export const STOP_CONTROL = '[data-stop-control]'
/** What counts as an overlay, once it is drawn. A page whose pointer events are locked (a Radix modal) is one too.
 *  Not `listbox`: Operate's decode list is one, on screen all the time. */
const OVERLAY = '[role="dialog"], [role="alertdialog"], [role="menu"], [aria-modal="true"]'
/** The points of a stop control tested for being the topmost thing there: 5 across, 3 down. */
const COLS = 5
const ROWS = 3

interface Box {
  left: number
  top: number
  right: number
  bottom: number
}

/** Whether any overlay is open. */
function overlayOpen(): boolean {
  if (document.body.style.pointerEvents === 'none') return true
  // Drawn, not merely mounted: a keep-alive screen's markup stays in the document behind `[hidden]`.
  for (const el of document.querySelectorAll(OVERLAY)) {
    const r = el.getBoundingClientRect()
    if (r.width > 0 && r.height > 0) return true
  }
  return false
}

/** The part of `el` the operator can see: its box cut to the window and to every ancestor that clips. Null if none. */
function visibleBox(el: HTMLElement): Box | null {
  const r = el.getBoundingClientRect()
  if (r.width <= 0 || r.height <= 0) return null
  const box: Box = { left: Math.max(r.left, 0), top: Math.max(r.top, 0), right: Math.min(r.right, innerWidth), bottom: Math.min(r.bottom, innerHeight) }
  for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
    const cs = getComputedStyle(p)
    if (cs.overflowX === 'visible' && cs.overflowY === 'visible') continue
    const q = p.getBoundingClientRect()
    box.left = Math.max(box.left, q.left)
    box.top = Math.max(box.top, q.top)
    box.right = Math.min(box.right, q.right)
    box.bottom = Math.min(box.bottom, q.bottom)
  }
  return box.right > box.left && box.bottom > box.top ? box : null
}

/** Whether some point of `el`'s visible part is `el` itself, topmost, not counting this layer's own mirrors. */
function reachable(el: HTMLElement, box: Box, layer: HTMLElement): boolean {
  for (let j = 1; j <= ROWS; j++) {
    for (let i = 1; i <= COLS; i++) {
      const x = box.left + ((box.right - box.left) * i) / (COLS + 1)
      const y = box.top + ((box.bottom - box.top) * j) / (ROWS + 1)
      const top = document.elementsFromPoint(x, y).find((e) => !layer.contains(e))
      if (top && (top === el || el.contains(top))) return true
    }
  }
  return false
}

/** Every computed style of `from` (and of each descendant) written inline onto its copy `to`: the mirror is drawn
 *  outside the tree the stylesheet's selectors reach it through, so it carries its looks with it. */
function copyStyles(from: Element, to: HTMLElement | SVGElement) {
  const cs = getComputedStyle(from)
  for (let k = 0; k < cs.length; k++) {
    const p = cs[k]
    to.style.setProperty(p, cs.getPropertyValue(p))
  }
  const a = from.children
  const b = to.children
  for (let k = 0; k < a.length && k < b.length; k++) copyStyles(a[k], b[k] as HTMLElement)
}

interface Mirror {
  node: HTMLElement
  key: string
}

export function StopLayer() {
  useEffect(() => {
    const layer = document.createElement('div')
    layer.className = 'stop-layer'
    layer.setAttribute('aria-hidden', 'true')
    document.body.appendChild(layer)
    const mirrors = new Map<HTMLElement, Mirror>()
    // While a press on a mirror is in progress the layer holds still: the press may close what covered the button,
    // and the mirror must still be there for the click that ends it.
    let pressing = false

    const drop = (el: HTMLElement) => {
      mirrors.get(el)?.node.remove()
      mirrors.delete(el)
    }

    const place = (el: HTMLElement, box: Box) => {
      const r = el.getBoundingClientRect()
      // The app is zoomed (`.app { zoom: var(--ui-zoom) }`) and this layer is not: the box is in window px, the
      // button's own lengths are not, and the mirror takes the button's zoom to match it.
      const z = el.offsetWidth > 0 ? r.width / el.offsetWidth : 1
      const key = `${el.outerHTML}|${r.left},${r.top},${r.width},${r.height}|${box.left},${box.top},${box.right},${box.bottom}|${z}`
      const had = mirrors.get(el)
      if (had?.key === key) return
      const node = el.cloneNode(true) as HTMLElement
      copyStyles(el, node)
      node.removeAttribute('id')
      node.removeAttribute('data-stop-control')
      node.setAttribute('data-stop-mirror', '')
      node.setAttribute('tabindex', '-1')
      const s = node.style
      s.position = 'fixed'
      s.margin = '0'
      s.zoom = String(z)
      s.left = `${r.left / z}px`
      s.top = `${r.top / z}px`
      s.right = 'auto'
      s.bottom = 'auto'
      s.transform = 'none'
      s.transition = 'none'
      s.animation = 'none'
      s.pointerEvents = 'auto'
      // Only the part the operator could see of the button is drawn (a clipping pane may cut it).
      s.clipPath = `inset(${(box.top - r.top) / z}px ${(r.right - box.right) / z}px ${(r.bottom - box.bottom) / z}px ${(box.left - r.left) / z}px)`
      node.addEventListener('pointerdown', (e) => {
        e.stopPropagation()
        pressing = true
      })
      node.addEventListener('mousedown', (e) => {
        e.preventDefault()
        e.stopPropagation()
      })
      node.addEventListener('click', (e) => {
        e.preventDefault()
        e.stopPropagation()
        pressing = false
        el.click()
      })
      if (had) had.node.replaceWith(node)
      else layer.appendChild(node)
      mirrors.set(el, { node, key })
    }

    const sync = () => {
      if (pressing) return
      if (!overlayOpen()) {
        for (const el of [...mirrors.keys()]) drop(el)
        return
      }
      const stops = new Set(document.querySelectorAll<HTMLElement>(STOP_CONTROL))
      for (const el of [...mirrors.keys()]) if (!stops.has(el)) drop(el)
      for (const el of stops) {
        const box = visibleBox(el)
        if (!box || reachable(el, box, layer)) drop(el)
        else place(el, box)
      }
    }

    // A press that ends anywhere but on its mirror (dragged off, cancelled) lets the layer move again.
    const released = () => setTimeout(() => (pressing = false), 0)
    window.addEventListener('pointerup', released, true)
    window.addEventListener('pointercancel', released, true)
    let frame = 0
    const tick = () => {
      sync()
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => {
      cancelAnimationFrame(frame)
      window.removeEventListener('pointerup', released, true)
      window.removeEventListener('pointercancel', released, true)
      layer.remove()
    }
  }, [])
  return null
}
