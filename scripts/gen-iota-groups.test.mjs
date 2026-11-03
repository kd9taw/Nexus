// Tests for the IOTA island list's download retry (scripts/gen-iota-groups.mjs).
//
// iota-world.org has answered HTTP 202 with no file while it is (re)building groups.json, and a
// release built in that window shipped without the list. A 202 or a 5xx is the server's own
// passing state, so the download asks again after a growing wait; anything else (a 404, a
// redirect) is an answer that will not change and fails at once. A fake fetch and sleep stand in
// for the network and the clock.
//
// Run: node --test scripts/gen-iota-groups.test.mjs

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { downloadWithRetry, RETRY_DELAYS_MS } from './gen-iota-groups.mjs'

/** A fetch answering `statuses` in turn (200 carries `body`), and the calls it saw. */
function fakeFetch(statuses, body = '[]') {
  const calls = []
  const fetchOnce = async () => {
    const status = statuses[calls.length]
    calls.push(status)
    if (status !== 200) throw Object.assign(new Error(`HTTP ${status}`), { status })
    return Buffer.from(body)
  }
  return { fetchOnce, calls }
}

function fakeSleep() {
  const waits = []
  return { sleep: async (ms) => void waits.push(ms), waits }
}

const quiet = () => {}

test('a 202 is asked again after a wait, and the file that follows is used', async () => {
  const { fetchOnce, calls } = fakeFetch([202, 202, 200], '[{"refno":"AF-001"}]')
  const { sleep, waits } = fakeSleep()
  const bytes = await downloadWithRetry({ fetchOnce, sleep, log: quiet })
  assert.equal(bytes.toString(), '[{"refno":"AF-001"}]')
  assert.deepEqual(calls, [202, 202, 200])
  assert.deepEqual(waits, RETRY_DELAYS_MS.slice(0, 2))
})

test('a 5xx is asked again too', async () => {
  const { fetchOnce, calls } = fakeFetch([503, 500, 200])
  const { sleep } = fakeSleep()
  await downloadWithRetry({ fetchOnce, sleep, log: quiet })
  assert.deepEqual(calls, [503, 500, 200])
})

test('the waits grow, and after the last one the download fails naming the attempts', async () => {
  assert.ok(RETRY_DELAYS_MS.length >= 3, 'a few retries')
  for (let i = 1; i < RETRY_DELAYS_MS.length; i++) {
    assert.ok(RETRY_DELAYS_MS[i] > RETRY_DELAYS_MS[i - 1], 'backoff grows')
  }
  const attempts = RETRY_DELAYS_MS.length + 1
  const { fetchOnce, calls } = fakeFetch(Array(attempts + 5).fill(202))
  const { sleep, waits } = fakeSleep()
  await assert.rejects(downloadWithRetry({ fetchOnce, sleep, log: quiet }), (e) => {
    assert.match(e.message, /HTTP 202/)
    assert.match(e.message, new RegExp(`after ${attempts} attempts`))
    return true
  })
  assert.equal(calls.length, attempts, 'no attempt past the last wait')
  assert.deepEqual(waits, RETRY_DELAYS_MS)
})

test('an answer that will not change fails at once, with no wait', async () => {
  for (const status of [404, 403, 302]) {
    const { fetchOnce, calls } = fakeFetch([status, 200])
    const { sleep, waits } = fakeSleep()
    await assert.rejects(downloadWithRetry({ fetchOnce, sleep, log: quiet }), new RegExp(`HTTP ${status}`))
    assert.deepEqual(calls, [status], `HTTP ${status} is not asked again`)
    assert.deepEqual(waits, [])
  }
})

test('CONTROL: a file on the first ask is used with no wait', async () => {
  const { fetchOnce, calls } = fakeFetch([200])
  const { sleep, waits } = fakeSleep()
  await downloadWithRetry({ fetchOnce, sleep, log: quiet })
  assert.deepEqual(calls, [200])
  assert.deepEqual(waits, [])
})
