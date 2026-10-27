// Same-origin browser API, the native client's API, and outbound station admission. No radio command router.
import { account, access, body, bodyBytes, browserOrigin, caller, caller as callerKey, CHALLENGE_MS, cookie, device, deviceAddedNotice, deviceCookie, deviceKeys, deviceNonce, digest,
  deviceKinds, fieldsOf, id, keepApprovals, label, lifetime, mailbox, mailboxHash, memberRole, native, nativeAccount, nativeDevice, nativeKind, proof, publicKey, rate, readEntitlement, Refusal, requireAdmin,
  lanEntitlement, requireMayPair, requireTrial, requireUnspentIdentity, requireValue, secret, signedBy, station, stationNonce, TRIAL_MS, UNTIL_REMOVED, untilRemoved, uuid, verifyProof } from './authority'
import type { AccountEntitlement, DeviceRow, RemoteEnv, Role, StationRow } from './authority'
import { observerDeadline } from '../../ui/src/remote-monitor/relay'
import { baseCall, hmac, markerInserts, requireUnspentMarkers, SPENT_MARKER, spentBindings, trialClaim, trialInput, trialProof } from './trial-identity'
import type { TrialInput } from './trial-identity'
import type { Identity } from '../../ui/src/remote-monitor/relay'
import { advertisedOperationVersion } from '../../ui/src/remote-web/operation-version'
export { StationRoom } from './room'

/** How long a station's connection is admitted before it has to connect again. A stream is a lane of
 *  one station connection and ends with it, so this is also the longest a stream can last. */
const STATION_ADMISSION_MS = 86400000
const DAY_MS = 86400000

// SIGN IN ONCE. Decisions the operator has not yet taken, each built as its recommendation and held in
// one place so that taking the other answer is one edit.
/** When an account's trial starts. false (recommended): at the station's first internet use (a ticket
 *  that is not LAN-only, a pipe, a relay credential), so an operator who uses only the LAN never spends
 *  it. true: when the shack signs in and binds the station, closest to the old rule (at approval). */
const TRIAL_STARTS_AT_BINDING = false
/** Whose plan a guest's internet use reads. true (recommended): the station owner's; a guest needs only
 *  a free account. false: the guest's own as well. */
const GUESTS_ON_OWNER_PLAN = true
/** Whether a device's sign-in approves it at a station that has no station proof (one paired by code
 *  whose operator has not yet confirmed the account there). true (recommended): it does not, and the
 *  device waits for the station as before. false: the account binding alone is trusted. */
const AUTO_APPROVE_NEEDS_STATION_PROOF = true
/** Bounds on what a signed-in account may do a day: bind (or confirm) a station, have a device
 *  approved by its sign-in, and, a station, invite people. */
const BINDINGS_PER_DAY = 5
const ENROLMENTS_PER_DAY = 5
const INVITES_PER_DAY = 20
/** The people a station may have invited at once. */
const INVITES_PER_STATION = 20
/** Checkouts and portal pages an account may open a minute: each one is a call to the payment provider. */
const BILLING_PER_MINUTE = 10
/** Account deletions and copies an account may ask for in ten minutes. */
const ACCOUNT_DATA_PER_TEN_MINUTES = 5
/** A subscription in these states has ended and can charge nothing more; any other can. */
const SUBSCRIPTION_ENDED = ['canceled', 'incomplete_expired']
/** The billing Worker's refusals the page can name. Anything else it answers is `serviceUnavailable`. */
const BILLING_REFUSALS = new Set(['invalidRequest', 'accountNotFound', 'accessDenied', 'alreadySubscribed', 'noSubscription', 'serviceUnavailable'])

function json(value: unknown, status = 200, headers?: HeadersInit): Response {
  const result = new Response(JSON.stringify(value), { status, headers })
  result.headers.set('content-type', 'application/json')
  result.headers.set('cache-control', 'no-store')
  result.headers.set('x-content-type-options', 'nosniff')
  result.headers.set('referrer-policy', 'no-referrer')
  return result
}
/** The billing Worker's answer as the page gets it: a page on the provider's own domain to go to, or a
 *  refusal by a name the page knows. Never a URL anywhere else, and never anything the Worker said besides. */
function billingAnswer(answer: unknown, page: string): Response {
  const value = answer as { url?: unknown; error?: unknown; status?: unknown } | null
  if (typeof value?.url === 'string' && value.url.startsWith(page)) return json({ url: value.url })
  const named = typeof value?.error === 'string' && BILLING_REFUSALS.has(value.error)
    && typeof value.status === 'number' && Number.isInteger(value.status) && value.status >= 400 && value.status <= 599
  throw named ? new Refusal(value.error as string, value.status as number) : new Refusal('serviceUnavailable', 503)
}
async function room(env: RemoteEnv, stationId: string, path: string, value?: unknown): Promise<Response> {
  // Construct fresh internal headers. Public requests cannot supply an admission.
  const upgrade = path === 'station' || path === 'browser' || path === 'client-pipe' || path === 'station-pipe'
  return env.STATIONS.get(env.STATIONS.idFromName(stationId)).fetch(new Request(`https://station.internal/${path}`, {
    method: upgrade ? 'GET' : 'POST',
    headers: upgrade ? { upgrade: 'websocket', 'x-nexus-admission': JSON.stringify(value) } : { 'content-type': 'application/json' },
    body: upgrade ? undefined : JSON.stringify(value ?? {}),
  }))
}
async function sync(env: RemoteEnv, row: StationRow, now: number): Promise<void> {
  const response = await room(env, row.id, 'policy', { access: await access(env, row, now) })
  requireValue(response.ok, 'serviceUnavailable', 503)
}

