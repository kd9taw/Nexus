// THE STATION CARD — one floating card that shows everything Nexus knows about a call, opened from
// wherever a call is drawn: the Logbook's Call cell, the Spots board, the decode list, the rosters,
// the log strip and the map. What it shows, each block absent when there is nothing to say:
//   · who: the callbook's answer (name, QTH, state, county, country, grid, zones, IOTA, e-mail, web,
//     QSL route and the LoTW / eQSL / card flags) with the photo when the callbook gives one;
//   · where and when: the distance and bearing the station resolves for the call (the number the
//     Rotor box and the recall card show, `useCallBearings`), the long path, and the station's local
//     time from the callbook's UTC offset;
//   · history: the contacts already in the log (`callHistory`, the recall card's question) as a band ×
//     mode grid of worked and confirmed, and the last one;
//   · needs: the NEW ONE / NEW BAND / NEW MODE answer the Stats pane asks (`get_call_stats`) for the
//     band and mode on the radio now, and the need chips the Needed board holds for the call;
//   · the newest spot of the call on the Spots board.
//
// FAST, AND NOTHING NEW ON THE WIRE. Opening the card asks nothing that leaves the computer: every
// block is an answer the station already holds, and until the callbook has been asked the who-block
// shows what the newest logged contact recorded (name, QTH, grid). The callbook is asked only when the
// operator presses the card's own Look up (the log strip Lookup's command, `qrzLookup`). Every answer a
// lookup in this window brought back is kept for the session (`features/callbookAnswers`), so a call
// the log strip, the Logbook or the roster already looked up opens with that answer and its photo, and
// a call is asked at most once however often its card opens.
//
// ⛔ IT NEVER COVERS A TRANSMIT CONTROL. The card is placed by `placeCallCard`, which keeps it off
// every visible TX strip, TX dock, FT QSO strip and Stop button (`TX_AVOID`) and off the call it was
// opened from, and gives it a shorter, scrolling box where the room is short. With no free room at
// all it does not show. That is geometry, so jsdom cannot see it: `scripts/browser-probe --desktop
// callcard` opens the card beside the real TX strip in Chrome and asks `elementFromPoint` at Stop TX.
//
// Portaled to <body> like ui/Dialog and ui/Tooltip (the layout contract's one exception): the box is
// placed in real window pixels and its content re-applies `--ui-zoom`.
//
// Esc closes it. Esc is also the stop key on every operating screen (useEscStop.ts, capture phase), so
// the same press that closes the card halts TX there, exactly as with every dialog: this listener
// never prevents or stops the key, so nothing here can come between Esc and the stop.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every operator-visible
// sentence comes from the catalog. Data is not translated: the call, names and places the callbook
// gives, grid squares, zone numbers, IOTA references, bands, modes, frequencies, dates and times, and
// the product names (QRZ, LoTW, eQSL) that are tokens in every language (QRZ_PAGE below).
import './CallCard.css'
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent } from 'react'
import { createPortal } from 'react-dom'
import { getCallStats, openQrzPage, qrzLookup } from '../api'
import type { AppSnapshot, CallStats, NeedAlert, NeedTag, QrzLookup, SpotRow } from '../types'
import type { CallHistory } from '../features/callHistory'
import { modeKey } from '../features/callHistory'
import { useLogAnswer } from '../features/logSource'
import { callbookAnswer, subscribeCallbookAnswers } from '../features/callbookAnswers'
import { emptyAnswer } from '../features/logAnswers'
import { useCallBearings, type CallAim } from '../features/callBearing'
import { callCardHoverOn } from '../features/callCardHover'
import { NEED_CHIP } from '../features/needVisuals'
import { planReturn, giveBack } from '../focusReturn'
import { withErrorToast } from '../toast'
import { fmtDistanceKm, useUnits } from '../units'
import { t } from '../i18n'

// ---- where the card may never stand ----------------------------------------------------------

/** Every transmit surface the card keeps off: the TX strip under each cockpit's scope, the TX dock,
 *  FT's QSO strip (its Stop TX and sequencer), SSTV's pinned transmit bar, and any Stop button
 *  wherever it is drawn (the top bar's cluster, the log confirm). A hidden keep-alive cockpit's copies
 *  have no box and drop out. */
export const TX_AVOID = '.cockpit-txstrip, .cockpit-txdock, .cockpit-qso, .sstv-tx-bar, .op-controls, .op-btn.stop'

