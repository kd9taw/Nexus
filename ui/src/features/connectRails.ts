// Connect's rail widths (close + resize, 2026-09-13) — the pure clamp and the per-surface
// record behind the two draggable rail separators. Pure apart from storage, so the clamp is
// unit-tested without a layout engine; components/connect/RailHandles.tsx measures the box.
//
// THE LAYOUT CONTRACT'S RULE, and why it shapes everything here: anything persisted that
// encodes a size is CLAMPED ON LOAD against the current window, not only at drag time. A rail
// dragged wide on a 3440 monitor must not open a 1024 pop-out with the map squeezed to nothing.
// So the STORED value is the operator's preference and is never rewritten by a re-clamp (the
// usePaneWidths pattern — a bigger window gets the width back); what the grid renders is that
// preference fitted into the box it is in right now.
//
// All numbers are CSS px (zoom-corrected by the caller), the unit the grid template reads.
import { surfaceGet, surfaceSet } from './windowScope'

/** Narrowest a rail may be dragged: the band advisor's row (band · bar · MUF) still fits. */
export const RAIL_MIN = 200
/** Widest a rail may be dragged on any window — past this a rail is a second map. */
export const RAIL_MAX = 720
/** The map never gets less than this from a rail drag (the xs stack's own globe floor). */
export const MAP_MIN = 280
/** Keyboard steps: an arrow, and Shift + an arrow. */
export const RAIL_STEP = 16
export const RAIL_STEP_BIG = 64

export type RailSide = 'left' | 'right'

/** A stored width per side; null = never sized, so the stylesheet's tier default applies. */
export interface RailWidths {
  left: number | null
  right: number | null
}

// PER-SURFACE: a width is a statement about ONE window's shape. A pop-out inherits the main
// window's on first open (surfaceGet) and is clamped against its own box before it paints.
const RAIL_KEY = 'nexus.connect.railWidths'

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

/** One rail against the room the other rail leaves. Never below RAIL_MIN — a rail the
 *  operator cannot grab back is worse than a map a few pixels short of its floor. */
export function clampRail(px: number, otherPx: number, room: number): number {
  return Math.round(Math.max(RAIL_MIN, Math.min(RAIL_MAX, room - otherPx, px)))
}

/**
 * Fit the stored preferences into the current box. Only STORED sides come back non-null: an
 * unsized rail keeps the stylesheet default, which is what makes the unconfigured layout
 * exactly the pre-feature one. `defaultPx` is that default, used as the other rail's width
 * when only one side is stored. Two stored rails that overflow shrink TOGETHER, in proportion,
 * so the second one is not starved to the floor by the first.
 */
export function fitRails(
  pref: RailWidths,
  ctx: { room: number; present: { left: boolean; right: boolean }; defaultPx: number },
): RailWidths {
  const { room, present, defaultPx } = ctx
  if (pref.left != null && pref.right != null && present.left && present.right) {
    let l = bound(pref.left)
    let r = bound(pref.right)
    if (l + r > room) {
      const k = room / (l + r)
      l = bound(l * k)
      r = bound(r * k)
    }
    return { left: l, right: r }
  }
  const other = (side: RailSide) => {
    const o = side === 'left' ? 'right' : 'left'
    return present[o] ? (pref[o] ?? defaultPx) : 0
  }
  const one = (side: RailSide): number | null => {
    const v = pref[side]
    if (v == null) return null
    return present[side] ? clampRail(v, other(side), room) : bound(v)
  }
  return { left: one('left'), right: one('right') }
}

/** A stored record from any input: junk, negatives, non-numbers and non-finite values are
 *  "never sized", never a throw. */
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
  for (const side of ['left', 'right'] as const) {
    const v = (obj as Record<string, unknown>)[side]
    if (typeof v === 'number' && Number.isFinite(v) && v > 0) out[side] = Math.round(v)
  }
  return out
}

export function loadRailWidths(): RailWidths {
  return parseRailWidths(surfaceGet(RAIL_KEY))
}

/** Persist the preference. A side back at its default is simply absent from the record. */
export function saveRailWidths(w: RailWidths): void {
  const out: Partial<Record<RailSide, number>> = {}
  if (w.left != null) out.left = w.left
  if (w.right != null) out.right = w.right
  surfaceSet(RAIL_KEY, JSON.stringify(out))
}