async function api(request: Request, env: RemoteEnv): Promise<Response> {
  const url = new URL(request.url), now = Date.now()
  requireValue(url.origin === env.PUBLIC_REMOTE_ORIGIN && !url.search, 'originDenied')
  const path = url.pathname.slice('/api/remote/'.length)
  if (request.method === 'GET' && path === 'config') return json({
    issuer: env.AUTH0_ISSUER, audience: env.AUTH0_AUDIENCE, clientId: env.AUTH0_CLIENT_ID,
    // The Native application Nexus signs in to for the internet road, or null while there is none:
    // never the page's own, which no native route takes.
    nativeClientId: nativeClientId(env),
    ready: env.AUTH0_CLIENT_ID !== 'unconfigured',
    revision: env.REMOTE_BUILD_REVISION ?? 'local',
    operationVersion: 2,
    operationMaxVersion: 3,
    operationFtVersion: 1,
    // Operation v5: the station pushes a settled control's outcome. A page that cannot see this
    // number negotiates v4 and polls exactly as before.
    operationPushVersion: 1,
    // A service rolled back past the audio lane reports nothing here, and a browser
    // that cannot see this number never offers listening at all.
    audioVersion: 1,
    // The stream's signalling lane. A page that cannot see this number never offers a stream.
    streamVersion: 1,
    // Subscribe and Manage: true only where the billing Worker is bound. The page shows neither button
    // otherwise, and an older service never says so.
    billing: env.BILLING !== undefined,
  })
  const match = /^stations\/([0-9a-f-]{36})\/(.+)$/.exec(path)
  // The native client's routes (Nexus on another computer), on its own token and its device key's
  // signature, with no Origin and no cookie. No other route takes that caller, and it takes no other.
  if (path.startsWith('client/') || match?.[2].startsWith('client/')) return client(request, env, path, match, now)
  if (request.method === 'GET' && match && ['connect', 'observe', 'pipe'].includes(match[2])) {
    const stationId = id(match[1])
    requireValue(request.headers.get('upgrade')?.toLowerCase() === 'websocket', 'webSocketRequired', 426)
    // The station's end of a native client's pipe (room.ts, pipe.ts): a second socket of its own,
    // opened when its main socket was asked to, for the pipe that message named.
    if (match[2] === 'pipe') {
      const row = await native(request, env, stationId)
      await rate(env, `station-pipe:${stationId}`, now, 60)
      return room(env, stationId, 'station-pipe', { access: await access(env, row, now), pipeId: id(request.headers.get('x-nexus-pipe-id')),
        identity: { stationId, accountId: row.account_id, generation: row.generation, expiresAt: now + STATION_ADMISSION_MS } })
    }
    if (match[2] === 'connect') {
      const row = await native(request, env, stationId)
      await rate(env, `connect:${stationId}`, now, 20)
      // Approvals given under the thirty/ninety-day rule last until removed from here on.
      await keepApprovals(env, stationId, now)
      return room(env, stationId, 'station', { access: await access(env, row, now),
        operationVersion: advertisedOperationVersion(
          ['1','2'].includes(request.headers.get('x-nexus-operation-version')??'') ? Number(request.headers.get('x-nexus-operation-version')) : 0,
          request.headers.get('x-nexus-operation-max-version') === '3' ? 3 : 0,
          request.headers.get('x-nexus-operation-ft-version') === '1' ? 1 : 0,
          request.headers.get('x-nexus-operation-push-version') === '1' ? 1 : 0,
        ),
        // Receive audio. Its own advertisement, independent of every other version:
        // a station that does not send this header never gets handed an `audioListen`,
        // whose unknown fields its message parser would refuse - taking the whole
        // control socket down with it.
        audioVersion: request.headers.get('x-nexus-audio-version') === '1' ? 1 : 0,
        // The stream's signalling lane, on the same terms: a station that does not send this header
        // is never handed a `streamSignal`.
        streamVersion: request.headers.get('x-nexus-stream-version') === '1' ? 1 : 0,
        // A LAN-only session's stream, on the same terms: only a station that sends this header is
        // handed one, with `lanOnly` on its signals, a field any other would close its socket on.
        streamLanVersion: request.headers.get('x-nexus-stream-lan-version') === '1' ? 1 : 0,
        // The browser's proof on the operation and audio lanes (S1-M1): handed on only to a station that
        // takes it, because any other refuses a field it does not know by closing its socket.
        laneVersion: request.headers.get('x-nexus-lane-signature-version') === '1' ? 1 : 0,
        // The native client's pipe, on the same terms: only a station that sends this header is ever
        // asked to open one (`pipeOpen`), a message any other would close its socket on.
        pipeVersion: request.headers.get('x-nexus-pipe-version') === '1' ? 1 : 0,
        identity: { stationId, accountId: row.account_id, generation: row.generation, expiresAt: now + STATION_ADMISSION_MS } })
    }
    browserOrigin(request, env)
    const protocols = (request.headers.get('sec-websocket-protocol') ?? '').split(',').map(s => s.trim())
    requireValue(protocols.length === 2 && protocols[0] === 'nexus-observe-v1' && /^ticket\.[0-9a-f]{64}$/.test(protocols[1]), 'invalidTicket', 401)
    // DELETE RETURNING makes replay refusal atomic across concurrent Worker instances.
    const ticket = await env.DB.prepare(`DELETE FROM tickets WHERE digest=? AND station_id=? AND expires_at>? RETURNING *`)
      .bind(await digest(protocols[1].slice(7)), stationId, now).first<{
        account_id: string; device_id: string; device_generation: number; identity_until: number; scope: string
      }>()
    requireValue(ticket, 'invalidTicket', 401)
    const row = await station(env, stationId)
    return room(env, stationId, 'browser', { access: await access(env, row, now), sessionId: uuid(),
      ...(ticket.scope === 'lan' ? { scope: 'lan' } : {}),
      identity: { accountId: ticket.account_id, deviceId: ticket.device_id,
        deviceGeneration: ticket.device_generation, expiresAt: ticket.identity_until },
      entitlement: await readEntitlement(env, ticket.account_id, now) })
  }
  requireValue(request.method === 'POST', 'methodNotAllowed', 405)

  if (path === 'enroll') {
    requireValue(!request.headers.has('origin'), 'originDenied')
    // PER-CALLER FIRST. `rate()` increments whenever a bucket is under its OWN limit, so checking
    // the shared bucket first let one refused caller spend a global slot on every rejected request:
    // ~1000 requests from a single host exhausted enroll-global and no operator anywhere could
    // pair a station until the fixed ten-minute window rolled. A caller already over its own
    // limit must never be able to touch the shared budget.
    //
    // WHY THE SHARED BUDGET CANNOT BE STARVED FROM ONE NETWORK. A caller is an IPv4 address or an
    // IPv6 /64 (see caller()), and every IPv6 caller is also capped per /48, in that order, before
    // anything reaches enroll-global. So one /64 can spend at most 5 shared slots and one /48 at
    // most 20, out of 1000: draining the pool takes 50 separate /48s or 200 separate IPv4
    // addresses, which is a distributed attacker and a job for an edge rule, not for this counter.
    // Keyed on the full address instead, one /64 spent the whole pool (security review H1).
    // The /48 cap still sits AFTER the /64 one, so a single flooding host cannot use up its
    // neighbours' share either. It also bounds rate_limits growth: at most one row per /64 the
    // caller holds, where a full-address key minted one per address.
    const { caller: from, network } = caller(request.headers.get('cf-connecting-ip'))
    await rate(env, `enroll:${from}`, now, 5, 600000)
    if (network) await rate(env, `enroll-network:${network}`, now, 20, 600000)
    // The sweep runs BEFORE the shared gate, and it is the service's only garbage collection:
    // enrollments, tickets and rate_limits are reaped nowhere else and there is no cron trigger.
    // Behind the shared gate, exhausting that bucket also stopped all cleanup - the denial and
    // the unbounded growth were the same bug, and the growth outlived the window.
    await env.DB.batch([
      env.DB.prepare('DELETE FROM enrollments WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM tickets WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM rate_limits WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM challenges WHERE expires_at<=?').bind(now),
    ])
    await rate(env, 'enroll-global', now, 1000, 600000)
    const input = await body(request, ['name']), stationId = uuid(), credential = secret(), code = secret().slice(0, 16)
    await env.DB.prepare('INSERT INTO enrollments(id,name,proof_hash,code_hash,expires_at) VALUES(?,?,?,?,?)')
      .bind(stationId, label(input.name), await digest(credential), await digest(code), now + 600000).run()
    return json({ id: stationId, proof: credential, code, expiresAt: now + 600000 })
  }
  if (path === 'enroll/check' || path === 'enroll/approve') {
    requireValue(!request.headers.has('origin'), 'originDenied')
    // `callsign` and `pc` are optional and approve-only: the shack's station callsign and its PC's two
    // identifier hashes, which a trial started here spends (trial-identity.ts). Accepted before any
    // Nexus sends them, because this body refuses a key it does not know.
    const input = path.endsWith('approve') ? await body(request, ['id', 'proof', 'credential'], ['callsign', 'pc'])
      : await body(request, ['id', 'proof'])
    const stationId = id(input.id), hash = await digest(proof(input.proof))
    // Bounded by CALLER before it is bounded by the id the caller chose. The per-station bucket
    // is keyed on attacker-supplied input, and rate() writes one rate_limits row per distinct
    // key, so without this a stranger could mint unbounded rows by sending fresh random UUIDs -
    // each refused 410, each leaving a row behind. The caller is an IPv4 address or an IPv6 /64,
    // with a /48 cap behind it, for the reason given at `enroll`: keyed on the full address, one
    // /64 rotating its addresses minted those rows without limit again.
    const { caller: from, network } = caller(request.headers.get('cf-connecting-ip'))
    await rate(env, `enrollcheck:${from}`, now, 60, 600000)
    if (network) await rate(env, `enrollcheck-network:${network}`, now, 240, 600000)
    await rate(env, `enrollment:${stationId}`, now, 30)
    const pending = await env.DB.prepare('SELECT account_id,approved,confirmed FROM enrollments WHERE id=? AND proof_hash=? AND expires_at>?')
      .bind(stationId, hash, now).first<{ account_id: string | null; approved: number; confirmed: number }>()
    requireValue(pending, 'pairingExpired', 410)
    // Exactly these two fields, and NEVER a third. The shack parses this with deny_unknown_fields
    // (src-tauri remote_service/mod.rs, `struct Check`), so an added field fails every check on every
    // Nexus build that predates it - pairing silently stops working. `confirmed` was added here for a
    // day and did precisely that. The browser learns it from session.pending; the shack learns it as
    // an `awaitingConfirmation` refusal from approve, which it can already parse.
    if (path.endsWith('check')) return json({ accountId: pending.account_id, approved: pending.approved === 1 })
    requireValue(pending.account_id, 'accountNotClaimed', 409)
    // THE GATE. Typing a code is not agreement to attach a station: a code the operator RECEIVED
    // from somebody else reaches exactly this point, and approval happens at the sender's own
    // shack, so the two-act protection cannot see it. Everything permanent is below this line -
    // the station row, its credential, and the trial clock that can never return to 'none'.
    requireValue(pending.confirmed === 1, 'awaitingConfirmation', 409)
    const before = await readEntitlement(env, pending.account_id, now)
    await requireMayPair(env, before, now)
    // A trial starts here only for an account that has neither had one nor holds a plan: paying never
    // touches the trials row. Its markers are checked first for a clear refusal, and again inside the
    // trial INSERT for the race.
    const starts = before.state === 'none'
    const claim = starts ? await trialClaim(env, input, pending.account_id, now) : null
    if (claim) await requireUnspentMarkers(env, pending.account_id, claim)
    const credentialHash = await digest(proof(input.credential))
    // Approval at the shack is the moment the trial starts, so the clock is written in the same
    // batch as the station. Station insert first on purpose: a trial with no station is
    // unrecoverable by the operator, whereas a station with no trial is repaired by the native
    // client's existing retry, which reuses the same credential. ON CONFLICT DO NOTHING against
    // the account_id primary key IS the concurrency argument, and it is also "one trial, ever"
    // written in SQL - two approvals racing cannot produce two clocks.
    await env.DB.batch([
      env.DB.prepare(`INSERT OR IGNORE INTO stations(id,account_id,name,credential_hash)
        SELECT id,account_id,name,? FROM enrollments e WHERE id=? AND proof_hash=? AND expires_at>?
        AND account_id IS NOT NULL AND approved=0 AND (SELECT COUNT(*) FROM stations s WHERE s.account_id=e.account_id AND s.enabled=1)<2`)
        .bind(credentialHash, stationId, hash, now),
      env.DB.prepare(`UPDATE enrollments SET approved=1 WHERE id=? AND proof_hash=?
        AND EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)`)
        .bind(stationId, hash, stationId, credentialHash),
      // The browser that confirmed this pairing is approved with it (one approval at the shack).
      // Bound to the station row carrying THIS credential, like the trial below, and skipped when a
      // device already holds the digest, so a retried approve adds nothing. The name is a label the
      // shack shows beside the browser code; the browser never asked to be named.
      // It lasts until removed (authority.ts, `UNTIL_REMOVED`). The approval time is recorded only for a
      // Nexus that binds its grants to the generation (authority.ts, `lifetime`). The device key (A5) is
      // the one the confirming browser brought, and none from a page that sent none.
      env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,approved,approved_at,expires_at,public_key)
        SELECT ?,e.id,e.account_id,'Paired browser',e.device_hash,1,?,?,e.public_key FROM enrollments e
        WHERE e.id=? AND e.proof_hash=? AND e.device_hash IS NOT NULL
        AND EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)
        AND NOT EXISTS(SELECT 1 FROM devices d WHERE d.station_id=e.id AND d.credential_hash=e.device_hash)`)
        .bind(uuid(), lifetime(request) ? now : null, UNTIL_REMOVED, stationId, hash, stationId, credentialHash),
      // ONE TRIAL PER MAILBOX, DECIDED IN SQL. `ON CONFLICT(account_id)` is the concurrency
      // argument for one ACCOUNT racing itself, and it always was; it is not one for one PERSON,
      // because two sign-ins by the same human are two account rows and conflict with nothing.
      // requireUnspentIdentity above is a check-then-write: two approvals in flight together both
      // read "no sibling trial" before either wrote one, and both got a fourteen-day clock. The
      // NOT EXISTS below closes that, and it closes it in the same statement as the write, so
      // SQLite's single-writer serialization is what makes it atomic rather than a claim about
      // where the await points fall. It is deliberately the same sibling test requireUnspentIdentity
      // makes - mailbox hash, or an address hash recorded before that column existed - so the two
      // cannot drift into disagreeing about who is the same person. An account with no verified
      // address matches no sibling and is unaffected, which is the documented existing behaviour.
      ...(starts ? [env.DB.prepare(`INSERT INTO trials(account_id, enabled, expires_at, started_at, source)
        SELECT ?,1,?,?,'trial' WHERE EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)
        AND NOT EXISTS(SELECT 1 FROM trials t JOIN accounts a ON a.id=t.account_id, accounts self
          WHERE self.id=? AND t.account_id<>self.id
          AND (a.mailbox_hash=self.mailbox_hash OR a.email_hash IN (self.email_hash, self.mailbox_hash)))
        ${claim ? `AND NOT EXISTS(SELECT 1 FROM trial_markers m WHERE ${SPENT_MARKER})` : ''}
        ON CONFLICT(account_id) DO NOTHING`)
        .bind(pending.account_id, now + TRIAL_MS, now, stationId, credentialHash, pending.account_id,
          ...(claim ? spentBindings(pending.account_id, claim) : [])),
      // What the trial spent, only if it was this approval that started it.
      ...(claim ? markerInserts(env, pending.account_id, claim, now) : [])] : []),
    ])
    // Read both rows back before answering 200. That makes "the station and the clock landed
    // together" an observable property of the response rather than a claim about D1's rollback
    // semantics, which are unproven here: every test runs against local Miniflare D1.
    const accepted = await env.DB.prepare('SELECT id FROM stations WHERE id=? AND credential_hash=? AND enabled=1')
      .bind(stationId, credentialHash).first()
    requireValue(accepted, 'stationLimit', 409)
    const started = await readEntitlement(env, pending.account_id, now)
    // Losing the race above is not a service failure. The NOT EXISTS guard is what makes one
    // trial per person true under concurrency, and the loser deserves the same named refusal a
    // sequential caller would have been given - trialActiveElsewhere - rather than a bare 503
    // that reads as "try again", which is exactly what it must not do.
    if (started.state !== 'active') {
      await requireUnspentIdentity(env, pending.account_id, now)
      if (claim) await requireUnspentMarkers(env, pending.account_id, claim)
    }
    requireValue(started.state === 'active', 'serviceUnavailable', 503)
    // The approved pairing browser, so the shack can give it station control and logging without
    // waiting for a browser list. Additive: the shack reads stationId and accountId by name and has
    // always ignored anything else here. `null` when the confirm came from a browser of an older era.
    // `generation` is additive here too: the shack reads `device.id` and `device.expiresAt` by name.
    // So is `publicKey` (A5), the key a Nexus that pins keys pins for this browser with the pairing.
    const paired = await env.DB.prepare(`SELECT d.id, d.expires_at AS expiresAt, d.generation, d.public_key AS publicKey FROM devices d
      JOIN enrollments e ON d.station_id=e.id AND d.credential_hash=e.device_hash
      WHERE e.id=? AND e.proof_hash=? AND d.approved=1 AND d.expires_at>?`)
      .bind(stationId, hash, now).first<{ id: string; expiresAt: number; generation: number; publicKey: string | null }>()
    return json({ stationId, accountId: pending.account_id, entitlement: started, device: paired ?? null })
  }

  if (match?.[2].startsWith('native/')) {
    const verb = match[2].slice(7)
    const row = await native(request, env, match[1], verb === 'revoke')
    await rate(env, `native:${row.id}`, now, 60)
    if (verb === 'devices') {
      await body(request, [])
      // The generation (and `renewsUntil`, null now that no approval renews: each lasts until removed) go
      // ONLY to a Nexus that asks. 1.12.0 parses this list with deny_unknown_fields, so a field added for
      // everyone would fail every refresh it makes. The device key (A5) likewise, on its own header.
      // The native client's devices go only to a Nexus that asks for kinds, and with their kind and
      // platform, for its approval pop-up; a browser is listed there as kind 'browser' on no platform.
      // That Nexus is also told, by name, which approvals last until removed (`untilRemoved`, 1 or 0).
      await keepApprovals(env, row.id, now)
      // A device removed on the web is left out of this list, so the station has now heard of it.
      await env.DB.prepare('UPDATE devices SET revoke_seen_at=? WHERE station_id=? AND revoked_at IS NOT NULL AND revoke_seen_at IS NULL')
        .bind(now, row.id).run()
      const kinds = deviceKinds(request)
      // A Nexus that checks proofs itself (`x-nexus-device-proof`) is also told each device's role, how
      // it was added, its proof and a computer's LAN key: the proof's nonce covers both of its keys.
      const key = (deviceKeys(request) ? ',public_key AS publicKey' : '') +
        (kinds ? ",COALESCE(kind,'browser') AS kind,platform,CASE WHEN approved=1 AND expires_at>=? THEN 1 ELSE 0 END AS untilRemoved" : '') +
        (request.headers.get('x-nexus-device-proof') === '1' ? ",COALESCE(role,'operate') AS role,COALESCE(added_by,'shack') AS addedBy,proof,lan_key AS lanKey,EXISTS(SELECT 1 FROM notices n WHERE n.device_id=devices.id AND n.sent_at IS NOT NULL) AS notified" : '')
      const forever = kinds ? [UNTIL_REMOVED] : []
      const only = kinds ? '' : ' AND kind IS NULL'
      const devices = lifetime(request)
        ? await env.DB.prepare(`SELECT id,name,approved,expires_at AS expiresAt,generation,NULL AS renewsUntil${key}
            FROM devices WHERE station_id=? AND expires_at>?${only} ORDER BY id LIMIT 8`).bind(...forever, row.id, now).all()
        : await env.DB.prepare(`SELECT id,name,approved,expires_at AS expiresAt${key} FROM devices WHERE station_id=? AND expires_at>?${only} ORDER BY id LIMIT 8`)
          .bind(...forever, row.id, now).all()
      return json({ devices: devices.results })
    }
    if (verb === 'approve-device' || verb === 'revoke-device') {
      const input = await body(request, ['deviceId']), deviceId = id(input.deviceId)
      const found = await env.DB.prepare('SELECT id FROM devices WHERE id=? AND station_id=? AND expires_at>?')
        .bind(deviceId, row.id, now).first()
      requireValue(found, 'deviceUnavailable', 404)
      const approve = verb === 'approve-device'
      // Every approval, including approving again, is a new generation, and lasts until removed. Revoking
      // (Remove) ends it now and clears the time.
      await env.DB.batch([
        env.DB.prepare('UPDATE devices SET approved=?,generation=generation+1,expires_at=?,approved_at=? WHERE id=? AND station_id=?')
          .bind(approve ? 1 : 0, approve ? UNTIL_REMOVED : now, approve && lifetime(request) ? now : null, deviceId, row.id),
        env.DB.prepare('UPDATE stations SET policy_version=policy_version+1 WHERE id=?').bind(row.id),
      ])
      await sync(env, await station(env, row.id), now)
      return json({ ok: true })
    }
    if (verb === 'revoke') {
      await body(request, [])
      await revokeStation(env, row.id, now)
      return json({ ok: true })
    }
    // People, invited at the station by address with a role. The service keeps only the mailbox's hash;
    // the station keeps the address, and checks a guest's proof against it itself.
    if (verb === 'invite') {
      const input = await body(request, ['email', 'role']), hash = await mailboxHash(input.email)
      requireValue(input.role === 'operate' || input.role === 'listen', 'invalidRequest', 400)
      await rate(env, `invite:${row.id}`, now, INVITES_PER_DAY, DAY_MS)
      const invited = await env.DB.prepare(`INSERT INTO invites(id,station_id,mailbox_hash,role,created_at)
        SELECT ?,?,?,?,? WHERE (SELECT COUNT(*) FROM invites WHERE station_id=? AND removed_at IS NULL AND mailbox_hash<>?)<?
        ON CONFLICT(station_id,mailbox_hash) DO UPDATE SET role=excluded.role, removed_at=NULL, created_at=excluded.created_at RETURNING id`)
        .bind(uuid(), row.id, hash, input.role, now, row.id, hash, INVITES_PER_STATION).first()
      requireValue(invited, 'inviteLimit', 409)
      return json({ ok: true })
    }
    if (verb === 'uninvite') {
      const input = await body(request, ['email'])
      await removeInvite(env, row, await mailboxHash(input.email), now, null)
      return json({ ok: true })
    }
    if (verb === 'key') {
      // The station's own signing key (security review S3-M1), recorded once in its pairing record.
      // The first key a station sends is kept, the same one again is answered as kept, and any other is
      // refused by name: a key changes only by pairing again, which makes a new station. Its pages
      // check the station's stream answers against this key, read from here, never through the relay.
      const input = await body(request, ['publicKey']), key = await publicKey(input.publicKey)
      await env.DB.prepare('UPDATE stations SET public_key=? WHERE id=? AND public_key IS NULL').bind(key, row.id).run()
      const kept = await env.DB.prepare('SELECT public_key FROM stations WHERE id=?').bind(row.id).first<{ public_key: string | null }>()
      requireValue(kept?.public_key === key, 'stationKeyPinned', 409)
      return json({ ok: true })
    }
    throw new Refusal('notFound', 404)
  }

  // Every remaining operation is account-authenticated and subject to exact Origin.
  // Destructured off deliberately: `identity` is spread into the browser identity below and on
  // into relay socket state, and the provider subject has no business travelling with it.
  // `verified` is destructured off for the same reason `subject` is: `identity` is spread into
  // the browser identity and on into relay socket state, and how this account was identified is
  // not something the wire needs to carry. `email` likewise — it is kept nowhere (only its hashes
  // are), and is read here only transiently to notify the owner when a device is added by sign-in.
  const { subject, verified, email, ...identity } = await account(request, env, now)
  await rate(env, `account:${identity.accountId}`, now, 120)
  const entitlement = await readEntitlement(env, identity.accountId, now)
  // The only privileged write in the service. It exists because every trial in a closed beta is
  // granted by hand, and the alternative was a wrangler command the operator can only run from a
  // laptop. Deliberately ONE action: each further admin verb is another way for a mistake here to
  // become an entitlement bypass.
  if (path === 'admin/grant-trial') {
    requireAdmin(env, subject)
    // `callsign`, optional: support settling a squatted callsign records this account as its proven
    // owner, so its future claims are never blocked by the squatter's. It writes no other marker.
    const input = await body(request, ['accountId', 'days'], ['callsign'])
    const target = id(input.accountId)
    const call = 'callsign' in input ? baseCall(input.callsign) : null
    requireValue(call === null || env.TRIAL_KEY, 'serviceNotConfigured', 503)
    requireValue(Number.isInteger(input.days) && (input.days as number) >= 1 && (input.days as number) <= 365,
      'invalidRequest', 400)
    const expiresAt = now + (input.days as number) * 86400000
    // Selecting FROM accounts means an id that does not exist grants nothing rather than leaving
    // an orphan row. UPDATE on conflict, never DELETE: the trials row is the durable proof an
    // account consumed its trial, and removing it re-opens the reinstall and re-pair abuse the
    // one-trial-ever rule exists to refuse.
    //
    // The first grant's `started_at` is KEPT, and a self-serve trial that is later extended by
    // hand records BOTH as 'trial+manual'. Overwriting them destroyed exactly the distinction
    // 0002_trial.sql and grant-trial.mjs both exist to preserve - that a hand grant and a
    // self-serve trial stay tellable apart forever. After an extension the honest answer to "did
    // this account ever earn a trial on its own?" is yes, and the row should still say so.
    await env.DB.prepare(`INSERT INTO trials(account_id, enabled, expires_at, started_at, source)
      SELECT id, 1, ?, ?, 'manual' FROM accounts WHERE id=?
      ON CONFLICT(account_id) DO UPDATE SET enabled=1, expires_at=excluded.expires_at,
        started_at=COALESCE(trials.started_at, excluded.started_at),
        source=CASE WHEN trials.source='trial' THEN 'trial+manual' ELSE 'manual' END`)
      .bind(expiresAt, now, target).run()
    const granted = await readEntitlement(env, target, now)
    requireValue(granted.state === 'active', 'stationUnavailable', 404)
    if (call !== null) {
      await env.DB.prepare(`INSERT INTO trial_markers(kind,value_hmac,account_id,proven,method,created_at) VALUES('call',?,?,1,'admin',?)
        ON CONFLICT(kind,value_hmac,account_id) DO UPDATE SET proven=1, method='admin'`)
        .bind(await hmac(env.TRIAL_KEY!, `call:${call}`), target, now).run()
    }
    return json({ accountId: target, entitlement: granted })
  }
  if (path === 'session') {
    await body(request, [])
    const rows = await env.DB.prepare('SELECT id,name,enabled,public_key FROM stations WHERE account_id=? AND enabled=1 ORDER BY id LIMIT 2')
      .bind(identity.accountId).all<{ id: string; name: string; enabled: number; public_key: string | null }>()
    // The browser's own fields as they have always been (`renewsUntil` now always null: no approval
    // renews), and `untilRemoved` for an approval that lasts until removed. The approval time itself stays
    // in the Worker.
    const stations = await Promise.all(rows.results.map(async row => {
      const current = await device(request, env, row.id, identity.accountId, now)
      // `stationKey` (S3-M1): the station's own signing key, which its stream answers must carry a
      // signature from; null for a station that has not sent one, whose streams the page refuses.
      return { id: row.id, name: row.name, stationKey: row.public_key, device: current && { id: current.id, name: current.name, generation: current.generation,
        approved: current.approved, expires_at: current.expires_at, renewsUntil: null, untilRemoved: untilRemoved(current),
        publicKey: current.public_key } }
    }))
    // A claim is durable on the enrollment row, but nothing put it on the wire, so the browser
    // could only remember "waiting for approval" in component state. A reload lost it, the
    // operator saw the pairing form again, re-typed the code and was refused - the claim UPDATE
    // requires account_id IS NULL - and concluded pairing had failed when it had actually worked.
    // SELECT the columns the browser can show and NOTHING ELSE: code_hash and proof_hash are the
    // credentials this table exists to protect and must never leave the Worker. `name` is the
    // string the operator typed at their own shack and pair/claim already returns it.
    const claimed = await env.DB.prepare(`SELECT id,name,expires_at,confirmed FROM enrollments
      WHERE account_id=? AND approved=0 AND expires_at>? ORDER BY expires_at DESC LIMIT 1`)
      .bind(identity.accountId, now).first<{ id: string; name: string; expires_at: number; confirmed: number }>()
    const pending = claimed ? { id: claimed.id, name: claimed.name, expiresAt: claimed.expires_at, confirmed: claimed.confirmed === 1 } : null
    // `identityVerified` is the signal that the one-trial-per-person check is actually LIVE. It
    // is false whenever the token carried no verified address - which is exactly the state in which
    // that check silently protects nothing. Without it there is no way to tell the two apart from
    // outside, and a provider that never sends the claim looks identical to one that does.
    // `beta`: Remote is free during the beta (authority.ts `REMOTE_BETA_FREE`), so the page says that
    // instead of counting a trial's days.
    return json({ accountId: identity.accountId, entitlement, stations, pending,
      identityVerified: verified, serverNow: now, beta: env.REMOTE_BETA_FREE === '1' })
  }
  // SUBSCRIBE AND MANAGE, through the private billing Worker, which holds the payment keys. Above the
  // entitlement, as `session` is: an account whose trial has ended is exactly the one that subscribes, and
  // what it buys reaches it only as the webhook writes it and `readEntitlement` reads it, the one access
  // check. Only the signed-in account's id goes over. Until the billing Worker is bound these say so.
  if (path === 'billing/checkout' || path === 'billing/portal') {
    requireValue(env.BILLING, 'billingNotAvailable', 503)
    const checkout = path === 'billing/checkout', input = await body(request, checkout ? ['plan'] : [])
    requireValue(!checkout || input.plan === 'monthly' || input.plan === 'annual', 'invalidRequest', 400)
    await rate(env, `billing:${identity.accountId}`, now, BILLING_PER_MINUTE)
    return checkout
      ? billingAnswer(await env.BILLING.checkout(identity.accountId, input.plan as 'monthly' | 'annual'), 'https://checkout.stripe.com/')
      : billingAnswer(await env.BILLING.portal(identity.accountId), 'https://billing.stripe.com/')
  }
  // THE ACCOUNT ITSELF: a copy of what the service holds about it, and deleting it. Above the entitlement,
  // as `session` is: an account whose access has ended, or been switched off, may still take its copy and go.
  // Deleting needs the word the page has the operator type, so no stray request deletes anything.
  if (path === 'account/export' || path === 'account/delete') {
    const input = await body(request, path === 'account/delete' ? ['confirm'] : [])
    requireValue(path === 'account/export' || input.confirm === 'DELETE', 'invalidRequest', 400)
    await rate(env, `${path}:${identity.accountId}`, now, ACCOUNT_DATA_PER_TEN_MINUTES, 600000)
    if (path === 'account/export') return json(await accountCopy(env, identity.accountId, now))
    const left = await deleteAccount(env, identity.accountId, now)
    // This browser's credential for each station it held one at goes with them.
    const headers = new Headers()
    for (const stationId of left) headers.append('set-cookie', cookie(stationId, '', 0))
    return json({ ok: true }, 200, headers)
  }
  // THE ACCOUNT'S DEVICES, on the page: every device of every station the account owns, and its own
  // devices at stations it was invited to, each removable here. Above the entitlement, as `revoke` is:
  // removing a device is never something an account has to pay for.
  if (path === 'account/devices') {
    await body(request, [])
    const devices = await env.DB.prepare(`SELECT d.id, d.station_id AS stationId, s.name AS stationName, d.name,
      COALESCE(d.kind,'browser') AS kind, d.platform, COALESCE(d.role,'operate') AS role, COALESCE(d.added_by,'shack') AS addedBy,
      d.account_id<>s.account_id AS guest, d.approved, d.approved=0 AND d.revoked_at IS NOT NULL AND d.revoke_seen_at IS NULL AS removing
      FROM devices d JOIN stations s ON s.id=d.station_id
      WHERE s.enabled=1 AND (s.account_id=? OR d.account_id=?) AND (d.expires_at>? OR (d.revoked_at IS NOT NULL AND d.revoke_seen_at IS NULL))
      ORDER BY s.id, d.id LIMIT 64`).bind(identity.accountId, identity.accountId, now)
      .all<{ guest: number; approved: number; removing: number }>()
    const invites = await env.DB.prepare(`SELECT i.id, i.station_id AS stationId, s.name AS stationName, i.role, i.created_at AS createdAt
      FROM invites i JOIN stations s ON s.id=i.station_id WHERE s.account_id=? AND s.enabled=1 AND i.removed_at IS NULL
      ORDER BY i.created_at LIMIT 64`).bind(identity.accountId).all()
    return json({ devices: devices.results.map(d => ({ ...d, guest: d.guest === 1, approved: d.approved === 1, removing: d.removing === 1 })),
      invites: invites.results })
  }
  if (path === 'account/remove-all') {
    await body(request, [])
    const removed = await env.DB.prepare(`UPDATE devices SET approved=0,generation=generation+1,expires_at=?,proof=NULL,revoked_at=?,revoke_seen_at=NULL
      WHERE expires_at>? AND (account_id=? OR station_id IN (SELECT id FROM stations WHERE account_id=? AND enabled=1)) RETURNING station_id`)
      .bind(now, now, now, identity.accountId, identity.accountId).all<{ station_id: string }>()
    for (const stationId of new Set(removed.results.map(r => r.station_id))) await bumpPolicy(env, stationId, now)
    return json({ ok: true, removed: removed.results.length })
  }
  const removal = /^account\/(devices|invites)\/([0-9a-f-]{36})\/remove$/.exec(path)
  if (removal) {
    await body(request, [])
    if (removal[1] === 'devices') {
      // The account's own device anywhere, or any device of a station it owns; anything else is refused
      // alike, whether it exists or not.
      const found = await env.DB.prepare(`SELECT d.station_id FROM devices d JOIN stations s ON s.id=d.station_id
        WHERE d.id=? AND s.enabled=1 AND (s.account_id=? OR d.account_id=?) AND d.expires_at>?`)
        .bind(removal[2], identity.accountId, identity.accountId, now).first<{ station_id: string }>()
      requireValue(found, 'deviceUnavailable', 404)
      await env.DB.prepare('UPDATE devices SET approved=0,generation=generation+1,expires_at=?,proof=NULL,revoked_at=?,revoke_seen_at=NULL WHERE id=?')
        .bind(now, now, removal[2]).run()
      await bumpPolicy(env, found.station_id, now)
      return json({ ok: true })
    }
    // An invite of a station the account owns.
    const found = await env.DB.prepare(`SELECT i.station_id, i.mailbox_hash FROM invites i JOIN stations s ON s.id=i.station_id
      WHERE i.id=? AND s.account_id=? AND s.enabled=1 AND i.removed_at IS NULL`)
      .bind(removal[2], identity.accountId).first<{ station_id: string; mailbox_hash: string }>()
    requireValue(found, 'inviteUnavailable', 404)
    await removeInvite(env, await station(env, found.station_id), found.mailbox_hash, now, now)
    return json({ ok: true })
  }
  if (path === 'pair/claim') {
    await requireMayPair(env, entitlement, now)
    await rate(env, `claim:${identity.accountId}`, now, 5, 600000)
    const input = await body(request, ['code'])
    requireValue(typeof input.code === 'string' && /^[0-9a-f]{16}$/.test(input.code), 'invalidPairingCode', 400)
    const row = await env.DB.prepare(`UPDATE enrollments SET account_id=? WHERE code_hash=? AND expires_at>?
      AND account_id IS NULL AND approved=0 RETURNING id,name`)
      .bind(identity.accountId, await digest(input.code), now).first()
    requireValue(row, 'invalidPairingCode', 400)
    return json({ station: row, accountId: identity.accountId })
  }
  if (path === 'pair/confirm') {
    await requireMayPair(env, entitlement, now)
    await rate(env, `confirm:${identity.accountId}`, now, 10, 600000)
    const input = await body(request, ['id'], ['publicKey'])
    requireValue(typeof input.id === 'string' && /^[0-9a-f-]{36}$/.test(input.id), 'invalidPairingCode', 400)
    // The confirming browser's device key (A5), which the approval gives the device it creates.
    const key = 'publicKey' in input ? await publicKey(input.publicKey) : null
    // Scoped to this account's own unapproved claim. An id alone is not authority: another account
    // holding the same id confirms nothing, and a claim already approved is past the point this
    // gate protects.
    //
    // ONE APPROVAL (operator decision 2026-09-13): the browser that confirms is the browser that
    // did the pairing, and approving at the shack approves it too. It cannot have a device row yet
    // (a device references a station, which does not exist until approval), so it gets its device
    // credential now, as the same cookie `device` sets, and the enrollment keeps only the digest.
    // Confirming again from another browser moves that approval to the other browser.
    const credential = secret()
    const row = await env.DB.prepare(`UPDATE enrollments SET confirmed=1, device_hash=?, public_key=?
      WHERE id=? AND account_id=? AND approved=0 AND expires_at>? RETURNING id,name`)
      .bind(await digest(credential), key, input.id, identity.accountId, now).first<{ id: string; name: string }>()
    requireValue(row, 'invalidPairingCode', 400)
    return json({ station: row, accountId: identity.accountId }, 200, { 'set-cookie': cookie(row.id, credential) })
  }
  requireValue(match, 'notFound', 404)
  const row = await station(env, match[1]), verb = match[2]
  // The station's owner, or a person invited to it. A guest's devices are their own account's, and what
  // they do at the station runs under the station's account (`plan` below), as its owner's devices do.
  const role = row.enabled === 1 ? await memberRole(env, row, identity.accountId) : null
  requireValue(role, 'stationUnavailable', 404)
  // Above requireTrial, with revoke and forget-device: renaming is housekeeping on your own
  // account, and an operator whose service access has lapsed should still be able to tidy it.
  // `name` is not part of StationAccess, so no policy version moves and no room needs syncing -
  // this changes a label, not an authority.
  if (verb === 'rename' || verb === 'revoke') requireValue(role === 'owner', 'stationUnavailable', 404)
  if (verb === 'rename') {
    const input = await body(request, ['name'])
    const name = label(input.name)
    await env.DB.prepare('UPDATE stations SET name=? WHERE id=? AND account_id=?')
      .bind(name, row.id, identity.accountId).run()
    return json({ id: row.id, name })
  }
  if (verb === 'revoke') {
    await body(request, [])
    await revokeStation(env, row.id, now)
    return json({ ok: true }, 200, { 'set-cookie': cookie(row.id, '', 0) })
  }
  if (verb === 'forget-device') {
    await body(request, [])
    const current = await device(request, env, row.id, identity.accountId, now)
    if (current) {
      await env.DB.batch([
        env.DB.prepare('UPDATE devices SET approved=0,generation=generation+1,expires_at=? WHERE id=?').bind(now, current.id),
        env.DB.prepare('UPDATE stations SET policy_version=policy_version+1 WHERE id=?').bind(row.id),
      ])
      await sync(env, await station(env, row.id), now)
    }
    return json({ ok: true }, 200, { 'set-cookie': cookie(row.id, '', 0) })
  }
  // Registering a browser is above the entitlement: enrolling is free, as the LAN is.
  if (verb === 'device') {
    const input = await body(request, ['name'], ['publicKey', 'proof']), name = label(input.name)
    // The browser's device key (A5), when the page sends one.
    const key = 'publicKey' in input ? await publicKey(input.publicKey) : null
    // Its page sign-in's proof, over that key: what approves it with no press at the station.
    requireValue(!('proof' in input) || key !== null, 'invalidRequest', 400)
    const approveAs = 'proof' in input ? await vouch(env, row, role, input.proof, env.AUTH0_CLIENT_ID, subject,
      await deviceNonce('browser', [key!]), now) : null
    const current = await device(request, env, row.id, identity.accountId, now)
    if (current) {
      // A browser approved before it had a key registers one here, and one that lost its stored key
      // brings a new one. The approval is left exactly as it is: the station's pin, not this row,
      // decides whether a browser can stream, and a key the station has not pinned is approved again
      // at the radio. Only this browser's own credential reaches this row.
      if (key !== null && key !== current.public_key) {
        await env.DB.prepare('UPDATE devices SET public_key=? WHERE id=? AND station_id=?').bind(key, current.id, row.id).run()
      }
      // One still waiting is approved in place by a sign-in for its key.
      if (approveAs && current.approved !== 1) {
        await approveByAccount(env, row.id, current.id, approveAs, input.proof as string, null, identity.accountId, now)
        // The 2026-10-23 sign-in decision: notify the STATION OWNER that a device was added by
        // sign-in. The notice goes to the owner's own address, which we have only when the caller IS
        // the owner (`role === 'owner'`); a guest's sign-in carries the guest's address, not the
        // owner's, so it sends nothing and the device stays held. A no-op too without the provider
        // secret. Once: this branch runs only on the waiting→approved transition.
        await deviceAddedNotice(env, { accountId: identity.accountId, stationId: row.id, deviceId: current.id,
          toEmail: role === 'owner' ? email : null, stationName: row.name, deviceName: name, kind: 'browser' }, now)
        return json({ deviceId: current.id, approved: true })
      }
      return json({ deviceId: current.id, approved: current.approved === 1 })
    }
    if (approveAs) await rate(env, `enrol:${identity.accountId}`, now, ENROLMENTS_PER_DAY, DAY_MS)
    const credential = secret(), deviceId = uuid()
    const inserted = await env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,expires_at,public_key,approved,approved_at,role,added_by,proof)
      SELECT ?,?,?,?,?,?,?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM devices WHERE station_id=? AND expires_at>?)<8 RETURNING id`)
      .bind(deviceId, row.id, identity.accountId, name, await digest(credential), approveAs ? UNTIL_REMOVED : now + 600000, key,
        approveAs ? 1 : 0, approveAs ? now : null, approveAs, approveAs ? 'account' : null, approveAs ? input.proof : null, row.id, now).first()
    requireValue(inserted, 'deviceLimit', 409)
    if (approveAs) {
      await bumpPolicy(env, row.id, now)
      // The 2026-10-23 sign-in decision: notify the STATION OWNER a device was added by sign-in —
      // to the owner's own address, which we have only when the caller is the owner (a guest's
      // sign-in sends nothing, the device stays held); a no-op too without the provider secret.
      await deviceAddedNotice(env, { accountId: identity.accountId, stationId: row.id, deviceId,
        toEmail: role === 'owner' ? email : null, stationName: row.name, deviceName: name, kind: 'browser' }, now)
    }
    return json({ deviceId, approved: approveAs !== null }, 200, { 'set-cookie': cookie(row.id, credential) })
  }
  // What the station's use runs on: the owner's own entitlement, which a guest uses too
  // (`GUESTS_ON_OWNER_PLAN`). Its first internet use starts the trial (`TRIAL_STARTS_AT_BINDING`).
  let plan = role === 'owner' ? entitlement : await readEntitlement(env, row.account_id, now)
  const internet = async (starts: boolean) => {
    if (starts) plan = await firstUse(env, row, plan, now)
    requireTrial(plan, now)
    if (role !== 'owner' && !GUESTS_ON_OWNER_PLAN) requireTrial(entitlement, now)
  }
  // Free on the shack's own network (authority.ts lanEntitlement): an account whose internet use has
  // ended may still take a LAN-only ticket for a browser already approved, and renew it. The relay
  // route and every other verb still need internet use.
  if (!(plan.access === 'lan' && (verb === 'ticket' || verb === 'renew'))) await internet(verb === 'turn')
  if (verb === 'ticket' || verb === 'renew' || verb === 'turn') {
    const input = await body(request, verb === 'renew' ? ['sessionId'] : [], verb === 'turn' ? [] : ['scope'])
    // `scope: 'lan'` asks for a LAN-only session: its stream is held to the shack's own network, and an
    // account whose internet use has ended may have one. Without it, the session is an internet one,
    // exactly as before. A renewal's scope only gets it past the gate here: the room keeps the scope
    // the session was admitted with.
    requireValue(!('scope' in input) || input.scope === 'lan', 'invalidRequest', 400)
    const lan = input.scope === 'lan'
    if (!lan) await internet(verb === 'ticket')
    const current = await device(request, env, row.id, identity.accountId, now)
    requireValue(current?.approved === 1, 'deviceNotApproved')
    const browser = { ...identity, accountId: row.account_id, deviceId: current.id, deviceGeneration: current.generation,
      expiresAt: Math.min(identity.expiresAt, current.expires_at) }
    const policy = await access(env, row, now)
    observerDeadline(policy, browser, lan ? lanEntitlement(plan, now) : plan, now)
    // The stream's relay, for exactly the browser a ticket is for, past exactly the same checks.
    // It renews no approval: a page asks for a relay as a stream starts, and that is not use. The
    // credential lasts the longest session, and never past the entitlement that a stream needs.
    if (verb === 'turn') {
      await rate(env, `turn:${row.id}:${current.id}`, now, 12)
      return json({ iceServers: await relayServers(env, Math.min(STATION_ADMISSION_MS, plan.expiresAt - now)) })
    }
    if (verb === 'renew') {
      const response = await room(env, row.id, 'renew', { access: policy,
        sessionId: id(input.sessionId), identity: browser, entitlement: plan })
      requireValue(response.ok, 'sessionNotApproved')
      // Use, once the live session this browser holds has been confirmed.
      return json({ ok: true, serverNow: now }, 200, await used(request, env, row.id, current, now))
    }
    await rate(env, `ticket:${row.id}:${current.id}`, now, 12)
    const ticket = secret()
    await env.DB.batch([
      env.DB.prepare('DELETE FROM tickets WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO tickets(digest,station_id,account_id,device_id,device_generation,identity_until,expires_at,scope) VALUES(?,?,?,?,?,?,?,?)')
        .bind(await digest(ticket), row.id, row.account_id, current.id, current.generation, browser.expiresAt, now + 15000, lan ? 'lan' : 'internet'),
    ])
    return json({ ticket, serverNow: now }, 200, await used(request, env, row.id, current, now))
  }
  throw new Refusal('notFound', 404)
}

