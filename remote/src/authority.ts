// Only this adapter turns provider claims and credential digests into authority.
// ObservationRelay never receives identity data taken directly from a socket.
import { createRemoteJWKSet, jwtVerify } from 'jose'
import type { Entitlement, Identity, StationAccess } from '../../ui/src/remote-monitor/relay'

export interface RemoteEnv {
  DB: D1Database
  STATIONS: DurableObjectNamespace
  ASSETS: Fetcher
  PUBLIC_REMOTE_ORIGIN: string
  AUTH0_ISSUER: string
  AUTH0_AUDIENCE: string
  AUTH0_CLIENT_ID: string
  /** The provider subject allowed to administer trials. Unset or empty means NOBODY can, which is
   *  the only safe default: this gates a write to the entitlement table, so a check that fails
   *  OPEN would let anyone grant themselves the trial the one-trial-ever rule exists to refuse. */
  ADMIN_SUBJECT?: string
  REMOTE_BUILD_REVISION?: string
}

export class Refusal extends Error {
  constructor(readonly code: string, readonly status = 403) { super(code) }
}
export function requireValue(ok: unknown, code = 'accessDenied', status = 403): asserts ok {
  if (!ok) throw new Refusal(code, status)
}
export const uuid = () => crypto.randomUUID()
export const secret = () => Array.from(crypto.getRandomValues(new Uint8Array(32)), b => b.toString(16).padStart(2, '0')).join('')
export async function digest(value: string): Promise<string> {
  return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(value))),
    b => b.toString(16).padStart(2, '0')).join('')
}
export function id(value: unknown): string {
  requireValue(typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value), 'invalidRequest', 400)
  return value
}
export function proof(value: unknown): string {
  requireValue(typeof value === 'string' && /^[0-9a-f]{64}$/.test(value), 'invalidRequest', 400)
  return value
}
export function label(value: unknown): string {
  requireValue(typeof value === 'string' && value.trim().length > 0 && [...value].length <= 48 &&
    !/[\p{Cc}\p{Cf}]/u.test(value), 'invalidRequest', 400)
  return value.trim()
}

export async function body(request: Request, fields: string[]): Promise<Record<string, unknown>> {
  requireValue(request.headers.get('content-type')?.split(';')[0] === 'application/json', 'invalidRequest', 400)
  const reader = request.body?.getReader()
  requireValue(reader, 'invalidRequest', 400)
  let bytes = 0
  const chunks: Uint8Array[] = []
  while (true) {
    const item = await reader.read()
    if (item.done) break
    bytes += item.value.length
    if (bytes > 4096) { await reader.cancel(); throw new Refusal('requestTooLarge', 413) }
    chunks.push(item.value)
  }
  const buffer = new Uint8Array(bytes)
  let offset = 0
  for (const chunk of chunks) { buffer.set(chunk, offset); offset += chunk.length }
  let value: unknown
  try { value = JSON.parse(new TextDecoder('utf-8', { fatal: true, ignoreBOM: false }).decode(buffer)) }
  catch { throw new Refusal('invalidRequest', 400) }
  requireValue(value && typeof value === 'object' && !Array.isArray(value), 'invalidRequest', 400)
  requireValue(Object.keys(value).every(key => fields.includes(key)) && fields.every(key => key in value), 'invalidRequest', 400)
  return value as Record<string, unknown>
}

export function browserOrigin(request: Request, env: RemoteEnv): void {
  requireValue(request.headers.get('origin') === env.PUBLIC_REMOTE_ORIGIN, 'originDenied')
}
export function bearer(request: Request): string {
  const header = request.headers.get('authorization') ?? ''
  requireValue(header.startsWith('Bearer ') && header.length <= 8192, 'signInRequired', 401)
  return header.slice(7)
}
/** The mailbox an address delivers to, which is what "one trial per person" actually has to key on.
 *
 *  THE RULE, and deliberately no more of it:
 *  - trim, and lower-case the whole address;
 *  - drop a `+tag` from the local part, for every domain;
 *  - for gmail.com and googlemail.com only, remove dots from the local part, and treat
 *    googlemail.com as gmail.com.
 *
 *  Plus-addressing is near universal (Gmail, Outlook, Proton, Fastmail, iCloud), and a provider
 *  that treated `+` as an ordinary character would be merging mailboxes nobody could sign up for
 *  twice in practice. Dots are different: they are significant at most providers, so ignoring them
 *  anywhere but Gmail, where Google documents that they are, would bind two real strangers. No
 *  other provider-specific rule is applied - each one is a guess about somebody else's mail server.
 *  A local part that would be left empty (`+x@`, `...@gmail.com`) is kept as it was rather than
 *  collapsed onto one shared key. */
