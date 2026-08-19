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
import type { BandChannel, SpotRow } from '../types'
import { openQrzPage } from '../api'
import { withErrorToast } from '../toast'
import { azimuthLabel, azimuthTitle, azimuthTo } from '../grid'
import { useEntityCentroids } from '../features/entityCentroids'
import { t } from '../i18n'

type SortKey = 'age' | 'call' | 'entity' | 'state' | 'band' | 'freq' | 'mode'

// Common HF + 6m bands always offered in the filter bar; augmented with any band present
// in the current spots.
const COMMON_BANDS = ['160m', '80m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m']

/** Compact age string from seconds-since-received (−1 = unknown). A number and its unit
 * letter, with no prose in it at all — a measurement, so it is not a catalog string. */
function ageLabel(secs: number): string {
  if (secs < 0) return '—'
  if (secs < 60) return `${secs}s`
  if (secs < 3600) return `${Math.round(secs / 60)}m`
  return `${Math.round(secs / 3600)}h`
}

interface Props {
  spots: SpotRow[]
  bandPlan: BandChannel[]
  selectedCall: string | null
  onSelect: (call: string) => void
  /** Work the spot — QSY to its freq/mode and open the matching cockpit. */
  onWork: (spot: SpotRow) => void
  onPopOut?: () => void
  /** The operator's own square — origin for the beam heading beside each entity. A
   * cluster/RBN spot carries no grid, so that heading is always the entity centre. */
  myGrid?: string
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

export function SpotsPanel({ spots, bandPlan, selectedCall, onSelect, onWork, onPopOut, myGrid = '' }: Props) {
  // Entity centres — the only geometry the firehose carries (a spot has no grid).
  const centroids = useEntityCentroids()
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
  // US-state (WAS) filter, from the roster-resolved state on each spot. Empty = all.
  const [states, setStates] = useSessionState<string[]>('nexus.spots.states', [])

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

  const hasActiveFilters =
    bands.length > 0 || hiddenModes.length > 0 || licensedOnly || states.length > 0

  const rows = useMemo(() => {
    const terms = query.toLowerCase().split(/\s+/).filter(Boolean)
    const filtered = spots.filter((s) => {
      if (licensedOnly && !s.licensed) return false
      if (hiddenModes.includes(s.submode ?? s.mode)) return false
      if (bands.length > 0 && !bands.includes(s.band)) return false
      // A state filter hides spots whose state is unknown (cluster spots of unheard stations).
      if (states.length > 0 && (!s.state || !states.includes(s.state))) return false
      if (terms.length > 0) {
        const hay = `${s.call} ${s.entity} ${s.spotter} ${s.mode} ${s.submode ?? ''} ${s.band} ${s.freqMhz.toFixed(4)}`.toLowerCase()
        for (const t of terms) if (!hay.includes(t)) return false
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
    return filtered
  }, [spots, hiddenModes, bands, states, sort, query, licensedOnly])

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
        <span className="np-hint">{t('spots.hint')}</span>
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
        {onPopOut && (
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
          {hasActiveFilters && (
            <button
              type="button"
              className="np-chip np-chip-clear"
              onClick={() => {
                setBands([])
                setHiddenModes([])
                setStates([])
                setLicensedOnly(false)
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
            {hasActiveFilters ? t('spots.empty.filtered') : t('spots.empty')}
          </div>
        ) : (
          rows.map((s) => {
            const canQsy = knownBands.has(s.band)
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
                  onWork(s)
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
                    const az = azimuthTo(myGrid, null, s.entity, centroids)
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
                <span className="np-why">{s.comment || '—'}</span>
              </div>
            )
          })
        )}
      </div>
    </main>
  )
}
