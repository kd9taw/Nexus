import { before, after, test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { generateKeyPair } from 'jose'
import { runtime, roomStatus } from './runtime.mjs'

let app
before(async () => { app = await runtime() })
after(async () => { await app?.mf.dispose() })
const fixtures = JSON.parse(await readFile(new URL('../../ui/src/remote-monitor/fixtures.v2.json', import.meta.url), 'utf8'))
const sample = () => {
  const frame = structuredClone(fixtures.spe)
  frame.source = 'native'
  return frame
}
const type = name => value => value?.type === name
async function admitted(pair) {
  const deviceId = await app.approved(pair)
  const station = await pair.native.open(pair.stationId)
  await station.take(value => value.type === 'watch' && value.enabled === false)
  const { value: ticket } = await pair.browser.post(`stations/${pair.stationId}/ticket`)
  const browser = await pair.browser.open(pair.stationId, ticket.ticket)
  const session = await browser.take(type('session'))
  return { station, browser, deviceId, session, ticket }
}
async function publish(station, sequence) {
  const request = await station.take(value => value.type === 'watch' && value.enabled)
  const frame = sample(); frame.sequence = sequence
  station.send({ type: 'publication', requestId: request.requestId, frame })
  return frame
}

test('provider signatures, authorized client and exact Origin are required; account creation grants no trial', async () => {
  const account = await app.owner(false)
  assert.ok(app.jwksReads() > 0, 'positive control: the real JOSE verifier fetched its pinned key set')
  await account.post('session', {}, 403, { origin: 'https://other.invalid' })
  const wrongClient = app.client(await app.token('synthetic|wrong-client', { azp: 'another-client' }))
  await wrongClient.post('session', {}, 401)
  const { privateKey } = await generateKeyPair('RS256')
  const wrongKey = app.client(await app.token('synthetic|wrong-key', {}, privateKey))
  await wrongKey.post('session', {}, 401)
  const anon = app.client(), { value: enrollment } = await anon.post('enroll', { name: 'Trial gate' })
  await account.post('pair/claim', { code: enrollment.code }, 403)
  const counts = await app.db.prepare('SELECT COUNT(*) AS count FROM stations WHERE account_id=?').bind(account.accountId).first()
  assert.equal(counts.count, 0)
})

test('account claim needs local proof approval; device cookies and station boundaries remain separate', async () => {
  const browser = await app.owner(), other = await app.owner(), desktop = app.client()
  const { value: enrollment } = await desktop.post('enroll', { name: 'Pair approval' })
  await browser.post('pair/claim', { code: enrollment.code })
  await other.post('pair/claim', { code: enrollment.code }, 400)
  const wrongProof = crypto.getRandomValues(new Uint8Array(32)).reduce((s,b) => s+b.toString(16).padStart(2,'0'), '')
  await desktop.post('enroll/approve', { id: enrollment.id, proof: wrongProof, credential: wrongProof }, 410)
  const { value: check } = await desktop.post('enroll/check', { id: enrollment.id, proof: enrollment.proof })
  assert.equal(check.accountId, browser.accountId); assert.equal(check.approved, false)
  await desktop.post('enroll/approve', { id: enrollment.id, proof: enrollment.proof, credential: wrongProof })
  const path = `stations/${enrollment.id}`, native = app.client(null, '', wrongProof)
  await other.post(`${path}/device`, { name: 'Wrong owner' }, 404)
  const { value: device, response } = await browser.post(`${path}/device`, { name: 'My test browser' })
  const setCookie = response.headers.get('set-cookie')
  assert.ok(setCookie.includes('HttpOnly') && setCookie.includes('Secure') && setCookie.includes('SameSite=Strict'))
  browser.setCookie(setCookie)
  await browser.post(`${path}/ticket`, {}, 403)
  await browser.post(`${path}/native/approve-device`, { deviceId: device.deviceId }, 403)
  await native.post(`${path}/native/approve-device`, { deviceId: device.deviceId })
  await browser.post(`${path}/ticket`)
  const cleared = app.client(browser.jwt)
  await cleared.post(`${path}/ticket`, {}, 403)
  const rows = await app.db.prepare('SELECT credential_hash FROM stations WHERE id=?').bind(enrollment.id).first()
  assert.equal(rows.credential_hash === wrongProof, false, 'only a digest is stored')
})

test('one-use tickets, real observation sockets, ACK backpressure and hibernation restoration', async () => {
  const pair = await app.paired(), live = await admitted(pair)
  await pair.browser.open(pair.stationId, live.ticket.ticket, 401)
  await publish(live.station, 1)
  const first = await live.browser.take(type('observation'))
  assert.equal(first.frame.source, 'native'); assert.equal(first.frame.sequence, 1)
  await publish(live.station, 2)
  live.browser.send({ type: 'ack', epoch: first.frame.epoch, sequence: 1 })
  const second = await live.browser.take(type('observation'))
  assert.equal(second.frame.sequence, 2, 'positive control: an ACK releases the coalesced publication')
  await publish(live.station, 3)
  const pendingRequest = await live.station.take(value => value.type === 'watch' && value.enabled)
  await app.evict(pair.stationId)
  live.browser.send({ type: 'ack', epoch: second.frame.epoch, sequence: 2 })
  await assert.rejects(live.browser.take(type('observation'), 100), /timeout/, 'hibernation discarded the in-memory latest frame')
  const duplicate = sample(); duplicate.sequence = 3
  live.station.send({ type: 'publication', requestId: pendingRequest.requestId, frame: duplicate })
  await assert.rejects(live.browser.take(type('observation'), 100), /timeout/, 'ordering survived wake and refused duplicate sequence')
  await publish(live.station, 4)
  const fourth = await live.browser.take(type('observation'))
  assert.equal(fourth.frame.sequence, 4)
  live.browser.send({ type: 'ack', epoch: fourth.frame.epoch, sequence: 4 })
  await pair.browser.post(`stations/${pair.stationId}/renew`, { sessionId: live.session.sessionId })
  await pair.native.post(`stations/${pair.stationId}/native/revoke-device`, { deviceId: live.deviceId })
  const closed = await live.browser.take(type('closed'))
  assert.equal(closed.code, 1008)
  await pair.browser.post(`stations/${pair.stationId}/ticket`, {}, 403)
  live.station.close()
})

test('command-shaped browser messages close the observer without reaching the station', async () => {
  const pair = await app.paired(), live = await admitted(pair)
  const pending = await live.station.take(value => value.type === 'watch' && value.enabled)
  live.browser.send({ type: 'ptt', enabled: true })
  assert.equal((await live.browser.take(type('closed'))).code, 1008)
  // Complete the observation already requested before the hostile message.
  // The next station message must be idle demand, never the browser's command.
  live.station.send({ type: 'publication', requestId: pending.requestId, frame: sample() })
  assert.deepEqual(await live.station.take(() => true), { type: 'watch', enabled: false })
  live.station.close()
})

test('station revocation reaches active sockets and native revoke remains idempotent', async () => {
  const pair = await app.paired(), live = await admitted(pair)
  assert.equal(live.browser.ws.readyState, 1)
  assert.equal(live.station.ws.readyState, 1)
  await pair.browser.post(`stations/${pair.stationId}/revoke`)
  const namespace = await app.mf.getDurableObjectNamespace('STATIONS')
  const status = await roomStatus(namespace.get(namespace.idFromName(pair.stationId)))
  assert.deepEqual(status, { enabled: false, online: false, observers: 0, sockets: 0, runtimeSockets: 0 }, 'revocation retires authority and all sockets')
  // Authority and the protocol close must retire promptly, independently of TCP
  // teardown. workerd also leaves both peers CLOSING in a minimal, unrelated
  // HTTP-initiated-close example; ws completes its 30-second close timeout.
  // Keep final close coverage too, without confusing it with access retirement.
  for (let tries = 0; tries < 20 && [live.browser, live.station].some(peer => peer.ws.readyState === 1); tries++) {
    await new Promise(resolve => setTimeout(resolve, 50))
  }
  assert.notEqual(live.browser.ws.readyState, 1)
  assert.notEqual(live.station.ws.readyState, 1)
  await assert.rejects(live.browser.take(type('observation'), 100), /timeout/)
  assert.equal((await live.browser.take(type('closed'), 35000)).code, 1001)
  assert.equal((await live.station.take(type('closed'))).code, 1008)
  await pair.native.open(pair.stationId, null, 401)
  await pair.native.post(`stations/${pair.stationId}/native/revoke`)
})

test('compiled hosted shell has pinned security headers and distributes its license texts', async () => {
  const response = await app.mf.dispatchFetch(app.origin)
  assert.equal(response.status, 200)
  const html = await response.text()
  assert.match(html, /Nexus Remote/)
  assert.match(html, /assets\/.*\.js/)
  const policy = response.headers.get('content-security-policy')
  assert.ok(policy.includes("frame-ancestors 'none'") && policy.includes('https://identity.remote-test.invalid'))
  assert.equal(policy.includes('unsafe-eval'), false)
  assert.equal(response.headers.get('permissions-policy'), 'camera=(), microphone=(), geolocation=()')
  const licenses = await app.mf.dispatchFetch(`${app.origin}/remote-licenses.txt`)
  assert.equal(licenses.status, 200)
  const text = await licenses.text()
  for (const marker of ['GNU GENERAL PUBLIC LICENSE', '@auth0/auth0-spa-js 2.24.1',
    'tokio-tungstenite 0.30.0', 'Community Data License Agreement', 'Copyright (c) 2018 Auth0']) assert.ok(text.includes(marker))
})

test('an in-flight publication survives the last browser disconnect and hibernation', async () => {
  const pair = await app.paired(), live = await admitted(pair)
  const pending = await live.station.take(value => value.type === 'watch' && value.enabled)
  live.browser.close()
  await live.browser.take(type('closed'))
  await app.evict(pair.stationId)
  live.station.send({ type: 'publication', requestId: pending.requestId, frame: sample() })
  await live.station.take(value => value.type === 'watch' && !value.enabled)
  const { value: ticket } = await pair.browser.post(`stations/${pair.stationId}/ticket`)
  const browser = await pair.browser.open(pair.stationId, ticket.ticket)
  await browser.take(type('session'))
  await publish(live.station, 2)
  assert.equal((await browser.take(type('observation'))).frame.sequence, 2)
  browser.close(); live.station.close()
})

test('tickets expire and concurrent redemption can admit only one observer', async () => {
  const pair = await app.paired(), live = await admitted(pair)
  const path = `stations/${pair.stationId}/ticket`
  const { value: expired } = await pair.browser.post(path)
  await app.db.prepare('UPDATE tickets SET expires_at=? WHERE station_id=?').bind(Date.now()-1, pair.stationId).run()
  await pair.browser.open(pair.stationId, expired.ticket, 401)
  const { value: ticket } = await pair.browser.post(path)
  // Both handshakes use the same opaque ticket, without allowing an assertion to
  // echo it in a failure. Exactly one DELETE RETURNING may consume it.
  const first = pair.browser.open(pair.stationId, ticket.ticket)
  const second = pair.browser.open(pair.stationId, ticket.ticket)
  const results = await Promise.allSettled([first, second])
  assert.equal(results.filter(result => result.status === 'fulfilled').length, 1)
  assert.equal(results.filter(result => result.status === 'rejected').length, 1)
  for (const result of results) if (result.status === 'fulfilled') result.value.close()
  live.browser.close(); live.station.close()
})

test('four observers are admitted and a fifth cannot consume an untracked socket', async () => {
  const pair = await app.paired(), live = await admitted(pair), browsers = [live.browser]
  for (let i = 1; i < 4; i++) {
    const { value } = await pair.browser.post(`stations/${pair.stationId}/ticket`)
    const browser = await pair.browser.open(pair.stationId, value.ticket)
    await browser.take(type('session')); browsers.push(browser)
  }
  const { value } = await pair.browser.post(`stations/${pair.stationId}/ticket`)
  await pair.browser.open(pair.stationId, value.ticket, 403)
  await publish(live.station, 1)
  for (const browser of browsers) assert.equal((await browser.take(type('observation'))).frame.sequence, 1)
  const ns = await app.mf.getDurableObjectNamespace('STATIONS')
  const state = await roomStatus(ns.get(ns.idFromName(pair.stationId)))
  assert.equal(state.observers, 4); assert.equal(state.sockets, 5)
  for (const browser of browsers) browser.close()
  live.station.close()
})

test('trial expiry alarms retire an observer even if it never attempts renewal', async () => {
  const pair = await app.paired()
  await app.db.prepare('UPDATE trials SET expires_at=? WHERE account_id=?').bind(Date.now()+1800, pair.browser.accountId).run()
  const live = await admitted(pair)
  await publish(live.station, 1)
  const frame = await live.browser.take(type('observation'))
  live.browser.send({ type: 'ack', epoch: frame.frame.epoch, sequence: frame.frame.sequence })
  const close = await live.browser.take(type('closed'), 3000)
  assert.equal(close.code, 1008)
  await pair.browser.post(`stations/${pair.stationId}/renew`, { sessionId: live.session.sessionId }, 403)
  await pair.browser.post(`stations/${pair.stationId}/ticket`, {}, 403)
  live.station.close()
})

test('station replacement retires former observers and rejects stale publications', async () => {
  const pair = await app.paired(), live = await admitted(pair)
  await publish(live.station, 2)
  const initial = await live.browser.take(type('observation'))
  live.browser.send({ type: 'ack', epoch: initial.frame.epoch, sequence: initial.frame.sequence })
  const replacement = await pair.native.open(pair.stationId)
  await replacement.take(value => value.type === 'watch' && !value.enabled)
  assert.equal((await live.browser.take(type('closed'))).code, 1001)
  const { value: ticket } = await pair.browser.post(`stations/${pair.stationId}/ticket`)
  const browser = await pair.browser.open(pair.stationId, ticket.ticket)
  await browser.take(type('session'))
  await publish(replacement, 1)
  await assert.rejects(browser.take(type('observation'), 100), /timeout/)
  await publish(replacement, 3)
  assert.equal((await browser.take(type('observation'))).frame.sequence, 3)
  browser.close(); replacement.close()
})
