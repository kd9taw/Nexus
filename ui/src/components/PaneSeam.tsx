// THE PANE SEAM (layout L1, 2026-09-28) — the ONE divider every resizable boundary in the app
// renders. It replaced three: Splitter (a strip's height as a % of its container), SplitterSeam
// (two panes' shares in the panel record) and App's rail resizer (the Tempo rails). Connect's rail
// handles (connect/RailHandles.tsx) were its first instance and keep their own clamp module
// (features/connectRails).
//
// Every PaneSeam, whatever it sizes:
//   · is a FOCUSABLE role="separator": the arrows along its axis move it the way they point, Shift
//     makes the step big, Home/End go to the smallest and largest the pane it sizes may be, and a
//     double-click or Backspace puts the default back (features/paneSeam `seamKey`);
//   · carries aria-valuenow/min/max, so a screen reader says where it stands;
//   · on a pointer drag paints a CSS variable LIVE (no React render per move) and commits ONCE, on
//     release; a click without a move commits nothing (it is half of a double-click);
//   · measures pointer and box in CSS px through `elZoom` (Chromium reports both zoomed);
//   · clamps a value that encodes a size against the LIVE box on load, on every resize and on
//     every step. The stored value is the operator's preference and a re-clamp never writes it,
//     so a bigger window gets it back (the layout contract; the connectRails discipline).
//
// Three kinds share the one element (`PaneSeam` picks by props):
//   · STRIP (`storageKey`): a scope/waterfall height as a % of its container, stored in
//     `nexus.split.<view>.<id>` (Splitter's keys and format, read unchanged). Its range is the
//     declared clamps (SplitClamp, in the sheet's own units) intersected with the range the layout
//     actually HONOURS, measured, because a neighbour can cap a strip below every rule of its own:
//     Phone's scope at 1024×768 stopped at ~242 CSS px under a declared 406, and the rest of the
//     drag was dead. The drag is relative to where it started, so a grab never jumps the divider.
//     A strip whose stock size is the SHEET's own (`defaultPct={null}`, SSTV's picture stage, a
//     grower) is painted nothing until the operator sizes it, carries `data-sized` once sized, and
//     a reset gives the sheet's own size back. A strip may also come AFTER its divider (`after`:
//     Operate's Tx1–Tx6 machine, below its divider), and then moving down shrinks it.
//   · SPLIT (`above`/`below`): two panes' shares in the panel record (panelState.setShares), as
//     `--pane-share` on flex panes or as fr tokens on a grid container (`columnsOn`, optionally
//     named: `columnVars`). Its value is the split as MEASURED on screen, because a pane's stock
//     share is a sheet default the record never saw (Operate's Band Activity : Rx Frequency is
//     1.6 : 1). Reset clears the pair back to that default. The drag maps the pointer's place in
//     the pair, the mapping SplitterSeam had. What it paints is the share × `scale`, so the pair
//     keeps its current total and nothing beside it moves.
//   · VALUE (`value`): the host owns the size (App's Tempo rails, usePaneWidths; a grid cockpit's
//     log column, panes/RegionColumnSeams). The divider steps, drags and resets it; the host
//     clamps and stores.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Its own prose is the
// tooltip; each divider's accessible name is its caller's `label`.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { KeyboardEvent, PointerEvent as ReactPointerEvent, RefObject } from 'react'
import { t } from '../i18n'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { MIN_SHARE, seamShares } from '../features/panelState'
import {
  SEAM_STEP_PX,
  SEAM_STEP_PX_BIG,
  SEAM_STEP_SPLIT,
  SEAM_STEP_SPLIT_BIG,
  parseSplitPct,
  resolveClamp,
  seamKey,
  type SeamAxis,
  type SplitClamp,
  type SplitGeom,
} from '../features/paneSeam'

/** Effective zoom on `el`: `currentCSSZoom` where the engine provides it (Chromium
 *  126+), else the `--ui-zoom` var the app publishes on <html>; 1 when neither reads. */
