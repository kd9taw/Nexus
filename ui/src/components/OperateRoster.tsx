// A WSJT-X / GridTracker-style Call Roster: one row per heard station as aligned,
// sortable columns (Call · Need · Country · Grid · Dist · Brg · SNR · Age) with
// roster filters (Needed-only, Hide-worked) and double-click-to-work. This is the
// "Roster" cockpit layout's primary surface — distinct from the waterfall-first
// "Classic" layout, not just a reshaped pane.
import { useEffect, useMemo, useState } from 'react'
import { openQrzPage } from '../api'
import { withErrorToast } from '../toast'
import { useRovingList } from '../useRovingList'
import type { NeedAlert, NeedTag, Station } from '../types'
import { gridToLatLon, haversineKm, bearingDeg, distanceLabel, bearingLabel, magneticDeg } from '../grid'
import { useUnits } from '../units'
import { getDeclination } from '../api'
import { NEED_CHIP } from '../features/needVisuals'
import { alertsForSurface, chaseRank, strongestNeed } from '../features/needs'
import { isIgnored } from '../txMessages'
import { isCallHidden, useHideCalls } from '../features/hideCalls'
import { loadRosterFilters, saveRosterFilters, type RosterFilters } from '../operateFilters'
import { hasOverridingNeed, isHiddenByCountry, useCountryExclude } from '../features/countryExclude'
import { CountryHiddenChip } from './CountryExclude'
import { RarityChip } from './RarityChip'

interface Props {
  stations: Station[]
  myGrid: string
  currentSlot: number
  needByCall: Map<string, NeedTag>
  /** FULL per-call alerts — a station needed on several dimensions (grid AND
   * band…) shows EVERY need chip, not just the top tier (operator report). */
  needAlertsByCall?: Map<string, NeedAlert[]>
  /** The band and operating mode THIS roster is showing, so a cross-band/cross-mode
   * alert can't paint a chip the operator cannot close here (see `tagsForSurface`).
   * The maps above are keyed by callsign alone and carry every band and mode, which is
   * how a CW "new mode" need appeared on a 30m FT8 roster. Omit both to keep the
   * ungated behaviour (surfaces that intentionally span bands, e.g. the map). */
  band?: string
  feedMode?: string
  selectedCall: string | null
  /** The station the sequencer is actually WORKING right now (`snap.qso.dxcall`), which is not
   * the same thing as the row the operator last clicked. Two operators asked for this (#16): in a
   * busy roster there was nothing at all to say which call the QSO in progress belongs to.
   *
   * Kept separate from `selectedCall` rather than folded into it — selection drives the Spot
   * button and the roving-focus model, and the station being worked is not a selection. A row can
   * be both, and then the working treatment wins. */
  workingCall?: string | null
  onSelect: (call: string) => void
  /** Work the station. Same positional signature as the cockpit's shared handler: the roster
   * passes the station's LAST-HEARD offset in the `freq` slot so RX/TX move onto them exactly
   * like a Band Activity double-click (the engine then applies the Hold-Tx rule). */
  onCall: (call: string, grid?: string, message?: string, snr?: number, freq?: number) => void
  /** Session-only ignore set (Alt-double-click) — ignored calls render dimmed. */
  ignoredCalls?: ReadonlySet<string>
  /** Toggle a call in/out of the session ignore set (Alt-double-click). */
  onToggleIgnore?: (call: string) => void
  /** Post the selected station to the DX cluster (spot it at the current dial).
   *  Absent = no cluster connected → the control hides. */
  onSpot?: (call: string) => void
}

type SortKey = 'need' | 'call' | 'country' | 'grid' | 'dist' | 'bearing' | 'snr' | 'age'

// The call roster shows only ACTIVELY-heard stations: a station drops off once
// it hasn't been decoded for this many T/R cycles, so the list reflects who's
// on the band right now rather than everyone heard since the last band change.
// 3 cycles ≈ 45 s on FT8 / 22 s on FT4 — tight enough to read as "live now" while
// still keeping anyone in an active QSO (a station is decoded every other slot, so
// its age stays ≤ ~2 as long as it's transmitting).
// (View-scoped — the backend roster is left intact so the Tempo/TempoFast presence
// and store-and-forward paths keep their longer retention.)
const ACTIVE_ROSTER_CYCLES = 3

