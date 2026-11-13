// The native client: Nexus on another computer driving its station over the internet. Its caller (an
// access token the Native application was given, and its device key's signature over one-use
// challenges), its devices under the account, the station key over the signed-in API, TURN for a
// device, and the room's pipe to the station. The issuer is the runtime's local stand-in: no real
// tenant, token or key is used or made, and every key here is made in memory.
import { before, after, test } from 'node:test'
import assert from 'node:assert/strict'
import { setTimeout as delay } from 'node:timers/promises'
import { generateKeyPair, SignJWT } from 'jose'
import WebSocket from 'ws'
import { runtime, roomStatus, socket } from './runtime.mjs'

const NATIVE = 'remote-test-native'
let app
before(async () => { app = await runtime({ bindings: { AUTH0_NATIVE_CLIENT_ID: NATIVE } }) })
after(async () => { await app?.mf.dispose() })

const DAY = 86400000, UNTIL_REMOVED = Date.UTC(9999, 11, 31, 23, 59, 59, 999)
const LIFETIME = { 'x-nexus-device-lifetime': '1' }, KINDS = { 'x-nexus-device-kind': '1' }, KEYS = { 'x-nexus-device-key': '1' }
const PIPE = { 'x-nexus-pipe-version': '1' }
const type = name => value => value?.type === name
const hex = bytes => Array.from(new Uint8Array(bytes), b => b.toString(16).padStart(2, '0')).join('')
const sha256 = async text => hex(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text)))
const subjectOf = jwt => JSON.parse(Buffer.from(jwt.split('.')[1], 'base64url')).sub
const deviceRow = id => app.db.prepare('SELECT approved,approved_at,expires_at,generation,kind,platform,public_key FROM devices WHERE id=?').bind(id).first()

/** A device key as the native client holds one: P-256, its public half as SPKI in lowercase hex. */
async function deviceKey() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify'])
  return { pair, publicKey: hex(await crypto.subtle.exportKey('spki', pair.publicKey)) }
}
/** The five lines a native request's signature covers, signed as the client signs them. */
async function sign(key, challenge, method, path, body, label = 'nexus-native-request/1') {
  const text = [label, challenge, method, path, await sha256(body)].join('\n')
  return hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, key.pair.privateKey, new TextEncoder().encode(text)))
}
/** The native client for a pairing's account: no Origin, no cookie, the Native application's token. */
async function nativeCaller(pair) {
  return app.client(null, '', await app.token(subjectOf(pair.browser.jwt), { azp: NATIVE }))
}
/** A request as the computer signs it, over a challenge it has just been given. */
async function signed(c, path, value = {}, expected = 200, { key = c.key, device = c.deviceId, extra = {} } = {}) {
  const { value: { challenge } } = await c.native.post('client/challenge')
  return c.native.post(path, value, expected, { 'x-nexus-challenge': challenge,
    'x-nexus-signature': await sign(key, challenge, 'POST', `/api/remote/${path}`, JSON.stringify(value)),
    ...(device ? { 'x-nexus-device-id': device } : {}), ...extra })
}
/** A computer registered as a device of the pairing's station, approved at the shack unless told not. */
async function computer(pair, { approve = true, lifetime = true, kind = 'computer', platform = 'windows', name = 'Shack laptop' } = {}) {
  pair ??= await app.paired()
  const c = { pair, stationId: pair.stationId, native: await nativeCaller(pair), key: await deviceKey() }
  const { value } = await signed(c, `stations/${pair.stationId}/client/device`, { name, kind, platform, publicKey: c.key.publicKey })
  c.deviceId = value.deviceId
  if (approve) await pair.native.post(`stations/${pair.stationId}/native/approve-device`, { deviceId: c.deviceId }, 200, lifetime ? LIFETIME : {})
  return c
}
/** A WebSocket upgrade with exactly these headers: the socket once it opens, or its refusal by code. */
function upgrade(path, headers) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`${app.origin.replace('http:', 'ws:')}/api/remote/${path}`, [], { headers })
    const peer = socket(ws)
    const timer = setTimeout(() => { ws.terminate(); reject(new Error('WebSocket handshake timeout')) }, 15000)
    ws.once('open', () => { clearTimeout(timer); resolve({ status: 101, peer }) })
    ws.once('unexpected-response', (_request, response) => {
      clearTimeout(timer)
      let text = ''
      response.on('data', chunk => { text += chunk })
      response.on('end', () => { ws.terminate(); let error = null; try { error = JSON.parse(text).error } catch { /* no body */ } resolve({ status: response.statusCode, error }) })
    })
    ws.on('error', error => { clearTimeout(timer); reject(error) })
  })
}
/** The computer's end of its pipe, signed as a GET over an empty body. */
async function openPipe(c, { device = c.deviceId, key = c.key } = {}) {
  const { value: { challenge } } = await c.native.post('client/challenge')
  const path = `stations/${c.stationId}/client/pipe`
  return upgrade(path, { ...c.native.headers(), 'x-nexus-device-id': device, 'x-nexus-challenge': challenge,
    'x-nexus-signature': await sign(key, challenge, 'GET', `/api/remote/${path}`, '') })
}
/** The station's end, as its main socket was asked to open it. */
const stationEnd = (pair, pipeId, headers = pair.native.headers()) => upgrade(`stations/${pair.stationId}/pipe`, { ...headers, 'x-nexus-pipe-id': pipeId })
/** Bytes no JSON reader takes for text: what a TLS record looks like from outside. */
const record = (...bytes) => Buffer.from([0x17, 0x03, 0x03, 0x00, bytes.length, ...bytes])
const binary = value => Buffer.isBuffer(value)
/** A whole pipe: the station's main socket advertising it, the computer's end, and the station's, with
 *  a first message each way, as the TLS handshake inside always makes. (Until a socket has sent one,
 *  local workerd can hold its close back for ws's thirty-second timeout: remote/README.md.) */
async function piped(c) {
  const station = c.station ?? await c.pair.native.open(c.stationId, null, 101, PIPE)
  c.station = station
  const opening = openPipe(c), asked = await station.take(type('pipeOpen'))
  const end = await stationEnd(c.pair, asked.pipeId), client = await opening
  assert.equal(end.status, 101, 'the station end opens')
  assert.equal(client.status, 101, 'the computer end opens')
  client.peer.ws.send(record(0x16)); assert.deepEqual(await end.peer.take(binary), record(0x16))
  end.peer.ws.send(record(0x16, 0x02)); assert.deepEqual(await client.peer.take(binary), record(0x16, 0x02))
  return { station, end: end.peer, client: client.peer, pipeId: asked.pipeId }
}
const room = async stationId => {
  const namespace = await app.mf.getDurableObjectNamespace('STATIONS')
  return namespace.get(namespace.idFromName(stationId))
}
const pipesOf = async stationId => { const status = await (await room(stationId)).status(); return await status.pipes }

// --- The caller -----------------------------------------------------------------------------------

