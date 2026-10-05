#!/usr/bin/env node
// The street map's host: what .github/workflows/street-maps.yml copies to maps.hamradiotools.io,
// and the index Nexus reads from it.
//
// WHAT IS HOSTED. One Protomaps basemap build (the planet as a single PMTiles file, about 139 GB
// of OpenStreetMap data under the ODbL), copied from its publisher into a Cloudflare R2 bucket
// that the custom domain STREET_MAP_ORIGIN serves; the fonts and icons the street map draws with,
// as one archive (scripts/street-maps-assets.sh builds it); and `streetmaps.json`, the index
// Nexus reads. crates/street-map/src/manifest.rs is that index's reader and
// crates/street-map/src/assets.rs the archive's installer. Nexus downloads only an area of the
// build, with range reads, so the host serves the file whole and unchanged.
//
// THE RULES, each tested in scripts/street-maps.test.mjs:
//   1. A build lives at an immutable key, planet-YYYYMMDD.pmtiles, never overwritten, so a
//      download can never mix two builds. The archive's key carries its SHA-256 for the same
//      reason.
//   2. Nothing is published until it is checked AS STORED: the build's size and the publisher's
//      BLAKE3 (b3sum), the archive's SHA-256, then both read back through the host, the build by
//      a range read as Nexus reads it. The index goes last; writing it is the switch.
//   3. A build or archive the index stopped naming is deleted once the index has named its
//      successor for RETAIN_DAYS (`published` records when), so a download begun on the old index
//      can finish.
//   4. The schedule refreshes a hosted copy and never makes the first one: hosting the first copy
//      is a publish, made by hand (`-f operation=copy`).
//   5. Only a build of the tile schema the app's style draws is copied (TILES_MAJOR), and the
//      icons are generated for that style's own version (STYLE_VERSION, STYLE_ICONS).
//
// Nothing here reads a credential. The workflow's bucket job runs `copy` and `retire` with the
// bucket's token in rclone's environment; this file only hands rclone its arguments.
//
// Run: node scripts/street-maps.mjs plan|copy|retire|flavor|check-assets (see main()).