export function elZoom(el: HTMLElement): number {
  const z = (el as HTMLElement & { currentCSSZoom?: number }).currentCSSZoom
  if (typeof z === 'number' && Number.isFinite(z) && z > 0) return z
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')
  const p = parseFloat(raw)
  return Number.isFinite(p) && p > 0 ? p : 1
}

/** A drag in progress: the pointer → value map (already clamped), the live painter, and the undo
 *  for a cancelled gesture. */
interface SeamDrag {
  at: (ev: PointerEvent) => number
  paint: (v: number) => void
  restore: () => void
}

/** What the one element needs from its kind. Values are in the kind's own unit: CSS px, or a
 *  fraction of a pair of panes. */
interface HandleProps {
  className: string
  axis: SeamAxis
  label: string
  /** Where it stands, for the value attributes. null = nothing measurable (a hidden box): they are
   *  left off rather than invented. */
  value: number | null
  min: number
  max: number
  /** value → the number announced (100 for a split's fraction). */
  ariaScale?: number
  step: number
  bigStep: number
  grows: 1 | -1
  /** Where it stands NOW, measured at key time. Omitted or null ⇒ `value`/`min`/`max`. */
  now?: () => { value: number; min: number; max: number } | null
  /** Clamp, apply and store (a key's step, a drag's release). */
  commit: (v: number) => void
  reset: () => void
  /** Begin a drag, or null when there is nothing to drag (a hidden, zero-size box). */
  drag: (e: ReactPointerEvent<HTMLDivElement>) => SeamDrag | null
}

function SeamHandle(h: HandleProps) {
  // A drag still in flight when the divider goes (its pane hidden, its view left) is cancelled
  // with it, so its window listeners cannot commit into a record on some later release.
  const inFlight = useRef<(() => void) | null>(null)
  useEffect(() => () => inFlight.current?.(), [])

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const at = h.now?.() ?? (h.value == null ? null : { value: h.value, min: h.min, max: h.max })
    const next = seamKey(e, { axis: h.axis, value: 0, min: 0, max: 0, ...at, step: h.step, bigStep: h.bigStep, grows: h.grows })
    // Nothing measurable: only a reset still means something.
    if (next === null || (next !== 'reset' && !at)) return
    e.preventDefault()
    if (next === 'reset') h.reset()
    else h.commit(next)
  }

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    const d = h.drag(e)
    if (!d) return
    e.preventDefault()
    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      /* no live pointer to capture (a synthetic event) — the window listeners still track it */
    }
    let moved = false
    const end = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      window.removeEventListener('pointercancel', cancel)
      document.body.classList.remove('resizing')
      inFlight.current = null
    }
    const move = (ev: PointerEvent) => {
      moved = true
      d.paint(d.at(ev))
    }
    const up = (ev: PointerEvent) => {
      end()
      if (moved) h.commit(d.at(ev))
    }
    const cancel = () => {
      end()
      d.restore()
    }
    document.body.classList.add('resizing')
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
    window.addEventListener('pointercancel', cancel)
    inFlight.current = cancel
  }

  const shown = (v: number) => Math.round(v * (h.ariaScale ?? 1))
  return (
    <div
      className={h.className}
      role="separator"
      tabIndex={0}
      aria-orientation={h.axis === 'y' ? 'horizontal' : 'vertical'}
      aria-label={h.label}
      aria-valuenow={h.value == null ? undefined : shown(h.value)}
      aria-valuemin={h.value == null ? undefined : shown(h.min)}
      aria-valuemax={h.value == null ? undefined : shown(Math.max(h.min, h.max))}
      title={t('paneSeam.title', { label: h.label })}
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onDoubleClick={() => h.reset()}
    />
  )
}

// ── STRIP ───────────────────────────────────────────────────────────────────────────────────