test('a native token opens the native routes alone, and a page\'s token opens none of them', async () => {
  const pair = await app.paired(), native = await nativeCaller(pair)
  // CONTROL: the Native application's token is taken here, for the same account the page signed in to.
  const { value: session } = await native.post('client/session')
  assert.equal(session.accountId, pair.browser.accountId)
  // The page's own token, here: with no Origin it reaches the verifier, and the verifier refuses it.
  assert.equal((await app.client(null, '', pair.browser.jwt).post('client/session', {}, 401)).value.error, 'signInRequired')
  // The native token on the page's route: refused with the page's own Origin, and without one.
  const nativeAsPage = app.client(await app.token(subjectOf(pair.browser.jwt), { azp: NATIVE }))
  assert.equal((await nativeAsPage.post('session', {}, 401)).value.error, 'signInRequired')
  assert.equal((await native.post('session', {}, 403)).value.error, 'originDenied')
  // CONTROL: the page's token still opens the page's route.
  await pair.browser.post('session')
  // Any other client id, a key the issuer never published, an unsigned or HMAC token, the station's
  // own credential: none is a native caller.
  const subject = subjectOf(pair.browser.jwt), { privateKey } = await generateKeyPair('RS256')
  const unsigned = `${Buffer.from('{"alg":"none"}').toString('base64url')}.${Buffer.from(JSON.stringify({ sub: subject, azp: NATIVE,
    iss: 'https://identity.remote-test.invalid/', aud: 'remote-test-api', iat: Math.floor(Date.now() / 1000), exp: Math.floor(Date.now() / 1000) + 3600 })).toString('base64url')}.`
  const hmac = await new SignJWT({ azp: NATIVE }).setProtectedHeader({ alg: 'HS256' }).setIssuer('https://identity.remote-test.invalid/')
    .setAudience('remote-test-api').setSubject(subject).setIssuedAt().setExpirationTime('1h').sign(crypto.getRandomValues(new Uint8Array(32)))
  for (const [why, credential] of [['another client', await app.token(subject, { azp: 'another-client' })],
    ['no client', await app.token(subject, { azp: undefined })], ['another key', await app.token(subject, { azp: NATIVE }, privateKey)],
    ['unsigned', unsigned], ['HMAC', hmac], ['the station credential', pair.stationCredential]]) {
    assert.equal((await app.client(null, '', credential).post('client/session', {}, 401)).value.error, 'signInRequired', why)
  }
  assert.equal((await app.client().post('client/session', {}, 401)).value.error, 'signInRequired', 'no credential at all')
})

test('a native route refuses whatever a page could send: an Origin, a cookie, a preflight, and no CORS answer ever', async () => {
  const pair = await app.paired(), native = await nativeCaller(pair)
  const answers = []
  const refused = async (extra, why) => {
    const { value, response } = await native.post('client/challenge', {}, 403, extra)
    assert.equal(value.error, 'originDenied', why); answers.push(response)
  }
  // CONTROL: the very same request without them is taken.
  answers.push((await native.post('client/challenge')).response)
  await refused({ origin: app.origin }, 'the service\'s own Origin')
  await refused({ origin: 'https://elsewhere.invalid' }, 'a foreign Origin')
  await refused({ origin: 'null' }, 'an opaque Origin')
  await refused({ cookie: `__Host-nexus-remote-${pair.stationId}=${'a'.repeat(64)}` }, 'a cookie and no Origin')
  const preflight = await app.mf.dispatchFetch(`${app.origin}/api/remote/client/challenge`, { method: 'OPTIONS',
    headers: { origin: 'https://elsewhere.invalid', 'access-control-request-method': 'POST', 'access-control-request-headers': 'authorization,x-nexus-signature' } })
  assert.equal(preflight.status, 403)
  assert.equal((await preflight.json()).error, 'originDenied')
  answers.push(preflight)
  for (const response of answers) {
    assert.deepEqual([...response.headers.keys()].filter(name => name.startsWith('access-control-')), [], `${response.status}: no CORS answer`)
  }
  // The station's routes stay as closed to this caller as to a page: a token is not a station credential.
  assert.equal((await native.post(`stations/${pair.stationId}/native/devices`, {}, 400)).value.error, 'invalidRequest')
})

test('an unconfigured Native application refuses every native route by name, and so does one that is the page\'s', async () => {
  for (const [why, bindings] of [['unset', {}], ['unconfigured', { AUTH0_NATIVE_CLIENT_ID: 'unconfigured' }], ['empty', { AUTH0_NATIVE_CLIENT_ID: '' }],
    ['the page\'s own client id', { AUTH0_NATIVE_CLIENT_ID: 'remote-test-client' }]]) {
    const service = await runtime({ bindings })
    try {
      const pair = await service.paired(), subject = subjectOf(pair.browser.jwt)
      for (const azp of [NATIVE, 'remote-test-client']) {
        const caller = service.client(null, '', await service.token(subject, { azp }))
        for (const path of ['client/challenge', 'client/session', `stations/${pair.stationId}/client/device`]) {
          assert.equal((await caller.post(path, {}, 503)).value.error, 'serviceNotConfigured', `${why}: ${path} as ${azp}`)
        }
      }
      // CONTROL: the page is unaffected.
      await pair.browser.post('session')
    } finally { await service.mf.dispose() }
  }
})

test('replay: a signed request is taken once, under a challenge its own account was given within the minute', async () => {
  const pair = await app.paired(), c = { pair, stationId: pair.stationId, native: await nativeCaller(pair), key: await deviceKey() }
  const path = `stations/${pair.stationId}/client/device`, value = { name: 'Shack laptop', kind: 'computer', platform: 'linux', publicKey: c.key.publicKey }
  const body = JSON.stringify(value), at = `/api/remote/${path}`
  const challenged = async () => (await c.native.post('client/challenge')).value
  const given = await challenged()
  assert.ok(given.expiresAt > Date.now() && given.expiresAt <= Date.now() + 60000, `a minute at most: ${given.expiresAt - Date.now()}`)
  const kept = async digest => (await app.db.prepare('SELECT COUNT(*) AS n FROM challenges WHERE digest=?').bind(digest).first()).n
  assert.deepEqual([await kept(given.challenge), await kept(await sha256(given.challenge))], [0, 1], 'only its digest is kept')
  const headers = { 'x-nexus-challenge': given.challenge, 'x-nexus-signature': await sign(c.key, given.challenge, 'POST', at, body) }
  // CONTROL: the request as signed is taken.
  const { value: first } = await c.native.post(path, value, 200, headers)
  assert.equal(first.approved, false)
  // The very same request again: its challenge is gone.
  assert.equal((await c.native.post(path, value, 401, headers)).value.error, 'invalidChallenge')
  // A challenge another account was given, well signed: refused alike.
  const other = await app.paired(), theirs = (await (await nativeCaller(other)).post('client/challenge')).value.challenge
  assert.equal((await c.native.post(path, value, 401, { 'x-nexus-challenge': theirs,
    'x-nexus-signature': await sign(c.key, theirs, 'POST', at, body) })).value.error, 'invalidChallenge')
  // One past its minute.
  const late = (await challenged()).challenge
  await app.db.prepare('UPDATE challenges SET expires_at=? WHERE digest=?').bind(Date.now() - 1, await sha256(late)).run()
  assert.equal((await c.native.post(path, value, 401, { 'x-nexus-challenge': late,
    'x-nexus-signature': await sign(c.key, late, 'POST', at, body) })).value.error, 'invalidChallenge')
  // A signature over anything but this request exactly, or by anything but the key it names.
  const stranger = await deviceKey()
  for (const [why, signature] of [
    ['another path', challenge => sign(c.key, challenge, 'POST', `/api/remote/stations/${other.stationId}/client/device`, body)],
    ['another body', challenge => sign(c.key, challenge, 'POST', at, JSON.stringify({ ...value, name: 'Another laptop' }))],
    ['another method', challenge => sign(c.key, challenge, 'GET', at, body)],
    ['another label', challenge => sign(c.key, challenge, 'POST', at, body, 'nexus-stream-offer/1')],
    ['another challenge', async () => sign(c.key, (await challenged()).challenge, 'POST', at, body)],
    ['another key', challenge => sign(stranger, challenge, 'POST', at, body)],
  ]) {
    const challenge = (await challenged()).challenge
    assert.equal((await c.native.post(path, value, 401, { 'x-nexus-challenge': challenge, 'x-nexus-signature': await signature(challenge) })).value.error,
      'invalidSignature', why)
    // Tried once: the challenge went with the attempt, so the right signature cannot follow it.
    assert.equal((await c.native.post(path, value, 401, { 'x-nexus-challenge': challenge,
      'x-nexus-signature': await sign(c.key, challenge, 'POST', at, body) })).value.error, 'invalidChallenge', `${why}, then the right one`)
  }
  // Shapes no signed request has.
  const fresh = (await challenged()).challenge, good = await sign(c.key, fresh, 'POST', at, body)
  for (const [why, extra] of [['none', {}], ['no signature', { 'x-nexus-challenge': fresh }], ['no challenge', { 'x-nexus-signature': good }],
    ['upper case', { 'x-nexus-challenge': fresh.toUpperCase(), 'x-nexus-signature': good }], ['a short signature', { 'x-nexus-challenge': fresh, 'x-nexus-signature': good.slice(2) }]]) {
    assert.equal((await c.native.post(path, value, 400, extra)).value.error, 'invalidRequest', why)
  }
  // By value: all of that made exactly the one device the first request asked for.
  const made = await app.db.prepare('SELECT id FROM devices WHERE station_id=? AND kind IS NOT NULL').bind(pair.stationId).all()
  assert.deepEqual(made.results.map(r => r.id), [first.deviceId])
})