import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { appendFileSync, lstatSync, readFileSync, readdirSync, realpathSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

/// The street-map host. crates/street-map/src/lib.rs `STREET_MAP_HOST` names the same one: Nexus
/// fetches `${STREET_MAP_ORIGIN}/streetmaps.json` and refuses an index whose URLs are elsewhere.
export const STREET_MAP_ORIGIN = 'https://maps.hamradiotools.io'
export const INDEX_NAME = 'streetmaps.json'
/// What the plan job hands the bucket job: the index a copy would publish, the archive, and the
/// decision with its reasons (for people; the bucket job re-derives what it acts on).
export const ASSETS_FILE = 'assets.tar.gz'
export const PLAN_FILE = 'plan.json'

/// The publisher's build index (per build: key, size, md5sum, b3sum, uploaded, version; oldest
/// first) and where the builds are served. The publisher keeps a build for about a week and asks
/// for a copy instead of hotlinking.
export const BUILDS_INDEX = 'https://build-metadata.protomaps.dev/builds.json'
export const BUILDS_ORIGIN = 'https://build.protomaps.com'

/// The tile schema the app's style draws: basemaps tiles 4.x. A build of another major version
/// renames layers or kinds, so copying it would break every pack.
export const TILES_MAJOR = 4
/// A whole planet build is 138.6 GB (2026-10-04) and has only grown. One far smaller is a failed
/// or partial build, never a newer planet.
export const MIN_BUILD_BYTES = 100e9

export const SCHEMA = 1
export const RETAIN_DAYS = 14
export const ATTRIBUTION = '© OpenStreetMap'
export const LICENSE = 'ODbL-1.0'

/// The workflow's two schedules, mapped to operations here (the test holds the workflow's cron
/// lines equal to these). 05:23 UTC is after the publisher's daily build, and off the hour.
export const COPY_CRON = '23 5 6 1,4,7,10 *' // quarterly, on the 6th of Jan, Apr, Jul and Oct
export const RETIRE_CRON = '23 5 22 * *' // monthly, so a quarterly copy's predecessor goes 16 days on
export const OPERATIONS = ['plan', 'copy', 'retire']

export const PLANET_KEY = /^planet-(\d{8})\.pmtiles$/
export const ASSETS_KEY = /^street-assets-([0-9a-f]{16})\.tar\.gz$/

/// The style the app draws the street map with: ui/package-lock.json's @protomaps/basemaps. The
/// sprites are generated from this version's own commit (scripts/street-maps-assets.sh), and the
/// test fails when the app's lock names another version.
export const STYLE_VERSION = '5.7.2'
/// Every icon name that style can ask its sprite sheets for, in its light and dark flavors: the
/// POI kinds (station drawn as train_station), the place symbols, the one-way arrow and the road
/// shields of one to five characters. Derived by evaluating its icon-image expressions; the test
/// derives it again whenever the app's node_modules holds the style.
export const STYLE_ICONS = [
  'NL:S-road-1char', 'NL:S-road-2char', 'NL:S-road-3char', 'NL:S-road-4char', 'NL:S-road-5char',
  'US:I-1char', 'US:I-2char', 'US:I-3char', 'US:I-4char', 'US:I-5char',
  'aerodrome', 'animal', 'arrow', 'artwork', 'attraction', 'bar', 'beach', 'beauty', 'bench',
  'books', 'building', 'bus_stop', 'cafe', 'capital', 'clothes', 'convenience', 'drinking_water',
  'electronics', 'fast_food', 'ferry_terminal', 'forest', 'garden',
  'generic_shield-1char', 'generic_shield-2char', 'generic_shield-3char', 'generic_shield-4char',
  'generic_shield-5char', 'library', 'marina', 'museum', 'park', 'peak', 'post_office',
  'restaurant', 'school', 'stadium', 'supermarket', 'theatre', 'toilets', 'townhall', 'townspot',
  'train_station', 'university', 'zoo',
]
/// Icons the style names that upstream's sprite generator leaves out, with the colour group the
/// style draws them in. The source drawing has the icon (sprites/refill.svg, `_townhall`), but the
/// generator's flavor lists and the published v4 sheets omit it, so MapLibre warns and a town hall
/// shows its name with no icon.
export const SPRITE_ADDITIONS = { townhall: 'slategray' }
export const SPRITE_LOOKS = ['light', 'dark']
export const FONT_STACKS = ['Noto Sans Regular', 'Noto Sans Medium', 'Noto Sans Italic']

// crates/street-map/src/assets.rs: the archive Nexus installs.
const MAX_ARCHIVE = 64 * 2 ** 20
const MAX_UNPACKED = 128 * 2 ** 20
const MAX_FILES = 20_000
const REQUIRED = ['OFL.txt', 'ICONS-LICENSE.txt']

const UA = 'Nexus street-map host (+https://github.com/kd9taw/Nexus)'
const DAY_MS = 86_400_000

/// A refusal: the run stops and changes nothing more.
export class Refusal extends Error {}

export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex')
export const assetsKey = (sha) => `street-assets-${sha.slice(0, 16)}.tar.gz`
const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000))
const iso = (ms) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')

// ---- The publisher's builds ----------------------------------------------------------------------

const BUILD_KEY = /^(\d{4})(\d{2})(\d{2})\.pmtiles$/

const calendarDay = (y, m, d) => new Date(Date.UTC(+y, +m - 1, +d)).toISOString().startsWith(`${y}-${m}-${d}`)

/// Why the publisher's entry cannot be copied, or null when it can.
function unusable(b) {
  if (b === null || typeof b !== 'object') return 'not an object'
  const m = typeof b.key === 'string' ? b.key.match(BUILD_KEY) : null
  if (!m || !calendarDay(m[1], m[2], m[3])) return 'its key is not YYYYMMDD.pmtiles'
  if (!Number.isSafeInteger(b.size) || b.size <= 0) return 'no size'
  if (typeof b.b3sum !== 'string' || !/^[0-9a-f]{64}$/.test(b.b3sum)) return 'no b3sum to check a copy against'
  if (typeof b.version !== 'string' || !/^\d+\.\d+\.\d+$/.test(b.version)) return 'no version'
  return null
}

