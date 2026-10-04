// THE SCOPE'S OVERLAYS — spot tags, the licence-class band edges and, on the RF scope pane, the FT
// decodes and the RX/TX offsets. The pure half: where each sits on the axis the scope draws, and the
// drawing itself on the overlays' own canvas (`.ph-scope-overlays`, PhoneScope). No React.
//
// DATA-ONLY LAYERS ON THEIR OWN CANVAS. The picture (trace, waterfall, 3D) is the spectrum renderer's,
// and the marks (dial, passband, notch, scale) are PhoneScope's overlay, redrawn on every poll. These
// are drawn again only when their data or the axis under them moves (PhoneScope's overlay key), so a
// new spot, a fresh decode or a privilege edge redraws no waterfall.
//
// ⚠️ THE BAND EDGES SHOW THE TRANSMIT GATE; THEY ARE NOT IT. The spans are the gate's own table
// (`get_privilege_spans` → `privileges::allowed_spans`, held to `tx_allowed` by its own test), and the
// tint marks the frequencies the operator's class may not EMIT the live section's signal on. Whether
// the dial may key is still the gate's question, answered by 🔒 TX-LOCKED: an emission is wider than
// its carrier, so a phone passband that crosses an edge is locked although its dial sits inside. Nothing
// here gates anything, and `Open` (no US table) is tinted nowhere because its gate refuses nowhere.
//
// SPOT TAGS ARE BANDMAP'S: its set (the scope's mode on its band), its fade, its mark and its collision
// rules — the freshest that fit, pushed forward then compressed back, each tick at its true frequency
// and the rest counted — laid along a horizontal axis, where a label has a width of its own. A click is
// BandMap's action too (the host's `onWorkSpot`); PhoneScope decides when one is offered.
import type { DecodeRow, SpotRow } from '../types'
import { isSymmetricMode, sidebandSign } from '../waterfall'
import type { AxisKind } from './markers'

// ---- Where an RF frequency sits on the drawn axis --------------------------------------------------
//
// The receiver markers' axis model (`markers.ts` `AxisKind`, the notch's mapping): a native RF row is
// absolute RF; Phone's carrier-centred audio axis is the offset from the dial; CW's audio window hears a
// signal ON the dial at the pitch (true CW), while the soundcard keyer and plain SSB audio hear the
// carrier at 0. An AM or FM baseband has no RF mapping (its audio is both sides at once), and an audio
// axis needs the dial.

/** Where `rfHz` sits on the drawn axis, or null where the axis has no honest place for it. */
export function rfOnAxis(a: AxisKind, rfHz: number): number | null {
  if (!Number.isFinite(rfHz)) return null
  if (a.rf) return rfHz
  const dial = a.dialHz
  if (dial == null || !(dial > 0)) return null
  const m = a.sideband.trim().toUpperCase()
  if (isSymmetricMode(m)) return null
  if (a.carrierCentered) return rfHz - dial
  return audioZero(a, m) + sidebandSign(a.sideband) * (rfHz - dial)
}

/** The RF frequency at axis point `axisHz`: `rfOnAxis` inverted. */
export function axisToRf(a: AxisKind, axisHz: number): number | null {
  if (!Number.isFinite(axisHz)) return null
  if (a.rf) return axisHz
  const dial = a.dialHz
  if (dial == null || !(dial > 0)) return null
  const m = a.sideband.trim().toUpperCase()
  if (isSymmetricMode(m)) return null
  if (a.carrierCentered) return dial + axisHz
  return dial + sidebandSign(a.sideband) * (axisHz - audioZero(a, m))
}

/** The audio frequency a signal ON the dial is heard at: the pitch in true CW, 0 otherwise. */
const audioZero = (a: AxisKind, mode: string) => (mode.startsWith('CW') && a.cwPitchRefDial ? a.pitchHz : 0)

// ---- Spot tags: BandMap's set, fade and mark --------------------------------------------------------

