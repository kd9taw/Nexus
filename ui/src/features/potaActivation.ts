// The activator's side of POTA, as the screen reads it — no React, no IO, fully node-testable.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
//
// What the station counts lives in the hot index (`tempo_core::logbook::hot::pota_key`): the
// contacts of one park on one UTC day, each `(call, band, mode)` once — a hunter's two-fer, or the
// same hunter again from another park, is one contact toward the ten. The warn-only dupe badge in
// the log form asks a wider question of the prior contacts it already holds for the typed call:
// whether this contact would add any row the upload credits, which is per park of theirs
// (`hot::their_pota_parks`, one export row each). So a hunter calling again from a NEW park of
// theirs is no dupe here though it adds nothing to the count: they need it for their own P2P.

import { t } from '../i18n'
import type { Activation, ActivationPark, LoggedActivation, LoggedQso } from '../types'
import { modeKey } from './callHistory'
import type { LogFilter } from './logFilter'

/** Contacts POTA requires from one park in one UTC day for a valid activation. */
export const POTA_VALID_QSOS = 10

const DAY = 86_400

/** The parks of a reference list ("US-0001,US-0002"): split on the separators the export reads,
 *  trimmed and upper-cased, empties dropped. */
export function parkRefs(list: string | null | undefined): string[] {
  return (list ?? '')
    .split(/[,;]/)
    .map((p) => p.trim().toUpperCase())
    .filter((p) => p !== '')
}

/** The upload rows a contact makes, as keys: the call exactly as logged (W1ABC/P is not W1ABC),
 *  the band, the mode through the log's fold (USB and LSB are SSB) — the counter's key
 *  (`pota_key` in the hot index) — and one POTA park of the other station's: one key per park of a
 *  hunter's two-fer, as the per-activation export writes one row each (`their_pota_parks`); one key
 *  with no park when they named none, or a reference of another program. */
export function potaKeys(
  call: string,
  band: string,
  mode: string,
  theirProgram: string | null | undefined,
  theirRef: string | null | undefined,
): string[] {
  const head = [call.trim().toUpperCase(), band.trim().toLowerCase(), modeKey(mode)]
  const theirs = (theirProgram ?? '').trim().toUpperCase() === 'POTA' ? [...new Set(parkRefs(theirRef))] : []
  return (theirs.length ? theirs : ['']).map((park) => [...head, park].join('\u0001'))
}

/** The parks of the activation running now; empty when not activating. */
export function activationParks(act: Activation | null | undefined): string[] {
  if (!act?.reference) return []
  return act.parks?.length ? act.parks.map((p) => p.reference) : parkRefs(act.reference)
}

export interface PotaDupe {
  /** The park of the activation the earlier contact was made from. */
  park: string
  /** The earlier contact. */
  qso: LoggedQso
}

/** ⭐ The warn-only POTA dupe: earlier contacts with this call, made on the contact's UTC day
 *  (`whenUnix`: now, or the time typed for a back-dated one) from a park of the activation running
 *  now, already holding every POTA key this one would add — one POTA counts once. `null` when not
 *  activating (or paused: nothing is tagged) or there is none. It never blocks: the hunter may
 *  still need the contact, and the upload check drops a dupe harmlessly.
 *
 *  `qsos` are the typed call's prior contacts (the history answer's — the call already matches,
 *  trimmed and upper-cased); `call` is checked again so a caller cannot pass another's. */
export function potaDupe(
  qsos: readonly LoggedQso[],
  act: Activation | null | undefined,
  entry: {
    call: string
    band: string
    mode: string
    theirProgram?: string | null
    theirRef?: string | null
    whenUnix: number
  },
): PotaDupe | null {
  const parks = act?.restored ? [] : activationParks(act)
  if (parks.length === 0 || !entry.band) return null
  const day = Math.floor(entry.whenUnix / DAY)
  const want = potaKeys(entry.call, entry.band, entry.mode, entry.theirProgram, entry.theirRef)
  const held = new Set<string>()
  let found: PotaDupe | null = null
  for (const q of qsos) {
    if (Math.floor(q.whenUnix / DAY) !== day) continue
    const park = parkRefs(q.ota?.myRef).find((p) => parks.includes(p))
    if (!park) continue
    const keys = potaKeys(q.call, q.band ?? '', q.mode ?? '', q.ota?.theirProgram, q.ota?.theirRef)
    if (!keys.some((k) => want.includes(k))) continue
    for (const k of keys) held.add(k)
    // The latest, so the badge names the contact the operator most likely remembers.
    if (!found || q.whenUnix > found.qso.whenUnix) found = { park, qso: q }
  }
  return want.every((k) => held.has(k)) ? found : null
}