/// The publisher's newest build that a copy can be checked against. Entries with no b3sum or a
/// malformed field are skipped (`skipped` lists those newer than the pick). The pick must be of the
/// tile schema the app draws and the size of a whole planet, or the run stops.
export function pickBuild(builds, { upstreamOrigin = BUILDS_ORIGIN, minBytes = MIN_BUILD_BYTES } = {}) {
  if (!Array.isArray(builds)) throw new Refusal("the publisher's build index is not a list")
  let newest = null
  const skipped = []
  for (const b of builds) {
    const why = unusable(b)
    if (why) skipped.push({ key: String(b?.key), why })
    else if (!newest || b.key > newest.key) newest = b
  }
  if (!newest) throw new Refusal(`the publisher's build index lists no usable build (${skipped.length} skipped)`)
  if (Number(newest.version.split('.')[0]) !== TILES_MAJOR) {
    throw new Refusal(
      `the newest build, ${newest.key}, is tiles ${newest.version}, and the app's street map draws tiles ` +
        `${TILES_MAJOR}.x: copying it would break every pack. Move the app's style first.`,
    )
  }
  if (newest.size < minBytes) {
    throw new Refusal(`the newest build, ${newest.key}, is ${newest.size} bytes: not a whole planet (at least ${minBytes})`)
  }
  const [, y, m, d] = newest.key.match(BUILD_KEY)
  const id = `${y}${m}${d}`
  return {
    build: {
      id,
      date: `${y}-${m}-${d}`,
      key: `planet-${id}.pmtiles`,
      upstream: `${upstreamOrigin}/${newest.key}`,
      bytes: newest.size,
      version: newest.version,
      b3sum: newest.b3sum,
    },
    skipped: skipped.filter((s) => s.key > newest.key),
  }
}

// ---- The index ---------------------------------------------------------------------------------

function onHost(url, origin) {
  if (!url.startsWith(`${origin}/`)) return false
  const path = url.slice(origin.length + 1)
  return path.length > 0 && /^[\x21-\x7e]+$/.test(path) && !path.includes('\\')
}

/// crates/street-map/src/manifest.rs `parse`, rule for rule: what Nexus accepts from the host.
/// The writer is held to it, so the job can never publish an index the app refuses.
export function parseIndex(text, origin = STREET_MAP_ORIGIN) {
  let m
  try {
    m = JSON.parse(text)
  } catch (e) {
    throw new Refusal(`streetmaps.json does not parse: ${e.message}`)
  }
  const obj = (v) => v !== null && typeof v === 'object' && !Array.isArray(v)
  if (!obj(m) || !obj(m.build) || !obj(m.assets)) throw new Refusal('streetmaps.json is not an index (schema, build, assets)')
  if (m.schema !== SCHEMA) throw new Refusal(`streetmaps.json schema ${m.schema} (Nexus reads schema ${SCHEMA})`)
  const { build: b, assets: a } = m
  for (const [v, what] of [[b.id, 'build.id'], [b.date, 'build.date'], [b.url, 'build.url'], [a.url, 'assets.url'], [a.sha256, 'assets.sha256']]) {
    if (typeof v !== 'string') throw new Refusal(`streetmaps.json ${what} is not a string`)
  }
  for (const [v, what] of [[b.upstream, 'build.upstream'], [b.version, 'build.version']]) {
    if (v != null && typeof v !== 'string') throw new Refusal(`streetmaps.json ${what} is not a string`)
  }
  for (const [v, what] of [[b.bytes, 'build.bytes'], [a.bytes, 'assets.bytes']]) {
    if (v != null && !(Number.isSafeInteger(v) && v >= 0)) throw new Refusal(`streetmaps.json ${what} is not a byte count`)
  }
  // The build id becomes part of every pack id, which is limited to [a-z0-9-].
  if (!(b.id.length > 0 && b.id.length <= 32 && !b.id.startsWith('-') && /^[a-z0-9-]+$/.test(b.id))) {
    throw new Refusal(`build id ${JSON.stringify(b.id)}`)
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(b.date)) throw new Refusal(`build date ${JSON.stringify(b.date)}`)
  for (const url of [b.url, a.url]) if (!onHost(url, origin)) throw new Refusal(`${url} is not on ${origin}`)
  if (!/^[0-9a-f]{64}$/.test(a.sha256)) throw new Refusal("the font and icon archive's SHA-256 is not 64 hex digits")
  return m
}

/// The index for a build and an archive. `published` is when the host starts serving it: the
/// clock the retirement of what it replaces runs from.
export function buildIndex({ build, assets, published, origin = STREET_MAP_ORIGIN }) {
  return {
    schema: SCHEMA,
    build: {
      id: build.id,
      date: build.date,
      url: `${origin}/${build.key}`,
      bytes: build.bytes,
      version: build.version,
      upstream: build.upstream,
      b3sum: build.b3sum,
    },
    assets: { url: `${origin}/${assets.key}`, sha256: assets.sha256, bytes: assets.bytes },
    attribution: ATTRIBUTION,
    license: LICENSE,
    published,
  }
}

/// The index as the host serves it, refused here if Nexus would refuse it.
export function indexText(index, origin = STREET_MAP_ORIGIN) {
  const text = JSON.stringify(index, null, 2) + '\n'
  parseIndex(text, origin)
  return text
}

/// Whether two indexes name the same things (`published` aside).
export const sameContent = (a, b) => JSON.stringify({ ...a, published: undefined }) === JSON.stringify({ ...b, published: undefined })

