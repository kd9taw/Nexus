import { test } from 'node:test'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import assert from 'node:assert/strict'
import { setTimeout as delay } from 'node:timers/promises'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { runtime, roomStatus } from './runtime.mjs'

/** A browser's device key, made for the run, and the proof it puts on what the relay's older lanes
 *  carry (security review S1-M1), made the way the page makes it: the real station requires it on
 *  every operation request but Stop and `state`, and on every Listen. */
async function deviceKey() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])
  const spki = new Uint8Array(await crypto.subtle.exportKey('spki', pair.publicKey))
  const hex = bytes => Buffer.from(bytes).toString('hex'), text = new TextEncoder()
  let seq = 0
  return {
    publicKey: hex(spki),
    fingerprint: hex(await crypto.subtle.digest('SHA-256', spki)),
    async prove(message, { stationId, deviceId, sessionId }) {
      if (message.type === 'operationRequest' && ['stopTransmit', 'state'].includes(message.request.type)) return message
      const listen = message.type === 'audioListen'
      const body = listen ? `{"listening":${message.listening},"leaseId":"${message.leaseId}"}` : JSON.stringify(message.request)
      const number = new Uint8Array(8)
      new DataView(number.buffer).setBigUint64(0, BigInt(++seq))
      const signed = new Uint8Array([...text.encode(listen ? 'nexus-listen/1' : 'nexus-operation/1'),
        ...new Uint8Array(await crypto.subtle.digest('SHA-256', text.encode(body))),
        ...text.encode(stationId), ...text.encode(deviceId), ...text.encode(sessionId), ...number])
      return { ...message, proof: { publicKey: hex(spki), seq, signature: hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, signed)) } }
    },
  }
}

