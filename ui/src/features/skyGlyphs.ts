// The sun as a map marker — ONE drawing for the 2-D map's canvas and the 3-D globe's sprite, so the
// two surfaces show the same glyph (the features/satIcon habit: a second hand-tuned copy drifts on
// the first tweak). Plain canvas calls, no state; the caller sets globalAlpha.
//
// The glyph has to read as the SUN and not as one more spot: the 15 m and 12 m spot dots are a
// yellow and an orange disc of about the same size, so the sun carries rays and a glow of its own
// hue, and a dark outline (the map's marker halo) that holds it apart from land, sea and night.

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
