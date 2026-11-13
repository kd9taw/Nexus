// One trial per callsign and per shack PC, beside the existing one per mailbox (authority.ts,
// requireUnspentIdentity). The shack sends its configured callsign and two hashes of the PC's
// identifiers with the approval, which is the moment a trial starts; the browser never supplies either.
// Neither is ever stored in the clear: each is an HMAC under TRIAL_KEY (0013_subscription.sql).
//
// What this can and cannot see: a US callsign is checked against the FCC's licence list, and proven
// when the account's verified mailbox is the licence's email on file. A non-US callsign is checked for
// shape only, and refused outright while the identity is required (the paid launch is US-only, ruling
// 1406), which also caps a mail domain's trials (`DOMAIN_TRIALS`). The PC identifiers change with a new VM or a registry edit: a speed bump, not a wall.
// A squatted callsign is settled by support, who grant the trial by hand and can record the proven
// owner (index.ts, admin/grant-trial).
import { Refusal, requireValue, type RemoteEnv } from './authority'

/** The parts a portable or operating-condition suffix adds, which are never the station's own call. */
const MODIFIERS = new Set(['P', 'M', 'MM', 'AM', 'QRP', 'A', 'R'])
const CALL_SHAPE = /^[A-Z0-9]{1,3}[0-9][A-Z0-9]{0,3}[A-Z]$/
/** US licences: the prefixes the FCC issues, territories included. */
const US_CALL = /^(A[A-L]|[KNW])/
/** How stale the licence copy may be before US trials are refused rather than waved through. */
export const ULS_STALE_MS = 3 * 24 * 60 * 60 * 1000

/** The base call a trial is keyed on: `W1ABC/P`, `VE3/W1ABC` and `w1abc` are all W1ABC.
 *  Upper-cased and trimmed; anything but letters, digits and `/`, or over 20 characters, is refused
 *  rather than repaired, because the station sends exactly what its operator configured. Of the parts
 *  left once modifiers and single digits are dropped, the one shaped like a callsign is the base; if
 *  two are, the longer, and the first of two equally long. */
export function baseCall(value: unknown): string {
  requireValue(typeof value === 'string', 'callsignInvalid', 400)
  const call = value.trim().toUpperCase()
  requireValue(call.length > 0 && call.length <= 20 && /^[A-Z0-9/]+$/.test(call), 'callsignInvalid', 400)
  const parts = call.split('/').filter(part => !MODIFIERS.has(part) && !/^[0-9]$/.test(part) && CALL_SHAPE.test(part))
  requireValue(parts.length > 0, 'callsignInvalid', 400)
  return parts.reduce((best, part) => part.length > best.length ? part : best)
}

export function usCall(base: string): boolean { return US_CALL.test(base) }

