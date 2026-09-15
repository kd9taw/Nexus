import { test } from 'node:test'
import { readFile, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import assert from 'node:assert/strict'
import { tempoConversations } from './tempo-fixture.mjs'
import { setTimeout as delay } from 'node:timers/promises'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { runtime, roomStatus } from './runtime.mjs'
import { recallReference, recallAdif } from './recall-reference.mjs'
import { insightsReference, insightsAdif } from './insights-reference.mjs'

for (const tier of ['FT8', 'FT4']) for (const prompt of [false, true]) test(`actual cloud ${tier} QSO exchange finishes with durable ${prompt ? 'confirmed' : 'current'} logging`, { timeout: 90000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
  const app = await runtime(), probe = await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
  let socket
  try {
    await probe.ready()
    const browser = await app.owner(), begin = await probe.send({ type: 'begin', name: 'FT QSO synthetic bench' })
    const stationId = begin.status.pairingId
    await browser.post('pair/claim', { code: begin.status.pairingCode }); await browser.post('pair/confirm', { id: stationId }); await probe.send({ type: 'refresh' })
    assert.equal((await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId })).ok, true)
    const { value: device, response } = await browser.post(`stations/${stationId}/device`, { name: 'FT logging browser' })
    browser.setCookie(response.headers.get('set-cookie')); await probe.send({ type: 'refresh' })
    await probe.send({ type: 'device', deviceId: device.deviceId, approve: true }); await probe.send({ type: 'enable' })
    const ns = await app.mf.getDurableObjectNamespace('STATIONS'), room = ns.get(ns.idFromName(stationId))
    for (let i = 0; i < 30 && !(await roomStatus(room)).online; i++) await delay(100)
    assert.equal((await roomStatus(room)).online, true)
    await probe.send({ type: 'refresh' })
    for (const type of ['loggingPermission', 'stationPermission', 'transmitPermission']) {
      const result = await probe.send({ type, deviceId: device.deviceId, allow: true })
      assert.equal(result.ok, true, `${type}: ${result.error}`)
    }
    const ticket = (await browser.post(`stations/${stationId}/ticket`)).value
    socket = await browser.open(stationId, ticket.ticket); socket.ackObservations(); await socket.take(v => v.type === 'session')
    const send = async args => {
      await delay(270)
      const request = { requestId: crypto.randomUUID(), ...args }
      socket.send({ type: 'operationRequest', operationVersion: 4, request })
      return { request, response: await socket.take(v => v.type === 'operationResponse' && v.requestId === request.requestId) }
    }
    // stationBusy means the native Engine was held and the request was refused before anything
    // changed, so an operator clicks again: retry once (each attempt re-reads its state). Any other
    // refusal, or a second stationBusy, still fails the case.
    const settle = async attempt => {
      let sent = await attempt()
      if (sent.response.error === 'stationBusy') {
        console.log('stationBusy, retrying once:', sent.request.type, sent.request.action?.action ?? '')
        await delay(300)
        sent = await attempt()
      }
      assert.equal(sent.response.error, undefined, JSON.stringify(sent.response))
      return { request: sent.request, value: sent.response.value }
    }
    const operation = args => settle(() => send(args))
    const initial = (await operation({ type: 'state' })).value
    const owner = (await operation({ type: 'acquire', stationBootId: initial.stationBootId })).value
    const action = a => settle(async () => {
      const state = (await operation({ type: 'heartbeat', leaseId: owner.leaseId })).value
      assert.equal(state.phase, 'controlling')
      return send({ type: 'stationControl', stationBootId: state.stationBootId, leaseId: state.leaseId,
        expectedRevision: state.revision, commandWindowId: state.commandWindowId, clientSequence: state.nextSequence,
        context: state.controls.context, action: a.action.startsWith('ft.') ? { ...a, transmitEpoch: state.transmitEpoch } : a })
    })
    let count = 0
    {
      await probe.send({ type: 'seedFtQso', tier, prompt })
      const selection = await probe.send({ type: 'ftCallDecode' })
      const called = await action({ action: 'ft.call', expectedTier: tier, selection })
      assert.equal(called.value.outcome, 'applied')
      assert.ok((await probe.send({ type: 'ftQsoStep', slot: 10 })).samples > 0)
      const report = await probe.send({ type: 'ftQsoStep', slot: 13, message: 'K2DEF W1AW -12' })
      assert.equal(report.qso.dxcall, 'W1AW')
      assert.equal(report.qso.rxReport, -12)
      assert.match(report.qso.txNow, /R[+-][0-9]{2}/)
      assert.ok((await probe.send({ type: 'ftQsoStep', slot: 14 })).samples > 0)
      const roger = await probe.send({ type: 'ftQsoStep', slot: 17, message: 'K2DEF W1AW RR73' })
      assert.match(roger.qso.txNow, /73/)
      assert.ok((await probe.send({ type: 'ftQsoStep', slot: 18 })).samples > 0)
      const before = await probe.send({ type: 'ftQsoEvidence' }), q = before.qso
      const logged = await action({ action: 'qso.logCurrent', expectedKey: before.currentQsoLogKey, expectedTier: tier,
        expectedQso: { dxcall: q.dxcall, state: q.state, txNow: q.txNow, cqRunning: q.cqRunning } })
      assert.equal(logged.value.outcome, 'applied', JSON.stringify(logged.value))
      assert.equal(logged.value.evidence, prompt ? 'pendingConfirmationSynced' : 'fileSynced')
      let after = await probe.send({ type: 'ftQsoEvidence' })
      if (prompt) {
        assert.equal(after.journal, true)
        assert.equal(after.records.length, count)
        assert.equal(after.pendingLog.call, 'W1AW')
        const record = after.pendingLog
        const confirmed = await action({ action: 'qso.confirm', expectedKey: after.pendingQsoLogKey,
          edits: { call: record.call, grid: record.grid, rstSent: record.rstSent, rstRcvd: record.rstRcvd } })
        assert.equal(confirmed.value.outcome, 'applied', JSON.stringify(confirmed.value))
        assert.equal(confirmed.value.evidence, 'fileSynced')
        assert.deepEqual((await operation({ type: 'result', operationId: confirmed.request.requestId })).value, confirmed.value)
        after = await probe.send({ type: 'ftQsoEvidence' })
      }
      count++
      assert.equal(after.records.length, count)
      assert.equal(after.journal, false)
      assert.equal(after.pendingLog, null)
      assert.match(after.adif, /W1AW/)
      assert.match(after.adif, /-12/)
      assert.deepEqual((await operation({ type: 'result', operationId: logged.request.requestId })).value, logged.value)
      assert.equal((await probe.send({ type: 'ftQsoEvidence' })).adif, after.adif)
    }
  } finally {
    socket?.close()
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

// One approval through workerd (operator decisions 2026-09-13 and 2026-09-14): approving the pairing
// turns Remote on and grants the browser that confirmed it station control, logging and, when ticked,
// FT8/FT4 transmit. All three come back after a restart with the TX-enable latch still off, and a
// later Turn off Remote is remembered.
test('actual native one approval: pairing turns Remote on, grants the pairing browser, keeps transmit across a restart with TX off', { timeout: 60000 }, async () => {
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
    const paired = await probe.send({ type: 'approve', enrollmentId: stationId, accountId: browser.accountId, transmit: true })
    assert.equal(paired.ok, true, paired.error)
    assert.ok(['connecting', 'connected', 'reconnecting'].includes(paired.status.phase), paired.status.phase)
    const { value: device } = await browser.post(`stations/${stationId}/device`, { name: 'Not asked for' })
    assert.equal(device.approved, true, 'the pairing browser was approved with the station')
    const granted = async label => {
      for (let i = 0; i < 100; i++) {
        const { status } = await probe.send({ type: 'status' })
        if ([status.stationPermissions, status.loggingPermissions, status.transmitPermissions].every(p => p?.includes(device.deviceId))) return status
        await delay(100)
      }
      assert.fail(`${label}: the pairing browser's grants never appeared`)
    }
    await granted('after the approval')
    assert.equal((await probe.send({ type: 'ftEvidence' })).txEnabled, false)
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
    assert.deepEqual(status.transmitPermissions, [])
  } finally {
    try { await probe.stop() } finally { await app.mf.dispose() }
  }
})

async function nativeProbe(binary, origin) {
  const profile=await mkdtemp(join(tmpdir(),'nexus-native-profile-'))
  const child = spawn(binary, ['--ignored', '--exact', 'remote_service::tests::cloud_runtime_probe', '--nocapture'], { stdio: ['pipe', 'pipe', 'pipe'], env:{...process.env,XDG_CONFIG_HOME:profile,APPDATA:profile,NEXUS_DATA_DIR:join(profile,'shared'),NEXUS_PROFILE:''} })
  const queue = [], waiting = []
  // Never forward raw probe output: its pipe includes the one-time pairing code.
  child.stderr.resume()
  let exited = false
  const exit = new Promise(resolve => {
    const finish = code => {
      exited = true
      for (const waiter of waiting.splice(0)) { clearTimeout(waiter.timer); waiter.reject(new Error('native probe exited')) }
      resolve(code)
    }
    child.once('exit', finish)
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
  async function receive() {
    if (queue.length) return queue.shift()
    if (exited) throw new Error('native probe exited')
    return new Promise((resolve, reject) => {
      const waiter = { resolve, reject, timer: null }
      waiter.timer = setTimeout(() => { waiting.splice(waiting.indexOf(waiter), 1); reject(new Error('native probe response timed out')) }, 15000)
      waiting.push(waiter)
    })
  }
  return {
    ready: receive,
    async send(action) { child.stdin.write(JSON.stringify(action) + '\n'); return receive() },
    async stop() {
      if (!exited) child.stdin.end('{"type":"exit"}\n')
      const timer = setTimeout(() => child.kill('SIGTERM'), 5000)
      const code = await exit; clearTimeout(timer)
      await rm(profile,{recursive:true,force:true})
      assert.equal(code, 0, 'native probe must exit successfully')
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
    assert.equal((await probe.send({ type: 'device', deviceId: request.deviceId, approve: true })).ok, true)
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
    socket.send({ type: 'applicationHello' })
    const capabilities = await socket.take(value => value.type === 'applicationCapabilities')
    assert.equal(capabilities.version, 1)
    assert.deepEqual(capabilities.commands, ['get_snapshot', 'get_settings', 'get_band_plan', 'get_spectrum_row', 'get_meters'])
    let originalTier
    for (const command of capabilities.commands) {
      const requestId = crypto.randomUUID()
      socket.send({ type: 'applicationRead', requestId, command, revision: null })
      const result = await socket.take(value => value.requestId === requestId)
      assert.equal(result.type, 'applicationResult', `${command} must return the actual native DTO`)
      assert.equal(result.command, command)
      assert.equal(result.baseRevision, null)
      assert.ok(result.ageMs < 3000)
      if (command === 'get_snapshot') {
        originalTier = result.data.link.tier
        assert.equal(result.data.mycall, 'N0CALL')
        assert.equal(result.data.radio.txEnabled, false)
        assert.ok(Array.isArray(result.data.stations))
      } else if (command === 'get_settings') {
        assert.equal(result.data.mycall, 'N0CALL')
        assert.equal(result.data.mygrid, 'AA00')
        for (const privateKey of ['radioProfiles','radios','qrzPassword','cloudlogKey','ampPort']) {
          assert.equal(Object.hasOwn(result.data, privateKey), false, 'unreviewed settings must stay at the shack')
        }
      } else if (command === 'get_band_plan') assert.ok(result.data.length > 0)
      else if (command === 'get_meters') assert.deepEqual(Object.keys(result.data).sort(), ['cwToneHz','rxLevel','smeterDb'])
      else assert.deepEqual(Object.keys(result.data).sort(), ['hiHz','loHz','row','source'])
      socket.send({ type: 'applicationAck', requestId })
    }
    for (const tier of ['TempoFast','TempoDeep']) {
      const native = await probe.send({type:'seedTempo',tier,conversations:tempoConversations(tier)})
      await delay(550) // The existing snapshot producer shares one sample per 500 ms.
      const requestId=crypto.randomUUID()
      socket.send({type:'applicationRead',requestId,command:'get_snapshot',revision:null})
      const result=await socket.take(value=>value.requestId===requestId)
      assert.equal(result.type,'applicationResult')
      assert.equal(result.data.link.tier,tier)
      assert.deepEqual(result.data.conversations,native.conversations,'the original v1 stream preserves every native delivery field and legacy message')
      assert.ok(native.conversations.some(c=>c.messages.some(m=>m.delivered)))
      assert.ok(native.conversations.some(c=>c.messages.some(m=>m.confirmed)))
      assert.equal(result.data.radio.txEnabled,false)
      socket.send({type:'applicationAck',requestId})
    }
    await probe.send({type:'seedTempo',tier:originalTier,conversations:[]})
    const { value: streamTicket } = await browser.post(`stations/${stationId}/ticket`)
    const stream = await browser.open(stationId, streamTicket.ticket)
    await stream.take(value => value.type === 'session')
    stream.send({ type: 'applicationHello', version: 2 })
    const streaming = await stream.take(value => value.type === 'applicationCapabilities')
    assert.equal(streaming.version, 2, 'the real native handshake advertises the stream extension')
    assert.equal(streaming.commands.length, 7)
    let requestId = crypto.randomUUID()
    stream.send({ type: 'applicationSubscribe', topics: streaming.commands, requestId })
    const received = new Set(), values = new Map()
    for (let batch = 0; batch < 25 && (batch < 3 || received.size < streaming.commands.length); batch++) {
      const frame = await stream.take(value => value.type === 'applicationFrame')
      assert.equal(frame.requestId, requestId)
      for (const update of frame.updates) {
        if (update.type === 'applicationError') { assert.equal(update.error, 'applicationBusy'); continue }
        assert.equal(update.type, 'applicationResult', `${update.command} returns a real native sample`)
        assert.ok(update.ageMs < 3000)
        const previous = values.get(update.command)
        if (update.baseRevision !== null) assert.equal(previous?.revision, update.baseRevision)
        const data = update.baseRevision === null ? update.data : { ...previous.data, ...update.data }
        for (const key of update.removed) delete data[key]
        values.set(update.command, { revision: update.revision, data })
        received.add(update.command)
        if (update.command === 'get_scope_snapshot') assert.deepEqual(Object.keys(data).sort(), ['hiHz', 'loHz', 'row', 'source'])
        if (update.command === 'get_cw_state') {
          assert.equal(typeof data.text, 'string'); assert.ok(Array.isArray(data.sent))
        }
      }
      const nextRequestId = crypto.randomUUID()
      stream.send({ type: 'applicationFrameAck', requestId, nextRequestId })
      requestId = nextRequestId
    }
    assert.deepEqual([...received].sort(), [...streaming.commands].sort())
    stream.send({ type: 'applicationSubscribe', topics: [], requestId: null })
    stream.close()
    const { value: queryTicket } = await browser.post(`stations/${stationId}/ticket`)
    const query = await browser.open(stationId, queryTicket.ticket)
    await query.take(value => value.type === 'session')
    query.send({ type: 'applicationHello', version: 3 })
    const queryCapabilities = await query.take(value => value.type === 'applicationCapabilities')
    assert.equal(queryCapabilities.version, 3)
    assert.ok(queryCapabilities.commands.includes('get_remote_page'))
    for (const collection of ['entities', 'log', 'decodes']) {
      let cursor = null, count = 0, snapshotId = null
      do {
        const requestId = crypto.randomUUID()
        query.send({ type: 'applicationQuery', requestId, collection, cursor, search: '', unconfirmed: false, after: null })
        const page = await query.take(value => value.requestId === requestId)
        assert.equal(page.type, 'applicationPage', 'real native producer must decode and answer the closed query envelope')
        assert.equal(page.offset, count)
        if (snapshotId) assert.equal(page.snapshotId, snapshotId)
        snapshotId = page.snapshotId; count += page.rows.length; cursor = page.nextCursor
        assert.ok(page.rows.length <= 128 && page.retained <= 3000)
        query.send({ type: 'applicationQueryAck', requestId })
      } while (cursor)
      if (collection === 'entities') assert.ok(count > 300, 'positive control: actual station DXCC table crossed multiple pages')
    }
    query.close()
    const keyboardBefore = await probe.send({ type: 'keyboardState' })
    const { value: keyboardTicket } = await browser.post(`stations/${stationId}/ticket`)
    const keyboard = await browser.open(stationId, keyboardTicket.ticket)
    await keyboard.take(value => value.type === 'session')
    keyboard.send({ type: 'applicationHello', version: 5 })
    const keyboardCapabilities = await keyboard.take(value => value.type === 'applicationCapabilities')
    assert.equal(keyboardCapabilities.version, 5, 'actual native headers and room admission negotiate keyboard samples')
    assert.equal(keyboardCapabilities.commands.length, 11)
    let keyboardCredit = crypto.randomUUID()
    keyboard.send({ type: 'applicationSubscribe', topics: ['get_rtty_state', 'get_psk_state'], requestId: keyboardCredit })
    const keyboardReceived = new Set()
    for (let batch = 0; batch < 10 && keyboardReceived.size < 2; batch++) {
      const frame = await keyboard.take(value => value.type === 'applicationFrame')
      assert.equal(frame.requestId, keyboardCredit)
      for (const update of frame.updates) {
        if (update.type === 'applicationError') { assert.equal(update.error, 'applicationBusy'); continue }
        assert.equal(update.type, 'applicationResult')
        assert.ok(['get_rtty_state', 'get_psk_state'].includes(update.command))
        if (keyboardReceived.has(update.command)) continue
        assert.equal(update.baseRevision, null)
        const mode = update.command === 'get_rtty_state' ? 'rtty' : 'psk'
        assert.deepEqual(update.data, keyboardBefore[mode], 'wire result equals the native cockpit DTO')
        assert.equal(update.data.text, 'CQ W1AW')
        assert.deepEqual(update.data.charConf, [30, 30, 30, 30, 30, 30, 30])
        assert.equal(update.data.armed, true)
        assert.equal(update.data.sending, false)
        assert.equal(update.data.latched, false)
        keyboardReceived.add(update.command)
      }
      const nextRequestId = crypto.randomUUID()
      keyboard.send({ type: 'applicationFrameAck', requestId: keyboardCredit, nextRequestId })
      keyboardCredit = nextRequestId
    }
    assert.equal(keyboardReceived.size, 2)
    assert.equal(keyboardBefore.psk.mode, 'qpsk31')
    assert.equal(keyboardBefore.psk.reverse, true)
    keyboard.send({ type: 'applicationSubscribe', topics: [], requestId: null })
    assert.deepEqual(await probe.send({ type: 'keyboardState' }), keyboardBefore, 'remote reads do not alter decoder state, TX or contacts')
    keyboard.close()
    const { log } = await probe.send({ type: 'seedRecallLog', adif: recallAdif() })
    assert.ok(log.length > 40, 'the independent desktop reference must receive the seeded log')
    const reference = await recallReference()
    const { value: recallTicket } = await browser.post(`stations/${stationId}/ticket`)
    const recall = await browser.open(stationId, recallTicket.ticket)
    await recall.take(value => value.type === 'session')
    recall.send({ type: 'applicationHello', version: 5 })
    assert.ok((await recall.take(value => value.type === 'applicationCapabilities')).commands.includes('get_remote_recall'))
    for (const call of ['W1AW', 'W1AW/P', 'DL2ABC', 'JA1ABC', '000']) {
      const requestId = crypto.randomUUID()
      recall.send({ type: 'applicationQuery', requestId, collection: 'recall', cursor: null, search: call, unconfirmed: false, after: null })
      const page = await recall.take(value => value.requestId === requestId)
      assert.equal(page.type, 'applicationPage')
      const source = page.meta.source
      const { qsos, dupeThisBand: _dupe, ...history } = reference.callHistory(log, call, '20m', 'FT8', true)
      assert.deepEqual(source.history, history, `complete exact-call history: ${call}`)
      assert.deepEqual(source.slots, reference.entitySlots(log, source.entity), `desktop entity slots and unknown-band rules: ${call}`)
      assert.deepEqual(page.rows, [...qsos].sort((a, b) => b.whenUnix - a.whenUnix).slice(0, 20))
      for (const band of ['20m', '40M', '80m']) for (const mode of ['CW', 'FT8', 'FT4', 'SSB']) for (const match of [false, true]) {
        const dupe = source.workedBandModes.some(([b, m]) => b === band.toLowerCase() && (!match || m === mode))
        assert.equal(dupe, reference.callHistory(log, call, band, mode, match).dupeThisBand)
      }
      recall.send({ type: 'applicationQueryAck', requestId })
    }
    recall.close()
    const desktop = await probe.send({ type: 'seedRecallLog', adif: insightsAdif() })
    assert.ok(desktop.log.length > 2000, 'summary must cover contacts beyond the browser log window')
    assert.ok(desktop.awards.vucc.satWorked > 0 && desktop.awards.iota.cardConfirmed > 0, 'the independent award reference exercises satellite and card-only IOTA credit')
    const statsReference = await insightsReference()
    const { value: summaryTicket } = await browser.post(`stations/${stationId}/ticket`)
    const summaries = await browser.open(stationId, summaryTicket.ticket)
    await summaries.take(value => value.type === 'session')
    summaries.send({ type: 'applicationHello', version: 6 })
    assert.ok((await summaries.take(value => value.type === 'applicationCapabilities')).commands.includes('get_remote_insights'))
    for (const collection of ['awards', 'statistics']) {
      const requestId = crypto.randomUUID()
      summaries.send({ type: 'applicationQuery', requestId, collection, cursor: null, search: '', unconfirmed: false, after: null })
      const page = await summaries.take(value => value.requestId === requestId)
      assert.equal(page.type, 'applicationPage')
      const value = statsReference.parseInsights(page, collection)
      assert.equal(value.logCount, desktop.log.length)
      if (collection === 'awards') assert.deepEqual(value.awards, desktop.awards, 'same native award inputs, credits and satellite split')
      else {
        assert.deepEqual(value.statistics, statsReference.computeLogStats(desktop.log), 'existing desktop Statistics roll-up, including locale tie ordering')
        assert.deepEqual(value.geography, desktop.geography, 'existing geographic result')
      }
      assert.ok(!JSON.stringify(page).includes('synthetic note'), 'the summary carries no contact notes')
      summaries.send({ type: 'applicationQueryAck', requestId })
    }
    summaries.close()
    const dxpeditions = await probe.send({ type: 'seedDxpeditions' })
    const { value: dxTicket } = await browser.post(`stations/${stationId}/ticket`)
    const dx = await browser.open(stationId, dxTicket.ticket)
    await dx.take(value => value.type === 'session')
    dx.send({ type: 'applicationHello', version: 7 })
    assert.ok((await dx.take(value => value.type === 'applicationCapabilities')).commands.includes('get_remote_dxpeditions'))
    const dxRequest = crypto.randomUUID()
    dx.send({ type: 'applicationQuery', requestId: dxRequest, collection: 'dxpeditions', cursor: null, search: '', unconfirmed: false, after: null })
    const dxPage = await dx.take(value => value.requestId === dxRequest)
    assert.equal(dxPage.type, 'applicationPage')
    const board = statsReference.parseDxpeditions(dxPage)
    assert.deepEqual(board.dxpeditions, JSON.parse(dxpeditions.boardJson), 'the compiled native projection preserves every desktop wire field, including f32 scores')
    assert.equal(board.source, dxpeditions.source); assert.equal(board.asOf, dxpeditions.asOf)
    assert.equal(board.windows, null, 'a cold prediction cache remains absent')
    dx.send({ type: 'applicationQueryAck', requestId: dxRequest }); dx.close()
    const bank = JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/memories.json', import.meta.url), 'utf8'))
    assert.equal((await probe.send({ type: 'publishMemories', bank: JSON.stringify(bank) })).accepted, true)
    const { value: memoryTicket } = await browser.post(`stations/${stationId}/ticket`)
    const memories = await browser.open(stationId, memoryTicket.ticket)
    await memories.take(value => value.type === 'session')
    memories.send({ type: 'applicationHello', version: 8 })
    assert.ok((await memories.take(value => value.type === 'applicationCapabilities')).commands.includes('get_remote_memories'))
    for (const valid of [true, false, true]) {
      assert.equal((await probe.send({ type: 'publishMemories', bank: valid ? JSON.stringify(bank) : '{}' })).accepted, valid)
      const requestId = crypto.randomUUID()
      memories.send({ type: 'applicationQuery', requestId, collection: 'memories', cursor: null, search: '', unconfirmed: false, after: null })
      const page = await memories.take(value => value.requestId === requestId)
      if (valid) {
        assert.equal(page.type, 'applicationPage')
        assert.deepEqual(statsReference.parseMemories(page).bank, bank, 'native cache and compiled UI parser preserve every canonical bank field')
      } else { assert.equal(page.type, 'applicationQueryError'); assert.equal(page.error, 'applicationUnavailable') }
      memories.send({ type: 'applicationQueryAck', requestId })
    }
    memories.close()
    const { value: otaTicket } = await browser.post(`stations/${stationId}/ticket`)
    const ota = await browser.open(stationId, otaTicket.ticket)
    await ota.take(value => value.type === 'session')
    ota.send({ type: 'applicationHello', version: 9 })
    assert.ok((await ota.take(value => value.type === 'applicationCapabilities')).commands.includes('get_remote_ota'))
    for (const [age, missing] of [[0, null], [900, 'SOTA'], [0, null]]) {
      assert.equal((await probe.send({ type: 'seedOta', age, missing })).seeded, true)
      const requestId = crypto.randomUUID()
      ota.send({ type: 'applicationQuery', requestId, collection: 'ota', cursor: null, search: '', unconfirmed: false, after: null })
      const page = await ota.take(value => value.requestId === requestId)
      assert.equal(page.type, 'applicationPage')
      const value = statsReference.parseOta(page)
      assert.equal(value.feeds[0].status, age ? 'expired' : 'ready')
      assert.equal(value.feeds[1].status, missing ? 'unavailable' : 'ready')
      if (!age) {
        assert.equal(value.feeds[0].spots.length, 3)
        assert.equal(value.feeds[0].spots[1].newPark, false, 'station imported hunted parks remain worked')
        assert.equal(value.hunt.reference, 'US-0004')
        assert.equal(value.activation.reference, 'US-0001')
      }
      ota.send({ type: 'applicationQueryAck', requestId })
    }
    ota.close()
    const { value: fdTicket } = await browser.post(`stations/${stationId}/ticket`)
    const fd = await browser.open(stationId, fdTicket.ticket)
    await fd.take(value => value.type === 'session')
    fd.send({ type: 'applicationHello', version: 10 })
    assert.ok((await fd.take(value => value.type === 'applicationCapabilities')).commands.includes('get_remote_field_day'))
    for (const active of [false, true]) {
      if (active) assert.equal((await probe.send({type:'seedFieldDay'})).seeded,true)
      const requestId=crypto.randomUUID()
      fd.send({type:'applicationQuery',requestId,collection:'fieldDay',cursor:null,search:'',unconfirmed:false,after:null})
      const page=await fd.take(value=>value.requestId===requestId)
      assert.equal(page.type,'applicationPage')
      const value=statsReference.parseFieldDay(page)
      assert.equal(value.active,active)
      if(active){
        assert.equal(value.fieldDay.qsoCount,2)
        assert.equal(value.fieldDay.points,3)
        assert.equal(value.fieldDay.totalScore,106)
        assert.deepEqual(value.settings.fdBonusesPlanned,['natural-power'])
        assert.deepEqual(value.settings.fdBonuses,['emergency-power'])
      } else assert.equal(value.fieldDay,null)
      fd.send({type:'applicationQueryAck',requestId})
    }
    fd.close()
    const js8Fixture=JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/js8.json',import.meta.url),'utf8'))
    const nativeJs8=await probe.send({type:'seedJs8',journal:{inbox:js8Fixture.state.inbox,heard:js8Fixture.state.stations,allcallReplied:[],nextInboxId:2}})
    assert.equal(nativeJs8.state.inbox.length,1)
    assert.ok(nativeJs8.state.queue.length>1)
    const {value:js8Ticket}=await browser.post(`stations/${stationId}/ticket`)
    const js8=await browser.open(stationId,js8Ticket.ticket)
    await js8.take(value=>value.type==='session')
    js8.send({type:'applicationHello',version:11})
    const js8Caps=await js8.take(value=>value.type==='applicationCapabilities')
    assert.equal(js8Caps.version,11)
    assert.ok(js8Caps.commands.includes('get_js8_state'))
    let js8Request=crypto.randomUUID()
    js8.send({type:'applicationSubscribe',topics:['get_js8_state'],requestId:js8Request})
    const js8Frame=await js8.take(value=>value.type==='applicationFrame')
    const js8Sample=js8Frame.updates.find(update=>update.command==='get_js8_state')
    assert.equal(js8Sample.type,'applicationResult')
    assert.deepEqual(js8Sample.data.state,nativeJs8.state,'the station read is the unchanged native JS8 DTO')
    assert.deepEqual(statsReference.parseJs8Sample(js8Sample.data,0,js8Sample.data.capturedAtMs),nativeJs8.state)
    js8.send({type:'applicationFrameAck',requestId:js8Request,nextRequestId:crypto.randomUUID()})
    js8.send({type:'applicationSubscribe',topics:[],requestId:null})
    js8Request=crypto.randomUUID()
    js8.send({type:'applicationQuery',requestId:js8Request,collection:'js8Context',cursor:null,search:'',unconfirmed:false,after:null})
    const js8Page=await js8.take(value=>value.requestId===js8Request)
    assert.equal(js8Page.type,'applicationPage')
    const js8Context=statsReference.parseJs8Context(js8Page)
    for(const call of ['W1AW','K2ABC']){
      const history=reference.callHistory(nativeJs8.log,call,'')
      const last=history.qsos.length?history.qsos.reduce((a,b)=>b.whenUnix>a.whenUnix?b:a):null
      assert.deepEqual(js8Context.history[call],{count:history.count,lastUnix:history.lastUnix,
        grid:(last?.grid??'').trim(),name:(last?.name??'').trim(),comment:(last?.comment??'').trim()})
    }
    assert.ok(js8Context.plan.length>0)
    js8.send({type:'applicationQueryAck',requestId:js8Request});js8.close()
    const nativeModes=await probe.send({type:'seedStationModes'})
    const {value:modesTicket}=await browser.post(`stations/${stationId}/ticket`)
    const modes=await browser.open(stationId,modesTicket.ticket)
    modes.ackObservations()
    await modes.take(value=>value.type==='session')
    modes.send({type:'applicationHello',version:16})
    const modesCapabilities=await modes.take(value=>value.type==='applicationCapabilities')
    assert.equal(modesCapabilities.version,16,'the actual station advertises the lookups and alerts extensions')
    assert.ok(modesCapabilities.commands.includes('get_remote_navigation'))
    assert.ok(modesCapabilities.commands.includes('get_remote_parks'))
    assert.ok(modesCapabilities.commands.includes('get_remote_pounce'))
    const modesRequest=crypto.randomUUID()
    modes.send({type:'applicationSubscribe',topics:['get_sstv_state','get_remote_aprs_state'],requestId:modesRequest})
    const modesFrame=await modes.take(value=>value.type==='applicationFrame')
    const sstvSample=modesFrame.updates.find(u=>u.command==='get_sstv_state')
    const aprsSample=modesFrame.updates.find(u=>u.command==='get_remote_aprs_state')
    assert.equal(sstvSample.type,'applicationResult');assert.equal(aprsSample.type,'applicationResult')
    const sstvState=statsReference.parseSstvSample(sstvSample.data,0)
    assert.deepEqual({...sstvState,gallery:sstvState.gallery.map((g,i)=>({...g,path:nativeModes.sstv.gallery[i].path}))},nativeModes.sstv)
    assert.ok(sstvState.gallery.every(g=>/^[a-f0-9-]{36}\.(png|bmp)$/.test(g.path)))
    const liveAprs=statsReference.parseAprsLive(aprsSample.data,0)
    assert.deepEqual(liveAprs.health,nativeModes.aprs.health)
    assert.deepEqual(liveAprs.isStatus,nativeModes.aprs.isStatus)
    assert.deepEqual(liveAprs.settings,nativeModes.aprs.settings)
    modes.send({type:'applicationFrameAck',requestId:modesRequest,nextRequestId:crypto.randomUUID()})
    modes.send({type:'applicationSubscribe',topics:[],requestId:null})
    const collectionSource={page:async args=>{
      await delay(140)
      const requestId=crypto.randomUUID()
      modes.send({type:'applicationQuery',requestId,...args})
      const page=await modes.take(value=>value.requestId===requestId)
      assert.equal(page.type,'applicationPage')
      modes.send({type:'applicationQueryAck',requestId})
      return page
    }}
    const aprsRoster=await statsReference.loadAprsRoster(collectionSource,()=>true)
    assert.deepEqual(aprsRoster.heard,nativeModes.heard)
    assert.deepEqual(aprsRoster.roster,nativeModes.roster)
    assert.equal(aprsRoster.roster.stations.length,2000)
    assert.equal(aprsRoster.heard.length,300)
    for(const [i,g] of sstvState.gallery.entries()){
      const image=await statsReference.loadSstvImage(collectionSource,g.path)
      assert.deepEqual(Buffer.from(await image.arrayBuffer()),Buffer.from(nativeModes.images[i].base64,'base64'))
    }
    // Reuse the negotiated browser session as the real app does across sections.
    // Opening one synthetic socket per section exceeds the real ticket rate cap.
    console.log('Navigation probe: seed')
    const nativeNavigation=await probe.send({type:'seedNavigation'})
    console.log('Navigation probe: subscribe')
    const navigation=modes
    const navigationRequest=crypto.randomUUID()
    navigation.send({type:'applicationSubscribe',topics:['get_remote_satellite_state'],requestId:navigationRequest})
    const navigationFrame=await navigation.take(value=>value.type==='applicationFrame')
    const navigationLive=navigationFrame.updates.find(u=>u.command==='get_remote_satellite_state')
    assert.equal(navigationLive.type,'applicationResult')
    console.log('Navigation probe: live sample')
    const liveState=statsReference.parseSatelliteLive(navigationLive.data,0)
    assert.deepEqual(liveState.settings,nativeNavigation.live.settings)
    assert.deepEqual(liveState.held,nativeNavigation.live.held)
    navigation.send({type:'applicationFrameAck',requestId:navigationRequest,nextRequestId:crypto.randomUUID()})
    navigation.send({type:'applicationSubscribe',topics:[],requestId:null})
    const navigationSource={page:async args=>{
      for(let attempt=0;attempt<50;attempt++){
        await delay(140)
        const requestId=crypto.randomUUID()
        navigation.send({type:'applicationQuery',requestId,...args})
        const page=await navigation.take(value=>value.requestId===requestId).catch(error=>{console.log('Navigation probe timeout',{collection:args.collection,hasCursor:!!args.cursor,closed:navigation.closed,code:navigation.closeCode,reason:navigation.closeReason});throw error})
        if(page.type==='applicationQueryError'&&page.error==='applicationBusy'){navigation.send({type:'applicationQueryAck',requestId});continue}
        assert.equal(page.type,'applicationPage',JSON.stringify({collection:args.collection,type:page.type,error:page.error}))
        navigation.send({type:'applicationQueryAck',requestId})
        return page
      }
      throw new Error('native navigation worker did not produce its document')
    }}
    console.log('Navigation probe: connect capture')
    const connect=await statsReference.loadNavigation(navigationSource,'connect','',()=>true)
    assert.deepEqual(connect.value.prop,nativeNavigation.prop)
    assert.equal(connect.value.coverage.logCount,nativeNavigation.logCount)
    console.log('Navigation probe: path capture')
    const path=await statsReference.loadNavigation(navigationSource,'path','PM95',()=>true)
    assert.equal(path.value.grid,'PM95');assert.ok(path.value.prediction.bands.length>0)
    assert.equal(path.stationContextId,connect.stationContextId)
    console.log('Navigation probe: full satellite capture')
    const satellites=await statsReference.loadNavigation(navigationSource,'satellites','',()=>true)
    assert.ok(satellites.value.view.birds.length>250,'full native catalog must survive chunked workerd transfer')
    assert.ok(satellites.value.view.birds.some(b=>b.name==='ISS (ZARYA)'))
    console.log('Navigation probe: detail capture')
    const satellite=await statsReference.loadNavigation(navigationSource,'satellite','ISS (ZARYA)',()=>true)
    assert.equal(satellite.value.detail.name,'ISS (ZARYA)')
    assert.equal(satellite.value.logCount,nativeNavigation.logCount)
    assert.equal(satellite.stationContextId,connect.stationContextId)
    const programFixture=JSON.parse(await readFile(new URL('../../ui/src/remote-web/__fixtures__/configuration-programming.json',import.meta.url),'utf8'))
    const nativeConfiguration=await probe.send({type:'seedConfiguration',projects:programFixture.projects})
    const settingsDoc=await statsReference.loadNavigation(navigationSource,'settings','',()=>true)
    assert.deepEqual(settingsDoc.value,nativeConfiguration.settings)
    const programmingDoc=await statsReference.loadNavigation(navigationSource,'programming','',()=>true)
    assert.deepEqual(programmingDoc.value,nativeConfiguration.programming)
    assert.equal(programmingDoc.value.projects[0].channels.length,1200)
    const parkCsv='reference,name,grid,location\n'+Array.from({length:40},(_,i)=>`US-${String(i).padStart(4,'0')},Synthetic Park ${i},FN31,US-CT`).join('\n')+'\nUS-9999,Acadia National Park,FN54,US-ME\n'
    for(const search of ['US-00','US-0001','ACADIA','ZZ-99']){
      const desktopParks=await probe.send({type:'seedParks',csv:parkCsv,search})
      const parks=statsReference.parseParks(await navigationSource.page({collection:'parks',cursor:null,search,unconfirmed:false,after:null}))
      assert.deepEqual(parks.parks,desktopParks.parks,`the station answers ${search} exactly as the desktop search does`)
      assert.deepEqual(parks.exact,desktopParks.exact)
      assert.equal(parks.parkCount,41)
    }
    const desktopConfirmations=await probe.send({type:'confirmationDiagnostics'})
    assert.ok(desktopConfirmations.report.diagnoses.length>0,'the seeded station log must carry diagnoses to compare')
    const confirmations=statsReference.parseConfirmations(await navigationSource.page({collection:'confirmations',cursor:null,search:'',unconfirmed:false,after:null}))
    assert.equal(confirmations.logCount,desktopConfirmations.logCount)
    assert.deepEqual(confirmations.report.diagnoses,desktopConfirmations.report.diagnoses.slice(0,50),'the desktop diagnoses, in the same order, cut to the panel')
    assert.deepEqual(confirmations.report.buckets,desktopConfirmations.report.buckets.map(({kind,count})=>({kind,count,qsoIndices:[]})))
    assert.deepEqual(confirmations.report.oneAway,desktopConfirmations.report.oneAway)
    assert.equal(confirmations.report.waitingOnPartner,desktopConfirmations.report.waitingOnPartner)
    assert.equal(confirmations.report.pendingLag,desktopConfirmations.report.pendingLag)
    const desktopPounce=await probe.send({type:'seedPounce',spots:[{call:'3Y0J',freqMhz:14.025,mode:'CW'},{call:'3Y0J',freqMhz:14.026,mode:'CW'},{call:'FT5ZM',freqMhz:99.9,mode:'FT8'},{call:'FT5ZM',freqMhz:21.074,mode:'FT8'}]})
    assert.ok(desktopPounce.fired.length>0,'the station detector must raise alerts for the comparison to mean anything')
    const servedPounce=statsReference.parsePounce(await navigationSource.page({collection:'pounce',cursor:null,search:'',unconfirmed:false,after:null}))
    assert.deepEqual(servedPounce.alerts,desktopPounce.fired,'the browser reads exactly the alerts the desktop Pounce raised')
    assert.equal(servedPounce.threshold,'atno')
    navigation.close()
    const second = await socket.take(value => value.type === 'observation')
    assert.ok(second.frame.sequence > first.frame.sequence)
    assert.equal(socket.closed,false,'observation must still be live before the local disable action')
    assert.equal((await probe.send({ type: 'disable' })).ok, true)
    await socket.take(value => value.type === 'closed')
    assert.equal((await probe.send({ type: 'publishMemories', bank: JSON.stringify(bank) })).accepted, false)
    const restarted = await probe.send({ type: 'restart' })
    assert.equal(restarted.status.stationId, stationId)
    assert.equal(restarted.status.phase, 'disabled')
    assert.equal((await roomStatus(room)).online, false)
    const forgotten = await probe.send({ type: 'forget' })
    assert.equal(forgotten.ok, true, JSON.stringify(forgotten)); assert.equal(forgotten.status.stationId, null)
    assert.equal((await app.db.prepare('SELECT enabled FROM stations WHERE id=?').bind(stationId).first()).enabled, 0)
  } finally { try { await probe.stop() } finally { await app.mf.dispose() } }
})


for (const operationVersion of [1, 2, 3, 4]) test(`actual cloud and native operations v${operationVersion} produce one durable QSO, preserve receipts and refuse local takeover`, {timeout:60000},async()=>{
 assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY)
 const app=await runtime(),probe=await nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY,app.origin)
 let socket
 try{
  await probe.ready();const browser=await app.owner();const begin=await probe.send({type:'begin',name:'Logging synthetic bench'}),stationId=begin.status.pairingId
  // One approval (operator decisions 2026-09-13/14): this browser confirms the pairing, so approving
  // the pairing approves it and turns Remote on. Enable again for a fresh authority, as before.
  await browser.post('pair/claim',{code:begin.status.pairingCode});const confirmed=await browser.post('pair/confirm',{id:begin.status.pairingId});browser.setCookie(confirmed.response.headers.get('set-cookie'));await probe.send({type:'refresh'});assert.equal((await probe.send({type:'approve',enrollmentId:stationId,accountId:browser.accountId})).ok,true)
  const {value:device}=await browser.post(`stations/${stationId}/device`,{name:'Logging browser'});assert.equal(device.approved,true,'the pairing browser is approved with the station');await probe.send({type:'refresh'});await probe.send({type:'enable'})
  const roomNamespace=await app.mf.getDurableObjectNamespace('STATIONS'),room=roomNamespace.get(roomNamespace.idFromName(stationId));for(let i=0;i<30&&!(await roomStatus(room)).online;i++)await delay(100);assert.equal((await roomStatus(room)).online,true)
  assert.deepEqual(await probe.send({type:'seedLogging'}),{count:0,adif:'',txEnabled:false})
  const ticket=(await browser.post(`stations/${stationId}/ticket`)).value;socket=await browser.open(stationId,ticket.ticket);socket.ackObservations();await socket.take(v=>v.type==='session')
  const envelope=request=>({type:'operationRequest',...(operationVersion>=2?{operationVersion}:{}),request})
  const operation=async args=>{await delay(270);const request={requestId:crypto.randomUUID(),...args};socket.send(envelope(request));
    try{return {request,response:await socket.take(v=>v.type==='operationResponse'&&v.requestId===request.requestId)}}
    catch(error){throw new Error(`operation ${request.type}/${request.action?.action??''} v${operationVersion} timed out; socket closed=${socket.closed} code=${socket.closeCode} reason=${socket.closeReason}`,{cause:error})}}
  // stationBusy means the native Engine was held and the request was refused before anything changed, so an
  // operator clicks again. A request the device is allowed to make retries once; `again` re-reads state first
  // when the request carries a command window. Any other refusal, or a second stationBusy, stands. Refusals
  // this case expects (localPermissionRequired, staleContext) are never wrapped.
  const allowed=async(first,again=first)=>{let sent=await first();if(sent.response.error==='stationBusy'){console.log('stationBusy, retrying once:',sent.request.type,sent.request.action?.action??'');await delay(300);sent=await again()}return sent}

  // The approval granted station control and logging; Turn on restores them once the service lists
  // the browser. Then RESTRICT the browser with the switches at the shack: that is what must still
  // refuse it a lease, exactly as a browser without permission always was.
  for(let i=0;i<50;i++){const {status}=await probe.send({type:'status'});if(status.loggingPermissions?.includes(device.deviceId)&&status.stationPermissions?.includes(device.deviceId))break;await delay(100)}
  let state=(await allowed(()=>operation({type:'state'}))).response.value;assert.equal(state.phase,'available','the approval allowed this browser')
  for(const type of ['loggingPermission','stationPermission']){const restricted=await probe.send({type,deviceId:device.deviceId,allow:false});assert.equal(restricted.ok,true,restricted.error)}
  state=(await allowed(()=>operation({type:'state'}))).response.value;assert.equal(state.phase,'localPermissionRequired');assert.equal((await operation({type:'acquire',stationBootId:state.stationBootId})).response.error,'localPermissionRequired')
  await probe.send({type:'refresh'});const permission=await probe.send({type:'loggingPermission',deviceId:device.deviceId,allow:true});assert.equal(permission.ok,true,permission.error);assert.deepEqual(permission.status.loggingPermissions,[device.deviceId])
  state=(await allowed(()=>operation({type:'acquire',stationBootId:state.stationBootId}))).response.value;assert.equal(state.phase,'controlling');assert.equal(state.txArmed,false)
  const record={call:'W1AW',grid:'FN31',country:null,state:null,band:'20m',freqMhz:14.25,mode:'SSB',rstSent:'59',rstRcvd:'57',name:'Joe',qth:'Newington',comment:'Cloud/native append test',notes:'Do not duplicate',whenUnix:Math.floor(Date.now()/1000),confirmed:false,awardConfirmed:false}
  const heartbeat=()=>allowed(()=>operation({type:'heartbeat',leaseId:state.leaseId}))
  const logRequest=s=>({type:'logManual',stationBootId:s.stationBootId,leaseId:s.leaseId,expectedRevision:s.revision,commandWindowId:s.commandWindowId,clientSequence:s.nextSequence,record})
  const logged=await allowed(()=>operation(logRequest(state)),async()=>operation(logRequest((await heartbeat()).response.value)));assert.equal(logged.response.value.outcome,'applied');assert.equal(logged.response.value.evidence,'fileSynced')
  let evidence=await probe.send({type:'loggingEvidence'});assert.equal(evidence.count,1);assert.match(evidence.adif,/W1AW/);assert.match(evidence.adif,/Do not duplicate/);assert.equal(evidence.txEnabled,false)
  await delay(270);socket.send(envelope(logged.request));const replay=await socket.take(v=>v.type==='operationResponse'&&v.requestId===logged.request.requestId);assert.deepEqual(replay,logged.response);assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
  if(operationVersion===4){
   // Edit and delete a contact through the real relay and native authority, found by the key of the
   // row the station's own log page serves. The logging grant alone allows it; nothing keys.
   const {createHash}=await import('node:crypto')
   const labeled=(peer,label,predicate)=>peer.take(predicate).catch(error=>{throw new Error(`${label} timed out; closed=${peer.closed} code=${peer.closeCode} reason=${peer.closeReason}`,{cause:error})})
   const canon=v=>v===null?'z':typeof v==='boolean'?(v?'t':'f'):typeof v==='number'?`n${v<0&&Math.round(Math.abs(v)*1e6)!==0?'-':''}${BigInt(Math.round(Math.abs(v)*1e6))};`:typeof v==='string'?`s${Buffer.byteLength(v)}:${v}`:Array.isArray(v)?`a${v.length}[${v.map(canon).join('')}]`:`o${Object.keys(v).length}{${Object.keys(v).sort().map(k=>canon(k)+canon(v[k])).join('')}}`
   const target=row=>({call:row.call,whenUnix:row.whenUnix,key:createHash('sha256').update(canon(row),'utf8').digest('hex')})
   // One page socket for the whole block: any browser leaving ends the shared logging lease.
   const {value:pageTicket}=await browser.post(`stations/${stationId}/ticket`);const pages=await browser.open(stationId,pageTicket.ticket);pages.ackObservations();await labeled(pages,'page session',v=>v.type==='session');pages.send({type:'applicationHello',version:3});await labeled(pages,'page capabilities',v=>v.type==='applicationCapabilities')
   const logRows=async()=>{const requestId=crypto.randomUUID();pages.send({type:'applicationQuery',requestId,collection:'log',cursor:null,search:'',unconfirmed:false,after:null});const page=await labeled(pages,'log page',v=>v.requestId===requestId);assert.equal(page.type,'applicationPage');pages.send({type:'applicationQueryAck',requestId});return page.rows}
   // The station shares a page-zero capture for a few seconds, so read until the page shows the change.
   // A real page heartbeats every second whatever it is showing; without that the 5 s lease lapses mid-poll.
   const rowFor=async(call,accept)=>{for(let i=0;i<40;i++){const row=(await logRows()).find(r=>r.call===call);if(row&&accept(row))return row;await heartbeat();await delay(250)}throw new Error(`the log page never showed ${call} as expected`)}
   const fresh=async()=>{const {response}=await heartbeat();assert.ok(response.value,`the logging lease heartbeat was refused: ${JSON.stringify(response)}`);return response.value}
   const write=build=>allowed(async()=>operation(build(await fresh())),async()=>operation(build(await fresh())))
   const changeRequest=(s,change)=>({type:'logChange',stationBootId:s.stationBootId,leaseId:s.leaseId,expectedRevision:s.revision,commandWindowId:s.commandWindowId,clientSequence:s.nextSequence,change})
   const second={...record,call:'K1ABC',comment:'Second contact',notes:null,whenUnix:record.whenUnix-60}
   assert.equal((await write(s=>({...logRequest(s),record:second}))).response.value.outcome,'applied')
   const capable=await fresh();assert.ok(capable.controls.capabilities.includes('logEdit'));assert.deepEqual(capable.actions,['log.manual'])
   const before=await rowFor('K1ABC',()=>true)
   const edited=await write(s=>changeRequest(s,{kind:'edit',target:target(before),record:{...second,comment:'Edited from the browser'}}))
   assert.deepEqual(edited.response.value,{operation:'logChange',operationId:edited.request.requestId,outcome:'applied',evidence:'fileSynced'})
   const afterEdit=await probe.send({type:'loggingEvidence'});assert.equal(afterEdit.count,2);assert.match(afterEdit.adif,/Edited from the browser/);assert.equal(afterEdit.txEnabled,false)
   await delay(270);socket.send(envelope(edited.request));assert.deepEqual(await labeled(socket,'edit replay',v=>v.type==='operationResponse'&&v.requestId===edited.request.requestId),edited.response);assert.deepEqual(await probe.send({type:'loggingEvidence'}),afterEdit)
   // Positive control: the pre-edit row is stale now, so a delete aimed at it changes nothing.
   const stale=await write(s=>changeRequest(s,{kind:'delete',target:target(before)}))
   assert.equal(stale.response.value.outcome,'rejected');assert.equal(stale.response.value.reason,'contextChanged');assert.deepEqual(await probe.send({type:'loggingEvidence'}),afterEdit)
   const edit=await rowFor('K1ABC',row=>row.comment==='Edited from the browser')
   const carded=await write(s=>changeRequest(s,{kind:'qslCard',target:target(edit),received:true}))
   assert.equal(carded.response.value.outcome,'applied',JSON.stringify(carded.response));assert.equal(carded.response.value.evidence,'fileSynced')
   const current=await rowFor('K1ABC',row=>row.qslRcvd?.card===true)
   const removed=await write(s=>changeRequest(s,{kind:'delete',target:target(current)}))
   assert.equal(removed.response.value.outcome,'applied',JSON.stringify(removed.response))
   evidence=await probe.send({type:'loggingEvidence'});assert.equal(evidence.count,1);assert.match(evidence.adif,/W1AW/);assert.doesNotMatch(evidence.adif,/K1ABC/)
   // A hunt tags the station's next contact with that call; it is station state, not a log write.
   const hunted=await write(s=>({type:'logChange',stationBootId:s.stationBootId,leaseId:s.leaseId,expectedRevision:s.revision,commandWindowId:s.commandWindowId,clientSequence:s.nextSequence,change:{kind:'hunt',call:'K2ABC',program:'POTA',reference:'US-0004'}}))
   assert.deepEqual(hunted.response.value,{operation:'logChange',operationId:hunted.request.requestId,outcome:'applied',evidence:'stationState'})
   assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence);assert.equal(evidence.txEnabled,false)
   // Your activation is station context too. End it again so nothing logged later in this run is stamped.
   const context=(s,change)=>({type:'logChange',stationBootId:s.stationBootId,leaseId:s.leaseId,expectedRevision:s.revision,commandWindowId:s.commandWindowId,clientSequence:s.nextSequence,change})
   const activated=await write(s=>context(s,{kind:'activation',program:'POTA',reference:'US-0001'}))
   assert.deepEqual(activated.response.value,{operation:'logChange',operationId:activated.request.requestId,outcome:'applied',evidence:'stationState'})
   // One activation's ADIF, read back in chunks through the relay: the station's own file, digest checked,
   // holding the activation's contact and none of the rest of the log.
   const activeContact={...record,call:'K3ACT',comment:'Activation contact',notes:null,whenUnix:record.whenUnix-120}
   assert.equal((await write(s=>({...logRequest(s),record:activeContact}))).response.value.outcome,'applied')
   assert.ok((await fresh()).controls.capabilities.includes('activationExport'))
   const readExport=async(selection,index)=>{const s=await fresh();const {response}=await allowed(()=>operation({type:'activationExport',stationBootId:s.stationBootId,leaseId:s.leaseId,selection,index}));assert.ok(response.value,JSON.stringify(response));return response.value}
   const listed=await readExport(null,0),mine=listed.activations.find(a=>a.reference==='US-0001');assert.ok(mine,JSON.stringify(listed))
   const selection={reference:mine.reference,dayStartUnix:mine.dayStartUnix,callsign:mine.callsign}
   const firstChunk=await readExport(selection,0),parts=[Buffer.from(firstChunk.base64,'base64')]
   for(let i=1;i<firstChunk.file.chunks;i++)parts.push(Buffer.from((await readExport(selection,i)).base64,'base64'))
   const exported=Buffer.concat(parts);assert.equal(exported.length,firstChunk.file.byteLength);assert.equal(createHash('sha256').update(exported).digest('hex'),firstChunk.file.sha256)
   assert.match(exported.toString(),/K3ACT/);assert.doesNotMatch(exported.toString(),/W1AW/)
   // Leave the log as the rest of this run expects it: remove the activation contact by its row key.
   const activeRow=await rowFor('K3ACT',()=>true)
   assert.equal((await write(s=>changeRequest(s,{kind:'delete',target:target(activeRow)}))).response.value.outcome,'applied')
   evidence=await probe.send({type:'loggingEvidence'});assert.doesNotMatch(evidence.adif,/K3ACT/);assert.equal(evidence.txEnabled,false)
   // A logging preference under the logging grant: confirmed by the station and saved to its settings file. The TX
   // latch and the dial do not move, the revision it was made against is stale afterwards, and a station preference
   // needs station control, which this browser does not hold here. Put the preference back for the rest of the run.
   const shownPrefs=await probe.send({type:'settingsEvidence'})
   const pref=await write(s=>changeRequest(s,{kind:'settings',revision:shownPrefs.revision,values:{autoLog:!shownPrefs.autoLog}}))
   assert.deepEqual(pref.response.value,{operation:'logChange',operationId:pref.request.requestId,outcome:'applied',evidence:'settingsSaved'})
   const savedPrefs=await probe.send({type:'settingsEvidence'})
   assert.equal(savedPrefs.autoLog,!shownPrefs.autoLog);assert.equal(savedPrefs.savedAutoLog,!shownPrefs.autoLog)
   assert.equal(savedPrefs.txEnabled,false);assert.equal(savedPrefs.dialHz,shownPrefs.dialHz)
   const stalePref=await write(s=>changeRequest(s,{kind:'settings',revision:shownPrefs.revision,values:{autoLog:shownPrefs.autoLog}}))
   assert.equal(stalePref.response.value.outcome,'rejected');assert.equal(stalePref.response.value.reason,'contextChanged')
   const noControl=await allowed(async()=>operation(changeRequest(await fresh(),{kind:'settings',revision:savedPrefs.revision,values:{contestCheck:'73'}})))
   assert.equal(noControl.response.error,'localPermissionRequired')
   const restored=await write(s=>changeRequest(s,{kind:'settings',revision:savedPrefs.revision,values:{autoLog:shownPrefs.autoLog}}))
   assert.equal(restored.response.value.evidence,'settingsSaved',JSON.stringify(restored.response));assert.equal((await probe.send({type:'settingsEvidence'})).autoLog,shownPrefs.autoLog)
   const ended=await write(s=>context(s,{kind:'clearActivation'}))
   assert.equal(ended.response.value.evidence,'stationState',JSON.stringify(ended.response));assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
   // Closing the page socket ends the lease, as any departure does. Wait for that, then take it again.
   pages.close()
   for(let i=0;i<50&&(await allowed(()=>operation({type:'state'}))).response.value?.phase==='controlling';i++)await delay(100)
   state=(await allowed(()=>operation({type:'acquire',stationBootId:state.stationBootId}))).response.value;assert.equal(state.phase,'controlling')
  }
  if(operationVersion>=2){
   assert.equal((await probe.send({type:'stationPermission',deviceId:device.deviceId,allow:true})).ok,true)
   const controls=(await allowed(()=>operation({type:'state'}))).response.value
   assert.deepEqual(controls.controls.capabilities,operationVersion>=3?['decoder','amplifier','frequency','mode','tier','ampFollowBand','workspace','decoderSettings','receiverSettings','receiverGain','bandSelection','receiverFilter','receiverDsp','phoneMode','workSpot','radioLevels','radioSelection','fmTuning','fmReceiver','aiCw','redecode','splitTuning','ritTuning','workDigitalSpot','repeaterTuning','memoryRecall','aprsTuning','rotator','rigScope','...(operationVersion===4?['qsoLogging','logEdit','qslMarks','otaHunt','otaActivation','selfSpot','activationExport','settingsLogging','settingsControl']:[])]:['decoder','amplifier'])
   const clearRequest=s=>({type:'stationControl',stationBootId:s.stationBootId,leaseId:s.leaseId,expectedRevision:s.revision,commandWindowId:s.commandWindowId,clientSequence:s.nextSequence,context:s.controls.context,action:{action:'decoder.clear',receiver:'cw'}})
   const cleared=await allowed(()=>operation(clearRequest(controls)),async()=>operation(clearRequest((await heartbeat()).response.value)))
   assert.equal(cleared.response.value.outcome,'applied');assert.equal(cleared.response.value.evidence,'receiverState')
   assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
   if(operationVersion>=3){
    // FT8/FT4 Work is its own v3 action. It must cross the real relay and native parser without
    // dropping the socket or the lease. The band does not match the dial, so the station answers a
    // closed rejection and leaves no pending receipt for the controls below (the probe has no radio
    // owner to answer a readback).
    const workRequest=s=>({type:'stationControl',stationBootId:s.stationBootId,leaseId:s.leaseId,expectedRevision:s.revision,commandWindowId:s.commandWindowId,clientSequence:s.nextSequence,context:s.controls.context,action:{action:'radio.workDigitalSpot',tier:'FT4',dialMhz:14.0815,band:'40m',call:'JA2DEF'}})
    const worked=await allowed(async()=>operation(workRequest((await heartbeat()).response.value)),async()=>operation(workRequest((await heartbeat()).response.value)))
    assert.equal(worked.response.value?.operation,'stationControl',JSON.stringify(worked.response))
    assert.equal(worked.response.value.outcome,'rejected')
    assert.equal((await heartbeat()).response.value.phase,'controlling')
    assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
   }
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
     // A station preference under station control: confirmed, saved, and the TX latch and dial untouched.
     const shownStation=await probe.send({type:'settingsEvidence'})
     const stationPref=s=>({type:'logChange',stationBootId:s.stationBootId,leaseId:s.leaseId,expectedRevision:s.revision,commandWindowId:s.commandWindowId,clientSequence:s.nextSequence,change:{kind:'settings',revision:shownStation.revision,values:{contestCheck:'73'}}})
     const savedStation=await allowed(async()=>operation(stationPref((await heartbeat()).response.value)))
     assert.deepEqual(savedStation.response.value,{operation:'logChange',operationId:savedStation.request.requestId,outcome:'applied',evidence:'settingsSaved'})
     const afterStation=await probe.send({type:'settingsEvidence'})
     assert.equal(afterStation.contestCheck,'73');assert.equal(afterStation.savedContestCheck,'73')
     assert.equal(afterStation.txEnabled,false);assert.equal(afterStation.dialHz,shownStation.dialHz)
     assert.equal(controls.transmitEpoch, null)
     const grant = await probe.send({type:'transmitPermission',deviceId:device.deviceId,allow:true})
     assert.equal(grant.ok,true,grant.error)
     assert.deepEqual(grant.status.transmitPermissions,[device.deviceId])
     const owner = (await allowed(()=>operation({type:'state'}))).response.value
     assert.match(owner.transmitEpoch,/^[0-9a-f]{16}$/)
     const stop = {type:'stopTransmit',stationBootId:owner.stationBootId,leaseId:owner.leaseId,transmitEpoch:owner.transmitEpoch}
     assert.deepEqual((await operation(stop)).response.value,{stop:'accepted'})
     assert.equal((await operation(stop)).response.error,'staleContext')
     const after = (await allowed(()=>operation({type:'state'}))).response.value
     assert.notEqual(after.transmitEpoch,owner.transmitEpoch)
     assert.equal(after.phase,'controlling')
     await stopLocal('ptt','with transmit permission')
     assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
     const command = (state, action) => operation({type:'stationControl',
       stationBootId:state.stationBootId,leaseId:state.leaseId,expectedRevision:state.revision,
       commandWindowId:state.commandWindowId,clientSequence:state.nextSequence,
       context:state.controls.context,action:{...action,transmitEpoch:state.transmitEpoch}})
     const action = (state, change) => allowed(() => command(state, change), async () => command(await ftState(), change))
     const ftState = async () => {
       const {response} = await heartbeat()
       assert.ok(response.value, `the FT lease heartbeat was refused: ${JSON.stringify(response)}`)
       return response.value
     }
     for (const tier of ['FT8','FT4']) {
       assert.equal((await probe.send({type:'seedFt',tier})).txEnabled,false)
       let ft = await ftState()
       assert.ok(ft.controls.capabilities.includes('ftOperate'))
       assert.ok(ft.controls.capabilities.includes('ftSettings'))
       for (const change of [{kind:'txOffset',hz:1800},{kind:'bothOffsets',hz:2200},{kind:'hold',on:true},{kind:'even',even:false},{kind:'auto',auto:true}]) {
         const before=await probe.send({type:'ftSettingsEvidence'})
         ft=await ftState()
         const changed=await action(ft,{action:'ft.setting',expectedTier:tier,expected:before.settings,change})
         assert.equal(changed.response.value?.outcome,'applied',JSON.stringify(changed.response))
         assert.equal(changed.response.value?.evidence,change.kind==='auto'?'stationState':'settingsSaved')
         const after=await probe.send({type:'ftSettingsEvidence'})
         assert.equal(after.txEnabled,false);assert.equal(after.owned,false)
         assert.notEqual(after.settings.key,before.settings.key)
         if(change.kind==='txOffset'||change.kind==='bothOffsets')assert.equal(after.settings.txOffsetHz,change.hz)
         if(change.kind==='bothOffsets')assert.equal(after.settings.rxOffsetHz,change.hz)
         if(change.kind==='hold')assert.equal(after.settings.holdTxFreq,true)
         if(change.kind==='even'){assert.equal(after.settings.txEven,false);assert.equal(after.settings.txCycleAuto,false)}
         if(change.kind==='auto')assert.equal(after.settings.txCycleAuto,true)
       }
       ft=await ftState()
       const cq = await action(ft,{action:'ft.cq',expectedTier:tier,direction:'DX'})
       assert.equal(cq.response.value?.outcome,'applied',JSON.stringify(cq.response))
       assert.deepEqual(await probe.send({type:'ftEvidence'}),{tier,txEnabled:true,owned:true,logCount:1})
       // Live RX and Skip Tx1 use the active FT owner without acquiring a new
       // transmitter or moving the TX marker. Exercise the actual cloud/host path.
       for(const change of [{kind:'rxOffset',hz:900},{kind:'skipTx1',on:true},{kind:'skipTx1',on:false}]){
         const before=await probe.send({type:'ftRuntimeEvidence'})
         ft=await ftState();assert.equal(ft.txArmed,true)
         assert.ok(ft.controls.capabilities.includes('ftRuntime'))
         const changed=await action(ft,{action:'ft.runtime',expectedTier:tier,expected:before.runtime,change})
         assert.equal(changed.response.value?.outcome,'applied',JSON.stringify(changed.response))
         assert.equal(changed.response.value?.evidence,change.kind==='rxOffset'?'settingsSaved':'stationState')
         const after=await probe.send({type:'ftRuntimeEvidence'})
         assert.equal(after.txEnabled,true);assert.equal(after.owned,true)
         assert.equal(after.runtime.settings.txOffsetHz,before.runtime.settings.txOffsetHz)
         assert.notEqual(after.runtime.settings.key,before.runtime.settings.key)
         if(change.kind==='rxOffset')assert.equal(after.runtime.settings.rxOffsetHz,900)
         else assert.equal(after.runtime.skipTx1,change.on)
       }
       ft = await ftState()
       assert.equal(ft.txArmed,true)
       assert.equal((await action(ft,{action:'ft.txEnabled',expectedTier:tier,on:false})).response.value?.outcome,'applied')
       assert.equal((await probe.send({type:'ftEvidence'})).txEnabled,false)
       ft = await ftState()
       const enabled = await action(ft,{action:'ft.txEnabled',expectedTier:tier,on:true})
       assert.equal(enabled.response.value?.outcome,'applied',JSON.stringify(enabled.response))
       assert.equal((await probe.send({type:'ftEvidence'})).txEnabled,true)
       ft = await ftState()
       const stop = {type:'stopTransmit',stationBootId:ft.stationBootId,leaseId:ft.leaseId,transmitEpoch:ft.transmitEpoch}
       assert.deepEqual((await operation(stop)).response.value,{stop:'accepted'})
       // The acknowledgement alone is not evidence of the native engine stopping.
       for(let i=0;i<30&&(await probe.send({type:'ftEvidence'})).txEnabled;i++)await delay(50)
       assert.deepEqual(await probe.send({type:'ftEvidence'}),{tier,txEnabled:false,owned:false,logCount:1})
       assert.equal((await action(ft,{action:'ft.cq',expectedTier:tier,direction:null})).response.error,'staleContext')
       assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
       const selection = await probe.send({type:'ftCallDecode'})
       assert.match(selection.message,/CQ W1AW FN31/)
       ft = await ftState()
       assert.ok(ft.controls.capabilities.includes('ftCall'))
       const called = await action(ft,{action:'ft.call',expectedTier:tier,selection})
       assert.equal(called.response.value?.outcome,'applied',JSON.stringify(called.response))
       const contact = await probe.send({type:'ftCallEvidence'})
       assert.equal(contact.qso.dxcall,'W1AW');assert.equal(contact.qso.dxgrid,'FN31')
       // N0CALL has a nonstandard FT suffix: preserve the native hashed-call form.
       assert.equal(contact.owned,true);assert.equal(contact.qso.txNow,'<W1AW> N0CALL')
       for(const change of [{kind:'resend'},{kind:'freeText',text:'TNX 73'},{kind:'monitor'}]){
         const before=(await probe.send({type:'ftCallEvidence'})).qso
         ft=await ftState();assert.ok(ft.controls.capabilities.includes('ftExchange'))
         const expectedQso={dxcall:before.dxcall,state:before.state,txNow:before.txNow,cqRunning:before.cqRunning}
         const result=await action(ft,{action:'ft.exchange',expectedTier:tier,expectedQso,change})
         assert.equal(result.response.value?.outcome,'applied',JSON.stringify(result.response))
         const after=await probe.send({type:'ftCallEvidence'})
         assert.equal(after.owned,true)
         if(change.kind==='freeText')assert.equal(after.qso.txNow,'W1AW N0CALL TNX 73')
         if(change.kind==='monitor'){assert.equal(after.qso.dxcall,null);assert.equal(after.qso.cqRunning,false)}
         assert.deepEqual(await probe.send({type:'loggingEvidence'}),{...evidence,txEnabled:true})
       }
       // N0CALL is nonstandard: native hashed_form drops its grid/report and
       // turns the R-report into RRR. Preserve those on-air forms in readback.
       for(const [text,expectedText] of [
         ['K2ABC N0CALL AA00','<K2ABC> N0CALL'],['K2ABC N0CALL -12','<K2ABC> N0CALL'],
         ['K2ABC N0CALL R-12','<K2ABC> N0CALL RRR'],['K2ABC N0CALL RR73','<K2ABC> N0CALL RR73'],['TNX 73','TNX 73']]){
         const before=(await probe.send({type:'ftCallEvidence'})).qso
         ft=await ftState();assert.ok(ft.controls.capabilities.includes('ftMessages'))
         const expectedQso={dxcall:before.dxcall,state:before.state,txNow:before.txNow,cqRunning:before.cqRunning}
         const result=await action(ft,{action:'ft.message',expectedTier:tier,expectedQso,call:'K2ABC',grid:'FN42',text})
         assert.equal(result.response.value?.outcome,'applied',JSON.stringify(result.response))
         const after=await probe.send({type:'ftCallEvidence'})
         assert.equal(after.owned,true);assert.equal(after.qso.dxcall,'K2ABC');assert.equal(after.qso.txNow,expectedText)
         assert.deepEqual(await probe.send({type:'loggingEvidence'}),{...evidence,txEnabled:true})
       }
       ft = await ftState()
       assert.deepEqual((await operation({type:'stopTransmit',stationBootId:ft.stationBootId,
         leaseId:ft.leaseId,transmitEpoch:ft.transmitEpoch})).response.value,{stop:'accepted'})
       for(let i=0;i<30&&(await probe.send({type:'ftEvidence'})).txEnabled;i++)await delay(50)
       assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
     }
   }
  }else assert.equal(Object.hasOwn(state,'controls'),false)
  const current=(await heartbeat()).response.value;assert.equal(current.phase,'controlling')
  assert.equal((await probe.send({type:'takeOverLogging'})).ok,true);const refused=await operation({...logged.request,requestId:crypto.randomUUID(),expectedRevision:current.revision,commandWindowId:current.commandWindowId,clientSequence:current.nextSequence,record:{...record,call:'K2ABC'}});assert.equal(refused.response.error,'localPermissionRequired');assert.deepEqual(await probe.send({type:'loggingEvidence'}),evidence)
  await probe.send({type:'loggingPermission',deviceId:device.deviceId,allow:true});assert.equal((await allowed(()=>operation({type:'result',operationId:logged.request.requestId}))).response.value.outcome,'applied')
  socket.close();socket=null
  // Remote remembers being on (operator decision 2026-09-13), and since the one-approval decision the
  // same day, FT8/FT4 transmit is remembered too. Restarting the actual native controller turns Remote
  // back on and gives remote logging, station control (v4) and FT8/FT4 transmit (v4, where it was
  // granted) back to a browser the actual service still lists at the same approval. No lease survives
  // the restart, the TX-enable latch is off, and a restored grant arms nothing on a fresh lease.
  if(operationVersion===4){
   assert.equal((await probe.send({type:'stationPermission',deviceId:device.deviceId,allow:true})).ok,true)
   const held=await probe.send({type:'transmitPermission',deviceId:device.deviceId,allow:true})
   assert.deepEqual(held.status.transmitPermissions,[device.deviceId],'positive control: transmit permission was held before the restart')
  }
  await delay(200)
  let restarted=await probe.send({type:'restart'});assert.notEqual(restarted.status.phase,'disabled');assert.ok(restarted.status.observationGeneration)
  for(let i=0;i<50&&!restarted.status.loggingPermissions.includes(device.deviceId);i++){await delay(100);restarted=await probe.send({type:'status'})}
  assert.deepEqual(restarted.status.loggingPermissions,[device.deviceId]);assert.equal(restarted.status.loggingController,null)
  assert.deepEqual(restarted.status.transmitPermissions,operationVersion===4?[device.deviceId]:[],'transmit comes back only where it was granted')
  const idle=await probe.send({type:'ftEvidence'});assert.equal(idle.txEnabled,false,'the TX-enable latch is off after the restart');assert.equal(idle.owned,false)
  if(operationVersion===4){
   assert.deepEqual(restarted.status.stationPermissions,[device.deviceId])
   for(let i=0;i<50&&restarted.status.phase!=='connected';i++){await delay(100);restarted=await probe.send({type:'status'})}
   for(let i=0;i<30&&!(await roomStatus(room)).online;i++)await delay(100)
   const again=(await browser.post(`stations/${stationId}/ticket`)).value;socket=await browser.open(stationId,again.ticket);socket.ackObservations();await socket.take(v=>v.type==='session')
   assert.equal((await probe.send({type:'seedFt',tier:'FT8'})).txEnabled,false)
   const fresh=(await allowed(()=>operation({type:'state'}))).response.value
   const leased=(await allowed(()=>operation({type:'acquire',stationBootId:fresh.stationBootId}))).response.value;assert.equal(leased.phase,'controlling')
   const beat=(await allowed(()=>operation({type:'heartbeat',leaseId:leased.leaseId}))).response.value
   assert.ok(beat.controls.capabilities.includes('ftOperate'),'positive control: the restored transmit grant is real')
   assert.match(beat.transmitEpoch,/^[0-9a-f]{16}$/);assert.equal(beat.txArmed,false,'a restored grant and a fresh lease arm nothing')
   const armed=await probe.send({type:'ftEvidence'});assert.equal(armed.txEnabled,false);assert.equal(armed.owned,false)
  }
 }finally{socket?.close();try{await probe.stop()}finally{await app.mf.dispose()}}
})
