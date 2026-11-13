// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT: the band and mode names
// in the sub-heading and the `—` placeholders, which are data and a glyph, not prose.

import { useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from 'react'
import type { LoggedQso } from '../types'
import { t } from '../i18n'
import { FtStopControl } from './FtStopControl'
import { upperTyped } from '../typedInput'
import { IotaPrefillField, iotaInvalid, iotaToLog, useIotaIslandPick, useIotaPrefill, type IotaOfferData } from './Iota'
import { withIotaIsland } from '../features/iotaGroups'

/** WSJT-X's Log QSO dialog's propagation modes (`widgets/logqso.cpp`, `prop_modes`): the ADIF
 *  `PROP_MODE` enumeration values, in its order. SAT is not here: it is offered only on a contact
 *  the satellite stamp already marked, which carries the satellite's name — TQSL refuses a SAT
 *  without one. */
const PROP_MODES = [
  'AS', 'AUE', 'AUR', 'BS', 'ECH', 'EME', 'ES', 'F2', 'FAI', 'INTERNET', 'ION', 'IRL', 'MS', 'RPT',
  'RS', 'TEP', 'TR',
] as const

/** A propagation mode's name, from the catalog: one literal key per mode, so every one is
 *  checked. */
function propModeName(code: string): string {
  switch (code) {
    case 'AS':
      return t('logPrompt.propMode.aircraftScatter')
    case 'AUE':
      return t('logPrompt.propMode.auroraE')
    case 'AUR':
      return t('logPrompt.propMode.aurora')
    case 'BS':
      return t('logPrompt.propMode.backScatter')
    case 'ECH':
      return t('logPrompt.propMode.echolink')
    case 'EME':
      return t('logPrompt.propMode.earthMoonEarth')
    case 'ES':
      return t('logPrompt.propMode.sporadicE')
    case 'F2':
      return t('logPrompt.propMode.f2Reflection')
    case 'FAI':
      return t('logPrompt.propMode.fieldAligned')
    case 'INTERNET':
      return t('logPrompt.propMode.internet')
    case 'ION':
      return t('logPrompt.propMode.ionoscatter')
    case 'IRL':
      return t('logPrompt.propMode.irlp')
    case 'MS':
      return t('logPrompt.propMode.meteorScatter')
    case 'RPT':
      return t('logPrompt.propMode.repeater')
    case 'RS':
      return t('logPrompt.propMode.rainScatter')
    case 'SAT':
      return t('logPrompt.propMode.satellite')
    case 'TEP':
      return t('logPrompt.propMode.transEquatorial')
    case 'TR':
      return t('logPrompt.propMode.tropoDucting')
    default:
      return code
  }
}

/** The room the card keeps from a stop control, and from the window's edge (CSS px). */
const CLEAR = 8

/** A box in the window's own (visual) pixels. */
interface Rect {
  left: number
  top: number
  right: number
  bottom: number
}

/**
 * THE CARD STANDS CLEAR OF EVERY STOP CONTROL (2026-11-13). Centred and capped to the window's height, at 1366×768
 * with Windows or Nexus at 175 % the card stood over each cockpit's Stop TX (and RTTY's, PSK's, JTTY's and SSTV's own
 * stops). The stop layer (StopLayer) draws a stop's mirror over whatever covers it, so the mirror stood over the
 * card's own boxes, and a click meant for one of them was a Stop TX (measured in Chrome on every cockpit).
 *
 * So while the card is open it measures every stop control on screen (`[data-stop-control]`, never the card's own
 * and never a mirror) and stands in the largest room of the window that holds none of them: above the highest stop,
 * below the lowest, left of the leftmost, right of the rightmost, or between two of them side by side. The room
 * chosen is the one that shows the most of the card; the card is centred in it, no wider than its own 420 px and no
 * taller than the room (it scrolls inside, as it did under its window-height cap). With no stop on screen (the
 * Logbook, Settings, a test without layout) nothing is set and the card is centred as before. Re-measured when the
 * window or the card's content changes size.
 */
function useClearOfStops(backdrop: RefObject<HTMLDivElement | null>, card: RefObject<HTMLDivElement | null>): CSSProperties | undefined {
  const [place, setPlace] = useState<CSSProperties | undefined>(undefined)
  useLayoutEffect(() => {
    const b = backdrop.current
    const c = card.current
    if (!b || !c) return
    const measure = () => {
      const win = b.getBoundingClientRect()
      // The app's zoom: the card is laid out in CSS px inside `.app`, the rects are the window's own.
      const z = b.offsetWidth > 0 ? win.width / b.offsetWidth : 1
      const stops: Rect[] = [...document.querySelectorAll<HTMLElement>('[data-stop-control]')]
        .filter((e) => !c.contains(e) && !e.closest('[data-stop-mirror], [hidden]'))
        .map((e) => e.getBoundingClientRect())
        .filter((r) => r.width > 0 && r.height > 0 && r.right > win.left && r.left < win.right && r.bottom > win.top && r.top < win.bottom)
      if (stops.length === 0) {
        setPlace((p) => (p === undefined ? p : undefined))
        return
      }
      const g = CLEAR * z
      const L = win.left + g
      const T = win.top + g
      const R = win.right - g
      const B = win.bottom - g
      const rooms: Rect[] = [
        { left: L, top: T, right: R, bottom: Math.min(...stops.map((r) => r.top)) - g },
        { left: L, top: Math.max(...stops.map((r) => r.bottom)) + g, right: R, bottom: B },
        { left: L, top: T, right: Math.min(...stops.map((r) => r.left)) - g, bottom: B },
        { left: Math.max(...stops.map((r) => r.right)) + g, top: T, right: R, bottom: B },
      ]
      // Between two stops side by side, the whole height: the strip's Stop TX on the left, a dock's stop on the right.
      const byX = [...stops].sort((a, d) => a.left - d.left)
      let reach = byX[0].right
      for (const r of byX.slice(1)) {
        if (r.left - g > reach + g) rooms.push({ left: reach + g, top: T, right: r.left - g, bottom: B })
        reach = Math.max(reach, r.right)
      }
      // How much of the card each room shows: its own width (420 px) and its content's height, as far as the room allows.
      const wantW = 420 * z
      const wantH = c.scrollHeight * z
      const shown = (r: Rect) => Math.max(0, Math.min(r.right - r.left, wantW)) * Math.max(0, Math.min(r.bottom - r.top, wantH))
      const best = rooms.reduce((a, d) => (shown(d) > shown(a) ? d : a))
      const w = Math.min(best.right - best.left, wantW)
      const h = Math.min(best.bottom - best.top, wantH)
      const next: CSSProperties = {
        position: 'absolute',
        left: Math.round((best.left - win.left + (best.right - best.left - w) / 2) / z),
        top: Math.round((best.top - win.top + (best.bottom - best.top - h) / 2) / z),
        width: Math.floor(w / z),
        maxHeight: Math.floor((best.bottom - best.top) / z),
      }
      setPlace((p) => (p && JSON.stringify(p) === JSON.stringify(next) ? p : next))
    }
    measure()
    // The window, and the card's own parts (a line that wraps, the IOTA box's note) change what it needs.
    const ro = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure)
    ro?.observe(b)
    for (const part of c.children) ro?.observe(part)
    window.addEventListener('resize', measure)
    return () => {
      ro?.disconnect()
      window.removeEventListener('resize', measure)
    }
  }, [backdrop, card])
  return place
}