/** BandMap's age fade: a spot of unknown age (−1) reads fresh; otherwise it fades to 40 % over 30 min. */
export function spotAlpha(ageSecs: number): number {
  return ageSecs < 0 ? 0.95 : Math.max(0.4, 1 - ageSecs / 1800)
}

/** BandMap's order of freshness for its cap: an unknown age counts as the freshest. */
export function spotFreshness(ageSecs: number): number {
  return ageSecs < 0 ? 0 : ageSecs
}

/** A spot as a scope tags it: the row, and the colour token of its mark (null = none). The hosts build
 *  these (`scopeSpots.ts`), so the scope itself never loads the need palette. */
export interface ScopeSpot {
  spot: SpotRow
  ink: string | null
}

// ---- BandMap's collision rules, along a horizontal axis --------------------------------------------

/** A label to place: its tick's true position, its width, and its rank (lower = kept first). */
export interface TagIn {
  x: number
  w: number
  rank: number
}

/**
 * Where each label goes, BandMap's way, in one dimension. More labels than fit side by side would
 * pile into unclickable overlaps, so the set is capped to the FRESHEST (lowest rank) that fit, and the
 * rest are counted; a fresh label that does not fit stops the cap there, so a staler one never takes
 * its place. The kept labels, in frequency order, start centred on their ticks inside the axis; a
 * forward pass pushes each clear of the one before, and if the last ran off the far end a backward
 * pass compresses them back. The ticks stay at their true frequencies; only the labels move.
 */
export function layoutTags(
  tags: readonly TagIn[],
  widthPx: number,
  gapPx: number,
): { placed: { i: number; left: number }[]; hidden: number } {
  const order = tags.map((_, i) => i).sort((a, b) => tags[a].rank - tags[b].rank || tags[a].x - tags[b].x)
  const kept: number[] = []
  let used = 0
  for (const i of order) {
    const need = used + (kept.length > 0 ? gapPx : 0) + tags[i].w
    if (need > widthPx) break
    kept.push(i)
    used = need
  }
  kept.sort((a, b) => tags[a].x - tags[b].x || a - b)
  const left = kept.map((i) => Math.min(Math.max(0, tags[i].x - tags[i].w / 2), widthPx - tags[i].w))
  for (let k = 1; k < kept.length; k++) left[k] = Math.max(left[k], left[k - 1] + tags[kept[k - 1]].w + gapPx)
  const n = kept.length
  if (n > 0 && left[n - 1] + tags[kept[n - 1]].w > widthPx) {
    left[n - 1] = widthPx - tags[kept[n - 1]].w
    for (let k = n - 2; k >= 0; k--) left[k] = Math.max(0, Math.min(left[k], left[k + 1] - gapPx - tags[kept[k]].w))
  }
  return { placed: kept.map((i, k) => ({ i, left: left[k] })), hidden: tags.length - n }
}

// ---- The licence-class band edges -------------------------------------------------------------------

/** The licence-class spans a scope tints from (`api.ts` `PrivilegeSpans`). */
export interface OverlaySpans {
  /** No edges: the gate allows every frequency (`Open`). */
  unrestricted: boolean
  /** `[lo, hi)` MHz where the class may transmit the section's emission, ascending. */
  spans: readonly (readonly [number, number])[]
}

/** The parts of `[loHz, hiHz]` (RF Hz) outside every span: where the class may not transmit. The spans
 *  are half-open like the gate's, and in whole hertz like the table they come from. */
export function outsideSpans(
  spansMhz: readonly (readonly [number, number])[],
  loHz: number,
  hiHz: number,
): [number, number][] {
  const out: [number, number][] = []
  if (!(hiHz > loHz)) return out
  const hz = spansMhz.map(([a, b]) => [Math.round(a * 1e6), Math.round(b * 1e6)] as const).sort((p, q) => p[0] - q[0])
  let at = loHz
  for (const [a, b] of hz) {
    if (b <= at) continue
    if (a >= hiHz) break
    if (a > at) out.push([at, a])
    at = b
    if (at >= hiHz) break
  }
  if (at < hiHz) out.push([at, hiHz])
  return out
}

