// Where a DXpedition calendar entry sends the operator, and what to call that
// destination. Pure → node-testable.
//
// The backend (`open_dxped_page`) makes the SAME choice and is the authority: it
// re-validates the scheme before anything reaches a browser. This mirror exists
// so the UI can name the destination honestly in a tooltip — "opens 3y0l.com" vs
// "no site announced, opens their QRZ page" — instead of promising a webpage and
// silently delivering something else.
import type { CalendarEntry } from '../../types'

export type DxpedLinkKind = 'site' | 'qrz'

export interface DxpedLink {
  kind: DxpedLinkKind
  /** Exactly what will open, so the tooltip can say so. */
  url: string
  /** Short human label for the affordance. */
  label: string
}

/** The base call QRZ's db pages key on — a portable prefix/suffix is dropped
 * ("PJ4/K1ABC" → "K1ABC"), matching `qrz_url` in src-tauri. */
export function baseCall(call: string): string {
  return (call.split('/').reduce((a, b) => (b.length > a.length ? b : a), '').match(/[a-zA-Z0-9]/g) ?? [])
    .join('')
    .toUpperCase()
}

/** Only http/https ever opens. The site comes from third-party HTML, so a
 * `javascript:`/`file:` href must not even be OFFERED, let alone opened. */
function usableSite(url: string | null | undefined): string | null {
  const u = url?.trim()
  if (!u || /\s/.test(u)) return null
  return /^https?:\/\//i.test(u) ? u : null
}

/**
 * The page this operation opens. `null` only when there is no callsign to fall
 * back on, which is the one case the affordance should not be offered at all.
 */
export function dxpedLink(
  entry: Pick<CalendarEntry, 'call'> & { website?: string | null },
): DxpedLink | null {
  const site = usableSite(entry.website)
  if (site) return { kind: 'site', url: site, label: 'Website' }
  const base = baseCall(entry.call)
  if (!base) return null
  return { kind: 'qrz', url: `https://www.qrz.com/db/${base}`, label: 'QRZ' }
}

/** Tooltip text — always names the actual destination, and says plainly when it
 * is the fallback rather than the operation's own site. */
export function dxpedLinkTitle(link: DxpedLink): string {
  return link.kind === 'site'
    ? `Open the expedition's website — ${link.url}`
    : `No website announced — open their QRZ page instead (${link.url})`
}
