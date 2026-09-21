// THE S-METER — a segmented horizontal scale, in the receive chain where it belongs.
//
// This replaces an arc-and-needle face that was tried first and rejected on sight: centred in a
// wide region with nothing beside it, it read as an ornament rather than an instrument. The
// lesson is placement more than shape — a meter belongs WITH the thing it measures, and the
// receive chain pane already holds BW, RF, NB, NR, notch, AGC, AF and squelch. Put among them
// it has company, it inherits their visual language, and the waterfall keeps its height, which
// the arc was spending ~150 px of.
//
// It is deliberately the SAME SHAPE as `TxMeters`' rows — label, track, value — because the
// cockpit should have ONE family of meters rather than one per author. What it adds is the
// printed scale, which the transmit bars do not have and an S-meter cannot do without: "S7"
// means nothing next to a bar unless the operator can see where S7 falls.
//
// ⚠️ PHONE ONLY. `TxMeters` has three hosts and `PhoneScope` has two; this is Phone's. A ruling
// taken about one surface must not be implemented in a component the others share — see
// `feedback-a-host-ruling-is-not-a-component-ruling`, and the Operate recall card that went 5 px
// past the fold when that was forgotten.
import type { RadioStatus } from '../types'
import { t } from '../i18n'

/** The scale's own name, as a rig prints it — a technical token, like `TxMeters`' four. */
const PLATE_S = 'S'

/**
 * S-units from a CAT reading in dB relative to S9 — S0 at −54 dB, 6 dB per unit, +60 over.
 *
 * ⚠️ THE SAME MAPPING `PhoneScope`'s strip uses. Exported so there is one of it: two readings of
 * one number that differ by a unit is worse than either being slightly off.
 */
export function sFrac(db: number): number {
  return Math.max(0, Math.min(1, (db + 54) / 114))
}

/** S-unit text for a dB reading. Never rounds UP over S9 — that would overstate a signal. */
export function sLabel(db: number): string {
  if (db >= 0) {
    const over = Math.round(db)
    return over > 0 ? `S9+${over}` : 'S9'
  }
  return `S${Math.max(0, Math.min(9, Math.round(9 + db / 6)))}`
}

/** Where S9 falls on the scale. Everything above it is the top band, and it is just under
 *  half — which is why a real face gives the whole right half to +dB. */
export const S9_FRAC = sFrac(0)

/** The printed scale. Odd S-units then the over-S9 marks, exactly as a rig prints them. */
const TICKS: { frac: number; label: string; hot?: boolean }[] = [
  { frac: sFrac(-48), label: '1' },
  { frac: sFrac(-36), label: '3' },
  { frac: sFrac(-24), label: '5' },
  { frac: sFrac(-12), label: '7' },
  { frac: sFrac(0), label: '9' },
  { frac: sFrac(20), label: '+20', hot: true },
  { frac: sFrac(40), label: '+40', hot: true },
  { frac: sFrac(60), label: '+60', hot: true },
]

/** Segment count. Enough that the bar reads as a level rather than a row of blocks, few enough
 *  that each is wide enough to see at operating distance in a pane this narrow. */
const SEGMENTS = 28

export interface SMeterProps {
  radio: RadioStatus
}

export function SMeter({ radio }: SMeterProps) {
  const db = radio.smeterDb
  // ⚠️ PAUSED, NOT ZEROED, while keyed. These fields are receive-side; a rig stops reporting
  // them on transmit, and a bar falling to the floor every time the operator keys would read
  // as "the signal went away" rather than "we are not listening".
  const keyed =
    radio.transmitting || radio.txBusyReason != null || radio.rigKeyed === true
  const live = db != null && !keyed
  const frac = live ? sFrac(db) : null

  return (
    <div
      className="ph-smeter"
      role="group"
      aria-label={t('scope.smeter.aria')}
      title={
        live
          ? t('scope.smeter.title', { reading: sLabel(db), db: Math.round(db) })
          : keyed
            ? t('scope.smeter.title.tx')
            : t('scope.smeter.title.none')
      }
    >
      <div className="ph-txmeter">
        <span className="ph-txmeter-label">{PLATE_S}</span>
        <div className="ph-smeter-track" data-testid="smeter-track">
          {Array.from({ length: SEGMENTS }, (_, i) => {
            // A segment lights when the reading reaches ITS point on the scale, so the last lit
            // segment IS the reading rather than the one after it.
            const at = (i + 0.5) / SEGMENTS
            const lit = frac != null && at <= frac
            return (
              <span
                key={i}
                className={`ph-smeter-seg${lit ? ' lit' : ''}${at > S9_FRAC ? ' hot' : ''}`}
              />
            )
          })}
        </div>
        {/* '—' when the rig does not report it, never 'S0': absent and "no signal" are
            different facts, and only one of them is about the radio. */}
        <span className="ph-txmeter-value" data-testid="smeter-value">
          {live ? sLabel(db) : '—'}
        </span>
      </div>
      {/* The printed scale. Positioned by the SAME `sFrac` that drives the segments, so a tick
          and the bar under it can never disagree about where S9 is. */}
      <div className="ph-smeter-scale" aria-hidden="true">
        {TICKS.map((tk) => (
          <span
            key={tk.label}
            className={tk.hot ? 'ph-smeter-tick hot' : 'ph-smeter-tick'}
            style={{ left: `${tk.frac * 100}%` }}
          >
            {tk.label}
          </span>
        ))}
      </div>
    </div>
  )
}