// ---- The FT cockpit on the RF scope -----------------------------------------------------------------

/** Which side of the dial a data signal sits on: below for LSB, above otherwise — the transmit gate's
 *  digital emission model (`Engine::emission_allowed`, `Engine::digital_emission_allowed`). */
export function dataSide(sideband: string): 1 | -1 {
  const m = sideband.trim().toUpperCase()
  return m === 'LSB' || m === 'PKTLSB' ? -1 : 1
}

/** The RF frequency of an audio offset: dial ± offset. */
export function rfOfOffset(dialHz: number, offsetHz: number, side: 1 | -1): number {
  return dialHz + side * offsetHz
}

/** A decode as the RF scope tags it: the sender, its audio offset, its SNR, and whether it calls us. */
export interface DecodeTag {
  call: string
  hz: number
  snr: number
  me: boolean
}

/** What the FT cockpit draws on its RF scope: the side of the dial, the RX and TX audio offsets, and
 *  the newest slot's decodes. */
export interface FtOverlay {
  side: 1 | -1
  rxHz: number
  txHz: number
  decodes: DecodeTag[]
}

/** The FT cockpit's overlay from its snapshot: the FT8 and FT4 decodes that name a sender, never our
 *  own transmission. */
export function ftOverlay(decodes: readonly DecodeRow[], rxHz: number, txHz: number, sideband: string): FtOverlay {
  return {
    side: dataSide(sideband),
    rxHz,
    txHz,
    decodes: decodes
      .filter((d) => (d.tier === 'FT8' || d.tier === 'FT4') && !d.mine && d.from)
      .map((d) => ({ call: d.from!, hz: d.freqHz, snr: d.snr, me: d.directedToMe })),
  }
}

// ---- The drawing ------------------------------------------------------------------------------------

/** The row the scope's DIAL and SUB plates take at the top, above the tag lane (CSS px). */
const PLATE_ROW = 14
/** A tag: its height, font size, side padding, and the gap between two labels (CSS px). */
const TAG_H = 13
const TAG_FONT = 10
const TAG_PAD = 4
const TAG_GAP = 4
/** The need colour's bar on a label's leading edge: BandMap's border (CSS px). */
const ACCENT = 3
/** The tick under the lane, at a tag's true frequency (CSS px). */
const TICK = 6
const TINT_ALPHA = 0.16
const EDGE_ALPHA = 0.6
/** The offset lines' plates: WSJT-X's own words, as the FT waterfall draws them. */
const RX_PLATE = 'RX'
const TX_PLATE = 'TX'

/** What the overlays paint with, read off their own canvas, which is a display well: the scope's floor
 *  is dark in both themes, so the marks on it take the dark palette (styles.css DISPLAY WELLS). */
export interface OverlayInks {
  ink: string
  ground: string
  /** The out-of-privilege tint: the TX-locked red (`--state-weak`, BandMap's blocked dial). */
  blocked: string
  rx: string
  tx: string
  /** A named token (`--need-entity`…), read once. */
  color: (token: string) => string
}

export function readOverlayInks(el: Element): OverlayInks {
  const style = getComputedStyle(el)
  const read = (name: string) => style.getPropertyValue(name).trim()
  const named = new Map<string, string>()
  return {
    ink: read('--well-ink'),
    ground: read('--well-bg'),
    blocked: read('--state-weak'),
    rx: read('--rx'),
    tx: read('--tx'),
    color: (token) => {
      if (!named.has(token)) named.set(token, read(token))
      return named.get(token)!
    },
  }
}

/** One frame of the overlays, in device px over the drawn axis `[lo, hi]`. */
export interface OverlayScene {
  w: number
  h: number
  /** Device px per CSS px of overlay text (PhoneScope's `scaleY × textScale`), and per line. */
  textPx: number
  lineW: number
  lo: number
  hi: number
  axis: AxisKind
  spans: OverlaySpans | null
  spots: readonly ScopeSpot[]
  ft: FtOverlay | null
  transmitting: boolean
}

