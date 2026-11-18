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
// The cached-copy fallback (`checkCacheDir`, `orCached`, `--fallback`) is tested last: the release
// jobs restore the last good lists from the Actions cache, which is untrusted input, so the
// directory is refused as a whole for anything but the three files the script writes, valid; the
// script falls back to it, and with no usable copy the run fails, which fails the release job.
//
// Run: node --test scripts/gen-iota-groups.test.mjs

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readdirSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync, symlinkSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { CACHE_FILES, check, checkCacheDir, checkIslands, downloadWithRetry, orCached, RETRY_DELAYS_MS } from './gen-iota-groups.mjs'

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

/** 1,000 made-up island groups in groups.json's shape, AF-001 and EU-005 among them. */
function groups() {
  const list = Array.from({ length: 1000 }, (_, i) => ({ refno: `OC-${String(i).padStart(3, '0')}`, name: `Test Group ${i}` }))
  list[0].refno = 'AF-001'
  list[1].refno = 'EU-005'
  return Buffer.from(JSON.stringify(list))
}

/** A fresh temporary directory, removed when the test ends. */
function tempDir(t) {
  const dir = mkdtempSync(join(tmpdir(), 'gen-iota-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  return dir
}

/** A cache directory holding what an earlier run wrote: both lists and a manifest dated `at`. */
function cacheDir(t, at = 1_700_000_000) {
  const dir = tempDir(t)
  writeFileSync(join(dir, 'iota_groups.json'), groups())
  writeFileSync(join(dir, 'iota_islands.json'), islands(10_000))
  writeFileSync(join(dir, 'iota_groups.manifest.json'), JSON.stringify({ fetchedAt: at }))
  return dir
}

const failing = (message) => async () => {
  throw new Error(message)
}

test('the cache directory check accepts exactly what the script writes, read once', (t) => {
  const got = checkCacheDir(cacheDir(t, 1_700_000_000))
  assert.equal(got.miss, undefined, got.miss)
  assert.deepEqual(got.groups, groups())
  assert.deepEqual(got.islands, islands(10_000))
  assert.equal(got.fetchedAt, 1_700_000_000)
})

test('the cache directory check takes anything else as a miss, saying why', (t) => {
  const miss = (dir) => checkCacheDir(dir).miss ?? ''
  assert.match(miss(join(tempDir(t), 'absent')), /no cached copy/)
  assert.match(miss(tempDir(t)), /no cached copy/)

  const extra = cacheDir(t)
  writeFileSync(join(extra, 'build.rs'), 'fn main() {}')
  assert.match(miss(extra), /unexpected entries .*build\.rs/)

  const sub = cacheDir(t)
  mkdirSync(join(sub, 'nested'))
  assert.match(miss(sub), /unexpected entries .*nested/)

  const short = cacheDir(t)
  rmSync(join(short, 'iota_islands.json'))
  assert.match(miss(short), /iota_islands\.json is missing/)

  const linked = cacheDir(t)
  const target = join(tempDir(t), 'elsewhere.json')
  writeFileSync(target, groups())
  rmSync(join(linked, 'iota_groups.json'))
  symlinkSync(target, join(linked, 'iota_groups.json'))
  assert.match(miss(linked), /iota_groups\.json .* not a regular file/)

  const big = cacheDir(t)
  writeFileSync(join(big, 'iota_groups.manifest.json'), ' '.repeat(CACHE_FILES['iota_groups.manifest.json'] + 1))
  assert.match(miss(big), /iota_groups\.manifest\.json .* over 65536/)

  const notJson = cacheDir(t)
  writeFileSync(join(notJson, 'iota_groups.json'), '<html>maintenance</html>')
  assert.match(miss(notJson), /failed its checks/)

  const shape = cacheDir(t)
  writeFileSync(join(shape, 'iota_islands.json'), '{"islands":[]}')
  assert.match(miss(shape), /failed its checks: the IOTA islands list is not a JSON array/)

  const undated = cacheDir(t)
  writeFileSync(join(undated, 'iota_groups.manifest.json'), '{"fetchedAt":"yesterday"}')
  assert.match(miss(undated), /no valid fetchedAt/)

  const arrayManifest = cacheDir(t)
  writeFileSync(join(arrayManifest, 'iota_groups.manifest.json'), '[1]')
  assert.match(miss(arrayManifest), /not a JSON object/)
})

test('a failed download takes the cached bytes, and says so', async () => {
  const lines = []
  const got = await orCached(failing('HTTP 202, after 4 attempts'), { file: 'iota_groups.json', checkFn: check, cached: groups(), log: (l) => lines.push(l) })
  assert.equal(got.cached, true)
  assert.deepEqual(got.bytes, groups())
  assert.deepEqual(lines, ['gen-iota-groups: HTTP 202, after 4 attempts; using the cached copy of iota_groups.json instead'])
})

test('a downloaded file that fails the checks takes the cached bytes too', async () => {
  const got = await orCached(async () => Buffer.from('<html>maintenance</html>'), { file: 'iota_groups.json', checkFn: check, cached: groups(), log: quiet })
  assert.equal(got.cached, true)
})

test('with no cached bytes the failure stands, with the reason there were none', async () => {
  await assert.rejects(
    orCached(failing('HTTP 202, after 4 attempts'), { file: 'iota_groups.json', checkFn: check, missWhy: 'no cached copy in /x', log: quiet }),
    /^Error: HTTP 202, after 4 attempts; no usable cached copy \(no cached copy in \/x\)$/,
  )
  await assert.rejects(orCached(failing('HTTP 202'), { file: 'iota_groups.json', checkFn: check, log: quiet }), /^Error: HTTP 202$/)
})

test('CONTROL: a good download is used, not the cached bytes', async () => {
  const lines = []
  const got = await orCached(async () => groups(), { file: 'iota_groups.json', checkFn: check, cached: Buffer.from('stale'), log: (l) => lines.push(l) })
  assert.equal(got.cached, false)
  assert.deepEqual(got.bytes, groups())
  assert.deepEqual(lines, [])
})

const SCRIPT = fileURLToPath(new URL('./gen-iota-groups.mjs', import.meta.url))
const run = (...args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' })

test('the script stages the cached lists when its sources fail, dated as the cached copy', (t) => {
  const cache = cacheDir(t, 1_700_000_000)
  const out = tempDir(t)
  const gone = join(out, 'no-such-file.json')
  const r = run(gone, out, '--islands', gone, '--fallback', cache)
  assert.equal(r.status, 0, r.stderr)
  assert.match(r.stderr, /cached copy in .* checked \(fetched 2023-11-14T22:13:20\.000Z\)/)
  assert.match(r.stderr, /using the cached copy of iota_groups\.json instead/)
  assert.match(r.stderr, /using the cached copy of iota_islands\.json instead/)
  assert.match(r.stdout, /IOTA lists from the cached copy \(fetched 2023-11-14T22:13:20\.000Z\): iota_groups\.json, iota_islands\.json/)
  assert.deepEqual(readFileSync(join(out, 'iota_groups.json')), groups())
  assert.deepEqual(readFileSync(join(out, 'iota_islands.json')), islands(10_000))
  const manifest = JSON.parse(readFileSync(join(out, 'iota_groups.manifest.json'), 'utf8'))
  assert.equal(manifest.fetchedAt, 1_700_000_000, 'the cached copy keeps its own date')
  assert.equal(manifest.groups, 1000)
  assert.equal(manifest.islands, 10_000)
})

test('a cache directory with anything unexpected in it is a miss: the script fails and stages nothing', (t) => {
  const cache = cacheDir(t)
  writeFileSync(join(cache, 'postinstall.sh'), 'echo hi')
  const out = tempDir(t)
  const gone = join(out, 'no-such-file.json')
  const r = run(gone, out, '--islands', gone, '--fallback', cache)
  assert.equal(r.status, 1)
  assert.match(r.stderr, /cache miss: unexpected entries .*postinstall\.sh/)
  assert.deepEqual(readdirSync(out), [])
})

test('with neither a source nor a cached copy the script fails and stages nothing', (t) => {
  const empty = tempDir(t)
  const out = tempDir(t)
  const gone = join(out, 'no-such-file.json')
  const r = run(gone, out, '--islands', gone, '--fallback', empty)
  assert.equal(r.status, 1)
  assert.match(r.stderr, /cache miss: no cached copy/)
  assert.match(r.stderr, /no usable cached copy/)
  assert.equal(existsSync(join(out, 'iota_groups.json')), false)
  assert.equal(existsSync(join(out, 'iota_groups.manifest.json')), false)
})

test('CONTROL: good sources are staged as new, with the cache directory unused', (t) => {
  const cache = cacheDir(t)
  const src = tempDir(t)
  const out = tempDir(t)
  writeFileSync(join(src, 'groups.json'), groups())
  writeFileSync(join(src, 'islands.json'), islands(10_001))
  const before = Math.floor(Date.now() / 1000)
  const r = run(join(src, 'groups.json'), out, '--islands', join(src, 'islands.json'), '--fallback', cache)
  assert.equal(r.status, 0, r.stderr)
  assert.doesNotMatch(r.stderr + r.stdout, /using the cached copy|IOTA lists from the cached copy/)
  const manifest = JSON.parse(readFileSync(join(out, 'iota_groups.manifest.json'), 'utf8'))
  assert.ok(manifest.fetchedAt >= before, 'dated now')
  assert.equal(manifest.islands, 10_001)
})

test('--checked stages lists another job handed over, checked, with their own date and no download', (t) => {
  const handed = cacheDir(t, 1_700_000_000)
  const out = tempDir(t)
  const r = run('--checked', handed, out)
  assert.equal(r.status, 0, r.stderr)
  assert.match(r.stderr, /handed-over lists in .* checked \(fetched 2023-11-14T22:13:20\.000Z\)/)
  assert.doesNotMatch(r.stderr + r.stdout, /attempt|cache miss|using the cached copy/)
  assert.deepEqual(readFileSync(join(out, 'iota_groups.json')), groups())
  assert.deepEqual(readFileSync(join(out, 'iota_islands.json')), islands(10_000))
  const manifest = JSON.parse(readFileSync(join(out, 'iota_groups.manifest.json'), 'utf8'))
  assert.equal(manifest.fetchedAt, 1_700_000_000)
  assert.equal(manifest.islands, 10_000)
})

test('--checked refuses a hand-over that fails the cache checks, and stages nothing', (t) => {
  for (const spoil of [
    (d) => writeFileSync(join(d, 'extra.sh'), 'echo hi'),
    (d) => rmSync(join(d, 'iota_islands.json')),
    (d) => writeFileSync(join(d, 'iota_groups.json'), '[]'),
  ]) {
    const handed = cacheDir(t)
    spoil(handed)
    const out = tempDir(t)
    const r = run('--checked', handed, out)
    assert.equal(r.status, 1)
    assert.match(r.stderr, /the handed-over IOTA lists cannot be used: /)
    assert.deepEqual(readdirSync(out), [])
  }
  const r = run('--checked', cacheDir(t), tempDir(t), '--fallback', cacheDir(t))
  assert.equal(r.status, 1)
  assert.match(r.stderr, /--checked takes no --fallback/)
})
