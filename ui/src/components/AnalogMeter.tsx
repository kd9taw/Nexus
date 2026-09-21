// THE ANALOG METER — one instrument, two roles, the way a rig's meter works.
//
// A rig has ONE meter. It reads S-units while you listen and swings to a transmit scale when
// you key, and the scales are all printed on the face whether or not the needle is on them.
// That is the whole design here, and it is also why this replaces two things rather than adding
// a third: Phone had a 10-px S bar buried in the scope header and four TX bar rows in the dock
// that render BLANK until the first over — so on receive there was nothing meter-shaped on the
// cockpit at all, and the operator reported exactly that ("I don't see the meters").
//
// ⚠️ PHONE ONLY, DELIBERATELY. `TxMeters` has three hosts and the scope strip has two; this
// instrument is the Phone cockpit's. A ruling taken about one surface must not be implemented in
// a component the other surfaces share — that mistake put the Operate recall card 5 px past the
// fold and reddened public main on 2026-09-21. CW and Operate keep their bars until someone asks
// for them to change.
//
// ⛔ WHAT IS NOT ON THIS FACE, AND WHY. A traditional meter also prints IC (collector current)
// and VCC (supply volts). Nexus has no CAT source for either — no field, no poll, nothing — so
// they are absent rather than drawn empty. Drawing a scale Nexus cannot feed is the "confident
// wrong number" failure in its most decorative form.
//
// ⚠️ AND A NEEDLE IS A STRONGER CLAIM THAN A BAR. That is the load-bearing caution for SWR: the
// bars were fixed on 2026-09-20 to stop advising "keep it under 2:1" over a reading the engine
// itself refuses to act on (a Xiegu reads 1.2:1 on its own front panel and 6:1 here, #292). A
// calibrated arc with numbered ticks re-commits that with more authority, so an unverified SWR
// scale keeps its needle and LOSES its numbers — see `swrScaleVerified` below.
import { useId, useState } from 'react'
import type { RadioStatus } from '../types'
import { t } from '../i18n'

/** The scale names, as a rig prints them on its meter — technical tokens exactly as the mode
 *  names are, gathered here so the catalog guard reads them as the deliberate constants they
 *  are (the same shape `TxMeters` uses for its four). */
const PLATE_S = 'S'
const PLATE_PO = 'PO'
const PLATE_SWR = 'SWR'
/** ⚠️ The mark that says Nexus cannot vouch for THIS rig's SWR scale — see `readingFor`. */
const PLATE_SWR_UNSCALED = 'SWR?'
const PLATE_ALC = 'ALC'
const PLATE_COMP = 'COMP'

/** The face's own geometry. A viewBox, not pixels: the host sizes it and the drawing scales. */
const VB_W = 240
const VB_H = 148
const PIVOT_X = VB_W / 2
const PIVOT_Y = 138
/** Sweep in degrees either side of vertical. 55° each way is the traditional ~110° face — wide
 *  enough that S9 and the +dB zone are visually distinct, narrow enough that the needle's angle
 *  reads as a quantity rather than as a direction. */
const HALF_SWEEP = 55
const NEEDLE_LEN = 112

/** Radii of the printed scale arcs, outermost first. */
const R_S = 106
const R_TX = 86
const R_AUX = 66

/** A point on the face at `frac` (0..1) along the sweep, `r` from the pivot. */
export function facePoint(r: number, frac: number): { x: number; y: number } {
  const deg = -HALF_SWEEP + Math.max(0, Math.min(1, frac)) * HALF_SWEEP * 2
  const a = (deg * Math.PI) / 180
  return { x: PIVOT_X + r * Math.sin(a), y: PIVOT_Y - r * Math.cos(a) }
}

/** The `d` for a scale arc drawn across the whole sweep at radius `r`. */
function arcPath(r: number): string {
  const a = facePoint(r, 0)
  const b = facePoint(r, 1)
  return `M ${a.x.toFixed(2)} ${a.y.toFixed(2)} A ${r} ${r} 0 0 1 ${b.x.toFixed(2)} ${b.y.toFixed(2)}`
}

/**
 * S-units from a CAT reading in dB relative to S9 — S0 at −54 dB, 6 dB per unit, +60 over.
 *
 * ⚠️ The SAME mapping `PhoneScope`'s strip uses, deliberately duplicated nowhere: exported from
 * here and imported there, so the bar and the needle can never disagree about what S7 means.
 * Two readings of one number that differ by a unit is worse than either being slightly off.
 */
export function sFrac(db: number): number {
  return Math.max(0, Math.min(1, (db + 54) / 114))
}

/** The printed S scale. S9 lands at 0.474 — just under half — which is why a real face has the
 *  whole +dB red zone in its right half. */
const S_TICKS: { frac: number; label: string; hot?: boolean }[] = [
  { frac: sFrac(-48), label: '1' },
  { frac: sFrac(-36), label: '3' },
  { frac: sFrac(-24), label: '5' },
  { frac: sFrac(-12), label: '7' },
  { frac: sFrac(0), label: '9' },
  { frac: sFrac(20), label: '+20', hot: true },
  { frac: sFrac(40), label: '+40', hot: true },
  { frac: sFrac(60), label: '+60', hot: true },
]

