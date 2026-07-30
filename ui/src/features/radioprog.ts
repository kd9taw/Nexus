// Pure view-model for the Program section (no React, no IO — unit-tested like
// memoryBank.ts): the channel-name engine (per-radio display caps, collision
// handling), band classification for the filter chips, and the band-aware
// auto-radius. The Channel/RepeaterRecord DTOs mirror the Rust serde camelCase
// shapes exactly (see crates/propagation/src/{memchan,repeaters}.rs).

import type { ProgChannel, RepeaterRecord } from '../types'
import type { Memory } from './memories'

/** Per-radio channel-name display caps (the header "Max name" select). */
export const NAME_CAPS = [
  { cap: 6, label: '6 — FT-60 class' },
  { cap: 7, label: '7 — Baofeng' },
  { cap: 8, label: '8 — most HTs' },
  { cap: 12, label: '12 — Yaesu mobile' },
  { cap: 16, label: '16 — Anytone' },
] as const

/** Clamp + clean a name the way radios display it (mirror of Rust sanitize_name):
 * uppercase, A–Z 0–9 space / - only, squeezed spaces, cut at `cap`. */
export function sanitizeName(name: string, cap: number): string {
  const cleaned = name
    .trim()
    .toUpperCase()
    .replace(/[^A-Z0-9 /-]/g, '')
    .replace(/ {2,}/g, ' ')
    .trim()
  return cleaned.slice(0, cap)
}

/** The on-air frequency "nickname" hams say: the kHz fraction with trailing
 * zeros dropped — 146.940→"94", 147.255→"255", 442.725→"725", 146.000→"0". */
export function freqTail(outputMhz: number): string {
  const khz = Math.round((outputMhz % 1) * 1000)
  let s = khz.toString().padStart(3, '0')
  while (s.length > 1 && s.endsWith('0')) s = s.slice(0, -1)
  return s
}

/** Squeeze a city to radio-display consonants: first char + consonants after. */
function squeezeCity(city: string, maxLen: number): string {
  const up = city
    .toUpperCase()
    .replace(/[^A-Z]/g, ' ')
    .trim()
  if (!up) return 'CHAN'
  const first = up[0]
  const rest = up
    .slice(1)
    .replace(/[AEIOU ]/g, '')
  return (first + rest).slice(0, maxLen)
}

/** Compose `call + tail` into `cap` chars: keep the space when it fits,
 * truncating the call first; drop the space when the cap is tight. */
function composeCallTail(call: string, tail: string, cap: number): string {
  const avail = cap - tail.length
  if (avail >= call.length + 1) return `${call} ${tail}`
  if (avail >= 3) return `${call.slice(0, avail - 1)} ${tail}`
  return `${call.slice(0, Math.max(1, avail))}${tail}`.slice(0, cap)
}

/** Derive a channel name per row (deterministic, unique within the list):
 * 1. the callsign as-is when it fits the cap and is unique ("that's the W9ABC
 *    repeater");
 * 2. on collision (a club's second machine) or overflow: truncated call + the
 *    frequency nickname — `W9AB 94`;
 * 3. no callsign at all: consonant-squeezed city + nickname — `GTLNB94`;
 * 4. pathological remaining duplicates get an A/B/C suffix. */
export function deriveNames(
  rows: { callsign: string; city: string; outputMhz: number }[],
  cap: number,
): string[] {
  // Strip /R-style suffixes — the base call is what operators call the machine.
  const calls = rows.map((r) => sanitizeName(r.callsign.split('/')[0] ?? '', cap))
  const counts = new Map<string, number>()
  for (const c of calls) {
    if (c) counts.set(c, (counts.get(c) ?? 0) + 1)
  }
  const names = rows.map((r, i) => {
    const call = calls[i]
    if (call && call.length <= cap && (counts.get(call) ?? 0) === 1) return call
    const tail = freqTail(r.outputMhz)
    if (call) return composeCallTail(call, tail, cap)
    return sanitizeName(squeezeCity(r.city, cap - tail.length) + tail, cap)
  })
  // A/B/C for anything still identical (same call, same kHz tail).
  const seen = new Map<string, number>()
  return names.map((n) => {
    const k = seen.get(n) ?? 0
    seen.set(n, k + 1)
    if (k === 0) return n
    const suffix = String.fromCharCode(64 + k) // 1→A, 2→B …
    return (n.length < cap ? n + suffix : n.slice(0, cap - 1) + suffix).slice(0, cap)
  })
}

/** Ham band for the filter chips ('' = outside the chip set, shown under All). */
export function bandOfMhz(mhz: number): string {
  if (mhz >= 28 && mhz <= 29.7) return '10m'
  if (mhz >= 50 && mhz <= 54) return '6m'
  if (mhz >= 144 && mhz <= 148) return '2m'
  if (mhz >= 219 && mhz <= 225) return '1.25m'
  if (mhz >= 420 && mhz <= 450) return '70cm'
  return ''
}

