// Path over the day — the selected station's strip (the `pathDay` box) and the ten DX regions'
// strips (the `pathRegions` box), drawn from the shack's day grid (features/pathDay).
//
// One grid per box: the hour axis on top (its 24 buttons scrub the map to that hour), a row per
// band coloured by the MODELLED reliability of the route shown, each cell marked with the route the
// model favours (▸ short, ◂ long; nothing when it favours neither), a violet underline where that
// route crosses the auroral zone or the polar cap at the hour's Kp, and the MUF ceiling as a line
// over the highest band under it. Under the bands: the grey-line row (◐ when either end is on the
// terminator) and the Kp ribbon, measured solid, forecast hollow, SWPC's latest reading carried
// forward dashed, unknown an empty dotted slot — the Kp outlook box's convention, so a forecast
// never reads as a reading. The legend says all of it, and that the
// figures are a monthly median; wherever a long-path figure can show, so does the long-path label.
//
// Display only: a click scrubs the map's terminator and ribbon, and turns nothing.
import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import type { NeedAlert, PathDay, PathDayCell, PathDayHour, PathPrediction, ZoneTally } from '../../types'
import { t } from '../../i18n'
import { fmtZ, heatColor } from '../../propViz'
import { getLogStats } from '../../api'
import { PATH_REGIONS, refreshFeed, useFeed } from '../../features/connectFeeds'
import {
  bestBand,
  cellView,
  ceilingRow,
  daySummary,
  kpCell,
  orderRegions,
  ROUTE_GLYPH,
  type DayMode,
} from '../../features/pathDay'
import { ROUTE_LP, ROUTE_SP } from '../../features/pathRibbon'
import { POLAR_CAP_DEG, utcHourLabel } from '../../features/pathWhy'

/** The hour a strip is scrubbed to: its UTC hour and the middle of that hour (ms) for the map. */
export interface DayScrub {
  hour: number
  ms: number
}

const HOURS = Array.from({ length: 24 }, (_, h) => h)
const MAJOR = [0, 6, 12, 18]
/** The Kp ribbon's and grey row's labels: tokens, the same in every language. */
const KP_LABEL = 'Kp'
const GREY_GLYPH = '◐'

// Literal keys in each switch, so the catalog's checks read every one of them.
function kpSourceText(s: PathDayHour['kpSource']): string {
  switch (s) {
    case 'measured':
      return t('pathWhy.kpSource.measured')
    case 'forecast':
      return t('pathWhy.kpSource.forecast')
    case 'current':
      return t('pathWhy.kpSource.current')
    case 'unknown':
      return t('pathWhy.kpSource.unknown')
  }
}

function modeTitle(m: DayMode): string {
  return m === 'best'
    ? t('pathDay.mode.title.best')
    : m === 'short'
      ? t('pathDay.mode.title.short')
      : t('pathDay.mode.title.long')
}

/** A region's name, or null for an id this build does not know. */
function regionName(id: string): string | null {
  switch (id) {
    case 'eu':
      return t('pathDay.region.eu')
    case 'ja':
      return t('pathDay.region.ja')
    case 'vk':
      return t('pathDay.region.vk')
    case 'pacific':
      return t('pathDay.region.pacific')
    case 'sa':
      return t('pathDay.region.sa')
    case 'af':
      return t('pathDay.region.af')
    case 'carib':
      return t('pathDay.region.carib')
    case 'me':
      return t('pathDay.region.me')
    case 'sas':
      return t('pathDay.region.sas')
    case 'polar':
      return t('pathDay.region.polar')
    default:
      return null
  }
}

/** The current UTC hour, re-read every minute. */
function useUtcHour(): number {
  const [h, setUtcHour] = useState(() => new Date().getUTCHours())
  useEffect(() => {
    const id = setInterval(() => setUtcHour(new Date().getUTCHours()), 60_000)
    return () => clearInterval(id)
  }, [])
  return h
}

function kpText(kp: number): string {
  return Number.isInteger(kp) ? String(kp) : kp.toFixed(1)
}

/** The hover words for one cell: both routes' figures, the model's call, polar, above the MUF. */
/** What a crossing is, worded as the why line and the route chip word it: the polar cap proper
 * when the route reaches 70° geomagnetic, else the auroral zone. A route outside the zone now that
 * this hour's FORECAST Kp would bring in reads "Auroral zone if Kp N (forecast from HHZ)". */