// --- Devices --------------------------------------------------------------------------------------

test('a computer waits for approval at the shack, is found again by its key, and is listed only to a Nexus that asks', async () => {
  const pair = await app.paired(), path = `stations/${pair.stationId}`
  const c = await computer(pair, { approve: false, platform: 'macos', name: 'Travel laptop' })
  const pending = await deviceRow(c.deviceId)
  assert.deepEqual({ ...pending, expires_at: 0 }, { approved: 0, approved_at: null, expires_at: 0, generation: 1, kind: 'computer', platform: 'macos',
    public_key: c.key.publicKey })
  assert.ok(pending.expires_at > Date.now() + 590000 && pending.expires_at <= Date.now() + 600000, 'ten minutes to be approved, as a browser has')
  // The same key again finds the same device, and is told it is waiting. Nothing it is sent is a credential.
  const { value: again, response } = await signed(c, `${path}/client/device`, { name: 'Travel laptop', kind: 'computer', platform: 'macos', publicKey: c.key.publicKey })
  assert.deepEqual(again, { deviceId: c.deviceId, approved: false, expiresAt: pending.expires_at, renewsUntil: null })
  assert.equal(response.headers.get('set-cookie'), null)
  // A Nexus that does not ask for kinds is never listed it: it could show a computer only as a browser.
  const listed = async headers => (await pair.native.post(`${path}/native/devices`, {}, 200, headers)).value.devices
  assert.deepEqual((await listed({})).map(d => Object.keys(d).sort()), [['approved', 'expiresAt', 'id', 'name']], 'the paired browser, as 1.12.0 parses it')
  assert.ok(!(await listed({ ...LIFETIME, ...KEYS })).some(d => d.id === c.deviceId))
  // One that asks is listed both, each with its kind and platform.
  const kinds = await listed(KINDS)
  assert.deepEqual(kinds.find(d => d.id === c.deviceId), { id: c.deviceId, name: 'Travel laptop', approved: 0, expiresAt: pending.expires_at,
    kind: 'computer', platform: 'macos', untilRemoved: 0 })
  assert.deepEqual(kinds.filter(d => d.id !== c.deviceId).map(d => [d.kind, d.platform]), [['browser', null]])
  assert.deepEqual(Object.keys((await listed({ ...KINDS, ...LIFETIME, ...KEYS })).find(d => d.id === c.deviceId)).sort(),
    ['approved', 'expiresAt', 'generation', 'id', 'kind', 'name', 'platform', 'publicKey', 'renewsUntil', 'untilRemoved'])
  // Approved at the shack: the computer is told so; its approval lasts until removed.
  await pair.native.post(`${path}/native/approve-device`, { deviceId: c.deviceId }, 200, LIFETIME)
  const approved = await deviceRow(c.deviceId)
  const { value: told } = await signed(c, `${path}/client/device`, { name: 'Travel laptop', kind: 'computer', platform: 'macos', publicKey: c.key.publicKey })
  assert.deepEqual(told, { deviceId: c.deviceId, approved: true, expiresAt: approved.expires_at, renewsUntil: null })
  assert.equal(approved.expires_at, UNTIL_REMOVED, 'until removed')
  assert.equal((await listed(KINDS)).find(d => d.id === c.deviceId).untilRemoved, 1)
  // A phone registers as one.
  const phone = await computer(pair, { approve: false, kind: 'phone', platform: 'ios', name: 'Pocket' })
  assert.deepEqual((await listed(KINDS)).find(d => d.id === phone.deviceId), { id: phone.deviceId, name: 'Pocket', approved: 0,
    expiresAt: (await deviceRow(phone.deviceId)).expires_at, kind: 'phone', platform: 'ios', untilRemoved: 0 })
  // Kinds and platforms that do not go together, or do not exist, are refused, and nothing is made.
  const before = (await app.db.prepare('SELECT COUNT(*) AS n FROM devices WHERE station_id=?').bind(pair.stationId).first()).n
  const key = await deviceKey(), probe = { stationId: pair.stationId, native: c.native, key }
  for (const [kind, platform] of [['phone', 'windows'], ['computer', 'ios'], ['tablet', 'android'], ['browser', 'linux'], ['computer', null], [1, 'linux'],
    ['computer', 'Windows'], ['constructor', 'windows']]) {
    assert.equal((await signed(probe, `${path}/client/device`, { name: 'Odd one', kind, platform, publicKey: key.publicKey }, 400)).value.error,
      'invalidRequest', `${kind} on ${platform}`)
  }
  for (const publicKey of [key.publicKey.toUpperCase(), key.publicKey.slice(0, -2), `${key.publicKey.slice(0, 54)}04${'01'.repeat(64)}`]) {
    assert.equal((await signed(probe, `${path}/client/device`, { name: 'Odd one', kind: 'computer', platform: 'linux', publicKey }, 400)).value.error, 'invalidRequest')
  }
  assert.equal((await app.db.prepare('SELECT COUNT(*) AS n FROM devices WHERE station_id=?').bind(pair.stationId).first()).n, before)
})