export const BAND_CHIPS = ['2m', '70cm', '1.25m', '6m', '10m'] as const

/** Band-aware auto radius (miles): the reach you'd realistically work the band
 * at HT/mobile power. Multiple bands → the widest; none selected → 50. */
export function autoRadiusMi(bands: string[]): number {
  const reach: Record<string, number> = {
    '10m': 100,
    '6m': 75,
    '2m': 50,
    '1.25m': 25,
    '70cm': 25,
  }
  const picked = bands.map((b) => reach[b] ?? 0).filter((n) => n > 0)
  return picked.length ? Math.max(...picked) : 50
}

export const RADIUS_CHIPS_MI = [10, 25, 50, 100, 200] as const

export function miToKm(mi: number): number {
  return mi * 1.609344
}

export function kmToMi(km: number): number {
  return km / 1.609344
}

/** Compass octant for a bearing in degrees ("NE", "S", …). */
export function octant(bearing: number): string {
  const names = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']
  return names[Math.round((((bearing % 360) + 360) % 360) / 45) % 8]
}

/** Repeater rows the v1 analog path can actually program (FM). Digital-only
 * machines stay visible in the picker (badged) but can't be ADDed. */
export function isProgrammable(r: RepeaterRecord): boolean {
  return r.fm
}

/** Mode badge for a picker row ('' = plain FM, no badge). */
export function modeBadge(r: RepeaterRecord): string {
  if (r.fm) return r.fusion ? '+YSF' : ''
  if (r.dmr) return 'DMR'
  if (r.dstar) return 'D-STAR'
  if (r.fusion) return 'YSF'
  return ''
}

/** A channel's FM repeater parameters for the rig path (tune-now, memories,
 * repeater_tune): shift direction, offset magnitude override in Hz (the EXACT
 * machine offset, so odd splits key the right input), and the CTCSS tone.
 *
 * `split` stores the ABSOLUTE TX frequency in `offsetMhz` (memchan.rs), so the
 * direction and magnitude are derived from it rather than read off `duplex`. */
export function rigRepeaterParams(c: ProgChannel): {
  shift: 'simplex' | 'plus' | 'minus'
  offsetHz: number
  toneHz: number
} {
  const toneHz = c.toneMode === 'tone' || c.toneMode === 'tsql' ? c.rtoneHz : 0
  if (c.duplex === 'simplex') return { shift: 'simplex', offsetHz: 0, toneHz }
  if (c.duplex === 'split') {
    const diff = c.offsetMhz - c.rxMhz
    return {
      shift: diff >= 0 ? 'plus' : 'minus',
      offsetHz: Math.round(Math.abs(diff) * 1e6),
      toneHz,
    }
  }
  return { shift: c.duplex, offsetHz: Math.round(c.offsetMhz * 1e6), toneHz }
}

/** Name for a single starred machine: the way operators say it out loud — call
 * plus the frequency nickname, "W9ABC 94". Unlike the channel-list builder there
 * is no radio display cap to fit (Memories never truncates a name; caps apply at
 * export), and the tail keeps a club's 2 m and 70 cm machines apart in the
 * cockpit strip. Falls back to the record's own name when there's no callsign. */
export function favoriteName(c: ProgChannel): string {
  const call = (c.source?.callsign ?? '').split('/')[0]?.trim().toUpperCase() ?? ''
  return call ? `${call} ${freqTail(c.rxMhz)}` : c.name
}

/** The Memory a programmable repeater channel becomes — ONE mapping shared by
 * "★ this machine" and "save the whole list", so the two paths can never drift
 * into disagreeing about what a repeater is.
 *
 * `site` carries the machine's coordinates when the caller still has the
 * directory record (the picker rows do; a reloaded channel list does not, since
 * radioprog.json persists channels only). Stored so distance and bearing are
 * RECOMPUTED against wherever the operator is now — a baked-in mileage would be
 * wrong the first time they operated portable. */
export function repeaterMemory(
  c: ProgChannel,
  name: string,
  site?: { lat: number; lon: number },
): Partial<Memory> & { rxMhz: number; mode: string } {
  const { shift, offsetHz, toneHz } = rigRepeaterParams(c)
  return {
    name,
    rxMhz: c.rxMhz,
    mode: 'FM',
    kind: shift === 'simplex' && !toneHz ? 'simplex' : 'repeater',
    offsetDir: shift,
    offsetMhz: offsetHz ? offsetHz / 1e6 : undefined,
    toneMode: toneHz ? 'tone' : 'none',
    ctcssEncHz: toneHz || undefined,
    callsign: c.source?.callsign || undefined,
    lat: site?.lat,
    lon: site?.lon,
    source: 'program',
  }
}