async function revokeStation(env: RemoteEnv, stationId: string, now: number): Promise<void> {
  await env.DB.batch([
    env.DB.prepare('UPDATE stations SET enabled=0,generation=generation+1,policy_version=policy_version+1 WHERE id=?').bind(stationId),
    env.DB.prepare('DELETE FROM tickets WHERE station_id=?').bind(stationId),
  ])
  await sync(env, await station(env, stationId), now)
}

/** What the service holds about an account, for the account to keep: its trial, its plan and the checkouts
 *  and refused payment events of it, whether trial markers were spent by it and when, its stations, every
 *  device of its stations and its own devices elsewhere, and the invitations it made and holds. Names,
 *  labels, roles and dates only: never a credential, proof, key, address fingerprint or marker value. */
async function accountCopy(env: RemoteEnv, accountId: string, now: number): Promise<unknown> {
  const all = async (sql: string, ...values: unknown[]) => (await env.DB.prepare(sql).bind(...values).all()).results
  const trial = await env.DB.prepare(`SELECT enabled=1 AS enabled, started_at AS startedAt, expires_at AS expiresAt, source
    FROM trials WHERE account_id=?`).bind(accountId).first<{ enabled: number }>()
  const subscription = await env.DB.prepare(`SELECT status, plan, period_end AS periodEnd, cancel_at_period_end=1 AS cancelAtPeriodEnd,
    first_refund_at AS firstRefundAt, customer_id AS customerId, subscription_id AS subscriptionId FROM subscriptions WHERE account_id=?`)
    .bind(accountId).first<{ cancelAtPeriodEnd: number }>()
  const stations = await all('SELECT id, name, enabled=1 AS enabled FROM stations WHERE account_id=? ORDER BY id', accountId)
  const devices = await all(`SELECT d.id, d.station_id AS stationId, d.name, COALESCE(d.kind,'browser') AS kind, d.platform,
    COALESCE(d.role,'operate') AS role, COALESCE(d.added_by,'shack') AS addedBy, d.approved=1 AS approved, d.approved_at AS approvedAt,
    d.expires_at AS expiresAt, d.revoked_at AS revokedAt, d.account_id=? AS yours
    FROM devices d JOIN stations s ON s.id=d.station_id WHERE d.account_id=? OR s.account_id=? ORDER BY d.station_id, d.id`, accountId, accountId, accountId)
  const truth = (row: Record<string, unknown>, ...keys: string[]) => ({ ...row, ...Object.fromEntries(keys.map(key => [key, row[key] === 1])) })
  return {
    exportedAt: now, accountId,
    trial: trial && truth(trial, 'enabled'),
    subscription: subscription && truth(subscription, 'cancelAtPeriodEnd'),
    checkouts: await all('SELECT id, created_at AS createdAt FROM checkout_sessions WHERE account_id=? ORDER BY created_at', accountId),
    refusedPaymentEvents: await all('SELECT event_id AS eventId, type, reason, at FROM billing_refusals WHERE account_id=? ORDER BY at', accountId),
    markers: (await all(`SELECT kind, proven=1 AS proven, method, created_at AS createdAt, first_refund_at AS firstRefundAt
      FROM trial_markers WHERE account_id=? ORDER BY kind, created_at`, accountId)).map(row => truth(row, 'proven')),
    stations: stations.map(row => truth(row, 'enabled')),
    devices: devices.map(row => truth(row, 'approved', 'yours')),
    invitationsMade: await all(`SELECT i.id, i.station_id AS stationId, i.role, i.created_at AS createdAt, i.removed_at AS removedAt
      FROM invites i JOIN stations s ON s.id=i.station_id WHERE s.account_id=? ORDER BY i.created_at`, accountId),
    invitationsReceived: await all(`SELECT i.station_id AS stationId, i.role, i.created_at AS createdAt, i.removed_at AS removedAt
      FROM invites i JOIN accounts a ON a.mailbox_hash=i.mailbox_hash JOIN stations s ON s.id=i.station_id
      WHERE a.id=? AND s.account_id<>a.id ORDER BY i.created_at`, accountId),
  }
}