// The old Remote page's FT8/FT4 and held-contact actions are gone from the station. A browser that
// still holds station control and a lease and sends one
// anyway, as a stale page could, is answered `stationUnsupported` through the real relay and the real
// station: nothing keys, nothing is logged, and its lease, its socket and its Stop are untouched.
test('actual native: the old page\'s FT8/FT4 and held-contact actions are refused, and the lease and Stop stay', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
  const app = await runtime(), probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  let socket
  try {
    await probe.ready()
    const browser = await app.owner(), begin = await probe.send({ type: 'begin', name: 'Old page bench' })
    const stationId = begin.status.pairingId
    await browser.post('pair/claim', { code: begin.status.pairingCode }); await browser.post('pair/confirm', { id: stationId }); await probe.send({ type: 'refresh' })
    assert.equal((await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId })).ok, true)
    // The browser's key, shown at the radio and pinned with the approval (A5, S1-M1).
    const key = await deviceKey()
    const { value: device, response } = await browser.post(`stations/${stationId}/device`, { name: 'Old page browser', publicKey: key.publicKey })
    browser.setCookie(response.headers.get('set-cookie')); await probe.send({ type: 'refresh' })
    await probe.send({ type: 'device', deviceId: device.deviceId, approve: true, key: key.fingerprint, kind: 'browser' }); await probe.send({ type: 'enable' })
    const ns = await app.mf.getDurableObjectNamespace('STATIONS'), room = ns.get(ns.idFromName(stationId))
    for (let i = 0; i < 30 && !(await roomStatus(room)).online; i++) await delay(100)
    assert.equal((await roomStatus(room)).online, true)
    await probe.send({ type: 'refresh' })
    const result = await probe.send({ type: 'stationPermission', deviceId: device.deviceId, allow: true })
    assert.equal(result.ok, true, `stationPermission: ${result.error}`)
    assert.deepEqual(await probe.send({ type: 'seedLogging' }), { count: 0, adif: '', txEnabled: false })
    assert.equal((await probe.send({ type: 'seedFt', tier: 'FT8' })).txEnabled, false)
    const ticket = (await browser.post(`stations/${stationId}/ticket`)).value
    socket = await browser.open(stationId, ticket.ticket); socket.ackObservations()
    const { sessionId } = await socket.take(v => v.type === 'session')
    const send = async args => {
      await delay(270)
      const request = { requestId: crypto.randomUUID(), ...args }
      socket.send(await key.prove({ type: 'operationRequest', operationVersion: 4, request }, { stationId, deviceId: device.deviceId, sessionId }))
      return { request, response: await socket.take(v => v.type === 'operationResponse' && v.requestId === request.requestId) }
    }
    // stationBusy means the request was refused before anything changed: retry once.
    const operation = async args => {
      let sent = await send(args)
      if (sent.response.error === 'stationBusy') { await delay(300); sent = await send(args) }
      return sent.response
    }
    const initial = (await operation({ type: 'state' })).value
    const owner = (await operation({ type: 'acquire', stationBootId: initial.stationBootId })).value
    assert.equal(owner.phase, 'controlling', 'premise: the browser holds the lease')
    assert.match(owner.transmitEpoch, /^[0-9a-f]{16}$/, 'premise: station control holds the stop token')
    assert.equal(owner.controls.capabilities.some(c => c.startsWith('ft') || c === 'qsoLogging'), false,
      `no FT8/FT4 or held-contact capability is offered: ${owner.controls.capabilities}`)
    const qso = { dxcall: 'W1AW', state: 'idle', txNow: null, cqRunning: false }
    for (const action of [
      { action: 'ft.cq', expectedTier: 'FT8', direction: null },
      { action: 'ft.txEnabled', expectedTier: 'FT8', on: true },
      { action: 'ft.cq', expectedTier: 'FT8', direction: 'DX' },
      { action: 'qso.logCurrent', expectedKey: '0123456789abcdef0123456789abcdef', expectedTier: 'FT8', expectedQso: qso },
      { action: 'qso.discard', expectedKey: '0123456789abcdef' },
    ]) {
      const state = (await operation({ type: 'heartbeat', leaseId: owner.leaseId })).value
      assert.equal(state.phase, 'controlling', `${action.action}: the lease is held`)
      const answer = await operation({ type: 'stationControl', stationBootId: state.stationBootId, leaseId: state.leaseId,
        expectedRevision: state.revision, commandWindowId: state.commandWindowId, clientSequence: state.nextSequence,
        context: state.controls.context, action: action.action.startsWith('ft.') ? { ...action, transmitEpoch: state.transmitEpoch } : action })
      assert.equal(answer.error, 'stationUnsupported', `${action.action}: ${JSON.stringify(answer)}`)
    }
    assert.equal((await operation({ type: 'result', operationId: crypto.randomUUID() })).error, 'stationUnsupported')
    const evidence = await probe.send({ type: 'ftEvidence' })
    assert.equal(evidence.txEnabled, false, 'nothing armed'); assert.equal(evidence.owned, false)
    assert.deepEqual(await probe.send({ type: 'loggingEvidence' }), { count: 0, adif: '', txEnabled: false }, 'nothing logged')
    // CONTROL: the same socket and lease still work, and Stop is still taken.
    const after = (await operation({ type: 'heartbeat', leaseId: owner.leaseId })).value
    assert.equal(after.phase, 'controlling', 'the refusals left the lease alone')
    const stop = await operation({ type: 'stopTransmit', stationBootId: after.stationBootId, leaseId: after.leaseId, transmitEpoch: after.transmitEpoch })
    assert.deepEqual(stop.value, { stop: 'accepted' }, JSON.stringify(stop))
    assert.equal(await probe.stop(), 0, 'native probe must exit successfully')
  } finally {
    socket?.close()
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

// One approval through workerd (operator decisions 2026-09-13 and 2026-09-14): approving the pairing
// turns Remote on and grants the browser that confirmed it station control. It comes back
// after a restart with the TX-enable latch still off, and a later Turn off Remote is remembered.
test('actual native one approval: pairing turns Remote on, grants the pairing browser, keeps its grants across a restart with TX off', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
  const app = await runtime(), probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  try {
    await probe.ready()
    const browser = await app.owner(), begin = await probe.send({ type: 'begin', name: 'One approval bench' })
    const stationId = begin.status.pairingId
    await browser.post('pair/claim', { code: begin.status.pairingCode })
    const { response } = await browser.post('pair/confirm', { id: stationId })
    browser.setCookie(response.headers.get('set-cookie'))
    await probe.send({ type: 'refresh' })
    const paired = await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId })
    assert.equal(paired.ok, true, paired.error)
    assert.ok(['connecting', 'connected', 'reconnecting'].includes(paired.status.phase), paired.status.phase)
    const { value: device } = await browser.post(`stations/${stationId}/device`, { name: 'Not asked for' })
    assert.equal(device.approved, true, 'the pairing browser was approved with the station')
    const granted = async label => {
      for (let i = 0; i < 100; i++) {
        const { status } = await probe.send({ type: 'status' })
        if (status.stationPermissions?.includes(device.deviceId)) return status
        await delay(100)
      }
      assert.fail(`${label}: the pairing browser's grants never appeared`)
    }
    await granted('after the approval')
    assert.equal((await probe.send({ type: 'ftEvidence' })).txEnabled, false)
    // An approval lasts until removed (operator decision 2026-10-10). One given under the old thirty-day
    // rule is kept on use: the service moves the expiry but never the generation, so the restart below
    // still restores control, with the latch off.
    const UNTIL_REMOVED = Date.UTC(9999, 11, 31, 23, 59, 59, 999)
    const deviceRow = () => app.db.prepare('SELECT generation,approved_at,expires_at FROM devices WHERE id=?').bind(device.deviceId).first()
    const approval = await deviceRow()
    assert.ok(approval.approved_at, 'this Nexus asked for the approval lifetime')
    assert.equal(approval.expires_at, UNTIL_REMOVED, 'until removed')
    await app.db.prepare('UPDATE devices SET expires_at=? WHERE id=?').bind(Date.now() + 86400000, device.deviceId).run()
    await browser.post(`stations/${stationId}/ticket`)
    const renewed = await deviceRow()
    assert.equal(renewed.expires_at, UNTIL_REMOVED, 'kept on use')
    assert.equal(renewed.generation, approval.generation)
    const listed = await probe.send({ type: 'refresh' })
    const shown = listed.status.devices.find(d => d.id === device.deviceId)
    assert.deepEqual([shown?.expiresAt, shown?.untilRemoved], [UNTIL_REMOVED, 1], 'the shack sees it until removed')
    await delay(300)
    await probe.send({ type: 'restart' })
    await granted('after a restart')
    const evidence = await probe.send({ type: 'ftEvidence' })
    assert.equal(evidence.txEnabled, false, 'the TX-enable latch is off after the restart')
    assert.equal(evidence.owned, false)
    assert.equal((await probe.send({ type: 'disable' })).ok, true)
    await delay(300)
    await probe.send({ type: 'restart' })
    let status
    for (let i = 0; i < 50 && !status?.stationId; i++) { status = (await probe.send({ type: 'status' })).status; if (!status.stationId) await delay(100) }
    assert.equal(status.phase, 'disabled', 'Turn off Remote is remembered')
    assert.equal(status.transmitPermissions, undefined, 'no FT8/FT4 transmit permission is kept or shown')
    assert.equal(status.loggingPermissions, undefined, 'no remote logging permission is kept or shown')
    assert.equal(await probe.stop(), 0, 'native probe must exit successfully')
  } finally {
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

// A5 across the real service and the real station: the key a browser sends reaches the station as
// its SHA-256 (the key itself never reaches the desktop page), approving with that key shown pins it,
// and the pin survives a restart. The key is made here, at run time, and never written down.
test('actual native A5: the station pins the device key it showed, and keeps it across a restart', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
  const app = await runtime(), probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  try {
    await probe.ready()
    const browser = await app.owner(), begin = await probe.send({ type: 'begin', name: 'Device key bench' })
    const stationId = begin.status.pairingId
    await browser.post('pair/claim', { code: begin.status.pairingCode })
    await browser.post('pair/confirm', { id: stationId })
    await probe.send({ type: 'refresh' })
    const paired = await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId })
    assert.equal(paired.ok, true, paired.error)
    const keyed = app.client(browser.jwt)
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])
    const spki = new Uint8Array(await crypto.subtle.exportKey('spki', pair.publicKey))
    const fingerprint = Buffer.from(await crypto.subtle.digest('SHA-256', spki)).toString('hex')
    const { value: requested, response } = await keyed.post(`stations/${stationId}/device`,
      { name: 'Keyed browser', publicKey: Buffer.from(spki).toString('hex') })
    keyed.setCookie(response.headers.get('set-cookie'))
    const listed = await probe.send({ type: 'refresh' })
    const shown = listed.status.devices.find(d => d.id === requested.deviceId)
    assert.equal(shown?.key, fingerprint, 'the station shows the SHA-256 of the key the browser sent')
    assert.equal('publicKey' in shown, false, 'the key itself never reaches the desktop page')
    assert.deepEqual(listed.status.pinnedDevices, [], 'control: nothing pinned before the approval')
    const approved = await probe.send({ type: 'device', deviceId: requested.deviceId, approve: true, key: fingerprint, kind: 'browser' })
    assert.equal(approved.ok, true, approved.error)
    assert.deepEqual(approved.status.pinnedDevices, [requested.deviceId])
    await delay(300)
    await probe.send({ type: 'restart' })
    let status
    for (let i = 0; i < 50 && !status?.pinnedDevices?.includes(requested.deviceId); i++) {
      status = (await probe.send({ type: 'refresh' })).status
      if (!status?.pinnedDevices?.includes(requested.deviceId)) await delay(100)
    }
    assert.deepEqual(status.pinnedDevices, [requested.deviceId], 'the pin survived a restart')
    assert.equal(await probe.stop(), 0, 'native probe must exit successfully')
  } finally {
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

// Operator ruling D4 across the real service and the real station: the browser that confirms the
// pairing brings its key, the approval gives it to the browser it approves, and the station pins it
// on first use. Both ends then show the same key: the station its SHA-256, the page its own.
test('actual native A5: approving the pairing pins the key the confirming browser brought', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
  const app = await runtime(), probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  try {
    await probe.ready()
    const browser = await app.owner(), begin = await probe.send({ type: 'begin', name: 'Pairing key bench' })
    const stationId = begin.status.pairingId
    await browser.post('pair/claim', { code: begin.status.pairingCode })
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])
    const spki = new Uint8Array(await crypto.subtle.exportKey('spki', pair.publicKey))
    const publicKey = Buffer.from(spki).toString('hex')
    const fingerprint = Buffer.from(await crypto.subtle.digest('SHA-256', spki)).toString('hex')
    const { response } = await browser.post('pair/confirm', { id: stationId, publicKey })
    browser.setCookie(response.headers.get('set-cookie'))
    await probe.send({ type: 'refresh' })
    const paired = await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId })
    assert.equal(paired.ok, true, paired.error)
    let status
    for (let i = 0; i < 50 && !status?.pinnedDevices?.length; i++) {
      status = (await probe.send({ type: 'refresh' })).status
      if (!status?.pinnedDevices?.length) await delay(100)
    }
    const [device] = status.devices
    assert.equal(device.key, fingerprint, 'the station shows the SHA-256 of the key the confirming browser brought')
    assert.deepEqual(status.pinnedDevices, [device.id], 'and it is pinned, on first use')
    const { value: session } = await browser.post('session')
    assert.equal(session.stations.find(s => s.id === stationId).device.publicKey, publicKey, 'the page reads its own key back')
    assert.equal(await probe.stop(), 0, 'native probe must exit successfully')
  } finally {
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

// The native client through the real service and the real station: Nexus on another computer registers
// its key as a device of the station (signed by that key, over a challenge, on the Native application's
// token, the runtime's stand-in issuer), the station is listed it as a computer with its platform because
// it asks for kinds, and approving it there pins its key and grants nothing on the relay's lanes.
test('actual native: a computer running Nexus is listed to the station as one, and approving it pins its key and grants nothing on the lanes', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
  const NATIVE = 'remote-test-native'
  const app = await runtime({ bindings: { AUTH0_NATIVE_CLIENT_ID: NATIVE } }), probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  try {
    await probe.ready()
    const browser = await app.owner(), begin = await probe.send({ type: 'begin', name: 'Computer bench' })
    const stationId = begin.status.pairingId
    await browser.post('pair/claim', { code: begin.status.pairingCode })
    await browser.post('pair/confirm', { id: stationId })
    await probe.send({ type: 'refresh' })
    const paired = await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId })
    assert.equal(paired.ok, true, paired.error)
    // The computer: its key made in memory, its requests signed as the native client signs them.
    const subject = JSON.parse(Buffer.from(browser.jwt.split('.')[1], 'base64url')).sub
    const computer = app.client(null, '', await app.token(subject, { azp: NATIVE }))
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])
    const spki = new Uint8Array(await crypto.subtle.exportKey('spki', pair.publicKey))
    const hex = bytes => Buffer.from(bytes).toString('hex')
    const fingerprint = hex(await crypto.subtle.digest('SHA-256', spki))
    const path = `stations/${stationId}/client/device`, body = { name: 'Shack laptop', kind: 'computer', platform: 'windows', publicKey: hex(spki) }
    const { value: { challenge } } = await computer.post('client/challenge')
    const signed = ['nexus-native-request/1', challenge, 'POST', `/api/remote/${path}`,
      hex(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify(body))))].join('\n')
    const signature = hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, new TextEncoder().encode(signed)))
    const { value: registered } = await computer.post(path, body, 200, { 'x-nexus-challenge': challenge, 'x-nexus-signature': signature })
    assert.equal(registered.approved, false, 'premise: it waits for the shack')
    const listed = await probe.send({ type: 'refresh' })
    const shown = listed.status.devices.find(d => d.id === registered.deviceId)
    assert.deepEqual(shown && { kind: shown.kind, platform: shown.platform, key: shown.key, name: shown.name, approved: shown.approved },
      { kind: 'computer', platform: 'windows', key: fingerprint, name: 'Shack laptop', approved: 0 }, 'listed to the station as a computer')
    assert.equal('publicKey' in shown, false, 'the key itself never reaches the desktop page')
    assert.deepEqual(listed.status.devices.filter(d => d.id !== registered.deviceId).map(d => d.kind), ['browser'],
      'control: the pairing\'s browser beside it, as a browser')
    // The shack's press, as its screen sends it: the key shown, and the kind it was shown as.
    const approved = await probe.send({ type: 'device', deviceId: registered.deviceId, approve: true, key: fingerprint, kind: 'computer' })
    assert.equal(approved.ok, true, approved.error)
    assert.deepEqual(approved.status.pinnedDevices, [registered.deviceId], 'its key is pinned, as a browser\'s is')
    assert.equal(approved.status.stationPermissions?.includes(registered.deviceId), false, 'nothing on the relay\'s lanes')
    assert.ok(approved.status.stationPermissions?.length, 'control: the pairing\'s browser holds station control')
    assert.equal(await probe.stop(), 0, 'native probe must exit successfully')
  } finally {
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

// S1-M1 and S3-M1 through the real relay and the real station: the station records its own key with
// the service when it connects, and its pages read it there; and the station takes control only on
// the proof of the key pinned for the browser at the radio, while state and Stop need none.
test('actual native S1-M1/S3-M1: the station\'s key is recorded for its pages, and control is taken only on the pinned key\'s proof', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
  const app = await runtime(), probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  let socket
  try {
    await probe.ready()
    const browser = await app.owner(), begin = await probe.send({ type: 'begin', name: 'Lane proof bench' })
    const stationId = begin.status.pairingId, key = await deviceKey(), forger = await deviceKey()
    await browser.post('pair/claim', { code: begin.status.pairingCode })
    const { response } = await browser.post('pair/confirm', { id: stationId, publicKey: key.publicKey })
    browser.setCookie(response.headers.get('set-cookie'))
    await probe.send({ type: 'refresh' })
    assert.equal((await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId })).ok, true)
    // S3-M1: the station sent its key before it connected; the account's page reads it from the service.
    let listed = null
    for (let i = 0; i < 50 && !listed; i++) {
      listed = (await browser.post('session')).value.stations.find(s => s.id === stationId)?.stationKey ?? null
      if (!listed) await delay(100)
    }
    assert.match(listed ?? '', /^3059301306072a8648ce3d020106082a8648ce3d03010703420004[0-9a-f]{128}$/, 'a P-256 key, recorded at the first connection')
    const ns = await app.mf.getDurableObjectNamespace('STATIONS'), room = ns.get(ns.idFromName(stationId))
    for (let i = 0; i < 30 && !(await roomStatus(room)).online; i++) await delay(100)
    for (let i = 0; i < 50; i++) { const { status } = await probe.send({ type: 'status' }); if (status.stationPermissions?.length && status.pinnedDevices?.length) break; await delay(100) }
    const { value: device } = await browser.post(`stations/${stationId}/device`, { name: 'Lane browser' })
    const ticket = (await browser.post(`stations/${stationId}/ticket`)).value
    socket = await browser.open(stationId, ticket.ticket); socket.ackObservations()
    const { sessionId } = await socket.take(v => v.type === 'session')
    const ids = { stationId, deviceId: device.deviceId, sessionId }
    // A request, proved with `by` (the pinned key unless said), or sent with none at all.
    const ask = async ({ by = key, unsigned = false, ...message }) => {
      for (let attempt = 0; ; attempt++) {
        await delay(270)
        const sent = { ...message, request: { ...message.request, requestId: crypto.randomUUID() } }
        socket.send(unsigned ? sent : await by.prove(sent, ids))
        const answer = await socket.take(v => v.type === 'operationResponse' && v.requestId === sent.request.requestId)
        if (answer.error !== 'stationBusy' || attempt) return answer
      }
    }
    const request = request => ({ type: 'operationRequest', operationVersion: 4, request })
    const state = (await ask(request({ type: 'state' }))).value
    assert.equal(state.phase, 'available', 'state needs no proof')
    // THE FINDING, through the real relay: an acquire with no proof. Then one signed by another key.
    const acquire = { type: 'acquire', stationBootId: state.stationBootId }
    const unsigned = await ask({ ...request(acquire), unsigned: true })
    assert.equal(unsigned.error, 'stationUnsupported', JSON.stringify(unsigned))
    assert.equal((await ask({ ...request(acquire), by: forger })).error, 'deviceKeyMismatch')
    // CONTROL: the same acquire, proved with the pinned key, takes control; Stop needs no proof.
    const owner = await ask(request(acquire))
    assert.equal(owner.value?.phase, 'controlling', JSON.stringify(owner))
    const held = (await ask(request({ type: 'state' }))).value
    const stop = await ask(request({ type: 'stopTransmit', stationBootId: held.stationBootId, leaseId: held.leaseId,
      transmitEpoch: held.transmitEpoch }))
    assert.deepEqual(stop.value, { stop: 'accepted' }, JSON.stringify(stop))
    assert.equal(await probe.stop(), 0, 'native probe must exit successfully')
  } finally {
    try { socket?.close() } catch { /* already closed */ }
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

async function nativeProbe(binary, origin) {
  const profile=await mkdtemp(join(tmpdir(),'nexus-native-profile-'))
  const child = spawn(binary, ['--ignored', '--exact', 'remote_service::tests::cloud_runtime_probe', '--nocapture'], { stdio: ['pipe', 'pipe', 'pipe'], env:{...process.env,XDG_CONFIG_HOME:profile,APPDATA:profile,NEXUS_DATA_DIR:join(profile,'shared'),NEXUS_PROFILE:''} })
  const queue = [], waiting = []
  // Never forward raw probe output: its pipe includes the one-time pairing code. Only a panic's place is kept,
  // `panicked at <file>:<line>:<col>`, which names a source line and nothing else.
  let panic = null
  createInterface({ input: child.stderr }).on('line', line => { panic ??= line.match(/panicked at (\S+:\d+:\d+)/)?.[1] ?? null })
  let exited = false, exitCode = null
  const gone = what => new Error(`native probe exited (code ${exitCode}${panic ? `, panicked at ${panic}` : ''}) before answering ${what}`)
  const exit = new Promise(resolve => {
    const finish = code => {
      exited = true; exitCode = code
      for (const waiter of waiting.splice(0)) { clearTimeout(waiter.timer); waiter.reject(gone(waiter.what)) }
      resolve(code)
    }
    // `close`, not `exit`: it comes once the pipes are read to the end, the panic's line included.
    child.once('close', finish)
    child.once('error', () => finish(null))
  })
  const lines = createInterface({ input: child.stdout })
  lines.on('line', line => {
    if (!line.startsWith('REMOTE_TEST:')) return
    const value = JSON.parse(line.slice('REMOTE_TEST:'.length))
    const waiter = waiting.shift()
    if (waiter) { clearTimeout(waiter.timer); waiter.resolve(value) } else queue.push(value)
  })
  child.stdin.write(JSON.stringify({ origin, configurationRoot:profile }) + '\n')
  async function receive(what) {
    if (queue.length) return queue.shift()
    if (exited) throw gone(what)
    return new Promise((resolve, reject) => {
      const waiter = { resolve, reject, timer: null, what }
      waiter.timer = setTimeout(() => { waiting.splice(waiting.indexOf(waiter), 1); reject(new Error(`native probe response to ${what} timed out`)) }, 15000)
      waiting.push(waiter)
    })
  }
  return {
    ready: () => receive('ready'),
    async send(action) { child.stdin.write(JSON.stringify(action) + '\n'); return receive(action.type) },
    /** Ends the probe and returns its exit code; a second call returns it again. It asserts nothing: a test
     *  asserts the code as its body's last step, because an error thrown from its `finally` would replace the
     *  body's own, and a probe that died mid-test then read as nothing but its exit code. */
    async stop() {
      if (!exited) child.stdin.end('{"type":"exit"}\n')
      const timer = setTimeout(() => child.kill('SIGTERM'), 5000)
      const code = await exit; clearTimeout(timer)
      await rm(profile,{recursive:true,force:true})
      return code
    },
  }
}

test('actual native controller pairs, stores authority, publishes real DTOs, disables and revokes through workerd', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY, 'run npm run test:native to build the actual native probe')
  const app = await runtime()
  const probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  try {
    assert.equal((await probe.ready()).ready, true)
    const browser = await app.owner()
    const begin = await probe.send({ type: 'begin', name: 'Native synthetic bench' })
    assert.equal(begin.ok, true); assert.equal(begin.status.phase, 'pairing')
    const stationId = begin.status.pairingId
    await browser.post('pair/claim', { code: begin.status.pairingCode })
    const check = await probe.send({ type: 'refresh' })
    assert.equal(check.status.accountId, browser.accountId)
    const approve = { type: 'approve', enrollmentId: stationId, accountId: browser.accountId }
    // Typing a code is not agreeing to attach the station. Approving at the shack first must be
    // refused with a reason the operator can act on - not "serviceUnavailable" - and create nothing.
    const early = await probe.send(approve)
    assert.equal(early.ok, false); assert.equal(early.error, 'awaitingConfirmation')
    assert.equal((await app.db.prepare('SELECT COUNT(*) AS count FROM stations WHERE id=?').bind(stationId).first()).count, 0)
    await browser.post('pair/confirm', { id: stationId })
    await probe.send({ type: 'vaultFailure', enabled: true })
    const refused = await probe.send(approve)
    assert.equal(refused.ok, false); assert.equal(refused.error, 'credentialStoreUnavailable')
    assert.equal((await app.db.prepare('SELECT COUNT(*) AS count FROM stations WHERE id=?').bind(stationId).first()).count, 0)
    await probe.send({ type: 'vaultFailure', enabled: false })
    const paired = await probe.send(approve)
    // Approving the pairing turns Remote on (operator decision 2026-09-14).
    assert.equal(paired.ok, true); assert.ok(['connecting', 'connected', 'reconnecting'].includes(paired.status.phase), paired.status.phase)
    // This client never kept the confirm cookie, so it registers as a second, unapproved browser
    // beside the approved pairing browser. The list is ordered by id, so find it by id.
    const { value: request, response } = await browser.post(`stations/${stationId}/device`, { name: 'Native probe browser' })
    browser.setCookie(response.headers.get('set-cookie'))
    await browser.post(`stations/${stationId}/ticket`, {}, 403)
    const requests = await probe.send({ type: 'refresh' })
    assert.equal(requests.status.devices.find(d => d.id === request.deviceId)?.approved, 0)
    assert.equal((await probe.send({ type: 'device', deviceId: request.deviceId, approve: true, kind: 'browser' })).ok, true)
    assert.equal((await probe.send({ type: 'enable' })).ok, true)
    const namespace = await app.mf.getDurableObjectNamespace('STATIONS')
    const room = namespace.get(namespace.idFromName(stationId))
    for (let tries = 0; tries < 30 && !(await roomStatus(room)).online; tries++) await new Promise(resolve => setTimeout(resolve, 100))
    assert.equal((await roomStatus(room)).online, true)
    const { value: ticket } = await browser.post(`stations/${stationId}/ticket`)
    const socket = await browser.open(stationId, ticket.ticket)
    await socket.take(value => value.type === 'session')
    const first = await socket.take(value => value.type === 'observation')
    assert.equal(first.frame.source, 'native')
    assert.equal(first.frame.station.radio.rigDialMhz, 14.074)
    assert.equal(first.frame.station.radio.rigKeyed, false)
    assert.equal(first.frame.station.amplifier.outputWatts, 12)
    assert.ok(first.frame.station.radio.readings.dial.ageMs < 3000)
    socket.send({ type: 'ack', epoch: first.frame.epoch, sequence: first.frame.sequence })
    socket.ackObservations()
    const second = await socket.take(value => value.type === 'observation')
    assert.ok(second.frame.sequence > first.frame.sequence)
    assert.equal(socket.closed,false,'observation must still be live before the local disable action')
    assert.equal((await probe.send({ type: 'disable' })).ok, true)
    await socket.take(value => value.type === 'closed')
    const restarted = await probe.send({ type: 'restart' })
    assert.equal(restarted.status.stationId, stationId)
    assert.equal(restarted.status.phase, 'disabled')
    assert.equal((await roomStatus(room)).online, false)
    const forgotten = await probe.send({ type: 'forget' })
    assert.equal(forgotten.ok, true, JSON.stringify(forgotten)); assert.equal(forgotten.status.stationId, null)
    assert.equal((await app.db.prepare('SELECT enabled FROM stations WHERE id=?').bind(stationId).first()).enabled, 0)
    assert.equal(await probe.stop(), 0, 'native probe must exit successfully')
  } finally { try { await probe.stop() } finally { await app.mf.dispose() } }
})


