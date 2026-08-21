// The N1MM-style "what's needed now" board: every needed station the engine sees
// (from the log — new DXCC/ATNO, new band-slot, new mode, new zone, needs-confirm),
// ranked by priority and boldly colored by the shared need palette. Single-click a
// row to QSY the radio to that band and listen. The same stations light up on the
// Connect map (shared needByCall), so this is the list half of "list + map".
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT come from it: the
// callsigns, DXCC entity names, band names, CQ zones, frequencies, headings and evidence lines
// the rows print (data), the MODE_OPTS labels (mode-class names), and the POTA/SOTA filter
// chips (the programmes' own names) — all invariant tokens, see `i18n/index.ts`. The need
// chips' words live in `features/needVisuals.ts`, which is migrated with that registry.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useRovingList } from '../useRovingList'
import type { BandChannel, FeedStatus, NeedAlert, NeedTag } from '../types'
import {
  filterAlerts,
  ageLabel,
  DEFAULT_FILTERS,
  ALL_MODES_ON,
  MODE_CLASSES,
  type NeededFilters,
  type NeedTypeFilter,
  type ModeClass,
  type ModeSet,
  NEED_TYPE_VALUES,
} from '../neededFilters'
import { pointRotator, readRotator, openQrzPage } from '../api'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { pushToast, withErrorToast } from '../toast'
import { RarityChip } from './RarityChip'
import { NEED_CHIP } from '../features/needVisuals'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { azimuthLabel, azimuthTitle, azimuthTo } from '../grid'
import { useEntityCentroids } from '../features/entityCentroids'

/** Defensive chip lookup — an unknown future tag renders visibly, never throws. */
function chipFor(tag: NeedTag): { label: string; cls: string; title: string } {
  return (
    NEED_CHIP[tag] ?? { label: String(tag).toUpperCase(), cls: 'confirm', title: String(tag) }
  )
}

/** Manual rotator control: live heading readout + point-at-azimuth. Rendered only
 * when a rotator is configured (same gate as the per-row ↗ buttons). */
function RotatorWidget() {
  const [az, setAz] = useState<number | null>(null)
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    let live = true
    const poll = () =>
      readRotator()
        .then((a) => live && setAz(a))
        .catch(() => {}) // best-effort — rotctld may be down; the readout just shows —
    poll()
    const id = window.setInterval(poll, 5000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])
  const point = async () => {
    // Mirrors the Go button's disabled gate — the Enter key path must never slew
    // the rotator on an empty field (Number('') is 0 → due North) or re-enter
    // while a turn is in flight.
    if (busy || input.trim() === '') return
    const deg = Number(input)
    if (!Number.isFinite(deg)) return
    const norm = ((deg % 360) + 360) % 360
    setBusy(true)
    try {
      await pointRotator(norm)
      pushToast(t('needed.rotator.pointed', { deg: Math.round(norm) }), 'success', 2500)
    } catch (e) {
      pushToast(typeof e === 'string' ? e : t('needed.rotator.failed'), 'error', 4000)
    } finally {
      setBusy(false)
    }
  }
  return (
    <span className="np-rotator" title={t('needed.rotator.title')}>
      <span className="np-rotator-az mono">{az != null ? `${Math.round(az)}°` : '—°'}</span>
      <input
        type="number"
        className="np-rotator-input mono"
        value={input}
        min={0}
        max={360}
        placeholder={t('needed.rotator.azimuth.placeholder')}
        aria-label={t('needed.rotator.azimuth.label')}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') void point()
        }}
      />
      <button
        type="button"
        className="np-rotator-go"
        disabled={busy || input.trim() === ''}
        onClick={() => void point()}
        title={t('needed.rotator.go.title')}
      >
        ↗
      </button>
    </span>
  )
}

type SortKey = 'priority' | 'call' | 'band' | 'entity' | 'mode' | 'zone'