export interface StripSeamProps {
  /** 'y' = the divider drags a HEIGHT (row-resize); 'x' drags a width. */
  axis: SeamAxis
  /** CSS variable the divider drives, e.g. "--cockpit-wf-h": a % of the strip's container. */
  varName: string
  /** The strip being sized. Its PARENT is the flex container its % basis resolves against, so
   *  that is where the variable is set and the box measured — per instance, not on <html>, so a
   *  divider in a detached window (or a kept-alive host) resizes its own strip and never a twin
   *  in another window. The strip comes BEFORE its divider in the tree (the divider sits after
   *  what it sizes), which is also what makes its ref ready when the divider mounts: a
   *  component's layout effects run before an ANCESTOR's ref is attached, never a preceding
   *  sibling's. A strip AFTER its divider says so with `after`. */
  strip: RefObject<HTMLElement | null>
  /** The strip sits AFTER its divider (below it, or to its right): moving the divider down or
   *  right SHRINKS it, the arrows and a drag act the other way round, and its ref is not attached
   *  yet when the divider's layout effect first runs (a FOLLOWING sibling's), so the divider takes
   *  one more synchronous pass, before anything paints, to find it. Operate's Tx1–Tx6 machine
   *  under its divider. */
  after?: boolean
  /** localStorage key (nexus.split.<section>.<id>). PER-SURFACE — scoped here rather than at the
   *  call sites, so a split can never be shared between a window and a pop-out with a different
   *  aspect. */
  storageKey: string
  /** Clamps for the strip, in the sheet's own units where the sheet sets them (SplitClamp). */
  min: SplitClamp
  max: SplitClamp
  /** Default size as a percentage of the container (until the first move, and after a reset).
   *  `null`: the strip keeps the SHEET's OWN size — a grower no fixed share reproduces at every
   *  window, SSTV's picture stage — until the operator sizes it, and a reset gives that back.
   *  Nothing is painted until then; a sized strip carries `data-sized`, the attribute its sheet
   *  rule keys the sized shape on (grow 0, this variable as the basis). */
  defaultPct: number | null
  /** Accessible name. */
  label: string
  /** The most of its container the strip may take, as a share (default 0.9, the drag's bound since
   *  Splitter). A strip that shares its container with something that must keep its room says so
   *  here: Connect's bottom strip, which leaves the map at least half of the grid (layout L7). */
  maxShare?: number
}

/** The container's CONTENT box along the axis, in CSS px: a flex item's % basis resolves against
 *  it, not against the border box the rect reports. 0 for a hidden box. */
function contentSpan(el: HTMLElement, axis: SeamAxis, z: number): number {
  const r = el.getBoundingClientRect()
  const outer = (axis === 'y' ? r.height : r.width) / z
  if (!(outer > 0)) return 0
  const cs = getComputedStyle(el)
  const px = (v: string) => {
    const n = parseFloat(v)
    return Number.isFinite(n) ? n : 0
  }
  const inset =
    axis === 'y'
      ? px(cs.paddingTop) + px(cs.paddingBottom) + px(cs.borderTopWidth) + px(cs.borderBottomWidth)
      : px(cs.paddingLeft) + px(cs.paddingRight) + px(cs.borderLeftWidth) + px(cs.borderRightWidth)
  return Math.max(0, outer - inset)
}

/** The geometry `el` lives in, for resolving a SplitClamp. `--vh-eff` is published on <html> by
 *  useViewport; the fallback matches its own formula, so a divider clamps sanely before the first
 *  stamp (and in tests). */
function splitGeom(el: HTMLElement, z: number): SplitGeom {
  const fontPx = parseFloat(getComputedStyle(el).fontSize)
  const vh = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--vh-eff'))
  return {
    fontPx: Number.isFinite(fontPx) && fontPx > 0 ? fontPx : 16,
    vhEff: Number.isFinite(vh) && vh > 0 ? vh : window.innerHeight / z,
  }
}

/** The range the LAYOUT honours for a strip, measured: its rendered size with the variable at 0
 *  and at a size no box can take. That catches the sheet's own min/max AND every limit a
 *  neighbour sets (a sibling's floor the container must still fit), which no declared clamp can.
 *  Both writes are undone before anything paints. null when the strip is not laid out (a hidden
 *  box, or jsdom), which leaves the declared clamps in charge. */
