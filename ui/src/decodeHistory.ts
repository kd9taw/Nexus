import type { DecodeRow, Tier } from './types'

// Pure model behind the Band Activity / Rx Frequency panes (OperateDecodes).
// Kept DOM-free so the WSJT-X-critical behaviors — chronological flow, the
// Rx-Frequency pane filter, and the band/tier history wipe — are unit-testable.

export type DecodeFilter = 'all' | 'cq' | 'cq73' | 'me' | 'rx' | 'b4' | 'new'
/** The filter chips in bar order. A runtime list so the bar OperateDecodes renders and the
 *  validator that sanitizes the persisted chip (operateFilters.ts) cannot drift apart — a
 *  chip added here appears in both, instead of one being silently reset on restart. */
export const DECODE_FILTERS: readonly DecodeFilter[] = ['all', 'cq', 'cq73', 'me', 'rx', 'b4', 'new']
export type DecodeSort = 'time' | 'snr' | 'freq' | 'dt'

/** A decode plus the slot + wall-clock time it was first heard (history bookkeeping). */
export interface DecodeEntry extends DecodeRow {
  slot: number
  /** Epoch ms when first heard — drives the per-row UTC column. */
  at: number
  /** Stable dedupe key (slot-scoped) — doubles as the React row key. */
  id: string
}

/** Rolling history cap (oldest rows dropped first). */
export const MAX_HISTORY = 300
/** "On RX freq" tolerance (Hz) — decodes within this of the RX marker. */
export const RX_TOL_HZ = 50
/** Fallback own-TX cycle key width (ms) when a mine row carries no txAt. */
const SLOT_MS = 15_000

/** T/R period per tier (s) — mirrors the engine's modes::slot_secs. The slot
 * index is floor(epoch_ms / period_ms), so slot × period = the period's UTC start. */
export const TIER_PERIOD_SECS: Record<Tier, number> = {
  FT8: 15,
  FT4: 7.5,
  // FST4/FST4W periods are an operator setting (15..1800); these are the
  // defaults. A decode row's true period comes from the ModeKind the engine
  // tagged it with.
  FST4: 15,
  FST4W: 120,
  MSK144: 15,
  JT65: 60,
  WSPR: 120,
  // Q65's period is an operator setting (15/30/60/120/300), so this table cannot
  // state it. 60 is the default and the EME working period; a decode row's true
  // period comes from the ModeKind the engine tagged it with.
  Q65: 60,
  TempoFast: 4,
  TempoDeep: 15,
}

/** UTC epoch-ms of a slot's period start (engine slots count from the Unix epoch). */
export function periodStartMs(slot: number, tier: Tier): number {
  return slot * TIER_PERIOD_SECS[tier] * 1000
}

