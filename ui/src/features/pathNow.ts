// What a flare or a proton event is doing NOW, in words — the shack's overlay
// (propagation::pathnow) on the Path card's why line, the ribbon, the path-over-the-day strip's now
// column, and the band outlook's / band-condition strip's "now" line.
//
// THE RULES THE WORDS KEEP.
// - Every figure on a path is OUR estimate from NOAA's inputs, so it always says "estimated from
//   NOAA's D-RAP grid HH:MMZ" or "estimated from GOES HH:MMZ" — never NOAA's own figure. Only the
//   recovery time NOAA's grid gives is "NOAA's own estimate"; the fit is "NOAA's rule of thumb".
// - Every clause carries its source's own time (the grid's valid time, the GOES sample's).
// - Below 1 dB nothing is said; past OUT_DB the figure is not quoted (the D-RAP law runs to
//   hundreds of dB under a big flare, which says only "gone").
// - The shack sends an overlay only at R1+ (a flare) or S1+ (a proton event), only beside fresh
//   samples, and the proton part only with a Kp to place the cap edge; with nothing sent nothing
//   is said here. The model's numbers are never touched.

import { t } from '../i18n'
import { flareClass } from '../mapGeo'
import { dayLabel } from '../components/prop/SolarTrends'
import type { BandNow, PathHourOverlay, PathSegmentNow, SpaceNow } from '../types'

/** From this many dB a path figure is worth a clause (the 1 dB the map's rings start at). */
export const SPEAK_DB = 1
/** Past this many dB a band is out, and the figure is not quoted. */
export const OUT_DB = 30