function honouredRange(
  strip: HTMLElement,
  target: HTMLElement,
  varName: string,
  axis: SeamAxis,
  z: number,
  /** A strip whose sized shape keys on `data-sized` is measured in that shape. */
  sizedMark: boolean,
): [number, number] | null {
  const size = () => {
    const r = strip.getBoundingClientRect()
    return (axis === 'y' ? r.height : r.width) / z
  }
  const was = target.style.getPropertyValue(varName)
  const marked = strip.hasAttribute('data-sized')
  if (sizedMark && !marked) strip.setAttribute('data-sized', '')
  target.style.setProperty(varName, '0px')
  const lo = size()
  target.style.setProperty(varName, '100000px')
  const hi = size()
  if (was) target.style.setProperty(varName, was)
  else target.style.removeProperty(varName)
  if (sizedMark && !marked) strip.removeAttribute('data-sized')
  return hi > 0 ? [lo, hi] : null
}

interface StripBox {
  el: HTMLElement
  /** Content span, CSS px. */
  span: number
  z: number
  lo: number
  hi: number
}

function StripSeam({ axis, varName, strip, storageKey, min, max, defaultPct, label, after = false, maxShare = 0.9 }: StripSeamProps) {
  const container = () => strip.current?.parentElement ?? null
  // The operator's PREFERENCE as stored (null: none, and the sheet's own size stands — only for a
  // strip whose default is the sheet's). A re-clamp never writes it. `undefined` = not read yet.
  const pref = useRef<number | null | undefined>(undefined)
  if (pref.current === undefined) pref.current = parseSplitPct(surfaceGet(storageKey)) ?? defaultPct
  // The % on the container now: the preference fitted into the live box, or a drag in flight;
  // null while the strip stands at the sheet's own size.
  const painted = useRef<number | null>(pref.current)
  const [view, setView] = useState<{ px: number; lo: number; hi: number } | null>(null)
  // A strip whose stock size is the sheet's own is MARKED while a size is painted: its sheet rule
  // keys the sized shape (grow 0, the variable as the basis) on `data-sized`.
  const ownSize = defaultPct == null
  const mark = (on: boolean) => {
    const s = strip.current
    if (!ownSize || !s) return
    if (on) s.setAttribute('data-sized', '')
    else s.removeAttribute('data-sized')
  }
  /** The strip's rendered size, CSS px. */
  const stripSize = (z: number) => {
    const r = strip.current?.getBoundingClientRect()
    return r ? (axis === 'y' ? r.height : r.width) / z : 0
  }

  const box = (): StripBox | null => {
    const el = container()
    if (!el) return null
    const z = elZoom(el)
    const span = contentSpan(el, axis, z)
    if (!(span > 0)) return null
    const g = splitGeom(el, z)
    let lo = resolveClamp(min, g)
    let hi = Math.min(resolveClamp(max, g), maxShare * span)
    const honoured = honouredRange(strip.current!, el, varName, axis, z, ownSize)
    if (honoured) {
      lo = Math.max(lo, honoured[0])
      hi = Math.min(hi, honoured[1])
    }
    // The layout wins a disagreement: past its ceiling the divider could not move at all.
    return { el, span, z, lo: Math.min(lo, hi), hi }
  }
  const clampIn = (b: StripBox, px: number) => Math.min(b.hi, Math.max(b.lo, px))
  /** Paint a size (CSS px) as the container's %, with no React render (a drag's moves). */
  const write = (b: StripBox, px: number) => {
    const pct = (px / b.span) * 100
    b.el.style.setProperty(varName, `${pct}%`)
    mark(true)
    painted.current = pct
    return pct
  }
  /** Paint AND publish (a load, a resize, a step): the value attributes follow. */
  const settle = (b: StripBox, px: number) => {
    const pct = write(b, px)
    setView((v) => (v && v.px === px && v.lo === b.lo && v.hi === b.hi ? v : { px, lo: b.lo, hi: b.hi }))
    return pct
  }
  /** Re-fit the PREFERENCE into the box as it is now: load, every resize, a reset. */
  const fit = () => {
    const pct = pref.current
    if (pct == null) {
      // No preference and the sheet's own size stands: nothing painted, nothing marked, and the
      // value is what renders (the range is measured in the sized shape all the same, and
      // stretched to hold that value, so what is announced is always inside it).
      container()?.style.removeProperty(varName)
      mark(false)
      painted.current = null
      const b = box()
      if (!b) return
      const px = stripSize(b.z)
      const lo = Math.min(b.lo, px)
      const hi = Math.max(b.hi, px)
      setView((v) => (v && v.px === px && v.lo === lo && v.hi === hi ? v : { px, lo, hi }))
      return
    }
    const b = box()
    if (!b) {
      // Hidden (a keep-alive host's 0×0): the stored % raw, as Splitter's mount did. The next real
      // box is re-fitted before it paints.
      container()?.style.setProperty(varName, `${pct}%`)
      mark(true)
      painted.current = pct
      return
    }
    settle(b, clampIn(b, (pct / 100) * b.span))
  }
  /** Where the strip stands, CSS px: the painted size, or the sheet's own. */
  const current = (b: StripBox) => clampIn(b, painted.current != null ? (painted.current / 100) * b.span : stripSize(b.z))
  const fitRef = useRef(fit)
  fitRef.current = fit

  // Before first paint, and on every resize of the container: a window resize, a zoom change, and
  // a kept-alive host shown again (it mounted at 0×0, where nothing could be clamped).
  // A strip at the sheet's own size can also change on its own (a grower, as its neighbours come
  // and go), so that one is observed too.
  // A strip after its divider has no ref yet on the first pass (above): one more pass, which
  // React runs synchronously before paint because it is scheduled from a layout effect.
  const [findTries, findAgain] = useState(0)
  useLayoutEffect(() => {
    if (!strip.current) {
      if (after && findTries === 0) findAgain(1)
      return
    }
    fitRef.current()
    const el = strip.current?.parentElement
    if (!el || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => fitRef.current())
    ro.observe(el)
    if (ownSize && strip.current) ro.observe(strip.current)
    return () => ro.disconnect()
  }, [strip, ownSize, after, findTries])

  const commit = (px: number) => {
    const b = box()
    if (!b) return
    const pct = settle(b, clampIn(b, px))
    pref.current = pct
    surfaceSet(storageKey, String(pct))
  }

  return (
    <SeamHandle
      className={`pane-splitter ${axis === 'y' ? 'horizontal' : 'vertical-inline'}`}
      axis={axis}
      label={label}
      value={view?.px ?? null}
      min={view?.lo ?? 0}
      max={view?.hi ?? 0}
      step={SEAM_STEP_PX}
      bigStep={SEAM_STEP_PX_BIG}
      grows={after ? -1 : 1}
      now={() => {
        const b = box()
        return b ? { value: current(b), min: b.lo, max: b.hi } : null
      }}
      commit={commit}
      reset={() => {
        pref.current = defaultPct
        // '' reads back as "never set" (parseSplitPct), which is what a sheet-sized default is.
        surfaceSet(storageKey, defaultPct == null ? '' : String(defaultPct))
        fit()
      }}
      drag={(e) => {
        const b = box()
        if (!b) return null
        // Relative to where the drag STARTED, from where the strip IS: a grab off-centre never
        // jumps the divider, and the pointer and the divider move together. From the sheet's own
        // size that is its rendered size, so the first paint lands exactly where it stood.
        const start = current(b)
        const p0 = axis === 'y' ? e.clientY : e.clientX
        const was = painted.current
        return {
          at: (ev) => clampIn(b, start + ((after ? -1 : 1) * ((axis === 'y' ? ev.clientY : ev.clientX) - p0)) / b.z),
          paint: (px) => write(b, px),
          restore: () => {
            if (was == null) {
              b.el.style.removeProperty(varName)
              mark(false)
            } else b.el.style.setProperty(varName, `${was}%`)
            painted.current = was
          },
        }
      }}
    />
  )
}

