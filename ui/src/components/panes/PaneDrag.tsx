// DRAGGING A PANE BY ITS TITLE (2026-10-08, the operator's "drag with the mouse"): in a cockpit that ⊞ Panels ▸
// Arrange can arrange, a pane is picked up by its title bar and dropped onto a column or between two panes; a
// line shows where it will land, a column with nothing on screen gets a zone where it would stand, and ⊞
// Arrange's own list reorders the same way. The arrows stay, for the keyboard and screen readers.
//
// A DROP IS THE ARROWS' MOVES. Where the pointer is released names a place and the pane it lands above; the
// cockpit hands that to the panel record (`dropPane`), which runs the ▲ ▼ ◀ ▶ moves to it (features/panelPlace
// `dropArranged`). So this file decides only WHERE; it never writes a record, and a place no run of the arrows
// reaches (a pinned pane in another column, a pane the left side does not list) is no place at all: no line is
// drawn there, and a release there cancels.
//
// POINTER EVENTS, not the HTML5 drag API: one path for a mouse, a pen and a finger (the hosted page and the FT
// pop-out offer Arrange too, on a phone), nothing the webview's own drag-and-drop handler can swallow, and the
// drop line, Escape and "a release outside cancels" are ours. A mouse or a pen lifts the pane after a few
// pixels; a finger after a still hold, so a swipe that starts on a title still scrolls the screen.
//
// WHAT CAN BE PICKED UP: an element marked `data-pane-grip` (a pane's title bar) whose id the cockpit's
// ArrangeSpec lists and which is on screen. Never from a button, a field, a link or a label inside the title —
// they keep their own clicks, and a double-click on a decode (in the pane's body, not its title) still calls the
// station. WHERE IT CAN GO: the places the cockpit names (`targets`: its columns, Phone's left side).
//
// THE STOP LINE is not near any of this. The TX strip, the dock, PTT, Tune and Stop TX have no pane id, so
// nothing in them is a grip, and they are no place: a release over them cancels. The drag starts and stops
// nothing on the air, and it never swallows a key — Escape cancels it AND still does what Escape does on that
// screen (on FT, Phone, CW and JS8 it also stops transmit). The marks it draws stand INSIDE the region they
// mark, under the sticky TX strip and dock (`.pane-drop-layer`, styles.css), so nothing it draws can cover
// Stop TX or Tune.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
import { useEffect, useMemo, useRef, useSyncExternalStore, type CSSProperties, type RefObject } from 'react'
import { announce } from '../../announce'
import { t } from '../../i18n'
import { arrangeIds, dropArranged, placedColumns, PANE_COLUMNS, type ArrangeSpec, type Arrangement, type PaneColumn, type PaneDrop } from '../../features/panelPlace'

/** A place a pane can be dropped: a column, or the left side. */
export type DropArea = PaneColumn | 'side'

/** One place, as the cockpit draws it now, in the order the places stand on screen: the element it is drawn
 *  in, or null for one with nothing on screen (a zone is drawn where it would stand). `areas`: the places that
 *  element draws, top to bottom — two where columns 1 and 2 share one column below three tracks. */
export interface PaneDropTarget {
  el: HTMLElement | null
  areas: readonly DropArea[]
}

export interface PaneDragOptions<P extends string> {
  /** Where a press can start a drag: the cockpit (or the list) the grips stand in. */
  root: RefObject<HTMLElement | null>
  /** The box the places stand in (the pane region, or the list): a place with nothing on screen gets its zone
   *  inside it, and a release counts only over it or over a drawn place, never over what covers them. */
  region: RefObject<HTMLElement | null>
  /** Off: nothing here starts a drag (the window does not arrange). */
  enabled: boolean
  spec: ArrangeSpec<P>
  arrangement: Arrangement<P>
  /** The arrows' own arguments: what they step past, whether the left side shows, the columns' order on
   *  screen — so a place the arrows cannot reach is no place here either. */
  shown: (id: P) => boolean
  sideShows: boolean
  order?: readonly PaneColumn[]
  targets: () => readonly PaneDropTarget[]
  /** The places stand one above the other (a narrow window). Read from their boxes when two are drawn. */
  stacked?: boolean
  /** The panes' names and the places' names, for the zones and for what a screen reader is told. */
  labels: Readonly<Record<P, string>>
  names: Readonly<Partial<Record<DropArea, string>>>
  onDrop: (id: P, drop: PaneDrop<P>) => void
}