/** `HH:MMZ` for a time on the same UTC day as `nowSec`, else the day in front. */
export function clockZ(unix: number, nowSec: number): string {
  const d = new Date(unix * 1000)
  const hm = `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
  return Math.floor(unix / 86_400) === Math.floor(nowSec / 86_400) ? hm : `${dayLabel(unix)} ${hm}`
}

/** One band's overlay, or null (no overlay, or a band it has no figure for). */
export function bandNowOf(o: PathHourOverlay | null | undefined, band: string | null): BandNow | null {
  if (!o || !band) return null
  return o.bands.find((b) => b.band === band) ?? null
}

function flareSource(o: PathHourOverlay, nowSec: number): string {
  const s = o.source
  if (s.haf === 'noaa' && s.drapValidAt != null)
    return t('pathNow.src.noaa', { time: clockZ(s.drapValidAt, nowSec) })
  return t('pathNow.src.goes', { time: clockZ(s.xrayAt ?? o.unix, nowSec) })
}

function recoveryWords(o: PathHourOverlay, nowSec: number): string | null {
  const r = o.source.recovery
  if (!r) return null
  const time = clockZ(r.at, nowSec)
  return r.source === 'noaa' ? t('pathNow.recovery.noaa', { time }) : t('pathNow.recovery.fit', { time })
}

function flareClause(o: PathHourOverlay, b: BandNow, nowSec: number): string | null {
  const db = b.flareDb
  if (db == null || db < SPEAK_DB) return null
  const source = flareSource(o, nowSec)
  const text =
    db > OUT_DB
      ? t('pathNow.flareOut', { db: OUT_DB, band: b.band, source })
      : t('pathNow.flare', { db: Math.round(db), band: b.band, source })
  const rec = recoveryWords(o, nowSec)
  return rec ? `${text}${t('pathWhy.sep')}${rec}` : text
}

function pcaClause(o: PathHourOverlay, b: BandNow, nowSec: number): string | null {
  const db = b.pcaDb
  const s = o.source.sScale
  if (db == null || db < SPEAK_DB || s == null || o.source.protonsAt == null) return null
  const source = t('pathNow.src.protons', { time: clockZ(o.source.protonsAt, nowSec) })
  return db > OUT_DB
    ? t('pathNow.pcaOut', { db: OUT_DB, band: b.band, s, source })
    : t('pathNow.pca', { db: Math.round(db), band: b.band, s, source })
}

/** The why line's "now" clauses for `band`: a flare's and a proton event's, each with its source
 *  and time. Empty when there is nothing to say. */
export function nowClauses(o: PathHourOverlay | null | undefined, band: string | null, nowSec: number): string[] {
  const b = bandNowOf(o, band)
  if (!o || !b) return []
  return [flareClause(o, b, nowSec), pcaClause(o, b, nowSec)].filter((x): x is string => x != null)
}

/** The expert rows behind the clauses: how the figure was counted, and that it is laid over the
 *  model, not in it. */
export function nowDetail(o: PathHourOverlay | null | undefined, band: string | null): string[] {
  const b = bandNowOf(o, band)
  if (!o || !b || (b.flareDb == null && b.pcaDb == null)) return []
  const crossings = 2 * b.hops
  return [
    b.hopsModelled
      ? t('pathNow.detail.hops', { crossings, hops: b.hops })
      : t('pathNow.detail.hopsAssumed', { crossings, hops: b.hops }),
    t('pathNow.detail.note'),
  ]
}

/** The path-over-the-day strip's now-cell hover for `band`, or null when the cell is not marked. */
export function nowHit(o: PathHourOverlay | null | undefined, band: string, nowSec: number): string | null {
  const c = nowClauses(o, band, nowSec)
  return c.length ? t('pathNow.cell', { text: c.join(t('pathWhy.sep')) }) : null
}

/** Which hatch the strip's now cell for `band` wears: the flare's when it speaks, else the proton
 *  event's, else none. */
export function nowHitKind(o: PathHourOverlay | null | undefined, band: string): 'flare' | 'pca' | null {
  const b = bandNowOf(o, band)
  if (!b) return null
  if (b.flareDb != null && b.flareDb >= SPEAK_DB) return 'flare'
  return b.pcaDb != null && b.pcaDb >= SPEAK_DB && o?.source.sScale != null ? 'pca' : null
}

/** The ribbon's "now" stretches for `band` (flare ≥ 1 / ≥ 10 dB, proton absorption ≥ 1 dB). */
export function overlaySegments(o: PathHourOverlay | null | undefined, band: string | null): PathSegmentNow[] {
  return bandNowOf(o, band)?.segments ?? []
}

/** The band outlook's and the band-condition strip's "now" lines, one per thing happening. */
export function spaceNowLines(s: SpaceNow | null | undefined, nowSec: number): string[] {
  if (!s) return []
  const lines: string[] = []
  const f = s.flare
  if (f && (f.out || f.weak)) {
    const cls = flareClass(f.xrayLong)
    const source =
      f.haf === 'noaa' && f.drapValidAt != null
        ? t('spaceNow.src.noaa', { time: clockZ(f.drapValidAt, nowSec) })
        : t('spaceNow.src.goes', { time: clockZ(f.xrayAt, nowSec) })
    const until = f.recovery
      ? f.recovery.source === 'noaa'
        ? t('spaceNow.until.noaa', { time: clockZ(f.recovery.at, nowSec) })
        : t('spaceNow.until.fit', { time: clockZ(f.recovery.at, nowSec) })
      : ''
    lines.push(
      f.out
        ? t('spaceNow.flare.out', { cls, r: f.r, band: f.out, until, source })
        : t('spaceNow.flare.weak', { cls, r: f.r, band: f.weak ?? '', until, source }),
    )
  }
  if (s.protons) lines.push(t('spaceNow.protons', { s: s.protons.s, time: clockZ(s.protons.at, nowSec) }))
  if (s.watch) {
    const w = s.watch
    lines.push(
      w.from > nowSec
        ? t('spaceNow.watch.ahead', { g: w.g, time: clockZ(w.from, nowSec) })
        : t('spaceNow.watch.now', { g: w.g, time: clockZ(w.to, nowSec) }),
    )
  }
  return lines
}
