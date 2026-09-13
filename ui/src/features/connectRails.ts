// Connect's rail widths (close + resize, 2026-09-13) — the pure clamp and the per-surface
// record behind the two draggable rail separators. Pure apart from storage, so every rule is
// unit-tested without a layout engine; components/connect/RailHandles.tsx measures the box.
//
// THE LAYOUT CONTRACT'S RULE, and why it shapes everything here: anything persisted that
// encodes a size is CLAMPED ON LOAD against the current window, not only at drag time. A rail
// dragged wide on a 3440 monitor must not open a 1024 pop-out with the map squeezed to nothing.
// So the STORED value is the operator's preference and is never rewritten by a re-clamp (the
// usePaneWidths pattern — a bigger window gets the width back); what the grid renders is that
// preference fitted into the box it is in right now.
//
// THE INVARIANTS, each of which a real browser once caught this file breaking (2026-09-13):
//   · every open rail renders within [RAIL_MIN, RAIL_MAX], and the pair never sums past the
//     room — so the map keeps MAP_MIN whenever two floored rails leave it. Widths round DOWN:
//     rounding a share up is how a "fitted" pair overran the map by 3 px. The DEFAULT rails obey
//     it too — two 248 px tier defaults left a 1024×768 window at 125% with a 254 px map.
//   · moving ONE rail never changes the OTHER rail's displayed width (stepRail / resetRail).
//     The move clamps against what the other rail shows, and leaves it showing exactly that.
//     Writing the other rail's raw preference back instead is how one arrow key once put a
//     squeezed 307 px rail back to its stored 500 and dropped the map to 87 px.
//   · a window resize or reload re-fits from the preferences (fitRails). When the pair does not
//     fit, the rail the operator set LAST keeps its width and the other gives way — which makes
//     a reload after a move show the moved rail exactly as moved, and never narrower than either
//     rail was on screen before it.
//
// All numbers are CSS px (zoom-corrected by the caller), the unit the grid template reads.
import { surfaceGet, surfaceSet } from './windowScope'

/** Narrowest a rail may be dragged: the band advisor's row (band · bar · MUF) still fits. */
export const RAIL_MIN = 200
/** Widest a rail may be dragged on any window — past this a rail is a second map. */
export const RAIL_MAX = 720
/** The map never gets less than this from the rails (the xs stack's own globe floor). */
export const MAP_MIN = 280
/** Keyboard steps: an arrow, and Shift + an arrow. */
export const RAIL_STEP = 16
export const RAIL_STEP_BIG = 64

export type RailSide = 'left' | 'right'

/** A width per side; null = never sized (stored) / the tier default (applied). `last` names the
 *  side the operator set most recently — the one that keeps its width when a pair must shrink. */
export interface RailWidths {
  left: number | null
  right: number | null
  last?: RailSide
}

/** The box a fit happens in: the room the rails may share, which rails are open, and the tier
 *  default an unsized rail renders at. */
export interface RailFitContext {
  room: number
  present: { left: boolean; right: boolean }
  defaultPx: number
}

/** A move's inputs and outputs: the stored preferences, and what the grid renders now. */
export interface RailState {
  pref: RailWidths
  applied: RailWidths
}

// PER-SURFACE: a width is a statement about ONE window's shape. A pop-out inherits the main
// window's on first open (surfaceGet) and is clamped against its own box before it paints.
const RAIL_KEY = 'nexus.connect.railWidths'

const otherSide = (side: RailSide): RailSide => (side === 'left' ? 'right' : 'left')

/**
 * The width the two rails may share: the grid box minus its padding, the gaps between the
 * columns that render, and the map's floor. An unmeasurable box (0×0 — a hidden keep-alive
 * host, or jsdom) is UNBOUNDED rather than zero: clamping against it would floor every rail.
 */
export function railRoom(boxW: number, pad: number, gap: number, columns: number): number {
  if (!(boxW > 0)) return Infinity
  const p = Number.isFinite(pad) ? pad : 0
  const g = Number.isFinite(gap) ? gap : 0
  return boxW - 2 * p - g * Math.max(0, columns - 1) - MAP_MIN
}

const bound = (px: number) => Math.round(Math.max(RAIL_MIN, Math.min(RAIL_MAX, px)))

/** One rail against the room the other rail leaves, rounding the room DOWN so the pair can
 *  never overrun it. Never below RAIL_MIN — a rail the operator cannot grab back is worse than
 *  a map a few pixels short of its floor on a window below the supported minimum. */
export function clampRail(px: number, otherPx: number, room: number): number {
  return Math.max(RAIL_MIN, Math.min(RAIL_MAX, Math.floor(room - otherPx), Math.round(px)))
}

/**
 * Fit the preferences into the current box — the LOAD and WINDOW-RESIZE path.
 *
 * Each open rail wants its stored width, or the tier default when it has none. If the pair fits,
 * that is what renders, and an unsized rail comes back null so the stylesheet default stays in
 * charge (which is what keeps the unconfigured layout byte-identical wherever it already fits).
 * If it does not fit:
 *   · the rail the operator set LAST (or the only stored rail, for a record written before
 *     `last` existed) keeps its want, down to leaving RAIL_MIN for the other, and the other takes
 *     the rest;
 *   · otherwise — both unsized, or both stored with no `last` — both shrink in proportion; a
 *     share that would drop under RAIL_MIN stays at RAIL_MIN and the OTHER side pays for it;
 *   · if even two floored rails do not leave the map MAP_MIN (a window below the supported
 *     floor), both hold RAIL_MIN: the map gives, every handle stays grabbable, nothing is trapped.
 */