function crossingWords(day: PathDay, route: 'short' | 'long' | null, hour: PathDayHour | undefined): string {
  const max = route === 'long' ? day.maxGmlatLong : day.maxGmlatShort
  if (max != null && max >= POLAR_CAP_DEG) return t('pathWhy.polar')
  const now = day.hours[day.nowHour]
  const inNow = route === 'long' ? now?.polarLong === true : now?.polarShort === true
  if (!inNow && hour && hour.kpSource === 'forecast' && hour.kp != null)
    return t('pathRoute.chip.stormIf', { kp: kpText(hour.kp), time: utcHourLabel(hour.unix) })
  return t('pathWhy.auroralZone')
}

function cellTitle(day: PathDay, band: string, h: number, c: PathDayCell, hour: PathDayHour | undefined, mode: DayMode, aboveMuf: boolean): string {
  const v = cellView(c, hour, mode)
  const parts = [t('pathDay.cell.short', { pct: c.shortPct })]
  if (c.longPct != null) parts.push(t('pathDay.cell.long', { pct: c.longPct }))
  if (c.verdict === 'short' || c.verdict === 'long') {
    const conf = c.confidence === 'likely' ? t('pathRoute.conf.likely') : t('pathRoute.conf.possible')
    parts.push(
      c.verdict === 'short' ? t('pathDay.cell.favours.short', { conf }) : t('pathDay.cell.favours.long', { conf }),
    )
  } else if (c.verdict === 'either') parts.push(t('pathDay.cell.either'))
  else if (c.verdict === 'unknown') parts.push(t('pathDay.cell.closed'))
  if (v.polar) parts.push(crossingWords(day, v.polarRoute, hour))
  if (aboveMuf) parts.push(t('pathDay.cell.aboveMuf'))
  return t('pathDay.cell.title', { band, time: fmtZ(h), text: parts.join(t('pathWhy.sep')) })
}

function greyTitle(h: PathDayHour, hour: number): string {
  const text =
    h.greyMe && h.greyDx ? t('pathWhy.grey.both') : h.greyMe ? t('pathWhy.grey.me') : t('pathWhy.grey.dx')
  return t('pathDay.grey.title', { time: fmtZ(hour), text })
}

/** A row of the grid: its label and 24 cells. */
interface Row {
  key: string
  label: ReactNode
  day: PathDay
  band: PathDay['bands'][number]
  /** This band's index in its day (for the MUF ceiling). */
  index: number
}

function Grid({
  rows,
  hours,
  nowHour,
  mode,
  scrub,
  onScrub,
  className,
  greyEnds = 'both',
}: {
  rows: Row[]
  hours: PathDayHour[]
  nowHour: number
  mode: DayMode
  scrub: number | null
  onScrub?: (s: DayScrub | null) => void
  className?: string
  /** `me`: the grey row marks the operator's end only (the region box: each region's far end
   * differs, the operator's does not). */
  greyEnds?: 'both' | 'me'
}) {
  const ceilings = useMemo(() => new Map(rows.map((r) => [r.day, ceilingRow(r.day, mode)])), [rows, mode])
  const pick = (h: number) => {
    if (!onScrub) return
    const at = hours[h]
    onScrub(h === scrub || !at ? null : { hour: h, ms: (at.unix + 1800) * 1000 })
  }
  const col = (h: number) => `${h === nowHour ? ' now' : ''}${h === scrub ? ' scrub' : ''}`
  return (
    <div className={`pd-grid${className ? ` ${className}` : ''}`} role="group" aria-label={t('pathDay.aria')}>
      <span className="pd-corner" />
      {HOURS.map((h) => (
        <button
          key={h}
          type="button"
          className={`pd-hour${MAJOR.includes(h) ? ' major' : ''}${col(h)}`}
          onClick={() => pick(h)}
          disabled={!onScrub}
          title={h === scrub ? t('pathDay.scrub.back') : t('pathDay.hour.title', { time: fmtZ(h) })}
          aria-label={h === scrub ? t('pathDay.scrub.back') : t('pathDay.hour.title', { time: fmtZ(h) })}
          aria-pressed={h === scrub}
        >
          {MAJOR.includes(h) ? h : ''}
        </button>
      ))}
      {rows.map((r) => (
        <div key={r.key} className="pd-row">
          <span className="pd-label">{r.label}</span>
          {r.band.cells.map((c, h) => {
            const v = cellView(c, r.day.hours[h], mode)
            const ceil = ceilings.get(r.day)?.[h]
            const aboveMuf = ceil == null || r.index < ceil
            return (
              <span
                key={h}
                className={`pd-cell${col(h)}${v.polar ? ' polar' : ''}${ceil === r.index ? ' ceil' : ''}${v.pct == null ? ' none' : ''}`}
                style={v.pct == null ? undefined : ({ background: heatColor(v.pct / 100) } as CSSProperties)}
                title={cellTitle(r.day, r.band.band, h, c, r.day.hours[h], mode, aboveMuf)}
                onClick={() => pick(h)}
              >
                {v.glyph}
              </span>
            )
          })}
        </div>
      ))}
      <div className="pd-row pd-grey">
        <span className="pd-label">{GREY_GLYPH}</span>
        {HOURS.map((h) => {
          const x = hours[h]
          const on = x && (greyEnds === 'me' ? x.greyMe : x.greyMe || x.greyDx)
          return (
            <span
              key={h}
              className={`pd-greycell${col(h)}`}
              title={on ? (greyEnds === 'me' ? t('pathDay.grey.title', { time: fmtZ(h), text: t('pathWhy.grey.me') }) : greyTitle(x, h)) : undefined}
            >
              {on ? GREY_GLYPH : ''}
            </span>
          )
        })}
      </div>
      <div className="pd-row pd-kp">
        <span className="pd-label">{KP_LABEL}</span>
        {HOURS.map((h) => {
          const x = hours[h]
          if (!x) return <span key={h} />
          const k = kpCell(x)
          // An unknown Kp has no number to print: the hover says only that it is unknown.
          const source = kpSourceText(x.kpSource)
          return (
            <span
              key={h}
              className={`pd-kpcell ${k.fill}${k.tone ? ` ${k.tone}` : ''}${col(h)}`}
              title={
                x.kp == null || k.tone == null
                  ? t('pathDay.grey.title', { time: fmtZ(h), text: source })
                  : t('pathDay.kp.title', { time: fmtZ(h), kp: kpText(x.kp), source })
              }
            />
          )
        })}
      </div>
    </div>
  )
}

