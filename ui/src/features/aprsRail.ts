// APRS'S BODY LAYOUT (layout L7) — the pure half of the station rail's width divider and of the
// map-side choice. components/AprsCockpit.tsx renders the body; components/AprsRailSeam.tsx
// measures the rail and paints its width.
//
// THE RAIL WIDTH IS CLAMPED BY THE LAYOUT ITSELF, the grid cockpits' log column pattern
// (features/paneColumns): the body carries `min(<px>px, max(50%, 420px))` and the template floors
// the rail's track at 260 px, so what renders is the operator's width clamped to the body on load,
// on every resize and on every zoom change, before any script runs — and the stored preference is
// never rewritten, so a bigger window gets it back.
//
// WHY THE CAP IS max(50 %, 420 px) AND NOT 50 %. The stock rail is 420 px wide at every width
// (the template's fixed maximum is paid out before the map's 1fr sees any space), and on a body
// under 840 px that is more than half of it. A plain 50 % cap would leave where the rail stands
// outside the divider's own range there, and its first step would jump the rail to half the body.
// With the stock width as the cap's floor the range always holds the rail's position, and no
// dragged width gives the map less room than the stock layout already does.

/** The narrowest the rail may be dragged, CSS px: the template's own `minmax(260px, …)`. */
export const APRS_RAIL_MIN = 260
/** The rail's width until the operator sets one, CSS px: the template's fallback. */
export const APRS_RAIL_STOCK = 420
/** The widest a dragged rail may be, as a share of the body (never less than the stock width). */
export const APRS_RAIL_MAX_SHARE = 0.5

/** `--aprs-rail-w` for a width the operator set: capped in the sheet (see the header). */
export function aprsRailValue(px: number): string {
  return `min(${Math.round(px)}px, max(${APRS_RAIL_MAX_SHARE * 100}%, ${APRS_RAIL_STOCK}px))`
}

/** The range the divider moves through, CSS px, for a body `contentW` CSS px wide: the floor up to
 *  half the body, or the stock width where that is more. */
export function aprsRailRange(contentW: number): { min: number; max: number } {
  return { min: APRS_RAIL_MIN, max: Math.max(APRS_RAIL_MIN, APRS_RAIL_STOCK, APRS_RAIL_MAX_SHARE * contentW) }
}

/** A stored width, or null for "never set" (and for anything that is not a width). */
export function parseAprsRail(raw: string | null): number | null {
  const v = Number(raw ?? '')
  return raw != null && raw.trim() !== '' && Number.isFinite(v) && v > 0 ? v : null
}