test('eight devices a station, computers and browsers together, and a station that is not the account\'s takes none', async () => {
  const pair = await app.paired(), path = `stations/${pair.stationId}`
  // The paired browser is one; seven computers make eight.
  for (let n = 0; n < 7; n++) await computer(pair, { approve: false, name: `Laptop ${n}` })
  const ninth = { stationId: pair.stationId, native: await nativeCaller(pair), key: await deviceKey() }
  assert.equal((await signed(ninth, `${path}/client/device`, { name: 'One too many', kind: 'computer', platform: 'linux', publicKey: ninth.key.publicKey }, 409)).value.error,
    'deviceLimit')
  assert.equal((await pair.browser.post(`${path}/device`, { name: 'A ninth browser' }, 409)).value.error, 'deviceLimit', 'a browser counts the computers too')
  // Another account's station, and a revoked one, are not this account's to register on.
  const other = await app.paired(), stray = { stationId: other.stationId, native: ninth.native, key: ninth.key }
  assert.equal((await signed(stray, `stations/${other.stationId}/client/device`, { name: 'Stray', kind: 'computer', platform: 'linux', publicKey: ninth.key.publicKey }, 404)).value.error,
    'stationUnavailable')
})

test('revoked as a browser is: at the shack, or by the computer itself; either way it must be approved again', async () => {
  const pair = await app.paired(), path = `stations/${pair.stationId}`
  const c = await computer(pair)
  // CONTROL: an approved computer's signed request is taken (no pipe is open, which is what renew says).
  assert.equal((await signed(c, `${path}/client/renew`, {}, 404)).value.error, 'pipeClosed')
  await pair.native.post(`${path}/native/revoke-device`, { deviceId: c.deviceId })
  assert.equal((await signed(c, `${path}/client/renew`, {}, 403)).value.error, 'deviceNotApproved')
  // Registering the same key again is a new device, waiting for the shack, never the old approval back.
  const { value: again } = await signed(c, `${path}/client/device`, { name: 'Shack laptop', kind: 'computer', platform: 'windows', publicKey: c.key.publicKey })
  assert.notEqual(again.deviceId, c.deviceId)
  assert.equal(again.approved, false)
  // The computer forgets itself: its approval ends now, its generation moves, and only its own key can ask.
  const own = await computer(pair)
  const before = await deviceRow(own.deviceId)
  assert.equal((await signed(own, `${path}/client/forget`, {}, 401, { key: c.key })).value.error, 'invalidSignature', 'another device\'s key')
  await signed(own, `${path}/client/forget`)
  const forgotten = await deviceRow(own.deviceId)
  assert.equal(forgotten.approved, 0)
  assert.equal(forgotten.generation, before.generation + 1)
  assert.ok(forgotten.expires_at <= Date.now())
  assert.equal((await signed(own, `${path}/client/renew`, {}, 403)).value.error, 'deviceNotApproved')
  // A browser's approval is never a computer's, not even signed with that browser's own key.
  const browserKey = await deviceKey()
  const { value: keyed } = await pair.browser.post(`${path}/device`, { name: 'Keyed browser', publicKey: browserKey.publicKey })
  await pair.native.post(`${path}/native/approve-device`, { deviceId: keyed.deviceId }, 200, LIFETIME)
  assert.equal((await signed(c, `${path}/client/renew`, {}, 403, { device: keyed.deviceId, key: browserKey })).value.error, 'deviceNotApproved')
})

test('the station key over the signed-in API: the account\'s own stations, each with the key it recorded', async () => {
  const pair = await app.paired(), native = await nativeCaller(pair)
  const station = async () => (await native.post('client/session')).value.stations
  // A station paired by code has no LAN key or station proof listed (test/account.test.mjs binds one that has).
  const listed = stationKey => [{ id: pair.stationId, name: 'Synthetic test station', stationKey, lanKey: null, proof: null, role: 'owner' }]
  assert.deepEqual(await station(), listed(null), 'none recorded yet')
  const key = await deviceKey()
  await pair.native.post(`stations/${pair.stationId}/native/key`, { publicKey: key.publicKey })
  assert.deepEqual(await station(), listed(key.publicKey))
  // Another account's station is never listed, and a revoked one goes from the list.
  await app.paired()
  await pair.browser.post(`stations/${pair.stationId}/revoke`)
  assert.deepEqual(await station(), [])
  const { value } = await native.post('client/session')
  assert.deepEqual(Object.keys(value).sort(), ['accountId', 'entitlement', 'identityVerified', 'serverNow', 'stations'])
  assert.equal(value.entitlement.state, 'active')
})

test('TURN credentials go to an approved, entitled computer alone, as a browser\'s stream gets them', async () => {
  const keyId = hex(crypto.getRandomValues(new Uint8Array(16))), token = hex(crypto.getRandomValues(new Uint8Array(32))), asked = []
  const answer = { iceServers: [{ urls: ['stun:stun.cloudflare.com:3478'] },
    { urls: ['turn:turn.cloudflare.com:3478?transport=udp', 'turn:turn.cloudflare.com:53?transport=udp'], username: 'u'.repeat(48), credential: 'p'.repeat(48) }] }
  const relayed = await runtime({ bindings: { AUTH0_NATIVE_CLIENT_ID: NATIVE, TURN_KEY_ID: keyId, TURN_KEY_TOKEN: token },
    relay: async request => { asked.push(await request.json()); return Response.json(answer) } })
  const saved = app
  try {
    app = relayed
    const pair = await relayed.paired(), path = `stations/${pair.stationId}/client/turn`
    const waiting = await computer(pair, { approve: false })
    assert.equal((await signed(waiting, path, {}, 403)).value.error, 'deviceNotApproved')
    assert.equal(asked.length, 0, 'a refusal never mints a credential')
    const c = await computer(pair)
    const { value, response } = await signed(c, path)
    assert.deepEqual(value, { iceServers: [{ urls: ['turn:turn.cloudflare.com:3478?transport=udp'], username: 'u'.repeat(48), credential: 'p'.repeat(48) }] })
    assert.equal(response.headers.get('cache-control'), 'no-store')
    assert.ok(asked[0].ttl > 3500 && asked[0].ttl <= 3600, `ends with the entitlement: ${asked[0].ttl}`)
    assert.ok(!JSON.stringify(value).includes(token) && !JSON.stringify(value).includes(keyId))
    // The page (its Origin and its token), or the computer without its key's signature, gets nothing.
    assert.equal((await pair.browser.post(path, {}, 403)).value.error, 'originDenied')
    assert.equal((await c.native.post(path, {}, 400, { 'x-nexus-device-id': c.deviceId })).value.error, 'invalidRequest')
    // The account's six an hour (ruling 1406) are the computer's and its browsers' together: five more
    // here, one from the page, and the computer's next is refused before the provider is asked.
    for (let ask = 0; ask < 4; ask++) await signed(c, path)
    await relayed.approved(pair)
    await pair.browser.post(`stations/${pair.stationId}/turn`)
    assert.equal((await signed(c, path, {}, 429)).value.error, 'tryLater')
    assert.equal(asked.length, 6)
    await relayed.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now() - 1000, pair.browser.accountId).run()
    assert.equal((await signed(c, path, {}, 403)).value.error, 'trialRequired')
    assert.equal(asked.length, 6)
  } finally { app = saved; await relayed.mf.dispose() }
})