export function fitRails(pref: RailWidths, ctx: RailFitContext): RailWidths {
  const { room, present, defaultPx } = ctx
  const def = bound(defaultPx)
  const want = (side: RailSide) => bound(pref[side] ?? defaultPx)
  /** An unsized rail rendering exactly its default is left to the stylesheet. */
  const out = (side: RailSide, v: number) => (pref[side] == null && v === def ? null : v)
  /** A closed rail renders nothing; its applied value is inert. */
  const closed = (side: RailSide) => (pref[side] == null ? null : bound(pref[side]!))

  if (!present.left || !present.right) {
    const one = (side: RailSide) =>
      present[side] ? out(side, Math.max(RAIL_MIN, Math.min(want(side), Math.floor(room)))) : closed(side)
    return { left: one('left'), right: one('right') }
  }

  let l = want('left')
  let r = want('right')
  if (l + r > room) {
    if (room < 2 * RAIL_MIN) {
      l = RAIL_MIN
      r = RAIL_MIN
    } else {
      const anchor: RailSide | null =
        pref.last && pref[pref.last] != null
          ? pref.last
          : pref.left != null && pref.right == null
            ? 'left'
            : pref.right != null && pref.left == null
              ? 'right'
              : null
      if (anchor) {
        const a = Math.min(anchor === 'left' ? l : r, Math.floor(room - RAIL_MIN))
        const b = Math.max(RAIL_MIN, Math.min(anchor === 'left' ? r : l, Math.floor(room - a)))
        ;[l, r] = anchor === 'left' ? [a, b] : [b, a]
      } else {
        const k = room / (l + r)
        let sl = Math.floor(l * k)
        let sr = Math.floor(r * k)
        if (sl < RAIL_MIN) {
          sl = RAIL_MIN
          sr = Math.min(r, Math.floor(room - RAIL_MIN))
        } else if (sr < RAIL_MIN) {
          sr = RAIL_MIN
          sl = Math.min(l, Math.floor(room - RAIL_MIN))
        }
        l = sl
        r = sr
      }
    }
  }
  return { left: out('left', l), right: out('right', r) }
}

/** What the other rail shows right now (0 when it is closed). */
function shownOther(state: RailState, side: RailSide, ctx: RailFitContext): number {
  const o = otherSide(side)
  return ctx.present[o] ? (state.applied[o] ?? bound(ctx.defaultPx)) : 0
}

/**
 * Move ONE rail to `px` (a keyboard step, a drag's release). It is clamped against the other
 * rail's DISPLAYED width, and the other rail is left displaying exactly that — its applied value
 * is untouched and its stored preference kept for a bigger window. The moved rail becomes
 * `last`, so a re-fit keeps it as moved.
 */
export function stepRail(state: RailState, side: RailSide, px: number, ctx: RailFitContext): RailState {
  const v = clampRail(px, shownOther(state, side, ctx), ctx.room)
  const pick = (s: RailSide, fallback: number | null) => (s === side ? v : fallback)
  return {
    pref: { left: pick('left', state.pref.left), right: pick('right', state.pref.right), last: side },
    applied: { left: pick('left', state.applied.left), right: pick('right', state.applied.right) },
  }
}

/**
 * Put ONE rail back to the tier default (a double-click), as far as the box allows beside the
 * other rail — which, as with a step, stays exactly where it is on screen.
 */
export function resetRail(state: RailState, side: RailSide, ctx: RailFitContext): RailState {
  const v = clampRail(ctx.defaultPx, shownOther(state, side, ctx), ctx.room)
  const applied = v === bound(ctx.defaultPx) ? null : v
  const last = state.pref.last === side ? undefined : state.pref.last
  const pref: RailWidths = {
    left: side === 'left' ? null : state.pref.left,
    right: side === 'right' ? null : state.pref.right,
  }
  if (last) pref.last = last
  return {
    pref,
    applied: { left: side === 'left' ? applied : state.applied.left, right: side === 'right' ? applied : state.applied.right },
  }
}

/** A stored record from any input: junk, negatives, non-numbers and non-finite values are
 *  "never sized", never a throw. `last` survives only when it names a side that is stored. */
export function parseRailWidths(raw: string | null): RailWidths {
  const out: RailWidths = { left: null, right: null }
  if (!raw) return out
  let obj: unknown
  try {
    obj = JSON.parse(raw)
  } catch {
    return out
  }
  if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return out
  const rec = obj as Record<string, unknown>
  for (const side of ['left', 'right'] as const) {
    const v = rec[side]
    if (typeof v === 'number' && Number.isFinite(v) && v > 0) out[side] = Math.round(v)
  }
  if ((rec.last === 'left' || rec.last === 'right') && out[rec.last] != null) out.last = rec.last
  return out
}

export function loadRailWidths(): RailWidths {
  return parseRailWidths(surfaceGet(RAIL_KEY))
}

/** Persist the preference. A side back at its default is simply absent from the record. */
export function saveRailWidths(w: RailWidths): void {
  const out: Partial<Record<RailSide, number>> & { last?: RailSide } = {}
  if (w.left != null) out.left = w.left
  if (w.right != null) out.right = w.right
  if (w.last && w[w.last] != null) out.last = w.last
  surfaceSet(RAIL_KEY, JSON.stringify(out))
}