// Persisted filter state key. PER-SURFACE: what THIS board shows — an HF-DX board beside
// a 2 m board wanting different filters is the whole point of a second window. (Note the
// bare name: any cleanup written as a glob over `nexus.*` would miss it.)
const FILTER_KEY = 'neededFilters'

function loadFilters(): NeededFilters {
  try {
    const raw = surfaceGet(FILTER_KEY)
    if (!raw) return { ...DEFAULT_FILTERS }
    const parsed = JSON.parse(raw) as Partial<NeededFilters> & { needType?: unknown }
    // Need-type is now a multi-select array (empty = All). Accept the new `needTypes`
    // array, else migrate the old single `needType` string. Sanitize each entry against
    // the KNOWN buckets — drop 'all' and any stale name from an older build so the board
    // never silently empties with no chip to explain why.
    const rawTypes: unknown[] = Array.isArray(parsed.needTypes)
      ? parsed.needTypes
      : parsed.needType != null
        ? [parsed.needType]
        : []
    const needTypes = rawTypes.filter(
      (t): t is NeedTypeFilter =>
        typeof t === 'string' && t !== 'all' && NEED_TYPE_VALUES.includes(t as NeedTypeFilter),
    )
    // Modes: each class independently on/off; missing/old persisted shapes default ON so
    // the board never silently hides a mode (the old single-select 'mode' is ignored —
    // upgrading users get all modes back, which is the point of this change).
    const pm = (parsed.modes ?? {}) as Partial<ModeSet>
    const modes: ModeSet = {
      Digital: typeof pm.Digital === 'boolean' ? pm.Digital : true,
      CW: typeof pm.CW === 'boolean' ? pm.CW : true,
      Phone: typeof pm.Phone === 'boolean' ? pm.Phone : true,
    }
    return {
      needTypes,
      bands: Array.isArray(parsed.bands) ? parsed.bands.filter((b) => typeof b === 'string') : [],
      modes,
    }
  } catch {
    return { ...DEFAULT_FILTERS }
  }
}

function saveFilters(f: NeededFilters): void {
  surfaceSet(FILTER_KEY, JSON.stringify(f))
}

// Band list shown in the filter bar: common HF + VHF bands (always present).
// In the rendered bar these are augmented with bands from current alerts.
const COMMON_BANDS = ['160m', '80m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m']

/** The need-type chips, in the order they are shown. Each VALUE is a persisted token. */
const NEED_TYPE_OPTS: NeedTypeFilter[] = [
  // (DXped restores the old "DXped only" toggle as a need-type chip.)
  'all',
  // Watch-list first: Wanted is the TOP tier (120) — the bucket existed
  // without a chip, so the rows it filters could be hidden but never asked for.
  'wanted',
  'atno',
  'newBand',
  'newMode',
  'newZone',
  'newGrid',
  'newState',
  'confirm',
  'dxped',
  'pota',
  'sota',
]

/** The programmes' own names — the same four letters in every language. */
const POTA_PROGRAM = 'POTA'
const SOTA_PROGRAM = 'SOTA'

/** The chip's word. A switch rather than a table so each key is a literal at its use. */
function needTypeLabel(value: NeedTypeFilter): string {
  switch (value) {
    case 'all':
      return t('needed.filter.all')
    case 'wanted':
      return t('needed.filter.wanted')
    case 'atno':
      return t('needed.filter.atno')
    case 'newBand':
      return t('needed.filter.newBand')
    case 'newMode':
      return t('needed.filter.newMode')
    case 'newZone':
      return t('needed.filter.newZone')
    case 'newGrid':
      return t('needed.filter.newGrid')
    case 'newState':
      return t('needed.filter.newState')
    case 'confirm':
      return t('needed.filter.confirm')
    case 'dxped':
      return t('needed.filter.dxped')
    case 'pota':
      return POTA_PROGRAM
    case 'sota':
      return SOTA_PROGRAM
  }
}

// Mode classes: the label IS the mode-class name, an invariant token in every language.
const MODE_OPTS: { value: ModeClass; label: string }[] = [
  { value: 'Digital', label: 'Digital' },
  { value: 'CW', label: 'CW' },
  { value: 'Phone', label: 'Phone' },
]

