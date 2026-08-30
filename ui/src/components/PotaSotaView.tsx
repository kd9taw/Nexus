/**
 * POTA / SOTA hunter view — pure hunter; the activator panel is intentionally
 * absent. The operator finds activators on the air now, clicks Hunt to QSY and
 * tag the next logged QSO with the park/summit reference.
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import { TreePine, Mountain, RefreshCw, X } from 'lucide-react'
import type { AppSnapshot, OtaSpot, Activation } from '../types'
import {
  clearHuntTarget,
  getOtaSpots,
  openPanelWindow,
  setHuntTarget,
  setActivation,
  clearActivation,
  getActivation,
  parksCount,
  huntedParksCount,
  downloadParks,
  importParksCsv,
  importHuntedParksCsv,
} from '../api'
import { pushToast, withErrorToast } from '../toast'
import { bandFromKhz, spotModeClass } from '../otaHunt'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { t } from '../i18n'
import { T } from '../i18n/T'

type Program = 'POTA' | 'SOTA' | 'Both'

/**
 * ⚠️ INVARIANT TOKENS — never translated, never in the catalog.
 *
 * POTA and SOTA are the programmes' own names, so the view's title and the activation
 * picker's two options print them as they are (a translated programme name names nothing).
 * The two references are the FORMAT the operator types, and the sources are websites.
 * Only "Both" — an English word, not a programme — is prose, and it has a catalog entry.
 */
const OTA_TITLE = 'POTA / SOTA'
const ACT_PROGRAMS = ['POTA', 'SOTA'] as const
const REF_EXAMPLES = { POTA: 'K-1234', SOTA: 'W7A/MN-001' } as const
const SOURCES: Record<Program, string> = {
  POTA: 'pota.app',
  SOTA: 'SOTAwatch',
  Both: 'pota.app + SOTAwatch',
}

/** kHz → "14.0740 MHz" display string (4 decimal places = 10 Hz resolution). */
function fmtFreq(khz: number): string {
  return `${(khz / 1000).toFixed(4)} MHz`
}

/** Truncate a park/summit name to `max` chars, appending '…' when cut. */
function truncName(name: string, max = 28): string {
  if (name.length <= max) return name
  return name.slice(0, max - 1) + '…'
}

/** Derive a unique, stable spot key for React list rendering. */
function spotKey(s: OtaSpot): string {
  return `${s.program}|${s.reference}|${s.activator}|${s.freqKhz}`
}

/** Sort spots: bandOpen first, then newPark, then by most-recent (preserve API order). */
/** How the spot list is ordered. 'value' is the default: workable-now first (band open,
 * then a new park) — the "why should I care" ranking. The rest are plain column sorts
 * (sortable-everywhere, 2026-07-21); this list is cards, not a grid, so the control is a
 * picker rather than clickable headers. */
export type OtaSort = 'value' | 'activator' | 'reference' | 'band' | 'mode'
const OTA_SORTS: readonly OtaSort[] = ['value', 'activator', 'reference', 'band', 'mode']
function isOtaSort(v: unknown): v is OtaSort {
  return typeof v === 'string' && (OTA_SORTS as readonly string[]).includes(v)
}

export function sortSpots(spots: OtaSpot[], key: OtaSort, asc: boolean): OtaSpot[] {
  const val = (s: OtaSpot): string | number => {
    switch (key) {
      case 'activator':
        return s.activator.toUpperCase()
      case 'reference':
        return s.reference.toUpperCase()
      case 'band':
        return s.freqKhz // frequency orders bands meaningfully
      case 'mode':
        return spotDisplayMode(s.mode)
      case 'value':
        return s.bandOpen ? 2 : s.newPark ? 1 : 0
    }
  }
  return [...spots].sort((a, b) => {
    const va = val(a)
    const vb = val(b)
    const c =
      typeof va === 'number' && typeof vb === 'number'
        ? va - vb
        : String(va).localeCompare(String(vb))
    // Direction is uniform across keys so the arrow glyph never lies: asc -> ▲ (A→Z,
    // low→high, worst-first), desc -> ▼. 'value' reads best-first at the shipped default
    // because that default is DESCENDING (sortAsc=false), not because this key inverts.
    const primary = asc ? c : -c
    if (primary !== 0) return primary
    const av = a.bandOpen ? 2 : a.newPark ? 1 : 0
    const bv = b.bandOpen ? 2 : b.newPark ? 1 : 0
    return bv - av
  })
}