/** A spot's label as a click target, in device px. */
export interface OverlayHit {
  l: number
  r: number
  t: number
  b: number
  spot: SpotRow
}

/** Draw the overlays on a cleared canvas: the band-edge tint, the FT offsets, then the tags. Returns
 *  each spot label's box (the click targets) and how many tags were drawn and counted out. */
export function drawOverlays(
  ctx: CanvasRenderingContext2D,
  s: OverlayScene,
  inks: OverlayInks,
): { hits: OverlayHit[]; tags: number; hidden: number } {
  const hits: OverlayHit[] = []
  const span = s.hi - s.lo
  if (!(span > 0) || !(s.w > 0) || !(s.h > 0)) return { hits, tags: 0, hidden: 0 }
  const xOf = (axisHz: number) => ((axisHz - s.lo) / span) * s.w
  const xRf = (rfHz: number): number | null => {
    const ax = rfOnAxis(s.axis, rfHz)
    return ax == null ? null : xOf(ax)
  }

  // ---- The band edges, under everything: the out-of-privilege tint, and a line at each edge ----
  if (s.spans && !s.spans.unrestricted) {
    const a = axisToRf(s.axis, s.lo)
    const b = axisToRf(s.axis, s.hi)
    if (a != null && b != null) {
      const rfLo = Math.min(a, b)
      const rfHi = Math.max(a, b)
      ctx.lineWidth = s.lineW
      for (const [lo, hi] of outsideSpans(s.spans.spans, rfLo, rfHi)) {
        const x0 = xRf(lo)
        const x1 = xRf(hi)
        if (x0 == null || x1 == null) continue
        ctx.globalAlpha = TINT_ALPHA
        ctx.fillStyle = inks.blocked
        ctx.fillRect(Math.min(x0, x1), 0, Math.abs(x1 - x0), s.h)
        ctx.globalAlpha = EDGE_ALPHA
        ctx.strokeStyle = inks.blocked
        for (const [rf, x] of [[lo, x0], [hi, x1]] as const) {
          // The view's own edge is not a privilege edge.
          if (rf <= rfLo || rf >= rfHi) continue
          ctx.beginPath()
          ctx.moveTo(Math.round(x), 0)
          ctx.lineTo(Math.round(x), s.h)
          ctx.stroke()
        }
      }
      ctx.globalAlpha = 1
    }
  }

  // ---- The FT offsets: the TX line (brighter while transmitting) and the RX line, as WSJT-X's ----
  const dial = s.axis.dialHz
  const ft = s.ft && dial != null && dial > 0 ? s.ft : null
  if (ft && dial != null) {
    ctx.font = `600 ${Math.max(8, Math.round(10 * s.textPx))}px system-ui, sans-serif`
    ctx.textAlign = 'left'
    const lines = [
      { hz: ft.txHz, color: inks.tx, alpha: s.transmitting ? 0.95 : 0.7, plate: TX_PLATE, rx: false },
      { hz: ft.rxHz, color: inks.rx, alpha: 0.9, plate: RX_PLATE, rx: true },
    ]
    for (const l of lines) {
      const x = xRf(rfOfOffset(dial, l.hz, ft.side))
      if (x == null || x < 0 || x > s.w) continue
      ctx.globalAlpha = l.alpha
      ctx.fillStyle = l.color
      ctx.fillRect(x - s.lineW, 0, 2 * s.lineW, s.h)
      // The two plates sit either side of the middle, so a TX on the RX offset still reads as both.
      ctx.globalAlpha = 1
      ctx.textBaseline = l.rx ? 'top' : 'bottom'
      ctx.fillText(l.plate, Math.min(s.w - 18 * s.textPx, x + 3 * s.textPx), s.h / 2 + (l.rx ? 2 : -2) * s.textPx)
    }
    ctx.globalAlpha = 1
  }

  // ---- The tags: spots (BandMap's), and on the RF scope the newest slot's decodes ----
  const laneTop = PLATE_ROW * s.textPx
  const tagH = TAG_H * s.textPx
  const tickTop = laneTop + tagH
  // No room under the plates for a lane and its ticks (a scope a few dozen px tall): no tags at all,
  // rather than labels written over the waterfall.
  if (s.h < (PLATE_ROW + TAG_H + TICK) * s.textPx) return { hits, tags: 0, hidden: 0 }
  ctx.font = `${Math.max(8, Math.round(TAG_FONT * s.textPx))}px system-ui, sans-serif`
  const pad = TAG_PAD * s.textPx
  const tags: (TagIn & { text: string; alpha: number; ink: string; accent: string | null; spot: SpotRow | null })[] = []
  for (const sp of s.spots) {
    const x = xRf(Math.round(sp.spot.freqMhz * 1e6))
    if (x == null || x < 0 || x > s.w) continue
    tags.push({
      x,
      w: ctx.measureText(sp.spot.call).width + 2 * pad,
      rank: spotFreshness(sp.spot.ageSecs),
      text: sp.spot.call,
      alpha: spotAlpha(sp.spot.ageSecs),
      ink: inks.ink,
      accent: sp.ink ? inks.color(sp.ink) : null,
      spot: sp.spot,
    })
  }
  if (ft && dial != null) {
    // One slot's decodes are all the same age, so a station calling us comes first, then the strongest;
    // they rank after any spot.
    const order = [...ft.decodes].sort((a, b) => Number(b.me) - Number(a.me) || b.snr - a.snr)
    order.forEach((d, k) => {
      const x = xRf(rfOfOffset(dial, d.hz, ft.side))
      if (x == null || x < 0 || x > s.w) return
      tags.push({
        x,
        w: ctx.measureText(d.call).width + 2 * pad,
        rank: 1e9 + k,
        text: d.call,
        alpha: 1,
        ink: d.me ? inks.tx : inks.ink,
        accent: null,
        spot: null,
      })
    })
  }
  const gap = TAG_GAP * s.textPx
  let laid = layoutTags(tags, s.w, gap)
  if (laid.hidden > 0) {
    // Room for the count at the far end, as wide as the largest it could be.
    const countW = ctx.measureText(`+${tags.length}`).width + pad
    laid = layoutTags(tags, s.w - countW - gap, gap)
  }
  ctx.lineWidth = s.lineW
  ctx.textAlign = 'left'
  ctx.textBaseline = 'middle'
  for (const p of laid.placed) {
    const t = tags[p.i]
    // The tick at the true frequency; the label above it may have moved aside.
    ctx.globalAlpha = t.alpha
    ctx.strokeStyle = t.ink
    ctx.beginPath()
    ctx.moveTo(Math.round(t.x), tickTop)
    ctx.lineTo(Math.round(t.x), tickTop + TICK * s.textPx)
    ctx.stroke()
    ctx.globalAlpha = t.alpha * 0.75
    ctx.fillStyle = inks.ground
    ctx.fillRect(p.left, laneTop, t.w, tagH)
    ctx.globalAlpha = t.alpha
    if (t.accent) {
      ctx.fillStyle = t.accent
      ctx.fillRect(p.left, laneTop, ACCENT * s.textPx, tagH)
    }
    ctx.fillStyle = t.ink
    ctx.fillText(t.text, p.left + pad, laneTop + tagH / 2)
    if (t.spot) hits.push({ l: p.left, r: p.left + t.w, t: laneTop, b: laneTop + tagH, spot: t.spot })
  }
  if (laid.hidden > 0) {
    ctx.globalAlpha = 0.85
    ctx.fillStyle = inks.ink
    ctx.textAlign = 'right'
    ctx.fillText(`+${laid.hidden}`, s.w - pad / 2, laneTop + tagH / 2)
  }
  ctx.globalAlpha = 1
  return { hits, tags: laid.placed.length, hidden: laid.hidden }
}
