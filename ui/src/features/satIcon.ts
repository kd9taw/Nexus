/** The satellite glyph — ONE shape definition, two renderers.
 *
 * The world map paints birds onto a canvas and the sky dome draws one in SVG,
 * so the drawing code genuinely cannot be shared. What must not diverge is the
 * OBJECT: two hand-tuned copies drift apart on the first tweak, and then the
 * operator has learned two different marks for the same spacecraft.
 *
 * The rects below are that object — a body between two solar panels, in glyph
 * units, centred on the origin — drawn tilted so it reads as a spacecraft and
 * not as a box. Consumers scale and place them; nobody re-types them.
 */

/** Tilt applied to the whole glyph, degrees clockwise. */
export const SAT_ICON_TILT_DEG = 45

export interface SatIconRect {
  x: number
  y: number
  w: number
  h: number
}

/** Body, then the two panels. Origin-centred, untilted, scale 1. */
export const SAT_ICON_RECTS: readonly SatIconRect[] = [
  { x: -2.4, y: -2.4, w: 4.8, h: 4.8 }, // body
  { x: -8, y: -1.4, w: 4.4, h: 2.8 }, // left panel
  { x: 3.6, y: -1.4, w: 4.4, h: 2.8 }, // right panel
]