interface Props {
  alerts: NeedAlert[]
  bandPlan: BandChannel[]
  selectedCall: string | null
  /** The operator's own square — the origin for the beam heading beside each entity.
   * A need alert never carries the station's grid, so that heading is always the
   * entity centre; omit this (or leave the grid unset) and no heading is shown. */
  myGrid?: string
  /** QSY the rig to a digital need — to the SPOT's exact frequency (DXpeditions run off the
   * standard FT8/FT4 watering hole), falling back to the band's dial only when the spot
   * carries no frequency. The spotting network's freq/band is the source of truth. */
  onQsy: (alert: NeedAlert) => void
  /** Select/highlight a station (also lit on the map). */
  onSelect: (call: string) => void
  /** Click-to-work a VOICE/CW need: QSY to the spot, open the matching cockpit, prefill
   * the log. Omitted in the popped-out window (no cross-window nav) → those rows fall
   * back to a plain band QSY. */
  onWork?: (alert: NeedAlert) => void
  /** Point the antenna rotator at this need's call (great-circle bearing). Omitted when
   * no rotator is configured → the ↗ button is hidden. */
  onPoint?: (call: string) => void
  /** Pop this board out into its own window (omit when already standalone). */
  onPopOut?: () => void
  /** Liveness of the human DX-cluster node — the SSB/phone source — plus its host, so the
   * board can say "Phone source: ve7cc.net:23 · live" right where phone needs appear. This
   * is the ONLY source of Phone needs (RBN has no phone), so an empty board reads correctly:
   * "source up, nothing I need is spotted" vs "source down". Omitted in the pop-out window. */
  phoneSource?: { status: FeedStatus; host: string | null; spotsSeen: number } | null
  /** Open Settings at a section id (see settings/registry.ts). Omitted in the pop-out window
   * (no cross-window nav) → the phone-source line stays plain text there. */
  onOpenSettings?: (target: string) => void
}

/** Compact phone-source descriptor for the board header: [css class, short text, tooltip].
 * The css class is a token; the other two are prose about the host, which is data. */
function phoneSourceLabel(src: { status: FeedStatus; host: string | null }): [string, string, string] {
  const host = src.host ?? 'cluster'
  switch (src.status.state) {
    case 'live':
      return ['good', t('needed.phone.live.text', { host }), t('needed.phone.live.title', { host })]
    case 'connected':
      return [
        'good',
        t('needed.phone.connected.text', { host }),
        t('needed.phone.connected.title', { host }),
      ]
    case 'connecting':
    case 'waiting':
      return [
        'weak',
        t('needed.phone.connecting.text', { host }),
        t('needed.phone.connecting.title', { host }),
      ]
    case 'reconnecting':
      return ['bad', t('needed.phone.down.text', { host }), t('needed.phone.down.title', { host })]
    case 'idle':
      return ['ok', t('needed.phone.idle.text', { host }), t('needed.phone.idle.title', { host })]
    default:
      return [
        'weak',
        t('needed.phone.unknown.text', { host, state: src.status.state }),
        t('needed.phone.unknown.title', { host, state: src.status.state }),
      ]
  }
}

