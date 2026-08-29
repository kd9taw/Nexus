// THE CW ZERO-BEAT INDICATOR.
//
// Operator, 2026-08-28: "I still think you should include some sort of tuning mechanism for
// when you have found the right audio freq for tuning CW. Like a light that comes on when you
// are zero beat to the CW signal." Approved as the light PLUS a direction cue — a dark light
// reads identically at 80 Hz off and at 400 Hz off, so a bare lamp confirms arrival without
// ever guiding you in.
//
// WHAT IT SHOWS. The scope beside it already draws the TARGET (the dashed hairline at your CW
// pitch); this is the other half — where the signal actually IS. The needle runs in the
// SCOPE'S OWN AXIS (low audio left, high audio right, whatever the sideband), so "centre the
// needle" and "put the peak on the hairline" are the same instruction and can never disagree.
// That is also why there is no "tune up / tune down" wording: which way the DIAL goes depends
// on CW-U versus CW-L, and a cue that inferred the rig's current sideband wrongly would send
// the operator the wrong way. The needle and the picture cannot be wrong together.
//
// ⛔ IT DISPLAYS AND NEVER ACTS. There is no auto-zero-beat, no VFO move, nothing keyed, and
// no path from this component to a CAT command. Notify, never act.
//
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts). The offset in Hz
// is a measurement this component formats invariantly and `Hz` is a unit symbol, exactly as
// `dB` is in LiveMeters — those stay in the code; every word is in the catalog under
// `cw.zeroBeat.*`.
import { useCwToneHz } from './LiveMeters'
import { ZERO_BEAT_RANGE_HZ, zeroBeatToleranceHz } from '../waterfall'
import { t } from '../i18n'

/** The unit symbol on the offset readout — a unit, not a word. */
const HZ = 'Hz'

/** Needle travel is inset from the ends so a pinned reading still reads as a needle and not
 * as the bar's own border. */
const NEEDLE_MIN_PCT = 2
const NEEDLE_MAX_PCT = 98

interface Props {
  /** The operator's CW pitch (Hz) — the zero-beat target, and the same value the scope
   * draws its marker at. */
  targetHz: number
  /** The rig's CW filter width (Hz); sets how close counts as zero beat. See
   * [`zeroBeatToleranceHz`]. */
  filterHz?: number | null
  /** False in a kept-alive hidden host — the widget then costs no poll at all. */
  active?: boolean
}

export function ZeroBeat({ targetHz, filterHz, active = true }: Props) {
  const toneHz = useCwToneHz(active)
  const tol = zeroBeatToleranceHz(filterHz)
  // `err` is the AUDIO offset: the received tone minus your pitch. Positive = the signal sits
  // above your pitch, i.e. right of the marker on the scope.
  const err = toneHz === null ? null : toneHz - targetHz
  const onFreq = err !== null && Math.abs(err) <= tol
  const pct =
    err === null
      ? 50
      : Math.min(
          NEEDLE_MAX_PCT,
          Math.max(NEEDLE_MIN_PCT, 50 + (err / ZERO_BEAT_RANGE_HZ) * 50),
        )

  // The readout. NO SIGNAL IS ITS OWN STATE and says so in words — a stale or invented zero on
  // a dead band is worse than a blank one, and it is the failure the operator would never see
  // coming (a confident needle at centre looks exactly like being perfectly tuned).
  const reading =
    err === null
      ? t('cw.zeroBeat.none')
      : onFreq
        ? t('cw.zeroBeat.locked')
        : `${err > 0 ? '+' : '-'}${Math.abs(Math.round(err))} ${HZ}`

  return (
    <span
      className={`zb${onFreq ? ' on' : ''}${err === null ? ' idle' : ''}`}
      role="group"
      aria-label={t('cw.zeroBeat.aria')}
      title={t('cw.zeroBeat.title', { tol: Math.round(tol) })}
    >
      <span className="zb-name">{t('cw.zeroBeat.label')}</span>
      <span className="zb-lamp" aria-hidden="true" />
      {/* The bar is decoration for the number beside it — the reading itself is text, in the
          DOM, and a screen reader gets it there. Deliberately NOT an aria-live region: this
          updates ten times a second and would narrate a new number every 100 ms. */}
      <span className="zb-bar" aria-hidden="true">
        <span className="zb-centre" />
        {err !== null && <span className="zb-needle" style={{ left: `${pct}%` }} />}
      </span>
      <span className="zb-read">{reading}</span>
    </span>
  )
}