// ── SPLIT ───────────────────────────────────────────────────────────────────────────────────

export interface SplitSeamProps {
  /** The pane above/left of the divider (grows when it moves down/right). */
  above: RefObject<HTMLElement | null>
  /** The pane below/right of the divider (grows when it moves up/left). */
  below: RefObject<HTMLElement | null>
  /** The share variable: set on each pane (flex), or as `${varName}-a`/`-b` fr tokens on
   *  `columnsOn` — a grid template cannot read a variable off its children. */
  varName: string
  /** Store the settled shares (panelState.setShares: one undoable step). */
  onCommit: (aboveShare: number, belowShare: number) => void
  /** Back to the sheet's stock split: the host clears both shares from its record. */
  onReset: () => void
  /** Accessible name. */
  label: string
  /** 'y' (default) splits stacked panes; 'x' splits side-by-side columns. */
  axis?: SeamAxis
  /** Grid-column mode: the container whose template consumes the fr tokens. */
  columnsOn?: RefObject<HTMLElement | null>
  /** Classes that place this divider in its container, beside its own (a grid cockpit's column
   *  divider rides the gap before its track: cockpit-panes.css `.cockpit-colseam`). */
  className?: string
  /** The grow (or fr) a share of 1 stands for: what is painted is share × scale. The shares
   *  committed are the pair's own (seamShares, summing to 2); the painted values keep the pair's
   *  current total, so a pair of unequal weights among other growers moves only its own boundary:
   *  fill panes in a column (CockpitPaneFrame `split`), and fr columns in a grid of three, where a
   *  pair painted at a new total would move the third column (Operate's Classic, layout L5).
   *  Default 1. */
  scale?: number
  /** Grid-column mode: the two fr tokens to paint, the first for `above`, when they are not
   *  `${varName}-a`/`-b` — a column shared by two dividers is one token read by both (Operate's
   *  Classic: Band Activity | the Rx Frequency column | Stations). */
  columnVars?: readonly [string, string]
  /** The two panes' own floors along the axis, CSS px (a template's `minmax(<floor>, …)`): the
   *  divider stops where either reaches its floor, and announces that as its range. Past a floor
   *  the grid freezes the floored track and hands the rest of the move to the tracks NOT in the
   *  pair — Operate's Classic at 1024×768 took 9 px from Band Activity on the Stations divider's
   *  first step — and the pointer goes on while the divider does not. */
  floors?: readonly [number, number]
}