// v4 gets a bigger clock than the rest, and it is a BUDGET, not a slackened assertion. v2-v3 run
// 4-6 s; v4 runs the stop-anything section (three local keyings, each behind the relay's 1.1 s Stop
// window), the preference save, and the whole FT8+FT4 settings/CQ/runtime sweep. MEASURED on an
// idle box (load 2.55): 61.9 s against the old 60 s — it was failing by 3%. That clock was set when
// the v4 block was a fraction of its current size, and everything since was added while this file
// could not run at all (a stray quote made it a SyntaxError from 2dfadeb3 until it was fixed), so
// nothing ever re-timed it. 180 s is ~3x the idle measurement, which leaves room for a loaded
// machine while still catching a real hang. If v4 grows again, split it rather than raising this.
//
// This clock is NOT the only budget the block spends, and raising it exposed the other one. The
// relay gives a browser a 60 s observation lease (OBSERVER_LEASE_MS), and this harness never
// renewed: MEASURED on an idle box, the main observe socket was open 61.2 s, so the lease lapsed
// 1.2 s before the block ended and killed whichever request was in flight with `browserAccessEnded`
// (the page socket was at 20.3 s, a third of the way to the same cliff). `observing` below renews
// it on the live client's own 30 s wall clock, which puts the budget on the GAP between renewals
// (measured max 30.3 s against 60 s) instead of on the length of the block, so adding operations
// here no longer walks toward that cliff. The node clock above still bounds the whole run.
// v1 is not run: it carries no station control, and with the remote logging permission gone nothing
// admits a v1 browser to a lease.
for (const operationVersion of [2, 3, 4]) test(`actual cloud and native operations v${operationVersion} hold the lease, take Stop and refuse local takeover`, {timeout:operationVersion>=4?180000:60000},async()=>{
 assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
 const app=await runtime(),probe=await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY,app.origin)
 let socket
 const renewals=new Map()
 try{
  await probe.ready();const browser=await app.owner();const begin=await probe.send({type:'begin',name:'Logging synthetic bench'}),stationId=begin.status.pairingId
  // One approval (operator decisions 2026-09-13/14): this browser confirms the pairing, so approving
  // the pairing approves it and turns Remote on. Enable again for a fresh authority, as before.
  // The confirming browser's key, pinned with the pairing (A5, ruling D4): what it sends carries a proof (S1-M1).
  const key=await deviceKey();let session=null
  await browser.post('pair/claim',{code:begin.status.pairingCode});const confirmed=await browser.post('pair/confirm',{id:begin.status.pairingId,publicKey:key.publicKey});browser.setCookie(confirmed.response.headers.get('set-cookie'));await probe.send({type:'refresh'});assert.equal((await probe.send({type:'approve',enrollmentId:stationId,accountId:browser.accountId})).ok,true)
  const {value:device}=await browser.post(`stations/${stationId}/device`,{name:'Logging browser'});assert.equal(device.approved,true,'the pairing browser is approved with the station');await probe.send({type:'refresh'});await probe.send({type:'enable'})
  const roomNamespace=await app.mf.getDurableObjectNamespace('STATIONS'),room=roomNamespace.get(roomNamespace.idFromName(stationId));for(let i=0;i<30&&!(await roomStatus(room)).online;i++)await delay(100);assert.equal((await roomStatus(room)).online,true)
  assert.deepEqual(await probe.send({type:'seedLogging'}),{count:0,adif:'',txEnabled:false})
  // A live browser renews its observation on a WALL CLOCK: every 30 s against the relay's 60 s
  // lease (ui/src/remote-web/client.ts sets that interval; relay.ts OBSERVER_LEASE_MS sets the
  // lease). Nothing in this harness ever renewed, so the lease held only because the block finished
  // inside 60 s — and the operations added for the satellite and program parity work pushed v4 past
  // it, killing whichever request happened to be in flight with `browserAccessEnded`. Renew on the
  // client's own clock, through the SERVICE's real renew route: a lapsed entitlement, a revoked
  // device or a dead session still refuses it and the lease still ends, so expiry stays observable.
  // A timer rather than a hook on `operation` is the point — the lease must not be something the
  // next operation added below can spend, or this comes back the moment the block grows again.
  const observing=(peer,sessionId)=>{const timer=setInterval(()=>{void browser.post(`stations/${stationId}/renew`,{sessionId}).catch(()=>{})},30000);timer.unref?.();renewals.set(peer,timer)}
  const leaving=peer=>{clearInterval(renewals.get(peer));renewals.delete(peer);peer.close()}
  const ticket=(await browser.post(`stations/${stationId}/ticket`)).value;socket=await browser.open(stationId,ticket.ticket);socket.ackObservations();observing(socket,session=(await socket.take(v=>v.type==='session')).sessionId)
  const envelope=request=>({type:'operationRequest',...(operationVersion>=2?{operationVersion}:{}),request})
  const signed=message=>key.prove(message,{stationId,deviceId:device.deviceId,sessionId:session})
  const operation=async args=>{await delay(270);const request={requestId:crypto.randomUUID(),...args};socket.send(await signed(envelope(request)));
    try{return {request,response:await socket.take(v=>v.type==='operationResponse'&&v.requestId===request.requestId)}}
    catch(error){throw new Error(`operation ${request.type}/${request.action?.action??''} v${operationVersion} timed out; socket closed=${socket.closed} code=${socket.closeCode} reason=${socket.closeReason}`,{cause:error})}}
  // stationBusy means the request was refused before anything changed, so an operator clicks again. A request
  // the device is allowed to make retries once; `again` re-reads state first when the request carries a command
  // window. Any other refusal, or a second stationBusy, stands. Refusals this case expects
  // (localPermissionRequired, staleContext) are never asked with a state read afresh, which would answer
  // something else: the stale FT command is asked again exactly as it was.
  // ⚠️ It does NOT only mean "the native Engine was held", which this comment used to claim. TWO unrelated
  // producers send the same string: `operations.rs` when the Engine `try_lock` fails, and `transport.rs` when
  // another operation is already in flight on this connection, which never touches the Engine. Nothing on the
  // wire tells them apart. The REPLAY below is deliberately not wrapped in this: a duplicate request must come
  // back from its receipt, and as of the fix in `operations.rs` that no longer needs the Engine at all.
  const allowed=async(first,again=first)=>{let sent=await first();if(sent.response.error==='stationBusy'){console.log('stationBusy, retrying once:',sent.request.type,sent.request.action?.action??'');await delay(300);sent=await again()}return sent}

  // The approval granted station control; Turn on restores it once the service lists
  // the browser. Then RESTRICT the browser with the switch at the shack: that is what must still
  // refuse it a lease, exactly as a browser without permission always was.
  for(let i=0;i<50;i++){const {status}=await probe.send({type:'status'});if(status.stationPermissions?.includes(device.deviceId))break;await delay(100)}
  let state=(await allowed(()=>operation({type:'state'}))).response.value;assert.equal(state.phase,'available','the approval allowed this browser')
  const restricted=await probe.send({type:'stationPermission',deviceId:device.deviceId,allow:false});assert.equal(restricted.ok,true,restricted.error)
  state=(await allowed(()=>operation({type:'state'}))).response.value;assert.equal(state.phase,'localPermissionRequired');assert.equal((await operation({type:'acquire',stationBootId:state.stationBootId})).response.error,'localPermissionRequired')
  await probe.send({type:'refresh'});const permission=await probe.send({type:'stationPermission',deviceId:device.deviceId,allow:true});assert.equal(permission.ok,true,permission.error);assert.deepEqual(permission.status.stationPermissions,[device.deviceId])
  state=(await allowed(()=>operation({type:'acquire',stationBootId:state.stationBootId}))).response.value;assert.equal(state.phase,'controlling');assert.equal(state.txArmed,false)
  const heartbeat=()=>allowed(()=>operation({type:'heartbeat',leaseId:state.leaseId}))
  const evidence=await probe.send({type:'loggingEvidence'});assert.deepEqual(evidence,{count:0,adif:'',txEnabled:false})
  if(operationVersion>=2){
   assert.equal((await probe.send({type:'stationPermission',deviceId:device.deviceId,allow:true})).ok,true)
   const controls=(await allowed(()=>operation({type:'state'}))).response.value
   assert.deepEqual(controls.controls.capabilities,operationVersion===4?['audioListen']:[])
   if (operationVersion === 4) {
     // Stop anything (operator decision 2026-09-14): station control alone holds the stop token, and a
     // browser's Stop unkeys a transmission started at the shack. Key locally through the probe (the
     // engine's own local verbs), stop through the actual cloud/native path, read the engine back.
     assert.match(controls.transmitEpoch, /^[0-9a-f]{16}$/, 'station control alone holds the stop token')
     // The relay admits at most two Stops per session in its 1 s window (operation-relay, unchanged);
     // an operator does not press Stop three times a second, so each round waits the window out.
     const stopWindow = () => delay(1100)
     const stopLocal = async (kind, label) => {
       await stopWindow()
       const keyed = await probe.send({type:'keyLocal',kind})
       assert.equal(keyed.keyed, true, `positive control: the local ${kind} transmission is live (${label})`)
       // A heartbeat, not a plain state read: this harness does not heartbeat on its own, and the waits
       // above outlast the 5 s lease otherwise (the browser client heartbeats every second).
       const current = (await heartbeat()).response.value
       assert.equal(current?.phase, 'controlling', `positive control: the lease is held (${label})`)
       assert.match(current.transmitEpoch, /^[0-9a-f]{16}$/, `station control holds the stop token (${label})`)
       const stopped = await operation({type:'stopTransmit',stationBootId:current.stationBootId,leaseId:current.leaseId,transmitEpoch:current.transmitEpoch})
       assert.deepEqual(stopped.response.value,{stop:'accepted'},`${kind} ${label}: ${JSON.stringify(stopped.response)}`)
       let after
       for(let i=0;i<40;i++){after=await probe.send({type:'keyEvidence'});if(!after.keyed)break;await delay(50)}
       assert.deepEqual(after,{keyed:false,manualPtt:false,tuning:false,txEnabled:false},`${kind} ${label}: unkeyed at the station`)
     }
     for (const kind of ['ptt','tune','cw']) await stopLocal(kind,'station control only')
     // The owner's Stop and its replay below are two more Stops: give them a window of their own, and
     // renew the lease the wait would otherwise outlast.
     await stopWindow()
     await heartbeat()
     // Stop anything (operator decision 2026-09-14): from v4 the stop token goes to any browser
     // holding station control; versions 1-3 carry no token at all. This line used to assert null
     // unconditionally, which contradicted the same object's assertion above.
     if (operationVersion >= 4) assert.match(controls.transmitEpoch, /^[0-9a-f]{16}$/)
     else assert.equal(controls.transmitEpoch, null)
     const owner = (await allowed(()=>operation({type:'state'}))).response.value
     assert.match(owner.transmitEpoch,/^[0-9a-f]{16}$/)
     const stop = {type:'stopTransmit',stationBootId:owner.stationBootId,leaseId:owner.leaseId,transmitEpoch:owner.transmitEpoch}
     assert.deepEqual((await operation(stop)).response.value,{stop:'accepted'})
     // Stop TX is never refused (operator ruling, 2026-10-03): the same Stop again, its token now
     // retired, is still a Stop. It was refused `staleContext` under the 2026-09-15 rule.
     assert.deepEqual((await operation(stop)).response.value,{stop:'accepted'})
     const after = (await allowed(()=>operation({type:'state'}))).response.value
     assert.notEqual(after.transmitEpoch,owner.transmitEpoch)
     assert.equal(after.phase,'controlling')
     assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
   }
  }
  const current=(await heartbeat()).response.value;assert.equal(current.phase,'controlling')
  // End remote control (operator decision 2026-10-10) drops everyone and holds until Remote is turned on
  // again: the browser's socket is closed by the station going offline, and nothing reaches the log.
  assert.equal((await probe.send({type:'takeOverLogging'})).ok,true)
  for(let i=0;i<100&&!socket.closed;i++)await delay(100)
  assert.equal(socket.closed,true,'End remote control left the browser connected');assert.equal(socket.closeReason,'stationOffline')
  for(let i=0;i<50&&(await roomStatus(room)).online;i++)await delay(100)
  assert.equal((await roomStatus(room)).online,false,'End remote control left the station online');assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
  leaving(socket)
  // Turned on again, the approved browser has what its approval gave, with no approval again.
  await probe.send({type:'enable'});for(let i=0;i<30&&!(await roomStatus(room)).online;i++)await delay(100);assert.equal((await roomStatus(room)).online,true)
  let restored=null;for(let i=0;i<100&&!restored;i++){const {status}=await probe.send({type:'status'});if(status.stationPermissions?.includes(device.deviceId))restored=status;else await delay(100)}
  assert.ok(restored,'the browser\'s station control did not come back at Turn on')
  const again=(await browser.post(`stations/${stationId}/ticket`)).value;socket=await browser.open(stationId,again.ticket);socket.ackObservations();observing(socket,session=(await socket.take(v=>v.type==='session')).sessionId)
  leaving(socket);socket=null
  // Remote remembers being on (operator decision 2026-09-13). Restarting the actual native controller
  // turns Remote back on and gives station control back to a browser the
  // actual service still lists at the same approval. No lease survives the restart, the TX-enable
  // latch is off, and a restored grant arms nothing on a fresh lease.
  if(operationVersion===4){
   assert.equal((await probe.send({type:'stationPermission',deviceId:device.deviceId,allow:true})).ok,true)
  }
  await delay(200)
  let restarted=await probe.send({type:'restart'});assert.notEqual(restarted.status.phase,'disabled');assert.ok(restarted.status.observationGeneration)
  for(let i=0;i<50&&!restarted.status.stationPermissions.includes(device.deviceId);i++){await delay(100);restarted=await probe.send({type:'status'})}
  assert.deepEqual(restarted.status.stationPermissions,[device.deviceId]);assert.equal(restarted.status.controller,null)
  const idle=await probe.send({type:'ftEvidence'});assert.equal(idle.txEnabled,false,'the TX-enable latch is off after the restart');assert.equal(idle.owned,false)
  if(operationVersion===4){
   assert.deepEqual(restarted.status.stationPermissions,[device.deviceId])
   for(let i=0;i<50&&restarted.status.phase!=='connected';i++){await delay(100);restarted=await probe.send({type:'status'})}
   for(let i=0;i<30&&!(await roomStatus(room)).online;i++)await delay(100)
   const again=(await browser.post(`stations/${stationId}/ticket`)).value;socket=await browser.open(stationId,again.ticket);socket.ackObservations();observing(socket,session=(await socket.take(v=>v.type==='session')).sessionId)
   assert.equal((await probe.send({type:'seedFt',tier:'FT8'})).txEnabled,false)
   const fresh=(await allowed(()=>operation({type:'state'}))).response.value
   const leased=(await allowed(()=>operation({type:'acquire',stationBootId:fresh.stationBootId}))).response.value;assert.equal(leased.phase,'controlling')
   const beat=(await allowed(()=>operation({type:'heartbeat',leaseId:leased.leaseId}))).response.value
   assert.deepEqual(beat.controls.capabilities,['audioListen'],'the restored grant is station control, and offers no FT8/FT4')
   assert.match(beat.transmitEpoch,/^[0-9a-f]{16}$/);assert.equal(beat.txArmed,false,'a restored grant and a fresh lease arm nothing')
   const armed=await probe.send({type:'ftEvidence'});assert.equal(armed.txEnabled,false);assert.equal(armed.owned,false)
  }
  assert.equal(await probe.stop(),0,'native probe must exit successfully')
 }finally{for(const timer of renewals.values())clearInterval(timer);socket?.close();try{await probe.stop()}finally{await app.mf.dispose()}}
})