function ModeToggle({ mode, onMode, hasLong }: { mode: DayMode; onMode: (m: DayMode) => void; hasLong: boolean }) {
  const label: Record<DayMode, string> = { best: t('pathDay.mode.best'), short: ROUTE_SP, long: ROUTE_LP }
  return (
    <span className="pd-modes" role="group" aria-label={t('pathDay.mode.aria')}>
      {(['best', 'short', 'long'] as const).map((m) => (
        <button
          key={m}
          type="button"
          className={`pd-mode${m === mode ? ' on' : ''}`}
          aria-pressed={m === mode}
          disabled={m !== 'short' && !hasLong}
          title={modeTitle(m)}
          onClick={() => onMode(m)}
        >
          {label[m]}
        </button>
      ))}
    </span>
  )
}

function Legend({ hasLong, showLong }: { hasLong: boolean; showLong: boolean }) {
  return (
    <div className="pd-legend">
      {hasLong && (
        <>
          <span>
            {ROUTE_GLYPH.short} {t('pathDay.legend.short')}
          </span>
          <span>
            {ROUTE_GLYPH.long} {t('pathDay.legend.long')}
          </span>
        </>
      )}
      <span>
        <i className="pd-swatch polar" /> {t('pathDay.legend.polar')}
      </span>
      <span>
        <i className="pd-swatch ceil" /> {t('pathDay.legend.muf')}
      </span>
      <span>{t('pathDay.legend.kp')}</span>
      <span className="pd-modelled">{t('pathDay.legend.modelled')}</span>
      {showLong && <span className="route-lp-badge">{t('pathRoute.lpBadge')}</span>}
    </div>
  )
}

/** "Best: 20m 12Z–16Z by the long path; 40m 20Z–00Z by the short path." */
export function daySummaryLine(day: PathDay, mode: DayMode): string {
  const runs = daySummary(day, mode)
  if (runs.length === 0) return t('pathDay.best.none')
  const words = runs.map((r) => {
    const v = { band: r.band, from: fmtZ(r.from), to: fmtZ(r.to) }
    if (day.mufLong == null) return t('pathDay.best.plain', { band: v.band, from: v.from, to: v.to })
    if (r.route === 'short') return t('pathDay.best.short', { band: v.band, from: v.from, to: v.to })
    if (r.route === 'long') return t('pathDay.best.long', { band: v.band, from: v.from, to: v.to })
    return t('pathDay.best.either', { band: v.band, from: v.from, to: v.to })
  })
  return t('pathDay.best.lead', { text: words.join(t('pathDay.best.sep')) })
}

/** The selected station's path over the day. Nothing without the shack's day (no grid, an older
 * shack): the box then shows its one line. */