export interface Box {
  left: number
  top: number
  right: number
  bottom: number
}

/** Where the card goes: its window position and the tallest it may be there. */
export interface Placement {
  left: number
  top: number
  width: number
  maxHeight: number
}

const MARGIN = 8
const GAP = 4
/** Below this the card is not worth drawing: the call and one line. */
const MIN_HEIGHT = 56

const overlaps = (a: Box, b: Box) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top

/**
 * Place a card of `size` for the call at `anchor`, inside a `view` window, touching none of `avoid`
 * and not the anchor itself. Pure, so the rule is tested without a browser.
 *
 * It tries three columns — under/over the call, to its right, to its left — and in each finds the
 * free stretches of height between the boxes it must keep off. The card takes the stretch that holds
 * it whole and lies nearest the call; where none holds it whole, the tallest, with the card scrolling
 * inside. Null when no stretch is even `MIN_HEIGHT` tall (or the card, if smaller): then it does not show.
 */
export function placeCallCard(anchor: Box, size: { w: number; h: number }, view: { w: number; h: number }, avoid: readonly Box[]): Placement | null {
  const w = Math.min(size.w, view.w - 2 * MARGIN)
  const want = Math.min(size.h, view.h - 2 * MARGIN)
  if (w <= 0 || want <= 0) return null
  const clampX = (x: number) => Math.max(MARGIN, Math.min(x, view.w - MARGIN - w))
  const columns = [...new Set([clampX(anchor.left), clampX(anchor.right + GAP), clampX(anchor.left - GAP - w)])]
  const blocks = [...avoid, anchor]
  const wantTop = anchor.bottom + GAP
  let best: { p: Placement; whole: boolean; height: number; dist: number } | null = null
  for (const left of columns) {
    const inColumn = blocks
      .filter((b) => b.right > left && b.left < left + w)
      .map((b) => [b.top - GAP, b.bottom + GAP] as const)
      .sort((a, b) => a[0] - b[0])
    // The free stretches of this column, top to bottom.
    const free: [number, number][] = []
    let from = MARGIN
    for (const [top, bottom] of inColumn) {
      if (top > from) free.push([from, Math.min(top, view.h - MARGIN)])
      from = Math.max(from, bottom)
    }
    if (from < view.h - MARGIN) free.push([from, view.h - MARGIN])
    for (const [a, b] of free) {
      const height = Math.min(want, b - a)
      // A stretch too short for even the call and one line is no place for it (a card that small
      // whole is fine).
      if (height < Math.min(MIN_HEIGHT, want)) continue
      // Under the call where the stretch allows, else as near to it as the stretch reaches.
      const top = Math.max(a, Math.min(wantTop, b - height))
      const whole = height >= want
      const dist = Math.abs(top - wantTop) + Math.abs(left - anchor.left)
      const better =
        !best ||
        (whole && !best.whole) ||
        (whole === best.whole && (whole ? dist < best.dist : height > best.height || (height === best.height && dist < best.dist)))
      if (better) best = { p: { left, top, width: w, maxHeight: height }, whole, height, dist }
    }
  }
  if (!best) return null
  // Belt and braces: a placement that touches a block is a bug in the arithmetic above; refuse it.
  const r: Box = { left: best.p.left, top: best.p.top, right: best.p.left + w, bottom: best.p.top + best.p.maxHeight }
  return avoid.some((b) => overlaps(r, b)) ? null : best.p
}

/** The boxes of every visible transmit surface in this window. */
export function txBoxes(): Box[] {
  return [...document.querySelectorAll(TX_AVOID)]
    .map((el) => el.getBoundingClientRect())
    .filter((r) => r.width > 0 && r.height > 0)
    .map((r) => ({ left: r.left, top: r.top, right: r.right, bottom: r.bottom }))
}

// ---- which card is open ----------------------------------------------------------------------

interface Opened {
  call: string
  anchor: HTMLElement
  /** Opened from the keyboard: the card takes the keyboard and gives it back on close. */
  keyboard: boolean
  /** Opened by the hover delay: it closes again when the pointer leaves. */
  hover: boolean
}

let opened: Opened | null = null
/** Hosts mounted in this window. With none (a pop-out window, a test of one surface), a trigger does
 *  what its element did before the card existed. */
let hosts = 0
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}
const readOpened = () => opened

