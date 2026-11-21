// STOP TX IS ALWAYS CLICKABLE, ON TOP OF WHATEVER IS OPEN (the operator's ruling: "always clickable on top").
//
// A dialog, a confirmation, a full-window scrim or a menu drawn over the top bar used to stand between the operator and
// Stop TX: the Radix dialogs lock the page's pointer events and dim it, the hand-rolled scrims (the prompt to log, the
// Logbook's confirmations, the launch radio picker) cover the window, and at a narrow window the band pill's menu is
// drawn over the top bar's Stop TX. In each case one click on Stop TX stopped nothing.
//
// THE RULE, once, for every overlay present and future: while any overlay is open (anything drawn with role dialog,
// alertdialog or menu, anything aria-modal, or a page whose pointer events are locked), every stop control
// on screen that the overlay covers ANY PART of gets a MIRROR in this layer — the one layer above every other
// (`.stop-layer`, the top z-index of the app; `stop-layer.test.ts` computes that it wins over every overlay's). The
// mirror is a copy of the button, drawn at the button's own place and size (Stop TX does not move), and a click on it
// is the button's own click: `original.click()`, so the stop runs through exactly the handler and the wire it always
// did (stop-control-wiring.test.tsx).
//
// A STOP CONTROL is an element carrying `data-stop-control`: Stop TX wherever it is drawn (the top bar, the TX strip,
// FT's strip), and the docks' stop keys (RTTY's, PSK's and JTTY's Esc Stop, RTTY's sequencer Abort, SSTV's Stop).
// Its value names the stop: `stop-tx`, `esc-stop`, `sequencer-abort`, `sstv-stop`. StopLayer.test.tsx pins the eight
// places the app writes it, exactly, and stop-line.test.tsx holds every Stop TX and dock stop of every cockpit to
// carrying it. Tune, PTT and the TX latch are
// not on it: the ruling is Stop TX and the docks' stops, and the latch's other click turns transmit ON. Roam's Stop
// stops roaming, not a transmission, and is not on it either.
//
// WHAT A PRESS ON A MIRROR DOES, AND DOES NOT:
//   · It stops: the original's click handler runs once.
//   · The overlay stays open: the press stops at this layer, so no overlay's "press outside" listener (Radix's are on
//     the document, in the bubble phase) hears it, and a hand-rolled backdrop's own click never gets it.
//   · The keyboard stays where it was (in the dialog): the mirror is not focusable (tabIndex -1, aria-hidden) and its
//     mousedown is prevented, so the press moves no focus and a dialog's focus trap is not touched.
//   · A disabled stop stays disabled: the mirror copies the button's `disabled`.
//   · ONE press is ONE stop. The pointer lands on the mirror, never on the button under it, and the mirror's click
//     calls the button's click once. The mirror takes no keyboard: not focusable, and Enter or Space on it does
//     nothing even if code focused it, so there is no key repeat through it. The button keeps its own keyboard.
//
// A MIRROR NEVER OUTLIVES OR MISSTATES ITS BUTTON. While a mirror stands, a MutationObserver watches the document and
// re-checks every mirror as soon as anything changes, before the next frame is painted: a button that is unmounted,
// put behind `[hidden]`, scrolled or clipped out of sight, or loses its marker loses its mirror at once, and a button
// whose markup changes (disabled, its label) gets a fresh mirror. The one exception is the mirror being pressed: it
// stays until that press ends (its click, or the pointer let go elsewhere), and goes then — a mirror removed under a
// pressed pointer would lose the press, and the stop with it. A click on a mirror checks its button as it lands:
//   · the button is there and enabled: its click runs, once;
//   · the button is disabled: nothing;
//   · the button has GONE (React replaced its node, a re-render between the press and the click): the click goes to
//     the stop that took its place — a connected, drawn, MARKED control with the SAME marker value (the stop's
//     identity: `stop-tx`, `esc-stop`, `sequencer-abort`, `sstv-stop`) standing under the press point — and to
//     nothing else. Never an unmarked control, never another kind of stop, never one somewhere else.
//
// Screen readers see only the button: the layer and every mirror are aria-hidden, and the mirror has no id. The
// button stays the accessible control (under a Radix dialog it is hidden from them with the rest of the page, as it
// always was).
// ANY PART COVERED IS COVERED. A press lands wherever the operator's pointer is, not at the button's centre, so a stop
// control is clear only when its centre AND every point of a 9 × 5 grid across its visible part are the button itself,
// topmost. A menu over part of Stop TX — the band menu over 78 % of it on FT at 1366×768 @175 %, a few of its points
// still its own — gets it a mirror. (The shell probes call a stop control covered when its centre is lost or under
// 90 % of its points are its own; every point is the stricter rule, so a stop the layer leaves alone passes theirs.)
// WHAT AN OVERLAY IS, FOR THIS LAYER: the element with the role (or aria-modal), everything inside it and everything it
// sits in. A stop control no part of that meets is not hit-tested at all, so A SCRIM MUST BE THE OVERLAY OR ITS
// ANCESTOR (a scrim beside the dialog it dims, or a pop-up portaled away from its overlay, is invisible to this layer),
// unless it is a Radix modal's own overlay, which locks the page's pointer events and so puts every stop in question.
// stop-layer.test.ts walks every full-window scrim the stylesheets declare and every place the app draws one.
// A stop control the overlay covers NONE of keeps no mirror and is clicked as it is (a menu beside Stop TX closes and
// lets the click through, MenuParts.tsx). With no overlay open there is no mirror at all, so nothing here can hide a
// layout fault the shell probes exist to catch.
//
// Esc is untouched: it was already a stop through any overlay (useEscStop listens on window in the capture phase).
import { useEffect } from 'react'

