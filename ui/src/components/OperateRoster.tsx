// A WSJT-X / GridTracker-style Call Roster: one row per heard station as aligned,
// sortable columns (Call · Calling · Need · Country · State · Grid · Dist · Brg · SNR · Age) with
// roster filters (Needed-only, Hide-worked) and double-click-to-work. This is the
// "Roster" cockpit layout's primary surface — distinct from the waterfall-first
// "Classic" layout, not just a reshaped pane.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every cell in this
// grid is DATA and stays in the code: the callsign, who it is calling, the need TAGS, the
// entity, the state/province, the grid square, the distance, the bearing and the SNR. So do
// the tokens the roster is built out of (ROSTER_TOKENS below). What moved is the prose around
// them — the column headings, the filters, and everything a row says out loud.
import { useEffect, useMemo, useState } from 'react'
import { t } from '../i18n'
import { openQrzPage } from '../api'
import { withErrorToast } from '../toast'
import { useRovingList } from '../useRovingList'
import type { NeedAlert, NeedTag, Station } from '../types'
import {
  gridToLatLon,
  haversineKm,
  distanceLabel,
  magneticDeg,
  azimuthLabel,
  azimuthTitle,
  azimuthTo,
} from '../grid'
import { useEntityCentroids } from '../features/entityCentroids'
import { useUnits } from '../units'
import { getDeclination } from '../api'
import { NEED_CHIP } from '../features/needVisuals'
import { alertsForSurface, chaseRank, isActivityTag, strongestNeed } from '../features/needs'
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

type SortKey =
  | 'need'
  | 'call'
  | 'calling'
  | 'country'
  | 'state'
  | 'grid'
  | 'dist'
  | 'bearing'
  | 'snr'
  | 'age'

// The call roster shows only ACTIVELY-heard stations: a station drops off once
// it hasn't been decoded for this many T/R cycles, so the list reflects who's
// on the band right now rather than everyone heard since the last band change.
// 3 cycles ≈ 45 s on FT8 / 22 s on FT4 — tight enough to read as "live now" while
// still keeping anyone in an active QSO (a station is decoded every other slot, so
// its age stays ≤ ~2 as long as it's transmitting).
// (View-scoped — the backend roster is left intact so the Tempo/TempoFast presence
// and store-and-forward paths keep their longer retention.)
const ACTIVE_ROSTER_CYCLES = 3

/**
 * The roster's invariant vocabulary — never translated, never locale-formatted.
 *
 * `SNR` is a measurement's name and the column an operator reads a report off; `CQ` is a
 * Q-code and is what the station is literally sending; `B4` is the log shorthand for "worked
 * before". The QRZ link reuses `callbook.qrzPage.*` — one act, one wording, five surfaces.
 */
const ROSTER_TOKENS = { snr: 'SNR', cq: 'CQ', b4: 'B4' } as const

/** Row freshness → opacity: full-strength when just heard, dimming as a station
 * ages toward the drop-off, so live stations visually pop over lingering ones.
 * Pure + exported for test. Floor 0.5 keeps an aging row readable. */
export function freshness(age: number): number {
  if (age <= 0) return 1
  const t = Math.min(age / ACTIVE_ROSTER_CYCLES, 1)
  return 1 - 0.5 * t // age 0 → 1.0, at the drop-off edge → 0.5
}

const snrClass = (snr: number) => (snr >= -10 ? 'good' : snr >= -18 ? 'ok' : 'weak')
/** Text-column compare that parks the EMPTY cells last in the ascending sense. Explicit
 * rather than the `?? '~'` sentinel used by the older columns, because that sentinel does
 * not do it: ICU collation orders punctuation BEFORE letters, so `'~'.localeCompare('CT')`
 * is -1 and the blanks come out on top. */
const byText = (a: string | null | undefined, b: string | null | undefined) =>
  a ? (b ? a.localeCompare(b) : -1) : b ? 1 : 0
/** Shared empty set so the ignore checks stay allocation-free per render. */
const EMPTY_IGNORES: ReadonlySet<string> = new Set()
/** The Age cell. The unit letter rides inside the message with its number, so a translation
 *  can never separate the two (the Now-Bar's rule). */
