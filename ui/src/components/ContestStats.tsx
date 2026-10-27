// ---------------------------------------------------------------------------
// ⭐ THE CONTEST STATS TAB — the Contest screen's second view: how the contest is going, live,
// from the contest log the session already holds. Totals and rates, contacts per UTC hour,
// distances, bands and modes, countries and grids, the contest's own multiplier boards and the
// score. "Share stats" saves a picture to Downloads; nothing is posted.
//
// READ-ONLY: it changes no contact, sends nothing and keys nothing. The numbers come from the
// same places the scoreboard reads (the engine's score and multiplier boards, `contestRate`,
// `contestOpTime`); what is new is worked out in `features/contestStats`. The one thing asked of
// the station is where each call is (`contestStatsPlaces`, the country file); a station that
// cannot answer (an older one, a Remote observation) leaves countries and distances unsaid.
//
// One scroll owner: the Contest screen's panel. The tab adds no scroller of its own.
// ---------------------------------------------------------------------------
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { t } from '../i18n'
import type { FieldDayStatus } from '../types'
import { contestStatsPlaces } from '../api'
import { contestStats, distanceStats, entityStats, type StatsPlace, type Tally } from '../features/contestStats'
import { renderStatsCard, saveStatsCard } from '../features/contestStatsCard'
import { contestRate } from '../features/contestRate'
import { formatOpTime, offTimeRule } from '../features/contestOpTime'
import { utcHhmm } from '../features/contestRemoval'
import { contestFileStem } from '../fdEvent'
import { gridToLatLon } from '../grid'
import { fmtDistanceKm, useUnits } from '../units'

/** No reading. A symbol, not prose: the scoreboard's own. */
const NONE = '—'
/** Between a call and its country. A separator, not prose. */
const SEP = ' · '
/** The countries listed before "Show all". */
const TOP_ENTITIES = 10

/** One counted figure. */
function Tile({ label, value, title }: { label: string; value: string | number; title?: string }) {
  return (
    <div className="contest-stats-tile" role="group" aria-label={t('contestStats.tile.aria', { label, value })} title={title}>
      <span className="contest-stats-val">{value}</span>
      <span className="contest-stats-label">{label}</span>
    </div>
  )
}

/** Horizontal bars, one per entry, the longest the busiest. */
function Bars({ rows, label }: { rows: Tally[]; label: string }) {
  const top = Math.max(1, ...rows.map((r) => r.count))
  return (
    <ul className="contest-stats-bars" aria-label={label}>
      {rows.map((r) => (
        <li key={r.key} aria-label={t('contestStats.bar.aria', { name: r.key, count: r.count })}>
          <span className="contest-stats-bar-key">{r.key}</span>
          <span className="contest-stats-bar-track" aria-hidden="true">
            <span className="contest-stats-bar-fill" style={{ width: `${(r.count / top) * 100}%` }} />
          </span>
          <span className="contest-stats-bar-count">{r.count}</span>
        </li>
      ))}
    </ul>
  )
}