function keyOf(url, origin, family) {
  const key = url.slice(origin.length + 1)
  if (!family.test(key)) throw new Refusal(`the index names ${url}, which is not a key this job writes`)
  return key
}

// ---- What a run does ---------------------------------------------------------------------------

/// The operation a run performs: the dispatch input, or the one its schedule stands for.
export function operationFor({ event, operation, schedule }) {
  if (event === 'schedule') {
    if (schedule === COPY_CRON) return 'copy'
    if (schedule === RETIRE_CRON) return 'retire'
    throw new Refusal(`a schedule street-maps.mjs does not know: ${JSON.stringify(schedule)}`)
  }
  if (!OPERATIONS.includes(operation)) throw new Refusal(`operation ${JSON.stringify(operation)} is not one of ${OPERATIONS.join(', ')}`)
  return operation
}

/// What the bucket job will do: `none`, `copy` or `retire`, and why. `hosted` is the index the
/// host serves now (null when none), `next` the index a copy would publish.
export function decide({ operation, scheduled, hosted, next }) {
  if (operation === 'plan') return { action: 'none', why: 'operation=plan prints the plan and touches nothing' }
  if (operation === 'retire') {
    return hosted
      ? { action: 'retire', why: `delete what the index stopped naming at least ${RETAIN_DAYS} days ago` }
      : { action: 'none', why: 'nothing is hosted, so nothing can be retired' }
  }
  if (!hosted) {
    return scheduled
      ? { action: 'none', why: 'nothing is hosted yet. Hosting the first copy is a publish, made by hand (operation=copy), never by the schedule' }
      : { action: 'copy', why: 'the first copy, asked for by hand' }
  }
  const [h, n] = [hosted.build, next.build]
  if (h.id > n.id) return { action: 'none', why: `the host serves build ${h.id}, newer than the publisher's newest, ${n.id}` }
  if (h.id === n.id && (h.b3sum !== n.b3sum || h.bytes !== n.bytes)) {
    throw new Refusal(`the publisher's build ${n.id} is not the one the host serves under that name; a build is never replaced in place`)
  }
  if (sameContent(hosted, next)) return { action: 'none', why: `the host already serves build ${n.id} with these fonts and icons` }
  return { action: 'copy', why: h.id === n.id ? `new fonts and icons for build ${n.id}` : `build ${n.id} replaces ${h.id}` }
}

/// What a retire run deletes: every build and archive the index does not name, once the index has
/// named its own for RETAIN_DAYS. It deletes nothing when the index cannot be read, has no publish
/// time, or names an object the bucket does not hold; it never touches the index or a name this
/// job does not write (`other`).
export function retirement({ indexText: text, names, now, origin = STREET_MAP_ORIGIN }) {
  const index = parseIndex(text, origin)
  const published = typeof index.published === 'string' ? Date.parse(index.published) : NaN
  if (!Number.isFinite(published)) throw new Refusal('the hosted index has no publish time, so nothing is retired')
  const current = [keyOf(index.build.url, origin, PLANET_KEY), keyOf(index.assets.url, origin, ASSETS_KEY)]
  for (const key of current) {
    if (!names.includes(key)) throw new Refusal(`the hosted index names ${key}, which the bucket does not hold; nothing is retired`)
  }
  const due = published + RETAIN_DAYS * DAY_MS
  const ours = (n) => PLANET_KEY.test(n) || ASSETS_KEY.test(n)
  const old = names.filter((n) => ours(n) && !current.includes(n)).sort()
  const ripe = now >= due
  return {
    remove: ripe ? old : [],
    keep: ripe ? [] : old,
    due: iso(due),
    current,
    other: names.filter((n) => !ours(n) && n !== INDEX_NAME).sort(),
  }
}

// ---- Reading the host --------------------------------------------------------------------------

/// The index the host serves now, or null when there is none yet: a name with no DNS record
/// (before the custom domain exists) or a 404 (before the first copy). Any other failure stops the
/// run, so "could not tell" never reads as "nothing hosted".
export async function readHosted({ origin = STREET_MAP_ORIGIN, fetchImpl = fetch } = {}) {
  const url = `${origin}/${INDEX_NAME}`
  let r
  try {
    r = await fetchImpl(url, { headers: { 'user-agent': UA }, signal: AbortSignal.timeout(30_000) })
  } catch (e) {
    if (e?.cause?.code === 'ENOTFOUND') return { index: null, why: `${new URL(origin).host} does not resolve` }
    throw new Refusal(`could not read ${url}: ${e?.cause?.code ?? e.message}`)
  }
  if (r.status === 404) return { index: null, why: `${url} answers 404` }
  if (!r.ok) throw new Refusal(`${url}: HTTP ${r.status}`)
  return { index: parseIndex(await r.text(), origin), why: 'served' }
}