/** Row freshness → opacity: full-strength when just heard, dimming as a station
 * ages toward the drop-off, so live stations visually pop over lingering ones.
 * Pure + exported for test. Floor 0.5 keeps an aging row readable. */
export function freshness(age: number): number {
  if (age <= 0) return 1
  const t = Math.min(age / ACTIVE_ROSTER_CYCLES, 1)
  return 1 - 0.5 * t // age 0 → 1.0, at the drop-off edge → 0.5
}

const snrClass = (snr: number) => (snr >= -10 ? 'good' : snr >= -18 ? 'ok' : 'weak')
/** Shared empty set so the ignore checks stay allocation-free per render. */
const EMPTY_IGNORES: ReadonlySet<string> = new Set()
function ageLabel(slots: number): string {
  if (slots <= 0) return 'now'
  if (slots < 60) return `${slots} sl`
  return `${Math.round(slots / 4)}m`
}

export function OperateRoster({
  stations,
  myGrid,
  currentSlot,
  needByCall,
  needAlertsByCall,
  selectedCall,
  workingCall = null,
  onSelect,
  onCall,
  ignoredCalls,
  onToggleIgnore,
  onSpot,
  band,
  feedMode,
}: Props) {
  // QTH magnetic declination (WMM) — the Brg column's tooltip shows the compass
  // heading a rotator zeroed on magnetic north needs.
  const units = useUnits()
  const [declination, setDeclination] = useState<number | null>(null)
  useEffect(() => {
    getDeclination()
      .then(setDeclination)
      .catch(() => {})
  }, [])
  const [sort, setSort] = useState<{ key: SortKey; dir: 'asc' | 'desc' }>({ key: 'need', dir: 'desc' })
  // Persisted per-surface (operateFilters.ts): the operator ticks Needed-only once and it is
  // still ticked after a restart. Defaults are both off, so nothing changes for anyone who
  // has never touched them.
  const [filters, setFilters] = useState(loadRosterFilters)
  const { neededOnly, hideWorked } = filters
  // Merges against the LIVE previous value (NeededPanel's toggleBand pattern) so ticking one
  // checkbox can never write away the other's state.
  const setFilter = (patch: Partial<RosterFilters>) => {
    setFilters((prev) => {
      const next = { ...prev, ...patch }
      saveRosterFilters(next)
      return next
    })
  }
  // The operator's country exclusion. Shared with Band Activity through one app-global key,
  // so the two panes can never show different bands.
  const countries = useCountryExclude()
  const hideCalls = useHideCalls()
  const me = useMemo(() => gridToLatLon(myGrid), [myGrid])

  const rows = useMemo(() => {
    const built = stations.map((s) => {
      const up = s.call.toUpperCase()
      const raw = needAlertsByCall?.get(up)
      // SURFACE GATE FIRST, rank what survives. The alerts map is keyed by callsign and
      // spans every band and mode, so ungated it painted a MODE chip from a CW need onto a
      // 30m FT8 roster (operator report 2026-07-29) — and let that unclosable need pick the
      // row's colour and its place in "sort by need". `alertsForSurface` drops what this
      // band + mode class cannot close; everything below then ranks only what is left.
      const alerts = band != null && feedMode != null ? alertsForSurface(raw, band, feedMode) : raw
      // Lead tag from the call's STRONGEST surviving alert, so the row's colour, its
      // aria-label and its chase rank all name the same need. needByCall resolved a
      // multi-band station to whichever alert came LAST, which is its WEAKEST — a new entity
      // on 20 m with a mere confirm on 40 m ranked as the confirm, three quarters of the way
      // down the list (operator report). It stays as the fallback for hosts that pass only
      // the top-tag map; when alerts WERE supplied and the gate cleared every one, nothing
      // is needed here, so the row stays null rather than reaching for an ungated tag.
      const need: NeedTag | null =
        strongestNeed(alerts)?.tags[0] ??
        (raw && raw.length > 0 ? null : (needByCall.get(up) ?? null))
      // Union of ALL need forms for the row (deduped, insertion-ordered by the alerts), from
      // the GATED set so the chip cluster and the row colour can never disagree.
      let needAll: NeedTag[] = []
      if (alerts && alerts.length > 0) {
        const seen = new Set<NeedTag>()
        for (const a of alerts) for (const t of a.tags) seen.add(t)
        needAll = [...seen]
      }
      if (need && needAll.length === 0) needAll = [need]
      const ll = s.grid ? gridToLatLon(s.grid) : null
      return {
        s,
        need,
        needAll,
        needRank: chaseRank(alerts, need),
        distKm: me && ll ? haversineKm(me, ll) : Infinity,
        brg: me && ll ? bearingDeg(me, ll) : 999,
        age: currentSlot - s.lastHeardSlot,
      }
    })
    // Keep only stations heard within the recency window — the roster stays a
    // live picture of the band, not a running tally.
    let f = built.filter((x) => x.age <= ACTIVE_ROSTER_CYCLES)
    // The country exclusion, on the SAME predicate Band Activity uses so the two panes
    // cannot drift on what "protected" means. `needAll` is the surface-GATED need set, so a
    // cross-band claim this roster could not close never rescues a row either.
    f = f.filter(
      (x) =>
        !isHiddenByCountry(
          {
            entity: x.s.country,
            call: x.s.call,
            qsoPartner: selectedCall,
            needed: hasOverridingNeed(x.needAll),
          },
          countries.hidden,
        ),
    )
    if (neededOnly) f = f.filter((x) => x.need != null)
    if (hideWorked) f = f.filter((x) => !x.s.worked || x.need != null)
    // Hide blocked (opt-in; default they render dimmed). The station being WORKED or
    // selected always stays — hiding your live QSO partner mid-exchange is the same
    // self-own the country exclusion guards against.
    if (filters.hideBlocked)
      f = f.filter(
        (x) =>
          !isIgnored(ignoredCalls ?? EMPTY_IGNORES, x.s.call) ||
          x.s.call === selectedCall ||
          x.s.call === workingCall,
      )
    // Wildcard call-hide (VP8* etc.) — display-only; the worked/selected station stays.
    if (hideCalls.entries.length > 0)
      f = f.filter(
        (x) =>
          !isCallHidden(x.s.call, hideCalls.entries) ||
          x.s.call === selectedCall ||
          x.s.call === workingCall,
      )
    const dir = sort.dir === 'asc' ? 1 : -1
    f.sort((a, b) => {
      let c = 0
      switch (sort.key) {
        case 'need':
          c = a.needRank - b.needRank
          break
        case 'call':
          c = a.s.call.localeCompare(b.s.call)
          break
        case 'country':
          c = (a.s.country ?? '~').localeCompare(b.s.country ?? '~')
          break
        case 'grid':
          // '~' sorts the grid-less to the end in both directions' ascending sense.
          c = (a.s.grid ?? '~').localeCompare(b.s.grid ?? '~')
          break
        case 'dist':
          c = a.distKm - b.distKm
          break
        case 'bearing':
          c = a.brg - b.brg
          break
        case 'snr':
          c = a.s.snr - b.s.snr
          break
        case 'age':
          c = a.age - b.age
          break
      }
      // The DIRECTION applies to the chosen column only. Multiplying the tiebreak by it too
      // inverted it on every descending sort — including the default need-desc view, where
      // stations of equal need came out weakest-signal-first. Of two equally-needed stations
      // the louder one is the better bet, whichever way the column is pointing.
      if (c !== 0) return c * dir
      return b.s.snr - a.s.snr // tiebreak: stronger signal first
    })
    return f
  }, [
    stations,
    needByCall,
    needAlertsByCall,
    band,
    feedMode,
    me,
    currentSlot,
    sort,
    neededOnly,
    hideWorked,
    filters.hideBlocked,
    hideCalls.entries,
    ignoredCalls,
    workingCall,
    countries.hidden,
    selectedCall,
  ])

  // Keyboard: arrow through rows, Enter selects, Shift+Enter works, Alt+Enter ignores.
  const roving = useRovingList(rows.length, (i, mods) => {
    const s = rows[i]?.s
    if (!s) return
    if (mods.alt) onToggleIgnore?.(s.call)
    else if (mods.shift) onCall(s.call, s.grid ?? undefined, undefined, undefined, s.freqHz ?? undefined)
    else onSelect(s.call)
  })

  const th = (key: SortKey, label: string, title?: string) => (
    <button
      type="button"
      className={`or-th${sort.key === key ? ' active' : ''}`}
      title={title ?? `Sort by ${label}`}
      onClick={() =>
        setSort((p) =>
          p.key === key
            ? { key, dir: p.dir === 'asc' ? 'desc' : 'asc' }
            : { key, dir: key === 'call' || key === 'country' || key === 'grid' || key === 'dist' ? 'asc' : 'desc' },
        )
      }
    >
      {label}
      {sort.key === key ? (sort.dir === 'asc' ? ' ▲' : ' ▼') : ''}
    </button>
  )

  return (
    <div className="operate-roster">
      <div className="or-filters">
        <strong>Call Roster</strong>
        <span className="or-count">{rows.length}</span>
        <label className="or-filter">
          <input
            type="checkbox"
            checked={neededOnly}
            onChange={(e) => setFilter({ neededOnly: e.target.checked })}
          /> Needed only
        </label>
        <label className="or-filter">
          <input
            type="checkbox"
            checked={hideWorked}
            onChange={(e) => setFilter({ hideWorked: e.target.checked })}
          /> Hide worked
        </label>
          <label className="or-filter" title="Drop blocked callsigns from the roster entirely (unchecked: they render dimmed). Alt-double-click a row to block or unblock; the auto-responder never answers blocked calls either way.">
            <input
              type="checkbox"
              checked={filters.hideBlocked === true}
              onChange={(e) => setFilter({ hideBlocked: e.target.checked })}
            /> Hide blocked
          </label>
        {/* Beside the row count, so a thinned roster always says why. The picker itself
            lives in the Band Activity chip bar — one control for one shared list. */}
        <CountryHiddenChip
          count={countries.keys.size}
          onClear={countries.clear}
          testId="or-hidden"
        />
        {onSpot && (
          <button
            type="button"
            className="or-filter or-spot"
            disabled={!selectedCall}
            onClick={() => selectedCall && onSpot(selectedCall)}
            title={
              selectedCall
                ? `Spot ${selectedCall} to the DX cluster at the current dial`
                : 'Select a station to spot it to the DX cluster'
            }
          >
            Spot{selectedCall ? ` ${selectedCall}` : ''}
          </button>
        )}
      </div>
      <div
        className="or-grid"
        role="grid"
        aria-label="Call roster — arrow to move, Enter to select, Shift+Enter to work"
        aria-rowcount={rows.length + 1}
        onKeyDown={roving.containerProps.onKeyDown}
      >
        <div className="or-row or-header" role="row">
          {th('call', 'Call')}
          {th('need', 'Need')}
          {th('country', 'Country')}
          {th('grid', 'Grid')}
          {th('dist', 'Dist')}
          {th('bearing', 'Brg')}
          {th('snr', 'SNR')}
          {th('age', 'Age')}
        </div>
        {rows.length === 0 ? (
          <div className="or-empty">No stations heard yet — decoded stations appear here as they arrive.</div>
        ) : (
          rows.map(({ s, need, needAll, age }, i) => {
            const chip = need ? NEED_CHIP[need] : null
            const ignoredRow = isIgnored(ignoredCalls ?? EMPTY_IGNORES, s.call)
            const rp = roving.rowProps(i)
            return (
              <div
                key={s.call}
                role="row"
                aria-selected={s.call === selectedCall}
                aria-label={`${s.call}${s.grid ? `, grid ${s.grid}` : ''}${need ? `, needed ${need}` : ''}${s.worked ? ', worked' : ''}${s.call === workingCall ? ', working now' : ''}`}
                tabIndex={rp.tabIndex}
                ref={rp.ref as (el: HTMLDivElement | null) => void}
                onFocus={rp.onFocus}
                className={`or-row${s.call === selectedCall ? ' selected' : ''}${
                  s.call === workingCall ? ' working' : ''
                }${s.worked ? ' worked' : ''}${
                  chip ? ` need-${chip.cls}` : ''
                }${ignoredRow ? ' ignored' : ''}`}
                style={{
                  opacity: s.call === selectedCall || s.call === workingCall ? 1 : freshness(age),
                }}
                onClick={() => {
                  roving.setActive(i)
                  onSelect(s.call)
                }}
                onDoubleClick={(e) =>
                  // Alt-double-click toggles the session ignore (stock WSJT-X).
                  e.altKey && onToggleIgnore
                    ? onToggleIgnore(s.call)
                    : onCall(s.call, s.grid ?? undefined, undefined, undefined, s.freqHz ?? undefined)
                }
                title={
                  ignoredRow
                    ? 'Ignored this session (Alt-double-click to restore)'
                    : `Double-click to work ${s.call}`
                }
              >
                <span className="or-call">
                  {s.call}
                  {s.worked && (
                    <span className="b4-chip" title="Worked before">
                      B4
                    </span>
                  )}
                  {s.lotwUser && (
                    <span className="lotw-mark" title="Uploads to LoTW — this contact should confirm">
                      L
                    </span>
                  )}
                  <button
                    type="button"
                    className="qrz-link"
                    onClick={(e) => {
                      e.stopPropagation()
                      void withErrorToast(() => openQrzPage(s.call), `Could not open ${s.call} on QRZ`)
                    }}
                    onDoubleClick={(e) => e.stopPropagation()}
                    title={`${s.call} on QRZ.com (opens your browser)`}
                  >
                    ↗
                  </button>
                </span>
                <span
                  className="or-need"
                  /* The cell clips chips (deliberate — stops the Zone chip overlapping the Call);
                     this title surfaces every need on hover so a clipped chip isn't silently lost. */
                  title={needAll.map((t) => NEED_CHIP[t]?.label).filter(Boolean).join(' · ') || undefined}
                >
                  {needAll.map((t) => {
                    const c = NEED_CHIP[t]
                    return (
                      c && (
                        <span key={t} className={`need-chip need-${c.cls}`} title={c.label}>
                          {c.short}
                        </span>
                      )
                    )
                  })}
                  {/* Rarity lives with the needs — both answer "why work this station?" — and the
                      widened Need column has room for the loud 💎 ULTRA pill the grid cell clipped. */}
                  <RarityChip rarity={s.gridRarity} />
                </span>
                <span className="or-country">{s.country ?? '—'}</span>
                <span className="or-gridc">{s.grid ?? '—'}</span>
                <span className="or-dist">{distanceLabel(myGrid, s.grid, units) ?? '—'}</span>
                <span
                  className="or-brg"
                  title={(() => {
                    const me = gridToLatLon(myGrid)
                    const them = s.grid ? gridToLatLon(s.grid) : null
                    if (!me || !them) return undefined
                    const t = bearingDeg(me, them)
                    const mg = magneticDeg(t, declination)
                    return mg != null ? `${t}° true · ${mg}° magnetic (WMM)` : `${t}° true`
                  })()}
                >
                  {bearingLabel(myGrid, s.grid) ?? '—'}
                </span>
                <span className={`or-snr snr-${snrClass(s.snr)}`}>
                  {s.snr > 0 ? '+' : ''}
                  {s.snr}
                </span>
                <span className="or-age">{ageLabel(age)}</span>
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}
