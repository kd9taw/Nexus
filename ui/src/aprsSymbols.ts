/**
 * APRS symbols — resolving a packet's symbol table + code to something drawable, and the artwork.
 *
 * An APRS station chooses an icon by two characters: a **symbol table identifier** and a **symbol
 * code**. Nexus drew every station as an identical grey dot, which threw all of that away — a
 * weather station, a truck on the interstate and the digipeater relaying them both looked the same.
 *
 * ## Why the artwork is drawn here rather than bundled
 *
 * The obvious move is to bundle the aprs.fi icon set (`hessu/aprs-symbols`). Its licensing does not
 * survive review: the repo ships **no LICENSE file**, and its own `COPYRIGHT.md` marks the majority
 * of the core symbols "Licensing: Unknown", tracing them to an unidentified original author. Three
 * more are corporate trademarks. Nexus is GPL-3.0-only and publishes source, so "probably fine"
 * is not a licence. These glyphs are therefore original work, drawn from the protocol spec's
 * DESCRIPTIONS of what each symbol means — no third-party artwork, nothing to attribute.
 *
 * ## The addressing rules, from APRS Protocol Reference 1.0.1 ch. 20
 *
 * | Identifier | Meaning |
 * |---|---|
 * | `/`   | primary table |
 * | `\`   | alternate table |
 * | `0-9` | numeric overlay, symbol from the ALTERNATE table (uncompressed reports) |
 * | `a-j` | numeric overlay 0-9, alternate table (COMPRESSED reports only — see below) |
 * | `A-Z` | alpha overlay, alternate table |
 *
 * A compressed position report may never begin with a digit, so a numeric overlay is carried as
 * `a-j` instead and maps back: `a`=0 … `j`=9. Miss that and every compressed overlaid station —
 * a large share of the LoRa trackers now on the air — resolves to a nonsense symbol.
 *
 * Overlays are accepted on ANY alternate-table symbol. The 1.0.1 spec lists a small
 * "[with overlay]" subset, but Bruninga's master list supersedes it: *"As of 1 October 2007, the
 * use of overlay characters on all alternate symbols was allowed as needed."*
 *
 * ## One artwork definition, two renderers
 *
 * Each glyph is an SVG path in a 24×24 box. The map turns it into a `Path2D` for the canvas; the
 * station list drops the same `d` into an `<svg>`. Neither can drift from the other, and the
 * resolution below is pure data, so all of it is testable without a canvas.
 */

/** A drawable glyph: the artwork identity, independent of which symbol code selected it. */
export type GlyphId =
  | 'car'
  | 'truck'
  | 'van'
  | 'jeep'
  | 'bus'
  | 'motorcycle'
  | 'bicycle'
  | 'person'
  | 'rv'
  | 'boat'
  | 'sailboat'
  | 'aircraft'
  | 'helicopter'
  | 'balloon'
  | 'digipeater'
  | 'igate'
  | 'weather'
  | 'house'
  | 'antenna'
  | 'repeater'
  | 'ambulance'
  | 'fire'
  | 'police'
  | 'tent'
  | 'dot'
  | 'circle'
  | 'triangle'
  | 'question'
  | 'unknown'

/** What a symbol resolved to. */
export interface ResolvedSymbol {
  glyph: GlyphId
  /** The character to draw ON TOP of the glyph, or null. Only alternate-table symbols carry one. */
  overlay: string | null
  /** Human-readable name, for the list column and the map tooltip. */
  label: string
  /** Which table the code came from — `alternate` symbols are the ones an overlay may modify. */
  table: 'primary' | 'alternate'
  /** True when the glyph is drawn nose-up and should rotate to the station's course. */
  rotates: boolean
  /** False when nothing matched and this is the fallback glyph. */
  known: boolean
}

interface Entry {
  glyph: GlyphId
  label: string
  rotates?: boolean
}