/// The build and the archive read back through the host, as an install reads them: the build's
/// first 127 bytes by a range read (a 206 whose Content-Range total is the build's size, starting
/// with the PMTiles v3 magic), and the archive whole (its SHA-256). A host that ignored the range
/// would answer 200 with 139 GB, so the body is only read after the status is right.
export async function checkServed(index, { fetchImpl = fetch } = {}) {
  const { build, assets } = index
  const r = await fetchImpl(build.url, { headers: { range: 'bytes=0-126', 'user-agent': UA }, signal: AbortSignal.timeout(60_000) })
  if (r.status !== 206) {
    await r.body?.cancel()
    throw new Refusal(`${build.url} answered a range read with HTTP ${r.status}, not 206; Nexus reads the build in ranges`)
  }
  const head = Buffer.from(await r.arrayBuffer())
  const total = Number(/\/(\d+)$/.exec(r.headers.get('content-range') ?? '')?.[1])
  if (total !== build.bytes) throw new Refusal(`${build.url} is ${total} bytes through the host, not ${build.bytes}`)
  if (head.length !== 127 || head.toString('latin1', 0, 7) !== 'PMTiles' || head[7] !== 3) {
    throw new Refusal(`${build.url} does not start with a PMTiles v3 header`)
  }
  const a = await fetchImpl(assets.url, { headers: { 'user-agent': UA }, signal: AbortSignal.timeout(120_000) })
  if (!a.ok) {
    await a.body?.cancel()
    throw new Refusal(`${assets.url}: HTTP ${a.status}`)
  }
  const got = sha256(Buffer.from(await a.arrayBuffer()))
  if (got !== assets.sha256) throw new Refusal(`${assets.url} serves SHA-256 ${got}, not ${assets.sha256}`)
}

/// The host serves exactly the index just written. A cached copy would keep installs on the old
/// build, so this waits briefly for it and then fails.
export async function checkServedIndex(text, { origin = STREET_MAP_ORIGIN, fetchImpl = fetch, tries = 6, wait = sleep } = {}) {
  const url = `${origin}/${INDEX_NAME}`
  for (let i = 1; ; i++) {
    const r = await fetchImpl(url, { headers: { 'user-agent': UA, 'cache-control': 'no-cache' }, signal: AbortSignal.timeout(30_000) })
    const body = r.ok ? await r.text() : (await r.body?.cancel(), null)
    if (body === text) return
    if (i >= tries) throw new Refusal(`${url} does not serve the index just written (HTTP ${r.status})`)
    await wait(10)
  }
}

// ---- The bucket (rclone) -----------------------------------------------------------------------

/// rclone with these arguments; its stdout, or a throw when it fails. Its log goes to stderr.
function rcloneCli(args) {
  return execFileSync('rclone', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], maxBuffer: 64 << 20 })
}

/// The bucket's objects at its top level, name → size.
function objectsIn(rclone, dest) {
  const list = JSON.parse(rclone(['lsjson', '--files-only', '--no-mimetype', '--no-modtime', dest]))
  return new Map(list.map((o) => [o.Path, o.Size]))
}

/// Check an object as stored, reading it whole. One that fails is deleted when the index has never
/// named it (no install can be reading it), and the run stops.
function checkStored({ rclone, dest, objects, key, bytes, hash, want, published, log }) {
  if (!objects.has(key)) throw new Refusal(`${key} is not in the bucket after its upload; nothing was published`)
  let bad = objects.get(key) === bytes ? null : `is ${objects.get(key)} bytes, not ${bytes}`
  if (!bad) {
    log(`checking the ${hash} of ${key} as stored (a full read) …`)
    const got = rclone(['hashsum', hash, '--download', `${dest}/${key}`]).trim().split(/\s+/)[0]
    if (got !== want) bad = `has ${hash} ${got}, not ${want}`
  }
  if (!bad) return
  if (!published) {
    log(`deleting ${key}: it fails its check and was never published`)
    rclone(['deletefile', `${dest}/${key}`])
  }
  throw new Refusal(`${key} ${bad}; nothing was published`)
}