export function openCallCard(call: string, anchor: HTMLElement, keyboard = false, hover = false): void {
  const cu = call.trim().toUpperCase()
  if (!cu) return
  opened = { call: cu, anchor, keyboard, hover }
  emit()
}

export function closeCallCard(): void {
  if (!opened) return
  opened = null
  emit()
}

const HOVER_OPEN_MS = 500
const HOVER_CLOSE_MS = 300
let hoverTimer: ReturnType<typeof setTimeout> | undefined
const clearHover = () => {
  clearTimeout(hoverTimer)
  hoverTimer = undefined
}
/** A desktop pointer: a touch screen has no hover to rest. */
const finePointer = () => typeof window.matchMedia !== 'function' || window.matchMedia('(hover: hover) and (pointer: fine)').matches

/**
 * Props that make an element open the card for `call`: a click, or Enter / Space on it while it has
 * the keyboard (a button activates on both), and the hover delay when the operator turned it on.
 * Spread them on a `<button>`. `fallback` is what the element did before (opening the callbook page),
 * kept where no card is hosted. `stop` keeps the click from the row the element sits in, as the
 * callbook links in the Spots board and the rosters always have.
 */
export function callCardTrigger(call: string, fallback?: () => void, stop = false) {
  return {
    'aria-haspopup': 'dialog' as const,
    title: t('callCard.open.title', { call: call.trim().toUpperCase() }),
    onClick: (e: ReactMouseEvent<HTMLElement>) => {
      // `stop`: the element sits in a row whose own click does something else (tune, select).
      if (stop) e.stopPropagation()
      clearHover()
      if (hosts === 0) {
        fallback?.()
        return
      }
      const el = e.currentTarget
      // A second click on the call that opened it closes it.
      if (opened && opened.anchor === el && !opened.hover) closeCallCard()
      else openCallCard(call, el, e.detail === 0)
    },
    onMouseEnter: (e: ReactMouseEvent<HTMLElement>) => {
      if (hosts === 0 || !callCardHoverOn() || !finePointer()) return
      const el = e.currentTarget
      clearHover()
      if (opened?.anchor === el) return
      hoverTimer = setTimeout(() => openCallCard(call, el, false, true), HOVER_OPEN_MS)
    },
    onMouseLeave: (e: ReactMouseEvent<HTMLElement>) => {
      clearHover()
      const el = e.currentTarget
      if (opened?.hover && opened.anchor === el) hoverTimer = setTimeout(closeCallCard, HOVER_CLOSE_MS)
    },
  }
}

/** The callbook page, as every surface opened it before the card. */
export const qrzPageOf = (call: string) => () =>
  void withErrorToast(() => openQrzPage(call), t('callbook.qrzPage.failed', { call }))

// ---- the answers it shows --------------------------------------------------------------------

/** The mode the radio is working now, as the log names it: what a NEW MODE is new against. */
export function liveMode(snap: Pick<AppSnapshot, 'radio' | 'link'>): string {
  switch (snap.radio.operatingMode) {
    case 'cw':
      return 'CW'
    case 'phone':
      return snap.radio.sideband === 'FM' ? 'FM' : 'SSB'
    case 'rtty':
      return 'RTTY'
    case 'keyboard':
      // PSK and JTTY are one section: the mode on the air is the station's `keyboardMode`.
      return snap.radio.keyboardMode || 'PSK31'
    default:
      return snap.link.tier
  }
}

export interface CallCardData {
  call: string
  /** The callbook's answer, once the operator asked for it. */
  book: QrzLookup | null
  /** Asking the callbook now. */
  looking: boolean
  /** Ask the callbook (the operator's press of Look up); absent where it cannot be asked. */
  onLookup?: () => void
  hist: CallHistory | null
  stats: CallStats | null
  needs: readonly NeedTag[]
  spot: SpotRow | null
  aim: CallAim | undefined
  band: string
  mode: string
  /** Now, for the station's local time. */
  nowMs: number
}