export function NeededPanel({
  alerts,
  bandPlan,
  selectedCall,
  myGrid = '',
  onQsy,
  onSelect,
  onWork,
  onPoint,
  onPopOut,
  phoneSource,
  onOpenSettings,
}: Props) {
  const [sort, setSort] = useState<{ key: SortKey; dir: 'asc' | 'desc' }>({
    key: 'priority',
    dir: 'desc',
  })
  const [filters, setFilters] = useState<NeededFilters>(loadFilters)
  const [filtersOpen, setFiltersOpen] = useState(false)
  // Persisted launch behavior for the detached window (read by App's auto-pop).
  const [autopop, setAutopop] = useState<boolean>(() => {
    try {
      return localStorage.getItem('nexus.needed.autopop') !== 'off'
    } catch {
      return true
    }
  })

  const knownBands = useMemo(() => new Set(bandPlan.map((b) => b.band)), [bandPlan])
  // Entity centres — the only geometry a need alert can offer, since it carries no grid.
  const centroids = useEntityCentroids()

  // All distinct bands present in the current alerts, merged with the common list.
  const availableBands = useMemo(() => {
    const alertBands = new Set(alerts.map((a) => a.band))
    // Preserve COMMON_BANDS order, then append any alert bands not in the common list.
    const result: string[] = []
    for (const b of COMMON_BANDS) {
      result.push(b)
    }
    for (const b of alertBands) {
      if (!result.includes(b)) result.push(b)
    }
    return result
  }, [alerts])

  const updateFilters = useCallback((next: NeededFilters) => {
    setFilters(next)
    saveFilters(next)
  }, [])

  const toggleNeedType = useCallback((value: NeedTypeFilter) => {
    setFilters((prev) => {
      // 'all' is the clear chip — it empties the multi-select. Every other value
      // toggles in/out, so several need types can be shown at once (like bands).
      const next: NeededFilters =
        value === 'all'
          ? { ...prev, needTypes: [] }
          : prev.needTypes.includes(value)
            ? { ...prev, needTypes: prev.needTypes.filter((t) => t !== value) }
            : { ...prev, needTypes: [...prev.needTypes, value] }
      saveFilters(next)
      return next
    })
  }, [])

  const toggleBand = useCallback((band: string) => {
    setFilters((prev) => {
      const next: NeededFilters = prev.bands.includes(band)
        ? { ...prev, bands: prev.bands.filter((b) => b !== band) }
        : { ...prev, bands: [...prev.bands, band] }
      saveFilters(next)
      return next
    })
  }, [])

  const toggleMode = useCallback((mode: ModeClass) => {
    setFilters((prev) => {
      const next: NeededFilters = {
        ...prev,
        modes: { ...prev.modes, [mode]: !prev.modes[mode] },
      }
      saveFilters(next)
      return next
    })
  }, [])

  const clearFilters = useCallback(() => {
    updateFilters({ ...DEFAULT_FILTERS, modes: { ...ALL_MODES_ON } })
  }, [updateFilters])

  const hasActiveFilters =
    filters.needTypes.length > 0 ||
    filters.bands.length > 0 ||
    MODE_CLASSES.some((c) => !filters.modes[c])

  const rows = useMemo(() => {
    const filtered = filterAlerts(alerts, filters)
    const dir = sort.dir === 'asc' ? 1 : -1
    filtered.sort((a, b) => {
      let c = 0
      switch (sort.key) {
        case 'priority':
          c = a.priority - b.priority
          break
        case 'call':
          c = a.call.localeCompare(b.call)
          break
        case 'band':
          c = a.band.localeCompare(b.band)
          break
        case 'entity':
          c = a.entity.localeCompare(b.entity)
          break
        case 'mode':
          c = a.mode.localeCompare(b.mode)
          break
        case 'zone':
          // zone 0 = unknown → keep at the end of ascending.
          c = (a.zone || 99) - (b.zone || 99)
          break
      }
      if (c === 0) c = b.priority - a.priority // tiebreak: hottest first
      return c * dir
    })
    return filtered
  }, [alerts, sort, filters])

  // Keyboard: arrow through rows, Enter selects + works/QSYs (same as a click).
  const roving = useRovingList(rows.length, (i) => {
    const a = rows[i]
    if (!a) return
    onSelect(a.call)
    if (onWork) onWork(a)
    else onQsy(a)
  })

  const th = (key: SortKey, label: string) => (
    <button
      type="button"
      className={`np-th${sort.key === key ? ' active' : ''}`}
      onClick={() =>
        setSort((p) =>
          p.key === key
            ? { key, dir: p.dir === 'asc' ? 'desc' : 'asc' }
            : { key, dir: key === 'priority' ? 'desc' : 'asc' },
        )
      }
    >
      {label}
      {sort.key === key ? (sort.dir === 'asc' ? ' ▲' : ' ▼') : ''}
    </button>
  )

  return (
    <main className="layout single needed-panel">
      <div className="np-head">
        <h2>{t('needed.title')}</h2>
        <span className="np-count">{rows.length}</span>
        {alerts.length !== rows.length && (
          <span className="np-count np-count-filtered">
            {t('needed.countFiltered', { count: alerts.length })}
          </span>
        )}
        <span className="np-hint">{t('needed.hint')}</span>
        {/* Filter toggle button */}
        <button
          type="button"
          className={`np-filter-toggle${filtersOpen || hasActiveFilters ? ' active' : ''}`}
          onClick={() => setFiltersOpen((v) => !v)}
          title={t('needed.filter.toggle.title')}
          aria-expanded={filtersOpen}
        >
          {/* funnel icon as inline SVG */}
          <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <path d="M1 2.5A.5.5 0 0 1 1.5 2h13a.5.5 0 0 1 .354.854L10 8.707V14.5a.5.5 0 0 1-.724.447l-4-2A.5.5 0 0 1 5 12.5V8.707L1.146 2.854A.5.5 0 0 1 1 2.5z"/>
          </svg>{' '}
          {hasActiveFilters ? t('needed.filter.toggle.active') : t('needed.filter.toggle.idle')}
        </button>
        {onPoint && <RotatorWidget />}
        {onPopOut && (
          <button
            type="button"
            className="np-popout"
            onClick={onPopOut}
            title={t('needed.popOut.title')}
          >
            {t('needed.popOut.label')}
          </button>
        )}
        <label className="np-autopop" title={t('needed.autoPop.title')}>
          <input
            type="checkbox"
            checked={autopop}
            onChange={(e) => {
              setAutopop(e.target.checked)
              try {
                localStorage.setItem('nexus.needed.autopop', e.target.checked ? 'on' : 'off')
              } catch {
                /* storage blocked — applies this session only */
              }
            }}
          />
          <span>{t('needed.autoPop.label')}</span>
        </label>
      </div>

      {/* Phone-source liveness — Phone needs come ONLY from the human DX-cluster node, so a
          dead/absent source explains an empty Phone column at a glance (RBN covers CW/digital). */}
      {phoneSource &&
        (phoneSource.status.enabled ? (
          (() => {
            const [cls, text, title] = phoneSourceLabel(phoneSource)
            // Diagnostic split: SSB spots actually received vs how many became needs.
            // 0 spots → SSB isn't reaching the app; spots>0 but needs=0 → arriving, but
            // none are a need for your log (so an empty Phone column is correct, not a bug).
            const ssb = phoneSource.spotsSeen
            const phoneNeeds = alerts.filter((a) => a.mode === 'Phone').length
            return (
              <div className={`np-phone-src ${cls}`} title={title}>
                {text}
                {t('needed.phone.spots', { count: ssb })}
                {t('needed.phone.needs', { count: phoneNeeds })}
              </div>
            )
          })()
        ) : (
          <div className="np-phone-src weak" title={t('needed.phone.off.title')}>
            {/* The toggle and the host field live in Integrations & Feeds; this line used to
                send the operator to "Settings ▸ Connections", which is the connectivity LOG
                (and was written as a tab, which it has never been). Now it takes them there.
                One sentence with the link as a MARKER — the button comes from here, never
                from the catalog. */}
            {onOpenSettings ? (
              <T
                k="needed.phone.off.link"
                tags={{
                  a: (
                    <button
                      type="button"
                      className="settings-linkbtn"
                      onClick={() => onOpenSettings('integrations-feeds')}
                    />
                  ),
                }}
              />
            ) : (
              t('needed.phone.off.plain')
            )}
          </div>
        ))}

      {/* Filter bar — visible when toggled open or when any filter is active */}
      {(filtersOpen || hasActiveFilters) && (
        <div className="np-filters" role="group" aria-label={t('needed.filters.aria')}>
          {/* Need type chips */}
          <div className="np-filter-group">
            {NEED_TYPE_OPTS.map((value) => {
              const active =
                value === 'all' ? filters.needTypes.length === 0 : filters.needTypes.includes(value)
              return (
                <button
                  key={value}
                  type="button"
                  className={`np-chip${active ? ' active' : ''}`}
                  onClick={() => toggleNeedType(value)}
                >
                  {needTypeLabel(value)}
                </button>
              )
            })}
          </div>

          <div className="np-filter-sep" aria-hidden="true" />

          {/* Band multi-select chips */}
          <div className="np-filter-group np-filter-bands">
            {availableBands.map((band) => (
              <button
                key={band}
                type="button"
                className={`np-chip${filters.bands.includes(band) ? ' active' : ''}`}
                onClick={() => toggleBand(band)}
              >
                {band}
              </button>
            ))}
          </div>

          <div className="np-filter-sep" aria-hidden="true" />

          {/* Mode chips — multi-select: tick the modes you operate (a non-CW op hides CW).
              Independent toggles, not exclusive; an "off" mode is dimmed. */}
          <div className="np-filter-group" role="group" aria-label={t('needed.filters.modes.aria')}>
            {MODE_OPTS.map((opt) => (
              <button
                key={opt.value}
                type="button"
                className={`np-chip${filters.modes[opt.value] ? ' active' : ''}`}
                aria-pressed={filters.modes[opt.value]}
                onClick={() => toggleMode(opt.value)}
                title={
                  filters.modes[opt.value]
                    ? t('needed.filter.mode.hide.title', { mode: opt.label })
                    : t('needed.filter.mode.show.title', { mode: opt.label })
                }
              >
                {opt.label}
              </button>
            ))}
          </div>

          {hasActiveFilters && (
            <button
              type="button"
              className="np-chip np-chip-clear"
              onClick={clearFilters}
              title={t('needed.filter.clear.title')}
            >
              {t('needed.filter.clear.label')}
            </button>
          )}
        </div>
      )}

      <div
        className="np-grid"
        role="grid"
        aria-label={t('needed.grid.aria')}
        onKeyDown={roving.containerProps.onKeyDown}
      >
        <div className="np-row np-header" role="row">
          {th('priority', t('needed.column.need'))}
          {th('call', t('needed.column.call'))}
          {th('entity', t('needed.column.entity'))}
          {th('band', t('needed.column.band'))}
          {th('mode', t('needed.column.mode'))}
          {th('zone', t('needed.column.zone'))}
          <span className="np-th-static">{t('needed.column.why')}</span>
        </div>
        {rows.length === 0 ? (
          <div className="np-empty">
            {hasActiveFilters ? t('needed.empty.filtered') : t('needed.empty')}
          </div>
        ) : (
          rows.map((a, i) => {
            const canQsy = knownBands.has(a.band)
            const isVoiceCw = a.mode === 'CW' || a.mode === 'Phone'
            // Every mode is click-to-work when the host wires onWork (main window):
            // the work path QSYs the rig AND opens the matching cockpit — a Digital
            // need must land in the FT8 cockpit, not just move the dial (the
            // "radio switched but the app didn't" bug). The pop-out has no onWork,
            // so its rows fall back to the QSY-only branch below.
            const workable = !!onWork
            const age = ageLabel(a.admittedAt)
            const evidenceLine = a.evidence
              ? (age ? `${a.evidence} · ${age}` : a.evidence)
              : null
            // A whole sentence per state, never a stem plus a tail: the dial frequency lands
            // in a different place in different languages. `toFixed(3)` is the invariant
            // frequency formatter — the string it makes is passed through untouched.
            const tooltipBody = workable
              ? a.freqMhz
                ? t('needed.row.work.titleFreq', {
                    call: a.call,
                    mode: a.mode,
                    band: a.band,
                    freq: a.freqMhz.toFixed(3),
                  })
                : t('needed.row.work.title', { call: a.call, mode: a.mode, band: a.band })
              : isVoiceCw
                ? t('needed.row.mainWindow.title', { call: a.call, mode: a.mode })
                : canQsy
                  ? a.freqMhz
                    ? t('needed.row.qsy.titleFreq', { freq: a.freqMhz.toFixed(3), call: a.call })
                    : t('needed.row.qsy.titleBand', { band: a.band, call: a.call })
                  : a.headline
            const fullTooltip = evidenceLine
              ? `${tooltipBody}\n${evidenceLine}`
              : tooltipBody
            // Always the entity centre here — a need alert carries no grid of its own.
            const az = azimuthTo(myGrid, null, a.entity, centroids)
            return (
              <div
                key={`${a.call}|${a.band}|${a.mode}`}
                role="row"
                aria-selected={a.call === selectedCall}
                aria-label={
                  az
                    ? t('needed.row.ariaAzimuth', {
                        call: a.call,
                        entity: a.entity,
                        deg: az.deg,
                        band: a.band,
                        mode: a.mode,
                        tags: a.tags.join(' '),
                      })
                    : t('needed.row.aria', {
                        call: a.call,
                        entity: a.entity,
                        band: a.band,
                        mode: a.mode,
                        tags: a.tags.join(' '),
                      })
                }
                tabIndex={roving.rowProps(i).tabIndex}
                ref={roving.rowProps(i).ref as (el: HTMLDivElement | null) => void}
                onFocus={roving.rowProps(i).onFocus}
                className={`np-row${a.call === selectedCall ? ' selected' : ''} need-${
                  a.tags[0] ? chipFor(a.tags[0]).cls : 'confirm'
                }`}
                title={fullTooltip}
                onClick={() => {
                  roving.setActive(i)
                  onSelect(a.call)
                  if (workable) onWork(a)
                  else if (canQsy) onQsy(a)
                }}
              >
                <span className="np-need">
                  {a.tags.map((tag) => (
                    <span
                      key={tag}
                      className={`need-chip need-${chipFor(tag).cls}`}
                      title={chipFor(tag).title}
                    >
                      {chipFor(tag).label}
                    </span>
                  ))}
                </span>
                <span className="np-call">
                  <button
                    type="button"
                    className="qrz-link-call"
                    onClick={(e) => { e.stopPropagation(); void withErrorToast(() => openQrzPage(a.call), t('callbook.qrzPage.failed', { call: a.call })) }}
                    title={t('callbook.qrzPage.title', { call: a.call })}
                  >
                    {a.call}
                  </button>
                  <RarityChip rarity={a.gridRarity} />
                  {onPoint && (
                    <button
                      type="button"
                      className="np-point"
                      title={t('needed.row.point.title', { call: a.call })}
                      onClick={(e) => {
                        e.stopPropagation()
                        onPoint(a.call)
                      }}
                    >
                      ↗
                    </button>
                  )}
                </span>
                {/* Entity, then its beam heading — the order every other logger uses.
                    Two spans, not one string: the name has to keep ellipsising in a
                    narrow column and the heading has to survive when it does. */}
                <span className="np-entity">
                  <span className="np-name">{a.entity || '—'}</span>
                  {az && (
                    <span className="np-az" title={azimuthTitle(az, a.entity)}>
                      {azimuthLabel(az)}
                    </span>
                  )}
                </span>
                <span className="np-band">{a.band}</span>
                <span
                  className={`np-mode-col np-mode-${a.mode.toLowerCase()}`}
                  title={t('needed.row.mode.title', { mode: a.mode })}
                >
                  {a.mode}
                </span>
                <span className="np-zone">{a.zone > 0 ? a.zone : '—'}</span>
                <span className="np-why">
                  {a.headline}
                  {evidenceLine && (
                    <span className="np-evidence">{evidenceLine}</span>
                  )}
                </span>
              </div>
            )
          })
        )}
      </div>
    </main>
  )
}