/// The copy, from the plan job's files in `from`: the build to its immutable key, the archive,
/// both checked as stored and through the host, then the index. Everything acted on is re-checked
/// here, because the plan job compiled third-party code; the hosted index is read from the bucket.
export async function copyToBucket({
  from,
  dest,
  rclone = rcloneCli,
  fetchImpl = fetch,
  now = () => Date.now(),
  origin = STREET_MAP_ORIGIN,
  upstreamOrigin = BUILDS_ORIGIN,
  minBytes = MIN_BUILD_BYTES,
  wait = sleep,
  log = (s) => console.error(s),
}) {
  const next = parseIndex(readFileSync(join(from, INDEX_NAME), 'utf8'), origin)
  const archive = readFileSync(join(from, ASSETS_FILE))
  const build = { ...next.build, key: keyOf(next.build.url, origin, PLANET_KEY) }
  const assets = { ...next.assets, key: keyOf(next.assets.url, origin, ASSETS_KEY) }
  if (build.key !== `planet-${build.id}.pmtiles`) throw new Refusal(`${build.key} is not build ${build.id}'s key`)
  if (build.upstream !== `${upstreamOrigin}/${build.id}.pmtiles`) throw new Refusal(`${build.upstream} is not the publisher's build ${build.id}`)
  if (!/^[0-9a-f]{64}$/.test(build.b3sum ?? '')) throw new Refusal(`build ${build.id} has no b3sum`)
  if (!(build.bytes >= minBytes)) throw new Refusal(`build ${build.id} is ${build.bytes} bytes: not a whole planet`)
  if (assets.sha256 !== sha256(archive) || assets.bytes !== archive.length || assets.key !== assetsKey(assets.sha256)) {
    throw new Refusal(`${ASSETS_FILE} is not the archive the index names`)
  }

  let objects = objectsIn(rclone, dest)
  const hosted = objects.has(INDEX_NAME) ? parseIndex(rclone(['cat', `${dest}/${INDEX_NAME}`]), origin) : null
  const hostedKeys = hosted ? [keyOf(hosted.build.url, origin, PLANET_KEY), keyOf(hosted.assets.url, origin, ASSETS_KEY)] : []
  if (hosted) {
    // The bucket's own index decides, not the one the plan job read through the host.
    const d = decide({ operation: 'copy', scheduled: false, hosted, next })
    if (d.action === 'none') {
      log(`nothing to copy: ${d.why}`)
      return null
    }
  }

  if (hostedKeys.includes(build.key)) {
    log(`${build.key} is the build the host serves; it is not copied or read again`)
  } else {
    if (objects.has(build.key)) {
      log(`${build.key} is already in the bucket (a run that stopped before publishing); checking it`)
    } else {
      log(`copying ${build.upstream} to ${build.key} (${build.bytes} bytes) …`)
      rclone(['copyurl', '--no-clobber', build.upstream, `${dest}/${build.key}`])
      objects = objectsIn(rclone, dest)
    }
    checkStored({ rclone, dest, objects, key: build.key, bytes: build.bytes, hash: 'blake3', want: build.b3sum, published: false, log })
  }
  if (!objects.has(assets.key)) {
    rclone(['copyto', join(from, ASSETS_FILE), `${dest}/${assets.key}`])
    objects = objectsIn(rclone, dest)
  }
  checkStored({ rclone, dest, objects, key: assets.key, bytes: assets.bytes, hash: 'sha256', want: assets.sha256, published: hostedKeys.includes(assets.key), log })

  await checkServed(next, { fetchImpl })
  const text = indexText({ ...next, published: iso(now()) }, origin)
  const file = join(from, `published-${INDEX_NAME}`)
  writeFileSync(file, text)
  log(`publishing ${INDEX_NAME}: build ${build.id}, ${assets.key}`)
  rclone(['copyto', '--header-upload', 'Cache-Control: no-cache', file, `${dest}/${INDEX_NAME}`])
  if (rclone(['cat', `${dest}/${INDEX_NAME}`]) !== text) throw new Refusal(`the stored ${INDEX_NAME} is not the one written`)
  await checkServedIndex(text, { origin, fetchImpl, wait })
  return text
}

/// A retire run: delete what `retirement` names, from the bucket's own index and listing.
export function retireFromBucket({ dest, rclone = rcloneCli, now = Date.now(), origin = STREET_MAP_ORIGIN, log = (s) => console.error(s) }) {
  const objects = objectsIn(rclone, dest)
  if (!objects.has(INDEX_NAME)) throw new Refusal(`the bucket holds no ${INDEX_NAME}; nothing is retired`)
  const r = retirement({ indexText: rclone(['cat', `${dest}/${INDEX_NAME}`]), names: [...objects.keys()], now, origin })
  for (const key of r.remove) {
    log(`deleting ${key}`)
    rclone(['deletefile', `${dest}/${key}`])
  }
  if (r.keep.length) log(`kept until ${r.due}: ${r.keep.join(', ')}`)
  if (!r.remove.length && !r.keep.length) log(`nothing to retire: the bucket holds only ${r.current.join(' and ')}`)
  if (r.other.length) log(`left alone, not written by this job: ${r.other.join(', ')}`)
  return r
}