/**
 * Primary-table (`/`) symbols. Every assignment is from the APRS 1.0.1 Appendix 2 chart and
 * Bruninga's `symbolsX.txt` master list — the protocol's own definitions of what each code MEANS.
 * A code absent from here falls back to the generic glyph rather than guessing.
 */
const PRIMARY: Record<string, Entry> = {
  '!': { glyph: 'police', label: 'Police / sheriff' },
  '#': { glyph: 'digipeater', label: 'Digipeater' },
  '&': { glyph: 'igate', label: 'HF gateway' },
  "'": { glyph: 'aircraft', label: 'Small aircraft', rotates: true },
  '-': { glyph: 'house', label: 'House (VHF)' },
  '.': { glyph: 'question', label: 'Unknown position' },
  '/': { glyph: 'dot', label: 'Dot' },
  ';': { glyph: 'tent', label: 'Campground' },
  '<': { glyph: 'motorcycle', label: 'Motorcycle', rotates: true },
  '>': { glyph: 'car', label: 'Car', rotates: true },
  I: { glyph: 'antenna', label: 'TCP/IP station' },
  O: { glyph: 'balloon', label: 'Balloon' },
  P: { glyph: 'police', label: 'Police car' },
  R: { glyph: 'rv', label: 'Recreational vehicle', rotates: true },
  U: { glyph: 'bus', label: 'Bus', rotates: true },
  X: { glyph: 'helicopter', label: 'Helicopter', rotates: true },
  Y: { glyph: 'sailboat', label: 'Sailboat', rotates: true },
  '[': { glyph: 'person', label: 'Person' },
  '^': { glyph: 'aircraft', label: 'Large aircraft', rotates: true },
  _: { glyph: 'weather', label: 'Weather station' },
  a: { glyph: 'ambulance', label: 'Ambulance', rotates: true },
  b: { glyph: 'bicycle', label: 'Bicycle', rotates: true },
  d: { glyph: 'fire', label: 'Fire station' },
  f: { glyph: 'fire', label: 'Fire truck', rotates: true },
  j: { glyph: 'jeep', label: 'Jeep', rotates: true },
  k: { glyph: 'truck', label: 'Truck', rotates: true },
  r: { glyph: 'repeater', label: 'Repeater' },
  s: { glyph: 'boat', label: 'Boat', rotates: true },
  u: { glyph: 'truck', label: 'Semi trailer', rotates: true },
  v: { glyph: 'van', label: 'Van', rotates: true },
  y: { glyph: 'antenna', label: 'House with yagi' },
}

/**
 * Alternate-table (`\`) symbols — the overlay-capable set. Far fewer are common on the air, and an
 * overlaid `\&` (gateway) or `\#` (digipeater) is most of what an operator actually sees.
 */
const ALTERNATE: Record<string, Entry> = {
  '#': { glyph: 'digipeater', label: 'Digipeater' },
  '&': { glyph: 'igate', label: 'Gateway / iGate' },
  '-': { glyph: 'house', label: 'House' },
  '.': { glyph: 'question', label: 'Unknown position' },
  '0': { glyph: 'circle', label: 'Circle' },
  '>': { glyph: 'car', label: 'Vehicle', rotates: true },
  A: { glyph: 'circle', label: 'Box' },
  W: { glyph: 'weather', label: 'Weather site' },
  '^': { glyph: 'aircraft', label: 'Aircraft', rotates: true },
  _: { glyph: 'weather', label: 'Weather site' },
  k: { glyph: 'jeep', label: 'SUV / 4x4', rotates: true },
  n: { glyph: 'triangle', label: 'Triangle' },
  s: { glyph: 'boat', label: 'Vessel', rotates: true },
  u: { glyph: 'truck', label: 'Truck', rotates: true },
  v: { glyph: 'van', label: 'Van', rotates: true },
  y: { glyph: 'weather', label: 'Skywarn' },
}

