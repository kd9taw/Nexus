// Greyline opportunity pane (B3 Expert view). Pure clock + geometry — no network. Shows
// the operator's next sunrise/sunset terminator countdown, which bands the model flags as
// greyline-favored now, and which DX entities are currently sitting on their own greyline
// (low-band long-path candidates). Re-renders on the snapshot poll, so the countdown is live.
import { nextTerminatorMs, solarElevationDeg } from '../../mapGeo'
import { gridToLatLon, bearingDeg } from '../../grid'
import { t } from '../../i18n'
import { T } from '../../i18n/T'
import type { PaneContext } from '../connect/paneContext'

function fmtZ(ms: number): string {
  const d = new Date(ms)
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

export function GreylineWindow({ ctx }: { ctx: PaneContext }) {
  const ll = ctx.myGrid ? gridToLatLon(ctx.myGrid) : null
  if (!ll) return <p className="pane-basic">{t('prop.greyline.noGrid')}</p>
  const now = Date.now()
  const next = nextTerminatorMs(ll.lat, ll.lon, now)
  const mins = Math.max(0, Math.round((next.atMs - now) / 60000))
  const when = mins >= 60 ? `${Math.floor(mins / 60)}h ${mins % 60}m` : `${mins}m`
  // DX entities currently on their greyline (|solar elevation| < 6°), from the anchored
  // spots, deduped by entity — the low-band long-path candidates. Keep the first spot's
  // location so we can tell the operator which way to point the beam.
  const onGrey = new Map<string, { lat: number; lon: number }>()
  for (const s of ctx.prop?.spots ?? []) {
    if (!s.entity || onGrey.has(s.entity)) continue
    if (Math.abs(solarElevationDeg(s.lat, s.lon, now)) < 6) onGrey.set(s.entity, { lat: s.lat, lon: s.lon })
  }
  const greyBands = (ctx.bandOutlook?.bands ?? []).filter((b) => b.grayline).map((b) => b.band)
  return (
    <div className="gl">
      <p className="gl-next">
        {/* Two whole sentences: "Sunrise"/"Sunset" is the SUBJECT, and a translator has to
            be free to put it where their language wants it. */}
        {next.kind === 'rise' ? (
          <T k="prop.greyline.next.rise" tags={{ b: <strong /> }} vals={{ when, at: fmtZ(next.atMs) }} />
        ) : (
          <T k="prop.greyline.next.set" tags={{ b: <strong /> }} vals={{ when, at: fmtZ(next.atMs) }} />
        )}
      </p>
      {greyBands.length > 0 && (
        <p className="gl-bands">{t('prop.greyline.favors', { bands: greyBands.join(', ') })}</p>
      )}
      {onGrey.size > 0 && (
        <p className="gl-regions">
          {t('prop.greyline.onGrey', {
            entities: [...onGrey.entries()]
              .slice(0, 6)
              .map(([entity, p]) => `${entity} ${bearingDeg(ll, p)}°`)
              .join(' · '),
          })}
        </p>
      )}
      {onGrey.size === 0 && greyBands.length === 0 && (
        <p className="cp-none">{t('prop.greyline.none')}</p>
      )}
    </div>
  )
}