export function mailbox(address: string): string {
  const email = address.trim().toLowerCase(), at = email.lastIndexOf('@')
  if (at < 1) return email
  let local = email.slice(0, at), domain = email.slice(at + 1)
  if (local.indexOf('+') > 0) local = local.slice(0, local.indexOf('+'))
  if (domain === 'googlemail.com') domain = 'gmail.com'
  if (domain === 'gmail.com') local = local.replace(/\./g, '') || local
  return `${local}@${domain}`
}

let jwks: { issuer: string; keys: ReturnType<typeof createRemoteJWKSet> } | undefined
/** The namespace an Auth0 login Action must use to put the email claims on the access token.
 *  Hardcoded rather than configured because it has to match the Action byte for byte, and a second
 *  place to set it is a second place for it to drift - a drift that fails SILENTLY, by simply never
 *  identifying anyone again. STAGING.md carries the Action to paste. */
const CLAIM_NS = 'https://nexus.hamradiotools.io/'

export async function account(request: Request, env: RemoteEnv, now: number): Promise<Identity & { subject: string; verified: boolean }> {
  browserOrigin(request, env)
  requireValue(env.AUTH0_ISSUER.startsWith('https://') && env.AUTH0_ISSUER.endsWith('/') &&
    env.AUTH0_CLIENT_ID !== 'unconfigured', 'serviceNotConfigured', 503)
  if (jwks?.issuer !== env.AUTH0_ISSUER) {
    jwks = { issuer: env.AUTH0_ISSUER, keys: createRemoteJWKSet(new URL('.well-known/jwks.json', env.AUTH0_ISSUER)) }
  }
  let subject: string, until: number, email: string | null = null
  try {
    const { payload } = await jwtVerify(bearer(request), jwks.keys, {
      issuer: env.AUTH0_ISSUER, audience: env.AUTH0_AUDIENCE, algorithms: ['RS256'],
      requiredClaims: ['sub', 'exp', 'iat'], currentDate: new Date(now), clockTolerance: 0,
    })
    requireValue(typeof payload.sub === 'string' && payload.sub.length > 0 && payload.sub.length <= 256 &&
      payload.azp === env.AUTH0_CLIENT_ID && typeof payload.iat === 'number' && payload.iat * 1000 <= now + 30000)
    subject = payload.sub
    until = Math.min(Number(payload.exp) * 1000, now + 3600000)
    // Only a VERIFIED address counts. An unverified one is a claim the holder typed, so keying
    // entitlement on it would replace "one sign-up per trial" with "one typed address per trial" -
    // no better, and it would wrongly bind two strangers who typed the same thing.
    // Read BOTH spellings. `getTokenSilently()` returns the ACCESS token for our own API
    // audience, and the `email` scope populates the ID token and /userinfo - not that. The claims
    // reach an access token only when a login Action puts them there, and Auth0 SILENTLY DROPS a
    // non-namespaced custom claim on a custom-API audience ("the transaction won't fail, but your
    // custom claim won't be added"), so the namespaced pair is the only one an Action can deliver.
    // The bare names are still read first: a provider that sends the OIDC-standard claims natively
    // is no less trustworthy, and reading both means neither arrangement silently does nothing.
    const verifiedClaim = payload.email_verified ?? payload[`${CLAIM_NS}email_verified`]
    const emailClaim = payload.email ?? payload[`${CLAIM_NS}email`]
    if (verifiedClaim === true && typeof emailClaim === 'string' &&
        emailClaim.length > 0 && emailClaim.length <= 320) {
      email = emailClaim.trim().toLowerCase()
    }
  } catch { throw new Refusal('signInRequired', 401) }
  // Two hashes of one verified address. `email_hash` is the ADDRESS, trim + lower-case only, and
  // keeps that exact meaning because every row written before normalisation holds it and a hash
  // cannot be normalised afterwards. `mailbox_hash` is the MAILBOX - see mailbox() for the rule -
  // and is what makes op@gmail.com, op+2@gmail.com and o.p@gmail.com one person. Both carry the
  // same `email:` prefix, so for an address that is already normal the two are identical.
  const emailHash = email ? await digest(`email:${email}`) : null
  const mailboxHash = email ? await digest(`email:${mailbox(email)}`) : null
  await env.DB.prepare('INSERT OR IGNORE INTO accounts(id, issuer, subject, email_hash, mailbox_hash) VALUES(?,?,?,?,?)')
    .bind(uuid(), env.AUTH0_ISSUER, subject, emailHash, mailboxHash).run()
  // Backfill only: an account created before these columns, or before its provider supplied a
  // verified address, learns its hashes on the next sign-in. Never overwrites a hash, and never
  // with null, so a provider that stops sending the claim cannot quietly unpick an account's identity.
  if (emailHash) {
    await env.DB.prepare(`UPDATE accounts SET email_hash=COALESCE(email_hash, ?), mailbox_hash=COALESCE(mailbox_hash, ?)
      WHERE issuer=? AND subject=? AND (email_hash IS NULL OR mailbox_hash IS NULL)`)
      .bind(emailHash, mailboxHash, env.AUTH0_ISSUER, subject).run()
  }
  const row = await env.DB.prepare('SELECT id FROM accounts WHERE issuer=? AND subject=?')
    .bind(env.AUTH0_ISSUER, subject).first<{ id: string }>()
  requireValue(row, 'serviceUnavailable', 503)
  // `subject` rides alongside Identity rather than inside it. The caller destructures it off, so
  // it never reaches the object that index.ts spreads into the browser identity and on into relay
  // socket state - the provider subject is not something the wire needs to carry.
  return { accountId: row.id, expiresAt: until, subject, verified: email !== null }
}