/** The generic glyph. Bruninga, 2004: an unassigned symbol shows "a circle with a slash through
 * it", meaning "not". A station always draws as SOMETHING — a blank would read as a bug. */
const FALLBACK: Entry = { glyph: 'unknown', label: 'Unknown symbol' }

/**
 * Resolve a packet's symbol table identifier + symbol code to a drawable glyph.
 *
 * Never fails: anything unrecognised — including the empty table/code a message or status packet
 * carries — resolves to the generic glyph with `known: false`.
 */
export function resolveSymbol(table: string, code: string): ResolvedSymbol {
  const t = (table ?? '').charAt(0)
  const c = (code ?? '').charAt(0)

  // `a-j` is a numeric overlay from a COMPRESSED report: map it back to its digit before anything
  // else looks at it. Skipping this turns every compressed overlaid tracker into a wrong symbol.
  let ident = t
  if (t >= 'a' && t <= 'j') {
    ident = String.fromCharCode(t.charCodeAt(0) - 97 + 48)
  }

  const isOverlay = (ident >= '0' && ident <= '9') || (ident >= 'A' && ident <= 'Z')
  const alternate = ident === '\\' || isOverlay
  const overlay = isOverlay ? ident : null

  const entry = (alternate ? ALTERNATE : PRIMARY)[c]
  const known = entry !== undefined && c !== ''
  const e = entry ?? FALLBACK
  return {
    glyph: e.glyph,
    overlay,
    label: e.label,
    table: alternate ? 'alternate' : 'primary',
    rotates: e.rotates === true,
    known,
  }
}

/**
 * Glyph artwork: SVG path data in a 24×24 box, centred on (12, 12).
 *
 * Deliberately simple silhouettes. These are read at 14–18 px on a map that may be showing a
 * hundred stations, where a recognisable shape beats a detailed one, and they must stay legible
 * as a single flat colour in both themes.
 */
