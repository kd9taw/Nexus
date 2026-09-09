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
const applicationSample = (requestId, command = 'get_snapshot') => ({ type: 'applicationResult', requestId, command,
  revision: 1, baseRevision: null, ageMs: 0, data: { mycall: 'N0CALL' }, removed: [] })
async function admitted(pair, applicationVersion = 0, extensions = {}) {
  const deviceId = await app.approved(pair)
  const station = await pair.native.open(pair.stationId, undefined, 101, { 'x-nexus-application-version': String(applicationVersion), ...extensions })
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

test('application reads use approved sockets, survive hibernation and retain observer-only authority', async () => {
  const pair = await app.paired(), live = await admitted(pair, 1)
  live.browser.send({ type: 'applicationHello' })
  const capabilities = await live.browser.take(type('applicationCapabilities'))
  assert.equal(capabilities.version, 1)
  assert.deepEqual(capabilities.commands, ['get_snapshot', 'get_settings', 'get_band_plan', 'get_spectrum_row', 'get_meters'])
  const requestId = crypto.randomUUID()
  live.browser.send({ type: 'applicationRead', requestId, command: 'get_snapshot', revision: null })
  const request = await live.station.take(type('applicationRead'))
  assert.notEqual(request.requestId, requestId, 'the room owns routing IDs')
  await app.evict(pair.stationId)
  live.station.send({ type: 'applicationResult', requestId: request.requestId, command: 'get_snapshot',
    revision: 1, baseRevision: null, ageMs: 0, data: { mycall: 'N0CALL', radio: { dialMhz: 3.573 } }, removed: [] })
  const result = await live.browser.take(type('applicationResult'))
  assert.equal(result.requestId, requestId)
  assert.equal(result.data.radio.dialMhz, 3.573)
  live.browser.send({ type: 'applicationAck', requestId })
  await publish(live.station, 1)
  const observation = await live.browser.take(type('observation'))
  live.browser.send({ type: 'ack', epoch: observation.frame.epoch, sequence: observation.frame.sequence })
  live.browser.send({ type: 'applicationRead', requestId: crypto.randomUUID(), command: 'set_tx_enabled', revision: null })
  assert.equal((await live.browser.take(type('closed'))).code, 1008)
  const namespace = await app.mf.getDurableObjectNamespace('STATIONS')
  const status = await roomStatus(namespace.get(namespace.idFromName(pair.stationId)))
  assert.equal(status.observers, 0, 'application rejection removes the authoritative observer immediately')
  assert.equal(status.online, true, 'the station survives one browser refusal')
  await assert.rejects(live.station.take(type('applicationRead'), 100), /timeout/)
  live.station.close()
})

test('an older station advertises an update requirement and continues its valid monitor stream', async () => {
  const pair = await app.paired(), live = await admitted(pair)
  live.browser.send({ type: 'applicationHello' })
  assert.deepEqual(await live.browser.take(type('applicationCapabilities')), { type: 'applicationCapabilities', version: 0, commands: [] })
  live.browser.send({ type: 'applicationRead', requestId: crypto.randomUUID(), command: 'get_snapshot', revision: null })
  assert.equal((await live.browser.take(type('applicationError'))).error, 'stationUpdateRequired')
  await assert.rejects(live.station.take(type('applicationRead'), 100), /timeout/)
  await publish(live.station, 1)
  const frame = await live.browser.take(type('observation'))
  assert.equal(frame.frame.sequence, 1)
  live.browser.close(); live.station.close()
})

test('v2 subscriptions share native samples across approved browsers and recover full bases after hibernation', async () => {
  const pair = await app.paired(), live = await admitted(pair, 2)
  const { value: ticket } = await pair.browser.post(`stations/${pair.stationId}/ticket`)
  const second = await pair.browser.open(pair.stationId, ticket.ticket)
  await second.take(type('session'))
  for (const browser of [live.browser, second]) {
    browser.send({ type: 'applicationHello', version: 2 })
    const capabilities = await browser.take(type('applicationCapabilities'))
    assert.equal(capabilities.version, 2)
    assert.ok(capabilities.commands.includes('get_cw_state'))
    browser.send({ type: 'applicationSubscribe', topics: ['get_snapshot', 'get_scope_snapshot', 'get_cw_state'], requestId: crypto.randomUUID() })
  }
  let watch = await live.station.take(type('applicationWatch'))
  assert.deepEqual(watch.topics, ['get_snapshot', 'get_scope_snapshot', 'get_cw_state'])
  await assert.rejects(live.station.take(type('applicationWatch'), 100), /timeout/, 'second browser reuses the same native watch')
  live.station.send({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId,
    updates: watch.topics.map(command => applicationSample(watch.requestId, command)) })
  const frames = await Promise.all([live.browser, second].map(browser => browser.take(type('applicationFrame'))))
  assert.notEqual(frames[0].requestId, frames[1].requestId)
  for (const frame of frames) {
    assert.equal(frame.updates.length, 3)
    assert.equal(frame.updates[0].data.mycall, 'N0CALL')
  }
  const old = await live.station.take(type('applicationCredit'))
  await app.evict(pair.stationId)
  live.browser.send({ type: 'applicationFrameAck', requestId: frames[0].requestId, nextRequestId: crypto.randomUUID() })
  second.send({ type: 'applicationFrameAck', requestId: frames[1].requestId, nextRequestId: crypto.randomUUID() })
  watch = await live.station.take(type('applicationWatch'))
  assert.notEqual(watch.watchId, old.watchId)
  live.station.send({ type: 'applicationBatch', watchId: old.watchId, requestId: old.requestId, updates: [applicationSample(old.requestId)] })
  await assert.rejects(live.browser.take(type('applicationFrame'), 100), /timeout/, 'retired epoch cannot refresh data')
  live.station.send({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId,
    updates: watch.topics.map(command => applicationSample(watch.requestId, command)) })
  for (const browser of [live.browser, second]) {
    const frame = await browser.take(type('applicationFrame'))
    assert.equal(frame.updates[0].baseRevision, null)
    browser.send({ type: 'applicationSubscribe', topics: [], requestId: null })
  }
  const idle = await live.station.take(value => value.type === 'applicationWatch' && value.topics.length === 0)
  assert.equal(idle.requestId, null)
  live.browser.close(); second.close(); live.station.close()
})

test('keyboard observation needs the complete native advertisement and survives room hibernation', async () => {
  const config = await (await fetch(`${app.origin}/api/remote/config`)).json()
  assert.equal(config.applicationVersion, 5)
  const headers = { 'x-nexus-application-stream-version': '2', 'x-nexus-application-query-version': '1',
    'x-nexus-application-recall-version': '1', 'x-nexus-application-keyboard-version': '1' }
  for (const [missing, expected] of [[null, 5], ['keyboard', 4], ['recall', 3], ['query', 2], ['stream', 1]]) {
    const advertisement = { ...headers }
    if (missing) delete advertisement[`x-nexus-application-${missing}-version`]
    const pair = await app.paired(), live = await admitted(pair, 1, advertisement)
    live.browser.send({ type: 'applicationHello', version: 5 })
    const capabilities = await live.browser.take(type('applicationCapabilities'))
    assert.equal(capabilities.version, expected)
    assert.equal(capabilities.commands.includes('get_rtty_state'), expected === 5)
    assert.equal(capabilities.commands.includes('get_psk_state'), expected === 5)
    if (expected === 5) {
      const topics = capabilities.commands.filter(command => !['get_remote_page', 'get_remote_recall'].includes(command))
      assert.equal(topics.length, 9)
      const requestId = crypto.randomUUID()
      live.browser.send({ type: 'applicationSubscribe', topics, requestId })
      let watch = await live.station.take(type('applicationWatch'))
      assert.deepEqual(watch.topics, topics)
      live.station.send({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId,
        updates: topics.map(command => applicationSample(watch.requestId, command)) })
      const first = await live.browser.take(type('applicationFrame'))
      assert.equal(first.updates.length, 9)
      await app.evict(pair.stationId)
      live.browser.send({ type: 'applicationFrameAck', requestId, nextRequestId: crypto.randomUUID() })
      const prior = watch
      watch = await live.station.take(type('applicationWatch'))
      assert.notEqual(watch.watchId, prior.watchId)
      assert.deepEqual(watch.topics, topics)
      live.station.send({ type: 'applicationBatch', watchId: watch.watchId, requestId: watch.requestId,
        updates: topics.map(command => applicationSample(watch.requestId, command)) })
      const resumed = await live.browser.take(type('applicationFrame'))
      assert.equal(resumed.updates.length, 9)
      assert.ok(resumed.updates.every(update => update.baseRevision === null))
      live.browser.send({ type: 'applicationSubscribe', topics: ['psk_type'], requestId: crypto.randomUUID() })
      assert.equal((await live.browser.take(type('closed'))).code, 1008)
      const namespace = await app.mf.getDurableObjectNamespace('STATIONS')
      assert.equal((await roomStatus(namespace.get(namespace.idFromName(pair.stationId)))).online, true)
    }
    live.browser.close(); live.station.close()
  }
})

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