/** Which transmit quantity the needle reads while keyed. A real meter has this switch. */
export type TxScale = 'po' | 'swr' | 'alc' | 'comp'

export interface MeterReading {
  /** 0..1 along the sweep, or null when the rig does not report this quantity. */
  frac: number | null
  /** What to print under the face. '—' when there is no reading. */
  value: string
  /** Numbered ticks for this scale, or [] when Nexus cannot stand behind the calibration. */
  ticks: { frac: number; label: string; hot?: boolean }[]
  /** The scale's own name, as a rig prints it. */
  plate: string
}

/**
 * THE ONE DECISION: what the needle is reading, and whether its scale may carry numbers.
 *
 * Exported and pure so the tests can ask it directly — the face is SVG and jsdom never lays out,
 * so the arithmetic is asserted here and only the structure is asserted on the render.
 */
export function readingFor(
  radio: RadioStatus,
  keyed: boolean,
  scale: TxScale,
  ratedW: number,
): MeterReading {
  if (!keyed) {
    const db = radio.smeterDb
    return {
      frac: db == null ? null : sFrac(db),
      value: db == null ? '—' : sLabel(db),
      ticks: S_TICKS,
      plate: PLATE_S,
    }
  }
  switch (scale) {
    case 'swr': {
      const swr = radio.txSwr
      // ⛔ ABSENT READS AS UNVERIFIED (`!== true`), the same direction the bars and Settings take.
      // Resolving a missing flag to "verified" is the one default that puts the confident wrong
      // number back on the face.
      const verified = radio.swrScaleVerified === true
      return {
        frac: swr == null ? null : Math.max(0, Math.min(1, (swr - 1) / 2)),
        value: swr == null ? '—' : `${swr.toFixed(1)}:1`,
        // The needle still MOVES on an unverified rig — you can see it rise, which is the part
        // that is true. What it must not do is claim 2.0 is at that tick.
        ticks: verified
          ? [
              { frac: 0, label: '1' },
              { frac: 0.25, label: '1.5' },
              { frac: 0.5, label: '2', hot: true },
              { frac: 1, label: '3', hot: true },
            ]
          : [],
        plate: verified ? PLATE_SWR : PLATE_SWR_UNSCALED,
      }
    }
    case 'alc': {
      const alc = radio.txAlc
      return {
        frac: alc == null ? null : Math.max(0, Math.min(1, alc)),
        value: alc == null ? '—' : `${Math.round(alc * 100)}%`,
        ticks: [
          { frac: 0, label: '0' },
          { frac: 0.5, label: '50' },
          { frac: 0.8, label: '80', hot: true },
          { frac: 1, label: '100', hot: true },
        ],
        plate: PLATE_ALC,
      }
    }
    case 'comp': {
      const db = radio.txCompDb
      return {
        frac: db == null ? null : Math.max(0, Math.min(1, db / 25)),
        value: db == null ? '—' : `${Math.round(db)} dB`,
        ticks: [
          { frac: 0, label: '0' },
          { frac: 0.4, label: '10' },
          { frac: 0.8, label: '20', hot: true },
        ],
        plate: PLATE_COMP,
      }
    }
    default: {
      const w = radio.txPoW
      // ⭐ FULL SCALE IS THE RADIO'S RATED POWER, not a hardcoded 100 (operator, 2026-09-21).
      // The bars used watts/100, so a 5 W QRP set never left the first tick and a 200 W amp
      // pinned — the two kinds of operator most likely to be watching the meter. `ratedW` is a
      // per-radio setting defaulting to 100, so it is right untouched on most HF rigs.
      const full = ratedW > 0 ? ratedW : 100
      return {
        frac: w == null ? null : Math.max(0, Math.min(1, w / full)),
        value: w == null ? '—' : `${Math.round(w)} W`,
        ticks: [
          { frac: 0, label: '0' },
          { frac: 0.25, label: `${Math.round(full / 4)}` },
          { frac: 0.5, label: `${Math.round(full / 2)}` },
          { frac: 1, label: `${full}` },
        ],
        plate: PLATE_PO,
      }
    }
  }
}

/** S-unit text for a dB reading. Never rounds UP over S9 — that would overstate a signal. */
export function sLabel(db: number): string {
  if (db >= 0) {
    const over = Math.round(db)
    return over > 0 ? `S9+${over}` : 'S9'
  }
  return `S${Math.max(0, Math.min(9, Math.round(9 + db / 6)))}`
}

/** The meter switch, with its keys spelled out.
 *
 * ⚠️ NOT `t(`meter.pick.${id}`)`. A template-literal key is invisible to the catalog scanners —
 * `hardcoded-strings.test.ts` reads every entry as an ORPHAN because no call site names it, and
 * `placeholders.test.ts` counts the call site as unreadable. A key no tool can see is a key
 * nobody can maintain, and the guard that would have told you it was missing stays quiet. */