export function ContestStats({
  fieldDay,
  nowUnix,
  myCall,
  myGrid,
  contest,
  mults,
  score,
}: {
  fieldDay: FieldDayStatus | null
  nowUnix: number
  /** The station's call, for its own country and the card. */
  myCall: string
  /** The station's grid, the point every distance is measured from; empty when not set. */
  myGrid: string
  /** The contest, as the screen's header names it. */
  contest: string
  /** The contest's own multiplier boards, as the scoreboard draws them. */
  mults: ReactNode
  /** The score and its arithmetic, as the scoreboard draws them. */
  score: ReactNode
}) {
  const units = useUnits()
  const log = useMemo(() => fieldDay?.log ?? [], [fieldDay?.log])
  const event = fieldDay?.event ?? ''
  // The break that counts as off time: this contest's own, or the default where none is on record.
  const { minOffSec, fromRules } = offTimeRule(event)
  const gridSlot = useMemo(() => {
    const i = (fieldDay?.receives ?? []).findIndex((r) => r.kind === 'grid')
    return i >= 0 ? i : null
  }, [fieldDay?.receives])

  const stats = useMemo(
    () =>
      contestStats(log, {
        nowUnix,
        windowStart: fieldDay?.eventStartUnix,
        windowEnd: fieldDay?.eventEndUnix,
        gridSlot,
        minOffSec,
      }),
    [log, nowUnix, fieldDay?.eventStartUnix, fieldDay?.eventEndUnix, gridSlot, minOffSec],
  )
  const current = useMemo(() => contestRate(log, nowUnix).windows[0], [log, nowUnix])

  // Where each call is: asked once per set of calls, the station's own included.
  const callKey = useMemo(() => [...new Set([myCall, ...log.map((q) => q.call)].filter(Boolean))].sort().join(' '), [log, myCall])
  const [places, setPlaces] = useState<ReadonlyMap<string, StatsPlace | null> | null>(null)
  const [placesFailed, setPlacesFailed] = useState(false)
  useEffect(() => {
    if (!callKey) return
    let live = true
    const calls = callKey.split(' ')
    contestStatsPlaces(calls, event).then(
      (answer) => {
        if (!live || !Array.isArray(answer)) return
        setPlaces(new Map(calls.map((c, i) => [c, answer[i] ?? null])))
        setPlacesFailed(false)
      },
      () => live && setPlacesFailed(true),
    )
    return () => {
      live = false
    }
  }, [callKey, event])

  const home = myGrid ? gridToLatLon(myGrid) : null
  const dist = useMemo(
    () => (places ? distanceStats(log, places, myGrid ? gridToLatLon(myGrid) : null, gridSlot) : null),
    [log, places, myGrid, gridSlot],
  )
  const ents = useMemo(
    () => (places ? entityStats(log, places, places.get(myCall)?.entity ?? null) : null),
    [log, places, myCall],
  )
  const [allEntities, setAllEntities] = useState(false)

  const perHour = (r: number | null) => (r === null ? NONE : t('contestStats.perHour', { rate: Math.round(r) }))
  const tiles = [
    { label: t('contestStats.contacts'), value: String(stats.contacts) },
    { label: t('contestStats.onTime'), value: stats.op ? formatOpTime(stats.op.onSec) : NONE },
    // The off time, under a label that counts the breaks it is made of.
    { label: t('contestStats.breaks', { count: stats.op?.breaks ?? 0 }), value: stats.op ? formatOpTime(stats.op.offSec) : NONE },
    { label: t('contestStats.overall'), value: perHour(stats.overallPerHour) },
    { label: t('contestStats.best60'), value: stats.best60 ? String(stats.best60.count) : NONE },
    { label: t('contestStats.current'), value: perHour(current.perHour) },
    {
      label: t('contestStats.sinceLast'),
      value: stats.lastContactUnix === null ? NONE : formatOpTime(Math.max(0, nowUnix - stats.lastContactUnix)),
    },
    { label: t('contestStats.today'), value: String(stats.today) },
  ]

  const share = () => {
    const card = renderStatsCard({
      call: myCall,
      contest,
      tiles: [
        tiles[0],
        tiles[1],
        tiles[3],
        tiles[4],
        { label: t('contestStats.score.points'), value: typeof fieldDay?.totalScore === 'number' ? String(fieldDay.totalScore) : NONE },
        { label: t('contestStats.score.mults'), value: typeof fieldDay?.multCount === 'number' ? String(fieldDay.multCount) : NONE },
        { label: t('contestStats.distance.longest'), value: dist?.longest ? fmtDistanceKm(dist.longest.km, units) : NONE },
        { label: t('contestStats.dx.entities'), value: ents ? String(ents.entities.length) : NONE },
      ],
      hours: stats.hours.map((h) => h.count),
      footer: t('contestStats.card.footer'),
    })
    const stamp = new Date(nowUnix * 1000).toISOString().slice(0, 10)
    void saveStatsCard(card, `${(myCall || 'nexus').toUpperCase()}-${contestFileStem(event)}-stats-${stamp}.png`)
  }

  const topHour = Math.max(1, ...stats.hours.map((h) => h.count))
  // An hour label under every Nth bar, so 48 labels never crowd.
  const labelEvery = Math.max(1, Math.ceil(stats.hours.length / 12))
  const entityRows = ents ? (allEntities ? ents.entities : ents.entities.slice(0, TOP_ENTITIES)) : []
  const minutes = Math.round(minOffSec / 60)

  return (
    <div className="contest-stats" role="tabpanel" id="contest-stats-panel" aria-label={t('contestStats.tab.stats')}>
      <div className="contest-stats-head">
        <h3>{t('contestStats.totals.head')}</h3>
        <button type="button" className="settings-refresh" onClick={share} title={t('contestStats.share.title')}>
          {t('contestStats.share')}
        </button>
      </div>
      <div className="contest-stats-tiles">
        {tiles.map((x) => (
          <Tile key={x.label} label={x.label} value={x.value} />
        ))}
      </div>
      <p className="contest-stats-note">
        {fromRules ? t('contestStats.offRule', { minutes }) : t('contestStats.offRule.default', { minutes })}
      </p>
      {stats.best60 && (
        <p className="contest-stats-note">{t('contestStats.best60.from', { time: utcHhmm(stats.best60.startUnix) })}</p>
      )}

      <section className="contest-stats-section" aria-label={t('contestStats.hours.head')}>
        <h3>{t('contestStats.hours.head')}</h3>
        {stats.hours.length === 0 ? (
          <p className="contest-stats-note">{t('contestStats.empty')}</p>
        ) : (
          <ol className="contest-stats-hours">
            {stats.hours.map((h, i) => (
              <li key={h.startUnix} aria-label={t('contestStats.hours.bar', { hour: utcHhmm(h.startUnix), count: h.count })}>
                <span className="contest-stats-hour-bar" aria-hidden="true">
                  <span className="contest-stats-hour-fill" style={{ height: `${(h.count / topHour) * 100}%` }} title={String(h.count)} />
                </span>
                <span className="contest-stats-hour-label" aria-hidden="true">
                  {i % labelEvery === 0 ? utcHhmm(h.startUnix).slice(0, 2) : ''}
                </span>
              </li>
            ))}
          </ol>
        )}
        {stats.outsideWindow > 0 && (
          <p className="contest-stats-note">{t('contestStats.hours.outside', { count: stats.outsideWindow })}</p>
        )}
      </section>

      <div className="contest-stats-grid">
        <section className="contest-stats-section" aria-label={t('contestStats.distance.head')}>
          <h3>{t('contestStats.distance.head')}</h3>
          {!home ? (
            <p className="contest-stats-note">{t('contestStats.distance.noGrid')}</p>
          ) : placesFailed && !places ? (
            <p className="contest-stats-note">{t('contestStats.unavailable')}</p>
          ) : dist && dist.longest ? (
            <>
              <div className="contest-stats-tiles">
                <Tile
                  label={t('contestStats.distance.longest')}
                  value={fmtDistanceKm(dist.longest.km, units)}
                />
                <Tile label={t('contestStats.distance.average')} value={dist.averageKm === null ? NONE : fmtDistanceKm(dist.averageKm, units)} />
              </div>
              <p className="contest-stats-longest">
                <span className="mono">{dist.longest.call}</span>
                {dist.longest.entity && SEP + dist.longest.entity}
              </p>
              <p className="contest-stats-note">{t('contestStats.distance.note')}</p>
            </>
          ) : (
            <p className="contest-stats-note">{t('contestStats.empty')}</p>
          )}
        </section>

        <section className="contest-stats-section" aria-label={t('contestStats.bands.head')}>
          <h3>{t('contestStats.bands.head')}</h3>
          {stats.bands.length ? <Bars rows={stats.bands} label={t('contestStats.bands.head')} /> : <p className="contest-stats-note">{t('contestStats.empty')}</p>}
        </section>

        <section className="contest-stats-section" aria-label={t('contestStats.modes.head')}>
          <h3>{t('contestStats.modes.head')}</h3>
          {stats.modes.length ? <Bars rows={stats.modes} label={t('contestStats.modes.head')} /> : <p className="contest-stats-note">{t('contestStats.empty')}</p>}
        </section>

        <section className="contest-stats-section" aria-label={t('contestStats.dx.head')}>
          <h3>{t('contestStats.dx.head')}</h3>
          <div className="contest-stats-tiles">
            <Tile label={t('contestStats.dx.entities')} value={ents ? ents.entities.length : NONE} />
            <Tile label={t('contestStats.dx.dx')} value={ents?.dx ?? NONE} />
            <Tile
              label={t('contestStats.dx.grids')}
              value={stats.grids ? stats.grids.length : NONE}
              title={stats.grids ? undefined : t('contestStats.dx.grids.none')}
            />
          </div>
          {placesFailed && !places ? (
            <p className="contest-stats-note">{t('contestStats.unavailable')}</p>
          ) : (
            ents && (
              <>
                {entityRows.length > 0 && (
                  <Bars rows={entityRows.map((e) => ({ key: e.entity, count: e.count }))} label={t('contestStats.dx.head')} />
                )}
                {ents.entities.length > TOP_ENTITIES && (
                  <button type="button" className="settings-refresh" aria-expanded={allEntities} onClick={() => setAllEntities((v) => !v)}>
                    {allEntities ? t('contestStats.dx.showFewer') : t('contestStats.dx.showAll', { count: ents.entities.length })}
                  </button>
                )}
                {ents.unplaced > 0 && <p className="contest-stats-note">{t('contestStats.dx.unplaced', { count: ents.unplaced })}</p>}
              </>
            )
          )}
        </section>
      </div>

      <section className="contest-stats-section" aria-label={t('contestStats.score.head')}>
        <h3>{t('contestStats.score.head')}</h3>
        {score}
      </section>

      <section className="contest-stats-section contest-stats-mults" aria-label={t('contestStats.mults.head')}>
        <h3>{t('contestStats.mults.head')}</h3>
        {mults}
      </section>
    </div>
  )
}