// --- The room's pipe ------------------------------------------------------------------------------

test('the pipe opens only once the station has opened its end, then passes binary both ways unread', async () => {
  const c = await computer()
  await app.db.prepare('UPDATE devices SET expires_at=? WHERE id=?').bind(Date.now() + DAY, c.deviceId).run()
  const station = await c.pair.native.open(c.stationId, null, 101, PIPE)
  await station.take(type('watch'))
  c.station = station
  const opening = openPipe(c)
  const asked = await station.take(type('pipeOpen'))
  assert.deepEqual(Object.keys(asked).sort(), ['pipeId', 'type'], 'the one message the main socket gains, and only its id')
  // The computer's socket is still waiting: it opens when the station's end does, not before.
  const raced = await Promise.race([opening.then(() => 'opened'), delay(300).then(() => 'waiting')])
  assert.equal(raced, 'waiting')
  const end = await stationEnd(c.pair, asked.pipeId)
  assert.equal(end.status, 101)
  const client = await opening
  assert.equal(client.status, 101)
  assert.equal(await pipesOf(c.stationId), 1)
  assert.equal((await deviceRow(c.deviceId)).expires_at, UNTIL_REMOVED, 'opening the station moves no approval: until removed')
  // Each message, as sent, in order, either way, and nothing else on either end.
  const out = [record(1, 2, 3), record(4), Buffer.alloc(16384, 7), record(5, 6)]
  for (const message of out) client.peer.ws.send(message)
  for (const message of out) assert.deepEqual(await end.peer.take(binary), message)
  const back = [record(9, 9), Buffer.alloc(65536, 1)]
  for (const message of back) end.peer.ws.send(message)
  for (const message of back) assert.deepEqual(await client.peer.take(binary), message)
  // The main socket saw nothing of the pipe's bytes.
  await assert.rejects(station.take(value => value?.type !== 'watch', 200), /timeout/)
  // Closing one end closes the other, and the room lets the pipe go.
  client.peer.close()
  assert.equal((await end.peer.take(type('closed'))).code, 1000)
  assert.equal(end.peer.closeReason, 'pipeClosed')
  assert.equal(await pipesOf(c.stationId), 0)
  station.close()
})

test('a station that never advertised the pipe is never asked for one, and its main socket stays exactly as it was', async () => {
  const c = await computer()
  // No station at all.
  assert.deepEqual(await openPipe(c), { status: 409, error: 'stationOffline' })
  const station = await c.pair.native.open(c.stationId)
  assert.deepEqual(await station.take(() => true), { type: 'watch', enabled: false })
  assert.deepEqual(await openPipe(c), { status: 409, error: 'pipeUnsupported' })
  // Nothing at all reached it, and it is still up.
  await assert.rejects(station.take(() => true, 300), /timeout/)
  assert.equal(station.ws.readyState, 1)
  station.close()
  // CONTROL: the same computer and station, advertising it, are asked.
  const advertised = await c.pair.native.open(c.stationId, null, 101, PIPE)
  const opening = openPipe(c)
  const asked = await advertised.take(type('pipeOpen'))
  assert.equal((await stationEnd(c.pair, asked.pipeId)).status, 101)
  assert.equal((await opening).status, 101)
  advertised.close()
})

test('only an approved computer, signed for its own key, opens a pipe; and only the station opens the other end', async () => {
  const c = await computer()
  c.station = await c.pair.native.open(c.stationId, null, 101, PIPE)
  const waiting = await computer(c.pair, { approve: false })
  waiting.station = c.station
  assert.deepEqual(await openPipe(waiting), { status: 403, error: 'deviceNotApproved' })
  assert.deepEqual(await openPipe(c, { key: waiting.key }), { status: 401, error: 'invalidSignature' })
  // The page's token, with no Origin (a browser cannot send one): refused before any key is read.
  const { value: { challenge } } = await c.native.post('client/challenge')
  const asPage = await upgrade(`stations/${c.stationId}/client/pipe`, { authorization: `Bearer ${c.pair.browser.jwt}`, 'x-nexus-device-id': c.deviceId,
    'x-nexus-challenge': challenge, 'x-nexus-signature': await sign(c.key, challenge, 'GET', `/api/remote/stations/${c.stationId}/client/pipe`, '') })
  assert.deepEqual(asPage, { status: 401, error: 'signInRequired' })
  // A page's WebSocket always carries its Origin.
  const { value: { challenge: second } } = await c.native.post('client/challenge')
  assert.deepEqual(await upgrade(`stations/${c.stationId}/client/pipe`, { ...c.native.headers(), origin: app.origin, 'x-nexus-device-id': c.deviceId,
    'x-nexus-challenge': second, 'x-nexus-signature': await sign(c.key, second, 'GET', `/api/remote/stations/${c.stationId}/client/pipe`, '') }),
  { status: 403, error: 'originDenied' })
  assert.equal(await pipesOf(c.stationId), 0)
  // The station's end: only with the station's credential, and only for a pipe the room is holding open.
  const opening = openPipe(c), asked = await c.station.take(type('pipeOpen'))
  const wrong = { authorization: `Bearer ${hex(crypto.getRandomValues(new Uint8Array(32)))}` }
  assert.deepEqual(await stationEnd(c.pair, asked.pipeId, wrong), { status: 401, error: 'stationNotApproved' })
  assert.deepEqual(await stationEnd(c.pair, asked.pipeId, c.native.headers()), { status: 400, error: 'invalidRequest' }, 'a token is no station credential')
  assert.deepEqual(await stationEnd(c.pair, crypto.randomUUID()), { status: 404, error: 'pipeUnavailable' })
  assert.equal((await stationEnd(c.pair, asked.pipeId)).status, 101)
  assert.equal((await opening).status, 101)
  assert.deepEqual(await stationEnd(c.pair, asked.pipeId), { status: 404, error: 'pipeUnavailable' }, 'a pipe has one station end')
  c.station.close()
})

test('a pipe whose station never opens its end is refused within its bound, and a late end is turned away', async () => {
  const c = await computer()
  const station = await c.pair.native.open(c.stationId, null, 101, PIPE)
  c.station = station
  const started = Date.now(), opening = openPipe(c), asked = await station.take(type('pipeOpen'))
  assert.deepEqual(await opening, { status: 503, error: 'stationUnavailable' })
  const waited = Date.now() - started
  assert.ok(waited >= 9500 && waited < 13000, `refused after ${waited} ms`)
  assert.deepEqual(await stationEnd(c.pair, asked.pipeId), { status: 404, error: 'pipeUnavailable' })
  assert.equal(await pipesOf(c.stationId), 0)
  station.close()
})

test('a message the pipe does not carry closes it at both ends: text, too large, or the station speaking first', async () => {
  for (const [why, send] of [['text', ends => ends.client.ws.send('{"type":"hello"}')],
    ['one byte over', ends => ends.client.ws.send(Buffer.alloc(65537, 1))],
    ['text from the station', ends => ends.end.ws.send('hello')]]) {
    const c = await computer(), ends = await piped(c)
    send(ends)
    assert.equal((await ends.client.take(type('closed'))).code, 1008, why)
    assert.equal((await ends.end.take(type('closed'))).code, 1008, why)
    assert.equal(ends.client.closeReason, 'invalidMessage', why)
    c.station.close()
  }
})