/** Only the pinned provider subject may administer trials.
 *
 *  Fails CLOSED: with ADMIN_SUBJECT unset or empty this refuses everyone, including the operator.
 *  The bug that shape guards against is "not configured yet, so do not block anyone" - open during
 *  a beta and open forever after. The empty-subject half is defence in depth rather than the thing
 *  holding it up: account() already refuses a JWT whose sub is empty, so an unset pin cannot match
 *  a caller regardless. There is no role table and no second credential - one equality against an
 *  identity the service has already cryptographically verified. */
export function requireAdmin(env: RemoteEnv, subject: string): void {
  const pinned = env.ADMIN_SUBJECT ?? ''
  requireValue(pinned.length > 0 && subject.length > 0 && subject === pinned, 'accessDenied')
}

// Fourteen days, measured from the moment a station is approved at the shack — not from
// sign-in, because an account with no paired station cannot use the service and would burn
// its trial waiting. The one place that writes this is the approve batch in index.ts.
export const TRIAL_MS = 14 * 24 * 60 * 60 * 1000

export async function trial(env: RemoteEnv, accountId: string, now: number): Promise<Entitlement> {
  const row = await env.DB.prepare('SELECT enabled, expires_at, started_at, source FROM trials WHERE account_id=?')
    .bind(accountId).first<{ enabled: number; expires_at: number; started_at: number | null; source: string | null }>()
  // Four distinct situations, named. Callers must never re-derive these from enabled+expiresAt:
  // that is the arithmetic this field exists to stop two separate consumers from guessing at.
  const state = !row ? 'none' : row.enabled !== 1 ? 'disabled' : row.expires_at > now ? 'active' : 'ended'
  return { accountId, enabled: row?.enabled === 1, expiresAt: row?.expires_at ?? 0,
    state, startedAt: row?.started_at ?? null, source: row?.source ?? null }
}

// The strict gate: is the service usable right now? ONE call site, index.ts:222, and it is
// reached only by `device`, `ticket` and `renew` - the `observe` route returns before it.
// Observe IS entitlement-gated, but by observerDeadline inside connectObserver (relay.ts:41),
// which is also what expires a session already in flight. Do not read this function as the
// whole entitlement story; an audit that does will conclude observe is unguarded here, or that
// it is guarded here, and both are wrong.
export function requireTrial(entitlement: Entitlement, now: number): void {
  requireValue(entitlement.enabled && entitlement.expiresAt > now, 'trialRequired')
}

// The admission gate, for the two endpoints that can START a trial. An account may pair while
// its trial runs, or if it has never had one. Ended and disabled are refused under their own
// codes, so the browser can say which of the two happened instead of showing one vague wall.
// There is deliberately no path back to `none`: the trials row is the durable proof that the
// trial was consumed, so deleting it is what would reopen reinstall and re-pair abuse.
export function requireEligible(entitlement: Entitlement): void {
  if (entitlement.state === 'active' || entitlement.state === 'none') return
  throw new Refusal(entitlement.state === 'ended' ? 'trialEnded' : 'trialDisabled')
}