/** A pane never goes below MIN_SHARE, so the divider never leaves this span of the pair. */
const SPLIT_LO = MIN_SHARE / 2
const SPLIT_HI = 1 - MIN_SHARE / 2

function SplitSeam({ above, below, varName, onCommit, onReset, label, axis = 'y', columnsOn, className, scale = 1, columnVars, floors }: SplitSeamProps) {
  // Read through a ref: a host builds the pair per render, and the measurement must not be
  // re-created (and its observers re-attached) every time it does.
  const floorsRef = useRef(floors)
  floorsRef.current = floors
  /** The split on screen — the first pane's fraction of the two — and the range it may take:
   *  SPLIT_LO … SPLIT_HI, narrowed to where neither pane is below its floor. null when nothing is
   *  laid out. */
  const measure = useCallback((): { f: number; lo: number; hi: number } | null => {
    const a = above.current
    const b = below.current
    if (!a || !b) return null
    const ra = a.getBoundingClientRect()
    const rb = b.getBoundingClientRect()
    const sa = axis === 'x' ? ra.width : ra.height
    const sb = axis === 'x' ? rb.width : rb.height
    if (!(sa + sb > 0)) return null
    let lo = SPLIT_LO
    let hi = SPLIT_HI
    const fl = floorsRef.current
    if (fl) {
      // Rects are zoomed; the floors are CSS px.
      const z = elZoom(a)
      const flo = (fl[0] * z) / (sa + sb)
      const fhi = 1 - (fl[1] * z) / (sa + sb)
      // Both panes ON their floors meet at one split (to rounding — measured at 1024×768 with a
      // stored layout at the far end, the two ends came out a hair crossed), and the divider stays
      // there. Floors the pair cannot both keep (a grid already overflowing) narrow nothing.
      if (flo <= fhi + 1e-6) {
        lo = Math.max(lo, Math.min(flo, fhi))
        hi = Math.min(hi, Math.max(flo, fhi))
      }
    }
    return { f: sa / (sa + sb), lo, hi }
  }, [above, below, axis])
  const [measured, setMeasured] = useState<{ f: number; lo: number; hi: number } | null>(null)
  const later = useRef(0)
  const remeasure = useCallback(() => {
    cancelAnimationFrame(later.current)
    later.current = requestAnimationFrame(() => setMeasured(measure()))
  }, [measure])
  // Measured once the commit is done, on every window resize (where a column's floor can start or
  // stop binding), whenever either pane changes size by itself, and after each commit of the
  // divider's own (below). The panes' own sizes are observed because a floor starts binding with
  // no window resize at all: a neighbour arriving after the first layout (measured in Chrome at
  // 1024×768: RTTY's waterfall came in 34 ms after the cockpit, took its 25 % from the pair and
  // left both panes at their floors, 50 %, while the divider still announced the stock 40 % it had
  // measured), a neighbour's divider, a pane shown beside them. Not in a layout effect: the pane
  // after the divider (Rx Frequency under its seam, Classic's Stations aside) has no ref yet while
  // the divider's layout effects run. Only the announced value waits for it — a key or a drag
  // measures at the moment it acts. A ratio needs no clamp against the box: the record's shares
  // are clamped on load (coercePanelLayout) and on every write (seamShares).
  useEffect(() => {
    setMeasured(measure())
    window.addEventListener('resize', remeasure)
    const ro = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => remeasure())
    for (const el of [above.current, below.current]) if (el) ro?.observe(el)
    return () => {
      window.removeEventListener('resize', remeasure)
      ro?.disconnect()
      cancelAnimationFrame(later.current)
    }
  }, [measure, remeasure, above, below])

  const clampIn = (f: number, r: { lo: number; hi: number } | null) =>
    Math.min(r?.hi ?? SPLIT_HI, Math.max(r?.lo ?? SPLIT_LO, f))
  /** The painted properties, as [element, property] pairs. */
  const targets = (): Array<[HTMLElement, string]> => {
    const c = columnsOn?.current
    if (c) return [[c, columnVars?.[0] ?? `${varName}-a`], [c, columnVars?.[1] ?? `${varName}-b`]]
    const a = above.current
    const b = below.current
    return a && b ? [[a, varName], [b, varName]] : []
  }
  const paint = (f: number) => {
    const [av, bv] = seamShares(f)
    const [pa, pb] = targets()
    if (!pa || !pb) return
    const unit = columnsOn?.current != null ? 'fr' : ''
    pa[0].style.setProperty(pa[1], `${av * scale}${unit}`)
    pb[0].style.setProperty(pb[1], `${bv * scale}${unit}`)
  }

  return (
    <SeamHandle
      className={`pane-splitter ${axis === 'x' ? 'col-seam' : 'horizontal'} seam${className ? ` ${className}` : ''}`}
      axis={axis}
      label={label}
      value={measured?.f ?? null}
      min={measured?.lo ?? SPLIT_LO}
      max={measured?.hi ?? SPLIT_HI}
      ariaScale={100}
      step={SEAM_STEP_SPLIT}
      bigStep={SEAM_STEP_SPLIT_BIG}
      grows={1}
      now={() => {
        const m = measure()
        return m == null ? null : { value: m.f, min: m.lo, max: m.hi }
      }}
      commit={(f) => {
        const [av, bv] = seamShares(clampIn(f, measure()))
        onCommit(av, bv)
        remeasure()
      }}
      reset={() => {
        onReset()
        remeasure()
      }}
      drag={() => {
        const a = above.current
        const b = below.current
        if (!a || !b) return null
        // The pair spans the start of the pane above/left to the end of the pane below/right. A
        // ratio of two zoomed lengths needs no zoom correction.
        const ra = a.getBoundingClientRect()
        const rb = b.getBoundingClientRect()
        const lo = axis === 'x' ? ra.left : ra.top
        const span = (axis === 'x' ? rb.right : rb.bottom) - lo
        if (!(span > 0)) return null
        // What the panes carried before the drag, so a cancel puts exactly that back; and the
        // range as it is at the grab (the floors stop the drag where they stop the layout).
        const props = targets()
        const was = props.map(([el, p]) => el.style.getPropertyValue(p))
        const range = measure()
        return {
          at: (ev) => clampIn(((axis === 'x' ? ev.clientX : ev.clientY) - lo) / span, range),
          paint,
          restore: () =>
            props.forEach(([el, p], i) => (was[i] ? el.style.setProperty(p, was[i]) : el.style.removeProperty(p))),
        }
      }}
    />
  )
}

