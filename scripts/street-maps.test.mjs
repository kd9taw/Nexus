// Tests for the street map's hosting job (scripts/street-maps.mjs, .github/workflows/street-maps.yml).
//
// The job runs unattended and publishes to every install that downloads a street map: a wrong pick
// publishes a build the app cannot draw, a wrong index sends installs nowhere, and a wrong deletion
// pulls a build out from under a download in progress. None of it can run against the bucket from
// a test, so the rules are tested as functions, the bucket as a stand-in that answers exactly the
// rclone commands the job sends (and refuses any other), and the host as a local server that
// answers range reads. Every refusal is paired with a control that passes.
//
// Run: node --test scripts/street-maps.test.mjs

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { createServer } from 'node:http'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

import * as sm from './street-maps.mjs'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const WORKFLOW = readFileSync(join(ROOT, '.github', 'workflows', 'street-maps.yml'), 'utf8')
const HEX = (c) => c.repeat(64)
const scratch = () => mkdtempSync(join(tmpdir(), 'street-maps-'))

// ---- The publisher's builds ----------------------------------------------------------------------

const entry = (key, over = {}) => ({
  key,
  size: 138_572_716_818,
  md5sum: 'pgpGmW7ARd3EMLMruyiuWg==',
  b3sum: HEX('b'),
  uploaded: '2026-10-04T08:52:07.756Z',
  version: '4.15.2',
  ...over,
})

test('the newest build a copy can be checked against is picked, in any order', () => {
  const builds = [
    entry('20240920.pmtiles', { b3sum: undefined }),
    entry('20261003.pmtiles', { b3sum: HEX('3') }),
    entry('20261004.pmtiles', { b3sum: HEX('4') }),
    entry('20261005.pmtiles', { b3sum: undefined }),
  ]
  for (const list of [builds, [...builds].reverse()]) {
    const { build, skipped } = sm.pickBuild(list)
    assert.deepEqual(build, {
      id: '20261004',
      date: '2026-10-04',
      key: 'planet-20261004.pmtiles',
      upstream: 'https://build.protomaps.com/20261004.pmtiles',
      bytes: 138_572_716_818,
      version: '4.15.2',
      b3sum: HEX('4'),
    })
    assert.deepEqual(skipped, [{ key: '20261005.pmtiles', why: 'no b3sum to check a copy against' }])
  }
})

test('malformed entries are skipped, never picked', () => {
  const good = entry('20261001.pmtiles')
  for (const bad of [
    entry('2026-10-09.pmtiles'),
    entry('20261332.pmtiles'),
    entry('20261009.mbtiles'),
    entry('20261009.pmtiles', { size: -1 }),
    entry('20261009.pmtiles', { size: '138572716818' }),
    entry('20261009.pmtiles', { b3sum: HEX('B') }),
    entry('20261009.pmtiles', { version: 'v4' }),
    null,
  ]) {
    assert.equal(sm.pickBuild([good, bad]).build.id, '20261001', JSON.stringify(bad))
  }
  assert.equal(sm.pickBuild([good, entry('20261009.pmtiles')]).build.id, '20261009', 'control')
})

test('a newest build of another tile schema stops the run', () => {
  const builds = [entry('20261004.pmtiles'), entry('20261005.pmtiles', { version: '5.0.0' })]
  assert.throws(() => sm.pickBuild(builds), (e) => e instanceof sm.Refusal && /tiles 5\.0\.0/.test(e.message))
  builds[1].version = '4.16.0'
  assert.equal(sm.pickBuild(builds).build.version, '4.16.0', 'control')
})

test('a build too small to be a whole planet stops the run', () => {
  assert.throws(() => sm.pickBuild([entry('20261004.pmtiles', { size: 5e9 })]), /not a whole planet/)
  assert.equal(sm.pickBuild([entry('20261004.pmtiles', { size: 100e9 })]).build.bytes, 100e9, 'control')
})

test('an index with nothing usable stops the run', () => {
  assert.throws(() => sm.pickBuild({ builds: [] }), /not a list/)
  assert.throws(() => sm.pickBuild([]), /no usable build/)
  assert.throws(() => sm.pickBuild([entry('20261004.pmtiles', { b3sum: undefined })]), /no usable build \(1 skipped\)/)
})

// ---- The index, against the app's reader (crates/street-map/src/manifest.rs) ----------------------

const ORIGIN = 'https://maps.example.org'
// manifest.rs's own test index, so these cases are its cases.
const rustIndex = (buildUrl, assetsUrl) =>
  `{"schema":1,"later":"ignored","build":{"id":"20261004","date":"2026-10-04","url":"${buildUrl}","bytes":5},` +
  `"assets":{"url":"${assetsUrl}","sha256":"${'0f'.repeat(32)}"}}`

