// THE BAND DROPDOWN'S CONDITIONS — the latest band-condition advisory, published once by the
// app's existing propagation poll and read by every band menu (BandPicker, FrequencyControl).
//
// ONE SOURCE. The cell a band menu draws comes from `bandConditionCell`, the helper the Band
// conditions strip draws its cells with, over the same `PropagationSnapshot.advisory.bands`. A
// second derivation here would let the dropdown say "Open" while the strip beside the map says
// "Closed", which is worse than no colour at all.
//
// A MODULE STORE, NOT A PROP. The band control is injected into seven cockpit headers and the
// TopBar; threading the advisory through each would touch every one of them for a read-only
// decoration. App already polls `get_propagation`; it publishes here, and the menus subscribe.
//
// ⚠️ UNKNOWN IS NEUTRAL, NEVER GREEN. No snapshot (the Remote browser, whose allowlist refuses
// `get_propagation`; a pop-out window before its first poll), an `offline` snapshot, a snapshot
// older than BAND_CONDITIONS_STALE_S, or a band the advisory did not report (2 m, 70 cm, 160 m
// on a quiet night) all resolve to `unknown` — a hollow dot and a neutral word. Showing "Open"
// on data we do not have is the one way this feature could mislead an operator.
import { useSyncExternalStore } from 'react'
import type { BandReport, PropagationSnapshot } from './types'
import { bandConditionCell } from './propViz'
import { t } from './i18n'

/** Older than this and the advisory is not shown. The snapshot refreshes every 5 minutes, so
 *  20 minutes is three missed refreshes: past it the colours would describe an earlier sky. */
export const BAND_CONDITIONS_STALE_S = 20 * 60

interface Published {
  bands: Map<string, BandReport>
  asOf: number
  source: PropagationSnapshot['source']
}

let current: Published | null = null
const listeners = new Set<() => void>()

/** Publish the latest propagation snapshot (or null to clear). Called from the app's poll. */
export function publishBandConditions(p: PropagationSnapshot | null): void {
  current = p
    ? { bands: new Map((p.advisory?.bands ?? []).map((b) => [b.band, b])), asOf: p.asOf, source: p.source }
    : null
  for (const l of listeners) l()
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

export type BandConditionState = 'open' | 'marginal' | 'closed' | 'unknown'

export interface BandCondition {
  state: BandConditionState
  /** The word shown beside the dot. */
  word: string
  /** The dot colour (a theme token); null for unknown, which draws a hollow neutral dot. */
  color: string | null
  /** Hover text: the strip's own cell title when known. */
  title: string
}

function resolve(pub: Published | null, band: string, nowS: number): BandCondition {
  const report = pub && pub.source !== 'offline' && nowS - pub.asOf <= BAND_CONDITIONS_STALE_S ? pub.bands.get(band) : undefined
  if (!report) {
    return { state: 'unknown', word: t('bandMenu.condition.unknown'), color: null, title: t('bandMenu.condition.unknown.title') }
  }
  const cell = bandConditionCell(report)
  const state: BandConditionState = cell.word === 'Closed' ? 'closed' : cell.word === 'Marginal' ? 'marginal' : 'open'
  return {
    state,
    word: cell.word,
    color: cell.color,
    title: t('prop.bandConditions.cell.title', { band, state: cell.word, sub: cell.sub ? ` · ${cell.sub}` : '', reason: report.reason }),
  }
}

/** The condition lookup for the current advisory. Re-renders when a new snapshot is published. */
export function useBandConditions(): (band: string) => BandCondition {
  const pub = useSyncExternalStore(subscribe, () => current, () => current)
  const nowS = Math.floor(Date.now() / 1000)
  return (band: string) => resolve(pub, band, nowS)
}
