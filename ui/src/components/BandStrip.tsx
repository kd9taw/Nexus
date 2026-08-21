// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the band and mode names (`modeLabel` is CW or
// SSB), the scale's edge frequencies, the dial reading, and every value in a spot's tooltip —
// `detail` is the spot line assembled from cluster data, and `de <spotter>` is the cluster's own
// shorthand. The need and badge WORDS arrive already translated from `needVisuals.ts` and
// `SpotLegend.tsx`, whose tables resolve through getters.
//
// The dial marker reads `txAllowed` to say the engine is blocking transmit here. It is a readout;
// this surface has no transmit control and is on no cockpit's stop-line census.
import { useRef, useState } from 'react'
import type { SpotRow, NeedTag, AppSnapshot } from '../types'
import { bandRangeForLabel, cwRangeForLabel } from '../band'
import { useWheelTune } from '../useWheelTune'
import { NEED_CHIP } from '../features/needVisuals'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { t } from '../i18n'
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

  // ── Tuning from the strip itself (#96) ─────────────────────────────────────────────────
  // The pop-out band map wheel-tunes (#39); the docked strip is the same frequency scale in
  // the same cockpits and stayed silent, which read as an inconsistency. Same shape as
  // BandMap's: all optional, and with these absent the strip is exactly the read-only scale
  // it was — which is what keeps the existing tests honest.
  /** Sideband to preserve so a strip tune never flips the mode. */
  sideband?: string
  /** Tune only when CAT is up and nothing is transmitting. Absent ⇒ read-only strip. */
  tuneEnabled?: boolean
  /** Wheel step (Hz), the operator's tuning step — same value the cockpit dials use. */
  stepHz?: number
  /** Settings ▸ Radio wheel sensitivity, so the strip matches every other dial. */
  wheelSensitivity?: number
  /** Fresh snapshot after a tune, so the dial marker moves without waiting for a poll. */
  onSnap?: (s: AppSnapshot) => void
}


/** The scale's unit, printed on the axis beside the edge frequency. A unit symbol is a token:
 * MHz is MHz in every language, and the guard is told so by this constant. */
const MHZ_UNIT = 'MHz'

/** Compact "how long ago" for a spot tooltip. The unit letter rides inside the message with its
 * number, so a translation can never separate the two. */
function ageLabel(secs: number): string {
  if (secs < 0) return ''
  if (secs < 60) return t('bandStrip.age.secs', { secs })
  const m = Math.floor(secs / 60)
  return m < 60 ? t('bandStrip.age.mins', { mins: m }) : t('bandStrip.age.hours', { hours: Math.floor(m / 60) })
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
  sideband,
  tuneEnabled,
  stepHz,
  wheelSensitivity,
  onSnap,
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
  // Wheel-tune the track, through the SAME hook the readout digits, the waterfall and the
  // pop-out band map (#39) use — coalescer, band-edge handling, per-event step cap and the
  // sensitivity setting are all the ones the operator already knows, never a second
  // implementation that drifts. `enabled` false makes it inert (the read-only case).
  // Called BEFORE the off-plan early return below: hooks must run on every render.
  const trackEl = useRef<HTMLDivElement | null>(null)
  useWheelTune(trackEl, {
    dialMhz,
    sideband: sideband || 'USB',
    enabled: tuneEnabled === true,
    stepHz: stepHz ?? 1000,
    sensitivity: wheelSensitivity,
    onSnap,
  })
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
          <span className="bandstrip-count">{t('bandStrip.offPlan', { band: band || '—' })}</span>
          {onPopOut && (
            <button
              type="button"
              className="bandstrip-popout"
              onClick={onPopOut}
              title={t('bandStrip.popout.title')}
            >
              {t('bandStrip.popout.label')}
            </button>
          )}
        </div>
        <div className="bandstrip-track">
          <div className="bandstrip-empty">
            {t('bandStrip.empty.noPlan', { band: band || t('bandStrip.empty.thisFrequency') })}
          </div>
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
            ? t('bandStrip.count', { count: phone.length, mode: modeLabel, band })
            : t('bandStrip.empty.none', { mode: modeLabel, band })}
        </span>
        <button
          type="button"
          className={`bandstrip-legend-toggle${showLegend ? ' on' : ''}`}
          onClick={toggleLegend}
          title={t('bandStrip.legend.title')}
          aria-pressed={showLegend}
        >
          {t('bandStrip.legend.label')}
        </button>
        {onPopOut && (
          <button
            type="button"
            className="bandstrip-popout"
            onClick={onPopOut}
            title={t('bandStrip.popout.title')}
          >
            {t('bandStrip.popout.label')}
          </button>
        )}
      </div>
      {showLegend && <SpotLegend />}
      <div
        ref={trackEl}
        className="bandstrip-track"
        title={
          tuneEnabled === true
            ? t('bandStrip.track.title.tunable', { band, lo: lo.toFixed(3), hi: hi.toFixed(3) })
            : t('bandStrip.track.title', { band, lo: lo.toFixed(3), hi: hi.toFixed(3) })
        }
      >
        {shade && shade.width > 0 && (
          <div
            className="bandstrip-shade"
            style={{ left: `${shade.left}%`, width: `${shade.width}%` }}
            title={t('bandStrip.shade.title')}
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
            // State/province, when the FCC index or a heard grid resolved one — the same
            // value the roster's State pill shows (operator ask, 2026-08-16).
            s.state,
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
              title={t('bandStrip.spot.title', { detail })}
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
          title={
            txAllowed
              ? t('bandStrip.dial.title', { freq: dialMhz.toFixed(3) })
              : t('bandStrip.dial.title.blocked', { freq: dialMhz.toFixed(3) })
          }
        />
      </div>
      <div className="bandstrip-axis mono">
        <span>{lo.toFixed(3)}</span>
        <span>
          {hi.toFixed(3)} {MHZ_UNIT}
        </span>
      </div>
    </div>
  )
}
