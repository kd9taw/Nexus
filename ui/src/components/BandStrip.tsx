import { useState } from 'react'
import type { SpotRow, NeedTag } from '../types'
import { bandRangeForLabel, cwRangeForLabel } from '../band'
import { NEED_CHIP } from '../features/needVisuals'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { BEACON_BADGE, SpotLegend, TYPE_BADGE } from './SpotLegend'

interface Props {
  /** Current operating band label (e.g. "20m"). */
  band: string
  /** Current dial frequency (MHz) — the "you are here" marker. */
  dialMhz: number
  /** Whether the current dial+mode is inside the operator's privileges (colors the marker). */
  txAllowed: boolean
  /** Operator's licensed phone sub-band [lo, hi) MHz — shaded. Absent = no shade. */
  phoneSegLo?: number | null
  phoneSegHi?: number | null
  /** All live cluster spots (unfiltered); the strip picks the ones matching `spotMode` on this band. */
  spots: SpotRow[]
  /** Which spot mode to plot — 'Phone' (SSB, default) for the Phone cockpit, 'CW' for the CW one. */
  spotMode?: 'Phone' | 'CW'
  /** Top need tag per heard call (UPPERCASE) — colours the tick by why it's worth working. */
  needByCall?: Map<string, NeedTag>
  /** Activity type per heard call (UPPERCASE) — flags POTA/SOTA/DXped independent of the need colour. */
  typeByCall?: Map<string, 'Pota' | 'Sota' | 'Dxped'>
  /** Work a spotted station — QSY to its exact freq + prefill the log (App's handleWorkSpot). */
  onWorkSpot: (s: SpotRow) => void
  /** When set, shows a "pop out" button that opens the full vertical band-map in its own window. */
  onPopOut?: () => void
}


/** Compact "how long ago" for a spot tooltip. */
function ageLabel(secs: number): string {
  if (secs < 0) return ''
  if (secs < 60) return `${secs}s ago`
  const m = Math.floor(secs / 60)
  return m < 60 ? `${m}m ago` : `${Math.floor(m / 60)}h ago`
}

/**
 * The spot band-activity strip — a proportional frequency scale for the CURRENT band with live
 * SSB cluster spots as clickable flags, the operator's licensed phone segment shaded, and a
 * "you are here" dial marker. This is the universal band-context answer for rigs without a native
 * panadapter: see at a glance where the SSB activity is (and how fresh), and click a flag to QSY
 * onto that station and prefill the log. Honest when quiet: says so rather than faking activity.
 */
