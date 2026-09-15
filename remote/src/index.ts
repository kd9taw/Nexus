// Same-origin browser API and outbound station admission. No radio command router.
import { account, access, APPROVAL_LIMIT_MS, APPROVAL_MS, body, browserOrigin, caller, cookie, device, deviceCookie, digest, id, label,
  lifetime, native, proof, rate, Refusal, renewsUntil, requireAdmin, requireEligible, requireTrial, requireUnspentIdentity, requireValue, secret, station,
  trial, TRIAL_MS, uuid } from './authority'
import type { DeviceRow, RemoteEnv, StationRow } from './authority'
import { observerDeadline } from '../../ui/src/remote-monitor/relay'
import { advertisedOperationVersion } from '../../ui/src/remote-web/operation-version'
export { StationRoom } from './room'

function json(value: unknown, status = 200, headers?: HeadersInit): Response {
  const result = new Response(JSON.stringify(value), { status, headers })
  result.headers.set('content-type', 'application/json')
  result.headers.set('cache-control', 'no-store')
  result.headers.set('x-content-type-options', 'nosniff')
  result.headers.set('referrer-policy', 'no-referrer')
  return result
}
async function room(env: RemoteEnv, stationId: string, path: string, value?: unknown): Promise<Response> {
  // Construct fresh internal headers. Public requests cannot supply an admission.
  const upgrade = path === 'station' || path === 'browser'
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
    ready: env.AUTH0_CLIENT_ID !== 'unconfigured',
    revision: env.REMOTE_BUILD_REVISION ?? 'local',
    applicationVersion: 17,
    operationVersion: 2,
    operationMaxVersion: 3,
    operationFtVersion: 1,
  })
  const match = /^stations\/([0-9a-f-]{36})\/(.+)$/.exec(path)
  if (request.method === 'GET' && match && ['connect', 'observe'].includes(match[2])) {
    const stationId = id(match[1])
    requireValue(request.headers.get('upgrade')?.toLowerCase() === 'websocket', 'webSocketRequired', 426)
    if (match[2] === 'connect') {
      const row = await native(request, env, stationId)
      await rate(env, `connect:${stationId}`, now, 20)
      return room(env, stationId, 'station', { access: await access(env, row, now),
        applicationVersion: request.headers.get('x-nexus-application-query-version') === '1' && request.headers.get('x-nexus-application-stream-version') === '2'
          ? request.headers.get('x-nexus-application-recall-version') === '1'
            ? request.headers.get('x-nexus-application-keyboard-version') === '1'
              ? request.headers.get('x-nexus-application-insights-version') === '1'
                ? request.headers.get('x-nexus-application-dxpeditions-version') === '1'
                  ? request.headers.get('x-nexus-application-memories-version') === '1'
                    ? request.headers.get('x-nexus-application-ota-version') === '1'
                      ? request.headers.get('x-nexus-application-field-day-version') === '1'
                        ? request.headers.get('x-nexus-application-js8-version') === '1' ? request.headers.get('x-nexus-application-station-modes-version') === '1' ? request.headers.get('x-nexus-application-navigation-version') === '1' ? request.headers.get('x-nexus-application-configuration-version') === '1' ? request.headers.get('x-nexus-application-lookups-version') === '1' ? request.headers.get('x-nexus-application-alerts-version') === '1' ? request.headers.get('x-nexus-application-rotator-version') === '1' ? 17 : 16 : 15 : 14 : 13 : 12 : 11 : 10 : 9 : 8 : 7 : 6
                : 5
              : 4
            : 3
          : request.headers.get('x-nexus-application-stream-version') === '2' ? 2
          : ['1', '2'].includes(request.headers.get('x-nexus-application-version') ?? '') ? Number(request.headers.get('x-nexus-application-version')) : 0,
        operationVersion: advertisedOperationVersion(
          ['1','2'].includes(request.headers.get('x-nexus-operation-version')??'') ? Number(request.headers.get('x-nexus-operation-version')) : 0,
          request.headers.get('x-nexus-operation-max-version') === '3' ? 3 : 0,
          request.headers.get('x-nexus-operation-ft-version') === '1' ? 1 : 0,
        ),
        identity: { stationId, accountId: row.account_id, generation: row.generation, expiresAt: now + 86400000 } })
    }
    browserOrigin(request, env)
    const protocols = (request.headers.get('sec-websocket-protocol') ?? '').split(',').map(s => s.trim())
    requireValue(protocols.length === 2 && protocols[0] === 'nexus-observe-v1' && /^ticket\.[0-9a-f]{64}$/.test(protocols[1]), 'invalidTicket', 401)
    // DELETE RETURNING makes replay refusal atomic across concurrent Worker instances.
    const ticket = await env.DB.prepare(`DELETE FROM tickets WHERE digest=? AND station_id=? AND expires_at>? RETURNING *`)
      .bind(await digest(protocols[1].slice(7)), stationId, now).first<{
        account_id: string; device_id: string; device_generation: number; identity_until: number
      }>()
    requireValue(ticket, 'invalidTicket', 401)
    const row = await station(env, stationId)
    return room(env, stationId, 'browser', { access: await access(env, row, now), sessionId: uuid(),
      identity: { accountId: ticket.account_id, deviceId: ticket.device_id,
        deviceGeneration: ticket.device_generation, expiresAt: ticket.identity_until },
      entitlement: await trial(env, ticket.account_id, now) })
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
    ])
    await rate(env, 'enroll-global', now, 1000, 600000)
    const input = await body(request, ['name']), stationId = uuid(), credential = secret(), code = secret().slice(0, 16)
    await env.DB.prepare('INSERT INTO enrollments(id,name,proof_hash,code_hash,expires_at) VALUES(?,?,?,?,?)')
      .bind(stationId, label(input.name), await digest(credential), await digest(code), now + 600000).run()
    return json({ id: stationId, proof: credential, code, expiresAt: now + 600000 })
  }
  if (path === 'enroll/check' || path === 'enroll/approve') {
    requireValue(!request.headers.has('origin'), 'originDenied')
    const input = await body(request, path.endsWith('approve') ? ['id', 'proof', 'credential'] : ['id', 'proof'])
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
    requireEligible(await trial(env, pending.account_id, now))
    await requireUnspentIdentity(env, pending.account_id, now)
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
      // The approval time is recorded only for a Nexus that binds its grants to the generation, so only
      // its approvals ever renew (authority.ts, `lifetime`).
      env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,approved,approved_at,expires_at)
        SELECT ?,e.id,e.account_id,'Paired browser',e.device_hash,1,?,? FROM enrollments e
        WHERE e.id=? AND e.proof_hash=? AND e.device_hash IS NOT NULL
        AND EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)
        AND NOT EXISTS(SELECT 1 FROM devices d WHERE d.station_id=e.id AND d.credential_hash=e.device_hash)`)
        .bind(uuid(), lifetime(request) ? now : null, now + APPROVAL_MS, stationId, hash, stationId, credentialHash),
      env.DB.prepare(`INSERT INTO trials(account_id, enabled, expires_at, started_at, source)
        SELECT ?,1,?,?,'trial' WHERE EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)
        ON CONFLICT(account_id) DO NOTHING`)
        .bind(pending.account_id, now + TRIAL_MS, now, stationId, credentialHash),
    ])
    // Read both rows back before answering 200. That makes "the station and the clock landed
    // together" an observable property of the response rather than a claim about D1's rollback
    // semantics, which are unproven here: every test runs against local Miniflare D1.
    const accepted = await env.DB.prepare('SELECT id FROM stations WHERE id=? AND credential_hash=? AND enabled=1')
      .bind(stationId, credentialHash).first()
    requireValue(accepted, 'stationLimit', 409)
    const started = await trial(env, pending.account_id, now)
    requireValue(started.state === 'active', 'serviceUnavailable', 503)
    // The approved pairing browser, so the shack can give it station control and logging without
    // waiting for a browser list. Additive: the shack reads stationId and accountId by name and has
    // always ignored anything else here. `null` when the confirm came from a browser of an older era.
    // `generation` is additive here too: the shack reads `device.id` and `device.expiresAt` by name.
    const paired = await env.DB.prepare(`SELECT d.id, d.expires_at AS expiresAt, d.generation FROM devices d
      JOIN enrollments e ON d.station_id=e.id AND d.credential_hash=e.device_hash
      WHERE e.id=? AND e.proof_hash=? AND d.approved=1 AND d.expires_at>?`)
      .bind(stationId, hash, now).first<{ id: string; expiresAt: number; generation: number }>()
    return json({ stationId, accountId: pending.account_id, entitlement: started, device: paired ?? null })
  }

  if (match?.[2].startsWith('native/')) {
    const verb = match[2].slice(7)
    const row = await native(request, env, match[1], verb === 'revoke')
    await rate(env, `native:${row.id}`, now, 60)
    if (verb === 'devices') {
      await body(request, [])
      // The generation and the renewal limit go ONLY to a Nexus that asks. 1.12.0 parses this list with
      // deny_unknown_fields, so a field added for everyone would fail every refresh it makes.
      const devices = lifetime(request)
        ? await env.DB.prepare(`SELECT id,name,approved,expires_at AS expiresAt,generation,
            CASE WHEN approved=1 AND approved_at IS NOT NULL THEN approved_at+? END AS renewsUntil
            FROM devices WHERE station_id=? AND expires_at>? ORDER BY id LIMIT 8`).bind(APPROVAL_LIMIT_MS, row.id, now).all()
        : await env.DB.prepare('SELECT id,name,approved,expires_at AS expiresAt FROM devices WHERE station_id=? AND expires_at>? ORDER BY id LIMIT 8')
          .bind(row.id, now).all()
      return json({ devices: devices.results })
    }
    if (verb === 'approve-device' || verb === 'revoke-device') {
      const input = await body(request, ['deviceId']), deviceId = id(input.deviceId)
      const found = await env.DB.prepare('SELECT id FROM devices WHERE id=? AND station_id=? AND expires_at>?')
        .bind(deviceId, row.id, now).first()
      requireValue(found, 'deviceUnavailable', 404)
      const approve = verb === 'approve-device'
      // Every approval, including approving again, is a new generation and a new thirty days; with the
      // lifetime header it also restarts the ninety-day limit. Revoking ends it now and clears the time.
      await env.DB.batch([
        env.DB.prepare('UPDATE devices SET approved=?,generation=generation+1,expires_at=?,approved_at=? WHERE id=? AND station_id=?')
          .bind(approve ? 1 : 0, approve ? now + APPROVAL_MS : now, approve && lifetime(request) ? now : null, deviceId, row.id),
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
    throw new Refusal('notFound', 404)
  }

  // Every remaining operation is account-authenticated and subject to exact Origin.
  // Destructured off deliberately: `identity` is spread into the browser identity below and on
  // into relay socket state, and the provider subject has no business travelling with it.
  // `verified` is destructured off for the same reason `subject` is: `identity` is spread into
  // the browser identity and on into relay socket state, and how this account was identified is
  // not something the wire needs to carry.
  const { subject, verified, ...identity } = await account(request, env, now)
  await rate(env, `account:${identity.accountId}`, now, 120)
  const entitlement = await trial(env, identity.accountId, now)
  // The only privileged write in the service. It exists because every trial in a closed beta is
  // granted by hand, and the alternative was a wrangler command the operator can only run from a
  // laptop. Deliberately ONE action: each further admin verb is another way for a mistake here to
  // become an entitlement bypass.
  if (path === 'admin/grant-trial') {
    requireAdmin(env, subject)
    const input = await body(request, ['accountId', 'days'])
    const target = id(input.accountId)
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
    const granted = await trial(env, target, now)
    requireValue(granted.state === 'active', 'stationUnavailable', 404)
    return json({ accountId: target, entitlement: granted })
  }
  if (path === 'session') {
    await body(request, [])
    const rows = await env.DB.prepare('SELECT id,name,enabled FROM stations WHERE account_id=? AND enabled=1 ORDER BY id LIMIT 2')
      .bind(identity.accountId).all<{ id: string; name: string; enabled: number }>()
    // The browser's own fields as they have always been, plus the end that use cannot move so the page
    // can warn before it. The approval time itself stays in the Worker.
    const stations = await Promise.all(rows.results.map(async row => {
      const current = await device(request, env, row.id, identity.accountId, now)
      return { id: row.id, name: row.name, device: current && { id: current.id, name: current.name, generation: current.generation,
        approved: current.approved, expires_at: current.expires_at, renewsUntil: renewsUntil(current) } }
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
    return json({ accountId: identity.accountId, entitlement, stations, pending,
      identityVerified: verified, serverNow: now })
  }
  if (path === 'pair/claim') {
    requireEligible(entitlement)
    await requireUnspentIdentity(env, identity.accountId, now)
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
    requireEligible(entitlement)
    await requireUnspentIdentity(env, identity.accountId, now)
    await rate(env, `confirm:${identity.accountId}`, now, 10, 600000)
    const input = await body(request, ['id'])
    requireValue(typeof input.id === 'string' && /^[0-9a-f-]{36}$/.test(input.id), 'invalidPairingCode', 400)
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
    const row = await env.DB.prepare(`UPDATE enrollments SET confirmed=1, device_hash=?
      WHERE id=? AND account_id=? AND approved=0 AND expires_at>? RETURNING id,name`)
      .bind(await digest(credential), input.id, identity.accountId, now).first<{ id: string; name: string }>()
    requireValue(row, 'invalidPairingCode', 400)
    return json({ station: row, accountId: identity.accountId }, 200, { 'set-cookie': cookie(row.id, credential) })
  }
  requireValue(match, 'notFound', 404)
  const row = await station(env, match[1]), verb = match[2]
  requireValue(row.account_id === identity.accountId && row.enabled === 1, 'stationUnavailable', 404)
  // Above requireTrial, with revoke and forget-device: renaming is housekeeping on your own
  // account, and an operator whose service access has lapsed should still be able to tidy it.
  // `name` is not part of StationAccess, so no policy version moves and no room needs syncing -
  // this changes a label, not an authority.
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
  requireTrial(entitlement, now)
  if (verb === 'device') {
    const input = await body(request, ['name']), name = label(input.name)
    const current = await device(request, env, row.id, identity.accountId, now)
    if (current) return json({ deviceId: current.id, approved: current.approved === 1 })
    const credential = secret(), deviceId = uuid()
    const inserted = await env.DB.prepare(`INSERT INTO devices(id,station_id,account_id,name,credential_hash,expires_at)
      SELECT ?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM devices WHERE station_id=? AND expires_at>?)<8 RETURNING id`)
      .bind(deviceId, row.id, identity.accountId, name, await digest(credential), now + 600000, row.id, now).first()
    requireValue(inserted, 'deviceLimit', 409)
    return json({ deviceId, approved: false }, 200, { 'set-cookie': cookie(row.id, credential) })
  }
  if (verb === 'ticket' || verb === 'renew') {
    const input = await body(request, verb === 'renew' ? ['sessionId'] : [])
    const current = await device(request, env, row.id, identity.accountId, now)
    requireValue(current?.approved === 1, 'deviceNotApproved')
    const browser = { ...identity, deviceId: current.id, deviceGeneration: current.generation,
      expiresAt: Math.min(identity.expiresAt, current.expires_at) }
    const policy = await access(env, row, now)
    observerDeadline(policy, browser, entitlement, now)
    if (verb === 'renew') {
      const response = await room(env, row.id, 'renew', { access: policy,
        sessionId: id(input.sessionId), identity: browser, entitlement })
      requireValue(response.ok, 'sessionNotApproved')
      // Renewed only once the live session this browser holds has been confirmed.
      return json({ ok: true, serverNow: now }, 200, await renewApproval(request, env, row.id, current, now))
    }
    await rate(env, `ticket:${row.id}:${current.id}`, now, 12)
    const ticket = secret()
    await env.DB.batch([
      env.DB.prepare('DELETE FROM tickets WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO tickets(digest,station_id,account_id,device_id,device_generation,identity_until,expires_at) VALUES(?,?,?,?,?,?,?)')
        .bind(await digest(ticket), row.id, identity.accountId, current.id, current.generation, browser.expiresAt, now + 15000),
    ])
    return json({ ticket, serverNow: now }, 200, await renewApproval(request, env, row.id, current, now))
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

// Browser approval lifetime (operator decision 2026-09-14). Reached only from `ticket` and `renew`, after
// the request has been admitted: the account's token, THIS browser's own credential, an approved and
// unexpired device, a live trial, and for `renew` the live session. So a page load, another browser of
// the same account, or a browser still waiting for approval renews nothing.
//
// It moves the expiry and nothing else. Not the generation, which live sessions and the shack's
// remembered grants are bound to; not `approved`; never a permission. The write is conditional on the
// row still being exactly the approval that was authorised, so a revoke or approval racing it wins.
// An approval with no approval time was given by a Nexus that binds its grants to the expiry, and
// never renews. Returns the response headers: the browser's credential, re-issued unchanged with the
// approval's new lifetime, or nothing.
const RENEWAL_STEP_MS = 3600000
async function renewApproval(request: Request, env: RemoteEnv, stationId: string, current: DeviceRow, now: number): Promise<HeadersInit | undefined> {
  if (current.approved !== 1 || current.approved_at === null) return undefined
  const limit = current.approved_at + APPROVAL_LIMIT_MS, until = Math.min(now + APPROVAL_MS, limit)
  // At most one write an hour per browser, except the final step, which lands exactly on the limit.
  if (until <= current.expires_at || (until < limit && until - current.expires_at < RENEWAL_STEP_MS)) return undefined
  const renewed = await env.DB.prepare(`UPDATE devices SET expires_at=? WHERE id=? AND station_id=? AND approved=1
    AND generation=? AND approved_at=? AND expires_at=? AND expires_at>? RETURNING id`)
    .bind(until, current.id, stationId, current.generation, current.approved_at, current.expires_at, now).first()
  const credential = deviceCookie(request, stationId)
  return renewed && credential ? { 'set-cookie': cookie(stationId, credential, Math.floor((until - now) / 1000)) } : undefined
}

export default {
  async fetch(request: Request, env: RemoteEnv): Promise<Response> {
    try {
      const url = new URL(request.url)
      if (url.pathname.startsWith('/api/')) return await api(request, env)
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
        "frame-ancestors 'none'", "base-uri 'none'", "object-src 'none'",
      ].join('; '))
      response.headers.set('referrer-policy', 'no-referrer')
      response.headers.set('x-content-type-options', 'nosniff')
      response.headers.set('permissions-policy', 'camera=(), microphone=(), geolocation=()')
      response.headers.set('cache-control', 'no-store')
      return response
    } catch (error) {
      // Never serialize exception details, provider responses, URLs or headers.
      return json({ error: error instanceof Refusal ? error.code : 'serviceUnavailable' }, error instanceof Refusal ? error.status : 503)
    }
  },
}