/** Has the PERSON behind this account already consumed a trial under another sign-in?
 *
 *  `one trial, ever` was one trial per provider subject: the same human signing in through a
 *  different Auth0 connection got a different `sub`, a new account row, and a fresh clock. A
 *  verified email is what those identities actually share, so a trial already spent under any
 *  account carrying the same verified address counts against this one.
 *
 *  An account with no verified address is not refused - it simply gets no extra protection, which
 *  is the old behaviour. The real ceiling on this is whether the tenant enforces verification at
 *  all; without that, a fresh unverified address is still a fresh trial. Documented in STAGING.md
 *  as an operator responsibility rather than pretended away here.
 *
 *  "The same verified address" means the same MAILBOX (mailbox()), and it has to keep recognising
 *  rows written before that rule existed, which hold only the address hash. So a sibling matches
 *  when its mailbox hash is ours, OR its address hash is either our address hash (the very same
 *  spelling) or our mailbox hash (it was recorded under the plain form of our mailbox). The one
 *  pairing this cannot see is an old row recorded under a non-plain spelling, from a different
 *  non-plain spelling (o.p@gmail.com then, op+2@gmail.com now) - until that account signs in again
 *  and learns its mailbox hash. There is no address left to normalise, only its hash. */
export async function requireUnspentIdentity(env: RemoteEnv, accountId: string, now: number): Promise<void> {
  const self = await env.DB.prepare('SELECT email_hash, mailbox_hash FROM accounts WHERE id=?')
    .bind(accountId).first<{ email_hash: string | null; mailbox_hash: string | null }>()
  if (!self?.email_hash && !self?.mailbox_hash) return
  const spent = await env.DB.prepare(`SELECT t.expires_at FROM trials t
    JOIN accounts a ON a.id = t.account_id
    WHERE a.id != ? AND (a.mailbox_hash = ? OR a.email_hash IN (?, ?)) LIMIT 1`)
    .bind(accountId, self.mailbox_hash, self.email_hash, self.mailbox_hash).first<{ expires_at: number }>()
  if (!spent) return
  // A sibling account is still RUNNING a trial: that is the same person with two sign-ins, not a
  // second entitlement, so point them at the one they already have rather than starting another.
  throw new Refusal(spent.expires_at > now ? 'trialActiveElsewhere' : 'trialEnded')
}
export type StationRow = { id: string; account_id: string; name: string; enabled: number; generation: number; policy_version: number }
export async function station(env: RemoteEnv, stationId: string): Promise<StationRow> {
  const row = await env.DB.prepare('SELECT id,account_id,name,enabled,generation,policy_version FROM stations WHERE id=?')
    .bind(id(stationId)).first<StationRow>()
  requireValue(row, 'stationUnavailable', 404)
  return row
}
export async function native(request: Request, env: RemoteEnv, stationId: string, allowRevoked = false): Promise<StationRow> {
  // The native client has no Origin. A page cannot use this as a cookie-auth path.
  requireValue(!request.headers.has('origin'), 'originDenied')
  const hash = await digest(proof(bearer(request)))
  // The credential match ALONE selects the row. `enabled` is judged afterwards, so a station whose
  // access was revoked can be told that it was revoked instead of being left to guess from the
  // same 401 a wrong credential gets - the shack could see only that it had stopped working.
  //
  // ⚠️ The order is the security property, not a style choice. A caller who does NOT hold the
  // station's credential still gets the undifferentiated `stationNotApproved`, so this cannot be
  // used to ask whether a station id exists or what became of it. Never hoist the enabled check
  // above the credential match to simplify this.
  const row = await env.DB.prepare('SELECT id,account_id,name,enabled,generation,policy_version FROM stations WHERE id=? AND credential_hash=?')
    .bind(id(stationId), hash).first<StationRow>()
  requireValue(row, 'stationNotApproved', 401)
  requireValue(allowRevoked || row.enabled === 1, 'stationRevoked', 401)
  return row
}
export async function access(env: RemoteEnv, row: StationRow, now: number): Promise<StationAccess> {
  const devices = await env.DB.prepare('SELECT id,generation,approved FROM devices WHERE station_id=? AND expires_at>? ORDER BY id LIMIT 9')
    .bind(row.id, now).all<{ id: string; generation: number; approved: number }>()
  requireValue(devices.results.length <= 8, 'deviceLimit', 409)
  return { stationId: row.id, accountId: row.account_id, enabled: row.enabled === 1,
    policyVersion: row.policy_version, stationGeneration: row.generation,
    devices: devices.results.map(d => ({ id: d.id, generation: d.generation, approved: d.approved === 1 })) }
}
/** Who an anonymous caller is, for rate limiting, and the wider network they sit in.
 *
 *  IPv4 is the FULL address, exactly as before, and has no wider network: carrier NAT puts whole
 *  towns behind one /24, so grouping by prefix there would refuse strangers for each other.
 *
 *  IPv6 is NOT the full address. Every VPS is handed a /64, 2^64 addresses it can source from at
 *  will, so a per-address bucket is no bound at all - rotating through them drained the shared
 *  enroll budget and nobody anywhere could pair a station. The caller is the /64, which is one
 *  host or one LAN, and the network is the /48, which is what one home connection, one site or
 *  one free tunnel is given (a /56 alone holds 256 /64s). Spellings are parsed, not string-sliced:
 *  `2001:db8:1::5` and `2001:0DB8:0001:0:0:0:0:5` are the same caller.
 *
 *  An IPv4-mapped address is an IPv4 caller. Anything unparseable is keyed on its own text, which
 *  is no weaker than the full-address key it replaces; Cloudflare sets this header, not the caller. */