// ── VALUE ───────────────────────────────────────────────────────────────────────────────────

export interface ValueSeamProps {
  axis: SeamAxis
  /** The class that places this divider in its grid, beside `pane-splitter`. */
  className: string
  /** The size it stands at, CSS px, and the host's clamps for it. null = nothing measurable (a
   *  hidden box): no values are announced and only a reset acts. */
  value: number | null
  min: number
  max: number
  /** +1: moving the divider down/right grows what it sizes; −1: moving it up/left does. */
  grows: 1 | -1
  /** Paint a size live, mid-drag — no React render. */
  onPaint: (v: number) => void
  /** Apply and store a size (a key's step, a drag's release). */
  onCommit: (v: number) => void
  /** This divider's default back. */
  onReset: () => void
  /** A cancelled drag: put back exactly what was painted before it. Omitted ⇒ paint the size it
   *  started at, which is right for a host that always paints a size (the Tempo rails) and wrong
   *  for one whose default is NO value at all (a grid cockpit's log column). */
  onCancel?: () => void
  /** Accessible name. */
  label: string
}

function ValueSeam({ axis, className, value, min, max, grows, onPaint, onCommit, onReset, onCancel, label }: ValueSeamProps) {
  // The floor wins a disagreement: a pane the operator cannot grab back is worse than a ceiling
  // overrun on a window below the supported minimum.
  const hi = Math.max(min, max)
  const clampV = (v: number) => Math.min(hi, Math.max(min, v))
  return (
    <SeamHandle
      className={`pane-splitter ${className}`}
      axis={axis}
      label={label}
      value={value}
      min={min}
      max={hi}
      step={SEAM_STEP_PX}
      bigStep={SEAM_STEP_PX_BIG}
      grows={grows}
      commit={(v) => onCommit(clampV(v))}
      reset={onReset}
      drag={(e) => {
        if (value == null) return null
        // Relative to where the drag started, in CSS px.
        const z = elZoom(e.currentTarget)
        const p0 = axis === 'x' ? e.clientX : e.clientY
        return {
          at: (ev) => clampV(value + (grows * ((axis === 'x' ? ev.clientX : ev.clientY) - p0)) / z),
          paint: onPaint,
          restore: onCancel ?? (() => onPaint(value)),
        }
      }}
    />
  )
}

// ── THE ONE COMPONENT ───────────────────────────────────────────────────────────────────────

export type PaneSeamProps = StripSeamProps | SplitSeamProps | ValueSeamProps

/** A divider. The props say what it sizes: a strip (`storageKey`), a pair of panes (`above`), or a
 *  size its host owns (`value`). A call site's kind never changes, so the element never remounts. */
export function PaneSeam(props: PaneSeamProps) {
  if ('storageKey' in props) return <StripSeam {...props} />
  if ('above' in props) return <SplitSeam {...props} />
  return <ValueSeam {...props} />
}