test("the reader's rules: a good index parses and unknown fields are ignored", () => {
  const m = sm.parseIndex(rustIndex(`${ORIGIN}/planet-20261004.pmtiles`, `${ORIGIN}/a.tar.gz`), ORIGIN)
  assert.equal(m.build.id, '20261004')
  assert.equal(m.build.bytes, 5)
  assert.equal(m.assets.bytes, undefined)
})

test("the reader's rules: an index pointing off the host is refused", () => {
  const good = `${ORIGIN}/a.tar.gz`
  for (const bad of [
    'https://evil.example/planet.pmtiles',
    'https://maps.example.org.evil.example/planet.pmtiles',
    'https://maps.example.org@evil.example/planet.pmtiles',
    'http://maps.example.org/planet.pmtiles',
    'https://maps.example.org',
    'https://maps.example.org/',
    'https://maps.example.org/a b.pmtiles',
  ]) {
    assert.throws(() => sm.parseIndex(rustIndex(bad, good), ORIGIN), sm.Refusal, bad)
    assert.throws(() => sm.parseIndex(rustIndex(good, bad), ORIGIN), sm.Refusal, `assets at ${bad}`)
  }
})

test("the reader's rules: malformed fields are refused", () => {
  const ok = rustIndex(`${ORIGIN}/p.pmtiles`, `${ORIGIN}/a.tar.gz`)
  assert.ok(sm.parseIndex(ok, ORIGIN), 'control')
  for (const [from, to] of [
    ['"schema":1', '"schema":2'],
    ['"20261004"', '"../x"'],
    ['"20261004"', '"Build_1"'],
    ['"2026-10-04"', '"4 Oct 2026"'],
    ['0f'.repeat(32), '0F'.repeat(32)],
    ['0f'.repeat(32), 'abc'],
  ]) {
    const bad = ok.replace(from, to)
    assert.notEqual(bad, ok)
    assert.throws(() => sm.parseIndex(bad, ORIGIN), sm.Refusal, to)
  }
  assert.throws(() => sm.parseIndex('[]', ORIGIN), sm.Refusal)
})

const BUILD = {
  id: '20261004',
  date: '2026-10-04',
  key: 'planet-20261004.pmtiles',
  upstream: 'https://build.protomaps.com/20261004.pmtiles',
  bytes: 138_572_716_818,
  version: '4.15.2',
  b3sum: HEX('b'),
}
const ASSETS = { key: sm.assetsKey(HEX('a')), sha256: HEX('a'), bytes: 7_123_456 }

// The index exactly as the host will serve it, so a change to its shape shows in review.
const GOLDEN = `{
  "schema": 1,
  "build": {
    "id": "20261004",
    "date": "2026-10-04",
    "url": "https://maps.hamradiotools.io/planet-20261004.pmtiles",
    "bytes": 138572716818,
    "version": "4.15.2",
    "upstream": "https://build.protomaps.com/20261004.pmtiles",
    "b3sum": "${HEX('b')}"
  },
  "assets": {
    "url": "https://maps.hamradiotools.io/street-assets-aaaaaaaaaaaaaaaa.tar.gz",
    "sha256": "${HEX('a')}",
    "bytes": 7123456
  },
  "attribution": "© OpenStreetMap",
  "license": "ODbL-1.0",
  "published": "2026-10-06T07:40:12Z"
}
`

test('the index the job writes is the one the app reads back: a round trip', () => {
  const text = sm.indexText(sm.buildIndex({ build: BUILD, assets: ASSETS, published: '2026-10-06T07:40:12Z' }))
  assert.equal(text, GOLDEN)
  const m = sm.parseIndex(text, 'https://maps.hamradiotools.io')
  assert.equal(m.schema, 1)
  const { key, ...named } = BUILD
  assert.deepEqual(m.build, { ...named, url: `https://maps.hamradiotools.io/${key}` })
  assert.deepEqual(m.assets, { url: `https://maps.hamradiotools.io/${ASSETS.key}`, sha256: ASSETS.sha256, bytes: ASSETS.bytes })
  assert.equal(m.attribution, '© OpenStreetMap')
  assert.equal(m.license, 'ODbL-1.0')
})

test('the writer refuses an index the app would refuse', () => {
  const index = sm.buildIndex({ build: { ...BUILD, id: 'Build_1' }, assets: ASSETS, published: '2026-10-06T07:40:12Z' })
  assert.throws(() => sm.indexText(index), /build id/)
  const elsewhere = sm.buildIndex({ build: BUILD, assets: ASSETS, published: 'x', origin: 'https://cdn.example' })
  assert.throws(() => sm.indexText(elsewhere), /is not on https:\/\/maps\.hamradiotools\.io/)
  assert.doesNotThrow(() => sm.indexText(sm.buildIndex({ build: BUILD, assets: ASSETS, published: 'x' })), 'control')
})

// ---- What a run does ---------------------------------------------------------------------------

const indexFor = (build, assets = ASSETS, published = '2026-07-06T07:00:00Z') => sm.buildIndex({ build, assets, published })
const older = { ...BUILD, id: '20260705', date: '2026-07-05', key: 'planet-20260705.pmtiles', b3sum: HEX('c') }