/** Delete an account. Returns every station this account held a device or station at, whose browser
 *  credential the caller may clear.
 *
 *  ⚠️ THE ORDER IS THE PROPERTY: nothing that can still charge may outlive the account. So the live
 *  subscription is ended FIRST, through the billing Worker, and nothing is touched unless it answers that
 *  the payment provider shows nothing left to charge (`{ cancelled }`). Any other answer, a throw, or, with
 *  billing off, a subscription that has not ended (nobody here can end it) refuses `subscriptionNotCancelled`
 *  with the account exactly as it was. Never move the cancel below the first write.
 *
 *  Then every session is cut while the rows that name it still exist: each station the account owns is
 *  revoked and its room told, and its own devices at other people's stations are removed and those rooms
 *  told. Then, in one batch, its stations go with everything of them (devices, anyone's tickets there, the
 *  invitations made there, notices), and its own devices, tickets, challenges, pairing requests and notices.
 *  A person invited to its stations loses that access; the invitations other people made to it are theirs
 *  and stay, though no sign-in reaches this account again.
 *
 *  What is kept is what stops a second free trial, and the billing record: an account that ever held a trial,
 *  a plan, a checkout or a trial marker keeps its row, with its two address fingerprints, under a subject no
 *  sign-in can present (`deleted:<id>`) and with no notice address; its trial row, ended now; its trial
 *  markers; and its subscription, checkouts and refused events. Signing in again makes a new, empty account,
 *  which the same address cannot start a second trial on. An account that held none of those goes entirely.
 *  Deleting again finds nothing of the old account and changes nothing kept. */