function ageLabel(slots: number): string {
  if (slots <= 0) return t('operate.roster.age.now')
  if (slots < 60) return t('operate.roster.age.slots', { count: slots })
  return t('operate.roster.age.minutes', { minutes: Math.round(slots / 4) })
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
  // Entity centroids, so Brg stops reading "—" for a station heard only in traffic
  // that carried no grid — which was most of the column on a busy band.
  const centroids = useEntityCentroids()
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
        strongestNeed(alerts)?.tags.find((t) => !isActivityTag(t)) ??
        (raw && raw.length > 0 ? null : (needByCall.get(up) ?? null))
      // Union of ALL need forms for the row (deduped, insertion-ordered by the alerts), from
      // the GATED set so the chip cluster and the row colour can never disagree.
      //
      // ACTIVITY TAGS ARE EXCLUDED (`isActivityTag`): DXped/POTA/SOTA say what a station is
      // DOING, not what you stand to gain, and a DXPED chip sat in this cluster claiming to be
      // a need even for a station already worked on the band (operator, 2026-08-23). The
      // activity is still shown — `activityTypeByCall` reads the same tags for the badge, and
      // the Needed board's filter is unaffected.
      let needAll: NeedTag[] = []
      if (alerts && alerts.length > 0) {
        const seen = new Set<NeedTag>()
        for (const a of alerts) for (const t of a.tags) if (!isActivityTag(t)) seen.add(t)
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
        // Sorts on the SAME number the Brg cell prints, centroid fallback included.
        // Left as `me && ll ? … : 999` it would file every `~` row at the end of a
        // Brg sort while showing it a real heading — the column would read as broken.
        // 999 still parks a row with no bearing at all after everything that has one.
        brg: azimuthTo(myGrid, s.grid, s.country, centroids)?.deg ?? 999,
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
        case 'calling':
          // The CQ-ing stations (calling nobody) group at the end — which is the reason to
          // sort this column at all: who is free to answer.
          c = byText(a.s.calling, b.s.calling)
          break
        case 'country':
          c = (a.s.country ?? '~').localeCompare(b.s.country ?? '~')
          break
        case 'state':
          c = byText(a.s.state, b.s.state)
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
    myGrid,
    centroids,
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
      title={title ?? t('operate.roster.sort.title', { column: label })}
      onClick={() =>
        setSort((p) =>
          p.key === key
            ? { key, dir: p.dir === 'asc' ? 'desc' : 'asc' }
            : {
                key,
                dir:
                  key === 'call' ||
                  key === 'calling' ||
                  key === 'country' ||
                  key === 'state' ||
                  key === 'grid' ||
                  key === 'dist'
                    ? 'asc'
                    : 'desc',
              },
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
        <strong>{t('operate.roster.title')}</strong>
        <span className="or-count">{rows.length}</span>
        <label className="or-filter">
          <input
            type="checkbox"
            checked={neededOnly}
            onChange={(e) => setFilter({ neededOnly: e.target.checked })}
          /> {t('operate.roster.filter.neededOnly')}
        </label>
        {/* The tooltip is not decoration. Two operators read a surviving B4 chip as this
            filter being broken (field reports, 2026-08-22) — it is not: `hideWorked` keeps a
            worked station that still fills a need, which is the whole point for a band-slot
            chaser. The neighbouring Hide blocked has always explained itself; the filter with
            the genuinely non-obvious rule was the one saying nothing. */}
        <label className="or-filter" title={t('operate.roster.filter.hideWorked.title')}>
          <input
            type="checkbox"
            checked={hideWorked}
            onChange={(e) => setFilter({ hideWorked: e.target.checked })}
          /> {t('operate.roster.filter.hideWorked')}
        </label>
          <label className="or-filter" title={t('operate.roster.filter.hideBlocked.title')}>
            <input
              type="checkbox"
              checked={filters.hideBlocked === true}
              onChange={(e) => setFilter({ hideBlocked: e.target.checked })}
            /> {t('operate.roster.filter.hideBlocked')}
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
                ? t('operate.roster.spot.title', { call: selectedCall })
                : t('operate.roster.spot.title.none')
            }
          >
            {/* Two WHOLE labels — the button names the station when there is one to name,
                never a stem with the callsign glued after it. */}
            {selectedCall
              ? t('operate.roster.spot.label.call', { call: selectedCall })
              : t('operate.roster.spot.label')}
          </button>
        )}
      </div>
      <div
        className="or-grid"
        role="grid"
        aria-label={t('operate.roster.grid.aria')}
        aria-rowcount={rows.length + 1}
        onKeyDown={roving.containerProps.onKeyDown}
      >
        <div className="or-row or-header" role="row">
          {th('call', t('operate.roster.col.call'))}
          {th(
            'calling',
            t('operate.roster.col.calling'),
            t('operate.roster.col.calling.title'),
          )}
          {th('need', t('operate.roster.col.need'))}
          {th('country', t('operate.roster.col.country'))}
          {/* Header stays "State" — the column is two letters wide and GridTracker/WSJT-X call
              it that — with the full meaning in the tooltip. */}
          {th('state', t('operate.roster.col.state'), t('operate.roster.col.state.title'))}
          {th('grid', t('operate.roster.col.grid'))}
          {th('dist', t('operate.roster.col.dist'))}
          {th('bearing', t('operate.roster.col.bearing'))}
          {th('snr', ROSTER_TOKENS.snr)}
          {th('age', t('operate.roster.col.age'))}
        </div>
        {rows.length === 0 ? (
          <div className="or-empty">{t('operate.roster.empty')}</div>
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
                // Four optional clauses, each interpolated WHOLE with its own separator —
                // never a sentence glued from fragments. `{{need}}` is a need TAG, a token.
                aria-label={t('operate.roster.row.aria', {
                  call: s.call,
                  grid: s.grid ? t('operate.roster.row.aria.grid', { grid: s.grid }) : '',
                  need: need ? t('operate.roster.row.aria.need', { need }) : '',
                  worked: s.worked ? t('operate.roster.row.aria.worked') : '',
                  working: s.call === workingCall ? t('operate.roster.row.aria.working') : '',
                })}
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
                    ? t('operate.row.ignored.title')
                    : t('operate.roster.row.work.title', { call: s.call })
                }
              >
                <span className="or-call">
                  {s.call}
                  {/* WSJT-X's two B4 scopes at once: solid = worked on THIS band (its
                      CallBand highlight), hollow = worked anywhere (Call). One glance says
                      which — and 'worked on 40m FT8, now on 40m phone' reads solid unless
                      the operator opts into mode-scoped matching in Settings. */}
                  {s.worked && (
                    <span
                      className={`b4-chip${s.workedBand ? ' b4-band' : ''}`}
                      title={s.workedBand ? t('operate.b4.sameBand') : t('operate.b4.otherBand')}
                    >
                      {ROSTER_TOKENS.b4}
                    </span>
                  )}
                  {s.lotwUser && (
                    <span className="lotw-mark" title={t('operate.roster.lotw.title')}>
                      L
                    </span>
                  )}
                  <button
                    type="button"
                    className="qrz-link"
                    onClick={(e) => {
                      e.stopPropagation()
                      void withErrorToast(
                        () => openQrzPage(s.call),
                        t('callbook.qrzPage.failed', { call: s.call }),
                      )
                    }}
                    onDoubleClick={(e) => e.stopPropagation()}
                    title={t('callbook.qrzPage.title', { call: s.call })}
                  >
                    ↗
                  </button>
                </span>
                {/* Who they are working right now — a station mid-exchange will not answer a
                    call, and "CQ" (addressing nobody) is the row to double-click. */}
                <span
                  className={`or-calling${s.calling ? '' : ' cq'}${!s.calling && s.cqDir ? ' directed' : ''}`}
                  title={
                    s.calling
                      ? t('operate.roster.calling.title', { call: s.calling })
                      : s.cqDir
                        ? // A DIRECTED CQ is not a call you can answer from the wrong place.
                          // Operator request: the roster said only "CQ", so he clicked a CQ DX
                          // from CONUS and found out over in Band Activity.
                          t('operate.roster.calling.cqDir.title', { dir: s.cqDir })
                        : t('operate.roster.calling.cq.title')
                  }
                >
                  {/* `CQ DX`, not a translated phrase: the modifier is what went on the air,
                      and CQ is a Q-code. Both are invariant tokens. */}
                  {s.calling ?? (s.cqDir ? `${ROSTER_TOKENS.cq} ${s.cqDir}` : ROSTER_TOKENS.cq)}
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
                {/* State or province, from the callsign (FCC index / the Canadian regional
                    numeral) or the heard grid — the same hint the needed board resolves with,
                    so a WAS chaser reads one answer, not two. A PILL, like the roster's other
                    badges: the operator scans this column, and two dim letters do not read at
                    a glance. Nothing to say → the plain em dash, no chrome around an absence. */}
                <span className="or-state">
                  {s.state ? (
                    <span
                      className="or-subdiv"
                      title={t('operate.roster.state.title', { call: s.call, state: s.state })}
                    >
                      {s.state}
                    </span>
                  ) : (
                    '—'
                  )}
                </span>
                <span className="or-gridc">{s.grid ?? '—'}</span>
                <span className="or-dist">{distanceLabel(myGrid, s.grid, units) ?? '—'}</span>
                {/* Brg falls back to the entity centre (shown `~`) when the station
                    never sent a grid — same rule as Band Activity, so the two panes
                    of one cockpit cannot disagree about where a station is. */}
                {(() => {
                  const az = azimuthTo(myGrid, s.grid, s.country, centroids)
                  return (
                    <span
                      className="or-brg"
                      title={az ? azimuthTitle(az, s.country, magneticDeg(az.deg, declination)) : undefined}
                    >
                      {azimuthLabel(az) ?? '—'}
                    </span>
                  )
                })()}
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
