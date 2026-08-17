// Pure prior-QSO summary for a callsign — no React, no IO, fully node-testable. The cockpit
// log strip loads the full log once (getLog) and feeds it here per typed call to answer the
// DXer questions: have I worked them, on this band (dupe), when last, how confirmed.

import type { LoggedQso } from '../types'
import { bandLabelForMhz, bandRangeForLabel } from '../band'

export interface CallHistory {
  /** Prior QSOs with this call (in the order given by the log). */
  qsos: LoggedQso[]
  count: number
  /** Worked at least once before (B4). */
  workedBefore: boolean
  /** Already worked on the CURRENT band (a dupe for this band). */
  dupeThisBand: boolean
  /** Most recent contact time (Unix seconds), or null if never worked. */
  lastUnix: number | null
  /** How many prior QSOs are confirmed (any channel). */
  confirmedCount: number
  /** Distinct bands worked, first-seen order. */
  bands: string[]
  /** Distinct modes worked, first-seen order. */
  modes: string[]
}

const EMPTY: CallHistory = {
  qsos: [],
  count: 0,
  workedBefore: false,
  dupeThisBand: false,
  lastUnix: null,
  confirmedCount: 0,
  bands: [],
  modes: [],
}

/** The award-identity key for a log row: the cty.dat-RESOLVED `entity` when the
 * backend supplied one, else the stored free-text `country` as a legacy
 * fallback. ONE key function for every entity comparison — keying on the raw
 * country string made the two identity systems disagree (QRZ writes "Germany"/
 * "Russia"; cty.dat says "Fed. Rep. of Germany"/"European Russia"), so the
 * NEW ONE badge fired on every German and Russian contact forever and the
 * Statistics and Awards entity counts could never match. Comparison only —
 * never display. */
export function entityKey(q: { entity?: string | null; country?: string | null }): string {
  return (q.entity ?? q.country ?? '').trim().toUpperCase()
}

/** The slot identity of a MODE: spellings of one identical mode fold together, and nothing
 * else does.
 *
 * ⚠️ THIS IS DELIBERATELY NOT `reconcile::mode_class`. That buckets everything into
 * CW / Phone / Digital, which is the granularity DXCC awards at — correct for matching a LoTW
 * confirmation, and far too coarse here. Folding to classes would tell an operator who has
 * worked an entity on FT8 that FT4 is not a new mode, and that an FM contact covers SSB. The
 * board shows specific modes and must keep doing so.
 *
 * What folds, and why each one is the SAME mode rather than a near neighbour:
 *   USB / LSB → SSB   ADIF models both as SUBMODEs of SSB; a contact on USB and one on LSB
 *                     are two SSB contacts, so badging "new mode — LSB" is simply wrong.
 *   BPSK31 → PSK31,   the same waveform under two spellings, both common in imported logs.
 *   BPSK63 → PSK63
 *
 * What does NOT fold, on purpose: FM and AM stay distinct from SSB (different modes, however
 * DXCC groups them); FT4 stays distinct from FT8. And the genuinely ambiguous tokens are left
 * exactly as they are rather than guessed at — a bare `MFSK` row may be FT4, JS8 or something
 * else entirely (the import promotes ADIF SUBMODE, so current imports store `FT4` directly;
 * bare MFSK is older residue), and `PH` is N3FJP's generic phone token which may have been FM.
 * Mapping either of those would invent a contact the operator never made. */
export function modeKey(mode: string | null | undefined): string {
  const m = (mode ?? '').trim().toUpperCase()
  switch (m) {
    case 'USB':
    case 'LSB':
      return 'SSB'
    case 'BPSK31':
      return 'PSK31'
    case 'BPSK63':
      return 'PSK63'
    default:
      return m
  }
}

/** The slot identity of a BAND: the stored token when it names a real band, else derived from
 * the contact's own frequency, else `null` for "cannot be known".
 *
 * Two things combined into a permanent false badge. The stored token is free text and does not
 * always name a band this app knows (`-fm` suffixes and similar), and the frequency was never
 * consulted as a fallback — so a row that did not parse contributed a token matching nothing,
 * and the live band read as new against it forever, on every poll, with no way for an operator
 * to clear it by operating.
 *
 * `null` is the honest third answer and is not the same as "not worked": a real imported log can
 * carry rows with an unparseable band AND `FREQ 0`, where the band the contact was made on is
 * genuinely unrecoverable. See [`entitySlots`] for what that then suppresses. */
export function bandKey(q: { band?: string | null; freqMhz?: number | null }): string | null {
  const token = (q.band ?? '').trim()
  if (token && bandRangeForLabel(token.toLowerCase())) return token.toUpperCase()
  const mhz = q.freqMhz ?? 0
  if (mhz > 0) {
    const derived = bandLabelForMhz(mhz)
    if (derived) return derived.toUpperCase()
  }
  return null
}

/** True only when the entity is KNOWN (non-empty resolved entity, or country as
 * the caller's fallback) and no log row's award identity matches it — never
 * claims "new DXCC" for a blank/unresolved entity. */
export function isNewEntity(
  log: { entity?: string | null; country?: string | null }[],
  entity: string | null | undefined,
): boolean {
  const c = (entity ?? '').trim().toUpperCase()
  if (!c) return false
  return !log.some((q) => entityKey(q) === c)
}