export function caller(address: string | null): { caller: string; network: string | null } {
  const text = (address ?? 'local').trim().toLowerCase()
  const groups = text.includes(':') ? ipv6(text) : null
  if (!groups) return { caller: text, network: null }
  if (groups.slice(0, 5).every(group => group === 0) && groups[5] === 0xffff) {
    return { caller: [groups[6] >> 8, groups[6] & 255, groups[7] >> 8, groups[7] & 255].join('.'), network: null }
  }
  const prefix = (count: number) => groups.slice(0, count).map(group => group.toString(16)).join(':')
  return { caller: `${prefix(4)}::/64`, network: `${prefix(3)}::/48` }
}
function ipv6(text: string): number[] | null {
  const halves = text.split('%')[0].split('::')
  if (halves.length > 2) return null
  const parse = (part: string, last: boolean): number[] | null => {
    if (!part) return []
    const groups: number[] = [], pieces = part.split(':')
    for (const [index, piece] of pieces.entries()) {
      if (/^[0-9a-f]{1,4}$/.test(piece)) { groups.push(parseInt(piece, 16)); continue }
      // A dotted IPv4 tail is legal only as the very last thing in the address.
      const octets = piece.split('.').map(Number)
      if (!last || index !== pieces.length - 1 || !/^\d{1,3}(\.\d{1,3}){3}$/.test(piece) || octets.some(o => o > 255)) return null
      groups.push(octets[0] << 8 | octets[1], octets[2] << 8 | octets[3])
    }
    return groups
  }
  const head = parse(halves[0], halves.length === 1), tail = halves.length === 2 ? parse(halves[1], true) : []
  if (!head || !tail) return null
  if (halves.length === 1) return head.length === 8 ? head : null
  const fill = 8 - head.length - tail.length
  return fill >= 1 ? [...head, ...new Array<number>(fill).fill(0), ...tail] : null
}

export async function rate(env: RemoteEnv, key: string, now: number, limit = 30, windowMs = 60000): Promise<void> {
  // One bounded counter per authority/IP, rather than a new stored row each
  // minute of a long observation session. The UPSERT also arbitrates races.
  const bucket = await digest(key), until = (Math.floor(now / windowMs) + 1) * windowMs
  const result = await env.DB.prepare(`INSERT INTO rate_limits(id,hits,expires_at) VALUES(?,1,?)
    ON CONFLICT(id) DO UPDATE SET
      hits=CASE WHEN expires_at<=? THEN 1 ELSE hits+1 END,
      expires_at=CASE WHEN expires_at<=? THEN excluded.expires_at ELSE expires_at END
    WHERE expires_at<=? OR hits<? RETURNING hits`)
    .bind(bucket, until, now, now, now, limit).first()
  requireValue(result, 'tryLater', 429)
}
export const cookieName = (stationId: string) => `__Host-nexus-remote-${id(stationId)}`
export function deviceCookie(request: Request, stationId: string): string | null {
  const name = `${cookieName(stationId)}=`
  const parts = (request.headers.get('cookie') ?? '').split(';').map(s => s.trim()).filter(s => s.startsWith(name))
  if (parts.length !== 1) return null
  const value = parts[0].slice(name.length)
  return /^[0-9a-f]{64}$/.test(value) ? value : null
}
export function cookie(stationId: string, value: string, maxAge = 2592000): string {
  return `${cookieName(stationId)}=${value}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=${maxAge}`
}
export type DeviceRow = { id: string; name: string; generation: number; approved: number; expires_at: number }
export async function device(request: Request, env: RemoteEnv, stationId: string, owner: string, now: number): Promise<DeviceRow | null> {
  const credential = deviceCookie(request, stationId)
  if (!credential) return null
  return env.DB.prepare(`SELECT id,name,generation,approved,expires_at FROM devices
    WHERE station_id=? AND account_id=? AND credential_hash=? AND expires_at>?`)
    .bind(stationId, owner, await digest(credential), now).first<DeviceRow>()
}