// ---- The fonts and icons -----------------------------------------------------------------------

/// A sprite generator flavor (protomaps/basemaps sprites/flavors/*.json: colour groups, and the
/// icon → group list it draws) with SPRITE_ADDITIONS added.
export function patchFlavor(text) {
  const flavor = JSON.parse(text)
  for (const [icon, group] of Object.entries(SPRITE_ADDITIONS)) {
    if (!flavor.flavors?.[group]) throw new Refusal(`the sprite flavor has no ${group} colours to draw ${icon} in`)
    flavor.icons[icon] ??= group
  }
  return JSON.stringify(flavor, null, 2) + '\n'
}

const DEVICE = /^(?:CON|PRN|AUX|NUL|COM\d|LPT\d)$/i

/// crates/street-map/src/store.rs `validate_asset_path`: a name the archive may hold.
export function assetPathOk(p) {
  if (p.length === 0 || p.length > 200 || p.includes('\0') || p.includes('\\') || p.startsWith('/')) return false
  const segments = p.split('/')
  return (
    segments.length <= 4 &&
    segments.every(
      (s) => s !== '.' && s !== '..' && /^[A-Za-z0-9 ,._@-]+$/.test(s) && !/^[. ]|[. ]$/.test(s) && !DEVICE.test(s.split('.')[0].trimEnd()),
    )
  )
}

/// The staged archive tree, before it is packed: exactly the files Nexus asks for (both licences,
/// all 256 glyph ranges of each font stack, the light and dark sheets at 1x and 2x), every name
/// one Nexus accepts, within its limits, and every icon the style names in every sheet.
export function checkAssetsTree(dir) {
  const files = new Map()
  const walk = (rel) => {
    for (const e of readdirSync(join(dir, rel), { withFileTypes: true })) {
      const p = rel ? `${rel}/${e.name}` : e.name
      if (!assetPathOk(p)) throw new Refusal(`${p}: a name Nexus refuses`)
      if (e.isDirectory()) walk(p)
      else if (e.isFile()) files.set(p, lstatSync(join(dir, p)).size)
      else throw new Refusal(`${p} is neither a file nor a folder`)
    }
  }
  walk('')
  const ranges = Array.from({ length: 256 }, (_, i) => `${i * 256}-${i * 256 + 255}.pbf`)
  const sheets = SPRITE_LOOKS.flatMap((look) => [look, `${look}@2x`])
  const expected = [
    ...REQUIRED,
    ...FONT_STACKS.flatMap((stack) => ranges.map((r) => `fonts/${stack}/${r}`)),
    ...sheets.flatMap((s) => [`sprites/${s}.json`, `sprites/${s}.png`]),
  ]
  const missing = expected.filter((p) => !files.has(p))
  const extra = [...files.keys()].filter((p) => !expected.includes(p))
  if (missing.length) throw new Refusal(`the archive lacks ${missing.length} file(s): ${missing.slice(0, 5).join(', ')}`)
  if (extra.length) throw new Refusal(`the archive would hold ${extra.length} file(s) Nexus never asks for: ${extra.slice(0, 5).join(', ')}`)
  const bytes = [...files.values()].reduce((a, b) => a + b, 0)
  if (files.size > MAX_FILES || bytes > MAX_UNPACKED) throw new Refusal(`the archive unpacks to ${files.size} files, ${bytes} bytes: over Nexus's limit`)
  for (const s of sheets) {
    const icons = JSON.parse(readFileSync(join(dir, 'sprites', `${s}.json`), 'utf8'))
    const lacking = STYLE_ICONS.filter((icon) => !Object.hasOwn(icons, icon))
    if (lacking.length) throw new Refusal(`sprites/${s}.json lacks icons the style ${STYLE_VERSION} names: ${lacking.join(', ')}`)
  }
  return { files: files.size, bytes }
}

// ---- The plan ----------------------------------------------------------------------------------