/** A box on screen, in viewport px. */
export interface Box {
  left: number
  top: number
  width: number
  height: number
}

/** What a drag in flight draws: the pane picked up, every place it can go (the one under the pointer hot),
 *  and the line where it lands. */
export interface DragView {
  source: Box | null
  places: Array<{ box: Box; area: DropArea; zone: boolean; name?: string; hot: boolean }>
  line: Box | null
}

export interface PaneDrag {
  subscribe: (cb: () => void) => () => void
  view: () => DragView | null
  /** End a drag in flight without dropping it. */
  cancel: () => void
}

/** A mouse or a pen lifts the pane after this many px; a finger after this hold, if it moved no more than the
 *  slop (a finger that moves first is scrolling). */
export const LIFT_PX = 5
export const TOUCH_HOLD_MS = 350
const TOUCH_SLOP_PX = 10
/** An empty place's zone, across (side by side) or down (stacked), and the reach of the nearest column for a
 *  pointer in the gap between two. */
const ZONE_PX = 56
const ZONE_GAP_PX = 4
const NEAR_PX = 24
/** The band at a scroller's edge where a drag scrolls it, and the most it scrolls a frame. */
const EDGE_PX = 32
const EDGE_STEP_PX = 14

// What a press must not start a drag from: the controls a title holds keep their own clicks.
const INTERACTIVE =
  'button, a[href], input, select, textarea, label, summary, [contenteditable=""], [contenteditable="true"], [role="button"], [role="checkbox"], [role="switch"], [role="menuitem"], [role="tab"], [role="slider"], [role="option"], [role="link"], [role="combobox"]'

const boxOf = (r: DOMRect | Box): Box => ({ left: r.left, top: r.top, width: r.width, height: r.height })
const right = (b: Box) => b.left + b.width
const bottom = (b: Box) => b.top + b.height
const inside = (b: Box, x: number, y: number) => x >= b.left && x <= right(b) && y >= b.top && y <= bottom(b)
const distance = (b: Box, x: number, y: number) =>
  Math.hypot(Math.max(b.left - x, 0, x - right(b)), Math.max(b.top - y, 0, y - bottom(b)))

/** One place as measured: its box (or its zone's), the panes on screen in it top to bottom, the places it draws. */
export interface MeasuredPlace<P extends string> {
  box: Box
  zone: boolean
  areas: readonly DropArea[]
  panes: Array<{ id: P; box: Box }>
}

/**
 * Where each place stands, from what is drawn: a drawn place is its element's box; a place with nothing on
 * screen is a zone at the edge of its drawn neighbours — between the two, or at the host's edge where it has
 * none on that side — `ZONE_PX` across, the host's height (or `ZONE_PX` down and the host's width, stacked).
 * Several empty places in a row stand side by side there.
 */