export function PathDayPane({
  pred,
  scrub,
  onScrub,
}: {
  pred: PathPrediction | null
  scrub: number | null
  onScrub?: (s: DayScrub | null) => void
}) {
  const [mode, setMode] = useState<DayMode>('best')
  const day = pred?.day
  const hasLong = day?.mufLong != null
  const shown: DayMode = hasLong ? mode : 'short'
  const rows = useMemo<Row[]>(
    () => (day ? day.bands.map((band, index) => ({ key: band.band, label: band.band, day, band, index })) : []),
    [day],
  )
  if (!day || rows.length === 0) return <div className="pane-basic">{t('connect.pane.pathDay.basic')}</div>
  return (
    <div className="path-day">
      <div className="pd-head">
        <span className="pd-summary">{daySummaryLine(day, shown)}</span>
        <ModeToggle mode={shown} onMode={setMode} hasLong={hasLong} />
      </div>
      <Grid rows={rows} hours={day.hours} nowHour={day.nowHour} mode={shown} scrub={scrub} onScrub={onScrub} />
      <Legend hasLong={hasLong} showLong={hasLong && shown !== 'short'} />
    </div>
  )
}

/** The ten DX regions' strips: each collapsed to its best band, ▸ to show every band. Ordered by
 * the operator's needed stations, then logged contacts, per region (fixed order on an empty log). */
export function PathRegionsPane({
  needs,
  scrub,
  onScrub,
  myGrid,
  engine,
}: {
  needs: readonly NeedAlert[]
  scrub: number | null
  onScrub?: (s: DayScrub | null) => void
  /** The operator's grid and the prediction engine: a change asks the shack for the sweep again,
   * rather than showing the old one until the next 15-minute poll. */
  myGrid?: string
  engine?: string
}) {
  const data = useFeed(PATH_REGIONS).value ?? null
  // The sweep is polled every 15 minutes; its "now" column follows the clock instead, checked
  // every minute, so it moves on at the hour as the station strip does.
  const nowHour = useUtcHour()
  const settings = useRef({ myGrid, engine })
  useEffect(() => {
    const was = settings.current
    settings.current = { myGrid, engine }
    if (was.myGrid !== myGrid || was.engine !== engine) void refreshFeed(PATH_REGIONS)
  }, [myGrid, engine])
  const [mode, setMode] = useState<DayMode>('best')
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set())
  // The log's contacts per CQ zone, asked once (the shack keeps the fold); none on a surface that
  // cannot ask the logbook, where the fixed order stands.
  const [logged, setLogged] = useState<ZoneTally[]>([])
  useEffect(() => {
    let live = true
    getLogStats()
      .then((s) => live && setLogged(s.byZone))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])
  const regions = useMemo(() => orderRegions(data?.regions ?? [], needs, logged), [data, needs, logged])
  const hasLong = regions.some((r) => r.day.mufLong != null)
  const shown: DayMode = hasLong ? mode : 'short'
  const rows = useMemo<Row[]>(
    () =>
      regions.flatMap((r) => {
        // A region this build has no name for (a newer shack's) is left out, never mislabelled.
        const name = regionName(r.id)
        if (name == null) return []
        const expanded = open.has(r.id)
        const toggle = (
          <button
            type="button"
            className="pd-region"
            aria-expanded={expanded}
            title={expanded ? t('pathDay.region.collapse', { region: name }) : t('pathDay.region.expand', { region: name })}
            onClick={() =>
              setOpen((s) => {
                const n = new Set(s)
                if (n.has(r.id)) n.delete(r.id)
                else n.add(r.id)
                return n
              })
            }
          >
            <span className="pd-region-name">{name}</span>
          </button>
        )
        const best = bestBand(r.day, shown)
        const bands = expanded ? r.day.bands : best ? [best] : []
        return bands.map((band, i) => ({
          key: `${r.id}-${band.band}`,
          label: (
            <>
              {i === 0 ? toggle : null}
              <span className="pd-band">{band.band}</span>
            </>
          ),
          day: r.day,
          band,
          index: r.day.bands.indexOf(band),
        }))
      }),
    [regions, open, shown],
  )
  if (rows.length === 0) return <div className="pane-basic">{t('connect.pane.pathRegions.basic')}</div>
  const hours = regions[0].day.hours
  return (
    <div className="path-day path-regions">
      <div className="pd-head">
        <span className="pd-summary" title={t('pathDay.region.order')}>
          {t('connect.pane.pathRegions.basic')}
        </span>
        <ModeToggle mode={shown} onMode={setMode} hasLong={hasLong} />
      </div>
      <Grid
        rows={rows}
        hours={hours}
        nowHour={nowHour}
        mode={shown}
        scrub={scrub}
        onScrub={onScrub}
        className="regions"
        greyEnds="me"
      />
      <Legend hasLong={hasLong} showLong={hasLong && shown !== 'short'} />
    </div>
  )
}
