// The replay bar — the six-hour scrub of the sun, the night side and the flare, on the 2-D map and
// the 3-D globe alike. It heads the chip stack while a replay is on, and the flare chip (which
// speaks for now) stands down.
//
// IT MUST NEVER READ AS NOW. Its head says REPLAY, the replayed UTC time and how long ago that was,
// in the replay ink with a dashed edge; the X-ray line names the GOES bucket peak it drew from and
// its own time, or says there is no reading for that time (and nothing is drawn for the flare); the
// footprint is always "estimated from GOES" (NOAA's grid is not kept for past times); and one line
// says everything else on the map is live. "Back to now" ends it. Nothing about a replay is saved.

import { t } from '../../i18n'
import { flareClass } from '../../mapGeo'
import { wxStampLabel } from '../../propViz'
import { REPLAY_STEP_SECS, type ReplayWindow } from '../../replay'
import { agoLabel } from './SpaceWxGauges'
import { FlareSparkline } from './FlareChip'

/** `HH:MMZ`. */
function hm(unix: number): string {
  const d = new Date(unix * 1000)
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}Z`
}

export function ReplayBar({
  atMs,
  win,
  nowMs,
  series,
  flux,
  footDrawn,
  onScrub,
  onBack,
}: {
  /** The replayed time (ms), already held inside `win`. */
  atMs: number
  /** The stretch a replay may show, or null when no trace is held any more (the sky alone). */
  win: ReplayWindow | null
  nowMs: number
  /** The held six-hour trace, for the sparkline. */
  series: [number, number][]
  /** The flux at the replayed time and its own sample time, or null (no reading there). */
  flux: { flux: number; at: number } | null
  /** Whether a flare footprint is drawn for the replayed time (M1 and above). */
  footDrawn: boolean
  onScrub: (ms: number) => void
  onBack: () => void
}) {
  const at = atMs / 1000
  return (
    <div className="flare-chip replay-chip" role="status" aria-live="polite" data-replay="">
      <div className="replay-chip-head">
        {t('map.replay.head', { time: wxStampLabel(at), ago: agoLabel(Math.max(0, nowMs / 1000 - at)) })}
      </div>
      <div className="flare-chip-line" data-flux={flux ? '' : undefined}>
        {flux ? t('map.replay.flux', { cls: flareClass(flux.flux), time: hm(flux.at) }) : t('map.replay.noFlux')}
      </div>
      {footDrawn && (
        <div className="flare-chip-line flare-chip-src" data-source="goes">
          {t('map.replay.est')}
        </div>
      )}
      {series.length > 1 && <FlareSparkline series={series} phase={{ noaa: false, beginAt: null, peakAt: null }} cursorAt={at} />}
      {win && (
        <input
          type="range"
          className="replay-slider"
          min={win.loMs}
          max={win.hiMs}
          step={REPLAY_STEP_SECS * 1000}
          value={atMs}
          onChange={(e) => onScrub(Number(e.target.value))}
          aria-label={t('map.replay.slider.aria')}
          aria-valuetext={wxStampLabel(at)}
        />
      )}
      <div className="flare-chip-line flare-chip-dim">{t('map.replay.live')}</div>
      <button type="button" className="flare-preview replay-back" onClick={onBack}>
        {t('map.replay.back')}
      </button>
    </div>
  )
}

/** The Layers panel's flare-row button: starts a replay at the trace's highest reading (the moment
 *  most worth seeing again), or ends one. Disabled, saying why, while no trace is held. */
export function ReplayButton({
  on,
  win,
  startMs,
  onStart,
  onStop,
}: {
  on: boolean
  win: ReplayWindow | null
  /** Where a new replay starts (ms): `replayStart`'s answer. */
  startMs: number | null
  onStart: (ms: number) => void
  onStop: () => void
}) {
  const held = win != null && startMs != null
  return (
    <button
      type="button"
      className={`flare-preview replay-start${on ? ' active' : ''}`}
      disabled={!on && !held}
      aria-pressed={on}
      title={on ? t('map.replay.stop.title') : held ? t('map.replay.start.title') : t('map.replay.start.none')}
      onClick={() => (on ? onStop() : win != null && startMs != null && onStart(startMs))}
    >
      {on ? t('map.replay.stop') : t('map.replay.start')}
    </button>
  )
}