test('the workflow maps its schedules and its dispatch to operations', () => {
  assert.equal(sm.operationFor({ event: 'schedule', schedule: sm.COPY_CRON }), 'copy')
  assert.equal(sm.operationFor({ event: 'schedule', schedule: sm.RETIRE_CRON }), 'retire')
  assert.throws(() => sm.operationFor({ event: 'schedule', schedule: '0 0 * * *' }), /a schedule street-maps\.mjs does not know/)
  for (const op of sm.OPERATIONS) assert.equal(sm.operationFor({ event: 'workflow_dispatch', operation: op }), op)
  assert.throws(() => sm.operationFor({ event: 'workflow_dispatch', operation: 'deploy' }), sm.Refusal)
  assert.throws(() => sm.operationFor({ event: 'push', operation: '' }), sm.Refusal)
})

test('plan touches nothing; a copy by hand makes the first copy; the schedule never does', () => {
  const next = indexFor(BUILD)
  assert.equal(sm.decide({ operation: 'plan', scheduled: false, hosted: null, next }).action, 'none')
  assert.equal(sm.decide({ operation: 'plan', scheduled: false, hosted: indexFor(older), next }).action, 'none')
  assert.deepEqual(sm.decide({ operation: 'copy', scheduled: false, hosted: null, next }), { action: 'copy', why: 'the first copy, asked for by hand' })
  const first = sm.decide({ operation: 'copy', scheduled: true, hosted: null, next })
  assert.equal(first.action, 'none')
  assert.match(first.why, /made by hand \(operation=copy\), never by the schedule/)
  assert.equal(sm.decide({ operation: 'copy', scheduled: true, hosted: indexFor(older), next }).action, 'copy', 'control: the schedule refreshes')
})

test('a copy happens only when the index would name something new', () => {
  const next = indexFor(BUILD, ASSETS, '2026-10-06T07:40:12Z')
  const same = sm.decide({ operation: 'copy', scheduled: true, hosted: indexFor(BUILD, ASSETS), next })
  assert.equal(same.action, 'none', 'the same build and archive, published earlier')
  const fonts = sm.decide({ operation: 'copy', scheduled: true, hosted: indexFor(BUILD, { ...ASSETS, sha256: HEX('d'), key: sm.assetsKey(HEX('d')) }), next })
  assert.deepEqual(fonts, { action: 'copy', why: 'new fonts and icons for build 20261004' })
  const newer = { ...BUILD, id: '20261101', date: '2026-11-01', key: 'planet-20261101.pmtiles' }
  assert.equal(sm.decide({ operation: 'copy', scheduled: false, hosted: indexFor(newer), next }).action, 'none', 'never back to an older build')
  assert.throws(
    () => sm.decide({ operation: 'copy', scheduled: false, hosted: indexFor({ ...BUILD, b3sum: HEX('e') }), next }),
    /never replaced in place/,
  )
})

test('retire runs only when something is hosted', () => {
  assert.equal(sm.decide({ operation: 'retire', scheduled: true, hosted: null, next: indexFor(BUILD) }).action, 'none')
  assert.equal(sm.decide({ operation: 'retire', scheduled: true, hosted: indexFor(BUILD), next: indexFor(BUILD) }).action, 'retire')
})

// ---- Retirement --------------------------------------------------------------------------------

const PUBLISHED = '2026-10-06T07:40:12Z'
const hostedText = sm.indexText(indexFor(BUILD, ASSETS, PUBLISHED))
const NAMES = [
  'streetmaps.json',
  'planet-20261004.pmtiles',
  'planet-20260705.pmtiles',
  ASSETS.key,
  'street-assets-0123456789abcdef.tar.gz',
  'notes.txt',
]
const at = (iso) => Date.parse(iso)

test('a replaced build and archive stay for the overlap, then go; nothing else ever does', () => {
  const early = sm.retirement({ indexText: hostedText, names: NAMES, now: at('2026-10-20T07:40:11Z') })
  assert.deepEqual(early.remove, [])
  assert.deepEqual(early.keep, ['planet-20260705.pmtiles', 'street-assets-0123456789abcdef.tar.gz'])
  assert.equal(early.due, '2026-10-20T07:40:12Z')
  const due = sm.retirement({ indexText: hostedText, names: NAMES, now: at('2026-10-20T07:40:12Z') })
  assert.deepEqual(due.remove, ['planet-20260705.pmtiles', 'street-assets-0123456789abcdef.tar.gz'])
  assert.deepEqual(due.current, ['planet-20261004.pmtiles', ASSETS.key])
  assert.deepEqual(due.other, ['notes.txt'])
  const later = sm.retirement({ indexText: hostedText, names: NAMES, now: at('2027-01-01T00:00:00Z') })
  for (const kept of ['streetmaps.json', 'planet-20261004.pmtiles', ASSETS.key, 'notes.txt']) assert.ok(!later.remove.includes(kept), kept)
})

