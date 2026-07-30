import { describe, expect, it } from 'vitest'
import {
  ageFade,
  APRS_REDRAW_MS,
  aprsRedrawMs,
  CATEGORY_VAR,
  GLYPH_PATHS,
  MIN_FADE,
  symbolCategory,
  resolveSymbol,
  showSymbolAt,
  sourceRing,
  SYMBOL_MIN_ZOOM,
  type GlyphId,
} from './aprsSymbols'

// The symbol table + code have been parsed off every packet since APRS shipped; the map just threw
// them away and drew a dot. These tests cover the resolution that turns them back into artwork —
// especially the two rules that are easy to get silently wrong (overlays, and the compressed-report
// a-j encoding), because a wrong symbol looks exactly as confident as a right one.

describe('primary table', () => {
  it('resolves the codes that dominate 144.390', () => {
    // Assignments are from APRS 1.0.1 Appendix 2 / Bruninga's symbolsX.txt.
    const cases: [string, GlyphId][] = [
      ['>', 'car'],
      ['k', 'truck'],
      ['v', 'van'],
      ['j', 'jeep'],
      ['[', 'person'],
      ['<', 'motorcycle'],
      ['b', 'bicycle'],
      ['_', 'weather'],
      ['-', 'house'],
      ['#', 'digipeater'],
      ['&', 'igate'],
      ['O', 'balloon'],
      ['^', 'aircraft'],
      [';', 'tent'],
      ['R', 'rv'],
    ]
    for (const [code, glyph] of cases) {
      const r = resolveSymbol('/', code)
      expect(r.glyph, `primary '${code}'`).toBe(glyph)
      expect(r.known).toBe(true)
      expect(r.table).toBe('primary')
      expect(r.overlay).toBeNull()
    }
  })

  it('a primary-table symbol never carries an overlay', () => {
    // Only the alternate table is overlaid. `/` and `\` are table identifiers, not overlay chars.
    expect(resolveSymbol('/', '>').overlay).toBeNull()
    expect(resolveSymbol('\\', '>').overlay).toBeNull()
  })
})

describe('overlays', () => {
  it('a digit or letter in the table slot selects the ALTERNATE table and draws on top', () => {
    // Real captured packets: `...4536.63NG02257.00E#...` is an overlaid digipeater, and
    // `...2537.90NR10014.20W&` is a receive-only iGate — the `R&` convention.
    const digi = resolveSymbol('G', '#')
    expect(digi.glyph).toBe('digipeater')
    expect(digi.overlay).toBe('G')
    expect(digi.table).toBe('alternate')

    const igate = resolveSymbol('R', '&')
    expect(igate.glyph).toBe('igate')
    expect(igate.overlay).toBe('R')
    expect(igate.table).toBe('alternate')

    const numbered = resolveSymbol('3', '#')
    expect(numbered.overlay).toBe('3')
    expect(numbered.table).toBe('alternate')
  })

  it('accepts an overlay on ANY alternate symbol, not the obsolete 1.0.1 subset', () => {
    // Bruninga superseded the "[with overlay]" list in 2007: overlays are allowed on all of them.
    const r = resolveSymbol('T', '_')
    expect(r.overlay).toBe('T')
    expect(r.glyph).toBe('weather')
    expect(r.known).toBe(true)
  })
})

describe('compressed-report overlays (the a-j rule)', () => {
  it('maps a-j back to 0-9 — a compressed position may never start with a digit', () => {
    // APRS 1.0.1 ch.20: "The lower-case letter is then mapped to the digits 0-9 (a=0, b=1 …)".
    // Miss this and every compressed overlaid tracker resolves to a nonsense symbol.
    expect(resolveSymbol('a', '#').overlay).toBe('0')
    expect(resolveSymbol('d', '#').overlay).toBe('3') // the spec's own worked example
    expect(resolveSymbol('j', '#').overlay).toBe('9')
  })

  it('an a-j identifier selects the alternate table, like the digit it stands for', () => {
    const viaLetter = resolveSymbol('d', '>')
    const viaDigit = resolveSymbol('3', '>')
    expect(viaLetter.table).toBe('alternate')
    expect(viaLetter.glyph).toBe(viaDigit.glyph)
  })

  it('does not swallow lowercase letters past j, which are ordinary symbol codes', () => {
    // `k` is a real primary-table code (truck). Treating it as an overlay would eat it.
    expect(resolveSymbol('/', 'k').glyph).toBe('truck')
    expect(resolveSymbol('k', '>').overlay).toBeNull()
  })
})

