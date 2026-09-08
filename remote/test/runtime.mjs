// Real workerd/D1/socket harness. All signing keys, tokens and pairing proofs are
// generated in memory; none are fixtures, log fields or test output.
import { Miniflare, convertV4MiniflareOptions } from 'miniflare'
import { generateKeyPair, exportJWK, SignJWT } from 'jose'
import { readFile } from 'node:fs/promises'
import { createServer } from 'node:net'
import assert from 'node:assert/strict'
import WebSocket from 'ws'

export async function runtime() {
  const reservation = createServer()
  await new Promise(resolve => reservation.listen(0, '127.0.0.1', resolve))
  const port = reservation.address().port
  await new Promise(resolve => reservation.close(resolve))
  const origin = `http://127.0.0.1:${port}`, issuer = 'https://identity.remote-test.invalid/'
  const { privateKey, publicKey } = await generateKeyPair('RS256', { extractable: true })
  const publicJwk = { ...await exportJWK(publicKey), kid: crypto.randomUUID(), alg: 'RS256', use: 'sig' }
  let jwksReads = 0
  const mf = new Miniflare(convertV4MiniflareOptions({
    name: 'remote-runtime-test', modules: true, scriptPath: new URL('../dist/index.js', import.meta.url).pathname,
    compatibilityDate: '2026-07-30', host: '127.0.0.1', port, cf: false,
    telemetry: { enabled: false },
    durableObjects: { STATIONS: { className: 'StationRoom', useSQLite: true } }, d1Databases: ['DB'],
    bindings: { PUBLIC_REMOTE_ORIGIN: origin, AUTH0_ISSUER: issuer, AUTH0_AUDIENCE: 'remote-test-api', AUTH0_CLIENT_ID: 'remote-test-client' },
    assets: {
      directory: new URL('../../ui/dist-remote', import.meta.url).pathname,
      routerConfig: { has_user_worker: true },
      binding: 'ASSETS', run_worker_first: ['/', '/index.html', '/api/*'],
      assetConfig: { html_handling: 'none', not_found_handling: 'none' },
    },
    outboundService: async request => {
      assert.equal(request.url, `${issuer}.well-known/jwks.json`, 'only the pinned JWKS endpoint may be fetched')
      jwksReads++
      return Response.json({ keys: [publicJwk] })
    },
  }))
  await mf.ready
  const db = await mf.getD1Database('DB')
  const migration = await readFile(new URL('../migrations/0001_observation.sql', import.meta.url), 'utf8')
  for (const statement of migration.split(';').map(s => s.trim()).filter(Boolean)) await db.prepare(statement).run()
  const token = (subject, claims = {}, key = privateKey) => new SignJWT({ azp: 'remote-test-client', ...claims })
    .setProtectedHeader({ alg: 'RS256', kid: publicJwk.kid }).setIssuer(issuer).setAudience('remote-test-api')
    .setSubject(subject).setIssuedAt().setExpirationTime('1h').sign(key)
  const idToken = (subject, nonce) => new SignJWT({ nonce })
    .setProtectedHeader({ alg: 'RS256', kid: publicJwk.kid }).setIssuer(issuer).setAudience('remote-test-client')
    .setSubject(subject).setIssuedAt().setExpirationTime('1h').sign(privateKey)
  let clientNo = 0
  function client(jwt, cookies = '', nativeToken = null) {
    const ip = `198.51.100.${++clientNo}`
    const headers = () => ({ 'content-type': 'application/json', 'cf-connecting-ip': ip,
      ...(jwt ? { origin, authorization: `Bearer ${jwt}` } : {}),
      ...(nativeToken ? { authorization: `Bearer ${nativeToken}` } : {}), ...(cookies ? { cookie: cookies } : {}) })
    return {
      headers,
      setCookie(value) { cookies = value?.split(';')[0] ?? '' },
      async post(path, value = {}, expected = 200, extra = {}) {
        const response = await mf.dispatchFetch(`${origin}/api/remote/${path}`, {
          method: 'POST', headers: { ...headers(), ...extra }, body: JSON.stringify(value),
        })
        assert.equal(response.status, expected, `HTTP status for ${path}`)
        return { value: await response.json(), response }
      },
      async open(stationId, ticket, expected = 101) {
        // Exercise actual TCP/WebSocket framing, including the close handshake.
        const ws = new WebSocket(`${origin.replace('http:', 'ws:')}/api/remote/stations/${stationId}/${ticket ? 'observe' : 'connect'}`,
          ticket ? ['nexus-observe-v1', `ticket.${ticket}`] : [], { headers: headers() })
        const peer = socket(ws)
        const status = await new Promise((resolve, reject) => {
          const timer = setTimeout(() => { ws.terminate(); reject(new Error('WebSocket handshake timeout')) }, 5000)
          ws.once('open', () => { clearTimeout(timer); resolve(101) })
          ws.once('unexpected-response', (_request, response) => { clearTimeout(timer); response.resume(); resolve(response.statusCode); ws.terminate() })
          ws.on('error', () => { clearTimeout(timer); reject(new Error('WebSocket handshake failed')) })
        })
        assert.equal(status, expected, 'WebSocket admission status')
        return status === 101 ? peer : null
      },
    }
  }
  async function owner(entitled = true) {
    const jwt = await token(`synthetic|${crypto.randomUUID()}`), browser = client(jwt)
    const { value } = await browser.post('session')
    if (entitled) await db.prepare('INSERT INTO trials(account_id,enabled,expires_at) VALUES(?,1,?)').bind(value.accountId, Date.now() + 3600000).run()
    return { ...browser, accountId: value.accountId, jwt }
  }
  async function paired() {
    const browser = await owner(), enrollmentClient = client()
    const { value: enrollment } = await enrollmentClient.post('enroll', { name: 'Synthetic test station' })
    await browser.post('pair/claim', { code: enrollment.code })
    const stationCredential = crypto.getRandomValues(new Uint8Array(32)).reduce((s, b) => s + b.toString(16).padStart(2, '0'), '')
    await enrollmentClient.post('enroll/approve', { id: enrollment.id, proof: enrollment.proof, credential: stationCredential })
    return { browser, native: client(null, '', stationCredential), stationId: enrollment.id, stationCredential }
  }
  async function approved(pair) {
    const path = `stations/${pair.stationId}`
    const { value, response } = await pair.browser.post(`${path}/device`, { name: 'Synthetic browser' })
    pair.browser.setCookie(response.headers.get('set-cookie'))
    await pair.native.post(`${path}/native/approve-device`, { deviceId: value.deviceId })
    return value.deviceId
  }
  return { mf, db, origin, token, idToken, owner, client, paired, approved, jwksReads: () => jwksReads,
    evict: stationId => mf.unsafeEvictDurableObject('remote-runtime-test', 'StationRoom', { name: stationId, webSockets: 'hibernate' }) }
}

