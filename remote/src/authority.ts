// Only this adapter turns provider claims and credential digests into authority.
// ObservationRelay never receives identity data taken directly from a socket.
import { createRemoteJWKSet, decodeJwt, jwtVerify } from 'jose'
import { OBSERVER_LEASE_MS, type Entitlement, type Identity, type StationAccess } from '../../ui/src/remote-monitor/relay'

export interface RemoteEnv {
  DB: D1Database
  STATIONS: DurableObjectNamespace
  ASSETS: Fetcher
  PUBLIC_REMOTE_ORIGIN: string
  AUTH0_ISSUER: string
  AUTH0_AUDIENCE: string
  AUTH0_CLIENT_ID: string
  /** The Native application's client id: Nexus on another computer, signed in through the system
   *  browser. Its tokens are taken on the native client's routes only, beside a signature by the
   *  device's own key, and the SPA's never are there. Unset or 'unconfigured', every one of those
   *  routes refuses `serviceNotConfigured`. */
  AUTH0_NATIVE_CLIENT_ID?: string
  /** The provider subject allowed to administer trials. Unset or empty means NOBODY can, which is
   *  the only safe default: this gates a write to the entitlement table, so a check that fails
   *  OPEN would let anyone grant themselves the trial the one-trial-ever rule exists to refuse. */
  ADMIN_SUBJECT?: string
  /** The stream's relay: a Cloudflare Realtime TURN key's id and its API token, both Worker
   *  secrets. Unset, the relay route refuses `relayNotConfigured` and every stream stays direct. */
  TURN_KEY_ID?: string
  TURN_KEY_TOKEN?: string
  REMOTE_BUILD_REVISION?: string
  /** The HMAC key for trial markers and the licence copy (trial-identity.ts). Unset, the callsign and
   *  shack-PC fields are checked for shape and nothing more: no marker is written or read. It can never
   *  be rotated, because the values it was taken over are not kept. */
  TRIAL_KEY?: string
  /** '1' once every Nexus that pairs sends its callsign and PC: an approve without them is refused
   *  `updateRequired` instead of starting a trial guarded by the mailbox alone. */
  TRIAL_IDENTITY_REQUIRED?: string
  /** '1' while Remote is free during the beta: a trial or a grant that has run out still allows
   *  internet use (`entitlementOf`). It takes away the END only: who may start one, a ban, deletion,
   *  the rate limits and the LAN rules are all as they are without it. Unset is today's trial. */
  REMOTE_BETA_FREE?: string
  /** The private billing Worker's `BillingService` entrypoint, which holds the payment keys this relay never
   *  does. Bound only once that environment's billing Worker is deployed (`billing.deployed` in
   *  scripts/staging-common.mjs); until then it is absent, the billing routes answer `billingNotAvailable`
   *  and the page shows no Subscribe or Manage button. */
  BILLING?: BillingService
  /** The email provider's API key for the owner-device notice (the 2026-10-23 sign-in decision), a Worker secret. ⛔ A
   *  CREDENTIAL GATE the operator wires in production; UNSET in every build today, so
   *  `deviceAddedNotice` sends nothing (a no-op, never an error). Never commit a real key. */
  OWNER_NOTICE_API_KEY?: string
  /** The From address the owner-device notice is sent as (the tenant's verified sender, e.g.
   *  `Nexus Remote <noreply@hamradiotools.io>`). Unset disables the notice with the key. */
  OWNER_NOTICE_FROM?: string
  /** Where the owner-device notice is POSTed. Defaults to the provider's endpoint; set only to
   *  point a test's capture server at it. Production leaves it unset (the default). */
  OWNER_NOTICE_ENDPOINT?: string
}
/** The billing Worker's RPC methods, as that Worker defines them: each answers `{ url }` (`cancel`:
 *  `{ cancelled }`) or `{ error, status }`. */
