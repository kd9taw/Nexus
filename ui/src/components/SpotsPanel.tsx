import { useStationControl } from '../stationAccess'
// The raw "Spots" board — every recent cluster/RBN spot (CW/Phone/Digital, all sources),
// NOT needs-gated. This is the SpotCollector/DXHeat-style firehose view: see everything,
// filter client-side. The Needed board stays the curated "what to work" list; this is the
// "what's on the air" list. Single-click a row to QSY/work the spot.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT come from it: every
// value a row prints (callsign, spotter, entity, US state, band, mode/submode, frequency,
// comment) and the age column below — all data and measurement, invariant in every locale.
import { useEffect, useMemo, useState } from 'react'
import type { BandChannel, NeedAlert, SpotRow } from '../types'
import { openQrzPage } from '../api'
import { withErrorToast } from '../toast'
import { azimuthLabel, azimuthTitle, azimuthTo } from '../grid'
import { useEntityCentroids } from '../features/entityCentroids'
import { alertsByCall, alertsForSurface, isActivityTag } from '../features/needs'
import { CONTINENT_CODES } from '../features/dxccGeo'
import { compileTerm, searchTerms } from '../searchQuery'
import { t } from '../i18n'
import { useWatchMatch } from '../watchlist'
import { WatchTile } from './WatchTile'

type SortKey = 'age' | 'call' | 'entity' | 'state' | 'band' | 'freq' | 'mode'

// Common HF + 6m bands always offered in the filter bar; augmented with any band present
// in the current spots.
const COMMON_BANDS = ['160m', '80m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m']
/** cty.dat's continent codes, in the order the spotted-from chips show them (#174). Tokens,
 * not prose: the same two letters in every language. The shared list — Band Activity's
 * hide-by-continent and the decode alert scope read the same one. */
const CONTINENT_ORDER = CONTINENT_CODES

/** Compact age string from seconds-since-received (−1 = unknown). A number and its unit
 * letter, with no prose in it at all — a measurement, so it is not a catalog string. */
function ageLabel(secs: number): string {
  if (secs < 0) return '—'
  if (secs < 60) return `${secs}s`
  if (secs < 3600) return `${Math.round(secs / 60)}m`
  return `${Math.round(secs / 3600)}h`
}

/** How long ago the operator worked a station, for the row badge. `ageLabel`'s vocabulary, with
 * DAYS added: the worked windows reach a week, and "168h" is not a thing anyone reads. */
function workedAgeLabel(secs: number): string {
  return secs < 86_400 ? ageLabel(secs) : `${Math.round(secs / 86_400)}d`
}

/**
 * How far back "worked" reaches for Hide worked. `'utcDay'` is since 0000Z on the STATION's
 * clock — the row's own `workedTodayUtc`, never a browser's idea of the date — and a number is
 * plain seconds. The ladder is the operator's (2026-09-17): the UTC day by default, because that
 * is the day POTA credits and the day a spotted station is "already in the log today", and the
 * hour/day/week rungs for events like 13 Colonies and Route 66 where one callsign is on the air
 * all week and a UTC day is far too coarse.
 */
export type WorkedWindow = 'utcDay' | 3600 | 14400 | 86400 | 604800
export const WORKED_WINDOWS: readonly WorkedWindow[] = ['utcDay', 3600, 14400, 86400, 604800]
export function isWorkedWindow(v: unknown): v is WorkedWindow {
  return (WORKED_WINDOWS as readonly unknown[]).includes(v)
}

/**
 * Was this spot's station worked within `window`?
 *
 * A row carrying NO flags — an older station, or one that never answered — is worked in no
 * window at all. The panel must never hide a row it cannot judge, which is the same fail-open
 * posture `spotterLocal === false` takes one filter along.
 */