export function socket(ws) {
  const queue = [], pending = []
  let closed = false, closeCode = null
  function publish(value) {
    const index = pending.findIndex(waiter => waiter.predicate(value))
    if (index < 0) queue.push(value)
    else { const waiter = pending.splice(index, 1)[0]; clearTimeout(waiter.timer); waiter.resolve(value) }
  }
  ws.addEventListener('message', event => { try { publish(JSON.parse(event.data)) } catch { publish(event.data) } })
  ws.addEventListener('close', event => { closed = true; closeCode = event.code; publish({ type: 'closed', code: event.code }) })
  if (typeof ws.accept === 'function') ws.accept()
  return {
    ws, get closed() { return closed }, get closeCode() { return closeCode },
    send(value) { ws.send(typeof value === 'string' ? value : JSON.stringify(value)) },
    close() { ws.close(1000, 'testComplete') },
    take(predicate, timeout = 2000) {
      const index = queue.findIndex(predicate)
      if (index >= 0) return Promise.resolve(queue.splice(index, 1)[0])
      return new Promise((resolve, reject) => {
        const waiter = { predicate, resolve, timer: null }
        waiter.timer = setTimeout(() => { const i = pending.indexOf(waiter); if (i >= 0) pending.splice(i, 1); reject(new Error('socket event timeout')) }, timeout)
        pending.push(waiter)
      })
    },
  }
}

export async function roomStatus(room) {
  const state = await room.status(), plain = {}
  for (const key of ['enabled', 'online', 'observers', 'sockets', 'runtimeSockets']) plain[key] = await state[key]
  return plain
}
