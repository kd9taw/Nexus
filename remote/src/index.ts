// Same-origin browser API and outbound station admission. No radio command router.
import { account, access, body, browserOrigin, cookie, device, digest, id, label,
  native, proof, rate, Refusal, requireTrial, requireValue, secret, station, trial, uuid } from './authority'
import type { RemoteEnv, StationRow } from './authority'
import { observerDeadline } from '../../ui/src/remote-monitor/relay'
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
    applicationVersion: 4,
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
          ? request.headers.get('x-nexus-application-recall-version') === '1' ? 4 : 3
          : request.headers.get('x-nexus-application-stream-version') === '2' ? 2
          : ['1', '2'].includes(request.headers.get('x-nexus-application-version') ?? '') ? Number(request.headers.get('x-nexus-application-version')) : 0,
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
      entitlement: await trial(env, ticket.account_id) })
  }
  requireValue(request.method === 'POST', 'methodNotAllowed', 405)

  if (path === 'enroll') {
    requireValue(!request.headers.has('origin'), 'originDenied')
    await rate(env, 'enroll-global', now, 1000, 600000)
    await rate(env, `enroll:${request.headers.get('cf-connecting-ip') ?? 'local'}`, now, 5, 600000)
    const input = await body(request, ['name']), stationId = uuid(), credential = secret(), code = secret().slice(0, 16)
    await env.DB.batch([
      env.DB.prepare('DELETE FROM enrollments WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM tickets WHERE expires_at<=?').bind(now),
      env.DB.prepare('DELETE FROM rate_limits WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO enrollments(id,name,proof_hash,code_hash,expires_at) VALUES(?,?,?,?,?)')
        .bind(stationId, label(input.name), await digest(credential), await digest(code), now + 600000),
    ])
    return json({ id: stationId, proof: credential, code, expiresAt: now + 600000 })
  }
  if (path === 'enroll/check' || path === 'enroll/approve') {
    requireValue(!request.headers.has('origin'), 'originDenied')
    const input = await body(request, path.endsWith('approve') ? ['id', 'proof', 'credential'] : ['id', 'proof'])
    const stationId = id(input.id), hash = await digest(proof(input.proof))
    await rate(env, `enrollment:${stationId}`, now, 30)
    const pending = await env.DB.prepare('SELECT account_id,approved FROM enrollments WHERE id=? AND proof_hash=? AND expires_at>?')
      .bind(stationId, hash, now).first<{ account_id: string | null; approved: number }>()
    requireValue(pending, 'pairingExpired', 410)
    if (path.endsWith('check')) return json({ accountId: pending.account_id, approved: pending.approved === 1 })
    requireValue(pending.account_id, 'accountNotClaimed', 409)
    requireTrial(await trial(env, pending.account_id), now)
    const credentialHash = await digest(proof(input.credential))
    await env.DB.batch([
      env.DB.prepare(`INSERT OR IGNORE INTO stations(id,account_id,name,credential_hash)
        SELECT id,account_id,name,? FROM enrollments e WHERE id=? AND proof_hash=? AND expires_at>?
        AND account_id IS NOT NULL AND approved=0 AND (SELECT COUNT(*) FROM stations s WHERE s.account_id=e.account_id AND s.enabled=1)<2`)
        .bind(credentialHash, stationId, hash, now),
      env.DB.prepare(`UPDATE enrollments SET approved=1 WHERE id=? AND proof_hash=?
        AND EXISTS(SELECT 1 FROM stations WHERE id=? AND credential_hash=?)`)
        .bind(stationId, hash, stationId, credentialHash),
    ])
    const accepted = await env.DB.prepare('SELECT id FROM stations WHERE id=? AND credential_hash=? AND enabled=1')
      .bind(stationId, credentialHash).first()
    requireValue(accepted, 'stationLimit', 409)
    return json({ stationId, accountId: pending.account_id })
  }

  if (match?.[2].startsWith('native/')) {
    const verb = match[2].slice(7)
    const row = await native(request, env, match[1], verb === 'revoke')
    await rate(env, `native:${row.id}`, now, 60)
    if (verb === 'devices') {
      await body(request, [])
      const devices = await env.DB.prepare('SELECT id,name,approved,expires_at AS expiresAt FROM devices WHERE station_id=? AND expires_at>? ORDER BY id LIMIT 8')
        .bind(row.id, now).all()
      return json({ devices: devices.results })
    }
    if (verb === 'approve-device' || verb === 'revoke-device') {
      const input = await body(request, ['deviceId']), deviceId = id(input.deviceId)
      const found = await env.DB.prepare('SELECT id FROM devices WHERE id=? AND station_id=? AND expires_at>?')
        .bind(deviceId, row.id, now).first()
      requireValue(found, 'deviceUnavailable', 404)
      const approve = verb === 'approve-device'
      await env.DB.batch([
        env.DB.prepare('UPDATE devices SET approved=?,generation=generation+1,expires_at=? WHERE id=? AND station_id=?')
          .bind(approve ? 1 : 0, approve ? now + 2592000000 : now, deviceId, row.id),
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
  const identity = await account(request, env, now)
  await rate(env, `account:${identity.accountId}`, now, 120)
  const entitlement = await trial(env, identity.accountId)
  if (path === 'session') {
    await body(request, [])
    const rows = await env.DB.prepare('SELECT id,name,enabled FROM stations WHERE account_id=? AND enabled=1 ORDER BY id LIMIT 2')
      .bind(identity.accountId).all<{ id: string; name: string; enabled: number }>()
    const stations = await Promise.all(rows.results.map(async row => ({ id: row.id, name: row.name,
      device: await device(request, env, row.id, identity.accountId, now) })))
    return json({ accountId: identity.accountId, entitlement, stations })
  }
  if (path === 'pair/claim') {
    requireTrial(entitlement, now)
    await rate(env, `claim:${identity.accountId}`, now, 5, 600000)
    const input = await body(request, ['code'])
    requireValue(typeof input.code === 'string' && /^[0-9a-f]{16}$/.test(input.code), 'invalidPairingCode', 400)
    const row = await env.DB.prepare(`UPDATE enrollments SET account_id=? WHERE code_hash=? AND expires_at>?
      AND account_id IS NULL AND approved=0 RETURNING id,name`)
      .bind(identity.accountId, await digest(input.code), now).first()
    requireValue(row, 'invalidPairingCode', 400)
    return json({ station: row, accountId: identity.accountId })
  }
  requireValue(match, 'notFound', 404)
  const row = await station(env, match[1]), verb = match[2]
  requireValue(row.account_id === identity.accountId && row.enabled === 1, 'stationUnavailable', 404)
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
      return json({ ok: true })
    }
    await rate(env, `ticket:${row.id}:${current.id}`, now, 12)
    const ticket = secret()
    await env.DB.batch([
      env.DB.prepare('DELETE FROM tickets WHERE expires_at<=?').bind(now),
      env.DB.prepare('INSERT INTO tickets(digest,station_id,account_id,device_id,device_generation,identity_until,expires_at) VALUES(?,?,?,?,?,?,?)')
        .bind(await digest(ticket), row.id, identity.accountId, current.id, current.generation, browser.expiresAt, now + 15000),
    ])
    return json({ ticket, serverNow: now })
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
        "img-src 'self' data:", `connect-src 'self' ${socketOrigin} ${issuer}`,
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