describe('the fallback is never a blank', () => {
  it('an unrecognised code still draws something', () => {
    const r = resolveSymbol('/', '')
    expect(r.glyph).toBe('unknown')
    expect(r.known).toBe(false)
    expect(GLYPH_PATHS[r.glyph]).toBeTruthy()
  })

  it('a message or status packet, which carries no symbol at all, resolves rather than throwing', () => {
    // The engine flattens these with a space for both fields.
    for (const [t, c] of [
      [' ', ' '],
      ['', ''],
    ]) {
      const r = resolveSymbol(t, c)
      expect(r.known).toBe(false)
      expect(r.glyph).toBe('unknown')
    }
  })

  it('the documented "unknown position" symbol is its own thing, not the fallback', () => {
    // `\.` means the POSITION is ambiguous — a real symbol with a real meaning, distinct from
    // "this code is not in our table".
    const r = resolveSymbol('\\', '.')
    expect(r.glyph).toBe('question')
    expect(r.known).toBe(true)
  })
})

describe('artwork', () => {
  it('every glyph a symbol can resolve to has a path', () => {
    // The exhaustive guard: adding a GlyphId without artwork would render nothing at all.
    const printable = Array.from({ length: 95 }, (_, i) => String.fromCharCode(33 + i))
    const seen = new Set<GlyphId>()
    for (const table of ['/', '\\', 'A', '7', 'd']) {
      for (const code of printable) seen.add(resolveSymbol(table, code).glyph)
    }
    for (const g of seen) {
      expect(GLYPH_PATHS[g], `glyph '${g}' has no path`).toBeTruthy()
    }
    expect(seen.size).toBeGreaterThan(10)
  })

  it('every declared path is non-trivial SVG path data', () => {
    for (const [id, d] of Object.entries(GLYPH_PATHS)) {
      expect(d.startsWith('M'), `${id} should start with a moveto`).toBe(true)
      expect(d.length, `${id} looks too short to be a real shape`).toBeGreaterThan(20)
    }
  })

  it('symbols that represent something with a heading are marked rotatable', () => {
    // A car drawn nose-up is wrong unless it turns with the course; a house never rotates.
    expect(resolveSymbol('/', '>').rotates).toBe(true)
    expect(resolveSymbol('/', '^').rotates).toBe(true)
    expect(resolveSymbol('/', 's').rotates).toBe(true)
    expect(resolveSymbol('/', '-').rotates).toBe(false)
    expect(resolveSymbol('/', '_').rotates).toBe(false)
    expect(resolveSymbol('/', '#').rotates).toBe(false)
  })
})

describe('the RF/internet distinction survives the move off the dot', () => {
  it('RF and both stay full strength — an own-antenna sighting is never dimmed', () => {
    expect(sourceRing('rf')).toEqual({ ring: 'solid', alpha: 1 })
    expect(sourceRing('both').alpha).toBe(1)
  })

  it('internet-only is dashed and dimmer, keeping the original solid-means-mine language', () => {
    const inet = sourceRing('inet')
    expect(inet.ring).toBe('dashed')
    expect(inet.alpha).toBeLessThan(1)
  })

  it('all three treatments are visually distinct', () => {
    const rings = (['rf', 'inet', 'both'] as const).map((s) => sourceRing(s).ring)
    expect(new Set(rings).size).toBe(3)
  })
})

describe('map calm at low zoom', () => {
  it('draws plain dots when zoomed out and symbols when zoomed in', () => {
    expect(showSymbolAt(1)).toBe(false)
    expect(showSymbolAt(SYMBOL_MIN_ZOOM - 0.1)).toBe(false)
    expect(showSymbolAt(SYMBOL_MIN_ZOOM)).toBe(true)
    expect(showSymbolAt(25)).toBe(true)
  })

  it('the APRS map opens above the threshold, so the local view always has symbols', async () => {
    const { APRS_HOME_ZOOM } = await import('./mapGeo')
    expect(showSymbolAt(APRS_HOME_ZOOM)).toBe(true)
  })
})

describe('category colours', () => {
  it('every glyph has a category, and only the fallbacks are neutral', () => {
    for (const g of Object.keys(GLYPH_PATHS) as GlyphId[]) {
      expect(CATEGORY_VAR[symbolCategory(g)], `glyph '${g}'`).toBeTruthy()
    }
    expect(symbolCategory('unknown')).toBe('other')
    expect(symbolCategory('question')).toBe('other')
  })

  it('groups symbols the way an operator would', () => {
    expect(symbolCategory('house')).toBe('fixed')
    expect(symbolCategory('car')).toBe('mobile')
    expect(symbolCategory('aircraft')).toBe('air')
    expect(symbolCategory('boat')).toBe('marine')
    expect(symbolCategory('weather')).toBe('wx')
    expect(symbolCategory('digipeater')).toBe('infra')
    expect(symbolCategory('igate')).toBe('infra')
  })

  it('colour is category identity, NOT severity', () => {
    // An ambulance is a VEHICLE. Painting it as its own alarming thing would turn the map into an
    // incident board and make colour mean two different things at once.
    expect(symbolCategory('ambulance')).toBe('mobile')
    expect(symbolCategory('police')).toBe('mobile')
    expect(symbolCategory('fire')).toBe('mobile')
    // ...but a fire STATION is infrastructure at a fixed place, and a fire truck is not.
    expect(symbolCategory('antenna')).toBe('fixed')
  })

  it('every category maps to a distinct CSS variable', () => {
    const vars = Object.values(CATEGORY_VAR)
    expect(new Set(vars).size).toBe(vars.length)
    for (const v of vars) expect(v.startsWith('--aprs-cat-')).toBe(true)
  })
})