interface Props {
  /** The completed contact awaiting confirm-before-log. */
  record: LoggedQso
  /** Log the (possibly edited) record, with what the station filled its IOTA box with, while the
   *  box still holds it. */
  onConfirm: (record: LoggedQso, iotaOffer?: IotaOfferData | null) => void
  /** Discard the contact without logging. */
  onDiscard: () => void
  /** How many MORE completed contacts are held behind this one (operator ruling 2026-09-19:
   *  queue them). 0 = this is the only one, and nothing is said about a queue. */
  waiting?: number
  /** How many contacts the queue logged WITHOUT review since this popup was last answered.
   *  0 = nothing to report, and the warning is absent. */
  autoLogged?: number
  onStop?: () => void
}

/** WSJT-X "Prompt me to log QSO" — a small confirm popup shown when a QSO
 * completes and the operator has asked to review before logging. Pre-fills the
 * exchanged details; the call/grid/reports and the comment stay editable, and so do the
 * station boxes WSJT-X's dialog has (power, operator, the grid it was made from), filled
 * with what Nexus stamped, and the propagation mode, which the station keeps for the next
 * contact as WSJT-X's "Retain" does. */
export function LogConfirm({ record, onConfirm, onDiscard, waiting = 0, autoLogged = 0, onStop }: Props) {
  const [call, setCall] = useState(record.call)
  const [grid, setGrid] = useState(record.grid ?? '')
  const [rstSent, setRstSent] = useState(record.rstSent?.toString() ?? '')
  const [rstRcvd, setRstRcvd] = useState(record.rstRcvd?.toString() ?? '')
  // #371: the contact's COMMENT, as held (our dB-reports one when that option is on), so an
  // untouched field logs exactly what it would have; the engine logs what is sent here.
  const [comment, setComment] = useState(record.comment ?? '')
  // The station boxes, as Nexus stamped them. A cleared one sends nothing, and the log fills it
  // in as it would have.
  const [power, setPower] = useState(record.txPower != null ? String(record.txPower) : '')
  const [operator, setOperator] = useState(record.operator ?? '')
  const [myGrid, setMyGrid] = useState(record.myGrid ?? '')
  const [propMode, setPropMode] = useState(record.propMode ?? '')
  // The IOTA reference: what the hold filled the contact with, else what the station names for this
  // call now (a lookup that answered after the hold). Logged as the box holds it, so the operator can
  // change or clear it first (operator ruling, 2026-11-10).
  const [iota, setIota] = useState(record.ota?.iota ?? '')
  // A reference the hold filled carries Nexus's fill mark: it is the busted call's if the operator
  // corrects the call, so the box lets go of it then and takes the corrected call's own.
  const heldFill = record.extra?.some(([k, v]) => k === 'APP_NEXUS_OFFER_HELD' && v.split(',').some((t) => t.trim() === 'IOTA'))
    ? (record.ota?.iota ?? null)
    : null
  const iotaPrefill = useIotaPrefill(call, iota, setIota, undefined, false, heldFill)
  // The island within that group: only the operator's pick from its list, never filled.
  const [island, setIsland] = useIotaIslandPick(iota)

  // RST is a free string now (CW "599" / phone "59" / digital "-12"); just trim.
  const parseRst = (v: string): string | null => {
    const t = v.trim()
    return t === '' ? null : t
  }

  // A box no spelling makes a reference of holds Log, with the box saying why: never logged as none.
  const iotaBad = iotaInvalid(iota)
  const confirm = () => {
    if (!call.trim() || iotaBad) return
    onConfirm({
      ...record,
      call: call.trim().toUpperCase(),
      grid: grid.trim() ? grid.trim().toUpperCase() : null,
      rstSent: parseRst(rstSent),
      rstRcvd: parseRst(rstRcvd),
      comment: comment.trim() || null,
      txPower: parsePower(power),
      operator: operator.trim().toUpperCase() || null,
      myGrid: myGrid.trim().toUpperCase() || null,
      propMode: propMode || null,
      ota: { ...record.ota, iota: iotaToLog(iota) },
      extra: withIotaIsland(record.extra, iotaToLog(iota) ? island : null),
    }, iotaPrefill.taken)
  }

  const backdropRef = useRef<HTMLDivElement>(null)
  const cardRef = useRef<HTMLDivElement>(null)
  const place = useClearOfStops(backdropRef, cardRef)

  // Watts, as the log writes TX_PWR; anything that is not a positive number sends nothing.
  const parsePower = (v: string): number | null => {
    const w = Number(v.trim())
    return v.trim() !== '' && Number.isFinite(w) && w > 0 ? w : null
  }

  // Log and Discard hold to the bottom of the card while it scrolls (styles.css, `.log-prompt
  // .logconfirm-actions`), so whatever is scrolled to its end — `scrollIntoView`, a focus moving into the
  // IOTA box — would land under them. The card keeps that much clear (`scroll-padding-bottom`): the
  // row's whole height, since scrolled to its end the row sits in its place, bottom padding and all.
  const actionsRef = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const row = actionsRef.current
    const card = row?.parentElement
    if (!row || !card) return
    const apply = () => card.style.setProperty('--log-prompt-actions-h', `${row.offsetHeight}px`)
    apply()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(apply)
    ro.observe(row)
    return () => ro.disconnect()
  }, [])

  return (
    <div ref={backdropRef} className="logconfirm-backdrop" role="dialog" aria-modal="true" aria-label={t('logPrompt.aria')}>
      <div ref={cardRef} className="logconfirm log-prompt" style={place}>
        <div className="logconfirm-head">
          <h2>{t('logPrompt.title')}</h2>
          <span className="logconfirm-sub">
            {record.band} · {record.mode}
          </span>
        </div>

        {/* The queue behind this contact. An operator who logs one and sees another appear
            should have been told it was coming. */}
        {waiting > 0 && (
          <p className="logconfirm-waiting" role="status">
            {t('logPrompt.waiting', { count: waiting })}
          </p>
        )}

        {/* A contact the queue logged at its cap, unreviewed. It is in the log, the connectors
            will upload it and LoTW will carry it under the operator's certificate, so this is a
            WARNING, not a note — and it belongs here rather than only in the Connections log,
            because this popup is where the operator comes back to. */}
        {autoLogged > 0 && (
          <p className="logconfirm-autologged" role="alert">
            {t('logPrompt.autoLogged', { count: autoLogged })}
          </p>
        )}

        <div className="logconfirm-grid">
          <label>
            <span>{t('logPrompt.call.label')}</span>
            <input
              className="mono"
              value={call}
              autoFocus
              onChange={(e) => setCall(upperTyped(e.target))}
            />
          </label>
          <label>
            <span>{t('logPrompt.grid.label')}</span>
            <input
              className="mono"
              value={grid}
              maxLength={6}
              placeholder="—"
              onChange={(e) => setGrid(upperTyped(e.target))}
            />
          </label>
          <label>
            <span>{t('logPrompt.rstSent.label')}</span>
            <input
              className="mono"
              value={rstSent}
              placeholder="—"
              onChange={(e) => setRstSent(e.target.value)}
            />
          </label>
          <label>
            <span>{t('logPrompt.rstRcvd.label')}</span>
            <input
              className="mono"
              value={rstRcvd}
              placeholder="—"
              onChange={(e) => setRstRcvd(e.target.value)}
            />
          </label>
          <label>
            <span>{t('logPrompt.power.label')}</span>
            <input
              className="mono"
              value={power}
              inputMode="decimal"
              placeholder="—"
              onChange={(e) => setPower(e.target.value)}
            />
          </label>
          <label>
            <span>{t('logPrompt.operator.label')}</span>
            <input
              className="mono"
              value={operator}
              placeholder="—"
              onChange={(e) => setOperator(upperTyped(e.target))}
            />
          </label>
          <label>
            <span>{t('logPrompt.myGrid.label')}</span>
            <input
              className="mono"
              value={myGrid}
              maxLength={10}
              placeholder="—"
              onChange={(e) => setMyGrid(upperTyped(e.target))}
            />
          </label>
          <label className="logconfirm-propmode" title={t('logPrompt.propMode.hint')}>
            <span>{t('logPrompt.propMode.label')}</span>
            <select value={propMode} onChange={(e) => setPropMode(e.target.value)}>
              <option value="">—</option>
              {record.propMode === 'SAT' && (
                <option value="SAT">{propModeName('SAT')}</option>
              )}
              {PROP_MODES.map((code) => (
                <option key={code} value={code}>
                  {propModeName(code)}
                </option>
              ))}
            </select>
          </label>
          <label className="logconfirm-comment">
            <span>{t('logPrompt.comment.label')}</span>
            <input
              value={comment}
              placeholder="—"
              onChange={(e) => setComment(e.target.value)}
            />
          </label>
        </div>

        <IotaPrefillField state={iotaPrefill} value={iota} island={{ value: island, onChange: setIsland }} />

        <div className="logconfirm-actions" ref={actionsRef}>
          {onStop && <FtStopControl onHaltTx={onStop} />}
          <button type="button" className="logconfirm-discard" onClick={onDiscard}>
            {t('logPrompt.discard')}
          </button>
          <button type="button" className="logconfirm-log" onClick={confirm} disabled={!call.trim() || iotaBad}>
            {t('logPrompt.log')}
          </button>
        </div>
      </div>
    </div>
  )
}