/** One park's standing today: its distinct contacts, how many more make it valid, and whether it
 *  is valid. */
export function parkProgress(count: number): { count: number; toGo: number; valid: boolean } {
  return { count, toGo: Math.max(0, POTA_VALID_QSOS - count), valid: count >= POTA_VALID_QSOS }
}

/** ⭐ A park's day under more than one call — KD9TAW and KD9TAW/M, say — as a plain note:
 *  "US-1234: contacts under 2 calls (KD9TAW 6, KD9TAW/M 4) — POTA counts each call separately".
 *  The park's count is every call's, but POTA credits each call's file on its own, so a "valid"
 *  park may be two short files. `null` with one call (or none). Shown by the strip, the
 *  Activation pane and the checklist alike. */
export function mixedCallsNote(p: ActivationPark): string | null {
  const calls = p.calls ?? []
  if (calls.length < 2) return null
  return t('ota.activation.calls', {
    reference: p.reference,
    count: calls.length,
    calls: calls.map((c) => `${c.callsign ?? t('ota.activation.calls.none')} ${c.count}`).join(', '),
  })
}

/** A legacy US park reference ("K-1234"): POTA renamed US parks to "US-" in 2024, and whether
 *  the upload still takes the old prefix is not known, so the screen says so. */
export function isLegacyUsRef(ref: string): boolean {
  return /^K-\d{4,5}$/.test(ref.trim().toUpperCase())
}

/** The instant the UTC day of `unix` starts. */
export function utcDayStart(unix: number): number {
  return Math.floor(unix / DAY) * DAY
}

/** ⭐ THIS ACTIVATION'S CONTACTS, as the Logbook asks for them: the first park, whole, from 0000Z of
 *  the UTC day the activation started — so a run across 0000Z is both days, and a two-fer is every
 *  contact once (each carries all its parks). POTA's unit is the park's UTC day, and the counter's
 *  and the export's are too, so a contact from the park earlier that day, before a Stop and a Start,
 *  is this activation's as well: it is in the same count and the same file. `null` when not
 *  activating. */
export function activationFilter(act: Activation | null | undefined): LogFilter | null {
  const parks = activationParks(act)
  if (parks.length === 0 || !act?.startedAt) return null
  return { myRef: parks[0], fromUnix: utcDayStart(act.startedAt) }
}

/** The per-park, per-UTC-day files this activation has made so far — the Logbook's export list,
 *  narrowed to the activation's parks and the days since it started — oldest day first, each day's
 *  parks in the activation's order. */
export function activationFiles(
  list: readonly LoggedActivation[],
  act: Activation | null | undefined,
): LoggedActivation[] {
  const parks = activationParks(act)
  if (parks.length === 0 || !act?.startedAt) return []
  const from = utcDayStart(act.startedAt)
  return list
    .filter((a) => parks.includes(a.reference.toUpperCase()) && a.dayStartUnix >= from)
    .sort(
      (a, b) =>
        a.dayStartUnix - b.dayStartUnix ||
        parks.indexOf(a.reference.toUpperCase()) - parks.indexOf(b.reference.toUpperCase()),
    )
}

/** A span of seconds as hours and minutes ("1 h 05 m"; "12 m" under an hour). Invariant tokens. */
export function spanLabel(secs: number): string {
  const m = Math.max(0, Math.floor(secs / 60))
  const h = Math.floor(m / 60)
  return h > 0 ? `${h} h ${String(m % 60).padStart(2, '0')} m` : `${m} m`
}

/** Seconds from `nowUnix` to the next 0000Z, when the counts start again. */
export function toUtcMidnight(nowUnix: number): number {
  return utcDayStart(nowUnix) + DAY - nowUnix
}
