// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog (`i18n/en.ts`); a hardcoded one fails CI. What does NOT come from
// the catalog: LOG_EXAMPLES and the four service/Q-code labels below, and every value the table
// prints — callsign, band, mode, frequency, RST, park reference, QSL letters. Those are wire
// formats, identical in every language. See the invariant-token rule in `i18n/index.ts`.

import { lazy, Suspense, useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { confirmDialog } from '../confirm'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { LoggedQso } from '../types'
import { gpuCapableForGlobe } from '../gpu'
import { SpotDialog } from './SpotDialog'

// The 3-D QSO globe band. Lazy so three.js/react-globe.gl only download when the
// Logbook actually shows it (same pattern as ConnectView's Globe3D) — a weak-GPU
// machine that fails `gpuCapableForGlobe` never pays for the chunk at all.
const QsoGlobe = lazy(() => import('./QsoGlobe'))

import {
  deleteQso,
  editQso,
  exportGeneralLog,
  exportLogForOperator,
  logOperators,
  getLog,
  importAdif,
  logQso,
  markLotwUploaded,
  markQslSent,
  purgeLog,
  qrzLookup,
  saveTextToDownloads,
  syncLotwReport,
  uploadLotwReport,
} from '../api'
import { pushToast, withErrorToast } from '../toast'
import { qrzPushQso, clublogPushQso, hrdlogPushQso, openQrzPage, syncQrz, downloadLotwReport, importPotaLog } from '../api'

interface Props {
  /** Default band / freq / mode for new manual entries (from the radio). */
  defaultBand: string
  defaultFreqMhz: number
  defaultMode: string
}

interface DraftQso {
  call: string
  grid: string
  band: string
  freq: string
  mode: string
  rstSent: string
  rstRcvd: string
  name: string
  qth: string
  comment: string
  notes: string
  /** UTC date+time the contact happened, as `YYYY-MM-DDTHH:MM` (the datetime-local format).
   * THE field that makes hand-logging work: you log a 2 m contact after the fact, so stamping
   * "now" writes the wrong time into the log and into every upload downstream. */
  whenUtc: string
  /** US state (WAS). The auto-log path fills this from the callsign/grid; a hand-logged
   * contact has no decode to derive it from, so it has to be typeable or WAS silently misses. */
  state: string
  /** TX power in watts (ADIF TX_PWR) — part of a complete hand-log, and some awards want it. */
  txPower: string
  /** POTA/SIG park references: the worked station's park (`ota.theirRef`, the hunter case) and
   * your own activation (`ota.myRef`). Editable so a park can be viewed, corrected or added on
   * an existing QSO (#60). Only these two of the five `Ota` fields are exposed; the loaded
   * program (POTA/SOTA/WWFF) and any `iota` ride through `submit` from the stored record so an
   * edit never clobbers a non-POTA program or drops an island reference. */
  parkTheirRef: string
  parkMyRef: string
}

/** The word the operator must type to arm the full-log purge (irreversible). It is COMPARED
 * against what they type, so it is a token and not prose — translating it would move the gate. */
const PURGE_WORD = 'DELETE'

/**
 * The invariant example values the hand-log form shows in empty fields, gathered so the guard can
 * prove they never became catalog entries. Callsign, grid, band, mode, signal reports, a state
 * code, a power in watts and two POTA references: every one a wire format, so every one the same
 * characters in every language. The two placeholders that are HUMAN prose (a first name, a town)
 * are catalog entries instead.
 */
const LOG_EXAMPLES = {
  call: 'W1AW',
  grid: 'FN31',
  band: '20m',
  mode: 'TempoFast',
  rstSent: '59 / 599 / -09',
  rstRcvd: '59 / 599 / -11',
  state: 'WI',
  txPower: '100',
  parkTheirRef: 'US-1234',
  parkMyRef: 'US-5678',
} as const

/** Q-codes and service names printed as labels. Proper nouns and shorthand, not words. */
const QRZ_LABEL = 'QRZ'
const EQSL_LABEL = 'eQSL'
const CLUBLOG_LABEL = 'CL'
const HRDLOG_LABEL = 'HL'
const QSL_MENU_LABEL = 'QSL▸'

/** Parse a `datetime-local` value as UTC seconds. The browser's own Date parsing treats a
 * bare `YYYY-MM-DDTHH:MM` as LOCAL time; a log is UTC, so an operator in EN52 typing the UTC
 * time off their clock would otherwise have it silently shifted by their offset. Returns null
 * for an empty/unparseable value so the caller can fall back. */
function parseUtcLocal(v: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(v.trim())
  if (!m) return null
  const [, y, mo, d, h, mi] = m
  const ms = Date.UTC(+y, +mo - 1, +d, +h, +mi)
  return Number.isNaN(ms) ? null : Math.floor(ms / 1000)
}

/** Format Unix seconds as the `datetime-local` UTC value the form edits. */
function toUtcLocal(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}T${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`
}

function fmtUtc(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(
    d.getUTCHours(),
  )}:${p(d.getUTCMinutes())}Z`
}

function fmtReport(v: string | null): string {
  return v && v.trim() !== '' ? v : '—'
}

/** ADIF QSL_SENT_VIA letter → human word for the "sent via …" note. The LETTER is the wire
 * value and stays a token; the word is prose, so it comes from the catalog. */
function qslViaLabel(via: string): string | null {
  switch (via.toUpperCase()) {
    case 'B':
      return t('logbook.qsl.via.bureau')
    case 'D':
      return t('logbook.qsl.via.direct')
    case 'E':
      return t('logbook.qsl.via.electronic')
    default:
      return null
  }
}

/** A quiet "sent <date> via <method>" note for a row that's been QSL-requested.
 *
 * FOUR WHOLE SENTENCES, not a stem plus two tails: a date and a delivery method land in a
 * different place in different languages, and a sentence glued from fragments cannot be
 * re-ordered by a translator. */
function fmtQslSent(sent: { sent: boolean; via: string | null; dateUnix: number | null }): string {
  const via = sent.via ? qslViaLabel(sent.via) ?? sent.via : null
  const date = sent.dateUnix ? fmtUtc(sent.dateUnix).slice(0, 10) : null
  if (date && via) return t('logbook.qsl.sentOnVia', { date, via })
  if (date) return t('logbook.qsl.sentOn', { date })
  if (via) return t('logbook.qsl.sentVia', { via })
  return t('logbook.qsl.sent')
}

// RST is a free string now (CW "599" / phone "59" / digital "-12"); just trim.
function parseReport(s: string): string | null {
  const t = s.trim()
  return t === '' ? null : t
}

// Sortable columns. `band` sorts by frequency (more meaningful than the label string).
type SortKey = 'call' | 'country' | 'band' | 'freq' | 'mode' | 'sent' | 'rcvd' | 'time' | 'park' | 'qsl'
function sortVal(q: LoggedQso, k: SortKey): string | number {
  switch (k) {
    case 'call':
      return q.call.toUpperCase()
    case 'country':
      return (q.country ?? '').toUpperCase()
    case 'band':
    case 'freq':
      return q.freqMhz
    case 'mode':
      return q.mode.toUpperCase()
    case 'sent':
      return (q.rstSent ?? '').toUpperCase()
    case 'rcvd':
      return (q.rstRcvd ?? '').toUpperCase()
    case 'time':
      return q.whenUnix
    case 'park':
      return (q.ota?.theirRef ?? q.ota?.myRef ?? '').toUpperCase()
    case 'qsl':
      return q.awardConfirmed ? 2 : q.confirmed ? 1 : 0
  }
}
/** Sensible default direction when switching TO a column: text ascending, numeric/time descending. */
function defaultAsc(k: SortKey): boolean {
  return k === 'call' || k === 'country' || k === 'mode' || k === 'sent' || k === 'rcvd' || k === 'park'
}

export function Logbook({
  defaultBand,
  defaultFreqMhz,
  defaultMode,
}: Props) {
  const [log, setLog] = useState<LoggedQso[]>([])
  const [showForm, setShowForm] = useState(false)
  const [draft, setDraft] = useState<DraftQso>(() => ({
    call: '',
    grid: '',
    band: defaultBand,
    freq: defaultFreqMhz.toFixed(4),
    mode: defaultMode,
    rstSent: '',
    rstRcvd: '',
    name: '',
    qth: '',
    comment: '',
    notes: '',
    whenUtc: '',
    state: '',
    txPower: '',
    parkTheirRef: '',
    parkMyRef: '',
  }))
  const [err, setErr] = useState<string | null>(null)
  // Operators present in the log (#25) — drives whether the per-operator export is offered.
  // Recomputed when the log size changes, which is when a new operator can first appear.
  const [operators, setOperators] = useState<string[]>([])
  useEffect(() => {
    // Nothing to ask about an empty log, and asking anyway is not free: this fires on mount,
    // before the log has loaded, and the extra state update it lands shifts the first few
    // renders. That is what put the Purge button's disabled read on the wrong side of a
    // waitFor in Logbook.test.tsx — a real timing change, not a flaky test.
    if (log.length === 0) {
      setOperators((prev) => (prev.length === 0 ? prev : []))
      return
    }
    void logOperators()
      .then((next) =>
        // Identity-stable when unchanged, so a reload of the same log does not re-render.
        setOperators((prev) =>
          prev.length === next.length && prev.every((v, i) => v === next[i]) ? prev : next,
        ),
      )
      .catch(() => {}) // no bridge / older core — just don't offer the split
  }, [log.length])
  const [qrzBusy, setQrzBusy] = useState(false)
  const [uploading, setUploading] = useState(false)
  const [search, setSearch] = useState('')
  // Filtering runs against a DEFERRED copy of the search so typing stays responsive on a 10k log —
  // the input updates instantly; the (memoized) filter/sort catches up a frame later.
  const deferredSearch = useDeferredValue(search)
  // Filter to contacts still lacking an award-eligible confirmation (the DX
  // chaser's "who do I still need a card/LoTW from" view).
  const [needsConfirmOnly, setNeedsConfirmOnly] = useState(false)
  // Purge-the-whole-log confirmation modal. `purgeText` must equal PURGE_WORD to
  // arm the danger button — a deliberate, typed gate for an irreversible wipe.
  const [showPurge, setShowPurge] = useState(false)
  const [purgeText, setPurgeText] = useState('')
  const [purging, setPurging] = useState(false)
  // "Mark all as already on LoTW" confirmation — for an imported legacy log that was
  // uploaded through another tool, so the unsent count reflects reality.
  const [showMarkLotw, setShowMarkLotw] = useState(false)
  // Index (in the loaded `log` array) being edited; null = the form logs a NEW QSO.
  const [editIndex, setEditIndex] = useState<number | null>(null)
  // Column sort — purely a VIEW concern; the backend `get_log` index is kept on each row so
  // edit/delete/mark still hit the right record. Default newest-first (the get_log order is
  // oldest-first, which the test user disliked).
  // Re-spot a logged contact to the cluster (row 📢): seeded with the row's call,
  // frequency and mode (operator ask 2026-07-21).
  const [spotSeed, setSpotSeed] = useState<{ call: string; freq: number; mode: string } | null>(null)
  const [sortKey, setSortKey] = useState<SortKey>('time')
  const [sortAsc, setSortAsc] = useState(false)
  // Export date range (#98) — "YYYY-MM-DD" UTC or '' = unbounded. Both empty = whole log.
  const [exportFrom, setExportFrom] = useState('')
  const [exportTo, setExportTo] = useState('')
  const fileRef = useRef<HTMLInputElement>(null)
  const syncRef = useRef<HTMLInputElement>(null)
  const potaRef = useRef<HTMLInputElement>(null)

  // QRZ lookup for the QSO being logged: fills grid (subscriber-only) + shows the
  // operator name. On-demand (QRZ free tier is ~100/day), one lookup per click.
  const onQrzLookup = async () => {
    const call = draft.call.trim()
    if (!call) return
    setQrzBusy(true)
    const r = await withErrorToast(() => qrzLookup(call), t('callbook.lookupFailed'))
    setQrzBusy(false)
    if (r) {
      if (r.grid && !draft.grid.trim()) setField('grid', r.grid)
      const preferredName = r.nickname || r.name
      if (preferredName && !draft.name.trim()) setField('name', preferredName)
      const detail = [r.name, r.grid && t('callbook.detail.grid', { grid: r.grid }), r.state]
        .filter(Boolean)
        .join(' · ')
      const vals = { call: r.call, detail: detail || r.country || t('callbook.detail.found') }
      pushToast(r.grid ? t('callbook.result', vals) : t('callbook.resultNoGrid', vals), 'info')
    }
  }

  const load = useCallback(() => {
    getLog()
      .then(setLog)
      .catch(() => {})
  }, [])

  useEffect(() => {
    load()
  }, [load])

  // Import an external ADIF logbook → real "needs" + B4. Read the file in the
  // browser/WebView (no fs plugin), hand the text to the engine.
  const onImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0]
    e.target.value = '' // let the same file be re-selected later
    if (!f) return
    const text = await f.text()
    const stats = await withErrorToast(() => importAdif(text), t('logbook.import.failed'))
    if (stats) {
      // An import of a LoTW/eQSL download adds nothing and updates everything —
      // say so, or the one toast that means "your awards were just repaired"
      // reads as "nothing happened, all dupes".
      // Three statements, each with its OWN count: one message cannot select a plural form
      // for two counts at once, so each carries its own (see the catalog's note).
      const dupes = stats.skipped ? t('logbook.import.dupes', { count: stats.skipped }) : ''
      const upgraded = stats.updated ? t('logbook.import.updated', { count: stats.updated }) : ''
      pushToast(
        `${t('logbook.import.imported', { count: stats.added })}${dupes}${upgraded}`,
        'success',
      )
      load()
    }
  }

  // Sync a LoTW (or any ADIF) confirmation report INTO the log: upgrades
  // confirmation + credit on already-logged QSOs (which a plain import skips).
  const onSyncFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0]
    e.target.value = ''
    if (!f) return
    const text = await f.text()
    const r = await withErrorToast(() => syncLotwReport(text), t('logbook.sync.failed'))
    if (r) {
      const vals = { confirmed: r.newlyConfirmed, credited: r.newlyCredited }
      pushToast(
        r.orphans.length
          ? t('logbook.sync.doneUnmatched', { ...vals, unmatched: r.orphans.length })
          : t('logbook.sync.done', vals),
        r.orphans.length ? 'info' : 'success',
      )
      load()
    }
  }

  // pota.app hunter/activator export → stamp park refs onto MATCHING logged QSOs.
  // Stamp-only by design (the operator's anti-abuse rule): never creates records,
  // never overwrites a ref. The reviewed-adds flow is a separate roadmap feature.
  const onPotaFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0]
    e.target.value = ''
    if (!f) return
    const text = await f.text()
    const r = await withErrorToast(() => importPotaLog(text), t('logbook.pota.failed'))
    if (r) {
      const skipped = r.unmatched ? t('logbook.pota.unmatched', { count: r.unmatched }) : ''
      const had = r.already ? t('logbook.pota.already', { count: r.already }) : ''
      pushToast(`${t('logbook.pota.stamped', { count: r.stamped })}${had}${skipped}`, 'success')
      load()
    }
  }

  // QSOs not yet sent to LoTW: award-unconfirmed + never uploaded or a prior bounce.
  // Mirrors the backend batch builder (lotw_unsent_indices): award-unconfirmed,
  // never-sent-or-bounced, AND the time of day is known — LoTW matches on time,
  // so a date-only import can never confirm and is excluded, with the count
  // shown separately so the operator learns why instead of wondering.
  const lotwEligible = (q: LoggedQso) =>
    !q.awardConfirmed &&
    (!q.upload?.lotw || ['rejected', 'authfail'].includes(q.upload.lotw.outcome))
  const unsentLotw = log.filter((q) => lotwEligible(q) && q.timeKnown !== false).length
  const timelessLotw = log.filter((q) => lotwEligible(q) && q.timeKnown === false).length

  // Sign + upload the unsent batch to LoTW via the operator's TQSL.
  const onUploadLotw = async () => {
    setUploading(true)
    const r = await withErrorToast(() => uploadLotwReport(), t('logbook.lotw.upload.failed'))
    setUploading(false)
    if (!r) return
    const n = r.dispatched
    if (r.outcome === 'none') pushToast(t('logbook.lotw.upload.nothingNew'), 'info')
    else if (r.outcome === 'pending')
      pushToast(t('logbook.lotw.upload.pending', { count: n }), 'success')
    else if (r.outcome === 'duplicate')
      pushToast(t('logbook.lotw.upload.duplicate', { count: n }), 'info')
    else if (r.outcome === 'retry')
      pushToast(r.detail || t('logbook.lotw.upload.retry'), 'error')
    else if (r.outcome === 'authfail')
      pushToast(
        r.detail
          ? t('logbook.lotw.upload.authFailedDetail', { detail: r.detail })
          : t('logbook.lotw.upload.authFailed'),
        'error',
      )
    else
      pushToast(
        r.detail
          ? t('logbook.lotw.upload.failedDetail', { detail: r.detail })
          : t('logbook.lotw.upload.failed'),
        'error',
      )
    load()
  }

  const onMarkLotwUploaded = async () => {
    setShowMarkLotw(false)
    const n = await withErrorToast(() => markLotwUploaded(), t('logbook.markLotw.failed'))
    if (n == null) return
    pushToast(
      n > 0
        ? // `count` selects the plural form; `formatted` is the grouped count the operator
          // reads. A count of contacts is not a technical quantity — see the catalog note.
          t('logbook.markLotw.done', { count: n, formatted: n.toLocaleString() })
        : t('logbook.markLotw.nothing'),
      'success',
    )
    load()
  }

  const setField = (k: keyof DraftQso, v: string) => {
    setErr(null)
    setDraft((prev) => ({ ...prev, [k]: v }))
  }

  // Open the form pre-filled to correct an existing entry (busted call, wrong band…).
  const startEdit = (q: LoggedQso, i: number) => {
    setErr(null)
    setEditIndex(i)
    setDraft({
      call: q.call,
      grid: q.grid ?? '',
      band: q.band,
      freq: q.freqMhz.toFixed(4),
      mode: q.mode,
      rstSent: q.rstSent ?? '',
      rstRcvd: q.rstRcvd ?? '',
      name: q.name ?? '',
      qth: q.qth ?? '',
      comment: q.comment ?? '',
      notes: q.notes ?? '',
      whenUtc: toUtcLocal(q.whenUnix),
      state: q.state ?? '',
      txPower: q.txPower != null ? String(q.txPower) : '',
      parkTheirRef: q.ota?.theirRef ?? '',
      parkMyRef: q.ota?.myRef ?? '',
    })
    setShowForm(true)
  }

  const cancelForm = () => {
    setShowForm(false)
    setEditIndex(null)
    setErr(null)
  }

  // Manual (re-)push of one logged QSO to QRZ — the VERIFICATION path: push a
  // real contact you already made and check it lands on logbook.qrz.com. A
  // "duplicate" answer is the benign proof it was already there.
  const onPushQrz = async (q: LoggedQso) => {
    try {
      const r = await qrzPushQso(q)
      if (r.result === 'ok' || r.result === 'replace') {
        pushToast(t('logbook.push.qrz.ok', { call: q.call }), 'success', 4000)
      } else if (r.result === 'duplicate') {
        pushToast(t('logbook.push.qrz.duplicate', { call: q.call }), 'success', 5000)
      } else {
        pushToast(
          t('logbook.push.qrz.rejected', { call: q.call, reason: r.reason ?? r.result }),
          'error',
          6000,
        )
      }
    } catch (e) {
      pushToast(t('logbook.push.qrz.failed', { detail: String(e) }), 'error', 6000)
    }
  }

  // Manual (re-)push of one logged QSO to ClubLog — same verification/bounce-
  // recovery role as onPushQrz; "duplicate" is the benign already-there answer.
  const onPushClublog = async (q: LoggedQso) => {
    try {
      const r = await clublogPushQso(q)
      if (r.result === 'ok' || r.result === 'modified') {
        pushToast(t('logbook.push.clublog.ok', { call: q.call }), 'success', 4000)
      } else if (r.result === 'duplicate') {
        pushToast(t('logbook.push.clublog.duplicate', { call: q.call }), 'success', 5000)
      } else {
        pushToast(
          t('logbook.push.clublog.rejected', { call: q.call, reason: r.message ?? r.result }),
          'error',
          6000,
        )
      }
    } catch (e) {
      pushToast(t('logbook.push.clublog.failed', { detail: String(e) }), 'error', 6000)
    }
  }

  // Manual (re-)push of one logged QSO to HRDLog.net — same verification/bounce-
  // recovery role as onPushQrz. HRDLog.net is a live-logging/awards site, NOT an
  // ARRL confirmation source, so a success here is not DXCC/WAS credit.
  const onPushHrdlog = async (q: LoggedQso) => {
    try {
      const r = await hrdlogPushQso(q)
      if (r.result === 'ok') {
        pushToast(t('logbook.push.hrdlog.ok', { call: q.call }), 'success', 4000)
      } else if (r.result === 'duplicate') {
        pushToast(t('logbook.push.hrdlog.duplicate', { call: q.call }), 'success', 5000)
      } else if (r.result === 'unknown') {
        // Transient by contract (server down / odd body) — saying "rejected"
        // would imply the QSO itself is permanently bad. Match the auto-push.
        pushToast(t('logbook.push.hrdlog.unavailable', { call: q.call }), 'info', 6000)
      } else {
        pushToast(
          t('logbook.push.hrdlog.rejected', { call: q.call, reason: r.message ?? r.result }),
          'error',
          6000,
        )
      }
    } catch (e) {
      pushToast(t('logbook.push.hrdlog.failed', { detail: String(e) }), 'error', 6000)
    }
  }

  // Record an operator-declared QSL request on a contact (a card/request WAS sent,
  // via bureau/direct/electronic). This is NOT a confirmation — it stays in the
  // needs-confirmation filter until the partner actually confirms.
  const onMarkQslSent = async (q: LoggedQso, i: number, via: 'B' | 'D' | 'E') => {
    const snap = await withErrorToast(() => markQslSent(i, via), t('logbook.qsl.markFailed'))
    if (snap) {
      pushToast(t('logbook.qsl.marked', { call: q.call, via: qslViaLabel(via) ?? via }), 'success')
      load()
    }
  }

  const onDelete = async (q: LoggedQso, i: number) => {
    if (
      !(await confirmDialog({
        title: t('logbook.delete.heading', { call: q.call, band: q.band }),
        body: t('logbook.delete.body'),
        confirmLabel: t('logbook.delete.confirm'),
        danger: true,
      }))
    )
      return
    const snap = await withErrorToast(() => deleteQso(i), t('logbook.delete.failed'))
    if (snap) {
      pushToast(t('logbook.delete.done', { call: q.call }), 'success')
      if (editIndex === i) cancelForm()
      load()
    }
  }

  const closePurge = () => {
    setShowPurge(false)
    setPurgeText('')
  }

  // Wipe the ENTIRE logbook (truncates the ADIF file). Armed only once the operator
  // types the confirmation word — an irreversible action gets a deliberate gate.
  const onPurge = async () => {
    if (purgeText.trim().toUpperCase() !== PURGE_WORD) return
    setPurging(true)
    const removed = await withErrorToast(() => purgeLog(), t('logbook.purge.failed'))
    setPurging(false)
    if (removed !== null && removed !== undefined) {
      pushToast(t('logbook.purge.done', { count: removed }), 'success')
      closePurge()
      cancelForm()
      load()
    }
  }

  const matchesSearch = useCallback(
    (q: LoggedQso): boolean => {
      if (needsConfirmOnly && q.awardConfirmed) return false
      const t = deferredSearch.trim().toLowerCase()
      if (!t) return true
      return (
        q.call.toLowerCase().includes(t) ||
        (q.country?.toLowerCase().includes(t) ?? false) ||
        (q.grid?.toLowerCase().includes(t) ?? false) ||
        q.band.toLowerCase().includes(t) ||
        q.mode.toLowerCase().includes(t) ||
        fmtUtc(q.whenUnix).toLowerCase().includes(t)
      )
    },
    [deferredSearch, needsConfirmOnly],
  )

  // Filter + sort ONCE per data/criteria change (not on every render, e.g. the frequent dial-poll
  // re-renders). `i` is the backend get_log index, kept glued to each record so edit/delete/mark
  // still target the right row regardless of display order.
  const rows = useMemo(() => {
    const out = log.map((q, i) => ({ q, i })).filter(({ q }) => matchesSearch(q))
    out.sort((a, b) => {
      const av = sortVal(a.q, sortKey)
      const bv = sortVal(b.q, sortKey)
      const cmp = av < bv ? -1 : av > bv ? 1 : a.q.whenUnix - b.q.whenUnix
      return sortAsc ? cmp : -cmp
    })
    return out
  }, [log, matchesSearch, sortKey, sortAsc])

  // 3-D globe band, gated on a real GPU (software renderers would make the whole
  // Logbook crawl — those machines just get the plain table). Probed once per mount.
  const [globeOk] = useState(gpuCapableForGlobe)
  const globeShown = globeOk && log.length > 0

  // Virtualize the row list: at 10k QSOs the old render put ~150k DOM nodes on screen (heavy scroll
  // + a full reconcile every dial-poll re-render). Now only the visible window mounts.
  const scrollRef = useRef<HTMLDivElement>(null)
  const rowsWrapRef = useRef<HTMLDivElement>(null)
  const [listOffset, setListOffset] = useState(0)
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 43, // ~row height; measureElement corrects per row
    overscan: 12,
    // The globe band + sticky search/header block sit INSIDE the scroll container
    // above the rows (globe scrolls away; search+headers pin). scrollMargin tells the
    // virtualizer how far into the scroll space the list starts — MEASURED, because
    // the sticky block's height isn't a constant; the row transform subtracts it back.
    scrollMargin: listOffset,
  })

  // Measure where the rows actually start inside .log-scroll (globe + sticky block).
  useLayoutEffect(() => {
    const el = rowsWrapRef.current
    if (!el) return
    const measure = () => setListOffset(el.offsetTop)
    measure()
    const ro = new ResizeObserver(measure)
    if (el.parentElement) ro.observe(el.parentElement)
    return () => ro.disconnect()
  })

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    const call = draft.call.trim().toUpperCase()
    if (!call) {
      setErr(t('logbook.form.callRequired'))
      return
    }
    const freq = Number(draft.freq)
    const existing = editIndex !== null ? log[editIndex] : undefined
    const parkTheirRef = draft.parkTheirRef.trim().toUpperCase() || null
    const parkMyRef = draft.parkMyRef.trim().toUpperCase() || null
    // Only the two park REFS are editable. The program (POTA/SOTA/WWFF) and any IOTA reference
    // ride through from the stored record: a park-only `ota` would trip the backend's ota-preserve
    // guard, which tests only the four park fields and would silently drop `iota`. With both refs
    // blank we send no `ota` at all, so that guard restores the stored `ota` untouched (which is
    // also why clearing a park to REMOVE it does not take effect — a separate backend change).
    const ota: LoggedQso['ota'] =
      parkTheirRef || parkMyRef
        ? {
            theirRef: parkTheirRef,
            theirProgram: parkTheirRef ? existing?.ota?.theirProgram || 'POTA' : null,
            myRef: parkMyRef,
            myProgram: parkMyRef ? existing?.ota?.myProgram || 'POTA' : null,
            iota: existing?.ota?.iota ?? null,
          }
        : undefined
    const record: LoggedQso = {
      call,
      grid: draft.grid.trim() || null,
      band: draft.band.trim(),
      freqMhz: Number.isNaN(freq) ? defaultFreqMhz : freq,
      mode: draft.mode.trim(),
      rstSent: parseReport(draft.rstSent),
      rstRcvd: parseReport(draft.rstRcvd),
      name: draft.name.trim() || null,
      qth: draft.qth.trim() || null,
      comment: draft.comment.trim() || null,
      notes: draft.notes.trim() || null,
      state: draft.state.trim().toUpperCase() || null,
      txPower: draft.txPower.trim() ? Number(draft.txPower) : null,
      ota,
      // The operator's typed UTC wins; otherwise keep the original (edit) or stamp now (new).
      // Hand-logging is inherently after the fact, so "now" is the wrong default whenever the
      // operator has told us when it actually happened.
      whenUnix: parseUtcLocal(draft.whenUtc)
        ?? (existing ? existing.whenUnix : Math.floor(Date.now() / 1000)),
      confirmed: existing ? existing.confirmed : false,
      awardConfirmed: existing ? existing.awardConfirmed : false,
      // Upload/confirmation policy lives in the BACKEND (Logbook::update_record),
      // which this payload cannot override: a CALLSIGN correction clears the
      // upload stamps (the corrected QSO re-queues to every service) and strips
      // confirmations matched on the busted call; every other edit preserves
      // them. The `upload: undefined` this line used to carry was triple-dead —
      // dropped by JSON.stringify, refilled by serde's default, and overwritten
      // from the stored record anyway.
      upload: existing?.upload,
    }
    if (editIndex !== null) {
      const idx = editIndex
      const snap = await withErrorToast(() => editQso(idx, record), t('logbook.form.saveFailed'))
      if (snap) {
        pushToast(t('logbook.form.updated', { call: record.call }), 'success')
        cancelForm()
        setDraft((prev) => ({ ...prev, call: '', grid: '', rstSent: '', rstRcvd: '', name: '', qth: '', comment: '', notes: '', whenUtc: '', state: '', txPower: '', parkTheirRef: '', parkMyRef: '' }))
        load()
      }
      return
    }
    const snap = await withErrorToast(() => logQso(record), t('logbook.form.logFailed'))
    if (snap) {
      load()
      setShowForm(false)
      setDraft((prev) => ({ ...prev, call: '', grid: '', rstSent: '', rstRcvd: '', name: '', qth: '', comment: '', notes: '', whenUtc: '', state: '', txPower: '' }))
      // QRZ/ClubLog/eQSL auto-upload happens in the BACKEND log funnel now
      // (every log path, the engine auto-log included); outcomes toast via the
      // snapshot uploadTick.
    }
  }

  // A clickable, sort-toggling column header. Clicking the active column flips direction;
  // clicking a new column jumps to its sensible default direction.
  const th = (label: string, k: SortKey) => (
    <button
      type="button"
      className={`log-cell log-th${sortKey === k ? ' sorted' : ''}`}
      role="columnheader"
      aria-sort={sortKey === k ? (sortAsc ? 'ascending' : 'descending') : 'none'}
      onClick={() => {
        if (sortKey === k) setSortAsc((v) => !v)
        else {
          setSortKey(k)
          setSortAsc(defaultAsc(k))
        }
      }}
    >
      {label}
      {sortKey === k ? (sortAsc ? ' ▲' : ' ▼') : ''}
    </button>
  )

  return (
    <section className="panel log-view logbook">
      <div className="panel-header log-header">
        <div className="log-title">
          <h2>{t('logbook.title')}</h2>
          <span className="count-badge">{log.length}</span>
          <span className="log-sub">{t('logbook.subtitle')}</span>
        </div>
        <div className="log-actions">
          <input
            ref={fileRef}
            type="file"
            accept=".adi,.adif,text/plain"
            style={{ display: 'none' }}
            onChange={onImportFile}
          />
          <button type="button" className="export-btn" onClick={() => fileRef.current?.click()}>
            {t('logbook.import.adif.label')}
          </button>
          <input
            ref={syncRef}
            type="file"
            accept=".adi,.adif,text/plain"
            style={{ display: 'none' }}
            onChange={onSyncFile}
          />
          <button
            type="button"
            className="export-btn"
            onClick={() => syncRef.current?.click()}
            title={t('logbook.sync.title')}
          >
            {t('logbook.sync.label')}
          </button>
          <input
            ref={potaRef}
            type="file"
            accept=".adi,.adif,.txt"
            style={{ display: 'none' }}
            onChange={onPotaFile}
          />
          <button
            type="button"
            className="export-btn"
            onClick={() => potaRef.current?.click()}
            title={t('logbook.pota.title')}
          >
            {t('logbook.pota.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            onClick={async () => {
              // The authenticated in-app LoTW download also existed but hid in Settings
              // (same story as QRZ, same day): fetches lotwreport.adi with the keychain
              // credentials + the incremental high-water cursor, merges confirmations.
              const r = await withErrorToast(
                () => downloadLotwReport(),
                t('logbook.fetchLotw.failed'),
              )
              if (r) {
                pushToast(
                  t('logbook.fetchLotw.done', {
                    confirmed: r.newlyConfirmed,
                    credited: r.newlyCredited,
                  }),
                  'success',
                )
                load()
              }
            }}
            title={t('logbook.fetchLotw.title')}
          >
            {t('logbook.fetchLotw.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            onClick={async () => {
              // Two-way QRZ Logbook sync existed but hid in Settings — the operator asked
              // for exactly this and couldn't find it (2026-07-21). Same command, surfaced
              // where log work happens; needs the per-logbook API key from Settings.
              const r = await withErrorToast(() => syncQrz(), t('logbook.qrzSync.failed'))
              if (r) {
                pushToast(
                  t('logbook.qrzSync.done', {
                    // `added` is optional on the shared sync result (0 for every sync but
                    // this one). It was interpolated straight into the old template, so an
                    // absent field printed the word "undefined"; the count is what selects
                    // the plural form now, and it has to be a number.
                    count: r.added ?? 0,
                    confirmed: r.newlyConfirmedAny,
                  }),
                  'success',
                )
                load()
              }
            }}
            title={t('logbook.qrzSync.title')}
          >
            {t('logbook.qrzSync.label')}
          </button>
          {/* Export date range (#98): bounds the ADIF/CSV exports below by UTC QSO date,
              inclusive. Empty = unbounded — no dates at all is the whole log, as before.
              The per-operator export stays deliberately unfiltered: it is the compliance
              path (POTA/FD submissions), and a stray leftover date silently truncating an
              uploaded log is the worse failure. */}
          <label className="log-export-range" title={t('logbook.export.from.title')}>
            <span>{t('logbook.export.from.label')}</span>
            <input
              type="date"
              className="settings-input log-export-date"
              value={exportFrom}
              onChange={(e) => setExportFrom(e.target.value)}
            />
          </label>
          <label className="log-export-range" title={t('logbook.export.to.title')}>
            <span>{t('logbook.export.to.label')}</span>
            <input
              type="date"
              className="settings-input log-export-date"
              value={exportTo}
              onChange={(e) => setExportTo(e.target.value)}
            />
          </label>
          <button
            type="button"
            className="export-btn"
            disabled={log.length === 0}
            onClick={() =>
              withErrorToast(async () => {
                const text = await exportGeneralLog('adif', exportFrom, exportTo)
                // Count what the file actually holds — with a date range the log length lies.
                const n = (text.match(/<eor>/gi) ?? []).length
                const stamp = new Date().toISOString().slice(0, 10)
                const path = await saveTextToDownloads(`nexus-log-${stamp}.adi`, text)
                pushToast(t('logbook.export.done', { count: n, path }), 'success')
              }, t('logbook.export.failed'))
            }
            title={
              exportFrom || exportTo
                ? t('logbook.export.adif.titleRange')
                : t('logbook.export.adif.title')
            }
          >
            {t('logbook.export.adif.label')}
          </button>
          {/* Per-operator export (#25). Shown only when the log actually HAS more than one
              operator in it — for the single-op station that is nearly everyone, a button that
              would produce exactly one file identical to Export ADIF is noise. POTA and Field
              Day both require each operator to submit their own log, and these get uploaded
              from a phone in a car park, so the filenames carry the callsign. */}
          {operators.length > 1 && (
            <button
              type="button"
              className="export-btn"
              onClick={() =>
                withErrorToast(async () => {
                  const stamp = new Date().toISOString().slice(0, 10)
                  const saved: string[] = []
                  for (const op of operators) {
                    const text = await exportLogForOperator(op)
                    const safe = op.replace(/[^A-Za-z0-9]+/g, '-')
                    saved.push(await saveTextToDownloads(`nexus-log-${stamp}-${safe}.adi`, text))
                  }
                  // The combined file as well, always: it is the only one that carries contacts
                  // logged with no operator set, and it is what the station itself uploads.
                  const all = await exportGeneralLog('adif')
                  saved.push(await saveTextToDownloads(`nexus-log-${stamp}.adi`, all))
                  pushToast(t('logbook.export.perOperator.done', { count: saved.length }), 'success')
                }, t('logbook.export.failed'))
              }
              title={t('logbook.export.perOperator.title', { operators: operators.join(', ') })}
            >
              {t('logbook.export.perOperator.label')}
            </button>
          )}
          <button
            type="button"
            className="export-btn"
            disabled={log.length === 0}
            onClick={() =>
              withErrorToast(async () => {
                const text = await exportGeneralLog('csv', exportFrom, exportTo)
                // Rows minus the header — with a date range the log length lies.
                const n = Math.max(0, text.trim().split('\n').length - 1)
                const stamp = new Date().toISOString().slice(0, 10)
                const path = await saveTextToDownloads(`nexus-log-${stamp}.csv`, text)
                pushToast(t('logbook.export.done', { count: n, path }), 'success')
              }, t('logbook.export.failed'))
            }
            title={
              exportFrom || exportTo
                ? t('logbook.export.csv.titleRange')
                : t('logbook.export.csv.title')
            }
          >
            {t('logbook.export.csv.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            onClick={onUploadLotw}
            disabled={uploading || unsentLotw === 0}
            title={
              // A second statement with its own count, appended — see the catalog note on the
              // import toast for why it is not one message.
              t('logbook.lotw.upload.title') +
              (timelessLotw ? t('logbook.lotw.upload.timeless', { count: timelessLotw }) : '')
            }
          >
            {uploading
              ? t('logbook.lotw.upload.busy')
              : unsentLotw
                ? t('logbook.lotw.upload.labelCount', { count: unsentLotw })
                : t('logbook.lotw.upload.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            onClick={() => setShowMarkLotw(true)}
            disabled={unsentLotw === 0}
            title={t('logbook.markLotw.title')}
          >
            {t('logbook.markLotw.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            onClick={() => (showForm ? cancelForm() : setShowForm(true))}
          >
            {showForm ? t('logbook.form.close') : t('logbook.form.open')}
          </button>
          <button
            type="button"
            className="export-btn danger"
            onClick={() => setShowPurge(true)}
            disabled={log.length === 0}
            title={t('logbook.purge.title')}
          >
            {t('logbook.purge.label')}
          </button>
        </div>
      </div>


      {showForm && (
        <form className="logbook-form" onSubmit={submit}>
          <div className="logbook-form-grid">
            <label className="logbook-field">
              <span>{t('logbook.field.call.label')}</span>
              <div className="settings-input-row">
                <input
                  className="settings-input"
                  value={draft.call}
                  onChange={(e) => setField('call', e.target.value)}
                  placeholder={LOG_EXAMPLES.call}
                  autoComplete="off"
                  spellCheck={false}
                />
                <button
                  type="button"
                  className="settings-refresh"
                  onClick={onQrzLookup}
                  disabled={qrzBusy || !draft.call.trim()}
                  title={t('logbook.field.qrz.title')}
                >
                  {qrzBusy ? '…' : QRZ_LABEL}
                </button>
              </div>
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.grid.label')}</span>
              <input className="settings-input" value={draft.grid} onChange={(e) => setField('grid', e.target.value)} placeholder={LOG_EXAMPLES.grid} autoComplete="off" spellCheck={false} />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.band.label')}</span>
              <input className="settings-input" value={draft.band} onChange={(e) => setField('band', e.target.value)} placeholder={LOG_EXAMPLES.band} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.freq.label')}</span>
              <input className="settings-input" type="number" step="0.0001" value={draft.freq} onChange={(e) => setField('freq', e.target.value)} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.mode.label')}</span>
              <input className="settings-input" value={draft.mode} onChange={(e) => setField('mode', e.target.value)} placeholder={LOG_EXAMPLES.mode} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.rstSent.label')}</span>
              <input className="settings-input" value={draft.rstSent} onChange={(e) => setField('rstSent', e.target.value)} placeholder={LOG_EXAMPLES.rstSent} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.rstRcvd.label')}</span>
              <input className="settings-input" value={draft.rstRcvd} onChange={(e) => setField('rstRcvd', e.target.value)} placeholder={LOG_EXAMPLES.rstRcvd} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.when.label')}</span>
              <input
                className="settings-input"
                type="datetime-local"
                value={draft.whenUtc}
                onChange={(e) => setField('whenUtc', e.target.value)}
                title={t('logbook.field.when.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.state.label')}</span>
              <input
                className="settings-input"
                value={draft.state}
                onChange={(e) => setField('state', e.target.value)}
                placeholder={LOG_EXAMPLES.state}
                maxLength={2}
                autoComplete="off"
                title={t('logbook.field.state.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.txPower.label')}</span>
              <input
                className="settings-input"
                type="number"
                min="0"
                value={draft.txPower}
                onChange={(e) => setField('txPower', e.target.value)}
                placeholder={LOG_EXAMPLES.txPower}
                autoComplete="off"
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.parkTheirs.label')}</span>
              <input
                className="settings-input"
                value={draft.parkTheirRef}
                onChange={(e) => setField('parkTheirRef', e.target.value)}
                placeholder={LOG_EXAMPLES.parkTheirRef}
                autoComplete="off"
                title={t('logbook.field.parkTheirs.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.parkMine.label')}</span>
              <input
                className="settings-input"
                value={draft.parkMyRef}
                onChange={(e) => setField('parkMyRef', e.target.value)}
                placeholder={LOG_EXAMPLES.parkMyRef}
                autoComplete="off"
                title={t('logbook.field.parkMine.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.name.label')}</span>
              <input className="settings-input" value={draft.name} onChange={(e) => setField('name', e.target.value)} placeholder={t('logbook.field.name.placeholder')} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.qth.label')}</span>
              <input className="settings-input" value={draft.qth} onChange={(e) => setField('qth', e.target.value)} placeholder={t('logbook.field.qth.placeholder')} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.comment.label')}</span>
              <input className="settings-input" value={draft.comment} onChange={(e) => setField('comment', e.target.value)} placeholder={t('logbook.field.comment.placeholder')} autoComplete="off" />
            </label>
            <label className="logbook-field logbook-field-wide">
              <span>{t('logbook.field.notes.label')}</span>
              <textarea
                className="settings-input logbook-notes"
                value={draft.notes}
                onChange={(e) => setField('notes', e.target.value)}
                placeholder={t('logbook.field.notes.placeholder')}
                rows={3}
              />
            </label>
          </div>
          <div className="logbook-form-actions">
            {err && <span className="settings-error" role="alert">{err}</span>}
            {editIndex !== null && (
              <span className="logbook-editing-note">{t('logbook.form.editingNote')}</span>
            )}
            <button type="submit" className="settings-save" disabled={!draft.call.trim()}>
              {editIndex !== null ? t('logbook.form.save') : t('logbook.form.log')}
            </button>
          </div>
        </form>
      )}

      <div className="log-table logbook-table" role="table">
        <div className="log-scroll" ref={scrollRef}>
          {globeShown && (
            <div className="log-globe-band">
              <Suspense fallback={<div className="log-globe-loading">{t('logbook.globe.loading')}</div>}>
                <QsoGlobe qsos={log} />
              </Suspense>
            </div>
          )}
          {/* Search + column headers BELOW the globe (operator 2026-07-21) — sticky, so
              once the globe scrolls away they pin to the top and the table reads normally. */}
          <div className="log-sticky">
      <div className="log-searchbar">
        <input
          className="settings-input log-search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t('logbook.search.placeholder')}
          autoComplete="off"
          spellCheck={false}
        />
        <button
          type="button"
          className={`log-filter-chip${needsConfirmOnly ? ' active' : ''}`}
          onClick={() => setNeedsConfirmOnly((v) => !v)}
          aria-pressed={needsConfirmOnly}
          title={t('logbook.filter.needsConfirmation.title')}
        >
          {t('logbook.filter.needsConfirmation.label')}
        </button>
        {search.trim() && (
          <button type="button" className="log-search-clear" onClick={() => setSearch('')} title={t('logbook.search.clear')}>
            ✕
          </button>
        )}
      </div>
        <div className="log-row logbook-row head" role="row">
          {th(t('logbook.column.call'), 'call')}
          {th(t('logbook.column.country'), 'country')}
          {th(t('logbook.column.band'), 'band')}
          {th(t('logbook.column.freq'), 'freq')}
          {th(t('logbook.column.mode'), 'mode')}
          {th(t('logbook.column.sent'), 'sent')}
          {th(t('logbook.column.rcvd'), 'rcvd')}
          {th(t('logbook.column.time'), 'time')}
          {th(t('logbook.column.park'), 'park')}
          {/* The QSL column's header is the Q-code itself, not a word for it. */}
          {th('QSL', 'qsl')}
          <span className="log-cell" role="columnheader" aria-label={t('logbook.column.actions')}></span>
        </div>
          </div>
          {log.length === 0 && <p className="empty">{t('logbook.empty')}</p>}
          {log.length > 0 && rows.length === 0 && (
            <p className="empty">{t('logbook.emptySearch', { query: deferredSearch.trim() })}</p>
          )}
          {rows.length > 0 && (
            <div
              ref={rowsWrapRef}
              className="log-rows"
              style={{ height: rowVirtualizer.getTotalSize(), position: 'relative', width: '100%' }}
            >
              {rowVirtualizer.getVirtualItems().map((vrow) => {
                const { q, i } = rows[vrow.index]
                return (
                  <div
                    className={`log-row logbook-row${editIndex === i ? ' editing' : ''}`}
                    role="row"
                    // The backend index `i` is unique per record → collision-proof even for two
                    // identical QSOs (double-clicked Log in the same second). Rows are stateless
                    // divs, so key churn after a delete-shift costs nothing.
                    key={`${q.call}-${q.whenUnix}-${i}`}
                    data-index={vrow.index}
                    ref={rowVirtualizer.measureElement}
                    style={{
                      position: 'absolute',
                      top: 0,
                      left: 0,
                      width: '100%',
                      // scrollMargin is baked into vrow.start (the globe band above);
                      // subtract it because rows are positioned within THIS container,
                      // which document flow already places below the band.
                      transform: `translateY(${vrow.start - rowVirtualizer.options.scrollMargin}px)`,
                      // Stripe by REAL index (inline beats the nth-child rule, which would otherwise
                      // stripe by render order and appear to "move" as the virtual window scrolls).
                      background: vrow.index % 2 ? 'color-mix(in srgb, var(--bg-elev) 50%, transparent)' : 'transparent',
                    }}
                  >
                <span className="log-cell mono">
                  <button
                    type="button"
                    className="qrz-link-call"
                    onClick={() =>
                      void withErrorToast(
                        () => openQrzPage(q.call),
                        t('callbook.qrzPage.failed', { call: q.call }),
                      )
                    }
                    title={t('callbook.qrzPage.title', { call: q.call })}
                  >
                    {q.call}
                  </button>
                </span>
                <span className="log-cell log-country" title={q.country ?? ''}>{q.country ?? '—'}</span>
                <span className="log-cell">{q.band}</span>
                <span className="log-cell mono">{q.freqMhz.toFixed(4)}</span>
                <span className="log-cell">{q.mode}</span>
                <span className="log-cell mono">{fmtReport(q.rstSent)}</span>
                <span className="log-cell mono">{fmtReport(q.rstRcvd)}</span>
                <span className="log-cell mono">{fmtUtc(q.whenUnix)}</span>
                <span
                  className="log-cell mono log-park"
                  title={
                    q.ota?.theirRef
                      ? t('logbook.row.park.worked', {
                          program: q.ota.theirProgram ?? 'POTA',
                          ref: q.ota.theirRef,
                        })
                      : q.ota?.myRef
                        ? t('logbook.row.park.mine', {
                            program: q.ota.myProgram ?? 'POTA',
                            ref: q.ota.myRef,
                          })
                        : ''
                  }
                >
                  {q.ota?.theirRef ?? (q.ota?.myRef ? `@${q.ota.myRef}` : '—')}
                </span>
                <span className="log-cell">
                  {q.qslRcvd && (q.qslRcvd.card || q.qslRcvd.lotw || q.qslRcvd.eqsl) ? (
                    // Per-source detail: which channel(s) actually confirmed.
                    <span
                      className={`log-qsl ${q.awardConfirmed ? 'ok' : 'eqsl'}`}
                      title={[
                        q.qslRcvd.lotw ? t('logbook.row.qsl.lotw') : null,
                        q.qslRcvd.card ? t('logbook.row.qsl.card') : null,
                        q.qslRcvd.eqsl ? t('logbook.row.qsl.eqsl') : null,
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    >
                      {[
                        q.qslRcvd.lotw ? 'L' : null,
                        q.qslRcvd.card ? 'C' : null,
                        q.qslRcvd.eqsl ? 'E' : null,
                      ]
                        .filter(Boolean)
                        .join('·')}
                    </span>
                  ) : q.awardConfirmed ? (
                    <span className="log-qsl ok" title={t('logbook.row.qsl.confirmed')}>
                      ✓
                    </span>
                  ) : q.confirmed ? (
                    <span className="log-qsl eqsl" title={t('logbook.row.qsl.eqslOnly')}>
                      {EQSL_LABEL}
                    </span>
                  ) : (
                    <span className="log-qsl none" title={t('logbook.row.qsl.none')}>
                      —
                    </span>
                  )}
                  {/* A request is NOT a confirmation — this rides alongside the QSL
                      state as a quiet muted marker so the row stays "needs conf". */}
                  {q.qslSent?.sent && (
                    <span
                      style={{ marginLeft: 4, opacity: 0.6, fontSize: '0.85em' }}
                      title={fmtQslSent(q.qslSent)}
                      aria-label={fmtQslSent(q.qslSent)}
                    >
                      ✉{q.qslSent.via ?? ''}
                    </span>
                  )}
                </span>
                <span className="log-cell log-rowactions">
                  <button
                    type="button"
                    className="log-rowbtn"
                    onClick={() => setSpotSeed({ call: q.call, freq: q.freqMhz, mode: q.mode })}
                    title={t('logbook.row.spot.title', { call: q.call })}
                    aria-label={t('logbook.row.spot.aria', { call: q.call })}
                  >
                    📢
                  </button>
                  <button
                    type="button"
                    className="log-rowbtn"
                    onClick={() => void onPushQrz(q)}
                    title={t('logbook.row.pushQrz.title', { call: q.call })}
                    aria-label={t('logbook.row.pushQrz.aria', { call: q.call })}
                  >
                    ↥
                  </button>
                  <button
                    type="button"
                    className="log-rowbtn"
                    onClick={() => void onPushClublog(q)}
                    title={t('logbook.row.pushClublog.title', { call: q.call })}
                    aria-label={t('logbook.row.pushClublog.aria', { call: q.call })}
                  >
                    {CLUBLOG_LABEL}
                  </button>
                  <button
                    type="button"
                    className="log-rowbtn"
                    onClick={() => void onPushHrdlog(q)}
                    title={t('logbook.row.pushHrdlog.title', { call: q.call })}
                    aria-label={t('logbook.row.pushHrdlog.aria', { call: q.call })}
                  >
                    {HRDLOG_LABEL}
                  </button>
                  {/* QSL-request queue: mark a card/request sent (once) on the
                      needs-confirmation view. Operator-declared, not a confirmation. */}
                  {needsConfirmOnly && !q.qslSent?.sent && (
                    <select
                      className="log-rowbtn"
                      style={{ fontSize: '0.85em' }}
                      value=""
                      onChange={(e) => {
                        const v = e.target.value as 'B' | 'D' | 'E' | ''
                        if (v) void onMarkQslSent(q, i, v)
                      }}
                      title={t('logbook.row.qslSent.title', { call: q.call })}
                      aria-label={t('logbook.row.qslSent.aria', { call: q.call })}
                    >
                      {/* The VALUES are the ADIF QSL_SENT_VIA letters; only the labels are prose. */}
                      <option value="">{QSL_MENU_LABEL}</option>
                      <option value="B">{t('logbook.row.qslSent.bureau')}</option>
                      <option value="D">{t('logbook.row.qslSent.direct')}</option>
                      <option value="E">{t('logbook.row.qslSent.electronic')}</option>
                    </select>
                  )}
                  <button
                    type="button"
                    className="log-rowbtn"
                    onClick={() => startEdit(q, i)}
                    title={t('logbook.row.edit', { call: q.call })}
                    aria-label={t('logbook.row.edit', { call: q.call })}
                  >
                    ✎
                  </button>
                  <button
                    type="button"
                    className="log-rowbtn danger"
                    onClick={() => onDelete(q, i)}
                    title={t('logbook.row.delete', { call: q.call })}
                    aria-label={t('logbook.row.delete', { call: q.call })}
                  >
                    ✕
                  </button>
                </span>
              </div>
                )
              })}
            </div>
          )}
        </div>
      </div>

      {showPurge && (
        <div
          className="logconfirm-backdrop"
          role="dialog"
          aria-modal="true"
          aria-label={t('logbook.purge.aria')}
          onClick={closePurge}
        >
          <div className="logconfirm purge-confirm" onClick={(e) => e.stopPropagation()}>
            <div className="logconfirm-head">
              <h2>{t('logbook.purge.heading')}</h2>
              <span className="logconfirm-sub danger">{t('logbook.purge.irreversible')}</span>
            </div>
            <p className="purge-warn">
              <T k="logbook.purge.warn" tags={{ b: <strong /> }} vals={{ count: log.length }} />
            </p>
            {/* The sync cursors are the non-obvious half of a purge, and getting it wrong cost an
                operator his whole confirmation history: each cursor means "I already hold every
                confirmation matched up to this date", which is a lie about an empty log. Purging
                clears them so the next pull is the full one the empty log needs — but that pull is
                far larger than a routine sync, and an operator who is not told that reads the wait
                as a hang. Says what happens, not what used to go wrong. */}
            <p className="purge-warn">
              <T k="logbook.purge.syncWarn" tags={{ b: <strong /> }} />
            </p>
            <label className="purge-field">
              <span>
                <T k="logbook.purge.typeWord" tags={{ b: <strong /> }} vals={{ word: PURGE_WORD }} />
              </span>
              <input
                className="settings-input mono"
                value={purgeText}
                autoFocus
                onChange={(e) => setPurgeText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && purgeText.trim().toUpperCase() === PURGE_WORD) void onPurge()
                  if (e.key === 'Escape') closePurge()
                }}
                placeholder={PURGE_WORD}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
            <div className="logconfirm-actions">
              <button type="button" className="logconfirm-discard" onClick={closePurge}>
                {t('logbook.purge.cancel')}
              </button>
              <button
                type="button"
                className="logconfirm-log danger"
                onClick={onPurge}
                disabled={purging || purgeText.trim().toUpperCase() !== PURGE_WORD}
              >
                {purging ? t('logbook.purge.busy') : t('logbook.purge.confirm', { count: log.length })}
              </button>
            </div>
          </div>
        </div>
      )}

      {showMarkLotw && (
        <div
          className="logconfirm-backdrop"
          role="dialog"
          aria-modal="true"
          aria-label={t('logbook.markLotw.aria')}
          onClick={() => setShowMarkLotw(false)}
        >
          <div className="logconfirm" onClick={(e) => e.stopPropagation()}>
            <div className="logconfirm-head">
              <h2>
                {t('logbook.markLotw.heading', {
                  count: unsentLotw,
                  formatted: unsentLotw.toLocaleString(),
                })}
              </h2>
            </div>
            <p className="purge-warn">
              <T
                k="logbook.markLotw.body"
                tags={{ b: <strong /> }}
                vals={{ count: unsentLotw, formatted: unsentLotw.toLocaleString() }}
              />
            </p>
            <div className="logconfirm-actions">
              <button
                type="button"
                className="logconfirm-discard"
                onClick={() => setShowMarkLotw(false)}
              >
                {t('logbook.markLotw.cancel')}
              </button>
              <button type="button" className="logconfirm-log" onClick={onMarkLotwUploaded}>
                {t('logbook.markLotw.confirm', { formatted: unsentLotw.toLocaleString() })}
              </button>
            </div>
          </div>
        </div>
      )}
      <SpotDialog
        open={spotSeed != null}
        onClose={() => setSpotSeed(null)}
        initialCall={spotSeed?.call ?? ''}
        freqMhz={spotSeed?.freq ?? 0}
        defaultComment={spotSeed?.mode ?? ''}
      />
    </section>
  )
}