export function measurePlaces<P extends string>(
  drawn: ReadonlyArray<{ box: Box | null; areas: readonly DropArea[]; panes: Array<{ id: P; box: Box }> }>,
  host: Box,
  stackedHint: boolean,
): MeasuredPlace<P>[] {
  const boxes = drawn.flatMap((d) => (d.box ? [d.box] : []))
  const stacked =
    boxes.length >= 2
      ? Math.min(right(boxes[0]), right(boxes[1])) - Math.max(boxes[0].left, boxes[1].left) > 0.5 * Math.min(boxes[0].width, boxes[1].width)
      : stackedHint
  const out: MeasuredPlace<P>[] = []
  for (let i = 0; i < drawn.length; i++) {
    const d = drawn[i]
    if (d.box) {
      out.push({ box: d.box, zone: false, areas: d.areas, panes: d.panes })
      continue
    }
    // The run of empty places this one is in, and the drawn places either side of it.
    let first = i
    while (first > 0 && !drawn[first - 1].box) first--
    let last = i
    while (last < drawn.length - 1 && !drawn[last + 1].box) last++
    const prev = first > 0 ? drawn[first - 1].box : null
    const next = last < drawn.length - 1 ? drawn[last + 1].box : null
    const count = last - first + 1
    const span = count * ZONE_PX + (count - 1) * ZONE_GAP_PX
    const lo = stacked ? host.top : host.left
    const hi = stacked ? bottom(host) : right(host)
    const centre =
      prev && next
        ? ((stacked ? bottom(prev) : right(prev)) + (stacked ? next.top : next.left)) / 2
        : prev
          ? hi - span / 2 - ZONE_GAP_PX
          : lo + span / 2 + ZONE_GAP_PX
    const start = Math.min(Math.max(centre - span / 2, lo), Math.max(lo, hi - span)) + (i - first) * (ZONE_PX + ZONE_GAP_PX)
    out.push({
      box: stacked
        ? { left: host.left, top: start, width: host.width, height: ZONE_PX }
        : { left: start, top: host.top, width: ZONE_PX, height: host.height },
      zone: true,
      areas: d.areas,
      panes: [],
    })
  }
  return out
}

/**
 * The drop under (x, y), or null for a point over no place. A zone takes the pane at the foot of its empty
 * place; a drawn place takes it right above the first pane there whose middle is below the point (or at the
 * foot), the point in the gap between two columns counting for the nearer one. Where one element draws two
 * places, the pane it lands above names the place, and the foot is the last of them. `line` is where it would
 * land, across the place, in the gap between the two panes. The dragged pane's own box counts for nothing.
 */
export function dropAt<P extends string>(
  places: readonly MeasuredPlace<P>[],
  x: number,
  y: number,
  dragged: P,
  areaOf: (id: P) => DropArea | undefined,
): { index: number; drop: PaneDrop<P>; line: Box | null } | null {
  const zone = places.findIndex((p) => p.zone && inside(p.box, x, y))
  if (zone >= 0) return { index: zone, drop: { area: places[zone].areas[0], before: null }, line: null }
  let index = places.findIndex((p) => !p.zone && inside(p.box, x, y))
  if (index < 0) {
    let best = NEAR_PX
    places.forEach((p, i) => {
      const d = p.zone ? Infinity : distance(p.box, x, y)
      if (d <= best) {
        best = d
        index = i
      }
    })
  }
  if (index < 0) return null
  const place = places[index]
  const others = place.panes.filter((p) => p.id !== dragged)
  const k = others.filter((p) => p.box.top + p.box.height / 2 < y).length
  const before = others[k]?.id ?? null
  const named = before != null ? areaOf(before) : undefined
  const area = named != null && place.areas.includes(named) ? named : place.areas[place.areas.length - 1]
  // In the gap right above the pane it lands above — between two panes that touch, its middle; where
  // something no drag moves stands between them (CW's Rig controls), right above the lower one, which is
  // where the pane lands — or right below the last pane, or at the top of an empty place.
  const gap = k > 0 && k < others.length ? others[k].box.top - bottom(others[k - 1].box) : Infinity
  const at =
    others.length === 0
      ? place.box.top + 4
      : k === others.length
        ? bottom(others[k - 1].box) + 3
        : gap < NEAR_PX
          ? (bottom(others[k - 1].box) + others[k].box.top) / 2
          : others[k].box.top - 3
  const mid = Math.min(Math.max(at, place.box.top + 1), bottom(place.box) - 2)
  return {
    index,
    drop: { area, before },
    line: { left: place.box.left + 4, top: mid - 1.5, width: Math.max(0, place.box.width - 8), height: 3 },
  }
}