test('retirement deletes nothing when it cannot be sure what the index names', () => {
  const now = at('2027-01-01T00:00:00Z')
  assert.throws(() => sm.retirement({ indexText: hostedText, names: NAMES.filter((n) => n !== ASSETS.key), now }), /does not hold/)
  const undated = sm.indexText({ ...indexFor(BUILD), published: undefined })
  assert.throws(() => sm.retirement({ indexText: undated, names: NAMES, now }), /no publish time/)
  assert.throws(() => sm.retirement({ indexText: '{', names: NAMES, now }), /does not parse/)
  assert.ok(sm.retirement({ indexText: hostedText, names: NAMES, now }).remove.length === 2, 'control')
})

// ---- The bucket and the host -------------------------------------------------------------------

const DEST = 'r2:bucket'
const fakeB3 = (b) => createHash('sha256').update('blake3:').update(b).digest('hex')
const PLANET = Buffer.concat([Buffer.from('PMTiles'), Buffer.from([3]), Buffer.alloc(4088, 7)])
const ARCHIVE = Buffer.from('a gzip ustar archive of fonts and icons')

/// rclone, for the commands the job sends and nothing else; `upstream` is what copyurl can fetch.
function fakeRclone(store, upstream = new Map()) {
  const calls = []
  const key = (p) => {
    assert.ok(p.startsWith(`${DEST}/`), p)
    return p.slice(DEST.length + 1)
  }
  const rclone = (args) => {
    calls.push(args.join(' '))
    const a = args
    if (a.length === 5 && a[0] === 'lsjson' && a.slice(1, 4).join(' ') === '--files-only --no-mimetype --no-modtime' && a[4] === DEST) {
      return JSON.stringify([...store].map(([Path, b]) => ({ Path, Name: Path, Size: b.length, IsDir: false })))
    }
    if (a.length === 2 && a[0] === 'cat') {
      if (!store.has(key(a[1]))) throw new Error(`rclone cat: ${a[1]}: object not found`)
      return store.get(key(a[1])).toString('utf8')
    }
    if (a.length === 4 && a[0] === 'copyurl' && a[1] === '--no-clobber') {
      if (store.has(key(a[3]))) throw new Error('CopyURL failed: file already exist')
      if (!upstream.has(a[2])) throw new Error(`copyurl: ${a[2]}: 404`)
      store.set(key(a[3]), upstream.get(a[2]))
      return ''
    }
    if (a.length === 4 && a[0] === 'hashsum' && a[2] === '--download') {
      const b = store.get(key(a[3]))
      return `${a[1] === 'sha256' ? sm.sha256(b) : a[1] === 'blake3' ? fakeB3(b) : assert.fail(a[1])}  ${key(a[3])}\n`
    }
    if (a[0] === 'copyto' && (a.length === 3 || (a.length === 5 && a[1] === '--header-upload' && a[2] === 'Cache-Control: no-cache'))) {
      store.set(key(a.at(-1)), readFileSync(a.at(-2)))
      return ''
    }
    if (a.length === 2 && a[0] === 'deletefile') {
      store.delete(key(a[1]))
      return ''
    }
    throw new Error(`the stand-in does not model: rclone ${a.join(' ')}`)
  }
  return { rclone, calls }
}

