import type { LoggedQso } from '../types'
import type { CallHistory } from '../features/callHistory'
import { openQrzPage } from '../api'
import { gridToLatLon, stationLatLon, distanceLabelAt, bearingLabelAt } from '../grid'

interface Props {
  call: string
  /** Current operating band, for the same-band dupe flag. */
  band?: string
  name?: string | null
  qth?: string | null
  grid?: string | null
  /** The station's exact callbook coordinates, when the lookup vouched for a real
   *  position. Preferred over `grid` for the distance/bearing line — a locator is a
   *  box, and its center is not where the station is. Absent ⇒ fall back to `grid`. */
  lat?: number | null
  lon?: number | null
  country?: string | null
  /** Callbook profile photo URL (QRZ/HamQTH). When present, replaces the initials avatar. */
  image?: string | null
  /** Operator's own Maidenhead grid, for the distance/bearing line. */
  myGrid?: string
  hist: CallHistory
  newEntity?: boolean
  newBandSlot?: boolean
  newModeSlot?: boolean
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/** Compact UTC date "14 Mar 26" for the prior-contact rows. */
function fmtDate(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCDate()).padStart(2, '0')} ${MONTHS[d.getUTCMonth()]} ${String(d.getUTCFullYear()).slice(2)}`
}

/** "59/59" sent/received, or "" when neither is present. */
function rstPair(q: LoggedQso): string {
  const s = (q.rstSent ?? '').trim()
  const r = (q.rstRcvd ?? '').trim()
  return s || r ? `${s || '—'}/${r || '—'}` : ''
}

/** Initials avatar from a callsign (e.g. "W1ABC" → "W1"). */
function initials(call: string): string {
  return call.trim().toUpperCase().slice(0, 2) || '?'
}

/**
 * The mid-rag-chew recall panel — a prominent "who is this + our history" card so the operator
 * can greet by name/QTH and pick the conversation back up. The identity header is surfaced big
 * (name · city · grid) for a quick glance; below it, a real list of prior contacts (date · band ·
 * mode · report · comment) gives durable relationship context, not just the last-QSO summary. Looks
 * complete with NO photo (initials avatar) — a QRZ photo is a later add-on, not a dependency.
 *
 * HISTORY OF THE `compact` VARIANT (0.18.0 → 2026-07-31, now deleted). The pre-overhaul cockpits
 * had no interposed scroller, so this card's height squeezed the operating panes directly the
 * moment 3 characters of a call were typed; 0.18.0 cut it to one line there, dropping the photo,
 * QTH, distance/bearing and note. The pane grid removed the mechanism: the cockpit log pane's
 * .pane-body is `flex:1; min-height:0; overflow:auto`, so a tall card scrolls inside the log
 * column and structurally cannot crush the cockpit — the operator asked for the full card back
 * (2026-07-31) and compact lost its last caller.
 *
 * ⚠️ Two lessons that must survive that deletion:
 *   - The previous-contacts list is a REAL list, never a "worked 3×" count (operator regression
 *     report, 2026-07-26): the question being asked on air is WHEN, on what BAND, and what did
 *     we exchange.
 *   - The list stays a BOUNDED internal scroller (.recall-log-list, fixed em ceiling): the pane
 *     body is the card's real scroller, and a nested full-length list fights it.
 */
export function RecallPanel({ call, band, name, qth, grid, lat, lon, country, image, myGrid, hist, newEntity, newBandSlot, newModeSlot }: Props) {
  const c = call.trim()
  if (c.length < 3) return null
  const cu = c.toUpperCase()
  const nm = name?.trim()
  const place = [qth?.trim(), grid?.trim() ? `(${grid.trim()})` : ''].filter(Boolean).join(' ')
  const ctry = country?.trim()
  const where = [place, ctry].filter(Boolean).join(' · ')
  // Distance + true bearing from the operator's QTH, computed from the BEST position each
  // side has: the callbook's exact coordinates when the lookup returned real ones (the very
  // input QRZ derives ITS figures from), else the center of a reported grid square.
  const me = myGrid ? gridToLatLon(myGrid) : null
  const them = stationLatLon(lat != null && lon != null ? { lat, lon } : null, grid)
  const geo = me && them ? `${distanceLabelAt(me, them)} · ${bearingLabelAt(me, them)}` : ''
  // Name whichever side is still a SQUARE rather than a point. A 4-character locator is
  // ±1° of longitude — a degree or so on a DX path, but up to ~29° on a station ~125 miles
  // away, and the reason a grid-derived heading disagrees with QRZ's. Saying so is what
  // keeps that error visible instead of silently wrong.
  const coarse = (g: string | null | undefined) => Boolean(g?.trim()) && g!.trim().length < 6
  const approx = [
    coarse(myGrid) ? `your ${myGrid!.trim().toUpperCase()} square (set a 6-character grid in Settings to sharpen it)` : '',
    !them || (lat != null && lon != null) || !coarse(grid) ? '' : `their ${grid!.trim().toUpperCase()} square`,
  ].filter(Boolean)
  const geoTitle =
    'Great-circle distance · true bearing from your QTH' +
    (approx.length ? ` — approximate: computed from the center of ${approx.join(' and ')}` : '')
  const confirmed = hist.confirmedCount > 0
  const needed = newEntity ? 'New DXCC!' : newBandSlot ? 'New band-slot' : newModeSlot ? 'New mode-slot' : null
  const prior = [...hist.qsos].sort((a, b) => b.whenUnix - a.whenUnix)
  const lastNote = prior.find((q) => (q.notes ?? '').trim())?.notes?.trim()
  // The link rides on CS resolution (operator, 2026-07-31: open the call's QRZ page from the
  // recall photo "to look at the page while you're working them"): once ANY identity content is
  // in — name, place line, or photo — the avatar is a live QRZ button, photo or initials alike.
  // Before that the card is still the "Tab or press Lookup" prompt, so the circle stays inert.
  const resolved = Boolean(nm || where || image)

  const avatarInner = (
    <>
      <span className="recall-avatar-initials" aria-hidden>
        {initials(cu)}
      </span>
      {image && (
        // CSP is null (tauri.conf.json) so the remote callbook image loads directly. Keyed by
        // URL so a new call's photo starts fresh; on a broken/hotlink-blocked URL it hides
        // itself, revealing the initials underneath.
        <img
          key={image}
          className="recall-avatar-img"
          src={image}
          alt=""
          loading="lazy"
          onError={(e) => {
            e.currentTarget.style.display = 'none'
          }}
        />
      )}
    </>
  )

  return (
    <div className="recall-card">
      <div className="recall-head">
        {resolved ? (
          // Same open_qrz_page path as the roster/logbook ↗ buttons: the Rust command derives
          // and sanitizes https://www.qrz.com/db/<base call>, so nothing URL-shaped is built
          // here. Errors are swallowed (fire-and-forget browser launch, AprsStationCard-style).
          <button
            type="button"
            className="recall-avatar"
            title={`Open ${cu} on QRZ (browser)`}
            aria-label={`Open ${cu} on QRZ (browser)`}
            // preventDefault on mousedown (the park-picker guard, above in LogEntry): the
            // browser-default focus-on-mousedown must not yank the caret out of the log form
            // mid-entry — the opened browser window takes over; nothing in-app moves or scrolls.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => void openQrzPage(cu).catch(() => {})}
          >
            {avatarInner}
            <span className="recall-avatar-qrz" aria-hidden>
              ↗
            </span>
          </button>
        ) : (
          <div className="recall-avatar" aria-hidden>
            {avatarInner}
          </div>
        )}
        <div className="recall-id">
          <div className="recall-name-row">
            <span className="recall-name">{nm || cu}</span>
            {nm && <span className="recall-call mono">{cu}</span>}
          </div>
          <div className="recall-where">
            {/* Names the button the operator actually sees: the log strip's callbook button is
                "Lookup" (it answers from QRZ or HamQTH), not "QRZ". */}
            {where || <span className="recall-where-empty">Tab or press Lookup for name / QTH</span>}
          </div>
          {geo && (
            <div className="recall-geo mono" title={geoTitle}>
              {geo}
            </div>
          )}
        </div>
        <div className="recall-badges">
          {hist.dupeThisBand && band && (
            <span className="recall-badge dupe" title={`Already worked on ${band} — logging now would be a dupe`}>
              Dupe {band}
            </span>
          )}
          {confirmed && (
            <span className="recall-badge ok" title={`${hist.confirmedCount} of ${hist.count} prior QSOs confirmed`}>
              ✓
            </span>
          )}
          {needed && (
            <span className="recall-badge need" title="Worth working — a new one for your log">
              ★ {needed}
            </span>
          )}
        </div>
      </div>

      {lastNote && (
        <div className="recall-note" title="Your most recent note on this station">
          📝 {lastNote}
        </div>
      )}

      {prior.length > 0 && (
        <div className="recall-log">
          <div className="recall-log-head">
            Previous contacts <span className="recall-log-count">{prior.length}</span>
          </div>
          {/* role=list: jsdom/AT reachability for rows scrolled under the em ceiling —
              carried over from the compact variant's list when compact was deleted. */}
          <div className="recall-log-list" role="list" aria-label={`Previous contacts with ${cu}`}>
            {prior.map((q, i) => {
              // Comment only — the private note is surfaced once, in the 📝 line above (showing
              // it here too would duplicate the newest QSO's note).
              const cmt = (q.comment ?? '').trim()
              return (
                <div className="recall-log-row" role="listitem" key={`${q.whenUnix}-${i}`}>
                  <span className="recall-log-date mono">{fmtDate(q.whenUnix)}</span>
                  <span className="recall-log-bm">{[q.band, q.mode].filter(Boolean).join(' ')}</span>
                  <span className="recall-log-rst mono">{rstPair(q)}</span>
                  <span className="recall-log-cmt" title={cmt}>
                    {cmt}
                  </span>
                </div>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