/**
 * The places of a grid cockpit's region as it draws them (Phone, CW, JS8), in their order on screen: each
 * column element with the places it draws that hold a pane on screen, and a zone for each that holds none —
 * before the element for its first place, after it for the second — so column 1 or column 2 can still be
 * dropped onto below three tracks, where the two share one element. An element of one place is that place
 * whether or not it holds a pane (the log column holds the log form).
 */
export function gridTargets(drawn: ReadonlyArray<{ el: HTMLElement | null; areas: readonly DropArea[] }>, has: (area: DropArea) => boolean): PaneDropTarget[] {
  const out: PaneDropTarget[] = []
  for (const d of drawn) {
    if (d.areas.length === 1) {
      out.push({ el: d.el, areas: d.areas })
      continue
    }
    const full = d.el ? d.areas.filter(has) : []
    if (!full.includes(d.areas[0])) out.push({ el: null, areas: [d.areas[0]] })
    if (full.length > 0) out.push({ el: d.el, areas: full })
    for (const area of d.areas.slice(1)) if (!full.includes(area)) out.push({ el: null, areas: [area] })
  }
  return out
}

/** The element a grip belongs to inside `el`: the grip's ancestor that is a child of `el` (the pane's box). */
function paneBoxIn(el: HTMLElement, grip: Element): Element {
  let node: Element = grip
  while (node.parentElement && node.parentElement !== el) node = node.parentElement
  return node
}

/** The panes on screen in a drawn place, top to bottom, by their grips: once each, only the cockpit's own. */
function panesIn<P extends string>(el: HTMLElement, ids: readonly string[]): Array<{ id: P; box: Box }> {
  const out: Array<{ id: P; box: Box }> = []
  for (const grip of el.querySelectorAll('[data-pane-grip]')) {
    const id = grip.getAttribute('data-pane-grip') ?? ''
    if (!ids.includes(id) || out.some((p) => p.id === id)) continue
    out.push({ id: id as P, box: boxOf(paneBoxIn(el, grip).getBoundingClientRect()) })
  }
  return out
}

/** The first box scrolling `el` vertically: itself or an ancestor. */
function scrollerOf(el: Element | null): HTMLElement | null {
  for (let node = el; node && node !== document.documentElement; node = node.parentElement) {
    const h = node as HTMLElement
    const oy = getComputedStyle(h).overflowY
    if ((oy === 'auto' || oy === 'scroll') && h.scrollHeight > h.clientHeight + 1) return h
  }
  return null
}

function createStore(): PaneDrag & { set: (v: DragView | null) => void; onCancel: (fn: (() => void) | null) => void } {
  let current: DragView | null = null
  let stop: (() => void) | null = null
  const listeners = new Set<() => void>()
  return {
    subscribe: (cb) => {
      listeners.add(cb)
      return () => listeners.delete(cb)
    },
    view: () => current,
    set: (v) => {
      current = v
      for (const cb of listeners) cb()
    },
    onCancel: (fn) => {
      stop = fn
    },
    cancel: () => stop?.(),
  }
}

/**
 * Picking a pane up by its title and dropping it (see the header). Returns the drag for the cockpit's
 * `PaneDropLayer`s, which draw it inside the places they stand in.
 */