export function workedWithin(
  s: Pick<SpotRow, 'workedAgoSecs' | 'workedTodayUtc'>,
  window: WorkedWindow,
): boolean {
  if (window === 'utcDay') return s.workedTodayUtc === true
  return s.workedAgoSecs != null && s.workedAgoSecs < window
}

/**
 * Does a need the Needed board admitted still apply HERE — on the band and mode this spot is on?
 *
 * The rescue that makes Hide worked safe, and it is the roster's own rule rather than one of this
 * panel's: gate the call's alerts to the surface (`alertsForSurface` — a 40 m new-band slot says
 * nothing about a 20 m spot), then keep the row if anything that survives is a real NEED. An
 * activity label (POTA/SOTA/DXpedition) is not: being a DXpedition is not something you can need.
 */
export function neededHere(alerts: NeedAlert[] | undefined, band: string, mode: string): boolean {
  return alertsForSurface(alerts, band, mode).some((a) => a.tags.some((tag) => !isActivityTag(tag)))
}

interface Props {
  spots: SpotRow[]
  bandPlan: BandChannel[]
  selectedCall: string | null
  onSelect: (call: string) => void
  /** Work the spot — QSY to its freq/mode and open the matching cockpit. */
  onWork: (spot: SpotRow) => void
  /** A browser Work for this row; the desktop leaves it unset and works every row. */
  canWork?: (spot: SpotRow) => boolean
  onPopOut?: () => void
  /** The operator's own square — origin for the beam heading beside each entity. A
   * cluster/RBN spot carries no grid, so that heading is always the entity centre. */
  myGrid?: string
  /** The Needed board's alerts, for the ONE thing Hide worked asks of them: a station still
   * needed on the band and mode it is spotted on is never hidden as worked (`neededHere`).
   * Absent = no rescue, which only ever shows fewer rows, never more. */
  needAlerts?: NeedAlert[]
}

/** View-session state: the Spots panel unmounts on every view switch, which wiped all
 * filters mid-session (operator report 2026-07-21: "Leaving SPOT and returning resets
 * all filters"). sessionStorage survives the remount and clears on app exit — exactly
 * "retain them until application exit". Falls back to plain state if storage throws. */
function useSessionState<T>(key: string, init: T): [T, React.Dispatch<React.SetStateAction<T>>] {
  const [v, setV] = useState<T>(() => {
    try {
      const raw = sessionStorage.getItem(key)
      if (raw != null) return JSON.parse(raw) as T
    } catch {
      /* ignore */
    }
    return init
  })
  useEffect(() => {
    try {
      sessionStorage.setItem(key, JSON.stringify(v))
    } catch {
      /* ignore */
    }
  }, [key, v])
  return [v, setV]
}