// All distinct modes present in the spot list (upper-cased for display).
const KNOWN_MODES = ['SSB', 'CW', 'FT8', 'FT4']
function spotDisplayMode(m: string): string {
  const u = m.trim().toUpperCase()
  return u || 'OTHER'
}

export interface OtaSpotClickArg {
  /** Activator callsign. */
  call: string
  /** Dial frequency in MHz. */
  freqMhz: number
  /** Band label (e.g. "20m"). */
  band: string
  /** Mode class — routes to the right cockpit. */
  modeClass: 'CW' | 'Phone' | 'Digital'
  /** The program ("POTA" | "SOTA") and reference for hunt-tagging. */
  program: string
  reference: string
}

interface Props {
  /** The current app snapshot — provides snap.hunt for the hunting banner. */
  snap: AppSnapshot
  /** Called when the operator clicks HUNT on a spot row.
   * App.tsx wires this to setHuntTarget + the same QSY path as handleWorkNeeded. */
  onHunt: (arg: OtaSpotClickArg) => void
  /** Called after clearHuntTarget completes so App can apply the fresh snapshot. */
  onSnap: (s: AppSnapshot) => void
  /** True in the torn-off window — hides the pop-out button there (the
   *  FieldDayScoreboard shape: the view opens its own window directly, so the
   *  docked and detached mounts stay one component). */
  detached?: boolean
}

