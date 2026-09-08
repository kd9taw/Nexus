import { test } from 'node:test'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { runtime, roomStatus } from './runtime.mjs'

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
    const second = await socket.take(value => value.type === 'observation')
    assert.ok(second.frame.sequence > first.frame.sequence)
    assert.equal((await probe.send({ type: 'disable' })).ok, true)
    await socket.take(value => value.type === 'closed')
    const restarted = await probe.send({ type: 'restart' })
    assert.equal(restarted.status.stationId, stationId)
    assert.equal(restarted.status.phase, 'disabled')
    assert.equal((await roomStatus(room)).online, false)
    const forgotten = await probe.send({ type: 'forget' })
    assert.equal(forgotten.ok, true); assert.equal(forgotten.status.stationId, null)
    assert.equal((await app.db.prepare('SELECT enabled FROM stations WHERE id=?').bind(stationId).first()).enabled, 0)
  } finally { try { await probe.stop() } finally { await app.mf.dispose() } }
})