describe('age fade', () => {
  const NOW = 1_700_000_000
  const heard = (minAgo: number) => NOW - minAgo * 60

  it('a freshly heard station is not dimmed at all', () => {
    expect(ageFade(heard(0), NOW, 20, 60)).toBe(1)
    expect(ageFade(heard(19), NOW, 20, 60)).toBe(1)
  })

  it('a station beaconing on any normal cycle never fades', () => {
    // Mobiles run 1-2 min and fixed stations commonly 10 — the fade must start past all of them,
    // or an active station dims for no reason, which is the flashing complaint in slow motion.
    for (const cycle of [1, 2, 5, 10]) {
      expect(ageFade(heard(cycle), NOW, 20, 60), `${cycle} min cycle`).toBe(1)
    }
  })

  it('eases across the stale band rather than snapping', () => {
    const mid = ageFade(heard(40), NOW, 20, 60)
    expect(mid).toBeLessThan(1)
    expect(mid).toBeGreaterThan(MIN_FADE)
    // Monotonic: older is always dimmer, never brighter.
    expect(ageFade(heard(50), NOW, 20, 60)).toBeLessThan(mid)
  })

  it('bottoms out rather than vanishing, so a station is never invisible-but-present', () => {
    expect(ageFade(heard(60), NOW, 20, 60)).toBe(MIN_FADE)
    expect(ageFade(heard(600), NOW, 20, 60)).toBe(MIN_FADE)
  })

  it('composes multiplicatively with the internet dimming', () => {
    // Both facts reduce how much a station should be asserting, so they stack.
    const fade = ageFade(heard(40), NOW, 20, 60)
    const combined = sourceRing('inet').alpha * fade
    expect(combined).toBeLessThan(fade)
    expect(combined).toBeLessThan(sourceRing('inet').alpha)
    expect(combined).toBeGreaterThan(0)
  })

  it('scales with a shortened window instead of ignoring it', () => {
    // ttl 15 / fade 5: a station silent 10 minutes is half-stale here but perfectly fresh at the
    // default. The thresholds have to travel with the data.
    expect(ageFade(heard(10), NOW, 5, 15)).toBeLessThan(1)
    expect(ageFade(heard(10), NOW, 20, 60)).toBe(1)
  })

  it('survives a degenerate window without dividing by zero', () => {
    expect(Number.isFinite(ageFade(heard(30), NOW, 60, 60))).toBe(true)
  })
})

describe('the APRS map keeps repainting itself', () => {
  it('asks for a repaint clock only when there is an APRS layer with stations on it', () => {
    expect(aprsRedrawMs(true, 5)).toBe(APRS_REDRAW_MS)
    // No layer, or nothing on it: no clock, so no other map view pays for this.
    expect(aprsRedrawMs(false, 5)).toBeNull()
    expect(aprsRedrawMs(true, 0)).toBeNull()
  })

  it('repaints often enough that a station can never be missing for long', () => {
    // ⭐ THE REGRESSION GUARD. Until 0.21.4 the canvas was kept painting by ACCIDENT: the 2 s poll
    // handed the map a freshly allocated array every tick, so the prop identity changed and forced
    // a redraw. Making that identity stable removed the only thing repainting a stateful canvas,
    // and stations could sit invisible until the 60 s greyline clock came round. The cadence is
    // explicit now, and must stay well under that minute.
    expect(APRS_REDRAW_MS).toBeLessThanOrEqual(5000)
    expect(APRS_REDRAW_MS).toBeGreaterThanOrEqual(1000) // ...but not a busy-loop
  })
})

describe('the fade is driven by a wall clock', () => {
  it('a station heard seconds ago is fully bright, not faded', () => {
    // The bug this pins: computing the fade from the 60 s greyline clock reported a fresh decode as
    // up to a minute old, quantising the fade into minute-wide steps.
    const now = 1_700_000_000
    for (const secondsAgo of [0, 1, 5, 30, 60, 120]) {
      expect(ageFade(now - secondsAgo, now, 20, 60), `${secondsAgo}s ago`).toBe(1)
    }
  })

  it('and a station is only dropped by the backend, never dimmed to nothing', () => {
    // Whatever the clock says, the floor keeps a still-retained station visible.
    expect(ageFade(1_700_000_000 - 59 * 60, 1_700_000_000, 20, 60)).toBeGreaterThan(0)
  })
})