async function deleteAccount(env: RemoteEnv, accountId: string, now: number): Promise<string[]> {
  const subscription = await env.DB.prepare('SELECT status FROM subscriptions WHERE account_id=?').bind(accountId).first<{ status: string }>()
  if (env.BILLING) {
    const answer = await env.BILLING.cancel(accountId).catch(() => null) as { cancelled?: unknown } | null
    requireValue(typeof answer?.cancelled === 'boolean', 'subscriptionNotCancelled', 503)
  } else {
    requireValue(!subscription || SUBSCRIPTION_ENDED.includes(subscription.status), 'subscriptionNotCancelled', 503)
  }
  const owned = (await env.DB.prepare('SELECT id FROM stations WHERE account_id=?').bind(accountId).all<{ id: string }>()).results.map(row => row.id)
  for (const stationId of owned) await revokeStation(env, stationId, now)
  const elsewhere = await env.DB.prepare(`UPDATE devices SET approved=0,generation=generation+1,expires_at=?,proof=NULL
    WHERE account_id=? AND station_id NOT IN (SELECT id FROM stations WHERE account_id=?) RETURNING station_id`)
    .bind(now, accountId, accountId).all<{ station_id: string }>()
  const guestAt = [...new Set(elsewhere.results.map(row => row.station_id))]
  for (const stationId of guestAt) await bumpPolicy(env, stationId, now)
  const mine = 'SELECT id FROM stations WHERE account_id=?'
  await env.DB.batch([
    env.DB.prepare(`DELETE FROM tickets WHERE account_id=? OR station_id IN (${mine}) OR device_id IN (SELECT id FROM devices WHERE account_id=?)`)
      .bind(accountId, accountId, accountId),
    env.DB.prepare('DELETE FROM challenges WHERE account_id=?').bind(accountId),
    env.DB.prepare(`DELETE FROM notices WHERE account_id=? OR station_id IN (${mine})`).bind(accountId, accountId),
    env.DB.prepare(`DELETE FROM invites WHERE station_id IN (${mine})`).bind(accountId),
    env.DB.prepare(`DELETE FROM devices WHERE account_id=? OR station_id IN (${mine})`).bind(accountId, accountId),
    env.DB.prepare('DELETE FROM enrollments WHERE account_id=?').bind(accountId),
    env.DB.prepare('DELETE FROM stations WHERE account_id=?').bind(accountId),
    env.DB.prepare('UPDATE trials SET expires_at=MIN(expires_at, ?) WHERE account_id=?').bind(now, accountId),
    env.DB.prepare(`DELETE FROM accounts WHERE id=? AND NOT EXISTS(SELECT 1 FROM trials WHERE account_id=?)
      AND NOT EXISTS(SELECT 1 FROM subscriptions WHERE account_id=?) AND NOT EXISTS(SELECT 1 FROM checkout_sessions WHERE account_id=?)
      AND NOT EXISTS(SELECT 1 FROM trial_markers WHERE account_id=?)`).bind(accountId, accountId, accountId, accountId, accountId),
    env.DB.prepare("UPDATE accounts SET subject='deleted:' || id, notice_address=NULL WHERE id=?").bind(accountId),
  ])
  return [...owned, ...guestAt]
}

