// Pure presentation helpers for the Propagation view (Mission-Control). Kept
// separate + unit-tested so the color/threshold/format logic is verifiable and
// the components stay declarative. Colors resolve to semantic tokens (DESIGN.md)
// except the heatmap, which uses the perceptual inferno LUT (dark=low, bright=high).
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every string this module
// RETURNS goes straight into a tooltip or a chip, so each is looked up when the string is
// built — these are functions, not module constants, so there is nothing to freeze. What
// stays here: band names, callsigns, grid squares, octants, km, MHz, dB, degrees, the ★/☆
// glyphs, and the `dualStateLabel` state WORD, which is the backend's `BandModeled` enum
// passed through and is compared against by `components/connect/paneFormat.ts`.
import { sampleLut } from './colormaps'
import { t } from './i18n'
import { STATUS, type StatusMeta } from './statusMeta'
import type {
  ActivityTier,
  BandModeled,
  GridRarity,
  Insight,
  InsightLevel,
  MapSpot,
  NeedKind,
  SatView,
  TrendDir,
} from './types'

/** Workability word → a semantic color token (`var(--…)`). */
export function workabilityVar(word: string): string {
  switch (word) {
    case 'Excellent':
    case 'Good':
      return 'var(--band-open)'
    case 'Fair':
      return 'var(--band-marginal)'
    case 'Marginal':
      return 'var(--snr-weak)'
    default: // Closed / unknown
      return 'var(--band-closed)'
  }
}

/** Activity tier → a semantic color token. Quiet/Closed are calm neutrals (NOT
 * red): red reads as an alert, but a quiet-yet-workable band is fine, and a
 * closed band should simply recede. Green/amber are reserved for real activity. */
export function tierVar(tier: ActivityTier): string {
  switch (tier) {
    case 'Active':
      return 'var(--band-open)' // green — real activity
    case 'Moderate':
      return 'var(--band-marginal)' // amber — some activity
    case 'Quiet':
      return 'var(--text-dim)' // neutral — open but quiet (gradient prior)
    default: // Closed
      return 'var(--text-faint)' // faint — recedes
  }
}

const NEED_ROLE: Record<NeedKind, keyof typeof STATUS> = {
  Atno: 'new-entity',
  NewBand: 'new-band',
  NewMode: 'new-mode',
  Confirm: 'confirmed',
  Satisfied: 'dupe',
}

/** Need tier → its color token + glyph + label (from the one statusMeta source). */
export function needMeta(need: NeedKind): StatusMeta {
  return STATUS[NEED_ROLE[need]]
}

/** A rarity gem's rendering, or null for tiers too common to decorate
 * (common/uncommon stay chipless — the board must not become confetti).
 * Tooltips are the explainability rule: rarity must never feel arbitrary. */
export function rarityMeta(
  r: GridRarity | null | undefined,
): { glyph: string; label: string; cls: string; title: string } | null {
  switch (r) {
    case 'rare':
      return {
        glyph: '◆',
        label: t('prop.rarity.rare.label'),
        cls: 'rare',
        title: t('prop.rarity.rare.title'),
      }
    case 'ultraRare':
      return {
        glyph: '◆◆',
        label: t('prop.rarity.ultra.label'),
        cls: 'ultra',
        title: t('prop.rarity.ultra.title'),
      }
    default:
      return null
  }
}

