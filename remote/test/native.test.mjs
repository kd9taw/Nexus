import { test } from 'node:test'
import { readFile } from 'node:fs/promises'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { runtime, roomStatus } from './runtime.mjs'
import { recallReference, recallAdif } from './recall-reference.mjs'
import { insightsReference, insightsAdif } from './insights-reference.mjs'

function nativeProbe(binary, origin) {
  const child = spawn(binary, ['--ignored', '--exact', 'remote_service::tests::cloud_runtime_probe', '--nocapture'], { stdio: ['pipe', 'pipe', 'pipe'] })
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
  child.stdin.write(JSON.stringify({ origin }) + '\n')
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
      assert.equal(code, 0, 'native probe must exit successfully')
    },
  }
}

test('actual native controller pairs, stores authority, publishes real DTOs, disables and revokes through workerd', { timeout: 60000 }, async () => {
  assert.ok(process.env.NEXUS_REMOTE_TEST_BINARY, 'run npm run test:native to build the actual native probe')
  const app = await runtime()
  const probe = nativeProbe(process.env.NEXUS_REMOTE_TEST_BINARY, app.origin)
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
    await probe.send({ type: 'vaultFailure', enabled: true })
    const refused = await probe.send(approve)
    assert.equal(refused.ok, false); assert.equal(refused.error, 'credentialStoreUnavailable')
    assert.equal((await app.db.prepare('SELECT COUNT(*) AS count FROM stations WHERE id=?').bind(stationId).first()).count, 0)
    await probe.send({ type: 'vaultFailure', enabled: false })
    const paired = await probe.send(approve)
    assert.equal(paired.ok, true); assert.equal(paired.status.phase, 'disabled')
    const { value: request, response } = await browser.post(`stations/${stationId}/device`, { name: 'Native probe browser' })
    browser.setCookie(response.headers.get('set-cookie'))
    await browser.post(`stations/${stationId}/ticket`, {}, 403)
    const requests = await probe.send({ type: 'refresh' })
    assert.equal(requests.status.devices[0].id, request.deviceId)
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
    socket.send({ type: 'applicationHello' })
    const capabilities = await socket.take(value => value.type === 'applicationCapabilities')
    assert.equal(capabilities.version, 1)
    assert.deepEqual(capabilities.commands, ['get_snapshot', 'get_settings', 'get_band_plan', 'get_spectrum_row', 'get_meters'])
    for (const command of capabilities.commands) {
      const requestId = crypto.randomUUID()
      socket.send({ type: 'applicationRead', requestId, command, revision: null })
      const result = await socket.take(value => value.requestId === requestId)
      assert.equal(result.type, 'applicationResult', `${command} must return the actual native DTO`)
      assert.equal(result.command, command)
      assert.equal(result.baseRevision, null)
      assert.ok(result.ageMs < 3000)
      if (command === 'get_snapshot') {
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
    const second = await socket.take(value => value.type === 'observation')
    assert.ok(second.frame.sequence > first.frame.sequence)
    assert.equal((await probe.send({ type: 'disable' })).ok, true)
    await socket.take(value => value.type === 'closed')
    assert.equal((await probe.send({ type: 'publishMemories', bank: JSON.stringify(bank) })).accepted, false)
    const restarted = await probe.send({ type: 'restart' })
    assert.equal(restarted.status.stationId, stationId)
    assert.equal(restarted.status.phase, 'disabled')
    assert.equal((await roomStatus(room)).online, false)
    const forgotten = await probe.send({ type: 'forget' })
    assert.equal(forgotten.ok, true); assert.equal(forgotten.status.stationId, null)
    assert.equal((await app.db.prepare('SELECT enabled FROM stations WHERE id=?').bind(stationId).first()).enabled, 0)
  } finally { try { await probe.stop() } finally { await app.mf.dispose() } }
})