test('a revoke at the shack, the station revoked, or a device approved again, closes the pipe at once', async () => {
  for (const [why, end] of [
    ['the device revoked', c => c.pair.native.post(`stations/${c.stationId}/native/revoke-device`, { deviceId: c.deviceId })],
    ['the device approved again', c => c.pair.native.post(`stations/${c.stationId}/native/approve-device`, { deviceId: c.deviceId }, 200, LIFETIME)],
    ['the computer forgetting itself', c => signed(c, `stations/${c.stationId}/client/forget`)],
    ['the station revoked', c => c.pair.browser.post(`stations/${c.stationId}/revoke`)],
  ]) {
    const c = await computer(), ends = await piped(c)
    await end(c)
    assert.equal((await ends.client.take(type('closed'))).code, 1008, why)
    assert.equal((await ends.end.take(type('closed'))).code, 1008, why)
    assert.equal(ends.client.closeReason, 'accessEnded', why)
    assert.equal(await pipesOf(c.stationId), 0, why)
    c.station.close()
  }
})

test('Remote access switched off by hand ends a pipe within about two seconds of traffic, as it ends a stream', async () => {
  const c = await computer(), ends = await piped(c)
  // CONTROL: traffic flows while the entitlement holds.
  ends.client.ws.send(record(1)); assert.deepEqual(await ends.end.take(binary), record(1))
  await app.db.prepare('UPDATE trials SET enabled=0 WHERE account_id=?').bind(c.pair.browser.accountId).run()
  const switched = Date.now()
  let closed = null
  for (let n = 0; n < 30 && !closed; n++) {
    if (ends.client.ws.readyState === 1) ends.client.ws.send(record(n))
    closed = await ends.client.take(type('closed'), 100).catch(() => null)
  }
  const after = Date.now() - switched
  assert.ok(closed, 'the pipe was still open 3 s after access was switched off')
  assert.equal(closed.code, 1008)
  assert.equal(ends.client.closeReason, 'accessEnded')
  assert.ok(after < 2500, `ended ${after} ms after access was switched off`)
  c.station.close()
})

test('a pipe lasts its lease: it ends with its approval or its entitlement unless renewed, and renewal is use', async () => {
  // Approved by a Nexus that binds its grants to the expiry, so nothing renews this approval.
  const ending = await computer(null, { lifetime: false })
  await app.db.prepare('UPDATE devices SET expires_at=? WHERE id=?').bind(Date.now() + 3000, ending.deviceId).run()
  const short = await piped(ending), opened = Date.now()
  const closed = await short.client.take(type('closed'), 8000)
  assert.equal(closed.code, 1008)
  assert.equal(short.client.closeReason, 'accessEnded')
  const lasted = Date.now() - opened
  assert.ok(lasted >= 2000 && lasted < 6000, `ended ${lasted} ms after it opened, with its approval`)
  ending.station.close()
  // An entitlement four seconds from its end: renewed once it is extended, the pipe outlives it.
  const c = await computer()
  await app.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now() + 4000, c.pair.browser.accountId).run()
  const ends = await piped(c)
  await app.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now() + 3600000, c.pair.browser.accountId).run()
  // An approval given under the thirty/ninety-day rule lasts until removed from its next use.
  await app.db.prepare('UPDATE devices SET expires_at=? WHERE id=?').bind(Date.now() + DAY, c.deviceId).run()
  const { value } = await signed(c, `stations/${c.stationId}/client/renew`)
  assert.deepEqual(Object.keys(value).sort(), ['ok', 'serverNow'])
  assert.equal((await deviceRow(c.deviceId)).expires_at, UNTIL_REMOVED, 'kept by use')
  await signed(c, `stations/${c.stationId}/client/renew`)
  assert.equal((await deviceRow(c.deviceId)).expires_at, UNTIL_REMOVED, 'and use moves it no further')
  await delay(5500)
  ends.client.ws.send(record(4, 2))
  assert.deepEqual(await ends.end.take(binary), record(4, 2), 'renewed, it outlived the entitlement it opened under')
  // A renewal for a device with no pipe says so.
  ends.client.close()
  await ends.end.take(type('closed'))
  assert.equal((await signed(c, `stations/${c.stationId}/client/renew`, {}, 404)).value.error, 'pipeClosed')
  c.station.close()
})

test('a pipe nobody renews ends with its sixty-second lease, as a browser session does', { timeout: 120000 }, async () => {
  const c = await computer(), ends = await piped(c), opened = Date.now()
  const closed = await ends.client.take(type('closed'), 70000)
  const lasted = Date.now() - opened
  assert.equal(closed.code, 1008)
  assert.equal(ends.client.closeReason, 'accessEnded')
  assert.ok(lasted >= 59000 && lasted < 66000, `ended ${lasted} ms after it opened`)
  c.station.close()
})

test('one pipe a device and four a station: a device\'s new pipe replaces its old one', async () => {
  const c = await computer(), first = await piped(c)
  const second = await piped(c)
  assert.equal((await first.client.take(type('closed'))).code, 1000)
  assert.equal(first.client.closeReason, 'replaced')
  assert.equal((await first.end.take(type('closed'))).code, 1000)
  second.client.ws.send(record(2)); assert.deepEqual(await second.end.take(binary), record(2), 'the new one carries')
  for (let n = 0; n < 3; n++) {
    const other = await computer(c.pair)
    other.station = c.station
    await piped(other)
  }
  assert.equal(await pipesOf(c.stationId), 4)
  const fifth = await computer(c.pair)
  fifth.station = c.station
  assert.deepEqual(await openPipe(fifth), { status: 409, error: 'pipeLimit' })
  await assert.rejects(c.station.take(type('pipeOpen'), 300), /timeout/, 'the station was not asked for a fifth')
  c.station.close()
})

test('a pipe survives a hibernation of the room: both ends come back paired, and it is still held to its rules', async () => {
  const c = await computer(), ends = await piped(c)
  await app.evict(c.stationId)
  ends.client.ws.send(record(1, 1)); assert.deepEqual(await ends.end.take(binary), record(1, 1))
  ends.end.ws.send(record(2, 2)); assert.deepEqual(await ends.client.take(binary), record(2, 2))
  // Woken, the room renews its lease as before.
  await signed(c, `stations/${c.stationId}/client/renew`)
  // And woken again, it still ends it on a revoke. (The room's status is read only after its last
  // eviction: a status read holds the room, which can then not be evicted.)
  await app.evict(c.stationId)
  await c.pair.native.post(`stations/${c.stationId}/native/revoke-device`, { deviceId: c.deviceId })
  assert.equal((await ends.client.take(type('closed'))).code, 1008)
  assert.equal(ends.client.closeReason, 'accessEnded')
  assert.equal((await ends.end.take(type('closed'))).code, 1008)
  const status = await roomStatus(await room(c.stationId))
  assert.equal(status.online, true, 'the station itself stays connected')
  assert.equal(await pipesOf(c.stationId), 0)
  c.station.close()
})

// --- Reopening with the device's key alone ------------------------------------------------------

/** A request with no account token, no Origin and no cookie: the computer reopening its station by its
 *  key, signed over a challenge its station gave the device for that. */
