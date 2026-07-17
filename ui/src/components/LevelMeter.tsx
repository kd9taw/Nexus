interface Props {
  /** Backend RX RMS, normalized 0–1. Rendered as a dB level. */
  value: number
  /** Accessible label / tooltip prefix. */
  label?: string
  /** compact = thin inline bar (TopBar); full = taller bar with ticks. */
  variant?: 'compact' | 'full'
}

// Full-scale dB for the meter's top. Matches WSJT-X's scale (0..~90).
const DB_MAX = 90

/**
 * Convert the backend's normalized RMS (0–1) to a WSJT-X-style dB reading:
 * dB = 20·log10(rms) + 90.3, which mirrors WSJT-X's 10·log10(Σx²/N) on int16
 * audio. A healthy FT8 input reads ~30 dB, full scale ~90 dB — so the number is
 * directly comparable to WSJT-X's meter (aim for ~30; decodes fine ~15–60).
 */
export function rxLevelDb(value: number): number {
  const v = Number.isFinite(value) ? value : 0
  if (v <= 0) return 0
  return Math.max(0, Math.min(DB_MAX, 20 * Math.log10(v) + 90.3))
}

// Zone thresholds in dB: below ~15 is too quiet (raise RX Gain), ~15–70 decodes
// fine (aim ~30), above ~70 is too hot (back off gain / rig audio).
function zone(db: number): 'low' | 'good' | 'hot' {
  if (db >= 70) return 'hot'
  if (db >= 15) return 'good'
  return 'low'
}

/**
 * Horizontal RX audio level meter on a WSJT-X-style dB scale. The fill colour
 * follows the zone (low / good / hot) so it's readable at a glance.
 */
export function LevelMeter({ value, label = 'RX level', variant = 'compact' }: Props) {
  const db = rxLevelDb(value)
  const pct = Math.round((db / DB_MAX) * 100)
  const z = zone(db)
  const dbLabel = `${Math.round(db)} dB`
  return (
    <div
      className={`level-meter ${variant} ${z}`}
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={DB_MAX}
      aria-valuenow={Math.round(db)}
      aria-valuetext={dbLabel}
      title={`${label}: ${dbLabel} (aim ~30 dB, like WSJT-X)`}
    >
      <div className="level-fill" style={{ width: `${pct}%` }} />
      {/* target marker at ~30 dB — the WSJT-X sweet spot */}
      <span className="level-target" aria-hidden />
    </div>
  )
}
