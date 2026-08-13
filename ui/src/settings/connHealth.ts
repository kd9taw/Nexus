/** Settings ▸ Connections — what a connector row actually MEANS.
 *
 * The panel used to paint its dot straight off `stored`, a keychain read. That answered
 * "does a secret exist", not "are my QSOs getting out", so a revoked ClubLog app-password
 * or a rotated QRZ Logbook key stayed green indefinitely — the secret had not stopped
 * existing, which was the only thing being checked.
 *
 * The derivation lives here rather than as inline ternaries in SettingsPanel.tsx for the
 * same reason `registry.ts` does: it is a judgement call with an order that matters, and a
 * judgement call belongs somewhere it can be tested. Pure data + pure helpers — no React,
 * no api, no storage.
 */
import type { CredStatus } from '../types'

/** The seven states a connector row can be in.
 *  - `none`    — no credential stored; nothing has been configured (grey)
 *  - `paused`  — a session kill-switch is holding every upload back (red)
 *  - `failing` — the last thing that happened was a failure (red)
 *  - `off`     — it uploads, but the operator turned auto-upload off (grey)
 *  - `working` — an upload got through, and nothing has failed since (green)
 *  - `lookup`  — lookup-only; it never uploads, so it cannot be behind (grey)
 *  - `idle`    — stored, enabled, and never once verified (amber) */
export type ConnState = 'none' | 'paused' | 'failing' | 'off' | 'working' | 'lookup' | 'idle'

/** Which state a row is in. The ORDER is the design:
 *
 *  1. No credential beats everything — there is nothing to be healthy about.
 *  2. A kill-switch beats a stale success: while it is set, nothing is being sent at all.
 *  3. A failure NEWER than the last success beats that success.
 *  4. Deliberately off beats "never verified" — an operator who turned it off is not
 *     looking at a problem.
 *  5. A success with nothing failing since is working.
 *  6. Lookup-only connectors never upload, so there is nothing to have gone wrong.
 *  7. Everything left is stored, on, and has never been exercised. ⚠️ THIS is the case the
 *     panel used to render GREEN, and it is the whole point of the change: honest
 *     degradation. "No news" is not good news; it is no news.
 */
export function connState(c: CredStatus): ConnState {
  if (!c.stored) return 'none'
  if (c.paused) return 'paused'
  if (c.lastFailureUnix != null && (c.lastSuccessUnix == null || c.lastFailureUnix > c.lastSuccessUnix))
    return 'failing'
  if (c.uploads && !c.enabled) return 'off'
  if (c.lastSuccessUnix != null) return 'working'
  if (!c.uploads) return 'lookup'
  return 'idle'
}

/** The `.conn-dot` / `.conn-state` modifier for a state. Four colours, not seven: the
 *  phrase beside the dot carries the distinction, the dot only has to say good/bad/unsure. */
export function dotClass(s: ConnState): 'on' | 'bad' | 'warn' | 'off' {
  switch (s) {
    case 'working':
      return 'on'
    case 'failing':
    case 'paused':
      return 'bad'
    case 'idle':
      return 'warn'
    default:
      return 'off'
  }
}

/** The words beside the dot. Plain, and each says what to DO where there is something. */
export function stateLabel(s: ConnState): string {
  switch (s) {
    case 'none':
      return 'no credential'
    case 'paused':
      return 'paused — auth failed, fix credentials'
    case 'failing':
      return 'failing'
    case 'off':
      return 'auto-upload off'
    case 'working':
      return 'working'
    case 'lookup':
      return 'lookup only'
    case 'idle':
      return 'stored — not verified yet'
  }
}

/** Compact "how long ago" for a row.
 *
 *  `neededFilters.ts`'s `ageLabel` caps at minutes and would print "10080 min ago" for a
 *  week, which is exactly the range this panel lives in — a connector can easily go a
 *  fortnight between uploads. So: minutes, then hours, then an actual date past ~48 h,
 *  because "312h ago" is not something anyone reads as a fortnight. */
export function whenLabel(unix: number | null | undefined, now = Date.now()): string {
  if (unix == null || unix <= 0) return ''
  const secs = Math.max(0, Math.floor(now / 1000 - unix))
  if (secs < 90) return 'just now'
  const m = Math.floor(secs / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 48) return `${h}h ago`
  return new Date(unix * 1000).toLocaleDateString()
}

/** The second line of a row: when it last worked, or when and why it last failed. Empty
 *  when there is nothing honest to say — an empty string renders as no line at all. */
export function whenText(c: CredStatus, s: ConnState, now = Date.now()): string {
  if (s === 'failing' || s === 'paused') {
    const when = whenLabel(c.lastFailureUnix, now)
    const why = c.lastFailureDetail?.trim()
    if (!when && !why) return ''
    // The service's own words. Sanitized upstream (never a raw path or secret).
    return `failed ${when}${why ? ` — ${why}` : ''}`.trim()
  }
  const when = whenLabel(c.lastSuccessUnix, now)
  return when ? `last upload ${when}` : ''
}
