// The flare chip — the words beside the flare layer, on the 2-D map and the 3-D globe alike.
//
// Folded it is two lines: what the flare is (class, R-scale, how high HF is absorbed under the
// sun) and where it is (NOAA's began / peak / ended, or our trend word marked estimated), then
// what the picture is drawn from and when. Unfolded it adds the bands out and weakened under the
// sun and at the operator's QTH, how long until HF recovers, and the six-hour X-ray sparkline.
//
// Every number carries its source and time: the class is GOES at its sample time, the footprint
// is NOAA's D-RAP grid at its valid time or "estimated from GOES", the recovery is NOAA's own or
// NOAA's empirical fit. A simulated flare (Preview) says PREVIEW and shows none of NOAA's words.

import { useState } from 'react'
import { t } from '../../i18n'
import { flareClass, flareRScale } from '../../mapGeo'
import { wxStampLabel } from '../../propViz'
import {
  flareHere,
  flarePhase,
  flareRecovery,
  flareUnderSun,
  XRAY_LIVE_SECS,
  type FlareFootprint,
  type FlareNoaa,
} from '../../flareNow'
import { agoLabel } from './SpaceWxGauges'
import { devXrayOverride } from '../../flareAlert'

/** `HH:MMZ` — chip times are always today's or the last few hours'. */
function hm(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

export function FlareChip({
  xrayLong,
  foot,
  noaa,
  trend,
  qth,
  nowMs,
  snapAt = null,
  preview = false,
}: {
  xrayLong: number
  foot: FlareFootprint
  /** NOAA's trace, event and grid; null for simulated flux. */
  noaa: FlareNoaa | null
  trend: 'rising' | 'steady' | 'falling' | null
  qth: { lat: number; lon: number } | null
  nowMs: number
  /** The prop snapshot's GOES sample time: the class comes from the snapshot when NOAA's fast lane
   *  is absent or stale, and this is then its time. */
  snapAt?: number | null
  /** Simulated flux from the Layers-panel Preview button — labelled so simulated data can never
   *  be mistaken for a real event. */
  preview?: boolean
}) {
  const [open, setOpen] = useState(false)
  const head = t('map.flare.chip', {
    cls: flareClass(xrayLong),
    r: flareRScale(xrayLong),
    haf: Math.round(foot.hafSub),
  })
  if (preview) {
    return (
      <div className="flare-chip" role="status" data-source={foot.source}>
        {head}
        {t('map.flare.phase.preview')}
      </div>
    )
  }
  const phase = flarePhase(noaa?.event, trend, nowMs, xrayLong)
  const phaseLine = phase.noaa
    ? phase.kind === 'ended'
      ? t('map.flare.noaa.ended', { end: hm(phase.endAt!), begin: hm(phase.beginAt!) })
      : phase.kind === 'peaked'
        ? phase.peakClass
          ? t('map.flare.noaa.peakedClass', { begin: hm(phase.beginAt!), peak: hm(phase.peakAt!), cls: phase.peakClass })
          : t('map.flare.noaa.peaked', { begin: hm(phase.beginAt!), peak: hm(phase.peakAt!) })
        : t('map.flare.noaa.rising', { begin: hm(phase.beginAt!) })
    : phase.kind === 'rising'
      ? t('map.flare.est.rising')
      : phase.kind === 'falling'
        ? t('map.flare.est.falling')
        : null
  // No countdown while it is still rising: NOAA's fit is for the decay.
  const rec = phase.kind === 'rising' ? null : flareRecovery(foot.source === 'noaa' ? noaa?.drap : null, xrayLong, nowMs)
  // The GOES sample the class came from: the fast lane's, else the snapshot's. The dev override's
  // flux is simulated and has no sample time.
  const xrayAt = noaa?.xrayAt ?? (devXrayOverride() == null ? snapAt : null)
  const xrayAge = xrayAt != null ? Math.max(0, nowMs / 1000 - xrayAt) : null
  const xtime = xrayAt == null ? null : xrayAge! >= XRAY_LIVE_SECS ? `${hm(xrayAt)}, ${agoLabel(xrayAge!)}` : hm(xrayAt)
  const src =
    foot.source === 'noaa'
      ? t('map.flare.src.noaa', { time: hm(foot.validAt!), xtime: xtime ?? '—' })
      : xtime
        ? t('map.flare.src.goes', { xtime })
        : t('map.flare.src.goesOnly')
  const srcTitle = [
    foot.source === 'noaa'
      ? t('map.flare.src.noaa.title', { time: wxStampLabel(foot.validAt!) })
      : t('map.flare.src.goes.title'),
    foot.gapMhz != null && foot.gapMhz > 2 ? t('map.flare.src.gap.title', { mhz: foot.gapMhz.toFixed(1) }) : null,
    t('map.flare.lightspeed'),
  ]
    .filter(Boolean)
    .join('\n')
  const sun = flareUnderSun(foot)
  const here = qth ? flareHere(foot, qth.lat, qth.lon, nowMs) : null
  return (
    <div className={`flare-chip${open ? ' open' : ''}`} role="status">
      <button
        type="button"
        className="flare-chip-head"
        aria-expanded={open}
        aria-label={t('map.flare.toggle.aria')}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="flare-chip-chev" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
        {head}
      </button>
      {phaseLine && <div className="flare-chip-line">{phaseLine}</div>}
      <div className="flare-chip-line flare-chip-src" data-source={foot.source} title={srcTitle}>
        {src}
      </div>
      {open && (
        <>
          <div className="flare-chip-line">
            {sun.out
              ? t('map.flare.sun.out', { out: sun.out, weak: sun.weak ?? sun.out })
              : sun.weak
                ? t('map.flare.sun.weak', { weak: sun.weak })
                : t('map.flare.sun.none')}
          </div>
          {here && (
            <div className="flare-chip-line">
              {here.night
                ? t('map.flare.here.night')
                : here.out
                  ? t('map.flare.here.out', { out: here.out, weak: here.weak ?? here.out })
                  : here.weak
                    ? t('map.flare.here.weak', { weak: here.weak })
                    : t('map.flare.here.none')}
            </div>
          )}
          {rec && (
            <div className="flare-chip-line">
              {rec.noaa ? t('map.flare.recovery.noaa', { mins: rec.mins }) : t('map.flare.recovery.fit', { mins: rec.mins })}
            </div>
          )}
          <div className="flare-chip-line flare-chip-dim">{t('map.flare.rings.legend')}</div>
          {noaa && noaa.series.length > 1 && <FlareSparkline series={noaa.series} phase={phase} />}
        </>
      )}
    </div>
  )
}

// Sparkline geometry (SVG user units; the CSS sets the drawn size).
const SPARK_W = 180
const SPARK_H = 24
const LOG_LO = -8 // A1
const LOG_HI = -3 // X10

/** The six-hour GOES long-band flux on a log scale, with the M1 and X1 lines and NOAA's begin and
 *  peak marks. */
export function FlareSparkline({
  series,
  phase,
}: {
  series: [number, number][]
  phase: { noaa: boolean; beginAt: number | null; peakAt: number | null }
}) {
  const t0 = series[0][0]
  const t1 = series[series.length - 1][0]
  const span = Math.max(1, t1 - t0)
  const x = (unix: number) => ((unix - t0) / span) * SPARK_W
  const y = (flux: number) => {
    const l = Math.max(LOG_LO, Math.min(LOG_HI, Math.log10(flux)))
    return SPARK_H - ((l - LOG_LO) / (LOG_HI - LOG_LO)) * SPARK_H
  }
  const d = series.map(([u, f], i) => `${i ? 'L' : 'M'}${x(u).toFixed(1)},${y(f).toFixed(1)}`).join('')
  const mark = (u: number | null) =>
    phase.noaa && u != null && u >= t0 && u <= t1 ? x(u).toFixed(1) : null
  const begin = mark(phase.beginAt)
  const peak = mark(phase.peakAt)
  return (
    <svg
      className="flare-spark"
      viewBox={`0 0 ${SPARK_W} ${SPARK_H}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={t('map.flare.spark.aria', { from: hm(t0), to: hm(t1) })}
    >
      <title>{t('map.flare.spark.aria', { from: hm(t0), to: hm(t1) })}</title>
      <line className="flare-spark-ref" x1={0} x2={SPARK_W} y1={y(1e-5)} y2={y(1e-5)} />
      <line className="flare-spark-ref x" x1={0} x2={SPARK_W} y1={y(1e-4)} y2={y(1e-4)} />
      {begin && <line className="flare-spark-mark" x1={begin} x2={begin} y1={0} y2={SPARK_H} />}
      {peak && <line className="flare-spark-mark peak" x1={peak} x2={peak} y1={0} y2={SPARK_H} />}
      <path className="flare-spark-line" d={d} />
    </svg>
  )
}