export const STOP_CONTROL = '[data-stop-control]'
/** What counts as an overlay, once it is drawn. A page whose pointer events are locked (a Radix modal) is one too.
 *  Not `listbox`: Operate's decode list is one, on screen all the time. */
const OVERLAY = '[role="dialog"], [role="alertdialog"], [role="menu"], [aria-modal="true"]'
/** The points of a stop control tested for being the topmost thing there: 9 across, 5 down, and the centre. */
const COLS = 9
const ROWS = 5

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

/** Whether any part of an open overlay is drawn over `box`: the overlay's own box, the box of everything it sits in
 *  (a scrim that holds a dialog, a popper's wrapper), up to <body>, and the box of everything in it (a part drawn
 *  outside the overlay's own box, placed absolutely or overflowing it). An overlay whose own box is empty counts too:
 *  its parts may still be drawn. Always true while the page's pointer events are locked: then nothing under the overlay
 *  can be pressed, wherever it is drawn. A stop control no part of any overlay meets cannot be covered by one, so it
 *  needs no hit test (the grid below costs 46 of them, on every frame while an overlay is open). */
function overlayMeets(box: Box): boolean {
  if (document.body.style.pointerEvents === 'none') return true
  const meets = (n: Element) => {
    const r = n.getBoundingClientRect()
    return r.left < box.right && r.right > box.left && r.top < box.bottom && r.bottom > box.top
  }
  for (const el of document.querySelectorAll(OVERLAY)) {
    for (let n: Element | null = el; n && n !== document.body && n !== document.documentElement; n = n.parentElement) {
      if (meets(n)) return true
    }
    for (const n of el.querySelectorAll('*')) if (meets(n)) return true
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

/** Whether every point of `el`'s visible part (the grid and the centre) is `el` itself, topmost, not counting this
 *  layer's own mirrors: false as soon as anything else stands over any of them. */
function uncovered(el: HTMLElement, box: Box, layer: HTMLElement): boolean {
  const own = (x: number, y: number) => {
    const top = document.elementsFromPoint(x, y).find((e) => !layer.contains(e))
    return top != null && (top === el || el.contains(top))
  }
  if (!own((box.left + box.right) / 2, (box.top + box.bottom) / 2)) return false
  for (let j = 1; j <= ROWS; j++) {
    for (let i = 1; i <= COLS; i++) {
      if (!own(box.left + ((box.right - box.left) * i) / (COLS + 1), box.top + ((box.bottom - box.top) * j) / (ROWS + 1))) return false
    }
  }
  return true
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

/** A button that can still have a mirror: in the document, still marked, not behind `[hidden]`, and in sight. */
function present(el: HTMLElement): boolean {
  return el.isConnected && el.matches(STOP_CONTROL) && el.closest('[hidden]') == null && visibleBox(el) != null
}

/** The stop that took a departed button's place: connected, drawn and marked with the same identity, under the press
 *  point. Null when there is none, which is the only safe answer then. */
function replacement(identity: string | null, x: number, y: number): HTMLElement | null {
  if (identity == null) return null
  for (const c of document.querySelectorAll<HTMLElement>(STOP_CONTROL)) {
    if (c.getAttribute('data-stop-control') !== identity || !present(c)) continue
    const r = c.getBoundingClientRect()
    if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return c
  }
  return null
}

/** What a mirror draws: the button's markup and where it stands. A change in either is a new mirror. */
function keyOf(el: HTMLElement, box: Box): string {
  const r = el.getBoundingClientRect()
  return `${el.outerHTML}|${r.left},${r.top},${r.width},${r.height}|${box.left},${box.top},${box.right},${box.bottom}`
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
    let pressed: { el: HTMLElement; x: number; y: number } | null = null
    // While the pointer is down on a stop control ITSELF, no new mirror is drawn: an overlay that opens on its own over
    // the button in that moment would put a mirror under the pointer, and the press — begun on the button, ended on
    // the mirror — would click neither. The press ends on the button and stops; the mirror follows it. Only the
    // primary pointer's main-button press holds, only that pointer's own release or cancel ends the hold, and every
    // primary press decides afresh (as does the window losing focus), so a release the page never heard cannot leave
    // the layer held.
    let holding = false
    let holdId = 0
    const onPointerDown = (e: PointerEvent) => {
      if (!e.isPrimary) return
      const t = e.target as Element | null
      holding = e.button === 0 && !!(t && !layer.contains(t) && t.closest?.(STOP_CONTROL))
      holdId = e.pointerId
    }
    const onBlur = () => {
      holding = false
    }

    const drop = (el: HTMLElement) => {
      mirrors.get(el)?.node.remove()
      mirrors.delete(el)
    }

    const place = (el: HTMLElement, box: Box) => {
      const r = el.getBoundingClientRect()
      // The app is zoomed (`.app { zoom: var(--ui-zoom) }`) and this layer is not: the box is in window px, the
      // button's own lengths are not, and the mirror takes the button's zoom to match it.
      const z = el.offsetWidth > 0 ? r.width / el.offsetWidth : 1
      const key = `${keyOf(el, box)}|${z}`
      const had = mirrors.get(el)
      if (had?.key === key) return
      const node = el.cloneNode(true) as HTMLElement
      copyStyles(el, node)
      node.removeAttribute('id')
      node.removeAttribute('data-stop-control')
      node.setAttribute('data-stop-mirror', '')
      node.setAttribute('tabindex', '-1')
      node.setAttribute('aria-hidden', 'true')
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
        pressed = { el, x: e.clientX, y: e.clientY }
      })
      node.addEventListener('mousedown', (e) => {
        e.preventDefault()
        e.stopPropagation()
      })
      node.addEventListener('click', (e) => {
        e.preventDefault()
        e.stopPropagation()
        const press = pressed?.el === el ? pressed : { el, x: e.clientX, y: e.clientY }
        pressed = null
        // The button is checked as the click lands, not as the mirror was drawn.
        let target: HTMLElement | null = el
        if (!present(el)) {
          target = replacement(el.getAttribute('data-stop-control'), press.x, press.y)
          drop(el)
        }
        // A disabled stop stops nothing; its mirror stays, disabled, as the button is.
        if (!target || (target as HTMLButtonElement).disabled) return
        target.click()
      })
      // No keyboard through the mirror: Enter and Space would activate it, and a held key would repeat.
      node.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' || e.key === ' ' || e.key === 'Spacebar') e.preventDefault()
      })
      if (had) had.node.replaceWith(node)
      else layer.appendChild(node)
      mirrors.set(el, { node, key })
      watch()
    }

    // Every standing mirror against its button, at once: dropped if the button went, redrawn if it changed. Not
    // held by a press, and not waiting for a frame.
    const refresh = () => {
      for (const [el, m] of [...mirrors]) {
        // The mirror under a pressed pointer stays as it is until the press ends (see the header).
        if (pressed?.el === el) continue
        if (!present(el)) drop(el)
        else if (!m.key.startsWith(`${keyOf(el, visibleBox(el)!)}|`)) place(el, visibleBox(el)!)
      }
      if (mirrors.size === 0) unwatch()
    }
    // Watching the whole document costs something on a busy screen, so it runs only while a mirror stands.
    const observer = new MutationObserver((records) => {
      if (records.every((r) => layer.contains(r.target))) return
      refresh()
    })
    let watching = false
    const watch = () => {
      if (watching) return
      watching = true
      observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true })
    }
    const unwatch = () => {
      watching = false
      observer.disconnect()
    }

    const sync = () => {
      refresh()
      if (pressed || holding) return
      if (!overlayOpen()) {
        for (const el of [...mirrors.keys()]) drop(el)
        return
      }
      const stops = new Set(document.querySelectorAll<HTMLElement>(STOP_CONTROL))
      for (const el of [...mirrors.keys()]) if (!stops.has(el)) drop(el)
      for (const el of stops) {
        const box = present(el) ? visibleBox(el) : null
        if (!box || !overlayMeets(box) || uncovered(el, box, layer)) drop(el)
        else place(el, box)
      }
    }

    // A press that ends anywhere but on its mirror (dragged off, cancelled) lets the layer move again.
    const released = (e: PointerEvent) => {
      const mine = e.pointerId === holdId
      setTimeout(() => {
        pressed = null
        if (mine) holding = false
        refresh()
      }, 0)
    }
    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('pointerup', released, true)
    window.addEventListener('pointercancel', released, true)
    window.addEventListener('blur', onBlur)
    let frame = 0
    const tick = () => {
      sync()
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => {
      cancelAnimationFrame(frame)
      unwatch()
      window.removeEventListener('pointerdown', onPointerDown, true)
      window.removeEventListener('pointerup', released, true)
      window.removeEventListener('pointercancel', released, true)
      window.removeEventListener('blur', onBlur)
      layer.remove()
    }
  }, [])
  return null
}
