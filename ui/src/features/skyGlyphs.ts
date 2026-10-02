// The sun and the moon as map markers — ONE drawing each for the 2-D map's canvas and the 3-D globe's
// sprites, so the two surfaces show the same glyphs (the features/satIcon habit: a second hand-tuned
// copy drifts on the first tweak). Plain canvas calls, no state; the caller sets globalAlpha.
//
// The sun has to read as the SUN and not as one more spot: the 15 m and 12 m spot dots are a yellow
// and an orange disc of about the same size, so the sun carries rays and a glow of its own hue, and a
// dark outline (the map's marker halo) that holds it apart from land, sea and night. The moon is
// told apart by its phase: a dark disc, lit as much as the real one is.

/** The two discs' radii on the 2-D map at marker scale 1 (MapView's `markerScaleFor`), in layout px;
 *  the 3-D globe keeps their proportion. The moon a little larger, so a thin crescent still shows. */
export const SUN_DISC = 5.5
export const MOON_DISC = 6.5

/** `#rrggbb` at `alpha` — a gradient stop in the ink's own hue. The sky tokens are plain hexes
 *  (styles.css MAP SKY); anything else paints nothing rather than a guessed colour. */
function inkAt(hex: string, alpha: number): string {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return 'rgba(0, 0, 0, 0)'
  const n = parseInt(m[1], 16)
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`
}

/** How far the sun glyph reaches from its centre, in disc radii — its glow's edge. */
export const SUN_REACH = 2.4

/**
 * The sun, centred on (`x`, `y`): a disc of radius `r` in `ink`, eight short rays and a soft glow of
 * the same hue, outlined in `halo`. Everything it draws lies within `SUN_REACH · r` of the centre.
 */
export function drawSun(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  r: number,
  ink: string,
  halo: string,
): void {
  const glow = ctx.createRadialGradient(x, y, r * 0.6, x, y, r * SUN_REACH)
  glow.addColorStop(0, inkAt(ink, 0.45))
  glow.addColorStop(1, inkAt(ink, 0))
  ctx.fillStyle = glow
  ctx.beginPath()
  ctx.arc(x, y, r * SUN_REACH, 0, Math.PI * 2)
  ctx.fill()

  ctx.beginPath()
  for (let i = 0; i < 8; i++) {
    const a = (i * Math.PI) / 4
    ctx.moveTo(x + Math.cos(a) * r * 1.4, y + Math.sin(a) * r * 1.4)
    ctx.lineTo(x + Math.cos(a) * r * 2, y + Math.sin(a) * r * 2)
  }
  // The halo UNDER each ray (drawn wider first), so a ray over pale land still has an edge.
  const ray = Math.max(1, r * 0.3)
  ctx.lineCap = 'round'
  ctx.strokeStyle = halo
  ctx.lineWidth = ray + 1.5
  ctx.stroke()
  ctx.strokeStyle = ink
  ctx.lineWidth = ray
  ctx.stroke()
  ctx.lineCap = 'butt'

  ctx.beginPath()
  ctx.arc(x, y, r, 0, Math.PI * 2)
  ctx.fillStyle = ink
  ctx.fill()
  ctx.strokeStyle = halo
  ctx.lineWidth = Math.max(1, r * 0.25)
  ctx.stroke()
}

/**
 * The moon, centred on (`x`, `y`), radius `r`, lit `illuminated` (0 new … 1 full): its whole disc in
 * `inks.dark`, then the lit part in `inks.lit`, bounded by the lit limb and the terminator — an
 * ellipse `r·|2k − 1|` wide that bows toward the lit limb while it is a crescent and away from it
 * once it is gibbous. `litRight` puts the lit limb on the right, as a waxing moon looks from the
 * northern hemisphere. A dark halo just outside the rim and a thin line of moonlight on it keep even
 * a new moon findable on a dark sea.
 */
export function drawMoon(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  r: number,
  illuminated: number,
  litRight: boolean,
  inks: { lit: string; dark: string },
  halo: string,
): void {
  ctx.beginPath()
  ctx.arc(x, y, r, 0, Math.PI * 2)
  ctx.fillStyle = inks.dark
  ctx.fill()

  const k = Math.max(0, Math.min(1, illuminated))
  if (k > 0.005) {
    ctx.save()
    ctx.translate(x, y)
    if (!litRight) ctx.scale(-1, 1)
    ctx.beginPath()
    // Down the lit limb, top to bottom through the right, and back up the terminator: through the
    // right (anticlockwise) while it is a crescent, through the left once it is gibbous.
    ctx.arc(0, 0, r, -Math.PI / 2, Math.PI / 2)
    const a = r * Math.abs(2 * k - 1)
    if (k < 0.5) ctx.ellipse(0, 0, a, r, 0, Math.PI / 2, -Math.PI / 2, true)
    else ctx.ellipse(0, 0, a, r, 0, Math.PI / 2, (3 * Math.PI) / 2, false)
    ctx.fillStyle = inks.lit
    ctx.fill()
    ctx.restore()
  }

  // The halo wholly outside the disc: on the rim it would eat a thin crescent.
  const h = Math.max(1.5, r * 0.25)
  ctx.beginPath()
  ctx.arc(x, y, r + h / 2, 0, Math.PI * 2)
  ctx.strokeStyle = halo
  ctx.lineWidth = h
  ctx.stroke()
  ctx.beginPath()
  ctx.arc(x, y, r, 0, Math.PI * 2)
  ctx.strokeStyle = inkAt(inks.lit, 0.55)
  ctx.lineWidth = Math.max(0.75, r * 0.1)
  ctx.stroke()
}