/** UTC HHMMSS (matches WSJT-X's compact time column / period separator). */
export function fmtUtc(atMs: number): string {
  const d = new Date(atMs)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getUTCHours())}${p(d.getUTCMinutes())}${p(d.getUTCSeconds())}`
}

/**
 * One pane filter predicate. The 'rx' case is the Rx Frequency pane: WSJT-X
 * shows our own TX, EVERY message addressed to MyCall (regardless of audio
 * offset — a caller off-frequency must never be missed), and anything within
 * ±RX_TOL_HZ of the RX marker.
 */
export function passesFilter(d: DecodeRow, filter: DecodeFilter, rxOffsetHz: number): boolean {
  switch (filter) {
    case 'cq':
      return d.isCq
    case 'cq73':
      // The CQ view plus RR73/73 signoffs (tester request): a signoff means that
      // frequency is about to free up. Plain 'cq' above stays signoff-free so the
      // chip existing operators use doesn't change under them. The flag is the
      // engine's parse-typed classification (Msg::is_signoff), not a text match.
      return d.isCq || Boolean(d.signoff)
    case 'me':
      return d.directedToMe
    case 'rx':
      return Boolean(d.mine) || d.directedToMe || Math.abs(d.freqHz - rxOffsetHz) <= RX_TOL_HZ
    case 'b4':
      return d.worked
    case 'new':
      // STRICTLY new ones (entity/grid) — a plain unworked CQ is not a "new one",
      // and including it diluted the chaser's most-used filter into noise.
      return Boolean(d.newDxcc || d.newGrid)
    default:
      return true
  }
}

/** Sort for display. 'time' is WSJT-X chronological: OLDEST first, newest at
 * the bottom (slot order, then first-heard order within a slot). */
export function orderEntries(list: DecodeEntry[], sort: DecodeSort): DecodeEntry[] {
  const out = [...list]
  out.sort((a, b) => {
    switch (sort) {
      case 'snr':
        return b.snr - a.snr
      case 'freq':
        return a.freqHz - b.freqHz
      case 'dt':
        return a.dtSec - b.dtSec
      default:
        return a.slot - b.slot || a.at - b.at
    }
  })
  return out
}

/**
 * The per-pane rolling history. WSJT-X flow: every period appends its decodes
 * as new lines (a station re-heard NEXT period gets a NEW row — "I see him
 * calling every cycle"); within one period, snapshot re-polls of the same
 * decode list dedupe to a single row (slot-scoped keys). Changing band or tier
 * wipes the pane — stale other-band rows are a mis-operation hazard.
 */
export class DecodeHistory {
  private map = new Map<string, DecodeEntry>()
  private scope: string | null = null
  /** The last NAMED band this pane was bound to — what an off-band dial keeps pointing at. */
  private lastBand = ''

  /**
   * Bind the history to a band+tier; a change WIPES it. Returns true if reset.
   *
   * AN OFF-BAND EXCURSION IS NOT A BAND CHANGE (operator ruling 2026-08-13: listening off the
   * ham bands is a first-class use case). Off the bands the backend sends `radio.band` as the
   * empty string — "no ham-band claim" — and keyed raw that empty label was a band NAME: a
   * twenty-second listen to WWV off the edge of 40m wiped the pane on the way out AND again on
   * the way back, undoing at the UI layer the backend's own rule that remembers the band it left
   * (`off_band_from`) so returning to it is a return, not a QSY. So the band component ignores an
   * empty label and keeps the last named band, which makes every off-band frequency one scope —
   * the same treatment tuning WITHIN a band already gets, and an off-band excursion is the same
   * kind of move. A TIER change still wipes regardless of where the dial is.
   */
  setScope(band: string, tier: Tier): boolean {
    const named = band.trim() ? band : this.lastBand
    this.lastBand = named
    const key = `${named}|${tier}`
    if (this.scope === key) return false
    this.scope = key
    this.map.clear()
    return true
  }

  /** Ingest one snapshot poll's decode list for the given slot. */
  ingest(decodes: DecodeRow[], slot: number, now: number = Date.now()): void {
    const m = this.map
    for (const d of decodes) {
      // Own TX rows key by the engine's actual TRANSMIT time (txAt), so the same
      // transmission re-emitted across poll boundaries stays ONE row and each
      // new cycle is a new row. Received decodes key per (slot, message, ~freq):
      // re-polls dedupe, re-transmissions next period append (WSJT-X flow).
      const id = d.mine
        ? `mine|${d.txAt ?? Math.floor(now / SLOT_MS)}`
        : `${slot}|${d.message}|${Math.round(d.freqHz / 5)}`
      const prev = m.get(id)
      // Keep the first-heard timestamp (chronological position is stable);
      // own-TX rows timestamp by their transmit cycle, not the ingest clock.
      const at = d.mine && d.txAt ? d.txAt * 1000 : (prev?.at ?? now)
      // #15: an own-TX row EVICTED by the row cap and re-served from the engine's
      // 30-deep TX ring must not land in the CURRENT period. It keeps its transmit
      // time (`at` above), but with no surviving entry to inherit a slot from it
      // used the INGEST slot — so a busy band resurrected old CQ calls under the
      // live period separator with stale clock times ("old calls duplicated in
      // the timeline"). Pin it to the period its own transmit time names.
      // (SLOT_MS is the FT8 period; on FT4 the estimate lands an older period —
      // never the live one, which is the failure that matters.)
      const slotFor =
        prev?.slot ??
        (d.mine && d.txAt != null
          ? slot - Math.max(0, Math.round((now - d.txAt * 1000) / SLOT_MS))
          : slot)
      m.set(id, { ...d, slot: slotFor, at, id })
    }
    if (m.size > MAX_HISTORY) {
      const drop = m.size - MAX_HISTORY
      const it = m.keys()
      for (let i = 0; i < drop; i++) m.delete(it.next().value as string)
    }
  }

  /** Wipe this pane (WSJT-X "Erase"). */
  erase(): void {
    this.map.clear()
  }

  /** All entries in insertion (≈ chronological) order. */
  entries(): DecodeEntry[] {
    return Array.from(this.map.values())
  }
}