const BAND_ORDER = ['2190m', '630m', '160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '1.25m', '70cm', '33cm', '23cm', '13cm']
const bandRank = (b: string) => {
  const i = BAND_ORDER.indexOf(b.toLowerCase())
  return i < 0 ? BAND_ORDER.length : i
}

/** The band × mode grid of the log's contacts with the call: worked, or confirmed. */
export function historyGrid(hist: CallHistory): { bands: string[]; modes: string[]; cell: (band: string, mode: string) => 'confirmed' | 'worked' | null } {
  const marks = new Map<string, 'confirmed' | 'worked'>()
  const bands = new Set<string>()
  const modes = new Set<string>()
  for (const q of hist.qsos) {
    const b = (q.band ?? '').trim().toLowerCase()
    const m = modeKey(q.mode)
    if (!b || !m) continue
    bands.add(b)
    modes.add(m)
    const k = `${b} ${m}`
    if (q.confirmed) marks.set(k, 'confirmed')
    else if (!marks.has(k)) marks.set(k, 'worked')
  }
  return {
    bands: [...bands].sort((a, b) => bandRank(a) - bandRank(b)),
    modes: [...modes].sort(),
    cell: (b, m) => marks.get(`${b} ${m}`) ?? null,
  }
}

const pad2 = (n: number) => String(n).padStart(2, '0')
const utcDate = (unix: number) => {
  const d = new Date(unix * 1000)
  return `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())}`
}
/** The wall clock at a UTC offset in hours ("14:05"), and the offset as written ("UTC−5", "UTC+5:30"). */
export function localTime(nowMs: number, offsetHours: number): { time: string; offset: string } {
  const d = new Date(nowMs + offsetHours * 3_600_000)
  const sign = offsetHours < 0 ? '−' : '+'
  const abs = Math.abs(offsetHours)
  const mins = Math.round((abs % 1) * 60)
  return {
    time: `${pad2(d.getUTCHours())}:${pad2(d.getUTCMinutes())}`,
    offset: `UTC${sign}${Math.floor(abs)}${mins ? `:${pad2(mins)}` : ''}`,
  }
}

/** The callbook page button's label: the service's name, a token in every language. */
const QRZ_PAGE = 'QRZ ↗'

/** The callbook photo, or the call's initials until it draws. A callbook answer without one says why
 *  on hover: QRZ gives the photo only to an XML subscription, so a free account's answer never carries
 *  it. The station card's and the Logbook contact card's. */
export function CallbookPhoto({ call, book }: { call: string; book: QrzLookup | null }) {
  return (
    <span className="callcard-avatar" aria-hidden title={book && !book.image ? t('callCard.photo.none') : undefined}>
      <span className="callcard-initials">{call.slice(0, 2)}</span>
      {book?.image && (
        <img
          key={book.image}
          className="callcard-photo"
          src={book.image}
          alt=""
          loading="lazy"
          onError={(e) => {
            e.currentTarget.style.display = 'none'
            e.currentTarget.parentElement?.setAttribute('title', t('callCard.photo.failed'))
          }}
        />
      )}
    </span>
  )
}

/** The card's face, from answers alone: what the tests render from a fixture. */
export function CallCardView({ data, onClose }: { data: CallCardData; onClose: () => void }) {
  const units = useUnits()
  const { call, book, hist, stats } = data
  const last = hist && hist.count > 0 ? [...hist.qsos].sort((a, b) => b.whenUnix - a.whenUnix)[0] : null
  // Until the callbook is asked, who and where are what the newest contact recorded.
  const name = (book ? book.nickname || book.name || '' : (last?.name ?? '')).trim()
  const town = (book ? [book.qth, book.state] : [last?.qth, last?.state]).map((s) => s?.trim()).filter(Boolean).join(', ')
  const where = [town, book?.county?.trim(), (book ? book.country : last?.country)?.trim()].filter(Boolean).join(' · ')
  const tokens = [
    (book ? book.grid : last?.grid)?.trim(),
    book?.cqZone != null ? `CQ ${book.cqZone}` : '',
    book?.ituZone != null ? `ITU ${book.ituZone}` : '',
    book?.iota?.trim() ? `IOTA ${book.iota.trim()}` : '',
  ].filter(Boolean)
  const bearing = data.aim && 'bearing' in data.aim ? data.aim.bearing : null
  const brg = bearing ? Math.round(bearing.pointed.bearing) % 360 : null
  const lt = book?.utcOffset != null ? localTime(data.nowMs, book.utcOffset) : null
  const qsl = [book?.lotw ? 'LoTW' : '', book?.eqsl ? 'eQSL' : '', book?.paperQsl ? t('callCard.qsl.card') : ''].filter(Boolean)
  const grid = hist && hist.count > 0 ? historyGrid(hist) : null
  const flags = stats ? [stats.newOne && t('callStats.flag.newOne'), stats.newBand && t('callStats.flag.newBand'), stats.newMode && t('callStats.flag.newMode')].filter(Boolean) : []
  const chips = data.needs.filter((n) => NEED_CHIP[n])
  return (
    <>
      {/* The text column takes the whole width beside the photo: Look up and the callbook page sit on
          their own line under it, and only × shares its row. With all three in this row, the name and
          the address were squeezed to the callsign's width (reported 2026-10-26). */}
      <div className="callcard-head">
        <CallbookPhoto call={call} book={book} />
        <div className="callcard-id">
          <div className="callcard-title">
            <span className="callcard-call mono">{call}</span>
            {name && <span className="callcard-name">{name}</span>}
          </div>
          {where && <div className="callcard-where">{where}</div>}
          {tokens.length > 0 && <div className="callcard-tokens mono">{tokens.join(' · ')}</div>}
          <div className="callcard-actions">
            {!book && data.onLookup && (
              <button type="button" className="callcard-lookup" onClick={data.onLookup} disabled={data.looking} title={t('logEntry.lookup.title')}>
                {data.looking ? t('callCard.looking') : t('logEntry.lookup.label')}
              </button>
            )}
            <button type="button" className="callcard-qrz" onClick={qrzPageOf(call)} title={t('callbook.qrzPage.title', { call })}>
              {QRZ_PAGE}
            </button>
          </div>
        </div>
        <button type="button" className="callcard-close" onClick={onClose} aria-label={t('callCard.close')} title={t('callCard.close')}>
          ×
        </button>
      </div>

      {(bearing || lt) && (
        <div className="callcard-row">
          {bearing && (
            <span className="mono" title={t('callCard.path.title')}>
              {t('callCard.path.short', { dist: fmtDistanceKm(bearing.km, units), deg: brg ?? 0 })}
              {' · '}
              {t('callCard.path.long', {
                dist: fmtDistanceKm(Math.max(0, 40_075 - bearing.km), units),
                deg: ((brg ?? 0) + 180) % 360,
              })}
            </span>
          )}
          {lt && <span className="mono">{t('callCard.localTime', { time: lt.time, offset: lt.offset })}</span>}
        </div>
      )}

      {(book?.email || book?.web || book?.qslVia || qsl.length > 0) && (
        <div className="callcard-row callcard-contact">
          {book?.email && <span className="callcard-dim">{t('callCard.email', { email: book.email })}</span>}
          {book?.web && <span className="callcard-dim">{t('callCard.web', { web: book.web })}</span>}
          {book?.qslVia && <span>{t('callCard.qslVia', { via: book.qslVia })}</span>}
          {qsl.length > 0 && <span>{t('callCard.qsl', { ways: qsl.join(' · ') })}</span>}
        </div>
      )}

      <div className="callcard-section">
        <div className="callcard-section-head">{t('callCard.needs.head', { band: data.band, mode: data.mode })}</div>
        {flags.length > 0 || chips.length > 0 ? (
          <div className="callcard-needs">
            {flags.map((f) => (
              <span key={String(f)} className="callcard-flag">
                {f}
              </span>
            ))}
            {chips.map((n) => (
              <span key={n} className={`need-chip need-${NEED_CHIP[n].cls}`} title={NEED_CHIP[n].title}>
                {NEED_CHIP[n].short}
              </span>
            ))}
          </div>
        ) : (
          <div className="callcard-dim">{stats ? t('callCard.needs.none') : t('callCard.needs.asking')}</div>
        )}
      </div>

      <div className="callcard-section">
        <div className="callcard-section-head">
          {hist && hist.count > 0 ? t('callCard.history.count', { count: hist.count }) : t('callCard.history.none')}
        </div>
        {last && (
          <div className="callcard-dim mono">
            {t('callCard.history.last', { date: utcDate(last.whenUnix), band: last.band ?? '', mode: last.mode ?? '' })}
          </div>
        )}
        {grid && grid.bands.length > 0 && (
          <table className="cstats-grid callcard-grid">
            <caption className="sr-only">{t('callCard.history.caption', { call })}</caption>
            <thead>
              <tr>
                <td />
                {grid.bands.map((b) => (
                  <th key={b} scope="col">
                    {b.replace(/m$/, '')}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {grid.modes.map((m) => (
                <tr key={m}>
                  <th scope="row" className="cstats-mode">
                    {m}
                  </th>
                  {grid.bands.map((b) => {
                    const c = grid.cell(b, m)
                    const words = c === 'confirmed' ? t('callStats.slot.confirmed') : c === 'worked' ? t('callStats.slot.worked') : t('callStats.slot.unworked')
                    return (
                      <td key={b} className="cstats-slot" data-slot={c ?? 'unworked'} title={`${b} ${m}: ${words}`}>
                        <span className="cstats-mark" aria-hidden="true" />
                        <span className="sr-only">{words}</span>
                      </td>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {data.spot && (
        <div className="callcard-section callcard-dim mono">
          {t('callCard.spot', {
            freq: data.spot.freqMhz.toFixed(3),
            spotter: data.spot.spotter,
            min: Math.max(0, Math.round(data.spot.ageSecs / 60)),
          })}
        </div>
      )}
    </>
  )
}

/** The live answers for one open card. */
function useCardData(call: string, snap: AppSnapshot, spots: readonly SpotRow[], needAlertsByCall: ReadonlyMap<string, NeedAlert[]>): CallCardData {
  const band = snap.radio.band
  const mode = liveMode(snap)
  // Nothing asked of the callbook on open: only the operator's Look up asks it. Any lookup of the call
  // in this window, the card's own or another surface's, lands here (`qrzLookup` keeps the answer).
  const book = useSyncExternalStore(subscribeCallbookAnswers, () => callbookAnswer(call))
  const [looking, setLooking] = useState(false)
  const live = useRef(true)
  useEffect(() => {
    live.current = true
    return () => {
      live.current = false
    }
  }, [])
  const onLookup = () => {
    setLooking(true)
    void withErrorToast(() => qrzLookup(call), t('callbook.lookupFailed')).then(() => {
      if (live.current) setLooking(false)
    })
  }
  const [stats, setStats] = useState<{ key: string; s: CallStats | null } | null>(null)
  const statsKey = `${call} ${band} ${mode} ${snap.logTick ?? 0}`
  useEffect(() => {
    let live = true
    getCallStats(call, band, mode).then(
      (s) => live && setStats({ key: statsKey, s }),
      () => live && setStats({ key: statsKey, s: null }),
    )
    return () => {
      live = false
    }
  }, [call, band, mode, statsKey])
  const q = { kind: 'callHistory', call, band, mode, matchMode: snap.b4MatchMode ?? false } as const
  const hist = useLogAnswer(q, snap.logTick) ?? emptyAnswer(q)
  const aim = useCallBearings([call]).get(call)
  const needs = [...new Set((needAlertsByCall.get(call) ?? []).filter((a) => a.band === band).flatMap((a) => a.tags))]
  const spot = spots.filter((s) => s.call.toUpperCase() === call).sort((a, b) => a.ageSecs - b.ageSecs)[0] ?? null
  return {
    call,
    book,
    looking,
    onLookup,
    hist,
    stats: stats?.key === statsKey ? stats.s : null,
    needs,
    spot,
    aim,
    band,
    mode,
    nowMs: Date.now(),
  }
}

const FOCUSABLE = 'button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])'

function CallCardPopover({ open, snap, spots, needAlertsByCall }: { open: Opened; snap: AppSnapshot; spots: readonly SpotRow[]; needAlertsByCall: ReadonlyMap<string, NeedAlert[]> }) {
  const data = useCardData(open.call, snap, spots, needAlertsByCall)
  const box = useRef<HTMLDivElement>(null)
  const inner = useRef<HTMLDivElement>(null)
  const [place, setPlace] = useState<Placement | null | undefined>(undefined)
  /** The keyboard is inside the card: on close it goes back to the call. */
  const focusInside = useRef(false)

  // Place it, and again whenever the window, a scroller or the card's own content moves.
  useLayoutEffect(() => {
    const replace = () => {
      if (!open.anchor.isConnected) return closeCallCard()
      const a = open.anchor.getBoundingClientRect()
      const c = inner.current?.getBoundingClientRect()
      if (!c || !box.current) return
      // The box is border-box with a border round the body: the size it needs is the body's plus that
      // border. Placed at the body's size alone, it came out 2 px short each way and scrolled both ways.
      const b = getComputedStyle(box.current)
      const bw = (parseFloat(b.borderLeftWidth) || 0) + (parseFloat(b.borderRightWidth) || 0)
      const bh = (parseFloat(b.borderTopWidth) || 0) + (parseFloat(b.borderBottomWidth) || 0)
      const p = placeCallCard(
        { left: a.left, top: a.top, right: a.right, bottom: a.bottom },
        { w: c.width + bw, h: c.height + bh },
        { w: window.innerWidth, h: window.innerHeight },
        txBoxes(),
      )
      setPlace((old) => (old && p && old.left === p.left && old.top === p.top && old.maxHeight === p.maxHeight ? old : p))
    }
    replace()
    window.addEventListener('resize', replace)
    window.addEventListener('scroll', replace, true)
    const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(replace) : null
    if (inner.current) ro?.observe(inner.current)
    return () => {
      window.removeEventListener('resize', replace)
      window.removeEventListener('scroll', replace, true)
      ro?.disconnect()
    }
  }, [open.anchor])

  // The keyboard comes in with a card opened from it, and goes back to the call on close.
  const placed = !!place
  const tookKeyboard = useRef(false)
  useEffect(() => {
    // Only once it is placed: a hidden box cannot take the keyboard.
    if (open.keyboard && placed && !tookKeyboard.current) {
      tookKeyboard.current = true
      box.current?.focus({ preventScroll: true })
    }
  }, [open.keyboard, placed])
  useEffect(() => {
    const anchor = open.anchor
    return () => {
      if (focusInside.current && anchor.isConnected) giveBack(planReturn(anchor, false))
    }
  }, [open])

  // Esc closes it (never prevented: it is the stop key too); a press outside it closes it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeCallCard()
    }
    const onDown = (e: PointerEvent) => {
      const at = e.target as Node | null
      if (at && (box.current?.contains(at) || open.anchor.contains(at))) return
      closeCallCard()
    }
    window.addEventListener('keydown', onKey)
    document.addEventListener('pointerdown', onDown, true)
    return () => {
      window.removeEventListener('keydown', onKey)
      document.removeEventListener('pointerdown', onDown, true)
    }
  }, [open.anchor])

  // Opened from the keyboard, Tab stays inside the card until it closes.
  const trap = (e: ReactKeyboardEvent) => {
    if (e.key !== 'Tab' || !open.keyboard || !box.current) return
    const els = [...box.current.querySelectorAll<HTMLElement>(FOCUSABLE)]
    if (els.length === 0) return
    const first = els[0]
    const last = els[els.length - 1]
    const at = document.activeElement
    if (e.shiftKey && (at === first || at === box.current)) {
      e.preventDefault()
      last.focus({ preventScroll: true })
    } else if (!e.shiftKey && at === last) {
      e.preventDefault()
      first.focus({ preventScroll: true })
    }
  }

  // Hidden until placed, and hidden where there is no room that keeps it off the transmit controls.
  const style: CSSProperties = place
    ? { left: place.left, top: place.top, maxWidth: place.width, maxHeight: place.maxHeight }
    : { left: 0, top: 0, visibility: 'hidden' }
  return createPortal(
    <div
      ref={box}
      className="callcard"
      role="dialog"
      aria-modal="false"
      aria-label={t('callCard.aria', { call: open.call })}
      data-placed={place ? 'yes' : 'no'}
      tabIndex={-1}
      style={style}
      onKeyDown={trap}
      onFocus={() => {
        focusInside.current = true
      }}
      onBlur={(e) => {
        if (!box.current?.contains(e.relatedTarget as Node | null)) focusInside.current = false
      }}
      onMouseEnter={() => open.hover && clearHover()}
      onMouseLeave={() => {
        if (open.hover) hoverTimer = setTimeout(closeCallCard, HOVER_CLOSE_MS)
      }}
    >
      <div ref={inner} className="callcard-body" style={{ zoom: 'var(--ui-zoom, 1)' }}>
        <CallCardView data={data} onClose={closeCallCard} />
      </div>
    </div>,
    document.body,
  )
}

/**
 * The window's one station card. Mount it once (App); every trigger in the window opens it.
 * `needAlertsByCall` is the Needed board's grouping (`alertsByCall`), `spots` the Spots board's rows.
 */
export function CallCardHost({ snap, spots, needAlertsByCall }: { snap: AppSnapshot; spots: readonly SpotRow[]; needAlertsByCall: ReadonlyMap<string, NeedAlert[]> }) {
  useEffect(() => {
    hosts++
    return () => {
      hosts--
      closeCallCard()
    }
  }, [])
  const open = useSyncExternalStore(subscribe, readOpened)
  if (!open) return null
  return <CallCardPopover key={`${open.call} ${open.keyboard}`} open={open} snap={snap} spots={spots} needAlertsByCall={needAlertsByCall} />
}
