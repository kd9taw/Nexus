// #480: THE LISTENING LEVEL'S BOOST. `monitorLevel` is a plain gain on the receive audio Nexus plays
// on this computer: 0..1 is what it always was (a level saved before the boost loads unchanged), and
// above 1 is up to +20 dB of boost, for a radio whose USB audio is set quiet for the decoders and
// does not follow the radio's volume knob. The backend bounds it at the same top
// (tempo-audio's `MONITOR_LEVEL_MAX`) and rounds boosted peaks off instead of clipping them. Only what
// the operator hears changes: the decoders read the capture, not this.
//
// THE SLIDER IS TWO HALVES, so neither range is crushed into a sliver of the other. The left half,
// 0..1, is the level itself, as it always was; the right half, 1..2, is the boost in decibels,
// 0..+20 dB evenly. A plain 0..10 gain slider would put the whole old range in its first tenth.

/** The top of the level: a gain of 10, +20 dB. */
export const LEVEL_MAX = 10
/** The slider's top: the level half, then the boost half. */
export const SLIDER_MAX = 2
/** The boost the right half spans, in dB. */
const BOOST_DB = 20

/** A saved level → the slider's position. Out-of-range and non-finite saved values are clamped. */
export function levelToSlider(level: number): number {
  const l = Number.isFinite(level) ? Math.min(Math.max(level, 0), LEVEL_MAX) : 0
  return l <= 1 ? l : 1 + (20 * Math.log10(l)) / BOOST_DB
}

/** The slider's position → the level to save (rounded, so a drag saves a tidy number). */
export function sliderToLevel(pos: number): number {
  const p = Math.min(Math.max(pos, 0), SLIDER_MAX)
  if (p <= 1) return Math.round(p * 100) / 100
  return Math.round(10 ** (((p - 1) * BOOST_DB) / 20) * 1000) / 1000
}

/** The boost a level gives, in whole dB; 0 at or below unity. */
export function boostDb(level: number): number {
  return level > 1 ? Math.round(20 * Math.log10(Math.min(level, LEVEL_MAX))) : 0
}

/** The level as a percentage of unity, for a level at or below it. */
export function levelPct(level: number): number {
  return Math.round(Math.min(Math.max(level, 0), 1) * 100)
}

/** The level's readout: "50%" at or below unity, "+12 dB" of boost above it (invariant units). */
export function levelText(level: number): string {
  const db = boostDb(level)
  return db > 0 ? `+${db} dB` : `${levelPct(level)}%`
}
