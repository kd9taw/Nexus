// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every reading in this
// pane is DATA and stays in the code: the callsign, the decoded message, the SNR and dB, the
// DT in seconds, the audio offset in Hz, the UTC stamp, the band, the tier and the country.
// So do the TOKENS the pane is built out of — gathered in DECODE_TOKENS below so a translator
// can see the whole set at once. What moved is the prose around them.
import { Fragment, useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { useRovingList } from '../useRovingList'
import { usePinnedScroll } from '../usePinnedScroll'
import type { DecodeRow, NeedAlert, Tier } from '../types'
import { resolveDecodeNeeds, isAwardNeed } from '../features/decodeNeeds'
import type { NeedBandScopes } from '../features/needs'
import { NEED_VISUALS, type NeedCat } from '../features/needVisuals'
import {
  DECODE_FILTERS,
  DecodeHistory,
  fmtUtc,
  orderEntries,
  passesFilter,
  periodStartMs,
  renderWindow,
  type DecodeFilter,
  type DecodeSort,
} from '../decodeHistory'
import { loadDecodeFilter, loadDecodeHideB4, loadDecodeHideBlocked, loadDecodeHideConfirmed, saveDecodeFilter, saveDecodeHideB4, saveDecodeHideBlocked, saveDecodeHideConfirmed } from '../operateFilters'
import { isHiddenByCountry, useCountryExclude } from '../features/countryExclude'
import { isCallHidden, useHideCalls } from '../features/hideCalls'
import { CountryExcludePicker, CountryHiddenChip } from './CountryExclude'
import { HideCallsPicker } from './HideCallsPicker'
import { gridFromMessage, isIgnored } from '../txMessages'
import { StateBlock } from './StateBlock'
import { RarityChip } from './RarityChip'
import { azimuthLabel, azimuthTitle, azimuthTo } from '../grid'
import { useEntityCentroids } from '../features/entityCentroids'
import { openQrzPage } from '../api'
import { withErrorToast } from '../toast'

/**
 * The pane's invariant vocabulary — never translated, never locale-formatted.
 *
 * `CQ` is a Q-code; `B4` is the log shorthand for "worked before" and names the filter chip
 * an operator matches against his own log; `HARQ`/`RV` name the incremental-redundancy
 * mechanism the decoder reports; `SNR` and `DT` are WSJT-X's own column names, printed
 * verbatim in the DT tooltip; `QRZ` is the site's name (QRZ_LABEL, as in Logbook.tsx).
 */
const DECODE_TOKENS = {
  cq: 'CQ',
  b4: 'B4',
  harq: 'HARQ',
  rv: 'RV',
  snr: 'SNR',
  dt: 'DT',
  qrz: 'QRZ',
} as const

/** JTAlert UDP highlight entry — bg/fg may be null/missing. */
export interface HighlightEntry {
  call: string
  bg?: string | null
  fg?: string | null
}

/**
 * Build a case-insensitive lookup Map from a highlights array.
 * Exported so OperateCockpit (and tests) can call it in useMemo.
 */
export function buildHighlightMap(
  highlights: HighlightEntry[] | undefined,
): Map<string, HighlightEntry> {
  const m = new Map<string, HighlightEntry>()
  if (!highlights) return m
  for (const h of highlights) {
    m.set(h.call.toUpperCase(), h)
  }
  return m
}

interface Props {
  /** This slot's decodes (the live per-slot feed from the snapshot). */
  decodes: DecodeRow[]
  /** Current slot index — stamps history rows + keys the period separators. */
  slot: number
  /** Current RX audio offset (Hz), for the "On RX freq" filter. */
  rxOffsetHz: number
  /** Current band (e.g. "20m") — a band change WIPES the pane (stale old-band
   * rows are a mis-operation hazard) and labels the period separators. */
  band: string
  /** Active mode/tier — sets the T/R period for separator UTC times; a tier
   * change wipes the pane like a band change. */
  tier: Tier
  /** Session count of IR-HARQ rescues (decodes recovered by combining). */
  harqRescues: number
  /** Work / answer a decoded station. `freq` = the decode's audio offset (Hz) so the
   * rig moves RX/TX onto it (WSJT-X double-click). */
  onCall: (call: string, grid?: string, message?: string, snr?: number, freq?: number) => void
  /** WSJT-X single-click SELECT: populate the Tx panel's DX Call/Grid from this
   * decode — no RF action, no TX. Grid is parsed from a trailing 4-char grid. */
  onSelectDecode?: (call: string, grid?: string, message?: string, snr?: number) => void
  /** Move RX onto a signal (Hz) WITHOUT starting a QSO — ctrl-double-click. */
  onSetRx?: (freqHz: number) => void
  /** The Tx panel's current DX call — its rows get the selected highlight. */
  selectedCall?: string | null
  /** Session-only ignore set (Alt-double-click) — ignored calls render dimmed. */
  ignoredCalls?: ReadonlySet<string>
  /** Toggle a call in/out of the session ignore set (Alt-double-click). */
  onToggleIgnore?: (call: string) => void
  /** Force a fixed filter and hide the filter chips (e.g. the Rx-Frequency pane
   * is a Band Activity locked to the 'rx' filter). */
  lockedFilter?: DecodeFilter
  /** Compact variant: hide the filter/sort controls (for a small secondary
   * pane like Rx Frequency). Erase stays (per-pane); the HARQ chip stays too —
   * it's session status, not a control. */
  compact?: boolean
  /** Header title (default "Band Activity"). */
  title?: string
  /**
   * The operator's own Maidenhead square — the origin every row's azimuth is
   * measured FROM. Optional and defaulting to empty: a host that doesn't pass it
   * (or an operator who has never set a grid) gets no azimuth column-side text at
   * all, which is the intended answer. There is no sensible default origin.
   */
  myGrid?: string
  /**
   * JTAlert-style UDP callsign highlights (built by OperateCockpit via
   * buildHighlightMap). When a row's from-call matches an entry, the row's
   * backgroundColor/color are overridden with the logger's chosen colors.
   * Inline style wins intentionally — JTAlert colors must show above theme classes.
   */
  highlights?: Map<string, HighlightEntry>
  /**
   * Live NeedAlerts keyed by UPPERCASE callsign (App builds it from the gated
   * alert set). Drives the per-row need micro-icons + need-based row colour so the
   * operator sees WHY a station is needed without leaving the band-activity view.
   * Optional — the Tempo rail / detached panel pass none and tagging no-ops.
   */
  needAlertsByCall?: Map<string, NeedAlert[]>
  /**
   * The operator's per-type alert BAND SCOPES (Settings ▸ Spots & Alerts). They gate the
   * need icons the same way they gate the sound/toast — a grid scope of VHF+ must take the
   * GRID icon off an HF row, which is the operator's twice-reported complaint. Needed here
   * as well as on the alert set because a row's entity/grid icon comes from the DECODE's own
   * engine flags. Every host passes it; omitting it withholds nothing.
   */
  needScopes?: NeedBandScopes
  /**
   * Called AFTER the internal erase() wipe so the cockpit can mirror the
   * operator's clear gesture to cooperating loggers via notifyErase (UDP Clear).
   * Only called on operator-initiated Erase, NOT on snap.clearTick (no echo loop).
   */
  onErase?: () => void
  /**
   * Bumped by an inbound UDP Clear (snap.clearTick). When the value CHANGES
   * (skipping mount), the pane wipes its history — same as Erase, but does NOT
   * invoke onErase (avoids echoing back to the logger).
   */
  clearTick?: number
  /** Externally-owned rolling history. The cockpit passes one per pane role so the
   * decode window SURVIVES a Classic ↔ Roster layout switch — with the default
   * component-local history, the switch remounts this pane and the accumulated
   * decodes vanished ("no decodes" mid-session; operator report 2026-07-21). When
   * omitted (detached panels, other hosts) a private history is used as before. */
  history?: DecodeHistory
  /**
   * Apply the operator's country exclusion to this pane (default on).
   *
   * OperateCockpit passes `false` for the Rx Frequency panes, and the split is the point:
   * Band Activity answers "who is on the band that I want to work" — a chase list, and the
   * right place to thin countries out. Rx Frequency answers "what is happening on MY
   * operating frequency", which is situational awareness — an excluded-country station
   * sitting on top of us is exactly what we need to see to understand why we are being
   * covered up, and hiding it would make our own frequency read as clear when it is not.
   */
  hideExcludedCountries?: boolean
}

/** Shared empty set so the ignore checks stay allocation-free per render. */
const NO_IGNORES: ReadonlySet<string> = new Set()

/** Shared empty map so the highlight lookups stay allocation-free per render. */
const NO_HIGHLIGHTS: Map<string, HighlightEntry> = new Map()

/** Shared empty map so need lookups stay allocation-free when no alerts are supplied. */
const NO_NEEDS: Map<string, NeedAlert[]> = new Map()

/** Shared empty set for a pane that does not apply the country exclusion. */
const NO_ENTITIES: ReadonlySet<string> = new Set()

/** Max need micro-icons shown per row before collapsing into a "+N" chip. */
const MAX_NEED_ICONS = 3

/**
 * Band Activity / Rx Frequency pane with stock WSJT-X flow: oldest at the top,
 * each period's decodes APPENDED at the bottom under a dim UTC+band separator
 * bar, pane pinned to the bottom. Scrolling up (> ~40 px from the bottom)
 * pauses the auto-scroll so you can read back; scrolling back near the bottom
 * resumes it. New rows never yank the view while you're reading.
 *
 * Click model is stock WSJT-X: single-click SELECTS (populates DX Call/Grid,
 * no RF action), double-click WORKS the station, ctrl-double-click moves RX
 * onto the signal without transmitting, Alt-double-click toggles a session
 * ignore. On top of the stock flow: filter chips (All / CQ / CQ+73 / To me /
 * On RX / B4 / New), sort, and a per-pane Erase (WSJT-X term).
 */
export function OperateDecodes({
  decodes,
  slot,
  rxOffsetHz,
  band,
  tier,
  harqRescues,
  onCall,
  onSelectDecode,
  onSetRx,
  selectedCall,
  ignoredCalls,
  onToggleIgnore,
  lockedFilter,
  compact = false,
  // A DEFAULT PARAMETER, so the lookup happens on every render rather than at import — a
  // module-level constant would freeze whichever locale loaded this file first.
  title = t('operate.decodes.title'),
  highlights = NO_HIGHLIGHTS,
  needAlertsByCall = NO_NEEDS,
  needScopes,
  onErase,
  clearTick = 0,
  history,
  hideExcludedCountries = true,
  myGrid = '',
}: Props) {
  // Cockpit-owned history when provided (survives layout remounts); private otherwise.
  const localHistRef = useRef<DecodeHistory | null>(null)
  if (history == null && localHistRef.current == null) {
    localHistRef.current = new DecodeHistory()
  }
  const histRef = { current: history ?? (localHistRef.current as DecodeHistory) }
  const [, setTick] = useState(0)
  // Persisted per-surface (operateFilters.ts) so the chip survives a restart. Only the pane
  // that RENDERS chips can write one: the locked panes (Rx Frequency) show no filter bar, so
  // their filterState is dead — `filter` below takes lockedFilter — and 'rx' can never
  // overwrite the Band Activity chip the operator chose.
  const [filterState, setFilterState] = useState<DecodeFilter>(loadDecodeFilter)
  // ⚠️ A COMPACT PANE SHOWS EVERYTHING. The rule above — only a pane that renders chips may
  // WRITE one — has to apply to READING too, and it did not: the Tempo rail's compact Band
  // Activity hides the chip bar and still applied the shared persisted chip. So a "CQ" chip
  // left set over in Operate silently filtered Tempo's list, with nothing on screen to say
  // so and no control to clear it — the pane just looked like a quiet band. `lockedFilter`
  // still wins where a pane genuinely declares one (Rx Frequency).
  const filter = lockedFilter ?? (compact ? 'all' : filterState)
  const pickFilter = (f: DecodeFilter) => {
    saveDecodeFilter(f)
    setFilterState(f)
  }
  const [sort, setSort] = useState<DecodeSort>('time')
  // The "hide B4" MODIFIER (field ask: "CQ only, but exclude B4") — ANDed with whichever
  // chip is lit, persisted like the chip itself. Inert while the B4 chip is active: that
  // chip's whole job is showing worked stations, and a modifier that blanked it would
  // read as a broken pane.
  const [hideB4, setHideB4] = useState<boolean>(loadDecodeHideB4)
  const pickHideB4 = (on: boolean) => {
    saveDecodeHideB4(on)
    setHideB4(on)
  }
  // "Hide blocked" — same modifier shape. Off (default): blocked calls keep their dimmed
  // look; on: gone from the pane. The auto-responder never answers them either way — that
  // guarantee is engine-side and does not depend on a display toggle.
  const [hideBlocked, setHideBlocked] = useState<boolean>(loadDecodeHideBlocked)
  const pickHideBlocked = (on: boolean) => {
    saveDecodeHideBlocked(on)
    setHideBlocked(on)
  }
  // "Hide confirmed on this band" (F4MQS): drop stations already award-confirmed here.
  // A still-new-on-band station always shows (confirmedBand is never set with newBand).
  const [hideConfirmed, setHideConfirmed] = useState<boolean>(loadDecodeHideConfirmed)
  const pickHideConfirmed = (on: boolean) => {
    saveDecodeHideConfirmed(on)
    setHideConfirmed(on)
  }

  // The operator's country exclusion — app-global (every window agrees) and RX-display
  // only. See features/countryExclude.ts for why it is not a Rust setting.
  const countries = useCountryExclude()
  const hideCalls = useHideCalls()
  // Entity centroids for the azimuth beside the country. Most decodes carry no grid
  // (only CQ/grid messages do), so without this the heading would appear on a
  // minority of rows and read as a glitch rather than a feature.
  const centroids = useEntityCentroids()
  const hiddenEntities = hideExcludedCountries ? countries.hidden : NO_ENTITIES

  // Bottom-pinned auto-scroll (WSJT-X flow) — the shared discipline, extracted
  // to usePinnedScroll (which keeps the every-render re-pin this pane's
  // keep-alive host depends on). `pinned` drives the "▲ reviewing" hint.
  const { ref: scrollRef, pinned, onScroll, repin } = usePinnedScroll<HTMLDivElement>()

  // Band/tier change wipes the pane BEFORE this poll's decodes are ingested
  // (effect order = declaration order).
  useEffect(() => {
    if (histRef.current.setScope(band, tier)) {
      repin()
      setTick((t) => t + 1)
    }
  }, [band, tier, repin])

  // Ingest this poll's decode list into the rolling history.
  useEffect(() => {
    histRef.current.ingest(decodes, slot)
    setTick((t) => t + 1)
  }, [decodes, slot])

  // Inbound UDP Clear: when clearTick changes (skip mount), wipe without
  // calling onErase (no echo loop back to the logger).
  const clearTickSeen = useRef(clearTick)
  useEffect(() => {
    if (clearTick !== clearTickSeen.current) {
      clearTickSeen.current = clearTick
      histRef.current.erase()
      repin()
      setTick((t) => t + 1)
    }
  }, [clearTick, repin])

  const ignores = ignoredCalls ?? NO_IGNORES
  const list = orderEntries(
    histRef.current
      .entries()
      .filter((d) => passesFilter(d, filter, rxOffsetHz))
      // "Hide B4" ANDs with the chip (never with the B4 chip itself). Own rows and the
      // station mid-QSO stay — hiding your own echo or your current partner as "worked"
      // would be the same class of self-own the country exclude guards against.
      .filter(
        (d) =>
          !(
            hideB4 &&
            filter !== 'b4' &&
            d.worked &&
            !d.mine &&
            (selectedCall == null || d.from !== selectedCall)
          ),
      )
      // "Hide blocked" ANDs the same way; own rows and the station mid-QSO always stay.
      .filter(
        (d) =>
          !(
            hideBlocked &&
            isIgnored(ignores, d.from) &&
            !d.mine &&
            (selectedCall == null || d.from !== selectedCall)
          ),
      )
      // "Hide confirmed on this band" — own rows and the working partner always stay.
      .filter(
        (d) =>
          !(
            hideConfirmed &&
            d.confirmedBand &&
            !d.mine &&
            (selectedCall == null || d.from !== selectedCall)
          ),
      )
      // Wildcard call-hide (VP8* etc.) — display-only; own rows and the QSO partner stay.
      .filter(
        (d) =>
          !(
            isCallHidden(d.from, hideCalls.entries) &&
            !d.mine &&
            (selectedCall == null || d.from !== selectedCall)
          ),
      )
      // The country exclusion is ANDed with the chip, and it is the last word on what the
      // pane shows — so `list.length` (the "N heard" readout) counts what is on screen.
      .filter(
        (d) =>
          !isHiddenByCountry(
            {
              entity: d.country,
              call: d.from,
              // The Tx panel's DX call: never hide the station we are working, mid-exchange.
              qsoPartner: selectedCall,
              addressedToMe: d.directedToMe || Boolean(d.mine),
              // NEEDED OUTRANKS EXCLUDED, on the engine's own flags — a new entity or a new
              // band slot still surfaces from a country the operator has switched off.
              needed: Boolean(d.newDxcc || d.newBand),
            },
            hiddenEntities,
          ),
      ),
    sort,
  )
  // What the pane DRAWS: the newest MAX_ROWS of what the filter kept. The store behind it is
  // ten times deeper (see decodeHistory's MAX_HISTORY) so a filtered pane can be scrolled back
  // through hours of its own frequency, but the row count that is actually painted — the cost
  // the 4 Hz ticker pays on a machine without GPU compositing — is exactly what it always was.
  // Everything below counts `shown`, not `list`: the "N heard" readout means what is on screen,
  // and the roving-keyboard index must address the rows that exist.
  const shown = renderWindow(list)

  // Wipe this pane (WSJT-X "Erase") and re-pin to the bottom.
  // Also calls onErase so the cockpit can mirror the gesture to loggers.
  const erase = () => {
    histRef.current.erase()
    repin()
    setTick((t) => t + 1)
    onErase?.()
  }

  const selectedUp = selectedCall?.trim().toUpperCase() || null

  // WSJT-X double-click dispatch: Alt = toggle session ignore; Ctrl = populate
  // DX fields + move RX onto the signal (no QSO start, no TX arm); plain = work.
  const handleDouble = (e: React.MouseEvent, d: DecodeRow) => {
    if (!d.from) return
    if (e.altKey) {
      onToggleIgnore?.(d.from)
      return
    }
    if (e.ctrlKey || e.metaKey) {
      onSelectDecode?.(d.from, gridFromMessage(d.message), d.message, d.snr)
      onSetRx?.(d.freqHz)
      return
    }
    onCall(d.from, undefined, d.message, d.snr, d.freqHz)
  }

  // Keyboard: arrow through rows, Enter selects, Shift+Enter works the station,
  // Alt+Enter toggles ignore — the pointerless equivalent of click/double-click.
  const roving = useRovingList(shown.length, (i, mods) => {
    const d = shown[i]
    if (!d?.from) return
    if (mods.alt) onToggleIgnore?.(d.from)
    else if (mods.shift) onCall(d.from, undefined, d.message, d.snr, d.freqHz)
    else onSelectDecode?.(d.from, gridFromMessage(d.message), d.message, d.snr)
  })

  const eraseBtn = (
    <button
      type="button"
      className="od-chip od-clear"
      onClick={erase}
      title={t('operate.decodes.erase.title')}
    >
      {t('operate.decodes.erase.label')}
    </button>
  )

  return (
    <section className={`operate-decodes${compact ? ' compact' : ''}`}>
      <div className="od-head">
        <h2>{title}</h2>
        {compact ? (
          eraseBtn
        ) : (
          <div className="od-controls">
            <div className="od-filters" role="group" aria-label={t('operate.decodes.filters.aria')}>
              {DECODE_FILTERS.map((f) => (
                <button
                  key={f}
                  type="button"
                  className={`od-chip${filter === f ? ' active' : ''}`}
                  aria-pressed={filter === f}
                  onClick={() => pickFilter(f)}
                  title={FILTER_TITLE[f]}
                >
                  {FILTER_LABEL[f]}
                </button>
              ))}
            </div>
            {/* Outside the chip group on purpose: those chips are one-of-N (aria-pressed),
                this is an independent set that ANDs with whichever one is lit. */}
            <button
              type="button"
              className={`od-chip od-blocked${hideBlocked ? ' active' : ''}`}
              aria-pressed={hideBlocked}
              onClick={() => pickHideBlocked(!hideBlocked)}
              title={t('operate.decodes.hideBlocked.title')}
            >
              {t('operate.decodes.hideBlocked.label')}
            </button>
            <button
              type="button"
              className={`od-chip od-conf${hideConfirmed ? ' active' : ''}`}
              aria-pressed={hideConfirmed}
              onClick={() => pickHideConfirmed(!hideConfirmed)}
              title={t('operate.decodes.hideConfirmed.title')}
            >
              {t('operate.decodes.hideConfirmed.label')}
            </button>
            <button
              type="button"
              className={`od-chip od-b4${hideB4 ? ' active' : ''}`}
              aria-pressed={hideB4}
              disabled={filter === 'b4'}
              onClick={() => pickHideB4(!hideB4)}
              title={
                filter === 'b4'
                  ? t('operate.decodes.hideB4.title.idle')
                  : t('operate.decodes.hideB4.title')
              }
            >
              −{DECODE_TOKENS.b4}
            </button>
            <CountryExcludePicker keys={countries.keys} onToggle={countries.toggle} paused={countries.paused} onPauseChange={countries.setPaused} entities={countries.entities} onToggleEntity={countries.toggleEntity} />
            <HideCallsPicker />
            <label className="od-sort">
              <span className="od-sort-label">{t('operate.decodes.sort.label')}</span>
              {/* The <option> VALUES are stored tokens; only the labels are words, and two
                  of the four are column tokens themselves. */}
              <select value={sort} onChange={(e) => setSort(e.target.value as DecodeSort)}>
                <option value="time">{t('operate.decodes.sort.time')}</option>
                <option value="snr">{DECODE_TOKENS.snr}</option>
                <option value="freq">{t('operate.decodes.sort.freq')}</option>
                <option value="dt">{DECODE_TOKENS.dt}</option>
              </select>
            </label>
            {eraseBtn}
          </div>
        )}
      </div>

      <div className="od-status">
        <span className={`od-paused${!pinned ? ' on' : ''}`} aria-live="polite">
          {pinned
            ? t('operate.decodes.heard', { count: shown.length })
            : t('operate.decodes.reviewing')}
        </span>
        {/* Beside the count, so a pane that is hiding rows always says so — including the
            compact panes, which render no chip bar to notice the picker in. */}
        {hideExcludedCountries && (
          <CountryHiddenChip
            count={countries.keys.size}
            onClear={countries.clear}
            testId="od-hidden"
          />
        )}
        {harqRescues > 0 && (
          <span
            className="harq-chip"
            title={t('operate.decodes.harq.title', { count: harqRescues })}
          >
            {DECODE_TOKENS.harq} ×{harqRescues}
          </span>
        )}
      </div>

      <div
        className="od-scroll"
        role="listbox"
        aria-label={t('operate.decodes.list.aria')}
        ref={scrollRef}
        onScroll={onScroll}
        onKeyDown={roving.containerProps.onKeyDown}
      >
        {shown.length === 0 &&
          // TWO different empty states, and conflating them cost a field report its
          // diagnosis: with the "To me" chip lit on a busy band this pane said
          // "No decodes yet — waiting for the next slot" while its history held
          // hundreds of rows the FILTER was hiding — which read as "decoder dead" and
          // blinded the one pane that could have answered whether decodes were
          // arriving. Say which it is.
          (histRef.current.entries().length === 0 ? (
            <StateBlock
              kind="empty"
              title={t('operate.decodes.empty.title')}
              detail={t('operate.decodes.empty.detail')}
            />
          ) : (
            <StateBlock
              kind="empty"
              title={t('operate.decodes.emptyFiltered.title', {
                filter: FILTER_LABEL[filter] ?? filter,
              })}
              detail={t('operate.decodes.emptyFiltered.detail', {
                count: histRef.current.entries().length,
              })}
            />
          ))}
        {shown.map((d, i) => {
          const ignoredRow = isIgnored(ignores, d.from)
          const selectedRow = !!d.from && !!selectedUp && d.from.toUpperCase() === selectedUp
          // JTAlert highlight lookup: match the from-call case-insensitively.
          const hlEntry = d.from ? highlights.get(d.from.toUpperCase()) : undefined
          const hlStyle = hlEntry
            ? {
                backgroundColor: hlEntry.bg ?? undefined,
                color: hlEntry.fg ?? undefined,
              }
            : undefined
          // Tooltip suffix for highlighted rows so the operator knows why the color appeared.
          // An appended CLAUSE carrying its own separator, interpolated whole.
          const hlTip = hlEntry ? t('operate.decodes.row.highlighted') : ''
          // Need context for this row (why is this station worth working) — icons + colour.
          const rowAlerts = d.from ? (needAlertsByCall.get(d.from.toUpperCase()) ?? []) : []
          const needs = resolveDecodeNeeds(d, band, rowAlerts, 'Digital', needScopes)
          // Beam heading for this row: the decode's own grid when it sent one, else
          // the centre of its entity (marked `~`), else nothing at all.
          const az = azimuthTo(myGrid, d.grid, d.country, centroids)
          const azText = azimuthLabel(az)
          return (
            <Fragment key={d.id}>
              {/* WSJT-X period separator: a dim bar with the period's UTC start +
                  band, whenever the T/R period changes (time-sorted view only).
                  A decode ingested at boundary slot s carries AUDIO from slot s-1 —
                  the separator stamps the RX period the signals were ON AIR in
                  (WSJT-X labels the audio period, not the decode moment). */}
              {/* ⚠️ `shown[i - 1]`, NEVER `list[i - 1]` — `i` indexes `shown`, and `shown` is
                  the newest MAX_ROWS of `list`, so once history passes 300 rows the two
                  indices point at completely different decodes. Comparing against `list`
                  matched each row's slot to one from the START of the buffer, minutes older,
                  which differs nearly every time: a separator between EVERY decode instead of
                  one per period. Reported twice (2026-08-21/22), both saying it begins "after
                  some time" — that is the buffer reaching MAX_ROWS, not elapsed time. */}
              {sort === 'time' && i > 0 && d.slot !== shown[i - 1].slot && (
                <div
                  className="od-period-sep"
                  role="separator"
                  aria-label={t('operate.decodes.period.aria', {
                    time: fmtUtc(periodStartMs(d.slot - 1, tier)),
                  })}
                >
                  <span className="od-sep-utc">{fmtUtc(periodStartMs(d.slot - 1, tier))}</span>
                  <span className="od-sep-band">{band}</span>
                </div>
              )}
              <div
                className={`decode-row ${rowClass(d, needs.rowNeed)}${selectedRow ? ' selected' : ''}${ignoredRow ? ' ignored' : ''}`}
                role="option"
                aria-selected={selectedRow}
                aria-label={
                  d.from
                    ? t('operate.decodes.row.aria', {
                        call: d.from,
                        snr: fmtSnr(d.snr),
                        hz: Math.round(d.freqHz),
                        message: d.message,
                        // Two optional clauses, each interpolated WHOLE with its own
                        // separator — never a sentence glued from fragments.
                        country: d.country
                          ? t('operate.decodes.row.aria.country', { country: d.country })
                          : '',
                        azimuth: az
                          ? az.approx
                            ? t('operate.decodes.row.aria.azimuth.approx', { deg: az.deg })
                            : t('operate.decodes.row.aria.azimuth', { deg: az.deg })
                          : '',
                      })
                    : d.message
                }
                tabIndex={roving.rowProps(i).tabIndex}
                ref={roving.rowProps(i).ref as (el: HTMLDivElement | null) => void}
                onFocus={roving.rowProps(i).onFocus}
                style={hlStyle}
                onClick={() => {
                  roving.setActive(i)
                  if (d.from) onSelectDecode?.(d.from, gridFromMessage(d.message), d.message, d.snr)
                }}
                onDoubleClick={(e) => handleDouble(e, d)}
                title={
                  ignoredRow
                    ? t('operate.row.ignored.title')
                    : d.from
                      ? t('operate.decodes.row.title', { call: d.from, highlight: hlTip })
                      : undefined
                }
              >
                <span
                  className={`decode-tier ${d.tier.toLowerCase()}`}
                  title={t('operate.decodes.tier.title', { tier: d.tier })}
                >
                  {d.tier}
                </span>
                <span className="decode-utc" title={t('operate.decodes.utc.title')}>{fmtUtc(d.at)}</span>
                <span className={`decode-snr ${snrClass(d.snr)}`}>{fmtSnr(d.snr)}</span>
                {/* On MSK144 dt is the ping's TIME WITHIN THE PERIOD (WSJT-X renames this
                    column "T"), so it is legitimately 0..period and the FT8 clock-skew
                    colouring would paint every healthy ping red. */}
                <span
                  className={`decode-dt ${d.tier === 'MSK144' ? 'ok' : dtClass(d.dtSec)}`}
                  title={
                    d.tier === 'MSK144'
                      ? t('operate.decodes.dt.title.msk144')
                      : t('operate.decodes.dt.title')
                  }
                >
                  {fmtDt(d.dtSec)}
                </span>
                <span className="decode-freq">{Math.round(d.freqHz)}</span>
                <span className="decode-msg" title={d.country ? `${d.message} · ${d.country}` : d.message}>
                  {d.message}
                  {/* WSJT-X AP / low-confidence markers: dim trailing annotations.
                      Both can appear on the same decode (AP-assisted but uncertain). */}
                  {(d.lowConf || d.ap) && (
                    <span className="decode-confidence-markers">
                      {d.lowConf && (
                        <span
                          className="decode-marker decode-marker-lc"
                          title={t('operate.decodes.marker.lowConf.title')}
                        >
                          ?
                        </span>
                      )}
                      {d.ap && (
                        <span
                          className="decode-marker decode-marker-ap"
                          title={t('operate.decodes.marker.ap.title')}
                        >
                          a
                        </span>
                      )}
                    </span>
                  )}
                  {/* Need chips: WHY this station is worth working (new DXCC/zone/band/mode/
                      grid, DXpedition, worked-but-unconfirmed) — the SAME labelled text chips
                      the Needed panel uses, so the two views read as one. Capped, with +N. */}
                  {needs.cats.length > 0 && (
                    <span className="decode-needs" aria-label={t('operate.decodes.needs.aria')}>
                      {needs.cats.slice(0, MAX_NEED_ICONS).map((c: NeedCat) => {
                        const v = NEED_VISUALS[c]
                        return (
                          <span key={c} className={`need-chip ${v.cls}`} title={v.title}>
                            {v.label}
                          </span>
                        )
                      })}
                      {needs.cats.length > MAX_NEED_ICONS && (
                        <span
                          className="decode-need-more"
                          title={needs.cats
                            .slice(MAX_NEED_ICONS)
                            .map((c) => NEED_VISUALS[c].title)
                            .join('\n')}
                        >
                          +{needs.cats.length - MAX_NEED_ICONS}
                        </span>
                      )}
                    </span>
                  )}
                  {d.worked && (
                    <span
                      className={`b4-chip${d.workedBand ? ' b4-band' : ''}`}
                      title={d.workedBand ? t('operate.b4.sameBand') : t('operate.b4.otherBand')}
                    >
                      {DECODE_TOKENS.b4}
                    </span>
                  )}
                  {d.isCq && !d.directedToMe && (
                    <span className="decode-tag cq">{DECODE_TOKENS.cq}</span>
                  )}
                  {d.directedToMe && (
                    <span className="decode-tag me">{t('operate.decodes.tag.you')}</span>
                  )}
                  {d.rv > 0 && (
                    <span className="harq-chip" title={t('operate.decodes.harqRv.title', { rv: d.rv })}>
                      {DECODE_TOKENS.harq}·{DECODE_TOKENS.rv}
                      {d.rv}
                    </span>
                  )}
                  <RarityChip rarity={d.gridRarity} />
                  {d.lotwUser && (
                    <span
                      className="lotw-mark"
                      title={t('operate.decodes.lotw.title', {
                        call: d.from ?? t('operate.decodes.lotw.thisStation'),
                      })}
                    >
                      L
                    </span>
                  )}
                  {d.country && <span className="decode-country">{d.country}</span>}
                  {/* The heading, immediately after the country — where every other
                      logger puts it, and where the tester asked for it. `.decode-country`
                      already carries `margin-left:auto`, so the pair floats to the right
                      of the message together and no other cell moves. */}
                  {az && azText && (
                    <span className="decode-az" title={azimuthTitle(az, d.country)}>
                      {azText}
                    </span>
                  )}
                </span>
                {d.from && (
                  // WSJT-X style: DOUBLE-CLICK the row to work the station (see onDoubleClick) — no
                  // per-row Work button, so each decode stays a single tight line. QRZ opens the
                  // callsign's page.
                  <button
                    type="button"
                    className="qrz-link-call decode-qrz"
                    onClick={(e) => { e.stopPropagation(); const c = d.from as string; void withErrorToast(() => openQrzPage(c), t('callbook.qrzPage.failed', { call: c })) }}
                    title={t('callbook.qrzPage.title', { call: d.from })}
                  >
                    {DECODE_TOKENS.qrz}
                  </button>
                )}
              </div>
            </Fragment>
          )
        })}
      </div>
    </section>
  )
}

// The chip vocabulary. THE WORDS RESOLVE LAZILY, through getters (the registry rule, batch 3):
// these are module constants a render reads, so looking them up at import would freeze
// whichever locale loaded this file first and no re-render could move it. The record SHAPE is
// unchanged, so every consumer — including the empty-state message that quotes the active
// chip — reads them exactly as before. `CQ`, `CQ+73` and `B4` are Q-code / log tokens and are
// not words at all.
const FILTER_LABEL: Record<DecodeFilter, string> = {
  get all() {
    return t('operate.decodes.filter.all')
  },
  cq: DECODE_TOKENS.cq,
  cq73: `${DECODE_TOKENS.cq}+73`,
  get me() {
    return t('operate.decodes.filter.me')
  },
  get rx() {
    return t('operate.decodes.filter.rx')
  },
  b4: DECODE_TOKENS.b4,
  get new() {
    return t('operate.decodes.filter.new')
  },
}
const FILTER_TITLE: Record<DecodeFilter, string> = {
  get all() {
    return t('operate.decodes.filter.title.all')
  },
  get cq() {
    return t('operate.decodes.filter.title.cq')
  },
  get cq73() {
    return t('operate.decodes.filter.title.cq73')
  },
  get me() {
    return t('operate.decodes.filter.title.me')
  },
  get rx() {
    return t('operate.decodes.filter.title.rx')
  },
  get b4() {
    return t('operate.decodes.filter.title.b4')
  },
  get new() {
    return t('operate.decodes.filter.title.new')
  },
}

/** DT (time offset, s) with sign; flags large skew. */
function fmtDt(dt: number): string {
  return `${dt >= 0 ? '+' : ''}${dt.toFixed(1)}`
}
function dtClass(dt: number): string {
  return Math.abs(dt) > 1.0 ? 'bad' : Math.abs(dt) > 0.5 ? 'warn' : 'ok'
}

/** Row highlight priority — a superset of stock WSJT-X: own TX (yellow) > directed to
 * me (pink) > award-grade need (new DXCC/zone/band/mode/grid — the chase outranks a
 * plain CQ) > CQ (green) > worked-but-unconfirmed (grey) > worked-before B4 (dimmed) >
 * new. `rowNeed` is the resolved need colour class (or null); `dxped/pota/sota` are
 * icon-only and never reach here. Falls back to the decode's own flags when no
 * NeedAlerts are supplied (Tempo rail / detached panel). */
function rowClass(d: DecodeRow, rowNeed: string | null): string {
  if (d.mine) return 'mine own-tx' // our own transmitted message — WSJT-X yellow
  if (d.directedToMe) return 'directed'
  if (isAwardNeed(rowNeed)) return rowNeed as string // need-entity/zone/band/mode/grid
  if (d.isCq) return 'cq'
  if (rowNeed === 'need-confirm') return 'need-confirm' // worked, needs a QSL
  if (d.worked) return 'worked'
  return 'new'
}

function fmtSnr(snr: number): string {
  return `${snr > 0 ? '+' : ''}${snr}`
}

function snrClass(snr: number): string {
  if (snr >= -10) return 'good'
  if (snr >= -18) return 'ok'
  return 'weak'
}