/// The custom domain in front of the bucket: serves `store`, answering ranges with 206.
async function host(store, { ignoreRange = false, index = null } = {}) {
  const server = createServer((req, res) => {
    const name = decodeURIComponent(req.url.slice(1))
    const body = name === 'streetmaps.json' && index !== null ? Buffer.from(index) : store.get(name)
    if (!body) return res.writeHead(404).end()
    const m = /^bytes=(\d+)-(\d+)$/.exec(req.headers.range ?? '')
    if (m && !ignoreRange) {
      const [from, to] = [+m[1], Math.min(+m[2], body.length - 1)]
      res.writeHead(206, { 'content-range': `bytes ${from}-${to}/${body.length}` })
      return res.end(body.subarray(from, to + 1))
    }
    res.writeHead(200).end(body)
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  return { server, origin: `http://127.0.0.1:${server.address().port}` }
}

const UPSTREAM = 'https://build.example'
const NOW = at('2026-10-06T09:00:00Z')

/// The plan job's files for a copy of `planet` (named in the index with `b3` as its b3sum).
function planFiles(origin, { b3 = fakeB3(PLANET), upstream = `${UPSTREAM}/20261004.pmtiles` } = {}) {
  const from = scratch()
  const build = { ...BUILD, bytes: PLANET.length, b3sum: b3, upstream }
  const assets = { key: sm.assetsKey(sm.sha256(ARCHIVE)), sha256: sm.sha256(ARCHIVE), bytes: ARCHIVE.length }
  writeFileSync(join(from, 'streetmaps.json'), sm.indexText(sm.buildIndex({ build, assets, published: '2026-10-06T05:30:00Z', origin }), origin))
  writeFileSync(join(from, 'assets.tar.gz'), ARCHIVE)
  return { from, build, assets }
}

async function copy(store, { upstream, hostOptions, files } = {}) {
  const h = await host(store, hostOptions)
  try {
    const f = files ? files(h.origin) : planFiles(h.origin)
    const fake = fakeRclone(store, upstream ?? new Map([[`${UPSTREAM}/20261004.pmtiles`, PLANET]]))
    const run = sm.copyToBucket({
      from: f.from,
      dest: DEST,
      rclone: fake.rclone,
      now: () => NOW,
      origin: h.origin,
      upstreamOrigin: UPSTREAM,
      minBytes: 0,
      wait: async () => {},
      log: () => {},
    })
    return { result: await run.then((text) => ({ text }), (error) => ({ error })), calls: fake.calls, ...f, origin: h.origin }
  } finally {
    h.server.close()
  }
}

test('a first copy copies once, checks it as stored and through the host, and publishes the index last', async () => {
  const store = new Map()
  const { result, calls, assets, origin } = await copy(store)
  assert.equal(result.error, undefined, result.error?.message)
  assert.deepEqual(calls, [
    `lsjson --files-only --no-mimetype --no-modtime ${DEST}`,
    `copyurl --no-clobber ${UPSTREAM}/20261004.pmtiles ${DEST}/planet-20261004.pmtiles`,
    `lsjson --files-only --no-mimetype --no-modtime ${DEST}`,
    `hashsum blake3 --download ${DEST}/planet-20261004.pmtiles`,
    calls[4], // the archive's upload, from the plan's folder
    `lsjson --files-only --no-mimetype --no-modtime ${DEST}`,
    `hashsum sha256 --download ${DEST}/${assets.key}`,
    calls[7], // the index's upload, last
    `cat ${DEST}/streetmaps.json`,
  ])
  assert.match(calls[4], new RegExp(`^copyto \\S+/assets\\.tar\\.gz ${DEST}/${assets.key}$`))
  assert.match(calls[7], new RegExp(`^copyto --header-upload Cache-Control: no-cache \\S+ ${DEST}/streetmaps\\.json$`))
  const published = sm.parseIndex(store.get('streetmaps.json').toString(), origin)
  assert.equal(published.published, '2026-10-06T09:00:00Z', 'stamped when it goes live, not when it was planned')
  assert.equal(store.get('streetmaps.json').toString(), result.text)
})

test('a stored build that fails its check is deleted unpublished, and nothing is published', async () => {
  const store = new Map()
  const damaged = Buffer.from(PLANET)
  damaged[100] ^= 1
  const { result, calls } = await copy(store, { upstream: new Map([[`${UPSTREAM}/20261004.pmtiles`, damaged]]) })
  assert.match(result.error?.message ?? '', /planet-20261004\.pmtiles has blake3 \w+, not \w+; nothing was published/)
  assert.ok(calls.includes(`deletefile ${DEST}/planet-20261004.pmtiles`))
  assert.deepEqual([...store.keys()], [], 'no build, no archive, no index')
})

test('a build already in the bucket is checked, never copied over', async () => {
  const store = new Map([['planet-20261004.pmtiles', PLANET]])
  const { result, calls } = await copy(store)
  assert.equal(result.error, undefined, result.error?.message)
  assert.ok(!calls.some((c) => c.startsWith('copyurl')), calls.join('\n'))
  assert.ok(calls.includes(`hashsum blake3 --download ${DEST}/planet-20261004.pmtiles`))
  assert.ok(store.has('streetmaps.json'))
})

test('the build the host serves is not copied or read again when only the fonts and icons change', async () => {
  const store = new Map([['planet-20261004.pmtiles', PLANET]])
  const { result, calls, origin } = await copy(store, {
    files: (origin) => {
      const f = planFiles(origin)
      const old = { key: sm.assetsKey(HEX('d')), sha256: HEX('d'), bytes: 3 }
      store.set(old.key, Buffer.from('old'))
      store.set('streetmaps.json', Buffer.from(sm.indexText(sm.buildIndex({ build: f.build, assets: old, published: '2026-07-06T07:00:00Z', origin }), origin)))
      return f
    },
  })
  assert.equal(result.error, undefined, result.error?.message)
  assert.ok(!calls.some((c) => c.startsWith('copyurl') || c.startsWith('hashsum blake3')), calls.join('\n'))
  assert.equal(sm.parseIndex(store.get('streetmaps.json').toString(), origin).assets.sha256, sm.sha256(ARCHIVE))
})

test('nothing is written when the bucket already names the same build and archive', async () => {
  const store = new Map([['planet-20261004.pmtiles', PLANET]])
  const { result, calls } = await copy(store, {
    files: (origin) => {
      const f = planFiles(origin)
      store.set(f.assets.key, ARCHIVE)
      store.set('streetmaps.json', Buffer.from(readFileSync(join(f.from, 'streetmaps.json'))))
      return f
    },
  })
  assert.equal(result.text, null)
  assert.deepEqual(calls, [`lsjson --files-only --no-mimetype --no-modtime ${DEST}`, `cat ${DEST}/streetmaps.json`])
})

test('a host that ignores the range read stops the copy before the index is published', async () => {
  const store = new Map()
  const { result } = await copy(store, { hostOptions: { ignoreRange: true } })
  assert.match(result.error?.message ?? '', /answered a range read with HTTP 200, not 206/)
  assert.ok(!store.has('streetmaps.json'))
  assert.ok(store.has('planet-20261004.pmtiles'), 'the checked build stays for the next run')
})

test('a host that keeps serving the old index fails the run', async () => {
  const store = new Map()
  const { result } = await copy(store, { hostOptions: { index: '{"stale": true}' } })
  assert.match(result.error?.message ?? '', /does not serve the index just written/)
})

test("the bucket job re-checks the plan job's files and acts on nothing else", async () => {
  const offPublisher = await copy(new Map(), { files: (origin) => planFiles(origin, { upstream: 'https://evil.example/20261004.pmtiles' }) })
  assert.match(offPublisher.result.error?.message ?? '', /is not the publisher's build 20261004/)
  assert.deepEqual(offPublisher.calls, [], 'refused before rclone ran')
  const swapped = await copy(new Map(), {
    files: (origin) => {
      const f = planFiles(origin)
      writeFileSync(join(f.from, 'assets.tar.gz'), 'another archive')
      return f
    },
  })
  assert.match(swapped.result.error?.message ?? '', /is not the archive the index names/)
  assert.deepEqual(swapped.calls, [])
})

test('a retire run deletes what is due from the bucket and nothing else', () => {
  const store = new Map(NAMES.map((n) => [n, Buffer.from(n === 'streetmaps.json' ? hostedText : 'x')]))
  const { rclone, calls } = fakeRclone(store)
  const r = sm.retireFromBucket({ dest: DEST, rclone, now: at('2026-10-22T05:23:00Z'), log: () => {} })
  assert.deepEqual(r.remove, ['planet-20260705.pmtiles', 'street-assets-0123456789abcdef.tar.gz'])
  assert.deepEqual(calls.filter((c) => c.startsWith('deletefile')), r.remove.map((k) => `deletefile ${DEST}/${k}`))
  assert.deepEqual([...store.keys()].sort(), ['notes.txt', 'planet-20261004.pmtiles', ASSETS.key, 'streetmaps.json'].sort())
  const empty = fakeRclone(new Map([['planet-20260705.pmtiles', Buffer.from('x')]]))
  assert.throws(() => sm.retireFromBucket({ dest: DEST, rclone: empty.rclone, log: () => {} }), /holds no streetmaps\.json/)
  assert.ok(!empty.calls.some((c) => c.startsWith('deletefile')))
})

test('the hosted index: none yet is told apart from could-not-tell', async () => {
  const h = await host(new Map([['streetmaps.json', Buffer.from(hostedText)]]))
  try {
    // Over real HTTP: an index naming another host than the one serving it is refused.
    await assert.rejects(sm.readHosted({ origin: h.origin }), /is not on http:\/\/127\.0\.0\.1/)
  } finally {
    h.server.close()
  }
  const answer = (status, body = '') => async () => new Response(body, { status })
  assert.deepEqual(await sm.readHosted({ fetchImpl: answer(404) }), { index: null, why: 'https://maps.hamradiotools.io/streetmaps.json answers 404' })
  assert.equal((await sm.readHosted({ fetchImpl: answer(200, hostedText) })).index.build.id, '20261004')
  await assert.rejects(sm.readHosted({ fetchImpl: answer(503) }), /HTTP 503/)
  const failing = (code) => async () => {
    throw new TypeError('fetch failed', { cause: { code } })
  }
  assert.deepEqual(await sm.readHosted({ fetchImpl: failing('ENOTFOUND') }), { index: null, why: 'maps.hamradiotools.io does not resolve' })
  await assert.rejects(sm.readHosted({ fetchImpl: failing('ECONNREFUSED') }), /could not read .* ECONNREFUSED/)
})

// ---- The fonts and icons -----------------------------------------------------------------------

test("the townhall icon is added to the sprite generator's flavors, in the style's colour group", () => {
  const upstream = JSON.stringify({ flavors: { base: ['#555555', '#ffffff'], slategray: ['#6A5B8F', '#E1F2FF'] }, icons: { library: 'slategray' } })
  const patched = JSON.parse(sm.patchFlavor(upstream))
  assert.deepEqual(patched.icons, { library: 'slategray', townhall: 'slategray' })
  assert.equal(sm.patchFlavor(sm.patchFlavor(upstream)), sm.patchFlavor(upstream), 'idempotent')
  assert.throws(() => sm.patchFlavor(JSON.stringify({ flavors: { base: [] }, icons: {} })), /no slategray colours/)
  assert.ok(Object.keys(sm.SPRITE_ADDITIONS).every((icon) => sm.STYLE_ICONS.includes(icon)))
})

test("archive names follow the app's asset path rule", () => {
  for (const ok of ['OFL.txt', 'fonts/Noto Sans Regular/0-255.pbf', 'sprites/light@2x.png', 'a/b/c/d']) assert.ok(sm.assetPathOk(ok), ok)
  for (const bad of ['', '../packs.json', '/etc/x', 'a\\b', 'a:b', 'a/b/c/d/e', '.hidden', 'trail.', 'trail ', 'CON', 'fonts/com1.pbf', 'a//b', 'x\0y']) {
    assert.ok(!sm.assetPathOk(bad), JSON.stringify(bad))
  }
})

/// A staged archive tree as street-maps-assets.sh builds it, less the files in `omit`, with the
/// icons in `drop` left out of the dark 2x sheet.
function assetsTree({ omit = [], drop = [] } = {}) {
  const dir = scratch()
  const put = (p, body) => {
    if (omit.includes(p)) return
    mkdirSync(dirname(join(dir, p)), { recursive: true })
    writeFileSync(join(dir, p), body)
  }
  put('OFL.txt', 'SIL Open Font License')
  put('ICONS-LICENSE.txt', 'MIT')
  for (const stack of sm.FONT_STACKS) for (let i = 0; i < 256; i++) put(`fonts/${stack}/${i * 256}-${i * 256 + 255}.pbf`, 'g')
  for (const sheet of sm.SPRITE_LOOKS.flatMap((look) => [look, `${look}@2x`])) {
    const names = sheet === 'dark@2x' ? sm.STYLE_ICONS.filter((n) => !drop.includes(n)) : sm.STYLE_ICONS
    put(`sprites/${sheet}.json`, JSON.stringify(Object.fromEntries(names.map((n, i) => [n, { x: i, y: 0, width: 1, height: 1, pixelRatio: 1 }]))))
    put(`sprites/${sheet}.png`, 'png')
  }
  return dir
}

test('the archive holds exactly what the app asks for, and every icon the style names', () => {
  assert.equal(sm.checkAssetsTree(assetsTree()).files, 2 + 3 * 256 + 8, 'control')
  assert.throws(() => sm.checkAssetsTree(assetsTree({ drop: ['townhall'] })), (e) => e.message === `sprites/dark@2x.json lacks icons the style ${sm.STYLE_VERSION} names: townhall`)
})

test('an archive missing a licence or a glyph range, or holding anything else, is refused', () => {
  assert.throws(() => sm.checkAssetsTree(assetsTree({ omit: ['ICONS-LICENSE.txt'] })), /lacks 1 file\(s\): ICONS-LICENSE\.txt/)
  assert.throws(() => sm.checkAssetsTree(assetsTree({ omit: ['fonts/Noto Sans Medium/65280-65535.pbf'] })), /lacks 1 file\(s\): fonts\/Noto Sans Medium\/65280-65535\.pbf/)
  const extra = assetsTree()
  writeFileSync(join(extra, 'fonts', 'Noto Sans Italic', 'extra.pbf'), 'g')
  assert.throws(() => sm.checkAssetsTree(extra), /Nexus never asks for: fonts\/Noto Sans Italic\/extra\.pbf/)
})

// ---- The style the app draws -------------------------------------------------------------------

const lockStyle = () => {
  const lock = join(ROOT, 'ui', 'package-lock.json')
  return existsSync(lock) ? JSON.parse(readFileSync(lock, 'utf8')).packages?.['node_modules/@protomaps/basemaps'] : undefined
}

test('the sprites are generated for the style version the app ships', (t) => {
  const style = lockStyle()
  if (!style) return t.skip("the app does not ship the street map's style yet")
  assert.equal(style.version, sm.STYLE_VERSION, `the app's @protomaps/basemaps moved: re-derive STYLE_ICONS and repin street-maps-assets.sh`)
})

/// Every name an icon-image expression of the style can produce: the shapes basemaps uses
/// (a literal; `match` on the POI kind, whose values come from the layer's filter; `concat` with a
/// shield text of one to five characters; `step` and `case`). Any other shape fails the test.
function styleIcons(layers) {
  const out = new Set()
  const kindsOf = (filter) => {
    const found = []
    const walk = (e) => {
      if (!Array.isArray(e)) return
      if (e[0] === 'in' && JSON.stringify(e[1]) === '["get","kind"]' && e[2]?.[0] === 'literal') found.push(...e[2][1])
      e.forEach(walk)
    }
    walk(filter)
    return found
  }
  const values = (e, kinds) => {
    if (typeof e === 'string') return [e]
    const [op, ...a] = e
    if (op === 'get' && a[0] === 'kind') return kinds
    if (op === 'match' && JSON.stringify(a[0]) === '["get","kind"]') {
      const pairs = a.slice(1, -1)
      return kinds.flatMap((k) => {
        for (let i = 0; i < pairs.length; i += 2) if ([pairs[i]].flat().includes(k)) return values(pairs[i + 1], [k])
        return values(a.at(-1), [k])
      })
    }
    if (op === 'match') return [...a.slice(1, -1).filter((_, i) => i % 2 === 1), a.at(-1)].flatMap((x) => values(x, kinds))
    if (op === 'concat') {
      const parts = a.map((x) => (x[0] === 'length' ? ['1', '2', '3', '4', '5'] : values(x, kinds)))
      return parts.reduce((acc, p) => acc.flatMap((s) => p.map((x) => s + x)), [''])
    }
    if (op === 'step') return [a[1], ...a.slice(2).filter((_, i) => i % 2 === 1)].flatMap((x) => values(x, kinds))
    if (op === 'case') return [...a.filter((_, i) => i % 2 === 1), a.at(-1)].flatMap((x) => values(x, kinds))
    throw new Error(`an icon-image this test does not model: ${JSON.stringify(e)}`)
  }
  for (const l of layers) if (l.layout?.['icon-image'] !== undefined) for (const v of values(l.layout['icon-image'], kindsOf(l.filter))) if (v) out.add(v)
  return [...out].sort()
}

test('STYLE_ICONS is every icon the style can name', async (t) => {
  assert.deepEqual([...sm.STYLE_ICONS], [...new Set(sm.STYLE_ICONS)].sort(), 'sorted, no repeats')
  const pkg = join(ROOT, 'ui', 'node_modules', '@protomaps', 'basemaps')
  if (!existsSync(join(pkg, 'package.json'))) return t.skip("the app's node_modules has no street map style")
  if (JSON.parse(readFileSync(join(pkg, 'package.json'), 'utf8')).version !== sm.STYLE_VERSION) return t.skip('another style version (the test above fails)')
  const style = await import(pathToFileURL(join(pkg, 'dist', 'esm', 'index.js')).href)
  for (const look of sm.SPRITE_LOOKS) {
    assert.deepEqual(styleIcons(style.layers('protomaps', style.namedFlavor(look), { lang: 'en' })), sm.STYLE_ICONS, look)
  }
})

// ---- The workflow ------------------------------------------------------------------------------

test('the workflow: dispatch defaults to plan, and its schedules are the ones mapped here', () => {
  const op = WORKFLOW.match(/\n {6}operation:\n((?: {8}.*\n)+)/)?.[1] ?? ''
  assert.match(op, /\n {8}default: plan\n/)
  assert.match(op, new RegExp(`\\n {8}options: \\[${sm.OPERATIONS.join(', ')}\\]\\n`))
  const crons = [...WORKFLOW.matchAll(/^ {4}- cron: '([^']+)'$/gm)].map((m) => m[1])
  assert.deepEqual(crons, [sm.COPY_CRON, sm.RETIRE_CRON])
})

test("the workflow: the bucket's token reaches only the bucket job's rclone steps", () => {
  const lines = WORKFLOW.split('\n')
  let job = null
  const refs = []
  lines.forEach((line, n) => {
    const j = line.match(/^ {2}([a-z][a-z0-9-]*):$/)
    if (j) job = j[1]
    if (/secrets/.test(line) && !/^\s*#/.test(line)) refs.push({ job, line, n })
  })
  assert.equal(refs.length, 6, refs.map((r) => r.line).join('\n'))
  for (const r of refs) {
    assert.equal(r.job, 'bucket', r.line)
    assert.match(r.line, /^ {10}RCLONE_CONFIG_R2_(ACCESS_KEY_ID|SECRET_ACCESS_KEY|ENDPOINT): .*\$\{\{ secrets\.(R2_ACCESS_KEY_ID|R2_SECRET_ACCESS_KEY|R2_ACCOUNT_ID) \}\}/, r.line)
    // step scope: the nearest key above at the step's indentation is its env.
    const above = lines.slice(0, r.n).reverse().find((l) => /^ {8}\S/.test(l))
    assert.equal(above, '        env:', r.line)
  }
  assert.match(WORKFLOW, /\n {2}bucket:\n(?: {4}.*\n)*? {4}environment: street-maps\n/)
  assert.doesNotMatch(WORKFLOW, /set -x|toJSON\(secrets\)/)
})

test('the workflow: rclone is a pinned release, checked against its SHA-256 before it runs', () => {
  const install = WORKFLOW.match(/- name: Install rclone[^\n]*\n {8}run: \|\n((?: {10}.*\n)+)/)?.[1] ?? ''
  assert.match(install, /https:\/\/downloads\.rclone\.org\/v1\.75\.1\/rclone-v1\.75\.1-linux-amd64\.zip/)
  assert.match(install, /echo "[0-9a-f]{64} {2}\$RUNNER_TEMP\/rclone\.zip" \| sha256sum -c -/)
})