export async function hmac(key: string, value: string): Promise<string> {
  const secret = await crypto.subtle.importKey('raw', new TextEncoder().encode(key), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign'])
  return Array.from(new Uint8Array(await crypto.subtle.sign('HMAC', secret, new TextEncoder().encode(value))),
    b => b.toString(16).padStart(2, '0')).join('')
}

/** A PC identifier as the shack sends it: the SHA-256 of the raw value (lowercase hex), or null when
 *  the PC has none worth sending (a placeholder hardware UUID). */
function pcHash(value: unknown): string | null {
  if (value === null) return null
  requireValue(typeof value === 'string' && /^[0-9a-f]{64}$/.test(value), 'invalidRequest', 400)
  return value
}

export type TrialClaim = { call: string | null; us: boolean; proven: boolean; pcHw: string | null; pcOs: string | null }
/** What a claim keeps before its callsign is checked against the licence list: the HMACs, and whether the
 *  call is a US one. Nothing in it is a value the shack sent. */
export type TrialInput = { call: string | null; us: boolean; pcHw: string | null; pcOs: string | null }

/** The approve body's optional `callsign` and `pc`, turned into the markers a trial would spend, or
 *  null when this approval is checked by mailbox only: no TRIAL_KEY, or a Nexus that predates the
 *  fields while they are not yet required. Refuses a malformed field whatever the configuration, so a
 *  Nexus that sends one learns of it on the first service that reads it. */
export async function trialClaim(env: RemoteEnv, input: Record<string, unknown>, accountId: string, now: number): Promise<TrialClaim | null> {
  const kept = await trialInput(env, input)
  return kept === null ? null : trialProof(env, kept, accountId, now)
}

/** The first half of `trialClaim`: the fields checked for shape and turned into HMACs, as a station that
 *  signs in keeps them until its trial starts. */
export async function trialInput(env: RemoteEnv, input: Record<string, unknown>): Promise<TrialInput | null> {
  const base = 'callsign' in input ? baseCall(input.callsign) : null
  let hw: string | null = null, os: string | null = null
  if ('pc' in input) {
    const pc = input.pc
    requireValue(pc && typeof pc === 'object' && !Array.isArray(pc) &&
      Object.keys(pc).every(key => key === 'hw' || key === 'os') && 'hw' in pc && 'os' in pc, 'invalidRequest', 400)
    hw = pcHash((pc as Record<string, unknown>).hw)
    os = pcHash((pc as Record<string, unknown>).os)
  }
  const required = env.TRIAL_IDENTITY_REQUIRED === '1'
  requireValue(!required || (base !== null && 'pc' in input), 'updateRequired')
  const key = env.TRIAL_KEY ?? ''
  if (!key) {
    // Required with no key to keep them by would be a requirement that checks nothing.
    requireValue(!required, 'serviceNotConfigured', 503)
    return null
  }
  if (base === null && !('pc' in input)) return null
  return { call: base === null ? null : await hmac(key, `call:${base}`), us: base !== null && usCall(base),
    pcHw: hw === null ? null : await hmac(key, `pc:hw:${hw}`),
    pcOs: os === null ? null : await hmac(key, `pc:os:${os}`) }
}

/** The second half: a US call checked against the licence list, and proven when the account's verified
 *  mailbox is the licence's email on file. */
export async function trialProof(env: RemoteEnv, kept: TrialInput, accountId: string, now: number): Promise<TrialClaim> {
  const key = env.TRIAL_KEY ?? ''
  requireValue(key, 'serviceNotConfigured', 503)
  // Required (the paid launch, ruling 1406), a trial is a US licence's: a claim with no call is refused
  // as the shack's missing callsign, and a call outside the US as one the licence list does not hold.
  if (env.TRIAL_IDENTITY_REQUIRED === '1') {
    requireValue(kept.call !== null, 'callsignInvalid', 400)
    requireValue(kept.us, 'callsignNotLicensed')
  }
  let proven = false
  if (kept.call !== null && kept.us) {
    const synced = await env.DB.prepare('SELECT synced_at FROM uls_sync WHERE id=1').first<{ synced_at: number }>()
    requireValue(synced && synced.synced_at > now - ULS_STALE_MS, 'serviceUnavailable', 503)
    const licence = await env.DB.prepare('SELECT active, email_hmac FROM uls_calls WHERE call_hmac=?')
      .bind(kept.call).first<{ active: number; email_hmac: string | null }>()
    // A US call the licence list does not know is refused, not waved through: otherwise any
    // invented call would start a trial, and the rule would do nothing.
    requireValue(licence?.active === 1, 'callsignNotLicensed')
    const self = await env.DB.prepare('SELECT mailbox_hash FROM accounts WHERE id=?').bind(accountId).first<{ mailbox_hash: string | null }>()
    proven = licence.email_hmac !== null && !!self?.mailbox_hash && licence.email_hmac === await hmac(key, `mailbox:${self.mailbox_hash}`)
  }
  return { call: kept.call, us: kept.us, proven, pcHw: kept.pcHw, pcOs: kept.pcOs }
}

/** THE PER-DOMAIN TRIAL COUNT, while the trial identity is required (ruling 1406, security review L6):
 *  self-serve trials started in the last thirty days by other accounts whose mailbox is at the same
 *  domain (`accounts.domain_hash`, none for a public provider). At DOMAIN_TRIALS, no more start there.
 *  The SQL condition over `accounts self` "this domain is spent", with its bindings: the pre-check below
 *  and both trial INSERTs (index.ts) make the same test, so racing starts cannot both pass it. */
export const DOMAIN_TRIALS = 3
export const DOMAIN_WINDOW_MS = 30 * 24 * 60 * 60 * 1000
export const DOMAIN_SPENT = `self.domain_hash IS NOT NULL AND (SELECT COUNT(*) FROM trials dt JOIN accounts da ON da.id=dt.account_id
  WHERE da.domain_hash=self.domain_hash AND da.id<>self.id AND dt.source LIKE 'trial%' AND dt.started_at>?)>=${DOMAIN_TRIALS}`
export const domainRequired = (env: RemoteEnv) => env.TRIAL_IDENTITY_REQUIRED === '1'
/** The INSERT's guard: an empty string while the count is off, its bindings otherwise. */
export function domainGuard(env: RemoteEnv, accountId: string, now: number): { sql: string; bindings: unknown[] } {
  return domainRequired(env)
    ? { sql: `AND NOT EXISTS(SELECT 1 FROM accounts self WHERE self.id=? AND ${DOMAIN_SPENT})`, bindings: [accountId, now - DOMAIN_WINDOW_MS] }
    : { sql: '', bindings: [] }
}
/** Refuses, by name, a trial whose account's mail domain has started its allowance this month. */
export async function requireUnspentDomain(env: RemoteEnv, accountId: string, now: number): Promise<void> {
  if (!domainRequired(env)) return
  const spent = await env.DB.prepare(`SELECT 1 AS spent FROM accounts self WHERE self.id=? AND ${DOMAIN_SPENT}`)
    .bind(accountId, now - DOMAIN_WINDOW_MS).first()
  if (spent) throw new Refusal('trialDomainLimit')
}

/** The SQL condition "another account already spent one of these", over `trial_markers m`, and its
 *  bindings: the same test the pre-check below makes and the trial INSERT repeats in its own statement,
 *  so two racing approvals cannot both pass it. A claimed callsign is spent by a proven marker, or by
 *  any marker when this claim is itself unproven: a proven owner is never blocked by a squatter. */
export const SPENT_MARKER = `m.account_id<>? AND ((m.kind='call' AND m.value_hmac=? AND (m.proven=1 OR ?=0))
  OR (m.kind='pc-hw' AND m.value_hmac=?) OR (m.kind='pc-os' AND m.value_hmac=?))`
export function spentBindings(accountId: string, claim: TrialClaim): unknown[] {
  return [accountId, claim.call, claim.proven ? 1 : 0, claim.pcHw, claim.pcOs]
}

/** Refuses, by name, a trial one of whose markers another account already spent. */
export async function requireUnspentMarkers(env: RemoteEnv, accountId: string, claim: TrialClaim): Promise<void> {
  const spent = await env.DB.prepare(`SELECT m.kind FROM trial_markers m WHERE ${SPENT_MARKER} ORDER BY m.kind='call' DESC LIMIT 1`)
    .bind(...spentBindings(accountId, claim)).first<{ kind: string }>()
  if (spent) throw new Refusal(spent.kind === 'call' ? 'trialUsedByCallsign' : 'trialUsedOnThisPc')
}

/** The statements that record a trial's markers, each landing only if this account's trial started at
 *  `now`: an approval that started nothing spends nothing. */
export function markerInserts(env: RemoteEnv, accountId: string, claim: TrialClaim, now: number): D1PreparedStatement[] {
  const rows: [string, string | null, number, string | null][] = [
    ['call', claim.call, claim.proven ? 1 : 0, claim.proven ? 'uls-email' : null],
    ['pc-hw', claim.pcHw, 0, null], ['pc-os', claim.pcOs, 0, null]]
  return rows.filter(([, value]) => value !== null).map(([kind, value, proven, method]) =>
    env.DB.prepare(`INSERT OR IGNORE INTO trial_markers(kind,value_hmac,account_id,proven,method,created_at)
      SELECT ?,?,?,?,?,? WHERE EXISTS(SELECT 1 FROM trials WHERE account_id=? AND started_at=? AND source='trial')`)
      .bind(kind, value, accountId, proven, method, now, accountId, now))
}