export function usePaneDrag<P extends string>(o: PaneDragOptions<P>): PaneDrag {
  const opts = useRef(o)
  opts.current = o
  const store = useMemo(() => createStore(), [])
  const { root, enabled } = o
  useEffect(() => {
    const host = root.current
    if (!enabled || !host) return
    // One press at a time: a second finger, or a press while a pane is up, starts nothing.
    let busy = false
    const onDown = (e: PointerEvent) => {
      if (busy || e.button !== 0 || (e.pointerType === 'touch' && !e.isPrimary)) return
      const target = e.target instanceof Element ? e.target : null
      const grip = target?.closest('[data-pane-grip]')
      if (!grip || !host.contains(grip)) return
      // A grip of a drag host nested in this one is that host's (⊞ Arrange's list inside a cockpit's header).
      const owner = grip.parentElement?.closest('[data-pane-drag]')
      if (owner && owner !== host && host.contains(owner)) return
      // A press on a control inside the title is that control's.
      for (let node: Element | null = target; node && node !== grip; node = node.parentElement) if (node.matches(INTERACTIVE)) return
      const o0 = opts.current
      const id = grip.getAttribute('data-pane-grip') as P
      if (!arrangeIds(o0.spec).includes(id) || !o0.shown(id)) return
      start(e, grip, id)
    }
    const start = (e: PointerEvent, grip: Element, id: P) => {
      const touch = e.pointerType === 'touch'
      const x0 = e.clientX
      const y0 = e.clientY
      const pointer = e.pointerId
      let lifted = false
      let last = { x: x0, y: y0 }
      let hold: number | null = null
      let frame: number | null = null
      let measured: MeasuredPlace<P>[] = []
      const can = new Map<string, boolean>()
      let hovered: { drop: PaneDrop<P> } | null = null
      let hotEl: HTMLElement | null = null
      busy = true
      const ids = arrangeIds(opts.current.spec) as readonly string[]
      const areaOf = (pid: P): DropArea | undefined => {
        const o1 = opts.current
        if (o1.sideShows && o1.spec.leftSide && (o1.arrangement.leftSide ?? []).includes(pid)) return 'side'
        const cols = placedColumns(o1.spec, o1.arrangement.place)
        return PANE_COLUMNS.find((c) => cols[c].includes(pid))
      }
      const allowed = (drop: PaneDrop<P>): boolean => {
        const key = `${drop.area} ${drop.before ?? ''}`
        let ok = can.get(key)
        if (ok == null) {
          const o1 = opts.current
          ok = dropArranged(o1.spec, o1.arrangement, id, drop, o1.shown, o1.sideShows, o1.order) != null
          can.set(key, ok)
        }
        return ok
      }
      const measure = () => {
        const o1 = opts.current
        const drawn = o1.targets().map((tg) => ({
          box: tg.el && tg.el.isConnected ? boxOf(tg.el.getBoundingClientRect()) : null,
          areas: tg.areas,
          panes: tg.el && tg.el.isConnected ? panesIn<P>(tg.el, ids) : [],
          el: tg.el,
        }))
        const inside = o1.region.current ?? host
        measured = measurePlaces(drawn, boxOf(inside.getBoundingClientRect()), o1.stacked ?? false)
        return drawn
      }
      // What is really under the pointer: a release counts only over the region or a drawn place — never over
      // something standing above them (the sticky TX strip over a scrolled region, a menu).
      const uncovered = (drawn: ReturnType<typeof measure>, x: number, y: number): boolean => {
        const under = document.elementFromPoint?.(x, y)
        if (!under) return true
        return [opts.current.region.current, ...drawn.map((d) => d.el)].some((el) => el != null && el.contains(under))
      }
      // A place the pane can go: any drop into one of its places that a run of the arrows reaches.
      const reachable = (p: MeasuredPlace<P>) =>
        p.areas.some((area) => [null, ...p.panes.map((q) => q.id)].some((before) => before !== id && allowed({ area, before })))
      const paint = (x: number, y: number) => {
        const drawn = measure()
        const hit = uncovered(drawn, x, y) ? dropAt(measured, x, y, id, areaOf) : null
        const ok = hit != null && allowed(hit.drop)
        hovered = ok ? { drop: hit!.drop } : null
        hotEl = hit != null ? drawn[hit.index]?.el ?? null : null
        const sourceGrip = host.querySelector(`[data-pane-grip="${id}"]`)
        let source: Box | null = null
        for (const p of measured) {
          const mine = p.panes.find((q) => q.id === id)
          if (mine) source = mine.box
        }
        if (!source && sourceGrip) source = boxOf(sourceGrip.getBoundingClientRect())
        store.set({
          source,
          places: measured.flatMap((p, i) =>
            reachable(p)
              ? [{ box: p.box, area: p.areas[0], zone: p.zone, name: p.zone ? opts.current.names[p.areas[0]] : undefined, hot: ok && hit!.index === i }]
              : [],
          ),
          line: ok ? hit!.line : null,
        })
      }
      // At a scroller's edge the drag scrolls it, a little more the nearer the edge, every frame it stays there.
      const edge = () => {
        frame = null
        if (!lifted) return
        // The place under the pointer scrolls (a crowded column), else whatever scrolls the region.
        const scroller = scrollerOf(hotEl ?? host)
        if (!scroller) return
        const r = scroller.getBoundingClientRect()
        const up = last.y - r.top
        const down = r.bottom - last.y
        const by = up < EDGE_PX ? -Math.ceil(EDGE_STEP_PX * (1 - up / EDGE_PX)) : down < EDGE_PX ? Math.ceil(EDGE_STEP_PX * (1 - down / EDGE_PX)) : 0
        if (by === 0 || last.y < r.top || last.y > r.bottom) return
        const was = scroller.scrollTop
        scroller.scrollTop = was + by
        if (scroller.scrollTop === was) return
        paint(last.x, last.y)
        frame = requestAnimationFrame(edge)
      }
      const lift = () => {
        hold = null
        lifted = true
        try {
          ;(grip as HTMLElement).setPointerCapture?.(pointer)
        } catch {
          /* a pointer that is no longer live — the window listeners still follow it */
        }
        window.getSelection?.()?.removeAllRanges()
        document.body.classList.add('pane-dragging')
        paint(last.x, last.y)
      }
      const end = () => {
        if (hold != null) window.clearTimeout(hold)
        if (frame != null) cancelAnimationFrame(frame)
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        window.removeEventListener('pointercancel', cancel)
        window.removeEventListener('keydown', onKey)
        window.removeEventListener('blur', cancel)
        window.removeEventListener('touchmove', onTouchMove)
        window.removeEventListener('contextmenu', onMenu)
        window.removeEventListener('selectstart', onSelect, true)
        document.body.classList.remove('pane-dragging')
        busy = false
        store.onCancel(null)
        if (lifted) store.set(null)
      }
      const cancel = () => end()
      const onMove = (ev: PointerEvent) => {
        if (ev.pointerId !== pointer) return
        last = { x: ev.clientX, y: ev.clientY }
        const moved = Math.hypot(ev.clientX - x0, ev.clientY - y0)
        if (!lifted) {
          // A finger that moves before the hold is up is scrolling; a mouse that moves far enough lifts.
          if (touch) {
            if (moved > TOUCH_SLOP_PX) end()
            return
          }
          if (moved < LIFT_PX) return
          lift()
          return
        }
        paint(ev.clientX, ev.clientY)
        if (frame == null) frame = requestAnimationFrame(edge)
      }
      const onUp = (ev: PointerEvent) => {
        if (ev.pointerId !== pointer) return
        const was = lifted
        if (was) paint(ev.clientX, ev.clientY)
        const drop = hovered?.drop ?? null
        end()
        if (!was) return
        // The click a release sends after a drag is not a press on whatever it lands on (the grip, which holds
        // the pointer, or what both the press and the release were over — inside this host either way).
        const swallow = (c: MouseEvent) => {
          if (!(c.target instanceof Node) || !host.contains(c.target)) return
          c.stopPropagation()
          c.preventDefault()
        }
        window.addEventListener('click', swallow, { capture: true, once: true })
        window.setTimeout(() => window.removeEventListener('click', swallow, { capture: true }), 0)
        if (!drop) return
        const o1 = opts.current
        o1.onDrop(id, drop)
        const place = o1.names[drop.area] ?? ''
        announce(
          drop.before != null
            ? t('panels.drag.dropped.above', { pane: o1.labels[id], place, next: o1.labels[drop.before] })
            : t('panels.drag.dropped.foot', { pane: o1.labels[id], place }),
        )
      }
      // Escape cancels the drag, and is never swallowed: on every screen that arranges, the same press stops
      // transmit (useEscStop).
      const onKey = (ev: KeyboardEvent) => {
        if (ev.key === 'Escape') cancel()
      }
      // A finger held on a title: once the pane is up, the screen does not scroll under it, and no menu opens.
      const onTouchMove = (ev: TouchEvent) => {
        if (lifted) ev.preventDefault()
      }
      const onMenu = (ev: Event) => {
        if (lifted || hold != null) ev.preventDefault()
      }
      // A press on a title that moves would select its text; nothing else about the press is held back, so a
      // plain click on a title still does whatever a click there did.
      const onSelect = (ev: Event) => ev.preventDefault()
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
      window.addEventListener('pointercancel', cancel)
      window.addEventListener('keydown', onKey)
      window.addEventListener('blur', cancel)
      window.addEventListener('selectstart', onSelect, true)
      if (touch) {
        window.addEventListener('touchmove', onTouchMove, { passive: false })
        window.addEventListener('contextmenu', onMenu)
        hold = window.setTimeout(lift, TOUCH_HOLD_MS)
      }
      store.onCancel(cancel)
    }
    host.addEventListener('pointerdown', onDown)
    return () => {
      host.removeEventListener('pointerdown', onDown)
      store.cancel()
    }
  }, [root, enabled, store])
  return store
}