export const GLYPH_PATHS: Record<GlyphId, string> = {
  // Vehicles are drawn nose-up so they can rotate to a course.
  car: 'M9 4h6l1.5 5H7.5L9 4zM7 9h10v9a1 1 0 0 1-1 1h-1.5v-2h-5v2H8a1 1 0 0 1-1-1V9zm1.5 2v2h2v-2h-2zm5 0v2h2v-2h-2z',
  truck: 'M8.5 3h7v6h-7V3zM7 10h10v8a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1v-8zm2 2v3h6v-3H9z',
  van: 'M7.5 4h9v14a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1V4zm1.5 2v3h6V6H9zm0 5v2h2v-2H9zm4 0v2h2v-2h-2z',
  jeep: 'M8 5h8v4H8V5zM6.5 10h11v7a1 1 0 0 1-1 1H15v-1.5H9V18H7.5a1 1 0 0 1-1-1v-7zm2 1.5v2.5h7v-2.5h-7z',
  bus: 'M7 3h10v15a1 1 0 0 1-1 1h-1v-1.5H9V19H8a1 1 0 0 1-1-1V3zm2 2v4h6V5H9zm0 6v2h2v-2H9zm4 0v2h2v-2h-2z',
  motorcycle:
    'M12 3l2 4h-4l2-4zM6 13a3 3 0 1 1 0 6 3 3 0 0 1 0-6zm12 0a3 3 0 1 1 0 6 3 3 0 0 1 0-6zM11 8h2l3 6h-2l-2-4-2 4H8l3-6z',
  bicycle:
    'M6 13a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7zm12 0a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7zM9 6h3l2 4h3v1.5h-4L11 8H9V6zm-.5 4.5h5l-2.5 5-2.5-5z',
  person:
    'M12 3a2.4 2.4 0 1 1 0 4.8A2.4 2.4 0 0 1 12 3zm-3 6h6a1 1 0 0 1 1 1v5h-2v6h-4v-6H8v-5a1 1 0 0 1 1-1z',
  rv: 'M6 5h12v11a1 1 0 0 1-1 1h-1.2a2 2 0 0 1-3.6 0H9.8a2 2 0 0 1-3.6 0H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm2 2v4h4V7H8zm6 0v4h2V7h-2z',
  boat: 'M11 3h2v6h4l-1 3H8l-1-3h4V3zM4 15h16l-2.5 5h-11L4 15z',
  sailboat: 'M12 2l6 11h-6V2zm-1 2v9H6l5-9zM4 16h16l-2.5 5H6.5L4 16z',
  aircraft: 'M12 2l2 6 8 5v2l-8-2.5V18l3 2v2l-5-1.5L7 22v-2l3-2v-5.5L2 15v-2l8-5 2-6z',
  helicopter:
    'M3 4h18v1.5h-8.2V8H16a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2h-1l3 4h-2l-3-4H9a3 3 0 0 1-3-3v-3a2 2 0 0 1 2-2h3.2V5.5H3V4z',
  balloon:
    'M12 2a6 6 0 0 1 6 6c0 3.5-3 6.5-5 8h-2c-2-1.5-5-4.5-5-8a6 6 0 0 1 6-6zm-1.5 16h3l-.5 4h-2l-.5-4z',
  // Infrastructure — a digipeater is a star by long convention.
  digipeater: 'M12 2l2.6 6.6L21.5 9l-5 4.7 1.5 7-6-3.6-6 3.6 1.5-7-5-4.7 6.9-.4L12 2z',
  igate: 'M12 2l7 4v5c0 5-3 8.5-7 11-4-2.5-7-6-7-11V6l7-4zm0 5a4 4 0 0 0-4 4h2.2a1.8 1.8 0 0 1 3.6 0H16a4 4 0 0 0-4-4zm0 3.2a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6z',
  weather:
    'M12 3a4.5 4.5 0 0 1 4.4 3.6A3.8 3.8 0 0 1 16 14H8a4 4 0 0 1-.6-8A4.5 4.5 0 0 1 12 3zM8.5 16l-1.5 5h2l1.5-5h-2zm5 0l-1.5 5h2l1.5-5h-2z',
  house: 'M12 3l9 8h-2.5v9h-5v-6h-3v6h-5v-9H3l9-8z',
  antenna:
    'M11 3h2v18h-2V3zM6.5 5.5L8 7a5.7 5.7 0 0 0 0 8l-1.5 1.5a7.8 7.8 0 0 1 0-11zm11 0a7.8 7.8 0 0 1 0 11L16 15a5.7 5.7 0 0 0 0-8l1.5-1.5z',
  repeater: 'M9 8h6l3 13h-3l-.7-3h-4.6L9 21H6L9 8zm1.6 3l-.7 3h4.2l-.7-3h-2.8zM6 2l1.6 1.6a6 6 0 0 0 0 8.4L6 13.6a8.3 8.3 0 0 1 0-11.6zm12 0a8.3 8.3 0 0 1 0 11.6L16.4 12a6 6 0 0 0 0-8.4L18 2z',
  ambulance:
    'M7 6h10v13a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V6zm4 2v2H9v2h2v2h2v-2h2v-2h-2V8h-2zM9 2h6v3H9V2z',
  fire: 'M12 2c1 3.5 5 5 5 9.5A5 5 0 0 1 12 22a5 5 0 0 1-5-5.5c0-2 1-3.5 2-4.5.3 1.2 1 2 2 2.2-.8-2.8.2-5.5 1-8.2z',
  police:
    'M12 2l8 3v6c0 5.5-3.4 9.5-8 11-4.6-1.5-8-5.5-8-11V5l8-3zm0 4.5l-1.4 3-3.1.3 2.3 2.1-.7 3.1 2.9-1.7 2.9 1.7-.7-3.1 2.3-2.1-3.1-.3-1.4-3z',
  tent: 'M12 3l9 17h-7l-2-5-2 5H3L12 3zm0 5.5L7.5 18h9L12 8.5z',
  // Neutral shapes.
  dot: 'M12 7a5 5 0 1 1 0 10 5 5 0 0 1 0-10z',
  circle: 'M12 4a8 8 0 1 1 0 16 8 8 0 0 1 0-16zm0 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11z',
  triangle: 'M12 3l9.5 17H2.5L12 3zm0 5L6.6 17.5h10.8L12 8z',
  question:
    'M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm0 2.2a6.8 6.8 0 1 0 0 13.6 6.8 6.8 0 0 0 0-13.6zM12 7.6a2.9 2.9 0 0 1 1.6 5.3c-.4.3-.6.5-.6.9v.5h-2v-.7c0-1 .5-1.6 1.2-2.1a.9.9 0 1 0-1.4-.8h-2A2.9 2.9 0 0 1 12 7.6zm-1 8.4h2v2h-2v-2z',
  // The generic fallback: Bruninga's "circle with a slash through it", meaning "not".
  unknown: 'M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm0 2.2a6.7 6.7 0 0 0-5.3 10.8L17 6.7A6.7 6.7 0 0 0 12 5.2zm5.3 3.2L7 17.3a6.7 6.7 0 0 0 10.3-8.9z',
}