async function reopened(c, verb, value = {}, expected = 200, { key = c.key, device = c.deviceId } = {}) {
  const bare = app.client(null, '', null), path = `stations/${c.stationId}/client`
  const { value: { challenge } } = await bare.post(`${path}/challenge`, {}, 200, { 'x-nexus-device-id': device })
  return bare.post(`${path}/${verb}`, value, expected, { 'x-nexus-device-id': device, 'x-nexus-challenge': challenge,
    'x-nexus-signature': await sign(key, challenge, 'POST', `/api/remote/${path}/${verb}`, JSON.stringify(value)) })
}

test('reopening: an approved computer opens its station again by its key alone, and its pipe with it, with no token', async () => {
  const c = await computer()
  // Past every check a token's request passes: no pipe is open, which is what renew says.
  assert.equal((await reopened(c, 'renew', {}, 404)).value.error, 'pipeClosed')
  // TURN: past the device, the trial and the approval, to the relay this runtime has none of.
  assert.equal((await reopened(c, 'turn', {}, 503)).value.error, 'relayNotConfigured')
  // The pipe itself, signed as a GET over an empty body, with no token.
  const station = await c.pair.native.open(c.stationId, null, 101, PIPE)
  const bare = app.client(null, '', null), path = `stations/${c.stationId}/client`
  const { value: { challenge } } = await bare.post(`${path}/challenge`, {}, 200, { 'x-nexus-device-id': c.deviceId })
  const opening = upgrade(`${path}/pipe`, { ...bare.headers(), 'x-nexus-device-id': c.deviceId, 'x-nexus-challenge': challenge,
    'x-nexus-signature': await sign(c.key, challenge, 'GET', `/api/remote/${path}/pipe`, '') })
  const asked = await station.take(type('pipeOpen'))
  const end = await stationEnd(c.pair, asked.pipeId), client = await opening
  assert.equal(client.status, 101, 'the computer\'s end opens on its key alone')
  client.peer.ws.send(record(0x16)); assert.deepEqual(await end.peer.take(binary), record(0x16))
  client.peer.ws.close(); station.close()
  // It forgets itself the same way, and then reopens nothing.
  await reopened(c, 'forget')
  assert.equal((await bare.post(`${path}/challenge`, {}, 401, { 'x-nexus-device-id': c.deviceId })).value.error, 'signInRequired')
})

test('reopening opens nothing else: no new device, no stations, no waiting or removed device, no other key, no page', async () => {
  const pair = await app.paired(), path = `stations/${pair.stationId}/client`
  const c = await computer(pair), bare = app.client(null, '', null)
  // CONTROL: the same computer reopens.
  assert.equal((await reopened(c, 'renew', {}, 404)).value.error, 'pipeClosed')
  // A signed-in computer asks its challenge where it always has: the station's is reopening's alone.
  assert.equal((await (await nativeCaller(pair)).post(`${path}/challenge`, {}, 404)).value.error, 'notFound')
  // Registering a device, the account's stations and an unbound challenge all need the token.
  assert.equal((await reopened(c, 'device', { name: 'Other', kind: 'computer', platform: 'windows', publicKey: (await deviceKey()).publicKey }, 401)).value.error, 'signInRequired')
  for (const route of ['client/challenge', 'client/session']) {
    assert.equal((await bare.post(route, {}, 401, { 'x-nexus-device-id': c.deviceId })).value.error, 'signInRequired', route)
  }
  // A device waiting for the shack, one removed there, a browser, an unknown id or station: one answer.
  const waiting = await computer(pair, { approve: false })
  const removed = await computer(pair)
  await pair.native.post(`stations/${pair.stationId}/native/revoke-device`, { deviceId: removed.deviceId })
  const { value: keyed } = await pair.browser.post(`stations/${pair.stationId}/device`, { name: 'A browser', publicKey: (await deviceKey()).publicKey })
  await pair.native.post(`stations/${pair.stationId}/native/approve-device`, { deviceId: keyed.deviceId }, 200, LIFETIME)
  for (const [why, device, at] of [['waiting', waiting.deviceId, path], ['removed', removed.deviceId, path],
    ['a browser', keyed.deviceId, path], ['unknown', crypto.randomUUID(), path], ['no id', '', path],
    ['another station', c.deviceId, `stations/${crypto.randomUUID()}/client`]]) {
    const headers = device ? { 'x-nexus-device-id': device } : {}
    assert.equal((await bare.post(`${at}/challenge`, {}, 401, headers)).value.error, 'signInRequired', why)
  }
  // Another device's key over this device's challenge.
  assert.equal((await reopened(c, 'renew', {}, 401, { key: (await deviceKey()) })).value.error, 'invalidSignature')
  // A page (its Origin, or a cookie) is refused before anything is read.
  assert.equal((await bare.post(`${path}/challenge`, {}, 403, { origin: app.origin, 'x-nexus-device-id': c.deviceId })).value.error, 'originDenied')
  assert.equal((await bare.post(`${path}/challenge`, {}, 403, { cookie: 'a=b', 'x-nexus-device-id': c.deviceId })).value.error, 'originDenied')
  // An entitlement that ended ends reopening's pipe and relay as it ends a token's.
  await app.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now() - 1000, pair.browser.accountId).run()
  assert.equal((await reopened(c, 'turn', {}, 403)).value.error, 'trialRequired')
})

/** A request with no token, sent as anyone who knows a device id may send one: its status alone. */
async function unproven(path, device, address, headers = {}) {
  const response = await app.mf.dispatchFetch(`${app.origin}/api/remote/${path}`, { method: 'POST', body: '{}',
    headers: { 'content-type': 'application/json', 'cf-connecting-ip': address, 'x-nexus-device-id': device, ...headers } })
  await response.arrayBuffer()
  return response.status
}
/** Room for a few hundred requests before the minute's buckets roll over under a test. */
async function freshMinute() {
  if (Date.now() % 60000 > 40000) await delay(60000 - Date.now() % 60000 + 50)
}
// Each its own /48, never the one the per-network test below spends.
const elsewhere = n => `2001:db8:${n.toString(16)}::1`

test('a flood of unproven reopening is never charged to the account: its signed-in and proven requests still pass', async () => {
  await freshMinute()
  const pair = await app.paired(), path = `stations/${pair.stationId}/client`
  const named = [await computer(pair), await computer(pair)], proven = await computer(pair)
  // Seventy for each id it knows, each from a network of its own, half of them asking a challenge and
  // half signed by nobody: none proves the device, so none may spend what the account's own requests need.
  let n = 0
  for (const c of named) {
    for (let i = 0; i < 70; i++) {
      await unproven(`${path}/${i % 2 ? 'renew' : 'challenge'}`, c.deviceId, elsewhere(++n),
        i % 2 ? { 'x-nexus-challenge': '0'.repeat(64), 'x-nexus-signature': '0'.repeat(128) } : {})
    }
  }
  // The account signed in, a device of it proving its key, and a device the flood named, all still pass.
  assert.equal((await (await nativeCaller(pair)).post('client/session')).value.stations[0].id, pair.stationId)
  assert.equal((await reopened(proven, 'renew', {}, 404)).value.error, 'pipeClosed')
  assert.equal((await reopened(named[0], 'renew', {}, 404)).value.error, 'pipeClosed')
})