/**
 * What a drag in flight draws, inside `host` — the region, the left side's column or ⊞ Arrange's list — and
 * only the part of it that falls inside `host`, so a mark can never stand over anything outside the place it
 * marks. Nothing while no drag is in flight: not even an empty element, so the host's tree is as it was.
 */
export function PaneDropLayer({ drag, host }: { drag: PaneDrag; host: RefObject<HTMLElement | null> }) {
  const view = useSyncExternalStore(drag.subscribe, drag.view, drag.view)
  const el = host.current
  if (!view || !el) return null
  const r = el.getBoundingClientRect()
  // The host's inside, where its children are drawn: its box less its borders and scrollbars.
  const clip: Box = { left: r.left + el.clientLeft, top: r.top + el.clientTop, width: el.clientWidth, height: el.clientHeight }
  const at = (b: Box): CSSProperties | null => {
    const left = Math.max(b.left, clip.left)
    const top = Math.max(b.top, clip.top)
    const w = Math.min(right(b), right(clip)) - left
    const h = Math.min(bottom(b), bottom(clip)) - top
    if (!(w > 0) || !(h > 0)) return null
    return {
      left: left - r.left - el.clientLeft + el.scrollLeft,
      top: top - r.top - el.clientTop + el.scrollTop,
      width: w,
      height: h,
    }
  }
  const source = view.source ? at(view.source) : null
  const line = view.line ? at(view.line) : null
  return (
    <div className="pane-drop-layer" aria-hidden="true">
      {source && <div className="pane-drop-source" style={source} />}
      {view.places.map((p, i) => {
        const style = at(p.box)
        return style ? (
          <div
            key={i}
            className={`pane-drop-place${p.zone ? ` pane-drop-zone${p.box.height > p.box.width ? ' pane-drop-tall' : ''}` : ''}${p.hot ? ' hot' : ''}`}
            data-drop-area={p.area}
            style={style}
          >
            {p.zone && p.name ? <span className="pane-drop-name">{p.name}</span> : null}
          </div>
        ) : null
      })}
      {line && <div className="pane-drop-line" style={line} />}
    </div>
  )
}