export function PotaSotaView({ snap, onHunt, onSnap, detached = false }: Props) {
  // Program + band filter persist for the same reason the sort and mode do: the operator
  // filed "leaving and returning resets all filters" as a bug. A stale/hand-edited value
  // falls back to the default rather than throwing.
  // PER-SURFACE, all of them: program, filters and sort describe what THIS board is
  // showing. A POTA board beside a SOTA board is the multi-window payoff.
  const [program, setProgram] = useState<Program>(() => {
    const raw = surfaceGet('nexus.ota.program')
    return raw === 'POTA' || raw === 'SOTA' || raw === 'Both' ? raw : 'POTA'
  })
  const [spots, setSpots] = useState<OtaSpot[]>([])
  const [loading, setLoading] = useState(false)
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null)
  // Band filter — set of band strings; empty = All.
  const [bandFilter, setBandFilter] = useState<string[]>(() => {
    try {
      const raw = surfaceGet('nexus.ota.bandFilter')
      const v: unknown = raw == null ? null : JSON.parse(raw)
      return Array.isArray(v) && v.every((x) => typeof x === 'string') ? v : []
    } catch {
      return []
    }
  })
  // Mode filter — a display-mode string or 'All'. Defaults to 'All' so phone/SSB
  // hunters (the POTA majority) see every spot out of the box, and REMEMBERS the
  // operator's last choice across reloads — so a CW hunter sets 'CW' once and it
  // sticks, without hiding SSB activity from everyone else on first run.
  // Sort choice survives leaving the view — same remount-state-loss class the operator
  // filed against the Spots panel. Stored raw like modeFilter below (no JSON) so a
  // hand-edited or stale value simply falls back to the default.
  const [sortKey, setSortKey] = useState<OtaSort>(() => {
    const raw = surfaceGet('nexus.ota.sortKey')
    return isOtaSort(raw) ? raw : 'value'
  })
  const [sortAsc, setSortAsc] = useState(() => surfaceGet('nexus.ota.sortAsc') === '1')
  useEffect(() => {
    surfaceSet('nexus.ota.sortKey', sortKey)
  }, [sortKey])
  useEffect(() => {
    surfaceSet('nexus.ota.sortAsc', sortAsc ? '1' : '0')
  }, [sortAsc])
  useEffect(() => {
    surfaceSet('nexus.ota.program', program)
  }, [program])
  useEffect(() => {
    surfaceSet('nexus.ota.bandFilter', JSON.stringify(bandFilter))
  }, [bandFilter])
  const [modeFilter, setModeFilter] = useState<string>(
    () => surfaceGet('nexus.ota.modeFilter') ?? 'All',
  )
  useEffect(() => {
    surfaceSet('nexus.ota.modeFilter', modeFilter)
  }, [modeFilter])

  const loadSpots = useCallback(async (p: Program) => {
    setLoading(true)
    let loaded: OtaSpot[] = []
    if (p === 'Both') {
      const [pota, sota] = await Promise.all([
        withErrorToast(
          () => getOtaSpots('POTA'),
          t('ota.spots.failed', { program: 'POTA' }),
        ).then((s) => s ?? []),
        withErrorToast(
          () => getOtaSpots('SOTA'),
          t('ota.spots.failed', { program: 'SOTA' }),
        ).then((s) => s ?? []),
      ])
      loaded = [...pota, ...sota]
    } else {
      const s = await withErrorToast(() => getOtaSpots(p), t('ota.spots.failed', { program: p }))
      loaded = s ?? []
    }
    setLoading(false)
    setSpots(loaded)
    setLastUpdated(new Date())
  }, [])

  // Initial load
  useEffect(() => {
    void loadSpots(program)
  }, [program, loadSpots])

  // Auto-poll every 60 s
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  useEffect(() => {
    if (intervalRef.current) clearInterval(intervalRef.current)
    intervalRef.current = setInterval(() => void loadSpots(program), 60_000)
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current)
    }
  }, [program, loadSpots])

  // Derive the set of distinct bands in the current spot list (for filter chips).
  const availableBands = (() => {
    const seen = new Set<string>()
    for (const s of spots) {
      const b = bandFromKhz(s.freqKhz)
      if (b !== '?') seen.add(b)
    }
    // Order by HF → VHF
    const ORDER = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '2m']
    return ORDER.filter((b) => seen.has(b))
  })()

  // Derive the set of distinct modes in the current spot list.
  const availableModes = (() => {
    const seen = new Set<string>()
    for (const s of spots) seen.add(spotDisplayMode(s.mode))
    // Preferred order first, then append any others.
    const result: string[] = []
    for (const m of KNOWN_MODES) if (seen.has(m)) result.push(m)
    for (const m of seen) if (!KNOWN_MODES.includes(m)) result.push(m)
    return result
  })()

  // Filter + sort
  const filtered = sortSpots(
    spots.filter((s) => {
      // Always honor the selected program — guards against stale spots lingering from the
      // previous program during an in-flight re-fetch (the "I'm on SOTA but still see POTA"
      // bug), and any mixed array. 'Both' shows everything.
      if (program !== 'Both' && s.program !== program) return false
      if (bandFilter.length > 0 && !bandFilter.includes(bandFromKhz(s.freqKhz))) return false
      if (modeFilter !== 'All' && spotDisplayMode(s.mode) !== modeFilter) return false
      return true
    }),
    sortKey,
    sortAsc,
  )

  const hunt = snap.hunt ?? null

  // My-side activation: the backend stamps my park ref onto every QSO I log while active.
  const [act, setAct] = useState<Activation | null>(null)
  const [actRef, setActRef] = useState('')
  const [actProg, setActProg] = useState('POTA')
  useEffect(() => {
    void getActivation()
      .then(setAct)
      .catch(() => {})
  }, [])
  const activating = act != null && act.reference != null

  const handleStartActivation = async () => {
    const ref = actRef.trim().toUpperCase()
    if (!ref) return
    const a = await withErrorToast(
      () => setActivation(actProg, ref),
      t('ota.activation.startFailed'),
    )
    if (a) {
      setAct(a)
      pushToast(
        // Both are non-null on a started activation; `?? ''` only keeps the types honest.
        t('ota.activation.started', { program: a.program ?? '', reference: a.reference ?? '' }),
        'success',
      )
    }
  }
  const handleStopActivation = async () => {
    const a = await withErrorToast(() => clearActivation(), t('ota.activation.stopFailed'))
    if (a) {
      setAct(a)
      setActRef('')
      pushToast(t('ota.activation.ended'), 'info', 2000)
    }
  }

  // Local park directory — download once / import a CSV, then search it offline in the log form.
  const [parkN, setParkN] = useState(0)
  const [parkBusy, setParkBusy] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)
  // Imported "Hunted Parks.CSV" — marks parks worked so new-park flags are right on CW hunts.
  const [huntedN, setHuntedN] = useState(0)
  const huntedFileRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    void parksCount()
      .then(setParkN)
      .catch(() => {})
    // Re-hydrate the imported-hunted-parks count so the button reflects a prior import after a
    // restart (the worked-set itself is reloaded from cache on the Rust side at startup).
    void huntedParksCount()
      .then(setHuntedN)
      .catch(() => {})
  }, [])
  const handleDownloadParks = async () => {
    setParkBusy(true)
    const n = await withErrorToast(() => downloadParks(), t('ota.parks.downloadFailed'))
    setParkBusy(false)
    if (n != null) {
      setParkN(n)
      // The count is grouped for display HERE, as it always was — `t()` never formats a
      // number (see the invariant-token rule in i18n/index.ts).
      pushToast(t('ota.parks.downloaded', { formatted: n.toLocaleString() }), 'success')
    }
  }
  const handleImportFile = async (file: File) => {
    setParkBusy(true)
    try {
      const csv = await file.text()
      const n = await importParksCsv(csv)
      setParkN(n)
      pushToast(t('ota.parks.imported', { formatted: n.toLocaleString() }), 'success')
    } catch (e) {
      pushToast(t('ota.parks.importFailed', { detail: String(e) }), 'error')
    } finally {
      setParkBusy(false)
    }
  }

  const handleImportHuntedFile = async (file: File) => {
    setParkBusy(true)
    try {
      const csv = await file.text()
      const n = await importHuntedParksCsv(csv)
      setHuntedN(n)
      pushToast(t('ota.hunted.imported', { formatted: n.toLocaleString() }), 'success')
      void loadSpots(program) // refresh NEW PARK badges against the new worked-set
    } catch (e) {
      pushToast(t('ota.hunted.importFailed', { detail: String(e) }), 'error')
    } finally {
      setParkBusy(false)
    }
  }

  const handleClearHunt = async () => {
    const s = await withErrorToast(() => clearHuntTarget(), t('ota.hunt.clearFailed'))
    if (s) {
      onSnap(s)
      pushToast(t('ota.hunt.cleared'), 'info', 2000)
    }
  }

  const handleHunt = async (s: OtaSpot) => {
    const freqMhz = s.freqKhz / 1000
    const band = bandFromKhz(s.freqKhz)
    const modeClass = spotModeClass(s.mode)

    // Tag the next QSO with this activator's park/summit.
    const snap2 = await withErrorToast(
      () => setHuntTarget(s.activator, s.program, s.reference),
      t('ota.hunt.setFailed', { call: s.activator }),
    )
    if (snap2) onSnap(snap2)

    // QSY + open the matching cockpit — same path as handleWorkNeeded.
    onHunt({ call: s.activator, freqMhz, band, modeClass, program: s.program, reference: s.reference })
  }

  const progIcon = (p: Program) => {
    if (p === 'SOTA') return <Mountain size={13} aria-hidden="true" />
    if (p === 'Both') return <><TreePine size={13} aria-hidden="true" /><Mountain size={13} aria-hidden="true" /></>
    return <TreePine size={13} aria-hidden="true" />
  }

  const lastUpdatedLabel = lastUpdated
    ? t('ota.lastUpdated', {
        time: lastUpdated.toLocaleTimeString([], {
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit',
        }),
      })
    : ''

  return (
    <section className="panel pota-view pota-hunter">
      <div className="panel-header">
        <h2>{OTA_TITLE}</h2>
        <span className="awards-sub">{t('ota.subtitle')}</span>
        {/* Multi-monitor tear-off — the pop-out the per-surface filter records were
            built for: a POTA board beside a SOTA board, each keeping its own
            program/filter/sort. Hidden in the already-torn-off window. */}
        {!detached && (
          <button
            type="button"
            className="pota-popout"
            onClick={() => void openPanelWindow('pota')}
            title={t('ota.popOut.title')}
          >
            {t('ota.popOut.label')}
          </button>
        )}
      </div>

      {/* Hunting banner — shown when a hunt target is active */}
      {hunt && (
        <div className="pota-hunt-banner" role="status" aria-live="polite">
          <span className="pota-hunt-icon" aria-hidden="true">{progIcon(hunt.program as Program)}</span>
          <span className="pota-hunt-text">
            <T
              k="ota.hunt.banner"
              tags={{ b: <strong />, note: <span className="pota-hunt-sub" /> }}
              vals={{ reference: hunt.reference, call: hunt.call }}
            />
          </span>
          <button
            type="button"
            className="pota-hunt-clear"
            onClick={() => void handleClearHunt()}
            title={t('ota.hunt.clear')}
            aria-label={t('ota.hunt.clear')}
          >
            <X size={13} aria-hidden="true" />
          </button>
        </div>
      )}

      {/* My activation — while active, every QSO I log is stamped with MY park (my_ref). */}
      <div className={`pota-activation${activating ? ' active' : ''}`}>
        {activating ? (
          <>
            <span className="pota-act-text">
              <T
                k="ota.activation.active"
                tags={{ b: <strong />, note: <span className="pota-act-sub" /> }}
                vals={{
                  program: act?.program ?? '',
                  reference: act?.reference ?? '',
                  count: act?.qsoCount ?? 0,
                }}
              />
            </span>
            {/* Ends the ACTIVATION — the park stamp on what you log — never a transmission. */}
            <button type="button" className="pota-hunt-clear" onClick={() => void handleStopActivation()} title={t('ota.activation.stop.title')}>
              <X size={13} aria-hidden="true" /> {t('ota.activation.stop.label')}
            </button>
          </>
        ) : (
          <>
            <span className="pota-act-label">{t('ota.activation.label')}</span>
            <select className="settings-input pota-act-prog" value={actProg} onChange={(e) => setActProg(e.target.value)}>
              {ACT_PROGRAMS.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
            <input
              className="settings-input mono pota-act-ref"
              value={actRef}
              onChange={(e) => setActRef(e.target.value.toUpperCase())}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void handleStartActivation()
              }}
              placeholder={actProg === 'SOTA' ? REF_EXAMPLES.SOTA : REF_EXAMPLES.POTA}
              autoComplete="off"
              spellCheck={false}
            />
            <button type="button" className="pota-act-start" onClick={() => void handleStartActivation()} disabled={!actRef.trim()}>
              {t('ota.activation.start')}
            </button>
          </>
        )}
      </div>

      {/* Local park directory — download/import once, then search offline in the log form. */}
      <div className="pota-parklist">
        <span className="pota-parklist-status">
          {parkN > 0
            ? t('ota.parks.have', { formatted: parkN.toLocaleString() })
            : t('ota.parks.none')}
        </span>
        <button type="button" className="pota-act-start" onClick={() => void handleDownloadParks()} disabled={parkBusy}>
          {parkBusy ? '…' : parkN > 0 ? t('ota.parks.update') : t('ota.parks.download')}
        </button>
        <button type="button" className="pota-parklist-import" onClick={() => fileRef.current?.click()} disabled={parkBusy}>
          {t('ota.parks.import')}
        </button>
        <button
          type="button"
          className="pota-parklist-import"
          onClick={() => huntedFileRef.current?.click()}
          disabled={parkBusy}
          title={t('ota.hunted.title')}
        >
          {huntedN > 0
            ? t('ota.hunted.have', { formatted: huntedN.toLocaleString() })
            : t('ota.hunted.import')}
        </button>
        <input
          ref={fileRef}
          type="file"
          accept=".csv,text/csv"
          style={{ display: 'none' }}
          onChange={(e) => {
            const f = e.target.files?.[0]
            if (f) void handleImportFile(f)
            e.target.value = ''
          }}
        />
        <input
          ref={huntedFileRef}
          type="file"
          accept=".csv,text/csv"
          style={{ display: 'none' }}
          onChange={(e) => {
            const f = e.target.files?.[0]
            if (f) void handleImportHuntedFile(f)
            e.target.value = ''
          }}
        />
      </div>

      {/* Program toggle + band/mode filters + refresh */}
      <div className="pota-controls">
        <div className="pota-controls-row">
          {/* Program tabs */}
          <div className="filter-row" role="tablist" aria-label={t('ota.program.aria')}>
            {(['POTA', 'SOTA', 'Both'] as Program[]).map((p) => (
              <button
                key={p}
                type="button"
                role="tab"
                aria-selected={program === p}
                className={`filter-chip${program === p ? ' active' : ''}`}
                onClick={() => setProgram(p)}
              >
                {/* The tab VALUE is the programme (or 'Both'); only 'Both' is a word. */}
                {p === 'Both' ? t('ota.program.both') : p}
              </button>
            ))}
          </div>

          {/* Refresh + timestamp */}
          <div className="pota-refresh-row">
            <button
              type="button"
              className="filter-chip pota-refresh-btn"
              onClick={() => void loadSpots(program)}
              disabled={loading}
              title={t('ota.refresh.title')}
              aria-label={t('ota.refresh.title')}
            >
              <RefreshCw size={12} className={loading ? 'spin' : ''} aria-hidden="true" />
              {t('ota.refresh.label')}
            </button>
            {lastUpdatedLabel && (
              <span className="pota-last-updated">{lastUpdatedLabel}</span>
            )}
          </div>
        </div>

        {/* Band filter chips */}
        {availableBands.length > 0 && (
          <div className="pota-filter-row" role="group" aria-label={t('ota.filter.band.aria')}>
            <span className="pota-filter-label">{t('ota.filter.band.label')}</span>
            <button
              type="button"
              className={`filter-chip${bandFilter.length === 0 ? ' active' : ''}`}
              onClick={() => setBandFilter([])}
            >
              {t('ota.filter.all')}
            </button>
            {availableBands.map((b) => (
              <button
                key={b}
                type="button"
                className={`filter-chip${bandFilter.includes(b) ? ' active' : ''}`}
                onClick={() =>
                  setBandFilter((prev) =>
                    prev.includes(b) ? prev.filter((x) => x !== b) : [...prev, b],
                  )
                }
              >
                {b}
              </button>
            ))}
          </div>
        )}

        {/* Mode filter chips */}
        {availableModes.length > 0 && (
          <div className="pota-filter-row" role="group" aria-label={t('ota.filter.mode.aria')}>
            <span className="pota-filter-label">{t('ota.filter.mode.label')}</span>
            <button
              type="button"
              className={`filter-chip${modeFilter === 'All' ? ' active' : ''}`}
              onClick={() => setModeFilter('All')}
            >
              {t('ota.filter.all')}
            </button>
            {availableModes.map((m) => (
              <button
                key={m}
                type="button"
                className={`filter-chip${modeFilter === m ? ' active' : ''}`}
                onClick={() => setModeFilter(m)}
              >
                {m}
              </button>
            ))}
          </div>
        )}

        {/* Sort (sortable-everywhere). Cards, not a column grid — so a picker rather
            than clickable headers; the arrow flips direction. */}
        <div className="pota-filter-row" role="group" aria-label={t('ota.sort.aria')}>
          <span className="pota-filter-label">{t('ota.sort.label')}</span>
          <select
            className="pota-sort-pick"
            value={sortKey}
            onChange={(e) => setSortKey(e.target.value as OtaSort)}
            title={t('ota.sort.title')}
          >
            {/* The <option> VALUES are the persisted sort keys; only the labels are prose. */}
            <option value="value">{t('ota.sort.value')}</option>
            <option value="activator">{t('ota.sort.activator')}</option>
            <option value="reference">{t('ota.sort.reference')}</option>
            <option value="band">{t('ota.sort.band')}</option>
            <option value="mode">{t('ota.sort.mode')}</option>
          </select>
          <button
            type="button"
            className="filter-chip"
            onClick={() => setSortAsc((v) => !v)}
            title={sortAsc ? t('ota.sort.asc.title') : t('ota.sort.desc.title')}
            aria-label={sortAsc ? t('ota.sort.asc.aria') : t('ota.sort.desc.aria')}
          >
            {sortAsc ? '▲' : '▼'}
          </button>
        </div>
      </div>

      {/* Spot list */}
      {filtered.length === 0 ? (
        <p className="aw-empty pota-empty">
          {loading
            ? t('ota.loading')
            : bandFilter.length > 0 || modeFilter !== 'All'
              ? t('ota.empty.filtered')
              : t('ota.empty', {
                  program: program === 'Both' ? t('ota.programs.both') : program,
                })}
        </p>
      ) : (
        <ul className="pota-spot-list" role="list">
          {filtered.map((s) => {
            const band = bandFromKhz(s.freqKhz)
            const displayMode = spotDisplayMode(s.mode)
            const fullName = s.name || '—'
            const tooltipParts: string[] = [
              `${s.program} ${s.reference} — ${fullName}`,
              `${fmtFreq(s.freqKhz)} · ${displayMode} · ${band}`,
            ]
            if (s.spotter) tooltipParts.push(t('ota.spot.spottedBy', { spotter: s.spotter }))
            if (s.comment) tooltipParts.push(s.comment)
            if (s.bandOpen) tooltipParts.push(t('ota.spot.bandOpen.tooltip'))
            const tooltip = tooltipParts.join('\n')

            return (
              <li
                key={spotKey(s)}
                className={`pota-spot pota-spot-v2${s.bandOpen ? ' pota-spot-open' : ''}${s.newPark ? ' pota-spot-new' : ''}`}
                title={tooltip}
              >
                <div className="pota-spot-main">
                  <div className="pota-spot-line1">
                    <span className="pota-spot-call">{s.activator}</span>
                    <span className="pota-spot-ref" title={`${s.program} ${s.reference}`}>
                      {s.reference}
                    </span>
                    {/* Badges */}
                    <span className="pota-spot-badges">
                      {s.newPark && (
                        <span
                          className="pota-badge pota-badge-new"
                          title={t('ota.badge.newPark.title')}
                        >
                          {t('ota.badge.newPark')}
                        </span>
                      )}
                      {s.bandOpen && (
                        <span
                          className="pota-badge pota-badge-open"
                          title={t('ota.badge.bandOpen.title')}
                        >
                          {t('ota.badge.bandOpen')}
                        </span>
                      )}
                    </span>
                  </div>
                  <div className="pota-spot-line2">
                    <span className="pota-spot-name" title={fullName}>
                      {truncName(fullName)}
                    </span>
                    <span className="pota-spot-meta">
                      {fmtFreq(s.freqKhz)}
                      <span className="pota-spot-band">{band}</span>
                      <span className="pota-spot-mode">{displayMode}</span>
                      {program === 'Both' && (
                        <span className="pota-spot-prog">{s.program}</span>
                      )}
                    </span>
                  </div>
                </div>
                <button
                  type="button"
                  className="pota-hunt-btn"
                  onClick={() => void handleHunt(s)}
                  title={t('ota.hunt.button.title', {
                    call: s.activator,
                    reference: s.reference,
                    freq: fmtFreq(s.freqKhz),
                  })}
                  aria-label={t('ota.hunt.button.aria', { call: s.activator })}
                >
                  {t('ota.hunt.label')}
                </button>
              </li>
            )
          })}
        </ul>
      )}

      <p className="settings-hint pota-source-hint">
        {t('ota.source.hint', { source: SOURCES[program] })}
      </p>
    </section>
  )
}