const PICKS: { id: TxScale; label: () => string; title: () => string }[] = [
  { id: 'po', label: () => t('meter.pick.po'), title: () => t('meter.pick.po.title') },
  { id: 'swr', label: () => t('meter.pick.swr'), title: () => t('meter.pick.swr.title') },
  { id: 'alc', label: () => t('meter.pick.alc'), title: () => t('meter.pick.alc.title') },
  { id: 'comp', label: () => t('meter.pick.comp'), title: () => t('meter.pick.comp.title') },
]

export interface AnalogMeterProps {
  radio: RadioStatus
  /** Keyed right now — the arbiter's answer, not the FT slot flag. Swaps the needle's role. */
  keyed: boolean
  /** This radio's rated output in watts; full scale for PO. Defaults to 100. */
  ratedW?: number
  /** Where the operator's power cap sits on the PO scale, 0..1, or null for no cap. Drawn as a
   *  mark rather than a limit — the cap is a fraction of the rig, which is what makes it
   *  placeable on a scale whose full-scale is that same rig's rated output. */
  capFrac?: number | null
}

export function AnalogMeter({ radio, keyed, ratedW = 100, capFrac = null }: AnalogMeterProps) {
  const [scale, setScale] = useState<TxScale>('po')
  const gid = useId()
  const r = readingFor(radio, keyed, scale, ratedW)
  const needle = facePoint(NEEDLE_LEN, r.frac ?? 0)

  return (
    <div className={`ph-meter${keyed ? ' keyed' : ''}`} role="group" aria-label={t('meter.aria')}>
      <svg
        className="ph-meter-face"
        viewBox={`0 0 ${VB_W} ${VB_H}`}
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label={t('meter.face.aria', { plate: r.plate, value: r.value })}
      >
        {/* The three printed arcs. They are on the face whatever the needle reads, exactly as a
            rig prints all its scales — which is what makes the instrument legible at a glance
            instead of a number that changes meaning without telling you. */}
        <path className="ph-meter-arc" d={arcPath(R_S)} />
        <path className="ph-meter-arc" d={arcPath(R_TX)} />
        <path className="ph-meter-arc ph-meter-arc--aux" d={arcPath(R_AUX)} />

        {/* The live scale's ticks and numbers, on the arc that scale owns. */}
        {r.ticks.map((tk) => {
          const outer = facePoint(keyed ? R_TX : R_S, tk.frac)
          const inner = facePoint((keyed ? R_TX : R_S) - 8, tk.frac)
          const text = facePoint((keyed ? R_TX : R_S) - 19, tk.frac)
          return (
            <g key={`${gid}-${tk.label}`} className={tk.hot ? 'ph-meter-tick hot' : 'ph-meter-tick'}>
              <line x1={outer.x} y1={outer.y} x2={inner.x} y2={inner.y} />
              <text x={text.x} y={text.y} textAnchor="middle" dominantBaseline="middle">
                {tk.label}
              </text>
            </g>
          )
        })}

        {/* The power cap, when there is one: a mark on the scale saying where your own ceiling
            sits. Only meaningful on PO, because the cap is a fraction of this rig's output. */}
        {keyed && scale === 'po' && capFrac != null && (
          <line
            className="ph-meter-cap"
            data-testid="meter-cap"
            x1={facePoint(R_TX + 5, capFrac).x}
            y1={facePoint(R_TX + 5, capFrac).y}
            x2={facePoint(R_TX - 12, capFrac).x}
            y2={facePoint(R_TX - 12, capFrac).y}
          />
        )}

        {/* The needle. Absent — not parked at zero — when the rig reports nothing: a needle at
            rest on the left is a READING of "no signal", and this is "no answer". */}
        {r.frac != null && (
          <line
            className="ph-meter-needle"
            data-testid="meter-needle"
            x1={PIVOT_X}
            y1={PIVOT_Y}
            x2={needle.x}
            y2={needle.y}
          />
        )}
        <circle className="ph-meter-hub" cx={PIVOT_X} cy={PIVOT_Y} r={5} />

        {/* The scale's own name, printed on the face where a rig prints it. */}
        <text className="ph-meter-plate" x={16} y={26}>
          {r.plate}
        </text>
      </svg>

      <div className="ph-meter-foot">
        <span className="ph-meter-value" data-testid="meter-value">
          {r.value}
        </span>
        {/* The meter switch. A rig has one; without it a single needle cannot show four transmit
            quantities. Receive has nothing to choose — S is the only thing a receiver meters —
            so the switch is disabled rather than hidden, which keeps the foot from changing
            height on every key-down (the dock is bottom-anchored above the PTT button). */}
        <div className="ph-meter-pick" role="group" aria-label={t('meter.pick.aria')}>
          {PICKS.map(({ id, label, title }) => (
            <button
              key={id}
              type="button"
              disabled={!keyed}
              aria-pressed={keyed && scale === id}
              className={`ph-meter-pick-btn${keyed && scale === id ? ' on' : ''}`}
              title={title()}
              onClick={() => setScale(id)}
            >
              {label()}
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}