/** Likelihood score (0..1) → an `rgb(...)` fill from the perceptual inferno LUT. */
export function heatColor(score: number, alpha = 1): string {
  const [r, g, b] = sampleLut('inferno', Math.max(0, Math.min(1, score)))
  // `alpha` lets a caller sit the ramp BACK against the panel rather than on top
  // of it. The inferno LUT runs black → purple → red → orange → yellow, which is
  // right for a matrix you are reading deliberately and much too loud for one you
  // are scrolling past: the operator's words for the DXpedition calendar were
  // "really loud ... a lot of yellows, oranges, and reds, and that really
  // dominates the whole screen". Blending toward the background keeps the ranking
  // legible while giving the page back to the text. Full strength is still the
  // default, so ActivityMatrix is untouched.
  return alpha >= 1 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${alpha})`
}

/** UTC hour (0–23) → "14Z". */
export function fmtZ(hour: number): string {
  return `${String(((hour % 24) + 24) % 24).padStart(2, '0')}Z`
}

/** Current UTC hour (0–23). Not pure — kept out of the tested set. */
export function nowUtcHour(): number {
  return new Date().getUTCHours()
}

export type Severity = 'quiet' | 'active' | 'warn'
export interface Impact {
  sev: Severity
  text: string
}

/** Plain-language HF impact for a space-weather index (numbers stay visible in the UI). */
export function sfiImpact(sfi: number): Impact {
  if (sfi >= 150) return { sev: 'active', text: t('prop.impact.sfi.high') }
  if (sfi >= 100) return { sev: 'active', text: t('prop.impact.sfi.moderate') }
  return { sev: 'quiet', text: t('prop.impact.sfi.low') }
}
export function kpImpact(kp: number): Impact {
  if (kp >= 5) return { sev: 'warn', text: t('prop.impact.kp.storm') }
  if (kp >= 4) return { sev: 'warn', text: t('prop.impact.kp.unsettled') }
  return { sev: 'quiet', text: t('prop.impact.kp.quiet') }
}
/** The model's "usable" (≥ Fair) cutoff — mirrors likelihood.rs Workability::from_score.
 * A per-UTC-hour likelihood at/above this reads as an open hour. */
export const OPEN_THRESHOLD = 0.3

/** Live timing for one outlook band: is it open THIS hour (and for how much longer), or when
 * does it next open? `hourly` is 24 per-UTC-hour likelihoods. '' when unknown/never-open.
 * The outlook shows peak workability + best window; this answers "…but is it open NOW?". */
export function bandTiming(hourly: number[], nowMs: number): string {
  if (!hourly || hourly.length < 24) return ''
  const d = new Date(nowMs)
  const nowH = d.getUTCHours()
  const nowMin = d.getUTCMinutes()
  const open = (h: number) => (hourly[((h % 24) + 24) % 24] ?? 0) >= OPEN_THRESHOLD
  if (open(nowH)) {
    let left = 0
    while (left < 24 && open(nowH + left)) left++
    const remMin = left * 60 - nowMin
    return remMin >= 90
      ? t('prop.bandTiming.openNowHours', { hours: Math.round(remMin / 60) })
      : t('prop.bandTiming.openNowMins', { mins: remMin })
  }
  for (let ahead = 1; ahead <= 24; ahead++) {
    if (open(nowH + ahead)) {
      const mins = ahead * 60 - nowMin
      const when = mins < 60 ? `${mins}m` : `${Math.floor(mins / 60)}h${mins % 60 ? ` ${mins % 60}m` : ''}`
      const z = `${String((nowH + ahead) % 24).padStart(2, '0')}00Z`
      return t('prop.bandTiming.opensIn', { when, at: z })
    }
  }
  return ''
}

/** IMF Bz (nT) — the leading geomagnetic signal (leads Kp by hours). Southward (negative)
 * couples solar-wind energy in: <=-10 strongly geoeffective, -10..-5 unsettled, else benign. */
export function bzImpact(bz: number): Impact {
  if (bz <= -10) return { sev: 'warn', text: t('prop.impact.bz.hardSouth') }
  if (bz <= -5) return { sev: 'warn', text: t('prop.impact.bz.south') }
  return { sev: 'quiet', text: t('prop.impact.bz.neutral') }
}
/** A-index (24 h average of geomagnetic activity — the day's character, where Kp is
 * the last 3 h). NOAA scale: <8 quiet · 8–15 unsettled · 16–29 active · 30+ storm. */
export function aImpact(a: number): Impact {
  if (a >= 30) return { sev: 'warn', text: t('prop.impact.a.storm') }
  if (a >= 16) return { sev: 'warn', text: t('prop.impact.a.active') }
  if (a >= 8) return { sev: 'active', text: t('prop.impact.a.unsettled') }
  return { sev: 'quiet', text: t('prop.impact.a.quiet') }
}
export function xrayImpact(cls: string): Impact {
  const c = cls.trim().charAt(0).toUpperCase()
  if (c === 'X' || c === 'M') return { sev: 'warn', text: t('prop.impact.xray.flare') }
  if (c === 'C') return { sev: 'active', text: t('prop.impact.xray.cClass') }
  return { sev: 'quiet', text: t('prop.impact.xray.none') }
}

// ───────────────── nerve-center: modeled state, trend, insights ─────────────────

/** Modeled openness → a band-state color token (green / amber / red). */
export function modeledVar(m: BandModeled): string {
  switch (m) {
    case 'Open':
      return 'var(--band-open)'
    case 'Marginal':
      return 'var(--band-marginal)'
    default: // Closed
      return 'var(--band-closed)'
  }
}

/** Insight level → a semantic color token. */
export function insightLevelVar(level: InsightLevel): string {
  switch (level) {
    case 'good':
      return 'var(--band-open)'
    case 'caution':
      return 'var(--alert-warning)'
    case 'alert':
      return 'var(--snr-weak)'
    default: // info
      return 'var(--text-dim)'
  }
}

/** Stable sort, most-prominent first: alert → caution → good → info. */
export function sortInsights(xs: Insight[]): Insight[] {
  const rank: Record<InsightLevel, number> = { alert: 0, caution: 1, good: 2, info: 3 }
  return xs
    .map((x, i) => [x, i] as const)
    .sort((a, b) => rank[a[0].level] - rank[b[0].level] || a[1] - b[1])
    .map(([x]) => x)
}

/** Trend direction → a glyph. */
export function trendArrow(dir: TrendDir): string {
  return dir === 'rising' ? '↑' : dir === 'falling' ? '↓' : '→'
}

/** Trend direction → a color token (rising reads positive in MUF/SFI context). */
export function trendVar(dir: TrendDir): string {
  return dir === 'rising'
    ? 'var(--band-open)'
    : dir === 'falling'
      ? 'var(--snr-weak)'
      : 'var(--text-dim)'
}

// Highest band whose nominal frequency sits at/below the MUF (the ceiling band).
// Mirrors the backend `band_at_or_below`; HF + 6m.
const MUF_LADDER: ReadonlyArray<readonly [number, string]> = [
  [1.9, '160m'],
  [3.6, '80m'],
  [5.36, '60m'],
  [7.1, '40m'],
  [10.13, '30m'],
  [14.1, '20m'],
  [18.1, '17m'],
  [21.2, '15m'],
  [24.9, '12m'],
  [28.5, '10m'],
  [50.2, '6m'],
]

/** Which band the MUF ceiling sits at (e.g. 22 MHz → "15m"); "" if below/at the floor. */
export function mufCeilingBand(mufMhz: number): string {
  if (!(mufMhz > 0)) return ''
  let label = ''
  for (const [f, l] of MUF_LADDER) {
    if (f <= mufMhz) label = l
    else break
  }
  return label
}

/** Combine MODELED openness + OBSERVED tier into the dual-state label that kills the
 * false "quiet = dead" reading: a band the model says is Open but with no spots reads
 * "Open · none heard", never "Quiet"/"dead". */
export function dualStateLabel(
  modeled: BandModeled | undefined,
  tier: ActivityTier,
): { word: string; sub: string } {
  // ⚠️ The WORD is the backend's `BandModeled` enum passed straight through, and
  // `components/connect/paneFormat.ts` compares against it (`!== 'Closed'`). It moves with
  // the rest of the backend's vocabulary, not here. The SUB-NOTE is ours.
  //
  // Observed activity PROVES the band is open, regardless of what the model says.
  if (tier === 'Active') return { word: 'Open', sub: t('prop.state.sub.active') }
  if (tier === 'Moderate') return { word: 'Open', sub: t('prop.state.sub.someActivity') }
  // Silent band: defer to the model. Open-but-unheard reads "Open · none heard" — the
  // key fix so a quiet band never reads as dead.
  const m: BandModeled = modeled ?? 'Open'
  if (m === 'Closed') return { word: 'Closed', sub: '' }
  return { word: m, sub: t('prop.state.sub.noneHeard') }
}

/** The map hover-tooltip line for a live cluster/RBN/PSKR spot — who/where/what
 * (call · band mode · freq · age · heard-you · ~location). Shared by the 2-D map
 * and the 3-D globe so both read identically. Any work-gesture hint is appended by
 * the caller (only the 2-D map has that double-click-to-work gesture). */
export function spotTooltip(sp: MapSpot): string {
  const age = sp.ageSecs < 60 ? `${sp.ageSecs}s` : `${Math.round(sp.ageSecs / 60)}m`
  const freq = sp.freqMhz ? ` · ${sp.freqMhz.toFixed(4).replace(/\.?0+$/, '')} MHz` : ''
  const mode = sp.mode ? ` ${sp.mode}` : ''
  const line = t('prop.spotTooltip', { call: sp.call, band: sp.band, mode, freq, age })
  return `${line}${sp.heardMe ? t('prop.spotTooltip.heardMe') : ''}${sp.approx ? t('prop.spotTooltip.approx') : ''}`
}

/** The map hover-tooltip line for an amateur satellite — which bird, ★ or not,
 * how high it is right now, and when it next comes over the operator.
 * `clickable` appends the select-for-passes hint (only the full map has that
 * gesture; the embedded detail globe does not).
 *
 * The altitude carries the word "alt" because this map's OTHER km figure is a
 * station's distance from the operator: an unlabelled "1234 km" under the
 * cursor would be read as range, and for a satellite those are wildly
 * different numbers. It is the live height off the `birds` row, never a
 * nominal orbit altitude — an elliptical bird's varies by hundreds of km
 * across one orbit, which is exactly what the operator is looking at. A bird
 * nothing carries elements for has no row, so it simply says nothing. */
export function satTooltip(
  name: string,
  chased: boolean,
  sats: SatView | null,
  nowSecs: number,
  clickable: boolean,
): string {
  const star = chased ? '★' : '☆'
  const bird = sats?.birds.find((b) => b.name === name)
  const alt = bird ? t('prop.satTooltip.alt', { km: Math.round(bird.altKm) }) : ''
  const pass = sats?.passes.find((pp) => pp.name === name && pp.losUnix > nowSecs)
  let when = t('prop.satTooltip.noPass')
  if (pass) {
    const at = new Date(pass.aosUnix * 1000)
    const hhmm = `${at.getHours().toString().padStart(2, '0')}:${at.getMinutes().toString().padStart(2, '0')}`
    when =
      pass.aosUnix <= nowSecs
        ? t('prop.satTooltip.inPass', { maxEl: Math.round(pass.maxElDeg) })
        : t('prop.satTooltip.nextPass', {
            at: hhmm,
            mins: Math.max(1, Math.round((pass.aosUnix - nowSecs) / 60)),
            maxEl: Math.round(pass.maxElDeg),
          })
  }
  return t('prop.satTooltip', {
    name,
    star,
    alt,
    when,
    click: clickable ? t('prop.satTooltip.clickForPasses') : '',
  })
}