/** A station's devices changed: its policy version moves, and its room hears of it. */
async function bumpPolicy(env: RemoteEnv, stationId: string, now: number): Promise<void> {
  await env.DB.prepare('UPDATE stations SET policy_version=policy_version+1 WHERE id=?').bind(stationId).run()
  await sync(env, await station(env, stationId), now)
}

/** The role a device is approved with by its sign-in's proof at this station, or null when it is left to
 *  wait for the station. A proof that fails is refused (`proofInvalid`), never downgraded to a wait: a
 *  device whose proof does not check has a bug the operator should see.
 *
 *  The owner's own device is Operate. A guest's takes its invite's role, matched on the proof's own
 *  verified address, as the station matches it, so the service approves no guest the station would not.
 *  None is approved at a station with no station proof (`AUTO_APPROVE_NEEDS_STATION_PROOF`): the station
 *  could not check the device's proof against an owner it does not know. */
async function vouch(env: RemoteEnv, row: StationRow, role: Role, value: unknown, clientId: string, subject: string,
  nonce: string, now: number): Promise<'operate' | 'listen' | null> {
  const { email } = await verifyProof(env, value, clientId, subject, nonce, now)
  const proven = await env.DB.prepare('SELECT proof FROM stations WHERE id=?').bind(row.id).first<{ proof: string | null }>()
  if (AUTO_APPROVE_NEEDS_STATION_PROOF && !proven?.proof) return null
  if (role === 'owner') return 'operate'
  if (email === null) return null
  const invite = await env.DB.prepare('SELECT role FROM invites WHERE station_id=? AND mailbox_hash=? AND removed_at IS NULL')
    .bind(row.id, await digest(`email:${mailbox(email)}`)).first<{ role: 'operate' | 'listen' }>()
  return invite?.role ?? null
}

/** A waiting device approved by its sign-in: until removed, a new generation, with its role and proof. */
async function approveByAccount(env: RemoteEnv, stationId: string, deviceId: string, role: 'operate' | 'listen', proof: string,
  lanKey: string | null, accountId: string, now: number): Promise<void> {
  await rate(env, `enrol:${accountId}`, now, ENROLMENTS_PER_DAY, DAY_MS)
  await env.DB.prepare(`UPDATE devices SET approved=1,generation=generation+1,expires_at=?,approved_at=?,role=?,added_by='account',proof=?,
    lan_key=COALESCE(?,lan_key),revoked_at=NULL,revoke_seen_at=NULL WHERE id=? AND station_id=?`)
    .bind(UNTIL_REMOVED, now, role, proof, lanKey, deviceId, stationId).run()
  await bumpPolicy(env, stationId, now)
}

/** A person's invite to a station removed, with every device that person has there, in one batch: their
 *  approvals end now and the room cuts their sessions. Never the owner's own devices. `revokedAt` is the
 *  time when it was done on the web (so the web can say when the station has heard), null at the station. */
async function removeInvite(env: RemoteEnv, row: StationRow, hash: string, now: number, revokedAt: number | null): Promise<void> {
  await env.DB.batch([
    env.DB.prepare('UPDATE invites SET removed_at=? WHERE station_id=? AND mailbox_hash=? AND removed_at IS NULL').bind(now, row.id, hash),
    env.DB.prepare(`UPDATE devices SET approved=0,generation=generation+1,expires_at=?,proof=NULL,revoked_at=?,revoke_seen_at=NULL
      WHERE station_id=? AND expires_at>? AND account_id<>? AND account_id IN (SELECT id FROM accounts WHERE mailbox_hash=?)`)
      .bind(now, revokedAt, row.id, now, row.account_id, hash),
  ])
  await bumpPolicy(env, row.id, now)
}

/** The trial of a station's account, started now if it has never had one: at the station's first
 *  internet use, by default (`TRIAL_STARTS_AT_BINDING`). The same rules as starting one at the approval
 *  of a pairing (`enroll/approve`): eligible, no trial spent by the same person under another sign-in,
 *  and none spent with the callsign or PC the shack sent when it signed in, all decided again inside the
 *  INSERT for the race. Any other state is answered unchanged. */
async function firstUse(env: RemoteEnv, row: StationRow, plan: AccountEntitlement, now: number): Promise<AccountEntitlement> {
  if (plan.state !== 'none') return plan
  await requireMayPair(env, plan, now)
  const kept = await env.DB.prepare('SELECT trial_claim FROM stations WHERE id=?').bind(row.id).first<{ trial_claim: string | null }>()
  const claim = kept?.trial_claim ? await trialProof(env, JSON.parse(kept.trial_claim) as TrialInput, row.account_id, now) : null
  if (claim) await requireUnspentMarkers(env, row.account_id, claim)
  // The sibling test is the one `enroll/approve` writes in its own trial INSERT, word for word.
  await env.DB.batch([
    env.DB.prepare(`INSERT INTO trials(account_id, enabled, expires_at, started_at, source)
      SELECT ?,1,?,?,'trial' WHERE NOT EXISTS(SELECT 1 FROM trials t JOIN accounts a ON a.id=t.account_id, accounts self
        WHERE self.id=? AND t.account_id<>self.id
        AND (a.mailbox_hash=self.mailbox_hash OR a.email_hash IN (self.email_hash, self.mailbox_hash)))
      ${claim ? `AND NOT EXISTS(SELECT 1 FROM trial_markers m WHERE ${SPENT_MARKER})` : ''}
      ON CONFLICT(account_id) DO NOTHING`)
      .bind(row.account_id, now + TRIAL_MS, now, row.account_id, ...(claim ? spentBindings(row.account_id, claim) : [])),
    ...(claim ? markerInserts(env, row.account_id, claim, now) : []),
  ])
  const started = await readEntitlement(env, row.account_id, now)
  if (started.state !== 'active') {
    await requireUnspentIdentity(env, row.account_id, now)
    if (claim) await requireUnspentMarkers(env, row.account_id, claim)
  }
  requireValue(started.state === 'active', 'serviceUnavailable', 503)
  return started
}