export function BandStrip({
  band,
  dialMhz,
  txAllowed,
  phoneSegLo,
  phoneSegHi,
  spots,
  spotMode = 'Phone',
  needByCall,
  typeByCall,
  onWorkSpot,
  onPopOut,
}: Props) {
  // Legend is opt-in but remembered — it answers "what do the colours mean?" once, then
  // stays out of the way. Default ON the first time so the key is discoverable.
  const [showLegend, setShowLegend] = useState(
    () => (surfaceGet('nexus.spotlegend') ?? '1') === '1',
  )
  const toggleLegend = () => {
    setShowLegend((v) => {
      surfaceSet('nexus.spotlegend', v ? '0' : '1')
      return !v
    })
  }
  // In the CW cockpit, clip the strip to the band's CW sub-band (band bottom → CW top) so it shows
  // ONLY the CW portion; the Phone cockpit still spans the whole allocation. But only while the dial
  // is actually IN that segment — if the operator tunes above CW top (into the data/phone part) fall
  // back to the whole band so the "you are here" marker isn't clamped to the right edge (misreading
  // their position). Also falls back if the band has no distinct CW segment defined.
  const cwRange = spotMode === 'CW' ? cwRangeForLabel(band) : null
  const dialInCw = cwRange != null && dialMhz >= cwRange.lo && dialMhz <= cwRange.hi
  const range = (dialInCw ? cwRange : null) ?? bandRangeForLabel(band)
  const modeLabel = spotMode === 'CW' ? 'CW' : 'SSB'

  // Dial off the UI band plan — either a band we have no range for, or (listening off the ham
  // bands, a first-class use case per the operator ruling 2026-08-13) the backend's honest empty
  // band label. This used to `return null`, which left the pane frame around it still titled
  // "Band activity" with nothing inside and no word about why. Say what BandMap says in the
  // identical case (BandMap.tsx) — one condition, one vocabulary — keeping the pop-out, which is
  // how the operator reaches the map that plots what IS spotted.
  if (!range) {
    return (
      <div className="bandstrip">
        <div className="bandstrip-head">
          <span className="bandstrip-count">{band || '—'} — off the band plan</span>
          {onPopOut && (
            <button
              type="button"
              className="bandstrip-popout"
              onClick={onPopOut}
              title="Open the vertical band map in its own window"
            >
              ⧉ Band map
            </button>
          )}
        </div>
        <div className="bandstrip-track">
          <div className="bandstrip-empty">no band-plan data for {band || 'this frequency'}</div>
        </div>
      </div>
    )
  }

  const phone = spots
    .filter((s) => s.mode === spotMode && s.band === band)
    .sort((a, b) => a.freqMhz - b.freqMhz)

  // Span the selected range (whole band for Phone, the CW sub-band for CW) so every part of it is
  // visible and clickable. The phone segment is shaded so the voice portion still reads at a glance.
  const lo = range.lo
  const hi = range.hi
  const span = Math.max(hi - lo, 1e-6)
  const pct = (mhz: number) => Math.min(100, Math.max(0, ((mhz - lo) / span) * 100))

  const shade =
    phoneSegLo != null && phoneSegHi != null
      ? { left: pct(phoneSegLo), width: pct(phoneSegHi) - pct(phoneSegLo) }
      : null

  return (
    <div className="bandstrip">
      <div className="bandstrip-head">
        {/* No title of its own (density pass 2026-08-04): BOTH hosts render this strip in a
            CockpitPaneFrame whose head reads "Band activity" and is the pane's accessible
            name — PhoneCockpit and CwCockpit, checked host by host before the delete, since
            a component with an unframed surface (as LogEntry has in SatellitesView) would
            have needed a prop instead. The live count below is not a duplicate and stays. */}
        <span className="bandstrip-count">
          {phone.length > 0
            ? `${phone.length} ${modeLabel} spot${phone.length === 1 ? '' : 's'} · ${band}`
            : `no ${modeLabel} spots on ${band} yet`}
        </span>
        <button
          type="button"
          className={`bandstrip-legend-toggle${showLegend ? ' on' : ''}`}
          onClick={toggleLegend}
          title="Show/hide the colour + type key"
          aria-pressed={showLegend}
        >
          Legend
        </button>
        {onPopOut && (
          <button
            type="button"
            className="bandstrip-popout"
            onClick={onPopOut}
            title="Open the vertical band map in its own window"
          >
            ⧉ Band map
          </button>
        )}
      </div>
      {showLegend && <SpotLegend />}
      <div className="bandstrip-track" title={`${band}: ${lo.toFixed(3)}–${hi.toFixed(3)} MHz`}>
        {shade && shade.width > 0 && (
          <div
            className="bandstrip-shade"
            style={{ left: `${shade.left}%`, width: `${shade.width}%` }}
            title="Your licensed phone segment on this band"
          />
        )}
        {phone.map((s, i) => {
          // Fade older spots so density + freshness read at a glance (fresh ≈ opaque, ~30 min → faint).
          const opacity = s.ageSecs < 0 ? 0.9 : Math.max(0.35, 1 - s.ageSecs / 1800)
          const cu = s.call.toUpperCase()
          // A one-way beacon/bulletin row never takes a need colour — see the note in BandMap:
          // needByCall is keyed by call, so a call that is both a beacon site and a real
          // station would otherwise colour its beacon row too.
          const beacon = s.beacon ? BEACON_BADGE[s.beacon] : null
          // Colour the tick by need tier (why it's worth working) — parity with the band map.
          const need = beacon ? undefined : needByCall?.get(cu)
          const needCls = need ? ` is-need need-${NEED_CHIP[need].cls}` : ''
          // Flag the activity type (POTA/SOTA/DXped) independent of the colour.
          const type = typeByCall?.get(cu)
          const badge = type ? TYPE_BADGE[type] : null
          const detail = [
            s.call,
            beacon?.word,
            need && NEED_CHIP[need].label,
            badge?.word,
            `${s.freqMhz.toFixed(3)} MHz`,
            ageLabel(s.ageSecs),
            s.spotter && `de ${s.spotter}`,
            s.comment,
          ]
            .filter(Boolean)
            .join(' · ')
          return (
            <button
              key={`${s.call}-${s.freqMhz}-${i}`}
              type="button"
              className="bandstrip-spot"
              style={{ left: `${pct(s.freqMhz)}%`, opacity }}
              title={`${detail} — click to work`}
              onClick={() => onWorkSpot(s)}
            >
              {beacon && (
                <span className={`bandstrip-type spot-type-badge ${beacon.cls}`}>{beacon.ch}</span>
              )}
              {badge && <span className={`bandstrip-type spot-type-badge ${badge.cls}`}>{badge.ch}</span>}
              <span className={`bandstrip-tick${needCls}`} />
              <span className="bandstrip-spot-call mono">{s.call}</span>
            </button>
          )
        })}
        <div
          className={`bandstrip-dial${txAllowed ? '' : ' blocked'}`}
          style={{ left: `${pct(dialMhz)}%` }}
          title={`You: ${dialMhz.toFixed(3)} MHz${txAllowed ? '' : ' — transmit blocked (outside your privileges)'}`}
        />
      </div>
      <div className="bandstrip-axis mono">
        <span>{lo.toFixed(3)}</span>
        <span>{hi.toFixed(3)} MHz</span>
      </div>
    </div>
  )
}