test('unproven reopening is held per address, per network and per device, each before the next is spent', async () => {
  await freshMinute()
  const pair = await app.paired(), path = `stations/${pair.stationId}/client/challenge`
  const c = await computer(pair), other = await computer(pair)
  const statuses = async (count, device, address) => {
    const seen = []
    for (let i = 0; i < count; i++) seen.push(await unproven(path, device, typeof address === 'function' ? address(i) : address))
    return seen
  }
  const tally = seen => ({ ok: seen.filter(s => s === 200).length, held: seen.filter(s => s === 429).length })
  // One address: thirty a minute, then held, and the ones held never reach the device's own bucket.
  assert.deepEqual(tally(await statuses(100, c.deviceId, '192.0.2.7')), { ok: 30, held: 70 })
  // One /64 is one caller, whatever address in it is used.
  assert.deepEqual(tally(await statuses(40, other.deviceId, i => `2001:db8:fffe:1::${(i + 1).toString(16)}`)), { ok: 30, held: 10 })
  // One /48 is sixty, across its /64s: thirty already spent above, thirty more from another /64.
  assert.deepEqual(tally(await statuses(40, other.deviceId, i => `2001:db8:fffe:2::${(i + 1).toString(16)}`)), { ok: 30, held: 10 })
  // ...and a third /64 of that /48 has nothing left, though its own bucket is untouched.
  assert.deepEqual(tally(await statuses(5, other.deviceId, i => `2001:db8:fffe:3::${(i + 1).toString(16)}`)), { ok: 0, held: 5 })
  // The device: a hundred and twenty a minute from everywhere together. Thirty spent from the one
  // address (not a hundred), so ninety more from networks of their own pass, and the next is held.
  assert.deepEqual(tally(await statuses(91, c.deviceId, i => elsewhere(0x1000 + i))), { ok: 90, held: 1 })
  // Another device is its own, and the address held for c is held for it too.
  assert.equal(await unproven(path, other.deviceId, elsewhere(0x2000)), 200)
  assert.equal(await unproven(path, other.deviceId, '192.0.2.7'), 429)
})

test('a reopening proven by its key is charged to its account, as a signed-in request is', async () => {
  await freshMinute()
  const pair = await app.paired(), devices = [await computer(pair), await computer(pair), await computer(pair)]
  // Registering the three spent six of the account's minute (a challenge and the device, each signed
  // in), so 114 proven renewals fill it.
  for (let i = 0; i < 114; i++) assert.equal((await reopened(devices[i % 3], 'renew', {}, 404)).value.error, 'pipeClosed', `renewal ${i}`)
  assert.equal((await reopened(devices[0], 'renew', {}, 429)).value.error, 'tryLater')
  assert.equal((await (await nativeCaller(pair)).post('client/session', {}, 429)).value.error, 'tryLater')
})

test('a reopening challenge is its device\'s alone, a few at a time, and never a signed-in request\'s', async () => {
  const pair = await app.paired(), path = `stations/${pair.stationId}/client`, bare = app.client(null, '', null)
  const a = await computer(pair), b = await computer(pair)
  const ask = async device => (await bare.post(`${path}/challenge`, {}, 200, { 'x-nexus-device-id': device })).value.challenge
  const renew = async (c, challenge, expected) => bare.post(`${path}/renew`, {}, expected, { 'x-nexus-device-id': c.deviceId,
    'x-nexus-challenge': challenge, 'x-nexus-signature': await sign(c.key, challenge, 'POST', `/api/remote/${path}/renew`, '{}') })
  // b, signing for itself over a challenge a was given: refused, and a's challenge is still a's.
  const forA = await ask(a.deviceId)
  assert.equal((await renew(b, forA, 401)).value.error, 'invalidChallenge')
  assert.equal((await renew(a, forA, 404)).value.error, 'pipeClosed', 'CONTROL: the challenge, used by its own device')
  // A signed-in computer's challenge is no reopening's, and a reopening's is no signed-in request's.
  const signedIn = (await a.native.post('client/challenge')).value.challenge
  assert.equal((await renew(a, signedIn, 401)).value.error, 'invalidChallenge')
  const reopening = await ask(a.deviceId), key = await deviceKey(), value = { name: 'New', kind: 'computer', platform: 'linux', publicKey: key.publicKey }
  assert.equal((await a.native.post(`${path}/device`, value, 401, { 'x-nexus-challenge': reopening,
    'x-nexus-signature': await sign(key, reopening, 'POST', `/api/remote/${path}/device`, JSON.stringify(value)) })).value.error, 'invalidChallenge')
  // Four outstanding a device: asking a fifth gives up the oldest, so whoever asks can never hold more.
  const asked = []
  for (let i = 0; i < 6; i++) asked.push(await ask(a.deviceId))
  assert.equal((await renew(a, asked[0], 401)).value.error, 'invalidChallenge', 'the oldest was given up')
  assert.equal((await renew(a, asked[1], 401)).value.error, 'invalidChallenge', 'so was the next')
  for (const challenge of asked.slice(2)) assert.equal((await renew(a, challenge, 404)).value.error, 'pipeClosed')
})

test('a flood of reopening challenges from elsewhere never gives up the one a device asked from its own address', async () => {
  const pair = await app.paired(), path = `stations/${pair.stationId}/client`, bare = app.client(null, '', null)
  const a = await computer(pair)
  const ask = async address => (await bare.post(`${path}/challenge`, {}, 200, { 'x-nexus-device-id': a.deviceId, 'cf-connecting-ip': address })).value.challenge
  const renew = async (challenge, expected) => bare.post(`${path}/renew`, {}, expected, { 'x-nexus-device-id': a.deviceId,
    'cf-connecting-ip': '198.51.100.20', 'x-nexus-challenge': challenge,
    'x-nexus-signature': await sign(a.key, challenge, 'POST', `/api/remote/${path}/renew`, '{}') })
  const own = await ask('198.51.100.20')
  // Ten asks in this device's name from one /64, each from an address of its own.
  const flood = []
  for (let i = 0; i < 10; i++) flood.push(await ask(`2001:db8:abcd:1::${(i + 1).toString(16)}`))
  assert.equal((await renew(own, 404)).value.error, 'pipeClosed', 'the device\'s own challenge outlived the flood')
  // That /64 is one requester: it still holds only its newest four.
  for (const challenge of flood.slice(0, 6)) assert.equal((await renew(challenge, 401)).value.error, 'invalidChallenge')
  for (const challenge of flood.slice(6)) assert.equal((await renew(challenge, 404)).value.error, 'pipeClosed')
})

test('the service names its Native application for Nexus to sign in to, and never the page\'s', async () => {
  const config = async service => (await service.mf.dispatchFetch(`${service.origin}/api/remote/config`)).json()
  assert.equal((await config(app)).nativeClientId, NATIVE)
  for (const bindings of [{}, { AUTH0_NATIVE_CLIENT_ID: 'unconfigured' }, { AUTH0_NATIVE_CLIENT_ID: 'remote-test-client' }]) {
    const service = await runtime({ bindings })
    try {
      const said = await config(service)
      assert.equal(said.nativeClientId, null, JSON.stringify(bindings))
      assert.equal(said.clientId, 'remote-test-client')
    } finally { await service.mf.dispose() }
  }
})