// THE NATIVE CLIENT. Nexus on another computer (later a phone app) driving its station over the
// internet: signed in through the system browser to the Native application (authority.ts,
// `nativeAccount`), with no Origin and no cookie, and acting as a device of that station, approved at
// the shack as a browser is and on the same terms: until removed, eight devices a station, removed at
// the shack or by itself. Its credential is its key: every request that
// acts as the device is signed by it over a one-use challenge (`signedBy`), and the shack pins that
// same key when it approves the device, for the pipe's own TLS to prove.
//
//   POST client/challenge              a challenge for the next signed request, good for a minute
//   POST client/session                the account's stations and those it is invited to, each with its
//                                      station key, LAN key, station proof and the account's role there
//   POST client/station                the shack signing in: its station bound to the account, by proof
//   POST stations/:id/client/station-proof  a station paired by code confirming its account, by proof
//   POST stations/:id/client/device    this device's key registered (signed by that key), or found again;
//                                      approved at once when its sign-in's proof says it is the owner's
//                                      or an invited person's
//   POST stations/:id/client/challenge a challenge for an approved device reopening, with no token
//   POST stations/:id/client/turn      TURN credentials for its internet roads
//   GET  stations/:id/client/pipe      its pipe to the station, through the room (pipe.ts)
//   POST stations/:id/client/renew     the pipe's lease renewed, as a browser renews its session
//   POST stations/:id/client/forget    its own approval ended, as a browser forgets itself
//
// REOPENING. A request with no account token is an approved device opening its station again
// (`reopener`): the device it names, approved at the shack and not removed, is its caller, under its
// station's account, and each of its verbs is signed by its key as any other. So Nexus keeps no
// account credential on the computer, and signs in only to register a device. Registering, the
// account's stations and a challenge without a station all still need the token.
//
// Until its signature is checked a reopening request is anyone's who knows a device id, so it is held
// in buckets of its own (`reopener`) and charged to its account's only once its key has signed it
// (`signer`): charged first, a flood of unsigned ones spent the account's whole minute and locked out
// its signed-in computer and every device of it. Its challenges are its device's alone, four at most from each requester.
async function client(request: Request, env: RemoteEnv, path: string, match: RegExpExecArray | null, now: number): Promise<Response> {
  const reopening = !request.headers.has('authorization')
  const caller = reopening ? await reopener(request, env, match, now) : await nativeAccount(request, env, now)
  if (!reopening) await rate(env, `client:${caller.accountId}`, now, 120)
  const entitlement = await readEntitlement(env, caller.accountId, now)
  if (path === 'client/challenge') {
    requireValue(request.method === 'POST', 'methodNotAllowed', 405)
    await body(request, [])
    const challenge = secret(), expiresAt = now + CHALLENGE_MS
    await env.DB.batch([
      env.DB.prepare('DELETE FROM challenges WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO challenges(digest,account_id,expires_at) VALUES(?,?,?)').bind(await digest(challenge), caller.accountId, expiresAt),
    ])
    return json({ challenge, expiresAt })
  }
  if (path === 'client/session') {
    requireValue(request.method === 'POST', 'methodNotAllowed', 405)
    await body(request, [])
    // The station key (S3-M1) over the signed-in API, never through the relay: the key the station's
    // end of the pipe must prove, which the computer keeps at first sight. With it the station's LAN key
    // and the station proof that vouches for both, which the computer checks with the provider's keys
    // before it trusts the LAN key; then the stations this account is invited to, with its role there.
    type Listed = { id: string; name: string; public_key: string | null; lan_key: string | null; proof: string | null; role: Role }
    const owned = await env.DB.prepare(`SELECT id,name,public_key,lan_key,proof,'owner' AS role FROM stations
      WHERE account_id=? AND enabled=1 ORDER BY id LIMIT 2`).bind(caller.accountId).all<Listed>()
    const invited = await env.DB.prepare(`SELECT s.id,s.name,s.public_key,s.lan_key,s.proof,i.role FROM invites i
      JOIN stations s ON s.id=i.station_id JOIN accounts a ON a.mailbox_hash=i.mailbox_hash
      WHERE a.id=? AND i.removed_at IS NULL AND s.enabled=1 AND s.account_id<>a.id ORDER BY s.id LIMIT 16`).bind(caller.accountId).all<Listed>()
    return json({ accountId: caller.accountId, entitlement, identityVerified: caller.verified, serverNow: now,
      stations: [...owned.results, ...invited.results].map(row => ({ id: row.id, name: row.name, stationKey: row.public_key,
        lanKey: row.lan_key, proof: row.proof, role: row.role })) })
  }
  // THE SHACK SIGNING IN. Its keys made first, their hash its sign-in's nonce (`stationNonce`), so the
  // provider-signed token names exactly the keys this station will hold: the account binds the station
  // with nothing typed and no code. The station is written for the token's account (two at most), with
  // its credential's digest, its signing key (as `native/key` keeps it), its LAN key and the proof. A
  // retry with the same credential finds the same station. Starts nothing on the trial clock unless
  // `TRIAL_STARTS_AT_BINDING`; the callsign and PC it sends are kept as HMACs until the trial starts.
  if (path === 'client/station') {
    requireValue(request.method === 'POST' && !reopening, 'methodNotAllowed', 405)
    await rate(env, `bind:${caller.accountId}`, now, BINDINGS_PER_DAY, DAY_MS)
    const input = await body(request, ['name', 'credential', 'stationKey', 'lanKey', 'proof'], ['callsign', 'pc'])
    const name = label(input.name), credentialHash = await digest(proof(input.credential))
    const stationKey = await publicKey(input.stationKey), lanKey = await publicKey(input.lanKey)
    // An operator's ban outranks binding; an ended trial does not, because the LAN is free.
    requireValue(entitlement.state !== 'disabled', 'trialDisabled')
    await verifyProof(env, input.proof, nativeClientId(env)!, caller.subject, await stationNonce(stationKey, lanKey), now)
    const kept = await trialInput(env, input)
    const found = await env.DB.prepare('SELECT id FROM stations WHERE account_id=? AND credential_hash=? AND enabled=1')
      .bind(caller.accountId, credentialHash).first<{ id: string }>()
    const stationId = found?.id ?? uuid()
    if (!found) {
      const inserted = await env.DB.prepare(`INSERT INTO stations(id,account_id,name,credential_hash,public_key,lan_key,proof,trial_claim)
        SELECT ?,?,?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM stations WHERE account_id=? AND enabled=1)<2 RETURNING id`)
        .bind(stationId, caller.accountId, name, credentialHash, stationKey, lanKey, input.proof, kept && JSON.stringify(kept), caller.accountId).first()
      requireValue(inserted, 'stationLimit', 409)
    }
    const bound = TRIAL_STARTS_AT_BINDING ? await firstUse(env, await station(env, stationId), entitlement, now) : entitlement
    return json({ stationId, accountId: caller.accountId, entitlement: bound })
  }
  requireValue(match, 'notFound', 404)
  const row = await station(env, match[1]), verb = match[2].slice('client/'.length)
  // The station's owner, or a person invited to it. A device is its own account's; what it does at the
  // station runs under the station's account (`plan`, `identity` below), as the owner's devices do.
  const role = row.enabled === 1 ? await memberRole(env, row, caller.accountId) : null
  requireValue(role, 'stationUnavailable', 404)
  let plan = role === 'owner' ? entitlement : await readEntitlement(env, row.account_id, now)
  const internet = async (starts: boolean) => {
    if (starts) plan = await firstUse(env, row, plan, now)
    requireTrial(plan, now)
    if (role !== 'owner' && !GUESTS_ON_OWNER_PLAN) requireTrial(entitlement, now)
  }
  // A station paired by code confirming its account: the shack signs in as it does to bind (§ client/
  // station) and proves it holds the station's credential. Signing in again replaces the proof, which is
  // how a station whose proof the provider can no longer verify (its key retired) is proven again.
  if (verb === 'station-proof') {
    requireValue(request.method === 'POST' && !reopening && role === 'owner', 'stationUnavailable', 404)
    await rate(env, `bind:${caller.accountId}`, now, BINDINGS_PER_DAY, DAY_MS)
    const input = await body(request, ['credential', 'stationKey', 'lanKey', 'proof'])
    const credentialHash = await digest(proof(input.credential))
    const stationKey = await publicKey(input.stationKey), lanKey = await publicKey(input.lanKey)
    const held = await env.DB.prepare('SELECT public_key FROM stations WHERE id=? AND credential_hash=?')
      .bind(row.id, credentialHash).first<{ public_key: string | null }>()
    requireValue(held, 'stationNotApproved', 401)
    // The signing key changes only by binding again, which makes a new station (as `native/key`).
    requireValue(held.public_key === null || held.public_key === stationKey, 'stationKeyPinned', 409)
    await verifyProof(env, input.proof, nativeClientId(env)!, caller.subject, await stationNonce(stationKey, lanKey), now)
    await env.DB.prepare(`UPDATE stations SET public_key=?,lan_key=?,proof=? WHERE id=? AND credential_hash=?
      AND (public_key IS NULL OR public_key=?)`).bind(stationKey, lanKey, input.proof, row.id, credentialHash, stationKey).run()
    return json({ ok: true })
  }
  if (verb === 'challenge') {
    // Only for reopening: a signed-in computer asks `client/challenge`.
    requireValue(reopening && request.method === 'POST', 'notFound', 404)
    await body(request, [])
    const challenge = secret(), expiresAt = now + CHALLENGE_MS, deviceId = id(request.headers.get('x-nexus-device-id'))
    const requester = callerKey(request.headers.get('cf-connecting-ip')).caller
    // The newest four a device are kept for each requester (an IPv4 address or IPv6 /64, as the
    // reopen buckets key it) and any older one of that requester's given up. So asking is never
    // refused, and asking in this device's name from elsewhere never gives up the device's own.
    await env.DB.batch([
      env.DB.prepare('DELETE FROM challenges WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO challenges(digest,account_id,device_id,requester,expires_at) VALUES(?,?,?,?,?)')
        .bind(await digest(challenge), caller.accountId, deviceId, requester, expiresAt),
      env.DB.prepare(`DELETE FROM challenges WHERE device_id=? AND requester=? AND rowid NOT IN
        (SELECT rowid FROM challenges WHERE device_id=? AND requester=? ORDER BY rowid DESC LIMIT 4)`)
        .bind(deviceId, requester, deviceId, requester),
    ])
    return json({ challenge, expiresAt })
  }
  // Every verb but `device` is this device's own: the device the request names, of this account and
  // station, within its approval or its wait for one, and the request signed by its key.
  const signer = async (bytes: Uint8Array): Promise<DeviceRow> => {
    const current = await nativeDevice(request, env, row.id, caller.accountId, now)
    requireValue(current?.public_key, 'deviceNotApproved')
    await signedBy(request, env, caller.accountId, reopening ? current.id : null, current.public_key, bytes, now)
    if (reopening) await rate(env, `client:${caller.accountId}`, now, 120)
    return current
  }
  const identity = (current: DeviceRow) => ({ accountId: row.account_id, deviceId: current.id,
    deviceGeneration: current.generation, expiresAt: Math.min(caller.expiresAt, current.expires_at) })
  if (verb === 'pipe') {
    requireValue(request.method === 'GET' && request.headers.get('upgrade')?.toLowerCase() === 'websocket', 'webSocketRequired', 426)
    await internet(true)
    const current = await signer(new Uint8Array())
    requireValue(current.approved === 1, 'deviceNotApproved')
    await rate(env, `pipe:${row.id}:${current.id}`, now, 12)
    const response = await room(env, row.id, 'client-pipe', { access: await access(env, row, now), identity: identity(current), entitlement: plan })
    // Opening the station from this computer is use, as a browser's ticket is.
    if (response.status === 101) await keepApprovals(env, row.id, now, current.id)
    return response
  }
  requireValue(request.method === 'POST', 'methodNotAllowed', 405)
  const bytes = await bodyBytes(request)
  // Above the trial, as a browser's own is: an operator whose access lapsed may still tidy up.
  if (verb === 'forget') {
    fieldsOf(bytes, [])
    const current = await signer(bytes)
    await env.DB.batch([
      env.DB.prepare('UPDATE devices SET approved=0,generation=generation+1,expires_at=? WHERE id=?').bind(now, current.id),
      env.DB.prepare('UPDATE stations SET policy_version=policy_version+1 WHERE id=?').bind(row.id),
    ])
    await sync(env, await station(env, row.id), now)
    return json({ ok: true })
  }
  // Above the entitlement: enrolling is free, as the LAN is.
  if (verb === 'device') {
    // A new device is the account's to add: signed in, never reopening.
    requireValue(!reopening, 'signInRequired', 401)
    const input = fieldsOf(bytes, ['name', 'kind', 'platform', 'publicKey'], ['lanKey', 'proof'])
    const name = label(input.name), key = await publicKey(input.publicKey), { kind, platform } = nativeKind(input.kind, input.platform)
    // A computer's LAN key, beside its road key: its proof covers both.
    const lanKey = 'lanKey' in input ? await publicKey(input.lanKey) : null
    // Signed by the key it registers, so whoever sends it holds that key.
    await signedBy(request, env, caller.accountId, null, key, bytes, now)
    const approveAs = 'proof' in input ? await vouch(env, row, role, input.proof, nativeClientId(env)!, caller.subject,
      await deviceNonce(kind, lanKey === null ? [key] : [key, lanKey]), now) : null
    const current = await env.DB.prepare(`SELECT id,name,generation,approved,approved_at,expires_at,public_key FROM devices
      WHERE station_id=? AND account_id=? AND public_key=? AND kind IS NOT NULL AND expires_at>? ORDER BY expires_at DESC LIMIT 1`)
      .bind(row.id, caller.accountId, key, now).first<DeviceRow>()
    // One still waiting is approved in place by a sign-in for its keys.
    if (current && approveAs && current.approved !== 1) {
      await approveByAccount(env, row.id, current.id, approveAs, input.proof as string, lanKey, caller.accountId, now)
      // The 2026-10-23 sign-in decision: notify the STATION OWNER a device was added by sign-in —
      // to the owner's own address, which we have only when the caller is the owner (a guest's
      // sign-in sends nothing, the device stays held); a no-op too without the provider secret.
      await deviceAddedNotice(env, { accountId: caller.accountId, stationId: row.id, deviceId: current.id,
        toEmail: role === 'owner' ? caller.email : null, stationName: row.name, deviceName: name, kind }, now)
      return json({ deviceId: current.id, approved: true, expiresAt: UNTIL_REMOVED, renewsUntil: null })
    }
    if (current) return json({ deviceId: current.id, approved: current.approved === 1, expiresAt: current.expires_at, renewsUntil: null })
    // Approved by its sign-in, until removed; or ten minutes to be approved at the shack, as a browser has.
    // The credential digest is of a value nobody holds: this device's credential is its key, and no
    // cookie can ever match its row.
    if (approveAs) await rate(env, `enrol:${caller.accountId}`, now, ENROLMENTS_PER_DAY, DAY_MS)
    const deviceId = uuid(), expiresAt = approveAs ? UNTIL_REMOVED : now + 600000
    const inserted = await env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,expires_at,public_key,kind,platform,
      lan_key,approved,approved_at,role,added_by,proof)
      SELECT ?,?,?,?,?,?,?,?,?,?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM devices WHERE station_id=? AND expires_at>?)<8 RETURNING id`)
      .bind(deviceId, row.id, caller.accountId, name, await digest(secret()), expiresAt, key, kind, platform, lanKey,
        approveAs ? 1 : 0, approveAs ? now : null, approveAs, approveAs ? 'account' : null, approveAs ? input.proof : null, row.id, now).first()
    requireValue(inserted, 'deviceLimit', 409)
    if (approveAs) {
      await bumpPolicy(env, row.id, now)
      // The 2026-10-23 sign-in decision: notify the STATION OWNER a device was added by sign-in —
      // to the owner's own address, which we have only when the caller is the owner (a guest's
      // sign-in sends nothing, the device stays held); a no-op too without the provider secret.
      await deviceAddedNotice(env, { accountId: caller.accountId, stationId: row.id, deviceId,
        toEmail: role === 'owner' ? caller.email : null, stationName: row.name, deviceName: name, kind }, now)
    }
    return json({ deviceId, approved: approveAs !== null, expiresAt, renewsUntil: null })
  }
  await internet(verb === 'turn')
  fieldsOf(bytes, [])
  const current = await signer(bytes)
  requireValue(current.approved === 1, 'deviceNotApproved')
  const policy = await access(env, row, now)
  // Past exactly the checks a browser's ticket is.
  observerDeadline(policy, identity(current), plan, now)
  if (verb === 'turn') {
    // The relay a browser's stream has, for this device's two internet roads. Each entry is a username
    // and a password, which is what the client core's TURN client takes.
    await rate(env, `turn:${row.id}:${current.id}`, now, 12)
    return json({ iceServers: await relayServers(env, Math.min(STATION_ADMISSION_MS, plan.expiresAt - now)) })
  }
  if (verb === 'renew') {
    const response = await room(env, row.id, 'pipe-renew', { access: policy, identity: identity(current), entitlement: plan })
    requireValue(response.ok, 'pipeClosed', 404)
    await keepApprovals(env, row.id, now, current.id)
    return json({ ok: true, serverNow: now })
  }
  throw new Refusal('notFound', 404)
}

// The caller of a request with no account token: the native device it names (`x-nexus-device-id`), of
// the station the path names, approved there and not removed, acting as its own account (the owner's, or
// a guest's, whose invite `client` checks as for a signed-in caller) for at most an hour, as a token would. What it may do is still each verb's own, signed by the device's
// key. No Origin and no cookie, as for any native caller. Refused alike, and before anything is read,
// for a path with no station or a device that is not one approved there: no token, no reopening.
//
// Its buckets, per caller FIRST as `enroll`'s are, so a caller over its own limit never spends the next:
// thirty a minute an IPv4 address or IPv6 /64, sixty an IPv6 /48, then a hundred and twenty for the
// device. One network can so spend at most half of a device's minute; taking all of it needs two /48s
// or four IPv4 addresses at once, which is a distributed attacker and a job for an edge rule.
async function reopener(request: Request, env: RemoteEnv, match: RegExpExecArray | null, now: number): Promise<Identity & { subject: string; verified: boolean; email: string | null }> {
  requireValue(!request.headers.has('origin') && !request.headers.has('cookie'), 'originDenied')
  requireValue(nativeClientId(env), 'serviceNotConfigured', 503)
  requireValue(match, 'signInRequired', 401)
  const deviceId = request.headers.get('x-nexus-device-id') ?? ''
  requireValue(/^[0-9a-f-]{36}$/.test(deviceId), 'signInRequired', 401)
  const { caller: from, network } = caller(request.headers.get('cf-connecting-ip'))
  await rate(env, `reopen-caller:${from}`, now, 30)
  if (network) await rate(env, `reopen-network:${network}`, now, 60)
  await rate(env, `reopen:${match[1]}:${deviceId}`, now, 120)
  const row = await station(env, match[1]).catch(() => null)
  requireValue(row, 'signInRequired', 401)
  const current = await env.DB.prepare(`SELECT account_id,approved,expires_at,public_key FROM devices
    WHERE id=? AND station_id=? AND kind IS NOT NULL AND expires_at>?`).bind(id(deviceId), row.id, now)
    .first<{ account_id: string; approved: number; expires_at: number; public_key: string | null }>()
  requireValue(current?.public_key && current.approved === 1, 'signInRequired', 401)
  // A reopening request carries no fresh sign-in, so no verified address: the owner notice never
  // fires on it (it only ever fires when a device is first added by a sign-in).
  return { accountId: current.account_id, expiresAt: Math.min(current.expires_at, now + 3600000), subject: '', verified: false, email: null }
}

// The Native application's client id, or null while the service has none: unset, or the page's own.
function nativeClientId(env: RemoteEnv): string | null {
  const id = env.AUTH0_NATIVE_CLIENT_ID
  return id && id !== 'unconfigured' && id !== env.AUTH0_CLIENT_ID ? id : null
}

// A browser's use of its approval (operator decision 2026-10-10: an approval lasts until removed).
// Reached only from `ticket` and `renew`, after the request has been admitted: the account's token, THIS
// browser's own credential, an approved device, a live trial, and for `renew` the live session. An
// approval given under the thirty/ninety-day rule becomes one until removed (`keepApprovals`), and the
// browser's credential is re-issued, unchanged, for as long as a browser keeps one, so a browser in use
// never loses it. It moves no generation and grants nothing. Returns the response headers.
async function used(request: Request, env: RemoteEnv, stationId: string, current: DeviceRow, now: number): Promise<HeadersInit | undefined> {
  await keepApprovals(env, stationId, now, current.id)
  const credential = deviceCookie(request, stationId)
  return credential ? { 'set-cookie': cookie(stationId, credential) } : undefined
}

// Every response sent over TLS tells the browser to keep to https:// here for a year, so after one
// visit an address typed or linked as http:// never leaves the browser in the clear. No
// includeSubDomains: it would reach only hosts under this one, and there are none. No preload: that
// binds the whole registrable domain. Never over plain HTTP, where a browser ignores it and the
// standard forbids it. Not on a WebSocket's 101 either: the page that opens a socket has already
// been told, and the relay's handshake stays exactly what it was.
function hsts(request: Request, response: Response): Response {
  if (!request.url.startsWith('https:') || response.status === 101) return response
  // A room's response has immutable headers; a copy of it does not.
  const result = new Response(response.body, response)
  result.headers.set('strict-transport-security', 'max-age=31536000')
  return result
}

// THE STREAM'S RELAY. Cloudflare Realtime TURN credentials, minted with the service's own key for
// one stream, lasting `ttlMs`. The page adds them beside its STUN server, and ICE prefers a direct path,
// so the relay carries a stream only where nothing direct works. The key's id and token are Worker
// secrets and go nowhere but the provider; the page is handed the TURN entries alone. Without the key
// this refuses `relayNotConfigured`; a provider that fails, is slow or answers anything else,
// `relayUnavailable`. Either way the page streams direct, as it did before the relay.
const RELAY_API = 'https://rtc.live.cloudflare.com/v1/turn/keys/'
/** The provider's answer within this, or no relay. Below the page's own wait for the route. */
const RELAY_DEADLINE_MS = 3000
const RELAY_URL = /^turns?:[0-9a-z.-]{1,253}:(\d{1,5})(\?transport=(udp|tcp))?$/i
async function relayServers(env: RemoteEnv, ttlMs: number): Promise<{ urls: string[]; username: string; credential: string }[]> {
  const keyId = env.TURN_KEY_ID ?? '', token = env.TURN_KEY_TOKEN ?? ''
  requireValue(/^[0-9A-Za-z_-]{1,128}$/.test(keyId) && token.length > 0, 'relayNotConfigured', 503)
  let answer: unknown
  try {
    const response = await fetch(`${RELAY_API}${keyId}/credentials/generate-ice-servers`, {
      method: 'POST', headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ ttl: Math.ceil(ttlMs / 1000) }), signal: AbortSignal.timeout(RELAY_DEADLINE_MS),
    })
    if (!response.ok) throw new Error('relayUnavailable')
    answer = await response.json()
  } catch { throw new Refusal('relayUnavailable', 503) }
  // The TURN entries and nothing else: the page has its own STUN server. A URL on port 53 is left out:
  // browsers may refuse TURN on the DNS port, and the provider offers the same transports on others.
  const servers = (answer as { iceServers?: unknown } | null)?.iceServers
  const relay = (Array.isArray(servers) ? servers.slice(0, 4) : []).flatMap(server => {
    const { urls, username, credential } = (server ?? {}) as Record<string, unknown>
    const turn = (Array.isArray(urls) ? urls : [urls]).filter((url): url is string => {
      const port = typeof url === 'string' ? RELAY_URL.exec(url)?.[1] : undefined
      return port !== undefined && Number(port) !== 53
    }).slice(0, 8)
    return turn.length > 0 && typeof username === 'string' && typeof credential === 'string' && username.length > 0
      && credential.length > 0 && username.length <= 1024 && credential.length <= 1024 ? [{ urls: turn, username, credential }] : []
  })
  requireValue(relay.length > 0, 'relayUnavailable', 503)
  return relay
}

export default {
  async fetch(request: Request, env: RemoteEnv): Promise<Response> {
    try {
      const url = new URL(request.url)
      // Plain HTTP reaches no route, no database and no sign-in. A read is sent to the same path and
      // query on the service's https:// origin. Anything else is refused as the API refuses a wrong
      // origin: its body and credentials have already crossed in the clear, and a 308 would resend
      // them as if all were well. A local http:// service (wrangler dev, the tests) has nowhere to send it.
      if (url.protocol === 'http:' && env.PUBLIC_REMOTE_ORIGIN.startsWith('https:')) {
        requireValue(request.method === 'GET' || request.method === 'HEAD', 'originDenied')
        return Response.redirect(`${env.PUBLIC_REMOTE_ORIGIN}${url.pathname}${url.search}`, 301)
      }
      if (url.pathname.startsWith('/api/')) return hsts(request, await api(request, env))
      // HTML normalization is disabled so /index.html cannot redirect around
      // these response headers. Resolve the home page explicitly, retaining
      // the browser's original callback URL for the SDK's PKCE completion.
      if (url.pathname === '/') url.pathname = '/index.html'
      url.search = ''
      const asset = await env.ASSETS.fetch(new Request(url, request))
      const response = new Response(asset.body, asset)
      const issuer = new URL(env.AUTH0_ISSUER).origin
      const socketOrigin = env.PUBLIC_REMOTE_ORIGIN.replace(/^http/, 'ws')
      response.headers.set('content-security-policy', [
        "default-src 'none'", "script-src 'self'", "style-src 'self' 'unsafe-inline'",
        // Received SSTV files and the local composer use validated Blob images.
        // This permission is image-only; scripts and objects retain their policy.
        "img-src 'self' data: blob:", `connect-src 'self' ${socketOrigin} ${issuer}`,
        `frame-src ${issuer}`, "worker-src 'self'", "form-action 'self'",
        // The web app manifest (the page's own, so a phone can put it on its home screen): `default-src 'none'`
        // refuses a manifest.
        "manifest-src 'self'",
        "frame-ancestors 'none'", "base-uri 'none'", "object-src 'none'",
      ].join('; '))
      response.headers.set('referrer-policy', 'no-referrer')
      response.headers.set('x-content-type-options', 'nosniff')
      // The stream's microphone over is this page's own: its origin may ask, and no frame or other
      // site can. The browser still asks the operator. The camera and location stay off.
      response.headers.set('permissions-policy', 'camera=(), microphone=(self), geolocation=()')
      response.headers.set('cache-control', 'no-store')
      return hsts(request, response)
    } catch (error) {
      // Never serialize exception details, provider responses, URLs or headers.
      return hsts(request, json({ error: error instanceof Refusal ? error.code : 'serviceUnavailable' }, error instanceof Refusal ? error.status : 503))
    }
  },
}