function describe({ operation, event, build, skipped, hosted, hostedWhy, assets, decision, text }) {
  const n = (x) => x.toLocaleString('en-US')
  const lines = [
    `Street maps: operation ${operation} (${event})`,
    `The publisher's newest build: ${build.id}, tiles ${build.version}, ${n(build.bytes)} bytes`,
    `  b3sum ${build.b3sum}`,
    `  from ${build.upstream}`,
    ...skipped.map((s) => `  (newer, skipped: ${s.key}, ${s.why})`),
    hosted
      ? `The host serves: build ${hosted.build.id} and ${hosted.assets.url}, published ${hosted.published ?? '(no time)'}`
      : `The host serves: nothing (${hostedWhy})`,
    `The fonts and icons: ${assets.key}, ${n(assets.bytes)} bytes, sha256 ${assets.sha256}`,
    'A copy:',
    `  1. copies the build to ${build.key} unless the bucket has it (a key is never overwritten)`,
    '  2. checks its size and BLAKE3 as stored, and deletes it unpublished if either is wrong',
    `  3. adds ${assets.key} unless the bucket has it, and checks its SHA-256 as stored`,
    `  4. reads both back through ${STREET_MAP_ORIGIN}, the build by a range read`,
    `  5. publishes ${INDEX_NAME} (below, with the time it goes live), then reads it back through the host`,
    `  6. leaves what it replaced for the monthly retire run at least ${RETAIN_DAYS} days on`,
    `Decision: ${decision.action}`,
    `  why: ${decision.why}`,
    `--- ${INDEX_NAME} a copy would publish ---`,
    text.trimEnd(),
  ]
  return lines.join('\n') + '\n'
}

async function plan({ out, event, operation, schedule }) {
  const op = operationFor({ event, operation, schedule })
  const r = await fetch(BUILDS_INDEX, { headers: { 'user-agent': UA }, signal: AbortSignal.timeout(60_000) })
  if (!r.ok) throw new Refusal(`${BUILDS_INDEX}: HTTP ${r.status}`)
  const { build, skipped } = pickBuild(await r.json())
  const { index: hosted, why: hostedWhy } = await readHosted()
  const archive = readFileSync(join(out, ASSETS_FILE))
  if (archive.length > MAX_ARCHIVE) throw new Refusal(`${ASSETS_FILE} is ${archive.length} bytes, over Nexus's ${MAX_ARCHIVE}`)
  const sha = sha256(archive)
  const assets = { key: assetsKey(sha), sha256: sha, bytes: archive.length }
  const next = buildIndex({ build, assets, published: iso(Date.now()) })
  const text = indexText(next)
  const decision = decide({ operation: op, scheduled: event === 'schedule', hosted, next })
  writeFileSync(join(out, INDEX_NAME), text)
  const record = { operation: op, event, ...decision, build, assets, hosted: hosted ?? null, hostedWhy }
  writeFileSync(join(out, PLAN_FILE), JSON.stringify(record, null, 2) + '\n')
  const summary = describe({ operation: op, event, build, skipped, hosted, hostedWhy, assets, decision, text })
  process.stdout.write(summary)
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, '```text\n' + summary + '```\n')
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `action=${decision.action}\n`)
}

async function main() {
  const [cmd, ...args] = process.argv.slice(2)
  const flags = (options) => parseArgs({ args, options, strict: true }).values
  if (cmd === 'plan') {
    const v = flags({ out: { type: 'string' }, event: { type: 'string' }, operation: { type: 'string', default: '' }, schedule: { type: 'string', default: '' } })
    if (!v.out || !v.event) throw new Refusal('plan needs --out and --event')
    await plan(v)
  } else if (cmd === 'copy') {
    const v = flags({ from: { type: 'string' }, dest: { type: 'string' } })
    if (!v.from || !v.dest) throw new Refusal('copy needs --from and --dest')
    await copyToBucket(v)
  } else if (cmd === 'retire') {
    const v = flags({ dest: { type: 'string' } })
    if (!v.dest) throw new Refusal('retire needs --dest')
    retireFromBucket(v)
  } else if (cmd === 'flavor' && args.length === 1) {
    process.stdout.write(patchFlavor(readFileSync(args[0], 'utf8')))
  } else if (cmd === 'check-assets' && args.length === 1) {
    const { files, bytes } = checkAssetsTree(args[0])
    console.error(`the fonts and icons: ${files} files, ${bytes} bytes, every icon the style ${STYLE_VERSION} names`)
  } else {
    throw new Refusal('usage: street-maps.mjs plan|copy|retire|flavor <file>|check-assets <dir>')
  }
}

const entry = process.argv[1] && realpathSync(process.argv[1])
if (entry === realpathSync(fileURLToPath(import.meta.url))) {
  main().catch((e) => {
    console.error(e instanceof Refusal ? `REFUSED: ${e.message}` : e)
    process.exit(1)
  })
}