/**
 * The same artwork as a canvas `Path2D`, built once per glyph and reused.
 *
 * The map redraws on every pan, zoom and decode with potentially hundreds of stations on screen;
 * re-parsing path data per station per frame would be pure waste. Constructed lazily so importing
 * this module stays safe where `Path2D` does not exist (jsdom under test).
 */
const PATH_CACHE = new Map<GlyphId, Path2D>()
export function glyphPath(id: GlyphId): Path2D {
  let p = PATH_CACHE.get(id)
  if (!p) {
    p = new Path2D(GLYPH_PATHS[id])
    PATH_CACHE.set(id, p)
  }
  return p
}

/** How a station's source is drawn once the dot has become a symbol. See `sourceRing`. */
export type SourceRing = 'solid' | 'double' | 'dashed'

/**
 * The RF-versus-internet treatment, moved off the dot and onto a ring around the glyph.
 *
 * The dot used to carry it: solid meant "my antenna heard this", hollow meant "only the internet
 * reports it". A symbol glyph cannot be hollow without becoming unreadable, so the SHAPE now says
 * what the station is and the RING says how it reached us — keeping the original language, where
 * a solid outline still means the operator's own receiver.
 *
 * Uses stroke style rather than colour so it survives both themes and stays legible against any
 * map background, and so it reads identically for a detailed glyph and the generic fallback.
 */
export function sourceRing(source: 'rf' | 'inet' | 'both'): {
  ring: SourceRing
  alpha: number
} {
  switch (source) {
    // Heard on RF: full strength, solid ring.
    case 'rf':
      return { ring: 'solid', alpha: 1 }
    // Heard BOTH ways: still our own reception, so still solid — doubled to say there is more
    // evidence, never dimmed.
    case 'both':
      return { ring: 'double', alpha: 1 }
    // Internet only: dashed ring and a dimmer glyph. Present on the map, visibly not something
    // this radio has proven it can hear.
    case 'inet':
      return { ring: 'dashed', alpha: 0.62 }
  }
}

/**
 * Below this map zoom a station draws as a plain dot instead of its symbol.
 *
 * At a continental scale a screen of 18 px glyphs is unreadable mush, and the symbol is not the
 * question being asked at that zoom — "where is there traffic" is. The APRS map opens well above
 * this (`APRS_HOME_ZOOM`), so the local view an operator actually uses always shows symbols.
 */
export const SYMBOL_MIN_ZOOM = 4

/** Should a symbol be drawn at this zoom, or a plain dot? */
export function showSymbolAt(zoom: number): boolean {
  return zoom >= SYMBOL_MIN_ZOOM
}
