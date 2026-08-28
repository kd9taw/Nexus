// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the callsign and its initials, the grid squares,
// the band and mode of every prior contact, the RST pair, the operator's own notes and comments,
// and the distance/bearing line (`units.ts` formats that at the display edge). The month
// abbreviations in `fmtDate` are date formatting, not catalog prose — the same ruling the
// DXpedition calendar carries — so they stay here with the rest of the date handling.
import type { LoggedQso } from '../types'
import type { CallHistory } from '../features/callHistory'
import { openQrzPage } from '../api'
import { withErrorToast } from '../toast'
import { gridToLatLon, stationLatLon, distanceLabelAt, bearingLabelAt } from '../grid'
import { t } from '../i18n'
import { useUnits } from '../units'

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
  /** Is this card in a BOUNDED rail that it shares with something else? Adds the
   *  `.cockpit-recall` placement class (cockpit-panes.css), which caps the card at a share
   *  of its column and lets it scroll inside itself. Only the Operate cockpit passes it:
   *  there the card shares `.cockpit-side` with the Stations roster and, unbounded, took
   *  the roster down to ~2 rows at 1024x768. The CW/Phone log panes pass nothing and are
   *  unchanged — their pane body is already the card's scroller and the operator asked for
   *  the full card there (2026-07-31). Default false. */
  bounded?: boolean
  /** Does the HOST offer a callbook Lookup button? The empty-QTH line is an instruction
   *  ("Tab or press Lookup for name / QTH") and it names a control that exists in the log
   *  strip and nowhere else. The FT cockpit resolves a call by itself and has no such
   *  button, so it passes false and the line is simply absent — a card that points at a
   *  control which is not on screen reads as a broken card. Default true: every host that
   *  says nothing is a log strip, unchanged. */
  hasLookup?: boolean
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
export function RecallPanel({ call, band, name, qth, grid, lat, lon, country, image, myGrid, hist, newEntity, newBandSlot, newModeSlot, hasLookup = true, bounded = false }: Props) {
  const units = useUnits()
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
  const geo = me && them ? `${distanceLabelAt(me, them, units)} · ${bearingLabelAt(me, them)}` : ''
  // Name whichever side is still a SQUARE rather than a point. A 4-character locator is
  // ±1° of longitude — a degree or so on a DX path, but up to ~29° on a station ~125 miles
  // away, and the reason a grid-derived heading disagrees with QRZ's. Saying so is what
  // keeps that error visible instead of silently wrong.
  const coarse = (g: string | null | undefined) => Boolean(g?.trim()) && g!.trim().length < 6
  const approx = [
    coarse(myGrid) ? t('recall.geo.approx.mine', { grid: myGrid!.trim().toUpperCase() }) : '',
    !them || (lat != null && lon != null) || !coarse(grid)
      ? ''
      : t('recall.geo.approx.theirs', { grid: grid!.trim().toUpperCase() }),
  ].filter(Boolean)
  // Whole sentences, never a clause glued onto a head: the "approximate" case is its own
  // message, and the word joining the two squares lives inside a message of its own — a
  // language that pairs them differently can only do that if the conjunction is translatable.
  const squares =
    approx.length === 2 ? t('recall.geo.approx.both', { mine: approx[0], theirs: approx[1] }) : approx[0]
  const geoTitle = approx.length ? t('recall.geo.title.approx', { squares }) : t('recall.geo.title')
  const confirmed = hist.confirmedCount > 0
  const needed = newEntity
    ? t('recall.need.entity')
    : newBandSlot
      ? t('recall.need.band')
      : newModeSlot
        ? t('recall.need.mode')
        : null
  const prior = [...hist.qsos].sort((a, b) => b.whenUnix - a.whenUnix)
  const lastNote = prior.find((q) => (q.notes ?? '').trim())?.notes?.trim()
  // The link needs only the CALLSIGN (operator, 2026-07-31: open the call's QRZ page from the
  // recall photo "to look at the page while you're working them"). It used to ride on CS
  // RESOLUTION — `nm || where || image` — and that was backwards, reported 2026-08-05 as "the
  // lookup button for qrz is not popping open a webpage": it withheld the link in exactly the
  // cases an operator reaches for the browser. No QRZ subscription (grid and state are
  // subscriber-only), no credentials configured, a lookup that failed, a call the callbook does
  // not know — an empty in-app lookup is a REASON to open the web page, not grounds to hide it.
  // Nothing here ever needed the lookup: qrz_url() builds and sanitizes the URL from the call
  // alone. The card still says "Tab or press Lookup for name / QTH" below, so an active link is
  // not mistaken for a resolution.

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
    <div className={`recall-card${bounded ? ' cockpit-recall' : ''}`}>
      <div className="recall-head">
        {/* Same open_qrz_page path as the roster/logbook ↗ buttons: the Rust command derives
            and sanitizes https://www.qrz.com/db/<base call>, so nothing URL-shaped is built
            here. Failures are TOASTED, not swallowed — there is no global unhandledrejection
            handler in this app, so a `.catch(() => {})` here (the old AprsStationCard-style
            fire-and-forget) turned a browser that refused to launch into "nothing happened",
            which is indistinguishable from a dead button. */}
        <button
          type="button"
          className="recall-avatar"
          title={t('recall.qrz.title', { call: cu })}
          aria-label={t('recall.qrz.title', { call: cu })}
          // preventDefault on mousedown (the park-picker guard, above in LogEntry): the
          // browser-default focus-on-mousedown must not yank the caret out of the log form
          // mid-entry — the opened browser window takes over; nothing in-app moves or scrolls.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() =>
            void withErrorToast(() => openQrzPage(cu), t('recall.qrz.error', { call: cu }))
          }
        >
          {avatarInner}
          <span className="recall-avatar-qrz" aria-hidden>
            ↗
          </span>
        </button>
        <div className="recall-id">
          <div className="recall-name-row">
            <span className="recall-name">{nm || cu}</span>
            {nm && <span className="recall-call mono">{cu}</span>}
          </div>
          <div className="recall-where">
            {/* Names the button the operator actually sees: the log strip's callbook button is
                "Lookup" (it answers from QRZ or HamQTH), not "QRZ". */}
            {where ||
              (hasLookup ? <span className="recall-where-empty">{t('recall.where.empty')}</span> : null)}
          </div>
          {geo && (
            <div className="recall-geo mono" title={geoTitle}>
              {geo}
            </div>
          )}
        </div>
        <div className="recall-badges">
          {hist.dupeThisBand && band && (
            <span className="recall-badge dupe" title={t('recall.dupe.title', { band })}>
              {t('recall.dupe.label', { band })}
            </span>
          )}
          {confirmed && (
            <span
              className="recall-badge ok"
              title={t('recall.confirmed.title', {
                confirmed: hist.confirmedCount,
                count: hist.count,
              })}
            >
              ✓
            </span>
          )}
          {needed && (
            <span className="recall-badge need" title={t('recall.need.title')}>
              ★ {needed}
            </span>
          )}
        </div>
      </div>

      {lastNote && (
        <div className="recall-note" title={t('recall.note.title')}>
          📝 {lastNote}
        </div>
      )}

      {prior.length > 0 && (
        <div className="recall-log">
          <div className="recall-log-head">
            {t('recall.log.head')} <span className="recall-log-count">{prior.length}</span>
          </div>
          {/* role=list: jsdom/AT reachability for rows scrolled under the em ceiling —
              carried over from the compact variant's list when compact was deleted. */}
          <div className="recall-log-list" role="list" aria-label={t('recall.log.aria', { call: cu })}>
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