export interface EntitySlots {
  /** The entity (country) has at least one prior QSO in the log. */
  workedEver: boolean
  /** Distinct bands worked for the entity, resolved through [`bandKey`], first-seen order. */
  bandsWorked: string[]
  /** Distinct modes worked for the entity, folded through [`modeKey`], first-seen order. */
  modesWorked: string[]
  /** At least one contact with this entity has a band that cannot be determined at all — an
   * unparseable token and no frequency to fall back on, which a real imported log does carry.
   *
   * The caller must not raise a New Band badge while this is set. We know the operator has
   * worked the entity and we do NOT know on which band, so "you have never worked them here"
   * is a claim the data does not support. Silence is the honest answer; the alternative is a
   * badge that is wrong for as long as the row exists and that no amount of operating clears. */
  bandUnknown: boolean
}

/** Per-ENTITY band/mode-slot summary — the DXCC-Challenge axis that per-call
 * `callHistory` can't answer: "I've worked this country, but is THIS band (or mode)
 * a new slot for it?". Matches the entity on `country` case-insensitively, exactly as
 * `isNewEntity` does (a blank/unresolved country never counts as worked). Bands and modes
 * are normalized (trim + UPPER) so membership tests tolerate case/whitespace; they are used
 * only for comparison, never displayed. */
export function entitySlots(
  log: {
    entity?: string | null
    country?: string | null
    band?: string | null
    mode?: string | null
    freqMhz?: number | null
  }[],
  entity: string | null | undefined,
): EntitySlots {
  const c = (entity ?? '').trim().toUpperCase()
  if (!c) {
    return { workedEver: false, bandsWorked: [], modesWorked: [], bandUnknown: false }
  }
  const bandsWorked: string[] = []
  const modesWorked: string[] = []
  let workedEver = false
  let bandUnknown = false
  for (const q of log) {
    if (entityKey(q) !== c) continue
    workedEver = true
    // Through the SAME two key functions the live side uses. Comparing the raw stored strings
    // is what produced the false badges: a band token that named no band matched nothing, and
    // USB against a log full of LSB read as a mode never worked.
    const b = bandKey(q)
    const m = modeKey(q.mode)
    if (b === null) {
      // Only when the row genuinely carried a band we could not place. A row with no band at
      // all is a different thing and stays silent as it always did.
      if ((q.band ?? '').trim() || (q.freqMhz ?? 0) > 0) bandUnknown = true
    } else if (!bandsWorked.includes(b)) {
      bandsWorked.push(b)
    }
    if (m && !modesWorked.includes(m)) modesWorked.push(m)
  }
  return { workedEver, bandsWorked, modesWorked, bandUnknown }
}

/** Summarize a call's prior contacts from the full log. Case-insensitive on the call;
 * `band` is the current operating band for the dupe check (pass '' to skip it). */
/**
 * `mode` + `matchMode`: the Dupe badge's scope (operator-relayed report, 2026-08-16 — a
 * 'Dupe 40m' shown for a station worked on 40m FT8 while running 40m phone). Default is
 * call+band, WSJT-X's own default scope; passing `matchMode: true` (Settings ▸
 * b4MatchMode) requires the MODE to match too, so a cross-mode contact is not a dupe.
 */
export function callHistory(
  log: LoggedQso[],
  call: string,
  band: string,
  mode = '',
  matchMode = false,
): CallHistory {
  const c = call.trim().toUpperCase()
  if (!c) return EMPTY
  const qsos = log.filter((q) => q.call.trim().toUpperCase() === c)
  if (qsos.length === 0) return EMPTY

  const bands: string[] = []
  const modes: string[] = []
  let lastUnix = 0
  let confirmedCount = 0
  let dupeThisBand = false
  for (const q of qsos) {
    if (q.band && !bands.includes(q.band)) bands.push(q.band)
    if (q.mode && !modes.includes(q.mode)) modes.push(q.mode)
    if (q.whenUnix > lastUnix) lastUnix = q.whenUnix
    if (q.confirmed) confirmedCount++
    // Case-folded like every sibling (band_key, dedup_key, reconcile): LoTW
    // exports spell bands uppercase, so a raw compare left the dupe cue dark
    // for every imported contact.
    if (band && (q.band ?? '').trim().toLowerCase() === band.trim().toLowerCase()) {
      const modeOk =
        !matchMode || (q.mode ?? '').trim().toUpperCase() === mode.trim().toUpperCase()
      if (modeOk) dupeThisBand = true
    }
  }
  return {
    qsos,
    count: qsos.length,
    workedBefore: true,
    dupeThisBand,
    lastUnix,
    confirmedCount,
    bands,
    modes,
  }
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/** One-line human-readable summary of prior contacts for the recall panel — the casual-first
 * differentiator ("3 QSOs — last on 20m SSB, 14 Mar 2026"), or a first-contact cue. Dates are
 * shown in UTC, the ham-log convention. */
export function historySummary(hist: CallHistory): string {
  if (!hist.workedBefore || hist.count === 0) return 'First contact — new station!'
  const last = hist.qsos.reduce((a, b) => (b.whenUnix > a.whenUnix ? b : a))
  const d = new Date(last.whenUnix * 1000)
  const date = `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`
  const where = [(last.band ?? '').trim(), (last.mode ?? '').trim()].filter(Boolean).join(' ')
  const label = hist.count === 1 ? 'QSO' : 'QSOs'
  return where
    ? `${hist.count} ${label} — last on ${where}, ${date}`
    : `${hist.count} ${label} — last ${date}`
}
