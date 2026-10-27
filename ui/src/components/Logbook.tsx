// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog (`i18n/en.ts`); a hardcoded one fails CI. What does NOT come from
// the catalog: the four service/Q-code labels below, and every value the table
// prints — callsign, band, mode, frequency, RST, park reference, QSL letters. Those are wire
// formats, identical in every language. See the invariant-token rule in `i18n/index.ts`.

import { Fragment, lazy, Suspense, useCallback, useDeferredValue, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { confirmDialog } from '../confirm'
import { oneLineTyped } from '../typedInput'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { Activation, ImportStats, LogExport, LoggedActivation, LoggedQso } from '../types'
import { ContactCard, type CardContact } from './ContactCard'
import { RecentlyDeleted } from './RecentlyDeleted'
import { LogTools, type LogTool } from './LogTools'
import { useCallbookUpdateEnd } from './LogCallbookUpdate'
import { sayUndone } from './QsoHistory'
import { gpuCapableForGlobe } from '../gpu'
import { GlobeGuard } from './GlobeGuard'
import { useLogbookGlobe } from '../features/logbookGlobe'
import { PaneSeam } from './PaneSeam'
import { LOG_GLOBE_SPLIT_MAX, LOG_GLOBE_SPLIT_MIN } from '../features/paneSeam'
import { emptyAnswer, type LogLocate, type LogPage, type LogQuestion } from '../features/logAnswers'
import { DEFAULT_LOG_QUERY, defaultAsc, fmtUtc, logQueryKey, type LogQuery, type LogSortKey } from '../features/logQuery'
import { failureToShow, logSource, useLogAnswer, useLogAnswers, useLogPages, useLogStatus } from '../features/logSource'
import { LOTW_SKIP_TOAST_MS, lotwSkipNote } from '../features/lotwSkips'
import { sayExportLacks } from '../features/exportLacks'
import { UTC_DATE_FORMAT, UTC_TIME_FORMATS, parseUtcDate, parseUtcTime, utcDate, utcDateTimeToUnix, utcTime } from '../features/utcLog'
import { SpotDialog } from './SpotDialog'
import { ImportPreview } from './ImportPreview'
import { importTooLarge } from '../features/logImport'
import { ParkStateReview } from './ParkStateReview'
import { ConfirmationReview } from './ConfirmationReview'
import { LogCountryPicker } from './LogCountryPicker'
import { LogColumnsMenu } from './LogColumnsMenu'
import { LogFilterBar } from './LogFilterBar'
import { clickSort, filterKey, sortPlace, type LogFilter, type LogSort } from '../features/logFilter'
import { activationFilter } from '../features/potaActivation'
import { useViewportClass, useViewportSize } from '../useViewport'
import {
  addedColumnText,
  clampLogColumns,
  columnName,
  DEFAULT_LOG_COLUMNS,
  forgetLegacyMoreColumns,
  loadLegacyMoreColumns,
  legacyLogColumns,
  LOG_COLUMNS,
  logGridTemplate,
  logTableMinWidth,
  renderedColumns,
  uploadMark,
  uploadOf,
  uploadWord,
  withColumnWidth,
  type LogColumn,
  type LogColumnsSaved,
  type UploadMark,
} from '../features/logColumns'
import { POTA_MIN_QSOS, activationKeyOf, exportActivation } from '../features/activationExport'

// The 3-D QSO globe band. Lazy so three.js/react-globe.gl only download when the
// Logbook actually shows it (same pattern as ConnectView's Globe3D) — a weak-GPU
// machine that fails `gpuCapableForGlobe` never pays for the chunk at all, nor does
// one without the WebGL 2 context three.js needs (GlobeGuard).
const QsoGlobe = lazy(() => import('./QsoGlobe'))

import {
  deleteQsoById,
  editQsoById,
  exportGeneralLog,
  exportSelectedLog,
  exportLogForOperator,
  logOperators,
  logActivations,
  restoreDeletedQso,
  undoHistoryRun,
  logQso,
  markLotwUploaded,
  markQslSentById,
  markQslCardById,
  setSatTagById,
  setDxccById,
  lotwSatNames,
  purgeLog,
  qrzLookup,
  saveTextToDownloads,
  syncLotwReport,
  uploadLotwReport,
  downloadEqslReport,
  getSettings,
} from '../api'
import { failureText, pushToast, withErrorToast } from '../toast'
import { qrzPushQso, clublogPushQso, hrdlogPushQso, wrlPushQso, syncQrz, downloadLotwReport, importPotaLog } from '../api'
import { qrzCorrectPreview, qrzCorrectApply, qrzCorrectUndoMiss } from '../api'
import { callCardTrigger, qrzPageOf } from './CallCard'
import type { FieldEdit, QrzCorrectResult, QsoEdit, RowAnswer, RowRef } from '../api'

interface Props {
  /** Default band / freq / mode for new manual entries (from the radio). */
  defaultBand: string
  defaultFreqMhz: number
  defaultMode: string
  /** Cross-view handoff from a cockpit's recall card (#192): open with this callsign already
   *  in the search box. `ts` is a nonce, not data — it is what makes a re-click of the SAME
   *  call refire after the operator has typed over the box (the `pendingWork` idiom). `editId`, when
   *  the handoff came from one contact (a recall card's row, Awards, a cockpit's recent contacts),
   *  also opens THIS view's form on that contact: the one editor, saving through the one write path. */
  focusCall?: { call: string; ts: number; editId?: string | null } | null
  /** Called once the handoff has been applied, so the parent can clear it — otherwise a later
   *  trip to the Logbook through the nav would re-apply a filter nobody asked for. */
  onConsumeFocusCall?: () => void
  /** The engine's `logTick` (desktop only): the list reloads when it moves, so a Remote
   *  browser's delete or edit reaches this view instead of leaving it on rows that have moved. */
  logTick?: number
  /** The saved column choice (`settings.json` → `logbookColumns`); absent = nothing saved yet. */
  columns?: LogColumnsSaved | null
  /** A changed column choice, to save — `null` forgets it (the default table). */
  onColumnsChange?: (columns: LogColumnsSaved | null) => void
  /** The activation running now, off the snapshot: while there is one, "This activation" filters
   *  the list to its contacts (features/potaActivation `activationFilter`). */
  activation?: Activation | null
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
  /** When the contact happened, in UTC: the date (`YYYY-MM-DD`) and a 24-hour time (`HH:MM` or
   * `HH:MM:SS`, features/utcLog). THE fields that make hand-logging work: you log a 2 m contact
   * after the fact, so stamping "now" writes the wrong time into the log and into every upload
   * downstream. The time is a text box, not a native control, because WebView2 draws those in
   * the OS locale — 00:58 read "12:58 AM" on a 12-hour PC, and records got "fixed" by 12 hours
   * (#280). Both blank = now (a new contact) or the stored time (an edit). */
  whenDate: string
  whenTime: string
  /** #329: when the contact ENDED (ADIF TIME_OFF), 24-hour UTC, on the contact's own date —
   * one box, because an end time on a different day is the rollover case and `submit` reads
   * it as one. The field was written at log time and exported all along, but nothing here
   * showed it, so a wrong end time (Ham Radio Deluxe's 00:00) could not even be seen.
   *
   * ⚠️ Blank means LEAVE ALONE, not "clear it": an omitted end time is restored from the
   * stored record by `Logbook::update_record`, so clearing one is deliberately not offered. */
  endTime: string
  /** US state (WAS). The auto-log path fills this from the callsign/grid; a hand-logged
   * contact has no decode to derive it from, so it has to be typeable or WAS silently misses. */
  state: string
  /** TX power in watts (ADIF TX_PWR) — part of a complete hand-log, and some awards want it. */
  txPower: string
  /** POTA/SIG park references: the worked station's park (`ota.theirRef`, the hunter case) and
   * your own activation (`ota.myRef`). Editable so a park can be viewed, corrected or added on
   * an existing QSO (#60). Only these two of the five `Ota` fields are exposed; the loaded
   * program (POTA/SOTA/WWFF) rides through `submit` from the stored record so an edit never
   * clobbers a non-POTA program. */
  parkTheirRef: string
  parkMyRef: string
  /** The worked station's IOTA reference ("NA-001"). Blank on an edit leaves the stored one, as
   * the park boxes do; anything else must be a reference (`IOTA_RE`). */
  iota: string
  /** #239: your own grid square and rig for this contact (ADIF MY_GRIDSQUARE / MY_RIG). */
  myGrid: string
  myRig: string
  /** #239: QSL sent via '' | 'B' | 'D' | 'E', or 'SENT' for a stored "sent" with no route; and
   * a paper card received ('1' | ''). Strings, like every draft field, so `setField` serves them. */
  qslSentVia: string
  qslCard: string
}

/** The word the operator must type to arm the full-log purge. It is COMPARED
 * against what they type, so it is a token and not prose — translating it would move the gate. */
const PURGE_WORD = 'DELETE'

/** One contact by its id, with the edit key a change to it must send: read as the list reads its rows,
 *  a page of the order the Logbook opens on. The order can move between the two questions (a contact
 *  logged meanwhile), so a page whose row is not this one is asked again; null when the log no longer
 *  holds it. */
async function keyedRow(id: string): Promise<{ row: LoggedQso; editKey: string } | null> {
  for (let attempt = 0; attempt < 3; attempt++) {
    const at = await logSource().ask({ kind: 'locate', query: DEFAULT_LOG_QUERY, id })
    if (at.index === null) return null
    const page = await logSource().ask({ kind: 'page', query: DEFAULT_LOG_QUERY, offset: at.index, limit: 1 })
    if (page.keys[0] === id && page.rows[0]) return { row: page.rows[0], editKey: page.editKeys[0] }
  }
  return null
}

/** Q-codes and service names printed as labels. Proper nouns and shorthand, not words. */
const QRZ_LABEL = 'QRZ'
/** The row's "correct at QRZ" mark — the service name plus a pencil, not prose. */
const QRZ_CORRECT_LABEL = 'QRZ✎'
const EQSL_LABEL = 'eQSL'
const CLUBLOG_LABEL = 'CL'
const HRDLOG_LABEL = 'HL'
// Technical product token, not prose — same ruling as the labels above.
const WRL_LABEL = 'WRL'
const QSL_MENU_LABEL = 'QSL▸'
/** The satellite tag menu. `SAT` is the ADIF `PROP_MODE` value itself, not a word. */
const SAT_MENU_LABEL = 'SAT▸'
/** The QSL column's header is the Q-code itself, not a word for it. */
const QSL_HEADER = 'QSL'
/** An upload column's mark, a glyph per state; its words are in the tooltip. */
const UPLOAD_MARK: Record<UploadMark, string> = { done: '✓', waiting: '…', failed: '✗', none: '—' }
/** An IOTA reference as the log stores one (tempo-core `valid_iota`): a continent, a hyphen and
 *  three digits. */
const IOTA_RE = /^(AF|AN|AS|EU|NA|OC|SA)-\d{3}$/
/** The added columns the edit form has a box for (so its read-only line leaves them out). */
const FORM_EDITS: ReadonlySet<string> = new Set(['grid', 'iota'])
/** One Alt+←/→ on a column header: a column's width moves this far. */
const RESIZE_STEP_PX = 16

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
/** The entities whose contacts take a US state or Canadian province, as cty.dat (and the
 *  callbook) name them. */
const STATE_ENTITIES: ReadonlySet<string> = new Set(['United States', 'Alaska', 'Hawaii', 'Canada'])

/** A contact with a hunted park or summit and no state, where a state applies (or the log cannot
 *  say whether one does): it counts for no state until the operator sets the one the activator was
 *  in. A park on a state line is logged this way on purpose, never guessed, until it is picked. */
function needsParkState(q: LoggedQso): boolean {
  return !!q.ota?.theirRef && !(q.state ?? '').trim() && (q.country == null || STATE_ENTITIES.has(q.country))
}

function parseReport(s: string): string | null {
  const t = s.trim()
  return t === '' ? null : t
}

// The columns' sort and the search box's filter are `features/logQuery` — the ONE copy the list's
// source runs, so what a page holds is exactly what this view used to sort and filter itself.
type SortKey = LogSortKey

/** Rows per Logbook page: what the list asks its source for at a time (SPEC-2 v2 §6). */
const LOG_PAGE = 128
/** A row's height before it is measured: its 22 px buttons plus padding. */
const ROW_ESTIMATE = 43
/** Rows the list renders beyond the viewport on each side (the virtualizer's `overscan`). */
const OVERSCAN = 12
/** Pages the list keeps of the orders it has read — the order on screen, and one coming. */
const HELD_PAGES = 64
/** The Logbook's LoTW-backlog question — the same one every render. */
const LOTW_BACKLOG = { kind: 'lotwBacklog' } as const satisfies LogQuestion
/** How many contacts the log holds: the count badge, the purge warning, the exports' gates. */
const LOG_SIZE = { kind: 'logSize' } as const satisfies LogQuestion


export function Logbook({
  defaultBand,
  defaultFreqMhz,
  defaultMode,
  focusCall,
  onConsumeFocusCall,
  logTick,
  columns,
  onColumnsChange,
  activation,
}: Props) {
  // The desktop asks `LogSource` for the pages of the list it shows (below, after the sort and
  // search state they are cut by). The source follows `logTick`, so the list moves when the log
  // changes under this view — a Remote browser's delete or edit, another instance's contact, a
  // connector stamp. The rows are addressed by key, so a stale list is refused rather than acted
  // on; following the tick is what keeps that refusal rare.
  // The whole log's size, not the list's: the count badge, the purge warning, the exports and the
  // globe key on it. The engine's answer is `undefined` until it lands, and for as long as the engine refuses the
  // question while a change is being saved (features/notAnswered): the gates count that as 0, with
  // nothing to export or purge yet, but only an ANSWERED 0 is an empty log.
  const askedLogSize = useLogAnswer(LOG_SIZE, logTick)
  const logSize = askedLogSize ?? emptyAnswer(LOG_SIZE)
  // Where that count stands. One held from before the latest change is out of date until the fresh
  // one lands, and is shown as such: a held 0 never as a count or an empty log.
  const sizeStatus = useLogStatus(LOG_SIZE)
  const sizeCurrent = sizeStatus === undefined || sizeStatus.state === 'current'
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
    whenDate: '',
    whenTime: '',
    endTime: '',
    state: '',
    txPower: '',
    parkTheirRef: '',
    parkMyRef: '',
    iota: '',
    myGrid: '',
    myRig: '',
    qslSentVia: '',
    qslCard: '',
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
    if (logSize === 0) {
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
  }, [logSize])
  // The satellites LoTW accepts, for the row's tag picker. A fixed backend table, so it is
  // fetched once rather than per log change — and it is a PICKER because TQSL matches
  // SAT_NAME against its own list and rejects anything else, which through the compliant
  // upload funnel takes the whole signed batch with it. A typed name is a permanent guess.
  const [satNames, setSatNames] = useState<string[]>([])
  useEffect(() => {
    void lotwSatNames()
      .then(setSatNames)
      .catch(() => {}) // no bridge / older core — just don't offer the picker
  }, [])
  // Activations present in the log: (your park) × (UTC day) × (the callsign you signed). Same
  // shape as the operator list above, and loaded the same way — a new activation can only first
  // appear when the log grows.
  const [activations, setActivations] = useState<LoggedActivation[]>([])
  const [activationKey, setActivationKey] = useState('')
  useEffect(() => {
    if (logSize === 0) {
      setActivations((prev) => (prev.length === 0 ? prev : []))
      return
    }
    void logActivations()
      .then((next) =>
        setActivations((prev) =>
          prev.length === next.length && prev.every((v, i) => activationKeyOf(v) === activationKeyOf(next[i]) && v.qsos === next[i].qsos)
            ? prev
            : next,
        ),
      )
      .catch(() => {}) // no bridge / older core — just don't offer the per-activation export
  }, [logSize])
  const [qrzBusy, setQrzBusy] = useState(false)
  const [uploading, setUploading] = useState(false)
  const [search, setSearch] = useState('')
  // Filtering runs against a DEFERRED copy of the search so typing stays responsive on a 10k log —
  // the input updates instantly; the (memoized) filter/sort catches up a frame later.
  const deferredSearch = useDeferredValue(search)
  // A previous-contact row in a cockpit's recall card hands the callsign over here (#192,
  // kr4fqg). Seeding the SEARCH box, not a row selection: a `LoggedQso` has no stable id and
  // the edit/delete API addresses rows by index, so an index carried across a view switch is
  // stale by construction. The matcher below already covers `call`, so this needs no new
  // filtering path — and because it lands in the visible box, the operator can see what is
  // being filtered and clear it with the ✕ that is already there.
  useEffect(() => {
    if (!focusCall) return
    setSearch(focusCall.call)
    onConsumeFocusCall?.()
  }, [focusCall, onConsumeFocusCall])
  // Filter to contacts still lacking an award-eligible confirmation (the DX
  // chaser's "who do I still need a card/LoTW from" view).
  const [needsConfirmOnly, setNeedsConfirmOnly] = useState(false)
  // The field filters (features/logFilter), answered by the engine; deferred as the search is.
  const [filter, setFilter] = useState<LogFilter>({})
  const thisActivation = activationFilter(activation)
  const deferredFilter = useDeferredValue(filter)
  // The rows picked for "Export selected", by contact id, and the row a Shift-click's run starts
  // from. A new search or filter clears them: a picked row the list no longer shows would be
  // exported unseen.
  const [picked, setPicked] = useState<ReadonlySet<string>>(() => new Set())
  const pickAnchor = useRef<number | null>(null)
  useEffect(() => {
    pickAnchor.current = null
    setPicked((was) => (was.size ? new Set() : was))
  }, [deferredSearch, needsConfirmOnly, deferredFilter])
  // THE COLUMNS (features/logColumns.ts). The choice is the one this view just made, else the saved
  // one, else — nothing saved yet — the table this browser had ("More columns", #239). Clamped on
  // every read against the window it is shown on, never only when it was dragged.
  const [madeColumns, setMadeColumns] = useState<LogColumnsSaved | null>(null)
  const viewport = useViewportClass()
  const windowPx = useViewportSize()?.width ?? null
  const narrowWindow = viewport === 'sm' || viewport === 'xs'
  const colChoice = useMemo(
    () => clampLogColumns(madeColumns ?? columns ?? legacyLogColumns(loadLegacyMoreColumns()), windowPx),
    [madeColumns, columns, windowPx],
  )
  const cols = useMemo(() => renderedColumns(colChoice, narrowWindow), [colChoice, narrowWindow])
  const gridTemplate = logGridTemplate(cols, colChoice, narrowWindow)
  const tableMinW = logTableMinWidth(cols, colChoice)
  const changeColumns = (next: LogColumnsSaved) => {
    setMadeColumns(next)
    onColumnsChange?.(next)
  }
  const resetColumns = () => {
    // The default table, and the browser's old switch with it, or it would bring "More columns"
    // back the next time nothing is saved.
    forgetLegacyMoreColumns()
    setMadeColumns(DEFAULT_LOG_COLUMNS)
    onColumnsChange?.(null)
  }
  // "Check park states": the operator-run review of hunted contacts whose park names another
  // state (ParkStateReview). Opened only by its button; it changes nothing until Apply.
  const [showParkStates, setShowParkStates] = useState(false)
  // "Check confirmations": the operator-run review of LoTW confirmations and upload marks that LoTW's
  // own records give another contact (ConfirmationReview). Opened only by its button; it downloads
  // nothing until Check, and changes nothing until Apply.
  const [showConfirmations, setShowConfirmations] = useState(false)
  // Purge-the-whole-log confirmation modal. `purgeText` must equal PURGE_WORD to
  // arm the danger button — a deliberate, typed gate for a wipe of the whole log.
  const [showPurge, setShowPurge] = useState(false)
  const [showDeleted, setShowDeleted] = useState(false)
  // The clean-up tools (LogTools.tsx): which one the dialog shows, while it is open.
  const [tools, setTools] = useState<LogTool | null>(null)
  const [purgeText, setPurgeText] = useState('')
  const [purging, setPurging] = useState(false)
  // "Mark all as already on LoTW" confirmation — for an imported legacy log that was
  // uploaded through another tool, so the unsent count reflects reality.
  const [showMarkLotw, setShowMarkLotw] = useState(false)
  // "Correct at QRZ" — one contact, never a selection, never automatic. `phase` is the whole
  // state machine: read QRZ's copy → show exactly what changes → send → report what QRZ said.
  const [qrzFix, setQrzFix] = useState<{
    q: LoggedQso
    phase: 'reading' | 'confirm' | 'sending' | 'done' | 'recovering'
    text: string
    result: QrzCorrectResult | null
  } | null>(null)
  // The row being edited, AS THE OPERATOR OPENED IT — never read back from the list at save time.
  // The list moves under an open form (a Remote delete, a connector stamp), and after a delete
  // above it the same place names a different contact; the target of the edit, and the fields
  // the form did not carry, come from this row. `key` (its id) only marks the row in the list,
  // and follows it wherever the list puts it. `ref` is what the desktop's save sends — the id and
  // the edit key of the version opened, so an edit made meanwhile elsewhere refuses the save
  // rather than being overwritten (null on a Remote browser, which names the row its own way).
  // null = the form logs a NEW QSO.
  const [editing, setEditing] = useState<{ key: string; row: LoggedQso; ref: RowRef | null } | null>(null)
  /** The contact open on the contact card (#313's "there's no View capability", and the card the
   *  operator asked for after it): every field shown and editable, saved through `saveEdit` — the
   *  edit form's one write path — so there is still one writer. */
  const [card, setCard] = useState<CardContact | null>(null)
  const editingKey = editing?.key ?? null
  // Column sort — the order the engine cuts the list's pages in (`features/logQuery`). Default
  // newest-first (the log's own order is oldest-first, which the test user disliked).
  // Re-spot a logged contact to the cluster (row 📢): seeded with the row's call,
  // frequency and mode (operator ask 2026-07-21).
  const [spotSeed, setSpotSeed] = useState<{ call: string; freq: number; mode: string } | null>(null)
  // Up to three keys: a header click orders by its column, a Shift-click adds it (features/logFilter).
  const [sort, setSort] = useState<LogSort>({ sort: 'time', asc: false, then: [] })
  // Export date range (#98) — "YYYY-MM-DD" UTC or '' = unbounded. Both empty = whole log.
  const [exportFrom, setExportFrom] = useState('')
  const [exportTo, setExportTo] = useState('')
  const fileRef = useRef<HTMLInputElement>(null)
  // The file the import preview is open on.
  const [importFile, setImportFile] = useState<{ text: string; name: string } | null>(null)
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

  // After this view's own write: bring the list up to date now, not on the next tick.
  const load = useCallback(() => {
    logSource().refresh()
  }, [])
  // An update from the callbook reports its end here, whether its dialog is still open or not.
  const followCallbookUpdate = useCallbookUpdateEnd(load)

  // Import an external ADIF logbook → real "needs" + B4. Read the file in the
  // browser/WebView (no fs plugin), hand the text to the engine.
  // The file opens the import preview (ImportPreview): nothing is written until the operator picks
  // Import or Import new only there, and the import it makes answers here, with its Undo.
  const onImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0]
    e.target.value = '' // let the same file be re-selected later
    if (!f) return
    const tooLarge = importTooLarge(f.size)
    if (tooLarge) {
      pushToast(tooLarge, 'error')
      return
    }
    setImportFile({ text: await f.text(), name: f.name })
  }
  const onImported = (stats: ImportStats) => {
    // An import of a LoTW/eQSL download adds nothing and updates everything —
    // say so, or the one toast that means "your awards were just repaired"
    // reads as "nothing happened, all dupes".
    // Three statements, each with its OWN count: one message cannot select a plural form
    // for two counts at once, so each carries its own (see the catalog's note).
    const dupes = stats.skipped ? t('logbook.import.dupes', { count: stats.skipped }) : ''
    const upgraded = stats.updated ? t('logbook.import.updated', { count: stats.updated }) : ''
    const run = stats.run
    pushToast(
      `${t('logbook.import.imported', { count: stats.added })}${dupes}${upgraded}`,
      'success',
      10_000,
      stats.added + stats.updated > 0 && run
        ? { actionLabel: t('logbook.undo'), action: () => undoRun(run, t('logbook.import.undone')) }
        : {},
    )
    load()
  }

  // Undo the run an import or a purge handed back — exactly that change, never "the latest of its
  // kind", which a contact logged a moment later would be — in one transaction. The toast's
  // action: a refusal (a large undo while transmitting) rejects with its reason, so the toast stays
  // up saying why and its Undo can be pressed again. Recently deleted offers the same undo after.
  const undoRun = async (run: string, done: string) => {
    const answer = await undoHistoryRun(run).catch((err: unknown) => {
      throw new Error(failureText(t('qso.history.undo.failed'), err))
    })
    sayUndone(answer, done, load)
    load()
  }

  // #348: ONE button for the confirmation downloads set up — LoTW and eQSL, one after another,
  // through the same commands their own button and Settings run — and one toast that says what each
  // one did. A service not set up is not asked (by its username).
  //
  // ⚠️ NOT the QRZ sync. It is two-way: it fetches the whole QRZ logbook and ADDS the contacts logged
  // elsewhere, which a button named for confirmations must not do. It keeps its own button.
  //
  // One download at a time: while this runs, LoTW's own button waits, and the other way round.
  const [fetching, setFetching] = useState<'all' | 'lotw' | null>(null)
  const fetchingAll = fetching === 'all'
  const onDownloadConfirmations = async () => {
    if (fetching) return
    setFetching('all')
    const s = await withErrorToast(() => getSettings(), t('logbook.confirmations.all.label'))
    if (!s) {
      setFetching(null)
      return
    }
    const sources: [string, () => Promise<{ newlyConfirmedAny: number }>][] = []
    if (s.lotwUsername.trim()) sources.push(['LoTW', downloadLotwReport])
    if (s.eqslUsername.trim()) sources.push(['eQSL', downloadEqslReport])
    if (sources.length === 0) {
      setFetching(null)
      pushToast(t('logbook.confirmations.all.none'), 'info', 8000)
      return
    }
    const said: string[] = []
    let failed = false
    for (const [service, run] of sources) {
      try {
        const r = await run()
        said.push(t('logbook.confirmations.all.ok', { service, confirmed: r.newlyConfirmedAny }))
      } catch (err) {
        failed = true
        said.push(t('logbook.confirmations.all.failed', { service, reason: err instanceof Error ? err.message : String(err) }))
      }
    }
    setFetching(null)
    pushToast(said.join(' · '), failed ? 'error' : 'success', 10000)
    load()
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

  // QSOs not yet sent to LoTW, and the date-only ones LoTW can never match (features/
  // lotwBacklog), asked of the log — once per change to it, never once per render: this view
  // re-renders on every snapshot.
  const { unsent: unsentLotw, timeless: timelessLotw } =
    useLogAnswer(LOTW_BACKLOG, logTick) ?? emptyAnswer(LOTW_BACKLOG)

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
    // Contacts that changed while TQSL signed are not marked (features/lotwSkips): say so, or
    // an upload count that does not clear reads as an upload that failed.
    const skipped = lotwSkipNote(r)
    if (skipped) pushToast(skipped, 'info', LOTW_SKIP_TOAST_MS)
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
  // Marks the time box while what is in it is not a 24-hour UTC time; saving then refuses.
  const whenTimeBad = draft.whenTime.trim() !== '' && parseUtcTime(draft.whenTime) === null
  // The same for the date box — a day that does not exist (2026-02-30) is refused, not rolled
  // forward, and the operator keeps what they typed instead of watching the box blank itself.
  const whenDateBad = draft.whenDate.trim() !== '' && parseUtcDate(draft.whenDate) === null
  // …and the END time box (#329), which takes the same 24-hour UTC shapes as the start.
  const endTimeBad = draft.endTime.trim() !== '' && parseUtcTime(draft.endTime) === null
  // …and for the export range below. A bound that is not a date HOLDS the export. The backend
  // refuses one (`export_general_log`: "bad export date … (expected YYYY-MM-DD)"), so the cost
  // of running anyway is a toasted error, not a wrong file — but the NATIVE control this
  // replaced never let that refusal happen: it emptied whatever it could not parse, an empty
  // bound means unbounded, and the export silently widened from a slice to the whole log. The
  // box keeps what was typed now, so saying no here is what keeps that quiet widening dead.
  const exportFromBad = exportFrom.trim() !== '' && parseUtcDate(exportFrom) === null
  const exportToBad = exportTo.trim() !== '' && parseUtcDate(exportTo) === null
  const exportRangeBad = exportFromBad || exportToBad

  // Open the form pre-filled to correct an existing entry (busted call, wrong band…).
  const startEdit = (q: LoggedQso, key: string, ref: RowRef | null) => {
    setErr(null)
    setEditing({ key, row: q, ref })
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
      whenDate: utcDate(q.whenUnix),
      whenTime: utcTime(q.whenUnix),
      endTime: q.timeOffUnix != null ? utcTime(q.timeOffUnix) : '',
      state: q.state ?? '',
      txPower: q.txPower != null ? String(q.txPower) : '',
      parkTheirRef: q.ota?.theirRef ?? '',
      parkMyRef: q.ota?.myRef ?? '',
      iota: q.ota?.iota ?? '',
      myGrid: q.myGrid ?? '',
      myRig: q.myRig ?? '',
      qslSentVia: q.qslSent?.sent ? (q.qslSent.via ?? 'SENT') : '',
      qslCard: q.qslRcvd?.card ? '1' : '',
    })
    setShowForm(true)
  }

  // THE EDIT HANDOFF (`focusCall.editId`): a contact clicked where another view lists it opens here,
  // in this form, exactly as its row's ✎ opens it. The row and its edit key are read from the engine
  // as the list reads them, so a save made after the contact changed elsewhere is refused, never
  // overwritten. The search the handoff also sets (above) puts the row on screen, marked. A Remote
  // browser edits nothing from here (`control`), so there the handoff only searches, as before.
  const handoff = useRef<number | null>(null)
  const alive = useRef(true)
  useEffect(() => () => void (alive.current = false), [])
  useEffect(() => {
    const id = focusCall?.editId
    if (!focusCall || !id) return
    const call = focusCall.call
    handoff.current = focusCall.ts
    const ts = focusCall.ts
    void keyedRow(id)
      .catch(() => null)
      .then((found) => {
        if (!alive.current || handoff.current !== ts) return
        if (found) startEdit(found.row, id, { id, editKey: found.editKey })
        else pushToast(t('logbook.form.editGone', { call }), 'error')
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per handoff (`ts` is its nonce)
  }, [focusCall])

  const cancelForm = () => {
    setShowForm(false)
    setEditing(null)
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

  // Correct ONE contact already in the QRZ logbook. A normal push cannot do this: QRZ answers
  // "Duplicate" and keeps its copy. This reads QRZ's own record back first and sends it whole
  // with only the corrected fields changed — every refusal, and the reading of QRZ's answer,
  // lives in the backend.
  const onCorrectQrz = async (q: LoggedQso) => {
    setQrzFix({ q, phase: 'reading', text: '', result: null })
    try {
      const p = await qrzCorrectPreview(q, 1)
      setQrzFix({ q, phase: 'confirm', text: p.confirmation, result: null })
    } catch (e) {
      // A refusal is not an error the operator caused; it is the reason this will not happen,
      // and it is written to be read.
      setQrzFix(null)
      pushToast(String(e), 'error', 9000)
    }
  }

  // The operator has read what will change and said yes.
  const onCorrectQrzConfirm = async () => {
    if (!qrzFix) return
    const q = qrzFix.q
    setQrzFix({ ...qrzFix, phase: 'sending' })
    try {
      const r = await qrzCorrectApply(q, 1)
      // ⚠️ `r.ok` is QRZ's RESULT=REPLACE and nothing else. A RESULT=OK means QRZ added a
      // SECOND copy instead of overwriting, and the dialog stays open saying so.
      setQrzFix({ q, phase: 'done', text: r.message, result: r })
    } catch (e) {
      setQrzFix({ q, phase: 'done', text: String(e), result: null })
    }
  }

  // Remove the duplicate a missed replace created — permanent, and only ever the record QRZ
  // itself reported adding.
  const onCorrectQrzRecover = async () => {
    if (!qrzFix) return
    setQrzFix({ ...qrzFix, phase: 'recovering' })
    try {
      const said = await qrzCorrectUndoMiss()
      setQrzFix({ ...qrzFix, phase: 'done', text: said, result: null })
      pushToast(said, 'success', 7000)
    } catch (e) {
      setQrzFix({ ...qrzFix, phase: 'done', text: String(e) })
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

  // Manual (re-)push of one logged QSO to World Radio League — same role as the
  // HRDLog button. A live-logging service, not an ARRL confirmation source.
  const onPushWrl = async (q: LoggedQso) => {
    try {
      const r = await wrlPushQso(q)
      if (r.result === 'accepted') {
        pushToast(t('logbook.push.wrl.ok', { call: q.call }), 'success', 4000)
      } else if (r.result === 'duplicate') {
        pushToast(t('logbook.push.wrl.duplicate', { call: q.call }), 'success', 5000)
      } else if (r.result === 'pending') {
        // Transient by contract (rate limit / server trouble) — the QSO is fine.
        pushToast(t('logbook.push.wrl.unavailable', { call: q.call }), 'info', 6000)
      } else {
        pushToast(
          t('logbook.push.wrl.rejected', { call: q.call, reason: r.message ?? r.result }),
          'error',
          6000,
        )
      }
    } catch (e) {
      pushToast(t('logbook.push.wrl.failed', { detail: String(e) }), 'error', 6000)
    }
  }

  /** What a change to one contact by `RowRef` did, said when it was NOT made: the contact changed
   *  since this view read it — in another window (a Remote browser), or by a sync filling a blank
   *  field — or it was deleted meanwhile. Either way nothing was written, and the list is read
   *  again so the row shows as it now is, or goes. Or the logbook was too busy to take it: nothing
   *  was written and nothing changed, so nothing on screen moves and a retry is one click. Whether
   *  the change was made. */
  const changeMade = (answer: RowAnswer, call: string): boolean => {
    if (answer.kind === 'busy') {
      pushToast(t('logbook.change.busy', { call }), 'error', 8000)
      return false
    }
    if (answer.kind === 'changed') pushToast(t('logbook.change.changed', { call }), 'error', 8000)
    else if (answer.kind === 'gone') pushToast(t('logbook.change.gone', { call }), 'error', 8000)
    else return true
    load()
    return false
  }

  // Record an operator-declared QSL request on a contact (a card/request WAS sent,
  // via bureau/direct/electronic). This is NOT a confirmation — it stays in the
  // needs-confirmation filter until the partner actually confirms.
  //
  // `via === null` CLEARS the mark instead (#180). Sending is once-only, so a mis-click
  // on Bureau/Direct/Electronic used to be permanent from the operator's chair: the three
  // send entries vanish and nothing put them back. Same reversal the inbound card has had
  // since #152 — a declaration the operator made by hand, they can unmake by hand.
  const onMarkQslSent = async (q: LoggedQso, ref: RowRef | null, via: 'B' | 'D' | 'E' | null) => {
    // The row on screen by its id and edit key, never its position (see `changeMade`).
    if (!ref) return
    const answer = await withErrorToast(() => markQslSentById(ref, via), t('logbook.qsl.markFailed'))
    if (answer && changeMade(answer, q.call)) {
      // Two literal keys, not one interpolated one — same reason as onMarkQslCard below.
      pushToast(
        via
          ? t('logbook.qsl.marked', { call: q.call, via: qslViaLabel(via) ?? via })
          : t('logbook.qsl.sentCleared', { call: q.call }),
        'success',
      )
      load()
    }
  }

  const onMarkQslCard = async (q: LoggedQso, ref: RowRef | null, received: boolean) => {
    if (!ref) return
    const answer = await withErrorToast(
      () => markQslCardById(ref, received),
      t('logbook.qsl.markFailed'),
    )
    if (answer && changeMade(answer, q.call)) {
      // Two literal keys, not one interpolated one: the i18n orphan guard scans for literal
      // `t('key')` references and a ternary inside the call is invisible to it — it flagged
      // both of these as unused catalog entries, which is exactly its job.
      pushToast(
        received
          ? t('logbook.qsl.cardMarked', { call: q.call })
          : t('logbook.qsl.cardCleared', { call: q.call }),
        'success',
      )
      load()
    }
  }

  // Set or REMOVE the satellite tag on a row (#313 follow-up). `name === null` is the removal,
  // and it is the whole reason this is a menu entry rather than two boxes on the edit form:
  // that form reads a blank field as LEAVE ALONE so a busted-call fix cannot strip a tag the
  // contact earned, so "clear it" has to be something the operator SAYS. Same shape as the
  // QSL-sent withdrawal above — an operator-declared fact, unmade only by the operator.
  const onSetSatTag = async (q: LoggedQso, ref: RowRef | null, name: string | null) => {
    if (!ref) return
    const answer = await withErrorToast(() => setSatTagById(ref, name), t('logbook.sat.failed'))
    if (answer && changeMade(answer, q.call)) {
      // Two literal keys, not one interpolated one — the i18n orphan guard cannot see a
      // ternary inside the call (same reason as onMarkQslCard above).
      pushToast(
        name
          ? t('logbook.sat.tagged', { call: q.call, sat: name })
          : t('logbook.sat.cleared', { call: q.call }),
        'success',
      )
      load()
    }
  }

  // Set — or CLEAR — by hand the DXCC entity a contact counts toward (ADIF DXCC), from its Country
  // cell. `dxcc === null` is the clear, chosen from the picker's own entry like the satellite
  // tag's: the contact counts by its call again — or by LoTW's number, which the log keeps where
  // LoTW confirmed it as another entity, and which then stands as the log's. Desktop only, like
  // the tag: no Remote capability carries it, so an observing browser shows the plain cell.
  const onSetDxcc = async (q: LoggedQso, ref: RowRef | null, dxcc: number | null) => {
    if (!ref) return
    const answer = await withErrorToast(() => setDxccById(ref, dxcc), t('logbook.country.failed'))
    if (answer && changeMade(answer, q.call)) {
      const now = answer.kind === 'applied' ? answer.current.row : null
      pushToast(
        now && now.dxcc == null
          ? t('logbook.country.cleared', { call: q.call })
          : t('logbook.country.set', { call: q.call, entity: now?.entity ?? now?.country ?? '' }),
        'success',
      )
      load()
    }
  }

  /** Ask, then delete: whether the contact was deleted. */
  const onDelete = async (q: LoggedQso, key: string, ref: RowRef | null): Promise<boolean> => {
    if (
      !(await confirmDialog({
        title: t('logbook.delete.heading', { call: q.call, band: q.band }),
        body: t('logbook.delete.body'),
        confirmLabel: t('logbook.delete.confirm'),
        danger: true,
      }))
    )
      return false
    if (!ref) return false
    const answer = await withErrorToast(() => deleteQsoById(ref), t('logbook.delete.failed'))
    if (!answer) return false
    if (answer.kind === 'busy') {
      // The logbook was too busy to take it: not deleted, and the row stays as it is.
      pushToast(t('logbook.delete.busy', { call: q.call }), 'error', 8000)
      return false
    }
    if (answer.kind === 'changed') {
      // Changed since it was shown: not deleted. The list shows it as it is now.
      pushToast(t('logbook.delete.changed', { call: q.call }), 'error', 8000)
      load()
      return false
    }
    const id = q.id
    pushToast(
      answer.kind === 'gone' ? t('logbook.delete.gone', { call: q.call }) : t('logbook.delete.done', { call: q.call }),
      answer.kind === 'gone' ? 'info' : 'success',
      10_000,
      answer.kind === 'deleted' && id ? {
        actionLabel: t('logbook.undo'),
        action: () => {
          void withErrorToast(() => restoreDeletedQso(id), t('logbook.deleted.failed')).then((a) => {
            if (a) { sayUndone(a, t('logbook.deleted.restored', { call: q.call }), load); load() }
          })
        },
      } : {},
    )
    if (editingKey === key) cancelForm()
    load()
    return true
  }

  const closePurge = () => {
    setShowPurge(false)
    setPurgeText('')
  }

  // Wipe the ENTIRE logbook (truncates the ADIF file). Armed only once the operator
  // types the confirmation word — a wipe of the whole log gets a deliberate gate.
  const onPurge = async () => {
    if (purgeText.trim().toUpperCase() !== PURGE_WORD) return
    setPurging(true)
    const purged = await withErrorToast(() => purgeLog(), t('logbook.purge.failed'))
    setPurging(false)
    if (purged) {
      const { removed, run } = purged
      pushToast(t('logbook.purge.done', { count: removed }), 'success', 15_000, removed > 0 ? {
        actionLabel: t('logbook.undo'),
        action: () => undoRun(run, t('logbook.purge.undone')),
      } : {})
      closePurge()
      cancelForm()
      load()
    }
  }

  // What the list shows: the search and the chip (the deferred search, so typing stays responsive
  // on a big log) and the sorted column. The desktop asks its source for the list in PAGES of this
  // order — the first here, for the count; the rest below, for the rows on screen — and holds only
  // those. A page is cut by `features/logQuery`, the filter and sort this view used to run itself.
  const query = useMemo<LogQuery>(
    () => ({
      sort: sort.sort,
      asc: sort.asc,
      search: deferredSearch,
      needsConfirmOnly,
      ...(sort.then.length ? { then: sort.then } : {}),
      ...(Object.keys(deferredFilter).length ? { filter: deferredFilter } : {}),
    }),
    [sort, deferredSearch, needsConfirmOnly, deferredFilter],
  )
  const queryKey = logQueryKey(query)
  const firstPage = { kind: 'page', query, offset: 0, limit: LOG_PAGE } as const
  const latestFirst = useLogAnswer(firstPage, logTick)
  const firstPageStatus = useLogStatus(firstPage)

  // THE LIST ON SCREEN IS ONE ORDER, AND IT IS SWAPPED WHOLE (v2 R4, R5). The source may already
  // hold a newer order than the list shows — a contact was logged, a row deleted. The list keeps
  // showing the order it has until EVERYTHING the new one needs is here: its first page (the
  // count), where the rows under the operator went (`locate`, below), and the pages that place
  // shows. Then it swaps in ONE render and is placed before the browser paints. A source that
  // answers one question at a time used to have the list swap on the first answer and wait for the
  // rest with the new rows at the old scroll: the row under the pointer sat 43 px off for as long
  // as they took (measured in Chrome: 1 frame at local speed, 10 at 150 ms).
  //
  // So the list keeps the pages it has read, by (query, order, offset) — the order on screen stays
  // here while the source moves on — and `shown` names the order it shows. A NEW list (the first,
  // a new sort or search) is shown as it comes, as before; only a change within one list waits.
  const held = useRef(new Map<string, LogPage>())
  const pageKey = (qk: string, orderRev: number, offset: number) => `${qk}|${orderRev}|${offset}`
  const remember = (page: LogPage | undefined) => {
    if (!page) return
    const k = pageKey(logQueryKey(page.query), page.orderRev, page.offset)
    const had = held.current.get(k)
    // Never back in content: an older read of the same rows in the same order is not applied (R4).
    if (had && had !== page && had.contentRev > page.contentRev) return
    held.current.delete(k)
    held.current.set(k, page)
    for (const oldest of held.current.keys()) {
      if (held.current.size <= HELD_PAGES) break
      held.current.delete(oldest)
    }
  }
  const recall = (orderRev: number, offset: number, qk = queryKey) => {
    const k = pageKey(qk, orderRev, offset)
    const page = held.current.get(k)
    if (page) {
      held.current.delete(k)
      held.current.set(k, page)
    }
    return page
  }
  remember(latestFirst)
  const [shown, setShown] = useState<{ queryKey: string; orderRev: number } | null>(null)
  const latestRev = latestFirst && logQueryKey(latestFirst.query) === queryKey ? latestFirst.orderRev : null
  const onScreen = shown?.queryKey === queryKey ? shown.orderRev : null
  // A NEW LIST IS SHOWN ONCE ITS FIRST PAGE IS HERE. Until then the list on screen stays — the old
  // sort or search, for the moment the new one takes to answer — where it used to be taken away: a
  // source that answers one question at a time (C17a's) left the list EMPTY for a frame or more,
  // "no contacts match" with it, and in a browser the scroll then fell back to the top of the pane,
  // the globe band in view and the search box pulled down the screen under the operator's typing.
  // A newer order of one list already waited this way (the swap, below).
  const kept = onScreen === null && latestRev === null ? shown : null
  /** The query of the list ON SCREEN: the new one, or — while its first page is on its way — the old. */
  const listKey = kept ? kept.queryKey : queryKey
  const showingRev = kept ? kept.orderRev : (onScreen ?? latestRev)
  const pendingRev = onScreen !== null && latestRev !== null && latestRev > onScreen ? latestRev : null
  const listTotal = showingRev === null ? 0 : (recall(showingRev, 0, listKey)?.total ?? 0)
  // WHAT THE LIST SAYS WHEN IT HAS NO ROWS — and a log that exists is never reported empty. "No
  // logged contacts yet." only on an answer that the LOG holds none: the engine's count of 0, or a
  // page the station sent for no search and no filter. A list that matched nothing says so. Until
  // the engine has counted the log, and sent the first page of a log it counted, the list is
  // reading the logbook: at every open for a moment, and for as long as the engine refuses the
  // questions while a change is being saved. So is a held 0 from before the latest change. A Remote
  // page on its way says so in its status line.
  //
  // A READ THAT FAILED says so, in the engine's words, with Retry (`load`): the count's, or the first
  // page's. Retry and reopening the view ask again. The rows the list had stay up under the line.
  // A refusal while a change is being saved stays quiet, even once its asks run out
  // (`failureToShow`): the list keeps saying it is reading the logbook.
  const readFailed = failureToShow(sizeStatus) ?? failureToShow(firstPageStatus)
  const listSays: 'failed' | 'reading' | 'empty' | 'noMatch' | null =
    readFailed !== null ? 'failed'
    : listTotal > 0 ? null
    : askedLogSize === undefined || (askedLogSize === 0 && !sizeCurrent) || (askedLogSize > 0 && showingRev === null) ? 'reading' : askedLogSize === 0 ? 'empty' : 'noMatch'

  // 3-D globe band, gated on a real GPU (software renderers would make the whole
  // Logbook crawl — those machines just get the plain table). Probed once per mount.
  const [globeOk] = useState(gpuCapableForGlobe)
  // D#278: the operator's own switch (Settings ▸ Appearance ▸ Map & globe). Off = no band at all,
  // and the table starts at the top.
  const [globeWanted] = useLogbookGlobe()
  const globeShown = globeOk && globeWanted && logSize > 0

  // #162: rows whose Comment is opened to its full length, keyed like the row itself. The column
  // clips to one line; clicking the comment wraps it in place (the virtualizer measures the
  // taller row), and clicking again folds it.
  const [openComments, setOpenComments] = useState<Set<string>>(() => new Set())
  const toggleComment = useCallback((key: string) => {
    setOpenComments((prev) => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }, [])

  // Virtualize the row list: at 10k QSOs the old render put ~150k DOM nodes on screen (heavy scroll
  // + a full reconcile every dial-poll re-render). Now only the visible window mounts.
  const scrollRef = useRef<HTMLDivElement>(null)
  const rowsWrapRef = useRef<HTMLDivElement>(null)
  // The globe band: its divider (layout L7) sizes it, and the list offset below follows it.
  const globeRef = useRef<HTMLDivElement>(null)
  const [listOffset, setListOffset] = useState(0)
  const rowVirtualizer = useVirtualizer({
    count: listTotal,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_ESTIMATE, // ~row height; measureElement corrects per row
    overscan: 12,
    // The globe band + sticky search/header block sit INSIDE the scroll container
    // above the rows (globe scrolls away; search+headers pin). scrollMargin tells the
    // virtualizer how far into the scroll space the list starts — MEASURED, because
    // the sticky block's height isn't a constant; the row transform subtracts it back.
    scrollMargin: listOffset,
  })

  // v2 §6 R5 — THE ROWS UNDER THE OPERATOR STAY PUT. When the log changes under the list (the
  // sequencer logs a contact, another writer deletes one), rows appear or vanish above the view and
  // every row below them changes place: the list used to slide a row under the pointer, so a click
  // meant for one contact's ✎ or ✕ could land on its neighbour. After every commit the rows at the
  // top edge of the view are noted — keys and pixel offsets (`anchor`). While a newer order is
  // pending, the first of them still in it is located there, and the list is swapped in placed so
  // that row sits exactly where it was (its successor holds if the row itself was deleted). A view
  // showing the list's first row is not held: a new contact appears at the top, as it always did.
  const anchor = useRef<{ rows: { key: string; delta: number }[]; atTop: boolean } | null>(null)
  const placement = useRef<{ index: number; delta: number } | null>(null)
  const justPlaced = useRef(false)

  // AN OPEN ROW'S HEIGHT GOES WITH IT (a bug older than this list). The list keeps each row's
  // measured height by the row's PLACE, and measures a row only when it mounts or resizes — so a
  // row that stays mounted while its place changes keeps its height at the old place. Every closed
  // row is as tall as the next, so that costs nothing, except for an OPEN row (a comment or note
  // opened to full length): after the order changed, its height stayed where it had been — a gap
  // there — and it was drawn into a closed row's height at its new place, its note under the rows
  // below, until the rows remounted. So the list knows where each open row's height sits
  // (`openAt`: its place in the order on screen), and a swap moves the height with it (in the
  // placing effect below, before the view is placed).
  const openAt = useRef<{ queryKey: string; at: Map<string, number> }>({ queryKey, at: new Map() })
  if (openAt.current.queryKey !== listKey) openAt.current = { queryKey: listKey, at: new Map() }
  /** The open rows' heights to move, taken as a swap is decided and applied in the render that
   *  shows the new order (when the list is its new length), before that render is placed. */
  const heightMoves = useRef<{ closed: number; moves: { key: string; from: number; to: number | null; size: number }[] } | null>(null)
  // …and A NEW LIST IS MEASURED AFRESH (the same bug, when the list is replaced rather than moved):
  // a new sort or search, or — in a Remote browser — the page the station sends coming back, which
  // is how a contact logged at the station arrives there. The heights kept by place were measured
  // for the old list. A row drawn in both stays mounted and is not measured again; a row mounting at
  // a place the old list measured takes that place's height until the browser reports its own, a
  // frame later. So while a row was open, its height stayed at its old place, over another row, and
  // the open row was drawn into a closed row's height. The first time a new list is drawn while a
  // row is open, the old list's heights are dropped and the rows drawn are measured, in the placing
  // effect below, before the paint. `drawnList` names the list on screen: the query's (once its
  // first page is here).
  const drawnList = showingRev === null ? null : listKey
  const heightsFor = useRef<string | null>(null)

  const virtualRows = rowVirtualizer.getVirtualItems()
  const held0 = anchor.current
  const anchorRows = pendingRev !== null && held0 !== null && !held0.atTop ? held0.rows : []
  // While a swap is pending: where the anchor rows went, and where the open rows went.
  const openKeys = pendingRev === null ? [] : [...openAt.current.at.keys()].filter((key) => openComments.has(key))
  const locateKeys = [...new Set([...anchorRows.map((row) => row.key), ...openKeys])]
  const located = useLogAnswers(
    locateKeys.map((key): LogQuestion => ({ kind: 'locate', query, id: key })),
    logTick,
    true,
  ) as readonly (LogLocate | undefined)[]
  const placeOf = new Map(locateKeys.map((key, k) => [key, located[k]] as const))
  // Where the list will stand in the pending order: `undefined` while that is not known yet;
  // `index: null` keeps the scroll (at the top, or when no anchor row survived).
  let target: { index: number | null; delta: number } | undefined
  if (pendingRev !== null) {
    target = { index: null, delta: 0 }
    for (const row of anchorRows) {
      const at = placeOf.get(row.key)
      if (!at || at.orderRev !== pendingRev) {
        target = undefined
        break
      }
      if (at.index !== null) {
        target = { index: at.index, delta: row.delta }
        break
      }
    }
  }
  const openPlacesKnown = openKeys.every((key) => placeOf.get(key)?.orderRev === pendingRev)
  // The pages that place shows (the viewport there, overscan included) — asked for now, with the
  // pages of the rows on screen, so the swap waits for nothing it will draw.
  const offsetsOf = (first: number, last: number) => {
    const out: number[] = []
    for (let o = Math.floor(Math.max(0, first) / LOG_PAGE) * LOG_PAGE; o <= last; o += LOG_PAGE) out.push(o)
    return out
  }
  // The pages the view shows — of the list on screen's own query only: while an old list is kept
  // (above), the new query is asked for its first page and nothing else.
  const visibleOffsets = !kept ? [...new Set(virtualRows.map((v) => Math.floor(v.index / LOG_PAGE) * LOG_PAGE))] : []
  const pendingTotal = pendingRev === null ? 0 : (recall(pendingRev, 0)?.total ?? 0)
  const targetOffsets =
    target === undefined || pendingTotal === 0
      ? []
      : target.index === null
        ? visibleOffsets.filter((o) => o < pendingTotal)
        : offsetsOf(
            target.index - OVERSCAN,
            Math.min(
              pendingTotal - 1,
              target.index + Math.ceil((scrollRef.current?.clientHeight ?? 0) / ROW_ESTIMATE) + 1 + OVERSCAN,
            ),
          )
  const pages = useLogPages(
    query,
    [...new Set([...visibleOffsets, ...targetOffsets])].filter((o) => o > 0),
    LOG_PAGE,
    logTick,
  )
  pages.forEach((page) => remember(page))
  const ready =
    pendingRev !== null &&
    target !== undefined &&
    openPlacesKnown &&
    [0, ...targetOffsets].every((o) => recall(pendingRev, o) !== undefined)
  /** The row drawn at `i`: the contact, its key in the list, and — on the desktop — the `RowRef` a
   *  change to it sends: its id and the edit key of the version this page read. */
  const rowAt = (i: number): { q: LoggedQso; key: string; ref: RowRef | null } | undefined => {
    if (showingRev === null) return undefined
    const offset = Math.floor(i / LOG_PAGE) * LOG_PAGE
    const page = recall(showingRev, offset, listKey)
    if (!page) return undefined
    const k = i - offset
    return k < page.rows.length
      ? { q: page.rows[k], key: page.keys[k], ref: { id: page.keys[k], editKey: page.editKeys[k] } }
      : undefined
  }

  /** Pick rows for "Export selected": one row toggles; a run (Shift) adds every row from the last
   *  one picked. A run is picked only once all of it has loaded — part of it would export less
   *  than the operator meant. */
  const pickRows = (index: number, run: boolean) => {
    const anchor = pickAnchor.current
    if (run && anchor !== null) {
      const ids: string[] = []
      for (let i = Math.min(anchor, index); i <= Math.max(anchor, index); i++) {
        const id = rowAt(i)?.q.id
        if (!id) {
          pushToast(t('logbook.export.selected.notLoaded'), 'info')
          return
        }
        ids.push(id)
      }
      setPicked((was) => new Set([...was, ...ids]))
      return
    }
    const id = rowAt(index)?.q.id
    if (!id) return
    pickAnchor.current = index
    setPicked((was) => {
      const next = new Set(was)
      if (!next.delete(id)) next.add(id)
      return next
    })
  }

  /** The height closed rows are drawn at: the one most of the rows drawn now have. */
  const closedHeight = () => {
    const seen = new Map<number, number>()
    for (const item of virtualRows) {
      const row = rowAt(item.index)
      if (row && !openComments.has(row.key)) seen.set(item.size, (seen.get(item.size) ?? 0) + 1)
    }
    let size = ROW_ESTIMATE
    let most = 0
    for (const [h, n] of seen)
      if (n > most) {
        size = h
        most = n
      }
    return size
  }

  // THE KEYBOARD GRID (v2 §6). The list is ONE Tab stop — a row, held by id so the operator's place
  // survives a newer order — and grid-local keys move it (no global shortcuts): ↑/↓ a row, PgUp/PgDn
  // a visible page, Home/End the first/last contact; Enter opens its contact card, Delete asks the
  // existing delete question, Esc closes the form (the question closes itself). A key on a row's own
  // control (its QSL menu, its buttons) stays that control's. The row the keys land on is scrolled
  // in whole, under the pinned search box and headers, and focused once it is drawn; a row whose
  // page has not landed is a placeholder, which holds the focus until it does.
  const keysId = useId()
  const [focusKey, setFocusKey] = useState<string | null>(null)
  const [focusTo, setFocusTo] = useState<number | null>(null)
  /** The row whose pencil opened the form (the keyboard goes back to it when it closes), and
   *  whether the keyboard is still to go INTO the form. */
  const formFromGrid = useRef<string | null>(null)
  const formWanted = useRef(false)
  const formRef = useRef<HTMLFormElement>(null)
  /** A contact deleted from the keys: the focus waits for the row that takes its place. */
  const focusAfter = useRef<string | null>(null)
  /** The pinned search box + headers' height, as of the last commit. */
  const stickyH = useRef(0)
  /** Where rows start to be SEEN, in the list's coordinates, at scroll `top`: under the sticky
   *  block, or at the list's own top while the globe band is still in view. */
  const floorAt = (top: number) => Math.max(top + stickyH.current, rowVirtualizer.options.scrollMargin)
  const firstSeen = virtualRows.find((v) => v.end > floorAt(rowVirtualizer.scrollOffset ?? 0) && rowAt(v.index))
  const tabStop =
    focusKey !== null && virtualRows.some((v) => rowAt(v.index)?.key === focusKey)
      ? focusKey
      : firstSeen
        ? (rowAt(firstSeen.index)?.key ?? null)
        : null
  /** Put the keyboard on row `index`: seen whole, then focused once drawn (the effect below). */
  const moveFocus = (index: number) => {
    const el = scrollRef.current
    const item = rowVirtualizer.measurementsCache[index]
    if (el && item) {
      const floor = floorAt(el.scrollTop)
      let top = el.scrollTop
      if (item.start < floor) top = el.scrollTop - (floor - item.start)
      else if (item.end > el.scrollTop + el.clientHeight) top = item.end - el.clientHeight
      if (top !== el.scrollTop) {
        el.scrollTop = top
        rowVirtualizer.scrollOffset = el.scrollTop // drawn there by the next render, not a frame on
      }
    }
    setFocusTo(index)
  }
  const gridStep = (key: string, index: number): number | null => {
    const page = Math.max(1, Math.floor(((scrollRef.current?.clientHeight ?? 0) - stickyH.current) / ROW_ESTIMATE))
    switch (key) {
      case 'ArrowDown':
        return index + 1
      case 'ArrowUp':
        return index - 1
      case 'PageDown':
        return index + page
      case 'PageUp':
        return index - page
      case 'Home':
        return 0
      case 'End':
        return listTotal - 1
      default:
        return null
    }
  }
  const onGridKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement
    if (!target.classList.contains('logbook-row')) return
    const index = Number(target.dataset.index)
    const to = gridStep(e.key, index)
    if (to !== null) {
      e.preventDefault()
      moveFocus(Math.max(0, Math.min(listTotal - 1, to)))
      return
    }
    const row = rowAt(index)
    if (e.key === ' ') {
      e.preventDefault()
      pickRows(index, e.shiftKey)
    } else if (e.key === 'Escape' && showForm) {
      e.preventDefault()
      cancelForm()
    } else if (e.key === 'Escape' && picked.size) {
      e.preventDefault()
      pickAnchor.current = null
      setPicked(new Set())
    } else if (row && (e.key === 'Enter' || e.key === 'Delete')) {
      e.preventDefault()
      if (e.key === 'Delete') {
        // The question takes the keyboard (a dialog puts it nowhere when it closes); the answer
        // gives it back to this place in the list: the same contact, or — once it is gone — the one
        // that takes its place.
        void onDelete(row.q, row.key, row.ref).then((deleted) => {
          focusAfter.current = deleted ? row.key : null
          setFocusTo(index)
        })
      } else {
        // Enter opens the row's contact card, as a double-click does; its pencil still opens the
        // edit form.
        setCard({ row: row.q, ref: row.ref })
      }
    }
  }
  // The keys' row, focused once it is drawn.
  useLayoutEffect(() => {
    stickyH.current = scrollRef.current?.querySelector<HTMLElement>('.log-sticky')?.offsetHeight ?? 0
    const wrap = rowsWrapRef.current
    if (focusTo === null || !wrap) return
    const at = Math.min(focusTo, listTotal - 1)
    if (focusAfter.current !== null && rowAt(at)?.key === focusAfter.current) return
    focusAfter.current = null
    const el = ([...wrap.children] as HTMLElement[]).find((c) => c.dataset.index === String(at))
    if (!el) return
    // Only while the keyboard is still the grid's: on a row, or lost with a row scrolled away.
    const active = document.activeElement
    if (active && active !== document.body && !wrap.contains(active)) {
      setFocusTo(null)
      return
    }
    el.focus({ preventScroll: true })
    if (!el.classList.contains('placeholder')) setFocusTo(null)
  })
  // A row's pencil: the keyboard goes into the form, and back to that row when it closes.
  useEffect(() => {
    if (showForm && formWanted.current) {
      formWanted.current = false
      formRef.current?.querySelector<HTMLElement>('input, select, textarea')?.focus()
    } else if (!showForm && formFromGrid.current !== null) {
      const key = formFromGrid.current
      formFromGrid.current = null
      const at = virtualRows.find((v) => rowAt(v.index)?.key === key)
      if (at) moveFocus(at.index)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- on the form opening and closing only
  }, [showForm])
  // The swapped-in order is placed after the render that shows it, before the browser paints —
  // its open rows' heights moved first, so the list is laid out, and placed, as the rows are.
  // (Declared before the swap below: effects run in order, so the swap's own render has passed.)
  useLayoutEffect(() => {
    // A new list drawn (`drawnList`, above). While a row is open, the old list's heights are dropped
    // and the rows drawn now measured — without scrolling: where the view goes is decided below.
    if (drawnList !== null && heightsFor.current !== drawnList) {
      const replaced = heightsFor.current !== null
      heightsFor.current = drawnList
      if (replaced && openComments.size > 0) {
        const adjust = rowVirtualizer.shouldAdjustScrollPositionOnItemSizeChange
        rowVirtualizer.shouldAdjustScrollPositionOnItemSizeChange = () => false
        try {
          rowVirtualizer.measure()
          rowVirtualizer.getVirtualItems() // the table at the estimate, then each drawn row's own height
          for (const el of rowsWrapRef.current?.children ?? []) {
            const index = (el as HTMLElement).dataset.index
            if (index !== undefined) rowVirtualizer.resizeItem(Number(index), (el as HTMLElement).offsetHeight)
          }
        } finally {
          rowVirtualizer.shouldAdjustScrollPositionOnItemSizeChange = adjust
        }
        rowVirtualizer.getVirtualItems()
      }
      // BACK TO THE TOP on a new sort, search or filter (v2 §6): the rows the operator was looking
      // at are not where they were, or not in this list at all. The new list's first row goes to the
      // top of the list, under the search box and the headers, which stay put — the globe band above
      // them is not brought back, or the box being typed in would jump. A view that has not scrolled
      // into the list is there already. (A newer order of the SAME list is not a new list: the swap
      // keeps the rows under the operator where they are, R5.) A Remote browser's new page is left
      // as it was: it empties while it loads.
      const el = scrollRef.current
      const rows = rowsWrapRef.current
      const sticky = el?.querySelector<HTMLElement>('.log-sticky')
      if (replaced && el && rows && sticky) {
        const top = Math.max(0, rows.offsetTop - sticky.offsetHeight)
        if (el.scrollTop > top) {
          el.scrollTop = top
          // …and drawn there by the render that follows, not when the scroll is reported: so the rows
          // noted under the operator are the new list's first rows, and a newer order arriving now
          // keeps the view at the top (R5's rule for a view at the top).
          rowVirtualizer.scrollOffset = el.scrollTop
          justPlaced.current = true
        }
      }
    }
    const moving = heightMoves.current
    heightMoves.current = null
    if (moving) {
      // Each vacated place gets a closed row's height, each new place the open row's own, through
      // the list's own `resizeItem` — its scroll adjustment held off: the swap places the view.
      // (A state change in a layout effect is drawn before the paint, so this lands in one frame.)
      const landing = new Set(moving.moves.map((m) => m.to))
      const adjust = rowVirtualizer.shouldAdjustScrollPositionOnItemSizeChange
      rowVirtualizer.shouldAdjustScrollPositionOnItemSizeChange = () => false
      try {
        for (const m of moving.moves) if (!landing.has(m.from)) rowVirtualizer.resizeItem(m.from, moving.closed)
        for (const m of moving.moves) if (m.to !== null) rowVirtualizer.resizeItem(m.to, m.size)
      } finally {
        rowVirtualizer.shouldAdjustScrollPositionOnItemSizeChange = adjust
      }
      for (const m of moving.moves) {
        if (m.to === null) openAt.current.at.delete(m.key)
        else openAt.current.at.set(m.key, m.to)
      }
      rowVirtualizer.getVirtualItems() // the list's table, recomputed with the moved heights
    }
    const p = placement.current
    placement.current = null
    const el = scrollRef.current
    if (!p || !el) return
    const start =
      rowVirtualizer.measurementsCache[p.index]?.start ?? p.index * ROW_ESTIMATE + rowVirtualizer.options.scrollMargin
    el.scrollTop = start - p.delta
    justPlaced.current = true
  })
  useLayoutEffect(() => {
    const el = scrollRef.current
    if (!el) return
    // Where each open row's height sits: its place among the rows drawn now (a row is measured
    // where it mounts; a swap moves the height with it). Closed rows are forgotten.
    const at = openAt.current.at
    for (const key of [...at.keys()]) if (!openComments.has(key)) at.delete(key)
    for (const item of virtualRows) {
      const row = rowAt(item.index)
      if (row && openComments.has(row.key)) at.set(row.key, item.index)
    }
    // The view was just placed: the rows noted now would be read at the old place. The scroll
    // that placing causes renders again, and they are noted then.
    if (justPlaced.current) {
      justPlaced.current = false
      return
    }
    const top = el.scrollTop
    const rows: { key: string; delta: number }[] = []
    let atTop = true
    for (const item of virtualRows) {
      if (item.end <= top) continue
      const row = rowAt(item.index)
      if (!row) continue
      if (rows.length === 0) atTop = item.index === 0
      rows.push({ key: row.key, delta: item.start - top })
      if (rows.length === 8) break
    }
    anchor.current = { rows, atTop }
  })
  // The swap: a new list is recorded as it is shown; a newer order within the list is swapped in
  // once `ready`, with its placement — a state change in a layout effect re-renders before paint.
  useLayoutEffect(() => {
    if (latestRev === null) return
    if (onScreen === null) {
      setShown({ queryKey, orderRev: latestRev })
      return
    }
    if (!ready || pendingRev === null || target === undefined) return
    const moves: { key: string; from: number; to: number | null; size: number }[] = []
    for (const key of openKeys) {
      const from = openAt.current.at.get(key)
      const to = placeOf.get(key)?.index ?? null
      const size = from === undefined ? undefined : rowVirtualizer.measurementsCache[from]?.size
      if (from !== undefined && size !== undefined && to !== from) moves.push({ key, from, to, size })
    }
    heightMoves.current = moves.length ? { closed: closedHeight(), moves } : null
    placement.current = target.index === null ? null : { index: target.index, delta: target.delta }
    setShown({ queryKey, orderRev: pendingRev })
  })

  // Measure where the rows actually start inside .log-scroll (globe + sticky block). The globe band
  // is observed too: its divider (layout L7) changes its height without a render of this view or a
  // resize of the scroller, and the rows start wherever the band now ends.
  useLayoutEffect(() => {
    const el = rowsWrapRef.current
    if (!el) return
    const measure = () => setListOffset(el.offsetTop)
    measure()
    const ro = new ResizeObserver(measure)
    if (el.parentElement) ro.observe(el.parentElement)
    if (globeRef.current) ro.observe(globeRef.current)
    return () => ro.disconnect()
  })

  /** THE ONE SAVE of an edit to a logged contact — the edit form's and the contact card's: one change
   *  through `editQsoById` (one history entry, one Undo), and the toast for each answer. A refusal
   *  changed nothing; the caller shows the contact as it now is. */
  const saveEdit = async (ref: RowRef, edit: QsoEdit, fields: FieldEdit[], call: string): Promise<RowAnswer | null> => {
    const answer = await withErrorToast(
      // The form's save sends no field changes, and its command is exactly the one it always was.
      () => (fields.length ? editQsoById(ref, edit, fields) : editQsoById(ref, edit)),
      t('logbook.form.saveFailed'),
    )
    if (!answer) return null
    if (answer.kind === 'busy') {
      // The logbook was too busy to take it: nothing saved, and what was typed stays, so Save
      // again tries the same change against the same version.
      pushToast(t('logbook.change.busy', { call }), 'error', 8000)
    } else if (answer.kind === 'changed') {
      // Changed since it was opened (another window, or a sync): nothing was saved.
      pushToast(t('logbook.change.changed', { call }), 'error', 8000)
      load()
    } else if (answer.kind === 'gone') {
      pushToast(t('logbook.change.gone', { call }), 'error', 8000)
      load()
    } else {
      pushToast(t('logbook.form.updated', { call: edit.call }), 'success')
      load()
    }
    return answer
  }

  /** Open the contact card on the contact `id`, as the engine holds it now. */
  const openCard = (id: string) => {
    void keyedRow(id)
      .catch(() => null)
      .then((found) => {
        if (!alive.current) return
        if (found) setCard({ row: found.row, ref: { id, editKey: found.editKey } })
        else {
          setCard(null)
          pushToast(t('logbook.form.editGone', { call: card?.row.call ?? '' }), 'error')
        }
      })
  }

  /** The contact `id` read again onto the card — only while the card is still open on it (a Redo
   *  pressed in a toast after the card closed opens nothing). */
  const reloadCard = (id: string) => {
    void keyedRow(id)
      .catch(() => null)
      .then((found) => {
        if (!alive.current || !found) return
        setCard((c) => (c?.row.id === id ? { row: found.row, ref: { id, editKey: found.editKey } } : c))
      })
  }

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    const call = draft.call.trim().toUpperCase()
    if (!call) {
      setErr(t('logbook.form.callRequired'))
      return
    }
    // The typed UTC instant (#280): the time box is free text, so it is checked here, and a
    // time that is not one is refused rather than guessed at. Both boxes blank keeps the
    // fallback below — now for a new contact, the stored time for an edit.
    const whenTyped = draft.whenDate.trim() !== '' || draft.whenTime.trim() !== ''
    const when = whenTyped ? utcDateTimeToUnix(draft.whenDate, draft.whenTime) : null
    if (whenTyped && when === null) {
      setErr(t('logbook.form.whenInvalid', { date: UTC_DATE_FORMAT, ...UTC_TIME_FORMATS }))
      return
    }
    if (endTimeBad) {
      // Named, not spread: `placeholders.test.ts` reads call sites literally and a spread is
      // one it has to skip, which is a guard this message would quietly cost coverage for.
      setErr(t('logbook.form.endInvalid', { short: UTC_TIME_FORMATS.short, long: UTC_TIME_FORMATS.long }))
      return
    }
    const freq = Number(draft.freq)
    const existing = editing?.row
    const parkTheirRef = draft.parkTheirRef.trim().toUpperCase() || null
    const parkMyRef = draft.parkMyRef.trim().toUpperCase() || null
    // The IOTA reference: refused unless it is one, rather than stored as junk no award counts.
    const iota = draft.iota.trim().toUpperCase() || null
    if (iota && !IOTA_RE.test(iota)) {
      setErr(t('logbook.form.iotaInvalid'))
      return
    }
    // The two park REFS and the IOTA reference are editable. The program (POTA/SOTA/WWFF) rides
    // through from the stored record. With every box blank we send no `ota` at all, so the
    // backend's ota-preserve guard restores the stored `ota` untouched (which is also why clearing
    // a park or an IOTA reference to REMOVE it does not take effect — blank means leave alone).
    const ota: LoggedQso['ota'] =
      parkTheirRef || parkMyRef || iota
        ? {
            theirRef: parkTheirRef,
            theirProgram: parkTheirRef ? existing?.ota?.theirProgram || 'POTA' : null,
            myRef: parkMyRef,
            myProgram: parkMyRef ? existing?.ota?.myProgram || 'POTA' : null,
            iota: iota ?? existing?.ota?.iota ?? null,
          }
        : undefined
    const startUnix = when ?? (existing ? existing.whenUnix : Math.floor(Date.now() / 1000))
    // #329: the end time is a time of day on the contact's OWN UTC date, so it is resolved
    // against the start above rather than parsed on its own. An end earlier in the day than
    // the start is the ROLLOVER — a contact that begins 23:58 and ends 00:03 ran five minutes,
    // not back 23h55m — which is exactly the case ADIF gives `QSO_DATE_OFF` a separate field
    // for. A blank box yields null and the record sends no end time at all: leave alone.
    const endTod = draft.endTime.trim() ? parseUtcTime(draft.endTime) : null
    const endUnix = endTod
      ? (() => {
          const midnight = Math.floor(startUnix / 86400) * 86400
          const end = midnight + endTod.h * 3600 + endTod.m * 60 + (endTod.s ?? 0)
          return end < startUnix ? end + 86400 : end
        })()
      : null
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
      whenUnix: startUnix,
      // #329. Blank sends `undefined`, which the backend reads as LEAVE ALONE — the stored
      // end time is restored by `Logbook::update_record`, so this box corrects one and never
      // clears one. That is why an empty box is not `null`.
      timeOffUnix: endUnix ?? undefined,
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
      // #239: your own grid and rig for this contact. Left empty on an edit, the backend keeps
      // what the record had (Logbook::update_record), as it does for the park refs.
      myGrid: draft.myGrid.trim().toUpperCase() || null,
      myRig: draft.myRig.trim() || null,
    }
    if (existing) {
      // The contact the form opened, by its id and the edit key of that version (never a new
      // contact: a form without one saves nothing).
      const ref = editing?.ref
      if (!ref) return
      // The record's fields as the form fills them, and — in the same ONE change — the QSL-sent
      // and paper-card marks (#239): the engine applies each only where it differs from the stored
      // contact, as the form's own follow-up commands used to. The park's programmes come from the
      // row opened; its IOTA, which no box shows, from the stored contact; a blank end time leaves
      // the stored one alone.
      const edit: QsoEdit = {
        call: record.call,
        grid: record.grid ?? null,
        state: record.state ?? null,
        band: record.band,
        freqMhz: record.freqMhz,
        mode: record.mode,
        rstSent: record.rstSent ?? null,
        rstRcvd: record.rstRcvd ?? null,
        name: record.name ?? null,
        qth: record.qth ?? null,
        comment: record.comment ?? null,
        notes: record.notes ?? null,
        txPower: record.txPower ?? null,
        whenUnix: record.whenUnix,
        timeOffUnix: endUnix,
        ota: {
          myProgram: ota?.myProgram ?? null,
          myRef: ota?.myRef ?? null,
          theirProgram: ota?.theirProgram ?? null,
          theirRef: ota?.theirRef ?? null,
          // Blank leaves the stored reference; the backend keeps it.
          iota,
        },
        myGrid: record.myGrid ?? null,
        myRig: record.myRig ?? null,
        qslSentVia: (draft.qslSentVia || null) as QsoEdit['qslSentVia'],
        qslCard: draft.qslCard === '1',
      }
      const answer = await saveEdit(ref, edit, [], existing.call)
      if (!answer || answer.kind === 'busy') return
      if (answer.kind === 'changed') {
        // The form now shows the contact as it is, and a save from it goes against that version.
        startEdit(answer.current.row, editing.key, { id: ref.id, editKey: answer.current.editKey })
        return
      }
      if (answer.kind === 'gone') {
        cancelForm()
        return
      }
      cancelForm()
      setDraft((prev) => ({ ...prev, call: '', grid: '', rstSent: '', rstRcvd: '', name: '', qth: '', comment: '', notes: '', whenDate: '', whenTime: '', endTime: '', state: '', txPower: '', parkTheirRef: '', parkMyRef: '', iota: '', myGrid: '', myRig: '', qslSentVia: '', qslCard: '' }))
      return
    }
    const snap = await withErrorToast(() => logQso(record), t('logbook.form.logFailed'))
    if (snap) {
      load()
      setShowForm(false)
      setDraft((prev) => ({ ...prev, call: '', grid: '', rstSent: '', rstRcvd: '', name: '', qth: '', comment: '', notes: '', whenDate: '', whenTime: '', endTime: '', state: '', txPower: '', iota: '', myGrid: '', myRig: '' }))
      // QRZ/ClubLog/eQSL auto-upload happens in the BACKEND log funnel now
      // (every log path, the engine auto-log included); outcomes toast via the
      // snapshot uploadTick.
    }
  }

  // RESIZING A COLUMN. The handle is the right edge of its header: dragged, the column takes a px
  // width (clamped as a saved one is) and the choice is saved once, on release; double-clicked, it
  // goes back to its share. Alt+←/→ on a focused header does the same from the keyboard, a step at
  // a time. The handle is not a control of its own: it sits inside the sort button, so a sort
  // header stays the one tab stop it was, and a drag never sorts (the click it ends is the
  // handle's, which goes no further).
  const resizeStart = useRef<{ id: string; x: number; px: number; zoom: number } | null>(null)
  const widthOf = (c: LogColumn, cell: HTMLElement | null) => colChoice.widths[c.id] ?? cell?.offsetWidth ?? c.minPx
  const resizeHandle = (c: LogColumn) =>
    c.id === 'actions' ? null : (
      <span
        className="log-col-resize"
        aria-hidden="true"
        onPointerDown={(e) => {
          if (e.button !== 0) return
          e.preventDefault()
          e.stopPropagation()
          e.currentTarget.setPointerCapture?.(e.pointerId)
          // Pointer travel is in window px; a width is in the zoomed table's own px.
          const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom'))
          resizeStart.current = {
            id: c.id,
            x: e.clientX,
            px: widthOf(c, e.currentTarget.parentElement),
            zoom: Number.isFinite(z) && z > 0 ? z : 1,
          }
        }}
        onPointerMove={(e) => {
          const r = resizeStart.current
          if (!r || r.id !== c.id) return
          setMadeColumns(withColumnWidth(colChoice, c.id, r.px + (e.clientX - r.x) / r.zoom, windowPx))
        }}
        onPointerUp={() => {
          if (resizeStart.current?.id !== c.id) return
          resizeStart.current = null
          changeColumns(colChoice)
        }}
        onPointerCancel={() => (resizeStart.current = null)}
        onClick={(e) => e.stopPropagation()}
        onDoubleClick={(e) => {
          e.stopPropagation()
          changeColumns(withColumnWidth(colChoice, c.id, null, windowPx))
        }}
      />
    )
  const resizeByKey = (e: React.KeyboardEvent<HTMLElement>, c: LogColumn) => {
    if (!e.altKey || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') || c.id === 'actions') return
    e.preventDefault()
    e.stopPropagation()
    const step = e.key === 'ArrowLeft' ? -RESIZE_STEP_PX : RESIZE_STEP_PX
    changeColumns(withColumnWidth(colChoice, c.id, widthOf(c, e.currentTarget) + step, windowPx))
  }
  const columnLabel = (c: LogColumn) => (c.id === 'qsl' ? QSL_HEADER : columnName(c))
  // The edit form's read-only line: the added columns the form has no box for, where the contact
  // holds a value.
  const alsoStored: [string, string][] = editing
    ? LOG_COLUMNS.filter((c) => !c.base && !c.more && !FORM_EDITS.has(c.id)).flatMap((c): [string, string][] => {
        const v =
          c.group === 'uploads'
            ? (() => {
                const st = uploadOf(editing.row, c.id)
                return st ? uploadWord(uploadMark(st)) : null
              })()
            : addedColumnText(editing.row, c.id)
        return v ? [[columnName(c), v]] : []
      })
    : []
  // A column the table did not have before the chooser: its text, or the upload mark.
  const addedCell = (c: LogColumn, q: LoggedQso) => {
    if (c.group === 'uploads') {
      const st = uploadOf(q, c.id)
      const mark = uploadMark(st)
      const word = uploadWord(mark)
      const title = st?.whenUnix ? t('logbook.upload.title', { state: word, date: fmtUtc(st.whenUnix) }) : word
      return (
        <span className={`log-cell log-upload ${mark}`} title={title} aria-label={title}>
          {UPLOAD_MARK[mark]}
        </span>
      )
    }
    const v = addedColumnText(q, c.id)
    return (
      <span className="log-cell mono" title={v ?? ''}>
        {v ?? '—'}
      </span>
    )
  }

  // A clickable, sort-toggling column header. Clicking the active column flips direction;
  // clicking a new column jumps to its sensible default direction; Shift adds it as the next key.
  const th = (c: LogColumn, k: SortKey) => {
    const at = sortPlace(sort, k)
    return (
      <button
        key={c.id}
        type="button"
        className={`log-cell log-th${at ? ' sorted' : ''}`}
        role="columnheader"
        aria-sort={at?.place === 1 ? (at.asc ? 'ascending' : 'descending') : 'none'}
        title={t('logbook.sort.shiftHint')}
        onClick={(e) => setSort((cur) => clickSort(cur, k, e.shiftKey, defaultAsc))}
        onKeyDown={(e) => resizeByKey(e, c)}
      >
        {columnLabel(c)}
        {at ? (at.asc ? ' ▲' : ' ▼') : ''}
        {at && (sort.then.length > 0) && <span className="log-th-place">{at.place}</span>}
        {resizeHandle(c)}
      </button>
    )
  }

  return (
    <section className="panel log-view logbook">
      <div className="panel-header log-header">
        <div className="log-title">
          <h2>{t('logbook.title')}</h2>
          {askedLogSize !== undefined && (sizeCurrent || askedLogSize > 0) && (
            <span className="count-badge" title={sizeCurrent ? undefined : t('logbook.count.stale')}>
              {logSize}
            </span>
          )}
          <span className="log-sub">{t('logbook.subtitle')}</span>
        </div>
        <div className="log-actions">
          <input
            ref={fileRef}
            type="file"
            accept=".adi,.adif,.adx,.cbr,.log,.txt"
            style={{ display: 'none' }}
            onChange={onImportFile}
          />
          <button
            type="button"
            className="export-btn"
            onClick={() => fileRef.current?.click()}
            title={t('logbook.import.title')}
          >
            {t('logbook.import.adif.label')}
          </button>
          <input
            ref={syncRef}
            type="file"
            accept=".adi,.adif,.txt"
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
            onClick={() => setShowParkStates(true)}
            title={t('logbook.parkStates.buttonTitle')}
          >
            {t('logbook.parkStates.button')}
          </button>
          <button
            type="button"
            className="export-btn"
            onClick={() => setShowConfirmations(true)}
            title={t('logbook.confirmations.buttonTitle')}
          >
            {t('logbook.confirmations.button')}
          </button>
          <button
            type="button"
            className="export-btn"
            onClick={() => void onDownloadConfirmations()}
            disabled={fetching !== null}
            title={t('logbook.confirmations.all.title')}
          >
            {fetchingAll ? t('logbook.confirmations.all.busy') : t('logbook.confirmations.all.label')}
          </button>
          <button
            type="button"
            className="export-btn"
            disabled={fetching !== null}
            onClick={async () => {
              // The authenticated in-app LoTW download also existed but hid in Settings
              // (same story as QRZ, same day): fetches lotwreport.adi with the keychain
              // credentials + the incremental high-water cursor, merges confirmations.
              if (fetching) return
              setFetching('lotw')
              const r = await withErrorToast(
                () => downloadLotwReport(),
                t('logbook.fetchLotw.failed'),
              ).finally(() => setFetching(null))
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
              className={`settings-input mono log-export-date${exportFromBad ? ' invalid' : ''}`}
              value={exportFrom}
              onChange={(e) => setExportFrom(e.target.value)}
              placeholder={UTC_DATE_FORMAT}
              maxLength={10}
              spellCheck={false}
              autoComplete="off"
              aria-invalid={exportFromBad}
            />
          </label>
          <label className="log-export-range" title={t('logbook.export.to.title')}>
            <span>{t('logbook.export.to.label')}</span>
            <input
              className={`settings-input mono log-export-date${exportToBad ? ' invalid' : ''}`}
              value={exportTo}
              onChange={(e) => setExportTo(e.target.value)}
              placeholder={UTC_DATE_FORMAT}
              maxLength={10}
              spellCheck={false}
              autoComplete="off"
              aria-invalid={exportToBad}
            />
          </label>
          <button
            type="button"
            className="export-btn"
            disabled={logSize === 0 || exportRangeBad}
            onClick={() =>
              withErrorToast(async () => {
                const exported = await exportGeneralLog('adif', exportFrom, exportTo)
                const text = exported.text
                // Count what the file actually holds — with a date range the log length lies.
                const n = (text.match(/<eor>/gi) ?? []).length
                if (n === 0) {
                  pushToast(t('logbook.export.nothing'), 'info')
                  sayExportLacks([exported])
                  return
                }
                const stamp = new Date().toISOString().slice(0, 10)
                const path = await saveTextToDownloads(`nexus-log-${stamp}.adi`, text)
                pushToast(t('logbook.export.done', { count: n, path }), 'success')
                sayExportLacks([exported])
              }, t('logbook.export.failed'))
            }
            title={
              exportRangeBad
                ? t('logbook.export.rangeInvalid', { date: UTC_DATE_FORMAT })
                : exportFrom || exportTo
                  ? t('logbook.export.adif.titleRange')
                  : t('logbook.export.adif.title')
            }
          >
            {t('logbook.export.adif.label')}
          </button>
          {/* The rows picked in the list — Ctrl/⌘-click, Shift-click, Space — written as the
              whole-log export writes each one. Beside the date range, not bounded by it. */}
          <button
            type="button"
            className="export-btn"
            disabled={picked.size === 0}
            onClick={() =>
              withErrorToast(async () => {
                const exported = await exportSelectedLog([...picked])
                const text = exported.text
                const n = (text.match(/<eor>/gi) ?? []).length
                if (n === 0) {
                  pushToast(t('logbook.export.nothing'), 'info')
                  sayExportLacks([exported])
                  return
                }
                const stamp = new Date().toISOString().slice(0, 10)
                const path = await saveTextToDownloads(`nexus-log-selected-${stamp}.adi`, text)
                pushToast(t('logbook.export.done', { count: n, path }), 'success')
                sayExportLacks([exported])
              }, t('logbook.export.failed'))
            }
            title={t('logbook.export.selected.title')}
          >
            {t('logbook.export.selected.label', { count: picked.size })}
          </button>
          {/* Per-activation export — the file POTA actually asks for. The date range above
              cannot produce it: an activator who worked a park in the afternoon and ragchewed
              from home in the morning has both in one UTC day, and "today to today" exports
              both (2026-09-09 report). The unit here is (your park) × (UTC day) × (the callsign
              you signed), which is what POTA credits and what its filename names — so two parks
              in one day are simply two rows and two files.

              ⚠️ Choosing an activation SUPERSEDES the date range; it does not intersect with it.
              An activation is already a bounded thing, so an intersection could only ever make
              the file SHORTER than the activation — silently, and after upload. That is the same
              ruling the per-operator export below already carries, for the same reason. Shown
              only when the log holds an activation at all: for a hunter-only station this button
              has nothing to offer. */}
          {activations.length > 0 && (
            <>
              <label className="log-export-range" title={t('logbook.export.activation.title')}>
                <span>{t('logbook.export.activation.label')}</span>
                <select
                  className="settings-input log-export-activation"
                  value={activationKey}
                  onChange={(e) => setActivationKey(e.target.value)}
                >
                  <option value="">{t('logbook.export.activation.none')}</option>
                  {activations.map((a) => (
                    <option key={activationKeyOf(a)} value={activationKeyOf(a)}>
                      {t('logbook.export.activation.option', {
                        // Callsign · park · date — the filename's own three fields, in the order
                        // POTA writes them, so the operator can see which file they will get.
                        activation: [a.callsign, a.reference, a.date].filter(Boolean).join(' · '),
                        count: a.qsos,
                      }) +
                        (a.qsos < POTA_MIN_QSOS
                          ? t('logbook.export.activation.short', { min: POTA_MIN_QSOS })
                          : '')}
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="button"
                className="export-btn"
                disabled={!activationKey}
                onClick={() =>
                  withErrorToast(async () => {
                    const a = activations.find((x) => activationKeyOf(x) === activationKey)
                    if (!a) return
                    await exportActivation(a)
                  }, t('logbook.export.failed'))
                }
                title={t('logbook.export.activation.buttonTitle')}
              >
                {t('logbook.export.activation.button')}
              </button>
            </>
          )}
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
                  const exported: LogExport[] = []
                  for (const op of operators) {
                    const file = await exportLogForOperator(op)
                    exported.push(file)
                    const safe = op.replace(/[^A-Za-z0-9]+/g, '-')
                    saved.push(await saveTextToDownloads(`nexus-log-${stamp}-${safe}.adi`, file.text))
                  }
                  // The combined file as well, always: it is the only one that carries contacts
                  // logged with no operator set, and it is what the station itself uploads.
                  const all = await exportGeneralLog('adif')
                  exported.push(all)
                  saved.push(await saveTextToDownloads(`nexus-log-${stamp}.adi`, all.text))
                  pushToast(t('logbook.export.perOperator.done', { count: saved.length }), 'success')
                  sayExportLacks(exported)
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
            disabled={logSize === 0 || exportRangeBad}
            onClick={() =>
              withErrorToast(async () => {
                const exported = await exportGeneralLog('csv', exportFrom, exportTo)
                const text = exported.text
                // Rows minus the header — with a date range the log length lies.
                const n = Math.max(0, text.trim().split('\n').length - 1)
                if (n === 0) {
                  pushToast(t('logbook.export.nothing'), 'info')
                  sayExportLacks([exported])
                  return
                }
                const stamp = new Date().toISOString().slice(0, 10)
                const path = await saveTextToDownloads(`nexus-log-${stamp}.csv`, text)
                pushToast(t('logbook.export.done', { count: n, path }), 'success')
                sayExportLacks([exported])
              }, t('logbook.export.failed'))
            }
            title={
              exportRangeBad
                ? t('logbook.export.rangeInvalid', { date: UTC_DATE_FORMAT })
                : exportFrom || exportTo
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
          <button type="button" className="export-btn" onClick={() => setShowDeleted(true)}>
            {t('logbook.deleted.button')}
          </button>
          <button type="button" className="export-btn" onClick={() => setTools('dupes')}>
            {t('logbook.tools.button')}
          </button>
          <button type="button" className="export-btn" onClick={() => setTools('callbook')}>
            {t('logbook.tools.callbook.button')}
          </button>
          <button
            type="button"
            className="export-btn danger"
            onClick={() => setShowPurge(true)}
            disabled={logSize === 0}
            title={t('logbook.purge.title')}
          >
            {t('logbook.purge.label')}
          </button>
        </div>
      </div>


      {showForm && (
        <form
          className="logbook-form"
          onSubmit={submit}
          ref={formRef}
          onKeyDown={(e) => {
            if (e.key !== 'Escape') return
            e.preventDefault()
            cancelForm()
          }}
        >
          <div className="logbook-form-grid">
            <label className="logbook-field logbook-field-call">
              <span>{t('logbook.field.call.label')}</span>
              <div className="settings-input-row">
                <input
                  className="settings-input"
                  value={draft.call}
                  onChange={(e) => setField('call', e.target.value)}
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
              <input className="settings-input" value={draft.grid} onChange={(e) => setField('grid', e.target.value)} autoComplete="off" spellCheck={false} />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.band.label')}</span>
              <input className="settings-input" value={draft.band} onChange={(e) => setField('band', e.target.value)} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.freq.label')}</span>
              <input className="settings-input" type="number" step="0.0001" value={draft.freq} onChange={(e) => setField('freq', e.target.value)} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.mode.label')}</span>
              <input className="settings-input" value={draft.mode} onChange={(e) => setField('mode', e.target.value)} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.rstSent.label')}</span>
              <input className="settings-input" value={draft.rstSent} onChange={(e) => setField('rstSent', e.target.value)} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.rstRcvd.label')}</span>
              <input className="settings-input" value={draft.rstRcvd} onChange={(e) => setField('rstRcvd', e.target.value)} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.date.label')}</span>
              {/* UTC as typed, beside the time and for the same reason (#280): a native date
                  control is drawn in the OS locale and its "Today" fills the LOCAL date, which
                  after 0000Z west of Greenwich is the previous UTC day. */}
              <input
                className={`settings-input mono logbook-when${whenDateBad ? ' invalid' : ''}`}
                value={draft.whenDate}
                onChange={(e) => setField('whenDate', e.target.value)}
                placeholder={UTC_DATE_FORMAT}
                maxLength={10}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={whenDateBad}
                title={t('logbook.field.when.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.time.label')}</span>
              {/* 24-hour UTC as typed — a native time control would draw it in the OS locale (#280). */}
              <input
                className={`settings-input mono${whenTimeBad ? ' invalid' : ''}`}
                value={draft.whenTime}
                onChange={(e) => setField('whenTime', e.target.value)}
                placeholder={UTC_TIME_FORMATS.short}
                maxLength={8}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={whenTimeBad}
                title={t('logbook.field.when.title')}
              />
            </label>
            {/* #329: the contact's END, beside the time it started. */}
            <label className="logbook-field">
              <span>{t('logbook.field.end.label')}</span>
              <input
                className={`settings-input mono${endTimeBad ? ' invalid' : ''}`}
                value={draft.endTime}
                onChange={(e) => setField('endTime', e.target.value)}
                placeholder={UTC_TIME_FORMATS.short}
                maxLength={8}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={endTimeBad}
                title={t('logbook.field.end.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.state.label')}</span>
              <input
                className="settings-input"
                value={draft.state}
                onChange={(e) => setField('state', e.target.value)}
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
                autoComplete="off"
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.parkTheirs.label')}</span>
              <input
                className="settings-input"
                value={draft.parkTheirRef}
                onChange={(e) => setField('parkTheirRef', e.target.value)}
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
                autoComplete="off"
                title={t('logbook.field.parkMine.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.iota.label')}</span>
              <input
                className="settings-input"
                value={draft.iota}
                onChange={(e) => setField('iota', e.target.value)}
                autoComplete="off"
                spellCheck={false}
                maxLength={6}
                aria-invalid={!!draft.iota.trim() && !IOTA_RE.test(draft.iota.trim().toUpperCase())}
                title={t('logbook.field.iota.title')}
              />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.name.label')}</span>
              <input className="settings-input" value={draft.name} onChange={(e) => setField('name', e.target.value)} autoComplete="off" />
            </label>
            <label className="logbook-field">
              <span>{t('logbook.field.qth.label')}</span>
              <input className="settings-input" value={draft.qth} onChange={(e) => setField('qth', e.target.value)} autoComplete="off" />
            </label>
            {/* #239: your own location and rig for THIS contact (ADIF MY_GRIDSQUARE / MY_RIG). */}
            <>
              <label className="logbook-field">
                <span>{t('logbook.field.myGrid.label')}</span>
                <input
                  className="settings-input"
                  value={draft.myGrid}
                  onChange={(e) => setField('myGrid', e.target.value)}
                  title={t('logbook.field.myGrid.title')}
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
              <label className="logbook-field">
                <span>{t('logbook.field.myRig.label')}</span>
                <input
                  className="settings-input"
                  value={draft.myRig}
                  onChange={(e) => setField('myRig', e.target.value)}
                  title={t('logbook.field.myRig.title')}
                  autoComplete="off"
                />
              </label>
            </>
            {/* #239: QSL status where the rest of the contact is edited — saved in the SAME change
                as the fields (`editQsoById`), and only where it differs from the stored contact.
                LoTW and eQSL confirmations stay the services'; only the card is the operator's to
                mark. Edit mode only: both marks address a logged record. */}
            {editingKey !== null && (
              <>
                <div className="logbook-field">
                  <label htmlFor="logbook-qsl-sent">{t('logbook.field.qslSent.label')}</label>
                  <select
                    id="logbook-qsl-sent"
                    className="settings-input"
                    value={draft.qslSentVia}
                    onChange={(e) => setField('qslSentVia', e.target.value)}
                  >
                    <option value="">{t('logbook.field.qslSent.none')}</option>
                    {draft.qslSentVia === 'SENT' && (
                      <option value="SENT">{t('logbook.field.qslSent.sent')}</option>
                    )}
                    <option value="B">{qslViaLabel('B')}</option>
                    <option value="D">{qslViaLabel('D')}</option>
                    <option value="E">{qslViaLabel('E')}</option>
                  </select>
                </div>
                <label className="logbook-field">
                  <span>{t('logbook.field.qslCard.label')}</span>
                  <input
                    type="checkbox"
                    checked={draft.qslCard === '1'}
                    onChange={(e) => setField('qslCard', e.target.checked ? '1' : '')}
                  />
                </label>
              </>
            )}
            {/* The shared comment: full width, always directly above Notes (it was a 140 px cell
                among the short fields). It wraps over lines but holds ONE, as ADIF's COMMENT does:
                a line break typed or pasted lands as a space, and Enter saves as the old box did.
                A stored comment with line breaks (an import's) is shown and saved as it is unless
                the operator edits this box. */}
            <label className="logbook-field logbook-field-wide">
              <span>{t('logbook.field.comment.label')}</span>
              <textarea
                className="settings-input logbook-comment"
                value={draft.comment}
                onChange={(e) => setField('comment', oneLineTyped(e.target))}
                onKeyDown={(e) => {
                  if (e.key !== 'Enter') return
                  e.preventDefault()
                  e.currentTarget.form?.requestSubmit()
                }}
                placeholder={t('logbook.field.comment.placeholder')}
                rows={2}
                autoComplete="off"
              />
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
            {/* What the contact carries that no box here edits — the zones, the county, the QSL
                dates, the upload state and the contest exchange — shown so an edit is made with
                the whole contact in view. Read-only: each is the record's or a service's. */}
            {alsoStored.length > 0 && (
              <div className="logbook-field logbook-field-wide logbook-also">
                <span>{t('logbook.form.alsoStored')}</span>
                <dl>
                  {alsoStored.map(([label, value]) => (
                    <div key={label}>
                      <dt>{label}</dt>
                      <dd className="mono">{value}</dd>
                    </div>
                  ))}
                </dl>
              </div>
            )}
          </div>
          <div className="logbook-form-actions">
            {err && <span className="settings-error" role="alert">{err}</span>}
            {editingKey !== null && (
              <span className="logbook-editing-note">{t('logbook.form.editingNote')}</span>
            )}
            {/* #239: leave the form without saving, as Esc does. */}
            <button type="button" className="logconfirm-discard" onClick={cancelForm}>
              {t('logbook.form.cancel')}
            </button>
            <button type="submit" className="settings-save" disabled={!draft.call.trim()}>
              {editingKey !== null ? t('logbook.form.save') : t('logbook.form.log')}
            </button>
          </div>
        </form>
      )}

      <div
        className={`log-table logbook-table${tableMinW ? ' wide' : ''}`}
        // The one template the header and every row take (`.logbook-table .logbook-row`), and a
        // wide table's floor: cut from the same column list the cells are.
        style={{ '--log-cols': gridTemplate, ...(tableMinW ? { '--log-min-w': tableMinW } : {}) } as React.CSSProperties}
        role="grid"
        aria-multiselectable
        aria-rowcount={listTotal + 1}
        aria-describedby={keysId}
      >
        {/* The grid's keys, for a screen reader: four whole sentences, never one glued. */}
        <p id={keysId} className="sr-only">
          <span>{t('logbook.keys.move')}</span> <span>{t('logbook.keys.edit')}</span>{' '}
          <span>{t('logbook.keys.delete')}</span> <span>{t('logbook.keys.close')}</span>
        </p>
        <div className="log-scroll" ref={scrollRef}>
          {globeShown && (
            <div className="log-globe-band" ref={globeRef}>
              {/* Where the globe cannot draw, or fails once drawn, the band says so and the log carries on. */}
              <GlobeGuard noWebgl2={t('logbook.globe.noWebgl2')} failed={t('logbook.globe.failed')}>
                <Suspense fallback={<div className="log-globe-loading">{t('logbook.globe.loading')}</div>}>
                  <QsoGlobe logTick={logTick} />
                </Suspense>
              </GlobeGuard>
            </div>
          )}
          {/* The globe | table divider (layout L7): the band's height, a % of the scroller, the
              band's own 320 px until the operator moves it. It scrolls away with the band. */}
          {globeShown && (
            <PaneSeam
              axis="y"
              varName="--log-globe-h"
              strip={globeRef}
              storageKey="nexus.split.logbook.globe"
              min={LOG_GLOBE_SPLIT_MIN}
              max={LOG_GLOBE_SPLIT_MAX}
              maxShare={Infinity}
              defaultPct={null}
              label={t('logbook.globe.height.label')}
            />
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
        <LogFilterBar filter={filter} onChange={setFilter} />
        {/* THE ACTIVATION'S OWN VIEW: the same rows, scoped — its parks' contacts on each UTC day it has
            run. A filter like any other, so Filters ▸ Save keeps it under a name. */}
        {thisActivation && (
          <button
            type="button"
            className={`log-filter-chip${filterKey(filter) === filterKey(thisActivation) ? ' active' : ''}`}
            aria-pressed={filterKey(filter) === filterKey(thisActivation)}
            onClick={() => setFilter(filterKey(filter) === filterKey(thisActivation) ? {} : thisActivation)}
            title={t('logbook.filters.activation.title', { reference: activation?.reference ?? '' })}
          >
            {t('logbook.filters.activation.label')}
          </button>
        )}
        {/* The column chooser (features/logColumns.ts). It took the place of "More columns" (#239),
            which showed eight extra columns or none. */}
        <LogColumnsMenu choice={colChoice} onChange={changeColumns} onReset={resetColumns} />
        {search.trim() && (
          <button type="button" className="log-search-clear" onClick={() => setSearch('')} title={t('logbook.search.clear')}>
            ✕
          </button>
        )}
      </div>
        <div className="log-row logbook-row head" role="row" aria-rowindex={1}>
          {/* #239 (swinn): "put the time and dates in their own fields". Both the Date and the
              Time header sort on the SAME key, because a record carries ONE instant: there is no
              ordering by time of day that ignores the date, and a header that looks like its
              neighbours while doing nothing is worse than two that say they order the same thing.
              Free-text columns do not sort: sorting a log by remark answers no question an
              operator asks. Of the passthrough tags only the county sorts, which has an index. */}
          {cols.map((c) =>
            c.sort ? (
              th(c, c.sort)
            ) : c.id === 'actions' ? (
              <span key={c.id} className="log-cell" role="columnheader" aria-label={t('logbook.column.actions')}></span>
            ) : (
              <span
                key={c.id}
                className="log-cell"
                role="columnheader"
                // An added column's header takes the keyboard's resize (Alt+←/→); a default one
                // keeps the tab order it always had.
                tabIndex={c.base ? undefined : 0}
                onKeyDown={(e) => resizeByKey(e, c)}
              >
                {columnLabel(c)}
                {resizeHandle(c)}
              </span>
            ),
          )}
        </div>
          </div>
          {listSays === 'failed' && (
            <p className="empty">
              {t('logbook.readFailed', { reason: readFailed ?? '' })}{' '}
              <button type="button" className="log-filter-chip" onClick={load}>
                {t('logbook.readFailed.retry')}
              </button>
            </p>
          )}
          {listSays === 'reading' && <p className="empty">{t('logbook.reading')}</p>}
          {listSays === 'empty' && <p className="empty">{t('logbook.empty')}</p>}
          {listSays === 'noMatch' && (
            <p className="empty">{t('logbook.emptySearch', { query: deferredSearch.trim() })}</p>
          )}
          {listTotal > 0 && (
            <div
              ref={rowsWrapRef}
              className="log-rows"
              style={{ height: rowVirtualizer.getTotalSize(), position: 'relative', width: '100%' }}
              onKeyDown={onGridKey}
            >
              {virtualRows.map((vrow) => {
                // A row whose page is still on its way: its place, kept, until the page lands.
                const row = rowAt(vrow.index)
                const placed = {
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  // scrollMargin is baked into vrow.start (the globe band above);
                  // subtract it because rows are positioned within THIS container,
                  // which document flow already places below the band.
                  transform: `translateY(${vrow.start - rowVirtualizer.options.scrollMargin}px)`,
                } as const
                if (!row)
                  return (
                    <div
                      className="log-row logbook-row placeholder"
                      role="row"
                      aria-busy="true"
                      aria-rowindex={vrow.index + 2}
                      tabIndex={-1}
                      key={`placeholder-${vrow.index}`}
                      data-index={vrow.index}
                      ref={rowVirtualizer.measureElement}
                      style={placed}
                    >
                      <span className="log-cell">{t('logbook.rows.loading')}</span>
                      {/* The row buttons' own box, unseen: a row's height is its 22 px buttons
                          plus padding, so a placeholder holding one is exactly as tall as the row
                          it stands for — nothing below it moves when the row lands, and the list
                          never measures a short row it then has to correct. */}
                      <span className="log-cell log-rowactions" aria-hidden="true">
                        <span className="log-rowbtn" style={{ visibility: 'hidden' }} />
                      </span>
                    </div>
                  )
                const { q, key, ref } = row
                const isPicked = !!q.id && picked.has(q.id)
                // One cell per column the table shows (features/logColumns.ts): the header and the
                // template are cut from the same list, so a cell cannot go without its track.
                const rowCell = (c: LogColumn) => {
                  switch (c.id) {
                    case 'call':
                      return (
                        <>
                          <span className="log-cell mono">
                            {/* The station card (its QRZ button opens the page this button used to). */}
                            <button
                              type="button"
                              className="qrz-link-call"
                              {...callCardTrigger(q.call, qrzPageOf(q.call))}
                            >
                              {q.call}
                            </button>
                          </span>
                        </>
                      )
                    case 'country':
                      return (
                        <>
                          {/* The country is the DXCC entity the contact is shown under (`q.entity`): the one
                              its call resolves to, or the one its record's DXCC number names where that is
                              another — a DXpedition on a 1×1 call, a deleted entity, a number set here by
                              hand. Imported contacts carry their logger's own spelling of it ("UNITED
                              STATES"), and they read as the same country as Nexus's own. The stored text
                              stays on the tooltip, and shows where neither names an entity. On the station
                              the cell is the picker that sets the number (LogCountryPicker). */}
                          <span className="log-cell log-country" title={q.country ?? ''}>
                            {ref ? (
                              <LogCountryPicker q={q} onPick={(dxcc) => void onSetDxcc(q, ref, dxcc)} />
                            ) : (
                              (q.entity ?? q.country ?? '—')
                            )}
                          </span>
                        </>
                      )
                    case 'band':
                      return (
                        <>
                          <span className="log-cell">{q.band}</span>
                        </>
                      )
                    case 'freq':
                      return (
                        <>
                          <span className="log-cell mono">{q.freqMhz.toFixed(4)}</span>
                        </>
                      )
                    case 'mode':
                      return (
                        <>
                          <span className="log-cell">{q.mode}</span>
                        </>
                      )
                    case 'sent':
                      return (
                        <>
                          <span className="log-cell mono">{fmtReport(q.rstSent)}</span>
                        </>
                      )
                    case 'rcvd':
                      return (
                        <>
                          <span className="log-cell mono">{fmtReport(q.rstRcvd)}</span>
                        </>
                      )
                    case 'date':
                      return (
                        <>
                          <span className="log-cell mono">{utcDate(q.whenUnix)}</span>
                        </>
                      )
                    case 'time':
                      return (
                        <>
                          {/* #239: the time of day, no `Z` — the header says UTC and the End column
                              (#329) next door already prints a bare `utcTime`.
                              ⚠️ `timeKnown: false` is an imported record whose source carried a DATE and
                              no time; `whenUnix` anchors it at midnight FOR ORDERING and its clock
                              reading is an artefact. The combined column printed that artefact as fact —
                              a column of its own cannot. the contact card already refuses the same thing, and
                              a bare 00:00 read as fact is what leaves those contacts unmatched at LoTW
                              and eQSL forever. */}
                          <span
                            className="log-cell mono"
                            title={q.timeKnown === false ? t('logbook.row.time.dateOnly') : ''}
                          >
                            {q.timeKnown === false ? '—' : utcTime(q.whenUnix)}
                          </span>
                        </>
                      )
                    case 'park':
                      return (
                        <>
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
                            {/* Before the reference, so a narrow Park column clips the reference and
                                never the mark. */}
                            {needsParkState(q) && (
                              <span
                                className="log-park-nostate"
                                role="img"
                                aria-label={t('logbook.row.park.noState')}
                                title={t('logbook.row.park.noState')}
                              />
                            )}
                            {q.ota?.theirRef ?? (q.ota?.myRef ? `@${q.ota.myRef}` : '—')}
                          </span>
                        </>
                      )
                    case 'qsl':
                      return (
                        <>
                          <span className="log-cell">
                            {q.qslRcvd &&
                            (q.qslRcvd.card || q.qslRcvd.lotw || q.qslRcvd.eqsl || q.qslRcvd.qrz) ? (
                              // Per-source detail: which channel(s) actually confirmed. QRZ has its own
                              // letter: a QRZ-only confirmation fell through to the yellow eQSL word below.
                              <span
                                className={`log-qsl ${q.awardConfirmed ? 'ok' : 'eqsl'}`}
                                title={[
                                  q.qslRcvd.lotw ? t('logbook.row.qsl.lotw') : null,
                                  q.qslRcvd.card ? t('logbook.row.qsl.card') : null,
                                  q.qslRcvd.eqsl ? t('logbook.row.qsl.eqsl') : null,
                                  q.qslRcvd.qrz ? t('logbook.row.qsl.qrz') : null,
                                ]
                                  .filter(Boolean)
                                  .join(' · ')}
                              >
                                {[
                                  q.qslRcvd.lotw ? 'L' : null,
                                  q.qslRcvd.card ? 'C' : null,
                                  q.qslRcvd.eqsl ? 'E' : null,
                                  q.qslRcvd.qrz ? 'Q' : null,
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
                        </>
                      )
                    case 'end':
                      return (
                        <>
                              {/* #329: the contact's end time. The cell shows the time of day only —
                                  the date is already in the Date column — so the tooltip carries the
                                  full UTC instant for a contact that ran past midnight. */}
                              <span className="log-cell mono" title={q.timeOffUnix != null ? fmtUtc(q.timeOffUnix) : ''}>
                                {q.timeOffUnix != null ? utcTime(q.timeOffUnix) : '—'}
                              </span>
                        </>
                      )
                    case 'myGrid':
                      return (
                        <>
                              <span className="log-cell mono">{q.myGrid ?? '—'}</span>
                        </>
                      )
                    case 'myRig':
                      return (
                        <>
                              <span className="log-cell" title={q.myRig ?? ''}>{q.myRig ?? '—'}</span>
                        </>
                      )
                    case 'name':
                      return (
                        <>
                              <span className="log-cell" title={q.name ?? ''}>{q.name ?? '—'}</span>
                        </>
                      )
                    case 'qth':
                      return (
                        <>
                              <span className="log-cell" title={q.qth ?? ''}>{q.qth ?? '—'}</span>
                        </>
                      )
                    case 'state':
                      return (
                        <>
                              <span className="log-cell mono">{q.state ?? '—'}</span>
                        </>
                      )
                    case 'power':
                      return (
                        <>
                              <span className="log-cell mono">{q.txPower ?? '—'}</span>
                        </>
                      )
                    case 'operator':
                      return (
                        <>
                              <span className="log-cell mono">{q.operator ?? '—'}</span>
                        </>
                      )
                    case 'notes':
                      return (
                        <>
                          {/* Comment + private note. Both were WRITE-ONLY here until 2026-08-23: the edit
                              form took them and the table never showed them back, so the only way to see
                              a note was to open the row you already had to guess held one (operator:
                              "how else do you remember the things you talked about in the last QSOs?").
                              The comment is short by design and shows inline; the private note is
                              multi-line and gets a 📝 marker with the text in the tooltip, the same
                              idiom the callsign-recall card already uses. */}
                          <span
                            className={`log-cell log-note${openComments.has(key) ? ' expanded' : ''}`}
                            title={[
                              (q.comment ?? '').trim() && `${t('logbook.row.notes.title')}: ${(q.comment ?? '').trim()}`,
                              (q.notes ?? '').trim() && `${t('logbook.row.notes.private')}: ${(q.notes ?? '').trim()}`,
                            ]
                              .filter(Boolean)
                              .join('\n\n')}
                          >
                            {/* #162: so is the 📝 flag. The private note was a marker and a tooltip and
                                nothing else — the one thing in this cell with no way to read it in the
                                row, which is the half of the issue the comment fix left open. It opens
                                the same cell the comment does, because the cell is the clip container:
                                one row, one open state, and a row holding both shows both. */}
                            {(q.notes ?? '').trim() && (
                              <button
                                type="button"
                                className="log-note-flag"
                                aria-expanded={openComments.has(key)}
                                aria-label={t('logbook.row.notes.readAria')}
                                onClick={() => toggleComment(key)}
                              >
                                📝
                              </button>
                            )}
                            {/* #162: the comment is its own toggle — its text is the accessible name, and
                                aria-expanded says whether the row shows it whole. */}
                            {(q.comment ?? '').trim() ? (
                              <button
                                type="button"
                                className="log-note-text"
                                aria-expanded={openComments.has(key)}
                                onClick={() => toggleComment(key)}
                              >
                                {(q.comment ?? '').trim()}
                              </button>
                            ) : (q.notes ?? '').trim() ? (
                              ''
                            ) : (
                              '—'
                            )}
                            {/* #162: the note itself, and only when the cell is open. Collapsed it is
                                not rendered at all, so the single line the column gets still belongs to
                                the comment — the note is multi-line free text and would win a fight for
                                it. Open, it takes a line of its own below (`flex: 1 0 100%`). */}
                            {openComments.has(key) && (q.notes ?? '').trim() && (
                              <span className="log-note-private">{(q.notes ?? '').trim()}</span>
                            )}
                          </span>
                        </>
                      )
                    case 'actions':
                      return (
                        <>
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
                              {/* #270: this was a bare ↥, and an operator looking for "Upload to QRZ"
                                  reported the button as gone. Named like its CL / HL / WRL siblings. */}
                              {QRZ_LABEL}
                            </button>
                            <button
                              type="button"
                              className="log-rowbtn"
                              onClick={() => void onCorrectQrz(q)}
                              title={t('logbook.row.qrzCorrect.title', { call: q.call })}
                              aria-label={t('logbook.row.qrzCorrect.aria', { call: q.call })}
                            >
                              {QRZ_CORRECT_LABEL}
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
                            <button
                              type="button"
                              className="log-rowbtn"
                              onClick={() => void onPushWrl(q)}
                              title={t('logbook.row.pushWrl.title', { call: q.call })}
                              aria-label={t('logbook.row.pushWrl.aria', { call: q.call })}
                            >
                              {WRL_LABEL}
                            </button>
                            {/* QSL handling for the row: mark a request SENT (once), and record the
                                paper card that came BACK. Operator-declared, not a confirmation.
                                
                                ⚠️ THE GATE USED TO BE `needsConfirmOnly && !q.qslSent?.sent`, and both
                                halves were wrong (#152, reported again after the 1.8.0 fix shipped).
                                
                                The filter half hid the whole menu unless the "needs confirmation" chip
                                happened to be on, so the reporter looked in the ordinary Logbook — where
                                anyone handling a stack of cards is — and found nothing. A fix nobody can
                                reach is not a fix.
                                
                                The sent half is worse: a paper QSL is a ROUND TRIP. You send, you wait
                                months, a card arrives. Removing the menu the moment you marked one sent
                                deleted the control for the arrival, so the very card the feature exists
                                to record could never be recorded. `q.qslSent?.sent` now hides only the
                                three SEND entries — you still cannot send twice — while the inbound
                                entries stay reachable for the life of the contact. */}
                            <select
                              className="log-rowbtn"
                              style={{ fontSize: '0.85em' }}
                              value=""
                              onChange={(e) => {
                                const v = e.target.value as 'B' | 'D' | 'E' | 'R' | 'r' | 's' | ''
                                // R/r/s are NOT ADIF letters — they are this menu's own entries for
                                // the RECEIVED card (#152), which has no QSL_SENT_VIA code because it
                                // is not a send at all, and for CLEARING the send (#180). Kept in the
                                // same menu because an operator handling a card thinks about one row,
                                // not two controls.
                                if (v === 'R') void onMarkQslCard(q, ref, true)
                                else if (v === 'r') void onMarkQslCard(q, ref, false)
                                else if (v === 's') void onMarkQslSent(q, ref, null)
                                else if (v) void onMarkQslSent(q, ref, v as 'B' | 'D' | 'E')
                              }}
                              title={t('logbook.row.qslSent.title', { call: q.call })}
                              aria-label={t('logbook.row.qslSent.aria', { call: q.call })}
                            >
                              {/* The VALUES are the ADIF QSL_SENT_VIA letters; only the labels are prose. */}
                              <option value="">{QSL_MENU_LABEL}</option>
                              {/* Sending is once-only; the arrival below is not. */}
                              {!q.qslSent?.sent && (
                                <>
                                  <option value="B">{t('logbook.row.qslSent.bureau')}</option>
                                  <option value="D">{t('logbook.row.qslSent.direct')}</option>
                                  <option value="E">{t('logbook.row.qslSent.electronic')}</option>
                                </>
                              )}
                              {/* ...and the way back out of it (#180). It has to be shown on exactly
                                  the condition that HIDES the three above: a clear that disappears
                                  once sent would be the same trap with an extra step in it. */}
                              {q.qslSent?.sent && (
                                <option value="s">{t('logbook.row.qslSent.clear')}</option>
                              )}
                              {/* INBOUND: the paper card that arrived. Nothing on the internet can
                                  report this, so the operator is the only source — and it is
                                  award-eligible (card OR LoTW), which is why its absence understated
                                  the awards view. The clear entry exists for a mis-tick. */}
                              <option value="R">{t('logbook.row.qslRcvd.card')}</option>
                              {q.qslRcvd?.card && (
                                <option value="r">{t('logbook.row.qslRcvd.clear')}</option>
                              )}
                            </select>
                            {/* SATELLITE TAG (PROP_MODE=SAT + SAT_NAME). The guide used to send an
                                operator who mis-tagged a contact out of the app — close Nexus, open
                                log.adi in a text editor — because the edit form carries neither field.
                                It still does not, deliberately: a blank field there means LEAVE ALONE,
                                which is what stops a busted-call fix stripping a tag the contact earned.
                                So the repair lives here, where choosing "Clear" is unmistakably a
                                decision and not a blank box submitted with everything else.

                                Both halves of the pair move together in the backend: TQSL rejects a lone
                                member and takes the batch with it. */}
                            {satNames.length > 0 && (
                              <select
                                className="log-rowbtn"
                                style={{ fontSize: '0.85em' }}
                                value=""
                                onChange={(e) => {
                                  const v = e.target.value
                                  // '' is the PLACEHOLDER — the state the select sits in when nothing
                                  // has been chosen. A non-choice must never clear a tag, so only the
                                  // explicit 'clear' entry passes null.
                                  if (v === 'clear') void onSetSatTag(q, ref, null)
                                  else if (v) void onSetSatTag(q, ref, v)
                                }}
                                title={t('logbook.row.sat.title', { call: q.call })}
                                aria-label={t('logbook.row.sat.aria', { call: q.call })}
                              >
                                <option value="">{SAT_MENU_LABEL}</option>
                                {satNames.map((n) => (
                                  <option key={n} value={n}>
                                    {n}
                                  </option>
                                ))}
                                {/* Shown when EITHER field is set: a record carrying a lone PROP_MODE=SAT
                                    or a lone SAT_NAME is the half-pair TQSL rejects, and clearing it is
                                    exactly the repair that record needs. */}
                                {(q.propMode || q.satName) && (
                                  <option value="clear">{t('logbook.row.sat.clear')}</option>
                                )}
                              </select>
                            )}
                            <button
                              type="button"
                              className="log-rowbtn"
                              onClick={() => {
                                formFromGrid.current = key
                                formWanted.current = true
                                startEdit(q, key, ref)
                              }}
                              title={t('logbook.row.edit', { call: q.call })}
                              aria-label={t('logbook.row.edit', { call: q.call })}
                            >
                              ✎
                            </button>
                            <button
                              type="button"
                              className="log-rowbtn danger"
                              onClick={() => onDelete(q, key, ref)}
                              title={t('logbook.row.delete', { call: q.call })}
                              aria-label={t('logbook.row.delete', { call: q.call })}
                            >
                              ✕
                            </button>
                          </span>
                        </>
                      )
                    default:
                      return addedCell(c, q)
                  }
                }
                return (
                  <div
                    className={`log-row logbook-row${editingKey === key ? ' editing' : ''}`}
                    // ⛔ DOUBLE-CLICK, NOT A TENTH BUTTON. A view button in the action cluster
                    // was built first and MEASURED: ten controls need ~318 px of button plus
                    // nine gaps against a 336 px track floor, and the harness reported
                    // `fits: false` at EVERY viewport, including the 1024 support floor.
                    // Widening the track takes that width from the columns at the narrowest
                    // size the app claims to support. Double-click costs no width, keeps the
                    // callsign selectable so it can still be copied, and is the idiom this app
                    // already uses in the decoded text (#262). The row title carries it,
                    // because the affordance is otherwise invisible.
                    onDoubleClick={() => setCard({ row: q, ref })}
                    // Ctrl/⌘-click picks the row for "Export selected", Shift-click a run — on the
                    // row itself, never through one of its controls.
                    onClick={(e) => {
                      if (!(e.ctrlKey || e.metaKey || e.shiftKey)) return
                      if ((e.target as HTMLElement).closest('button, a, input, select, textarea, label')) return
                      e.preventDefault()
                      pickRows(vrow.index, e.shiftKey)
                    }}
                    title={t('logbook.row.view.title', { call: q.call })}
                    role="row"
                    aria-selected={isPicked}
                    // The row's key is its id: unique per record — collision-proof even for two
                    // identical QSOs (double-clicked Log in the same second) — and it FOLLOWS the
                    // row when the list moves, so an open comment or the edit mark stays on it.
                    key={key}
                    data-index={vrow.index}
                    aria-rowindex={vrow.index + 2}
                    tabIndex={key === tabStop ? 0 : -1}
                    onFocus={() => setFocusKey(key)}
                    ref={rowVirtualizer.measureElement}
                    style={{
                      ...placed,
                      // Stripe by REAL index (inline beats the nth-child rule, which would otherwise
                      // stripe by render order and appear to "move" as the virtual window scrolls).
                      background: isPicked
                        ? 'color-mix(in srgb, var(--accent) 22%, transparent)'
                        : vrow.index % 2
                          ? 'color-mix(in srgb, var(--bg-elev) 50%, transparent)'
                          : 'transparent',
                    }}
                  >
                {cols.map((c) => (
                  <Fragment key={c.id}>{rowCell(c)}</Fragment>
                ))}
              </div>
                )
              })}
            </div>
          )}
        </div>
      </div>

      {importFile && (
        <ImportPreview
          text={importFile.text}
          fileName={importFile.name}
          onClose={() => setImportFile(null)}
          onImported={onImported}
        />
      )}

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
              <T k="logbook.purge.warn" tags={{ b: <strong /> }} vals={{ count: logSize }} />
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
                {purging ? t('logbook.purge.busy') : t('logbook.purge.confirm', { count: logSize })}
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
      {qrzFix && (
        <div
          className="logconfirm-backdrop"
          role="dialog"
          aria-modal="true"
          aria-label={t('logbook.qrzCorrect.aria')}
        >
          <div className="logconfirm">
            <div className="logconfirm-head">
              <h2>{t('logbook.qrzCorrect.heading', { call: qrzFix.q.call })}</h2>
            </div>
            {/* The body is composed in Rust: it names the callsign, the date and every field
                that will change, or reports exactly what QRZ answered. Shown verbatim so the
                one place that knows what a correction does is the place that says so. */}
            <p className="purge-warn" style={{ whiteSpace: 'pre-wrap' }}>
              {qrzFix.phase === 'reading'
                ? t('logbook.qrzCorrect.reading', { call: qrzFix.q.call })
                : qrzFix.text}
            </p>
            <div className="logconfirm-actions">
              {qrzFix.phase === 'done' ? (
                <>
                  <button
                    type="button"
                    className="logconfirm-discard"
                    onClick={() => setQrzFix(null)}
                  >
                    {t('logbook.qrzCorrect.close')}
                  </button>
                  {/* Offered, never automatic: QRZ's delete is permanent. */}
                  {qrzFix.result?.canRecover && (
                    <button
                      type="button"
                      className="logconfirm-log"
                      onClick={() => void onCorrectQrzRecover()}
                    >
                      {t('logbook.qrzCorrect.recover')}
                    </button>
                  )}
                </>
              ) : (
                <>
                  <button
                    type="button"
                    className="logconfirm-discard"
                    onClick={() => setQrzFix(null)}
                  >
                    {t('logbook.qrzCorrect.cancel')}
                  </button>
                  <button
                    type="button"
                    className="logconfirm-log"
                    disabled={qrzFix.phase !== 'confirm'}
                    onClick={() => void onCorrectQrzConfirm()}
                  >
                    {qrzFix.phase === 'sending'
                      ? t('logbook.qrzCorrect.busy')
                      : qrzFix.phase === 'recovering'
                        ? t('logbook.qrzCorrect.recovering')
                        : t('logbook.qrzCorrect.confirm')}
                  </button>
                </>
              )}
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
      {/* Mounted beside SpotDialog at the section root, not inside the virtualised rows:
          the row that opened it can be recycled out from under the view while it is open. */}
      <ContactCard
        open={card}
        onClose={() => setCard(null)}
        onSwitch={openCard}
        save={saveEdit}
        onChanged={load}
        onReload={reloadCard}
        logTick={logTick}
      />
      <RecentlyDeleted open={showDeleted} onClose={() => setShowDeleted(false)} onRestored={load} />
      <LogTools
        open={tools !== null}
        tool={tools ?? 'dupes'}
        onTool={setTools}
        onClose={() => setTools(null)}
        onChanged={load}
        onCallbookStarted={followCallbookUpdate}
        picked={picked}
        query={query}
        shown={listTotal}
      />
      <ParkStateReview open={showParkStates} onClose={() => setShowParkStates(false)} onApplied={load} />
      <ConfirmationReview open={showConfirmations} onClose={() => setShowConfirmations(false)} onApplied={load} />
    </section>
  )
}