export function SpotsPanel({ spots, bandPlan, selectedCall, onSelect, onWork, canWork, onPopOut, myGrid = '', needAlerts }: Props) {
  const control = useStationControl()
  // Entity centres — the only geometry the firehose carries (a spot has no grid).
  const centroids = useEntityCentroids()
  // The operator's watch list, live — the matcher the Call Roster and the Stations list ask, so
  // a watched station wears the same WATCH tile on this board.
  const watchOf = useWatchMatch()
  // ONE flat mode filter: the SPECIFIC modes present (CW/Phone/FT8/FT4/RTTY/Digital…), each a
  // show/hide toggle. Stores the HIDDEN set (empty = all shown) so a mode that first appears
  // mid-session shows by default instead of being silently hidden.
  const [hiddenModes, setHiddenModes] = useSessionState<string[]>('nexus.spots.hiddenModes', [])
  const [bands, setBands] = useSessionState<string[]>('nexus.spots.bands', []) // empty = all
  const [sort, setSort] = useSessionState<{ key: SortKey; dir: 'asc' | 'desc' }>('nexus.spots.sort', { key: 'age', dir: 'asc' })
  const [filtersOpen, setFiltersOpen] = useSessionState('nexus.spots.filtersOpen', false)
  // Freeform search over the firehose: space-separated terms AND together, each term
  // matching ANY field (call/entity/spotter/mode/band/frequency) — so "w1 20m cw"
  // narrows to W1-calls spotted on 20 m CW.
  const [query, setQuery] = useSessionState('nexus.spots.query', '')
  // Privilege filter (operator 2026-07-21): hide spots you may not transmit to. The
  // `licensed` flag is computed backend-side from the SAME tables as the TX lockout;
  // an Open-class (non-US) operator has every spot licensed, so the toggle is a no-op.
  const [licensedOnly, setLicensedOnly] = useSessionState('nexus.spots.licensedOnly', false)
  // "Heard on my continent" — keep only spots at least one voice on the operator's OWN
  // CONTINENT reported. The same question the Needed board asks; the panel had no locality test
  // at all, so a US operator saw JA stations only Europe and Asia had heard, which says nothing
  // about a path from here (operator, 2026-08-19).
  //
  // ⚠️ IT WAS LABELLED "Heard near me", and a reporter asked us to BUILD a "spotted from Europe
  // only" filter while standing in front of it, default on. The label named a feeling; the
  // predicate is a continent. The words on the chip now say what the tooltip always did.
  //
  // DEFAULT ON, and the count of what it hides is printed beside it — a filter that removes
  // rows silently is how "my spots disappeared" becomes an unanswerable report.
  const [localOnly, setLocalOnly] = useSessionState('nexus.spots.localOnly', true)
  // Hide worked (operator decision 2026-09-17): a station already in the log is not what an
  // operator is scanning this board for. DEFAULT ON, like the locality chip, and for the same
  // reason it prints its count: what it hides has to be visible and one click away.
  const [hideWorked, setHideWorked] = useSessionState('nexus.spots.hideWorked', true)
  // How far back "worked" reaches. Session-scoped like every other filter here, and validated on
  // read — a stale or hand-edited value falls back to the UTC day rather than hiding nothing.
  const [storedWindow, setWorkedWindow] = useSessionState<WorkedWindow>('nexus.spots.workedWindow', 'utcDay')
  const workedWindow = isWorkedWindow(storedWindow) ? storedWindow : 'utcDay'
  // US-state (WAS) filter, from the roster-resolved state on each spot. Empty = all.
  const [states, setStates] = useSessionState<string[]>('nexus.spots.states', [])
  // #174 — where a spot was REPORTED from: the continents and DXCC countries of every voice for
  // it (spotter + corroborators), resolved in Rust because the UI has no cty.dat. Empty = all.
  // A spot stays when ANY voice matches — the same "one voice is enough" rule as the continent
  // chip, which asks the narrower question "heard on MY continent".
  const [spotterConts, setSpotterConts] = useSessionState<string[]>('nexus.spots.spotterConts', [])
  const [spotterEntities, setSpotterEntities] = useSessionState<string[]>('nexus.spots.spotterEntities', [])

  const knownBands = useMemo(() => new Set(bandPlan.map((b) => b.band)), [bandPlan])

  const availableBands = useMemo(() => {
    const result = [...COMMON_BANDS]
    for (const s of spots) if (s.band && !result.includes(s.band)) result.push(s.band)
    return result
  }, [spots])
  // The SPECIFIC modes present in the firehose (skimmer submode, else the class label), in a
  // natural operating order (CW, Phone, then the digital submodes), unknowns trailing alpha.
  const availableModes = useMemo(() => {
    const set = new Set<string>()
    for (const s of spots) set.add(s.submode ?? s.mode)
    const order = ['CW', 'Phone', 'FT8', 'FT4', 'RTTY', 'PSK', 'Digital']
    const rank = (m: string) => {
      const i = order.indexOf(m)
      return i < 0 ? order.length : i
    }
    return [...set].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b))
  }, [spots])
  // US states present (resolved for stations heard before with a grid).
  const availableStates = useMemo(() => {
    const set = new Set<string>()
    for (const s of spots) if (s.state) set.add(s.state)
    return [...set].sort()
  }, [spots])

  // Toggle a mode's visibility: add/remove it from the hidden set (all shown by default).
  const toggleMode = (m: string) =>
    setHiddenModes((prev) => (prev.includes(m) ? prev.filter((x) => x !== m) : [...prev, m]))
  const toggleBand = (b: string) =>
    setBands((prev) => (prev.includes(b) ? prev.filter((x) => x !== b) : [...prev, b]))
  const toggleState = (st: string) =>
    setStates((prev) => (prev.includes(st) ? prev.filter((x) => x !== st) : [...prev, st]))
  // #174: only the continents and countries some voice in the current feed actually has, like
  // the state chips — a chip that can never match is a trap. Continents in cty.dat's fixed
  // order; countries alphabetical.
  const availableSpotterConts = useMemo(() => {
    const set = new Set<string>()
    for (const s of spots) for (const c of s.spotterConts ?? []) set.add(c)
    return CONTINENT_ORDER.filter((c) => set.has(c)).concat([...set].filter((c) => !CONTINENT_ORDER.includes(c)).sort())
  }, [spots])
  const availableSpotterEntities = useMemo(() => {
    const set = new Set<string>()
    for (const s of spots) for (const e of s.spotterEntities ?? []) set.add(e)
    return [...set].sort((a, b) => a.localeCompare(b))
  }, [spots])
  const toggleSpotterCont = (c: string) =>
    setSpotterConts((prev) => (prev.includes(c) ? prev.filter((x) => x !== c) : [...prev, c]))
  const toggleSpotterEntity = (e: string) =>
    setSpotterEntities((prev) => (prev.includes(e) ? prev.filter((x) => x !== e) : [...prev, e]))

  const hasActiveFilters =
    bands.length > 0 ||
    hiddenModes.length > 0 ||
    licensedOnly ||
    localOnly ||
    hideWorked ||
    states.length > 0 ||
    spotterConts.length > 0 ||
    spotterEntities.length > 0

  // The Needed board's alerts by call, for the rescue in `neededHere`.
  const needsByCall = useMemo(() => alertsByCall(needAlerts ?? []), [needAlerts])

  const { rows, workedHidden } = useMemo(() => {
    // Terms still narrow (AND), which is right here: a spot row is call + entity + spotter
    // + mode + band + frequency flattened together, so "20m ft8" means both. What changed is
    // that a term may now carry `*`/`?` and be matched as a whole-word pattern — `PA*` finds
    // the PA prefix here exactly as it does in the Stations list. A term without a wildcard
    // behaves as it always has, so nobody's saved habits move.
    const terms = searchTerms(query).map(compileTerm)
    // How many rows Hide worked is holding back RIGHT NOW — counted LAST, after everything else
    // the operator asked for, so the number on the chip is exactly what one click brings back.
    let worked = 0
    const filtered = spots.filter((s) => {
      if (licensedOnly && !s.licensed) return false
      if (localOnly && s.spotterLocal === false) return false
      if (hiddenModes.includes(s.submode ?? s.mode)) return false
      if (bands.length > 0 && !bands.includes(s.band)) return false
      // A state filter hides spots whose state is unknown (cluster spots of unheard stations).
      if (states.length > 0 && (!s.state || !states.includes(s.state))) return false
      // Spotted-from (#174): ANY voice on a chosen continent / in a chosen country keeps the
      // spot. A spot whose voices could not be placed matches nothing, like an unknown state.
      if (spotterConts.length > 0 && !(s.spotterConts ?? []).some((c) => spotterConts.includes(c))) return false
      if (spotterEntities.length > 0 && !(s.spotterEntities ?? []).some((e) => spotterEntities.includes(e))) return false
      if (terms.length > 0) {
        const hay = `${s.call} ${s.entity} ${s.spotter} ${s.mode} ${s.submode ?? ''} ${s.band} ${s.freqMhz.toFixed(4)}`.toUpperCase()
        for (const t of terms) if (!t(hay)) return false
      }
      // Worked LAST, and the rescue rides with it: a station you still need on THIS band and
      // mode stays whatever the log says, which is what makes hiding by callsign safe. So does
      // a station on your watch list — it counts as a need, worked or not (maintainer,
      // 2026-09-23), as it does under the Call Roster's Hide worked.
      if (
        hideWorked &&
        workedWithin(s, workedWindow) &&
        !neededHere(needsByCall.get(s.call.toUpperCase()), s.band, s.submode ?? s.mode) &&
        !watchOf({ call: s.call, entity: s.entity, grid: s.grid })
      ) {
        worked++
        return false
      }
      return true
    })
    const dir = sort.dir === 'asc' ? 1 : -1
    filtered.sort((a, b) => {
      let c = 0
      switch (sort.key) {
        case 'age':
          c = a.ageSecs - b.ageSecs
          break
        case 'call':
          c = a.call.localeCompare(b.call)
          break
        case 'entity':
          c = a.entity.localeCompare(b.entity)
          break
        case 'state':
          // Unknown states sort LAST both directions — '—' rows are noise when sorting by state.
          c = (a.state || '\u{10FFFF}').localeCompare(b.state || '\u{10FFFF}')
          break
        case 'band':
          c = a.freqMhz - b.freqMhz // band sort by frequency reads naturally
          break
        case 'freq':
          c = a.freqMhz - b.freqMhz
          break
        case 'mode':
          c = a.mode.localeCompare(b.mode)
          break
      }
      if (c === 0) c = a.ageSecs - b.ageSecs // tiebreak: newest first
      return c * dir
    })
    return { rows: filtered, workedHidden: worked }
  }, [spots, hiddenModes, bands, states, spotterConts, spotterEntities, sort, query, licensedOnly, localOnly, hideWorked, workedWindow, needsByCall, watchOf])

  // How many rows the locality filter is holding back RIGHT NOW — the honest half of a filter
  // that is on by default. Counted against everything else the operator has chosen, so it says
  // "hidden from what you asked for", not "hidden from the firehose".
  const farHidden = useMemo(
    () => (localOnly ? spots.filter((s) => s.spotterLocal === false).length : 0),
    [spots, localOnly],
  )

  const th = (key: SortKey, label: string) => (
    <button
      type="button"
      className={`np-th${sort.key === key ? ' active' : ''}`}
      onClick={() =>
        setSort((p) =>
          p.key === key ? { key, dir: p.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: 'asc' },
        )
      }
    >
      {label}
      {sort.key === key ? (sort.dir === 'asc' ? ' ▲' : ' ▼') : ''}
    </button>
  )

  return (
    <main className="layout single needed-panel spots-panel">
      <div className="np-head">
        <h2>{t('spots.title')}</h2>
        <span className="np-count">{rows.length}</span>
        {spots.length !== rows.length && <span className="np-count np-count-filtered">{t('spots.countFiltered', { count: spots.length })}</span>}
        <span className="np-hint">{control ? t('spots.hint') : t('remote.collectionObserver')}</span>
        <span className="np-search">
          <input
            type="search"
            value={query}
            placeholder={t('spots.search.placeholder')}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') setQuery('')
            }}
            aria-label={t('spots.search.label')}
          />
          {query && (
            <button type="button" className="np-search-clear" onClick={() => setQuery('')} title={t('spots.search.clear')}>
              ✕
            </button>
          )}
        </span>
        <button
          type="button"
          className={`np-filter-toggle${filtersOpen || hasActiveFilters ? ' active' : ''}`}
          onClick={() => setFiltersOpen((v) => !v)}
          title={t('spots.filter.toggle.title')}
          aria-expanded={filtersOpen}
        >
          <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <path d="M1 2.5A.5.5 0 0 1 1.5 2h13a.5.5 0 0 1 .354.854L10 8.707V14.5a.5.5 0 0 1-.724.447l-4-2A.5.5 0 0 1 5 12.5V8.707L1.146 2.854A.5.5 0 0 1 1 2.5z" />
          </svg>{' '}
          {hasActiveFilters ? t('spots.filter.toggle.active') : t('spots.filter.toggle.idle')}
        </button>
        {control && onPopOut && (
          <button type="button" className="np-popout" onClick={onPopOut} title={t('spots.popOut.title')}>
            {t('spots.popOut.label')}
          </button>
        )}
      </div>

      {(filtersOpen || hasActiveFilters) && (
        <div className="np-filters" role="group" aria-label={t('spots.filters.aria')}>
          <div className="np-filter-group np-filter-bands">
            {availableBands.map((band) => (
              <button
                key={band}
                type="button"
                className={`np-chip${bands.includes(band) ? ' active' : ''}`}
                onClick={() => toggleBand(band)}
              >
                {band}
              </button>
            ))}
          </div>
          {availableModes.length > 1 && (
            <>
              <div className="np-filter-sep" aria-hidden="true" />
              <div className="np-filter-group" role="group" aria-label={t('spots.filters.modes.aria')}>
                {availableModes.map((m) => {
                  const shown = !hiddenModes.includes(m)
                  return (
                    <button
                      key={m}
                      type="button"
                      className={`np-chip${shown ? ' active' : ''}`}
                      aria-pressed={shown}
                      onClick={() => toggleMode(m)}
                      title={
                        shown
                          ? t('spots.filter.mode.hide.title', { mode: m })
                          : t('spots.filter.mode.show.title', { mode: m })
                      }
                    >
                      {m}
                    </button>
                  )
                })}
              </div>
            </>
          )}
          {/* US-state chips — only states we could resolve (a station heard before with a grid). */}
          {availableStates.length > 0 && (
            <>
              <div className="np-filter-sep" aria-hidden="true" />
              <div className="np-filter-group" role="group" aria-label={t('spots.filters.states.aria')}>
                {availableStates.map((st) => (
                  <button
                    key={st}
                    type="button"
                    className={`np-chip${states.includes(st) ? ' active' : ''}`}
                    aria-pressed={states.includes(st)}
                    onClick={() => toggleState(st)}
                    title={t('spots.filter.state.title', { state: st })}
                  >
                    {st}
                  </button>
                ))}
              </div>
            </>
          )}
          {/* Spotted from (#174) — continents, then countries, of the voices in this feed. */}
          {availableSpotterConts.length > 0 && (
            <>
              <div className="np-filter-sep" aria-hidden="true" />
              <div className="np-filter-group" role="group" aria-label={t('spots.filters.spotterConts.aria')}>
                {availableSpotterConts.map((c) => (
                  <button
                    key={c}
                    type="button"
                    className={`np-chip${spotterConts.includes(c) ? ' active' : ''}`}
                    aria-pressed={spotterConts.includes(c)}
                    onClick={() => toggleSpotterCont(c)}
                    title={t('spots.filter.spotterCont.title', { continent: c })}
                  >
                    {c}
                  </button>
                ))}
              </div>
            </>
          )}
          {availableSpotterEntities.length > 0 && (
            <>
              <div className="np-filter-sep" aria-hidden="true" />
              <div className="np-filter-group" role="group" aria-label={t('spots.filters.spotterEntities.aria')}>
                {availableSpotterEntities.map((e) => (
                  <button
                    key={e}
                    type="button"
                    className={`np-chip${spotterEntities.includes(e) ? ' active' : ''}`}
                    aria-pressed={spotterEntities.includes(e)}
                    onClick={() => toggleSpotterEntity(e)}
                    title={t('spots.filter.spotterEntity.title', { country: e })}
                  >
                    {e}
                  </button>
                ))}
              </div>
            </>
          )}
          <div className="np-filter-sep" aria-hidden="true" />
          <button
            type="button"
            className={`np-chip${licensedOnly ? ' active' : ''}`}
            aria-pressed={licensedOnly}
            onClick={() => setLicensedOnly((v) => !v)}
            title={t('spots.filter.privileges.title')}
          >
            {t('spots.filter.privileges.label')}
          </button>
          {/* Locality. The count of what it is hiding rides ON the chip, so the answer to
              "where are my spots" is on screen rather than in a support thread. */}
          <button
            type="button"
            className={`np-chip${localOnly ? ' active' : ''}`}
            aria-pressed={localOnly}
            onClick={() => setLocalOnly((v) => !v)}
            title={t('spots.filter.local.title')}
          >
            {localOnly && farHidden > 0
              ? t('spots.filter.local.hidden', { count: farHidden })
              : t('spots.filter.local.label')}
          </button>
          {/* Hide worked + how far back it reaches. Same bargain as the chip above: on by
              default, and the count of what it is hiding is ON it, one click from coming back. */}
          <button
            type="button"
            className={`np-chip${hideWorked ? ' active' : ''}`}
            aria-pressed={hideWorked}
            onClick={() => setHideWorked((v) => !v)}
            title={t('spots.filter.worked.title')}
          >
            {hideWorked && workedHidden > 0
              ? t('spots.filter.worked.hidden', { count: workedHidden })
              : t('spots.filter.worked.label')}
          </button>
          <select
            className="sp-worked-window"
            value={String(workedWindow)}
            onChange={(e) =>
              setWorkedWindow(e.target.value === 'utcDay' ? 'utcDay' : (Number(e.target.value) as WorkedWindow))
            }
            aria-label={t('spots.filter.workedWindow.aria')}
            title={t('spots.filter.workedWindow.title')}
          >
            {/* The <option> VALUES are the persisted window; only the labels are prose. */}
            <option value="utcDay">{t('spots.filter.workedWindow.utcDay')}</option>
            <option value="3600">{t('spots.filter.workedWindow.hour1')}</option>
            <option value="14400">{t('spots.filter.workedWindow.hours4')}</option>
            <option value="86400">{t('spots.filter.workedWindow.hours24')}</option>
            <option value="604800">{t('spots.filter.workedWindow.days7')}</option>
          </select>
          {hasActiveFilters && (
            <button
              type="button"
              className="np-chip np-chip-clear"
              onClick={() => {
                setBands([])
                setHiddenModes([])
                setStates([])
                setSpotterConts([])
                setSpotterEntities([])
                setLicensedOnly(false)
                setLocalOnly(false)
                setHideWorked(false)
              }}
              title={t('spots.filter.clear.title')}
            >
              {t('spots.filter.clear.label')}
            </button>
          )}
        </div>
      )}

      <div className="np-grid sp-grid" role="table">
        <div className="np-row np-header" role="row">
          {th('age', t('spots.column.age'))}
          {th('call', t('spots.column.call'))}
          {th('entity', t('spots.column.entity'))}
          {th('state', t('spots.column.state'))}
          {th('band', t('spots.column.band'))}
          {th('freq', t('spots.column.freq'))}
          {th('mode', t('spots.column.mode'))}
          <span className="np-th-static">{t('spots.column.spotter')}</span>
          <span className="np-th-static">{t('spots.column.comment')}</span>
        </div>
        {rows.length === 0 ? (
          <div className="np-empty">
            {/* When Hide worked is what emptied the board, say so and name the way back — a
                default-on filter that leaves a blank panel is the report this feature must not
                generate. */}
            {workedHidden > 0
              ? t('spots.empty.worked', { count: workedHidden })
              : hasActiveFilters
                ? t('spots.empty.filtered')
                : t('spots.empty')}
          </div>
        ) : (
          rows.map((s) => {
            const workable = control || !!canWork?.(s)
            const canQsy = workable && knownBands.has(s.band)
            // Worked inside the window the chip is set to — so the badge below answers for the
            // rows Hide worked would drop, and says nothing about a contact from last month.
            const workedAge =
              s.workedAgoSecs != null && workedWithin(s, workedWindow)
                ? workedAgeLabel(s.workedAgoSecs)
                : null
            const watch = watchOf({ call: s.call, entity: s.entity, grid: s.grid })
            return (
              <div
                key={`${s.call}|${s.freqMhz}|${s.spotter}`}
                role="row"
                className={`np-row sp-row${s.call === selectedCall ? ' selected' : ''}`}
                title={
                  canQsy
                    ? t('spots.row.work.title', {
                        call: s.call,
                        mode: s.mode,
                        freq: s.freqMhz.toFixed(3),
                        spotter: s.spotter,
                      })
                    : t('spots.row.title', {
                        call: s.call,
                        freq: s.freqMhz.toFixed(3),
                        spotter: s.spotter,
                      })
                }
                onClick={() => {
                  onSelect(s.call)
                  if (workable) onWork(s)
                }}
              >
                <span className="np-age">{ageLabel(s.ageSecs)}</span>
                <span className="np-call">
                  <button
                    type="button"
                    className="qrz-link-call"
                    onClick={(e) => { e.stopPropagation(); void withErrorToast(() => openQrzPage(s.call), t('callbook.qrzPage.failed', { call: s.call })) }}
                    title={t('callbook.qrzPage.title', { call: s.call })}
                  >
                    {s.call}
                  </button>
                </span>
                {/* Entity then heading, same cell shape as the Needed board — the two
                    boards sit one click apart and have to read as one thing. */}
                <span className="np-entity">
                  <span className="np-name">{s.entity || '—'}</span>
                  {(() => {
                    const az = azimuthTo(myGrid, s.grid, s.entity, centroids)
                    return az ? (
                      <span className="np-az" title={azimuthTitle(az, s.entity)}>
                        {azimuthLabel(az)}
                      </span>
                    ) : null
                  })()}
                </span>
                {/* The panel already FILTERS by state; now it shows the value it filters on
                    (operator ask, 2026-08-16). FCC-index / heard-grid resolved; '—' = unknown
                    (a cluster spot of a station never heard, or a non-US/VE call). */}
                <span className="sp-state">{s.state || '—'}</span>
                <span className="np-band">{s.band || '—'}</span>
                <span className="sp-freq">{s.freqMhz.toFixed(3)}</span>
                <span
                  className={`np-mode-col np-mode-${s.mode.toLowerCase()}`}
                  title={
                    s.submode
                      ? t('spots.row.mode.submode.title', { submode: s.submode, mode: s.mode })
                      : t('spots.row.mode.title', { mode: s.mode })
                  }
                >
                  {s.submode ?? s.mode}
                </span>
                <span className="sp-spotter">{s.spotter}</span>
                <span className="np-why">
                  {/* The WATCH tile leads the cell, ahead of the worked badge: the comment
                      ellipsizes, and first is the one place the tile cannot be cut. */}
                  {watch && <WatchTile entry={watch} />}
                  {/* What Hide worked would drop, said on the row itself — so turning the chip
                      off answers "which of these have I worked, and when" without a tooltip.
                      In the widest column, because the age is the part worth reading. */}
                  {workedAge && (
                    <span
                      className="sp-worked"
                      title={t('spots.row.worked.title', { call: s.call, age: workedAge })}
                    >
                      {t('spots.row.worked', { age: workedAge })}
                    </span>
                  )}
                  {s.comment || (workedAge || watch ? '' : '—')}
                </span>
              </div>
            )
          })
        )}
      </div>
    </main>
  )
}