export interface BillingService {
  checkout(accountId: string, plan: 'monthly' | 'annual'): Promise<unknown>
  portal(accountId: string): Promise<unknown>
  /** Ends the account's live subscription at once, and closes its checkouts that could still be paid,
   *  answering `{ cancelled }` only once the payment provider shows nothing left that can charge. */
  cancel(accountId: string): Promise<unknown>
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
/** The browser's device key (security review M1; the stream's A5): an ECDSA P-256 public key as SPKI
 *  DER in lowercase hex, 91 bytes, the only half of the key that ever leaves the browser. The shape is
 *  the one fixed P-256 SPKI header and an uncompressed point; importing it is what proves the point is
 *  on the curve. The station pins its SHA-256 at the radio: this side never admits anything on it. */
const DEVICE_KEY = /^3059301306072a8648ce3d020106082a8648ce3d03010703420004[0-9a-f]{128}$/
export async function publicKey(value: unknown): Promise<string> {
  requireValue(typeof value === 'string' && DEVICE_KEY.test(value), 'invalidRequest', 400)
  const der = new Uint8Array(value.length / 2).map((_, i) => parseInt(value.slice(2 * i, 2 * i + 2), 16))
  try { await crypto.subtle.importKey('spki', der, { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']) }
  catch { throw new Refusal('invalidRequest', 400) }
  return value
}

/** A JSON object carrying exactly `fields`, and any of `optional`, within 4 KiB. */
export async function body(request: Request, fields: string[], optional: string[] = []): Promise<Record<string, unknown>> {
  return fieldsOf(await bodyBytes(request), fields, optional)
}
/** A JSON request's body within 4 KiB, as the bytes sent: what a native request's signature covers. */
export async function bodyBytes(request: Request): Promise<Uint8Array> {
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
  return buffer
}
/** Those bytes as a JSON object carrying exactly `fields`, and any of `optional`. */
export function fieldsOf(buffer: Uint8Array, fields: string[], optional: string[] = []): Record<string, unknown> {
  let value: unknown
  try { value = JSON.parse(new TextDecoder('utf-8', { fatal: true, ignoreBOM: false }).decode(buffer)) }
  catch { throw new Refusal('invalidRequest', 400) }
  requireValue(value && typeof value === 'object' && !Array.isArray(value), 'invalidRequest', 400)
  requireValue(Object.keys(value).every(key => fields.includes(key) || optional.includes(key)) && fields.every(key => key in value), 'invalidRequest', 400)
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

/** What a newly signed-in device is, for the owner notice (the 2026-10-23 sign-in decision): who it belongs to, which
 *  station, and how to recognise it. `toEmail` is the STATION OWNER's verified address, read
 *  transiently and never stored; `null` when the owner's address is not available on this sign-in —
 *  a guest's sign-in carries the guest's address, not the owner's — which turns the notice into a
 *  no-op (nothing is sent, `sent_at` stays null, the device stays held: "no email, no unlock"). */
export interface DeviceAddedNotice {
  accountId: string
  stationId: string
  deviceId: string
  toEmail: string | null
  stationName: string
  deviceName: string
  kind: string
}
/** Email the account holder that a device was added to their Nexus Remote account by a sign-in
 *  (the 2026-10-23 sign-in decision). The durable `notices` row is written FIRST and is the
 *  precondition the station's transmit hold lifts against; delivery (who/when/which station, how to
 *  revoke) is its gated second half.
 *
 *  ⛔ CREDENTIAL GATE (gate 5): the provider key (`OWNER_NOTICE_API_KEY`) and sender
 *  (`OWNER_NOTICE_FROM`) are production Worker secrets the operator wires. With either unset — the
 *  case in every build today — or with no owner address on this sign-in, the row is still written
 *  but no mail is sent and `sent_at` stays null. `sent_at` is set ONLY when the provider accepts the
 *  send (a 2xx); a rejection or a network failure leaves it null. Since `notified` is `sent_at IS
 *  NOT NULL`, each of these leaves the device held ("no email, no unlock", operator 2026-10-24). The
 *  real key is never committed and no live mail is sent here; the address is used once and dropped. */
export async function deviceAddedNotice(env: RemoteEnv, notice: DeviceAddedNotice, now: number): Promise<void> {
  // The DURABLE ENQUEUE comes first: one `notices` row a device, the record the station's transmit
  // hold lifts against (the native list surfaces its existence as `notified`). A store error leaves
  // no row — the device is added but stays held (fail-closed) — and never fails the registration.
  try {
    await env.DB.prepare(`INSERT INTO notices(id, account_id, station_id, device_id, created_at)
      SELECT ?,?,?,?,? WHERE NOT EXISTS(SELECT 1 FROM notices WHERE device_id=?)`)
      .bind(uuid(), notice.accountId, notice.stationId, notice.deviceId, now, notice.deviceId).run()
  } catch {
    return
  }
  const key = env.OWNER_NOTICE_API_KEY, from = env.OWNER_NOTICE_FROM
  if (!key || !from || !notice.toEmail) return
  const manage = `${env.PUBLIC_REMOTE_ORIGIN}/remote/`
  const text =
    `A device was just added to your Nexus Remote account by signing in.\n\n` +
    `Device: ${notice.deviceName} (${notice.kind})\n` +
    `Station: ${notice.stationName}\n` +
    `When: ${new Date(now).toISOString()}\n\n` +
    `It can control your station, but it cannot transmit for the first 15 minutes.\n` +
    `If this was not you, remove it now at ${manage}`
  // Mark the notice sent ONLY when the provider ACCEPTS it (a 2xx). `notified` — the precondition
  // the station's transmit hold lifts against — is `sent_at IS NOT NULL`, so a rejection (401/422/
  // a 5xx) or a network failure leaves `sent_at` null and the device held past its window until a
  // shack Approve ("no email, no unlock", operator 2026-10-24).
  let accepted = false
  try {
    const res = await fetch(env.OWNER_NOTICE_ENDPOINT || 'https://api.resend.com/emails', {
      method: 'POST',
      headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json' },
      body: JSON.stringify({
        from,
        to: notice.toEmail,
        subject: 'A device was added to your Nexus Remote account',
        text,
      }),
    })
    accepted = res.ok
  } catch {
    accepted = false
  }
  if (!accepted) return
  await env.DB.prepare('UPDATE notices SET sent_at=? WHERE device_id=? AND sent_at IS NULL')
    .bind(now, notice.deviceId).run()
}

let jwks: { issuer: string; keys: ReturnType<typeof createRemoteJWKSet> } | undefined
/** The provider's signing keys, fetched from the configured issuer and cached for this isolate. */
function keySet(env: RemoteEnv): ReturnType<typeof createRemoteJWKSet> {
  if (jwks?.issuer !== env.AUTH0_ISSUER) {
    jwks = { issuer: env.AUTH0_ISSUER, keys: createRemoteJWKSet(new URL('.well-known/jwks.json', env.AUTH0_ISSUER)) }
  }
  return jwks.keys
}
/** The namespace an Auth0 login Action must use to put the email claims on the access token.
 *  Hardcoded rather than configured because it has to match the Action byte for byte, and a second
 *  place to set it is a second place for it to drift - a drift that fails SILENTLY, by simply never
 *  identifying anyone again. STAGING.md carries the Action to paste. */
const CLAIM_NS = 'https://nexus.hamradiotools.io/'

export async function account(request: Request, env: RemoteEnv, now: number): Promise<Identity & { subject: string; verified: boolean; email: string | null }> {
  browserOrigin(request, env)
  return signedIn(request, env, now, env.AUTH0_CLIENT_ID)
}
/** The native client's caller: Nexus on another computer, signed in through the system browser to
 *  the Native application. The same issuer, API and account as the hosted page's, but only a token
 *  that Native application was given (`azp`), so the page's tokens open no native route and these
 *  open no route of the page's (which takes only the SPA's, and only with its exact Origin).
 *
 *  It carries no Origin and holds no cookie. A request with either came from a browser, so it is
 *  refused before its token is read: that is what keeps every native route out of a page's reach
 *  (no CSRF, whatever credential the page holds), as `native()` keeps the station's. */
export async function nativeAccount(request: Request, env: RemoteEnv, now: number): Promise<Identity & { subject: string; verified: boolean; email: string | null }> {
  requireValue(!request.headers.has('origin') && !request.headers.has('cookie'), 'originDenied')
  const clientId = env.AUTH0_NATIVE_CLIENT_ID || 'unconfigured'
  // Never the SPA's own id, which would let the page's tokens in here.
  requireValue(clientId !== env.AUTH0_CLIENT_ID, 'serviceNotConfigured', 503)
  return signedIn(request, env, now, clientId)
}
async function signedIn(request: Request, env: RemoteEnv, now: number, clientId: string): Promise<Identity & { subject: string; verified: boolean; email: string | null }> {
  requireValue(env.AUTH0_ISSUER.startsWith('https://') && env.AUTH0_ISSUER.endsWith('/') &&
    clientId !== 'unconfigured', 'serviceNotConfigured', 503)
  let subject: string, until: number, email: string | null = null
  try {
    const { payload } = await jwtVerify(bearer(request), keySet(env), {
      issuer: env.AUTH0_ISSUER, audience: env.AUTH0_AUDIENCE, algorithms: ['RS256'],
      requiredClaims: ['sub', 'exp', 'iat'], currentDate: new Date(now), clockTolerance: 0,
    })
    requireValue(typeof payload.sub === 'string' && payload.sub.length > 0 && payload.sub.length <= 256 &&
      payload.azp === clientId && typeof payload.iat === 'number' && payload.iat * 1000 <= now + 30000)
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
  // `subject` and `email` ride alongside Identity rather than inside it. The caller destructures
  // them off, so neither reaches the object that index.ts spreads into the browser identity and on
  // into relay socket state - the provider subject is not something the wire needs to carry, and
  // the plaintext address is kept NOWHERE (only its hashes are stored): `email` is handed to the
  // caller transiently for the owner-device notice (the 2026-10-23 sign-in decision) and must be used and dropped there,
  // never stored or sent to the wire.
  return { accountId: row.id, expiresAt: until, subject, verified: email !== null, email }
}

/** How long after its sign-in a proof is taken, and how far ahead of this service's clock its signing
 *  time may be. A proof is bounded by when it was signed (`iat`), never by the token's `exp`: the
 *  provider's ID tokens last ten hours by default, and a proof is only ever presented just after the
 *  sign-in that made it. */
export const PROOF_FRESH_MS = 10 * 60000
export const PROOF_SKEW_MS = 60000

/** What a sign-in vouches for: SHA-256, in base64url, of a label naming what is vouched for, a zero byte,
 *  for a device its kind and a zero byte, then each key's SPKI DER in order. DER is self-delimiting and
 *  the kinds are a fixed set, so no two lists of keys hash the same bytes; the zero bytes make that plain.
 *  The station's keys are its signing key and its LAN key, in that order; a computer's its road key and
 *  its LAN key, a phone's and a browser's their one key. Nexus and the page put this in the sign-in as its
 *  nonce, so the token the provider signs names exactly these keys. */
async function nonceOf(label: string, kind: string | null, keys: string[]): Promise<string> {
  const parts = [new TextEncoder().encode(label), new Uint8Array([0]),
    ...(kind === null ? [] : [new TextEncoder().encode(kind), new Uint8Array([0])]), ...keys.map(fromHex)]
  const bytes = new Uint8Array(parts.reduce((n, part) => n + part.length, 0))
  parts.reduce((at, part) => { bytes.set(part, at); return at + part.length }, 0)
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))
  return btoa(String.fromCharCode(...hash)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}
export const stationNonce = (stationKey: string, lanKey: string) => nonceOf('nexus-station/1', null, [stationKey, lanKey])
export const deviceNonce = (kind: string, keys: string[]) => nonceOf('nexus-device/1', kind, keys)

/** A proof: an ID token from `subject`'s sign-in to the application `clientId`, carrying `nonce`, signed
 *  by the provider within the last ten minutes. Answers its verified address, if it carries one.
 *
 *  Checked as the shack will check it, so the service never stores what the shack would refuse: RS256
 *  by a key the issuer publishes, the exact issuer, the audience (a list only with `azp` naming the
 *  application, and any `azp` naming it), the subject, the nonce, and the signing time. The token's
 *  own `exp` is read only as a token's (it is after its `iat`): the age bound is `iat`'s. */
export async function verifyProof(env: RemoteEnv, value: unknown, clientId: string, subject: string, nonce: string, now: number): Promise<{ email: string | null }> {
  requireValue(typeof value === 'string' && value.length <= 4096, 'invalidRequest', 400)
  try {
    const issued = decodeJwt(value).iat
    requireValue(typeof issued === 'number' && Number.isFinite(issued))
    const { payload } = await jwtVerify(value, keySet(env), {
      issuer: env.AUTH0_ISSUER, audience: clientId, algorithms: ['RS256'],
      requiredClaims: ['sub', 'iat', 'nonce'], currentDate: new Date(issued * 1000), clockTolerance: 0,
    })
    requireValue(payload.iat === issued && issued * 1000 <= now + PROOF_SKEW_MS && issued * 1000 >= now - PROOF_FRESH_MS)
    requireValue(payload.sub === subject && subject.length > 0 && payload.nonce === nonce)
    requireValue(!Array.isArray(payload.aud) || payload.azp === clientId)
    requireValue(payload.azp === undefined || payload.azp === clientId)
    const email = payload.email_verified === true && typeof payload.email === 'string' && payload.email.length > 0 &&
      payload.email.length <= 320 ? payload.email.trim().toLowerCase() : null
    return { email }
  } catch { throw new Refusal('proofInvalid', 403) }
}

/** Who an account is at a station: its owner, a person invited there with a role, or nobody (null). An
 *  invite is matched on the mailbox of the account's verified address (`mailbox_hash`, written only from
 *  a verified address at sign-in), and only while it has not been removed. */
export type Role = 'owner' | 'operate' | 'listen'
export async function memberRole(env: RemoteEnv, row: StationRow, accountId: string): Promise<Role | null> {
  if (row.account_id === accountId) return 'owner'
  const invite = await env.DB.prepare(`SELECT i.role FROM invites i JOIN accounts a ON a.mailbox_hash=i.mailbox_hash
    WHERE i.station_id=? AND a.id=? AND i.removed_at IS NULL`).bind(row.id, accountId).first<{ role: 'operate' | 'listen' }>()
  return invite?.role ?? null
}
/** The hash an invite keeps of an address: the mailbox it delivers to, as `signedIn` keeps an account's. */
export async function mailboxHash(address: unknown): Promise<string> {
  requireValue(typeof address === 'string' && address.length <= 320 && /^[^\s@]+@[^\s@]+$/.test(address.trim()), 'invalidRequest', 400)
  return digest(`email:${mailbox(address.trim().toLowerCase())}`)
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

/** After a paid period ends, the days it still counts: one, or seven while a payment is failing and
 *  the provider is still retrying it. */
export const PAID_GRACE_MS = 24 * 60 * 60 * 1000
export const PAST_DUE_GRACE_MS = 7 * 24 * 60 * 60 * 1000
/** During the beta, how far ahead an account's internet use is dated. Every session takes the nearer of
 *  this and its own lease, so switching the beta off ends internet use within a day at most, as the
 *  trial's own date would have. */
export const BETA_LEASE_MS = 24 * 60 * 60 * 1000

export type Plan = { kind: 'trial' | 'manual' | 'subscription' | null; status: string | null; periodEnd: number | null; renews: boolean }
/** The one answer to "what may this account do now", from its trial and its subscription together.
 *  `enabled`, `expiresAt` and `state` keep their meaning on the wire: internet use allowed, until when,
 *  and the four named situations, so every gate that reads them is unchanged. `access` adds whether the
 *  account may still use its stations on its own network ('lan') once internet use has ended, and
 *  `plan` is what the page needs to say why. */
export type AccountEntitlement = Entitlement & { access: 'internet' | 'lan' | 'none'; plan: Plan }
export type EntitlementRow = { enabled: number | null; expires_at: number | null; started_at: number | null; source: string | null
  status: string | null; plan: string | null; period_end: number | null; cancel_at_period_end: number | null
  /** 1 when the account was deleted (its subject renamed `deleted:<id>`); the beta never extends it. */
  deleted?: number | null }

/** Until when a subscription in this status allows internet use; 0 when it does not. A subscription
 *  that is not on one of our prices has no plan, and pays for nothing. */
function paidUntil(row: EntitlementRow): number {
  if (row.status === null || row.period_end === null || row.plan === null) return 0
  if (row.status === 'active' || row.status === 'trialing') return row.period_end + PAID_GRACE_MS
  if (row.status === 'past_due') return row.period_end + PAST_DUE_GRACE_MS
  return 0
}

export function entitlementOf(accountId: string, row: EntitlementRow, now: number, beta = false): AccountEntitlement {
  // A disabled trials row is an operator's ban, and outranks anything paid: no internet, no LAN.
  const disabled = row.enabled !== null && row.enabled !== 1
  // The beta keeps a trial or a grant running past its date. Only one the account already holds: an
  // account that never had one still starts it through the same gates (`requireMayPair`).
  // Never for a deleted account: deletion ended its trial, and the beta must not quietly restart it.
  const extend = beta && row.deleted !== 1
  const trialUntil = row.enabled === 1 ? extend ? Math.max(row.expires_at ?? 0, now + BETA_LEASE_MS) : row.expires_at ?? 0 : 0
  const until = disabled ? 0 : Math.max(trialUntil, paidUntil(row))
  const internet = until > now
  // Four distinct situations, named. Callers must never re-derive these from enabled+expiresAt:
  // that is the arithmetic this field exists to stop two separate consumers from guessing at.
  const state = disabled ? 'disabled' : internet ? 'active' : row.enabled !== null || row.status !== null ? 'ended' : 'none'
  const kind = row.status !== null ? 'subscription' : row.enabled === null ? null : row.source?.startsWith('trial') ? 'trial' : 'manual'
  return { accountId, enabled: !disabled && until > 0, expiresAt: disabled ? row.expires_at ?? 0 : until,
    state, startedAt: row.started_at ?? null, source: row.source ?? null,
    access: internet ? 'internet' : disabled ? 'none' : 'lan',
    plan: { kind, status: row.status, periodEnd: row.period_end,
      renews: row.plan !== null && (row.status === 'active' || row.status === 'trialing') && row.cancel_at_period_end !== 1 } }
}

export async function readEntitlement(env: RemoteEnv, accountId: string, now: number): Promise<AccountEntitlement> {
  const row = await env.DB.prepare(`SELECT t.enabled, t.expires_at, t.started_at, t.source,
    s.status, s.plan, s.period_end, s.cancel_at_period_end,
    (SELECT CASE WHEN acc.subject LIKE 'deleted:%' THEN 1 ELSE 0 END FROM accounts acc WHERE acc.id=a.id) AS deleted
    FROM (SELECT ? AS id) a LEFT JOIN trials t ON t.account_id=a.id LEFT JOIN subscriptions s ON s.account_id=a.id`)
    .bind(accountId).first<EntitlementRow>()
  requireValue(row, 'serviceUnavailable', 503)
  return entitlementOf(accountId, row, now, env.REMOTE_BETA_FREE === '1')
}

/** The account as a LAN-only session sees it (a ticket of scope 'lan'): usable on the shack's own
 *  network for as long as Remote is not switched off for the account, whatever its internet use. The
 *  date is a lease the session renews, never the account's: the observer and stream deadlines taken
 *  from it end the session within one lease of a switch-off, and nothing internet-only reads it. Only
 *  the session's observation, its lease and Stop, and a stream held to the LAN (stream-relay.ts) are
 *  open to such a session (room.ts); every other lane keeps the account's own entitlement. */
export function lanEntitlement(entitlement: AccountEntitlement, now: number): AccountEntitlement {
  // Named, never "not none": a reading without the field is no permission.
  return { ...entitlement, enabled: entitlement.access === 'lan' || entitlement.access === 'internet', expiresAt: now + OBSERVER_LEASE_MS }
}

// The strict gate: is the service usable right now? Called in index.ts for the browser's `device`,
// `ticket`, `renew` and `turn`, and the native client's `device`, `turn`, `pipe` and `renew` - the
// `observe` route returns before it.
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

/** The pairing gate, at the three steps that lead to an approval: eligible, and, unless the account
 *  already has internet use (a running trial, or a plan), not a person whose trial was spent under
 *  another sign-in. A subscriber pairs whatever a sibling account spent. */
export async function requireMayPair(env: RemoteEnv, entitlement: AccountEntitlement, now: number): Promise<void> {
  requireEligible(entitlement)
  if (entitlement.access !== 'internet') await requireUnspentIdentity(env, entitlement.accountId, now)
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
  const spent = await env.DB.prepare(`SELECT t.expires_at, t.enabled, a.subject LIKE 'deleted:%' AS deleted FROM trials t
    JOIN accounts a ON a.id = t.account_id
    WHERE a.id != ? AND (a.mailbox_hash = ? OR a.email_hash IN (?, ?)) LIMIT 1`)
    .bind(accountId, self.mailbox_hash, self.email_hash, self.mailbox_hash).first<{ expires_at: number; enabled: number; deleted: number }>()
  if (!spent) return
  // A sibling account is still RUNNING a trial: that is the same person with two sign-ins, not a
  // second entitlement, so point them at the one they already have rather than starting another.
  // During the beta a sibling's trial runs on past its date unless that account was deleted.
  const running = spent.expires_at > now || (env.REMOTE_BETA_FREE === '1' && spent.enabled === 1 && spent.deleted !== 1)
  throw new Refusal(running ? 'trialActiveElsewhere' : 'trialEnded')
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
/** The browser credential's lifetime in its cookie, in seconds: the longest a browser keeps one (Chrome
 *  caps Max-Age at 400 days). An approval no longer ends by itself (`UNTIL_REMOVED`), so the credential is
 *  re-issued with this whenever its browser is admitted, and only a browser left unused that long loses it. */
export const COOKIE_MAX_AGE_S = 400 * 86400
export function cookie(stationId: string, value: string, maxAge = COOKIE_MAX_AGE_S): string {
  return `${cookieName(stationId)}=${value}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=${maxAge}`
}
/** How long an approval lasts (operator decision 2026-10-10, replacing the 2026-09-14 rule of thirty days
 *  from last use and ninety at most): until it is removed. It never lapses, used or not; only Remove at
 *  the shack, the device forgetting itself, or revoking the station ends it.
 *
 *  An approved device's `expires_at` is this, the last millisecond of the year 9999. Every Nexus already
 *  holds a device to `expiresAt > now` and nothing else (1.17 and 1.18 filter and restore by it, and show
 *  it as a date: "9999-12-31"), so to all of them it is an approval that does not end; it fits a safe
 *  integer, an i64 and a u64. A Nexus that asks for kinds is also told so by name (`untilRemoved`), and so
 *  is the browser's own page. */
export const UNTIL_REMOVED = Date.UTC(9999, 11, 31, 23, 59, 59, 999)
/** Sent by a Nexus that binds its remembered grants to the approval GENERATION. Only that Nexus is
 *  shown the generation, and only approvals it gives record an approval time: 1.12.0 binds restored
 *  grants to the expiry (moving it would silently drop them) and parses the device list with
 *  deny_unknown_fields (an added field fails every refresh). */
export function lifetime(request: Request): boolean {
  return request.headers.get('x-nexus-device-lifetime') === '1'
}
/** Sent by a Nexus that pins browsers' device keys (A5). Only that Nexus is listed them: 1.13 and older
 *  parse the device list with deny_unknown_fields, so a key listed to them would fail every refresh. */
export function deviceKeys(request: Request): boolean {
  return request.headers.get('x-nexus-device-key') === '1'
}
/** Sent by a Nexus that knows the native client's devices. Only that Nexus is listed them, with every
 *  device's kind and platform: an older one could show a computer only as a browser, and parses the
 *  list with deny_unknown_fields besides. */
export function deviceKinds(request: Request): boolean {
  return request.headers.get('x-nexus-device-kind') === '1'
}
/** Does this approval last until it is removed? */
export function untilRemoved(row: Pick<DeviceRow, 'approved' | 'expires_at'>): boolean {
  return row.approved === 1 && row.expires_at >= UNTIL_REMOVED
}
/** Each approval of this station that still stands, given by a Nexus that binds its grants to the
 *  generation (`approved_at` recorded), made one that lasts until removed: the approvals given under the
 *  thirty/ninety-day rule. The generation is not moved, so every grant bound to it stands, and nothing
 *  ended is revived. An approval given by an older Nexus (no approval time) keeps its fixed end, because
 *  that Nexus binds its restored grants to the expiry. Asked for whenever the station connects or lists
 *  its devices, and as a device is used. */
export async function keepApprovals(env: RemoteEnv, stationId: string, now: number, deviceId?: string): Promise<void> {
  await env.DB.prepare(`UPDATE devices SET expires_at=? WHERE station_id=? AND approved=1 AND approved_at IS NOT NULL
    AND expires_at>? AND expires_at<?${deviceId === undefined ? '' : ' AND id=?'}`)
    .bind(...[UNTIL_REMOVED, stationId, now, UNTIL_REMOVED, ...(deviceId === undefined ? [] : [deviceId])]).run()
}
export type DeviceRow = { id: string; name: string; generation: number; approved: number; approved_at: number | null; expires_at: number
  public_key: string | null }
export async function device(request: Request, env: RemoteEnv, stationId: string, owner: string, now: number): Promise<DeviceRow | null> {
  const credential = deviceCookie(request, stationId)
  if (!credential) return null
  return env.DB.prepare(`SELECT id,name,generation,approved,approved_at,expires_at,public_key FROM devices
    WHERE station_id=? AND account_id=? AND credential_hash=? AND expires_at>?`)
    .bind(stationId, owner, await digest(credential), now).first<DeviceRow>()
}

/** What a native request's signature covers begins with this, so a signature made for one is never one
 *  for anything else a device key signs (a stream's offer, the older lanes' proofs). */
export const NATIVE_REQUEST = 'nexus-native-request/1'
/** How long a challenge waits for the request it was asked for. */
export const CHALLENGE_MS = 60000
const fromHex = (hex: string) => new Uint8Array(hex.length / 2).map((_, i) => parseInt(hex.slice(2 * i, 2 * i + 2), 16))
/** A native request signed by `key`: its `x-nexus-signature` (ECDSA P-256 over SHA-256, IEEE P1363
 *  r||s, lowercase hex) over five lines - the label, its `x-nexus-challenge`, the method, the path,
 *  and SHA-256 of the body exactly as sent, in lowercase hex.
 *
 *  The challenge is one this service gave the same account within the last minute, and it is DELETED
 *  as it is read, before any signature is tried. So a request is taken once: a replay finds nothing,
 *  and a challenge another account was given, or one past its minute, is refused the same way.
 *  DELETE ... RETURNING is what makes that hold across concurrent Worker instances, as for tickets.
 *  `deviceId` is the reopening device the challenge was given to, or null for a signed-in caller's:
 *  each takes only its own. */
export async function signedBy(request: Request, env: RemoteEnv, accountId: string, deviceId: string | null, key: string, bytes: Uint8Array, now: number): Promise<void> {
  const challenge = request.headers.get('x-nexus-challenge') ?? '', signature = request.headers.get('x-nexus-signature') ?? ''
  requireValue(/^[0-9a-f]{64}$/.test(challenge) && /^[0-9a-f]{128}$/.test(signature), 'invalidRequest', 400)
  const taken = await env.DB.prepare('DELETE FROM challenges WHERE digest=? AND account_id=? AND device_id IS ? AND expires_at>? RETURNING digest')
    .bind(await digest(challenge), accountId, deviceId, now).first()
  requireValue(taken, 'invalidChallenge', 401)
  const sent = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), b => b.toString(16).padStart(2, '0')).join('')
  const signed = new TextEncoder().encode([NATIVE_REQUEST, challenge, request.method, new URL(request.url).pathname, sent].join('\n'))
  let valid = false
  try {
    const verifier = await crypto.subtle.importKey('spki', fromHex(key), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify'])
    valid = await crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, verifier, fromHex(signature), signed)
  } catch { valid = false }
  requireValue(valid, 'invalidSignature', 401)
}
/** The native device a request names (`x-nexus-device-id`), of this station and account and not
 *  lapsed, or null. A browser's row is never one, whatever its id: a native caller is never admitted on
 *  a browser's approval, and a browser never on a native device's. */
export async function nativeDevice(request: Request, env: RemoteEnv, stationId: string, accountId: string, now: number): Promise<DeviceRow | null> {
  return env.DB.prepare(`SELECT id,name,generation,approved,approved_at,expires_at,public_key FROM devices
    WHERE id=? AND station_id=? AND account_id=? AND kind IS NOT NULL AND expires_at>?`)
    .bind(id(request.headers.get('x-nexus-device-id')), stationId, accountId, now).first<DeviceRow>()
}
/** A native device's kind and the platforms it may name, as the shack shows them at approval. */
const PLATFORMS: Record<string, readonly string[]> = { computer: ['windows', 'macos', 'linux'], phone: ['ios', 'android'] }
export function nativeKind(kind: unknown, platform: unknown): { kind: string; platform: string } {
  requireValue(typeof kind === 'string' && typeof platform === 'string' && Object.hasOwn(PLATFORMS, kind) &&
    PLATFORMS[kind].includes(platform), 'invalidRequest', 400)
  return { kind, platform }
}
