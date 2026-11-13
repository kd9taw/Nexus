// Tests for the IOTA island list's download retry (scripts/gen-iota-groups.mjs).
//
// iota-world.org has answered HTTP 202 with no file while it is (re)building groups.json, and a
// release built in that window shipped without the list. A 202 or a 5xx is the server's own
// passing state, so the download asks again after a growing wait; anything else (a 404, a
// redirect) is an answer that will not change and fails at once. A network failure with no answer
// at all (a timeout, DNS, a reset) is retried the same way. A fake fetch and sleep stand in
// for the network and the clock.
//
// The islands list's checks (`checkIslands`) are tested at the end, on made-up islands in IOTA's
// islands.json shape: the real list is never in the repository.
//
// Run: node --test scripts/gen-iota-groups.test.mjs

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { checkIslands, downloadWithRetry, RETRY_DELAYS_MS } from './gen-iota-groups.mjs'

/**
 * A fetch answering `statuses` in turn (200 carries `body`), and the calls it saw. `'net'` is a
 * network failure with no HTTP answer (a timeout, DNS, a reset), marked as the script marks one;
 * `'big'` is a refusal of the file itself (larger than the limit), which carries no status either.
 */
function fakeFetch(statuses, body = '[]') {
  const calls = []
  const fetchOnce = async () => {
    const status = statuses[calls.length]
    calls.push(status)
    if (status === 'net') throw Object.assign(new Error('the IOTA list server could not be reached (fetch failed)'), { network: true })
    if (status === 'big') throw new Error('the IOTA list is larger than 2097152 bytes')
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

test('a network failure with no HTTP answer is asked again, with the same waits and the same end', async () => {
  const { fetchOnce, calls } = fakeFetch(['net', 202, 200])
  const { sleep, waits } = fakeSleep()
  await downloadWithRetry({ fetchOnce, sleep, log: quiet })
  assert.deepEqual(calls, ['net', 202, 200])
  assert.deepEqual(waits, RETRY_DELAYS_MS.slice(0, 2))

  const attempts = RETRY_DELAYS_MS.length + 1
  const dead = fakeFetch(Array(attempts + 5).fill('net'))
  const slept = fakeSleep()
  await assert.rejects(
    downloadWithRetry({ fetchOnce: dead.fetchOnce, sleep: slept.sleep, log: quiet }),
    new RegExp(`could not be reached.*after ${attempts} attempts`),
  )
  assert.equal(dead.calls.length, attempts)
  assert.deepEqual(slept.waits, RETRY_DELAYS_MS)
})

test('CONTROL: a refused file (no status, not a network failure) fails at once', async () => {
  const { fetchOnce, calls } = fakeFetch(['big', 200])
  const { sleep, waits } = fakeSleep()
  await assert.rejects(downloadWithRetry({ fetchOnce, sleep, log: quiet }), /larger than/)
  assert.deepEqual(calls, ['big'])
  assert.deepEqual(waits, [])
})

/** `n` made-up islands in islands.json's shape (every value a string), the first in AF-001 and
 *  the second in EU-005; `edit` changes the list before it is serialized. */
function islands(n, edit = (l) => l) {
  const list = Array.from({ length: n }, (_, i) => ({
    id: String(i + 1),
    refno: i === 0 ? 'AF-001' : i === 1 ? 'EU-005' : `OC-${String(i % 1000).padStart(3, '0')}`,
    name: `Test Island ${i + 1}`,
    comment: '',
    subgroup: 'OC-000-0-0',
    excluded: '0',
  }))
  return Buffer.from(JSON.stringify(edit(list)))
}

test('the islands list parses: every island with an id, a reference and a name counts', () => {
  assert.equal(checkIslands(islands(10_000)), 10_000)
  // Numbers as well as strings; entries without a valid id or reference are skipped, not fatal.
  const mixed = islands(10_002, (l) => {
    l[2].id = 3
    l[3].id = '0'
    l[4].refno = 'XX-001'
    return l
  })
  assert.equal(checkIslands(mixed), 10_000)
})

test('an islands list that is short, changed or doubtful is refused', () => {
  assert.throws(() => checkIslands(islands(9_999)), /only 9999 islands/)
  assert.throws(() => checkIslands(Buffer.from('{"islands":[]}')), /not a JSON array/)
  assert.throws(() => checkIslands(Buffer.from('<html>maintenance</html>')))
  assert.throws(() => checkIslands(islands(10_001, (l) => ((l[5].id = l[6].id), l))), /listed twice/)
  assert.throws(() => checkIslands(islands(10_000, (l) => ((l[0].refno = 'AF-002'), l))), /no island in AF-001/)
  assert.throws(() => checkIslands(islands(10_000, (l) => ((l[7].name = 'x'.repeat(201)), l))), /longer than 200/)
})
