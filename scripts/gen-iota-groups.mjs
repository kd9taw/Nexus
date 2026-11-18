#!/usr/bin/env node
// Fetch + check IOTA Ltd's island-group list (groups.json) and its islands list (islands.json), and
// write the copies that ship with Nexus: src-tauri/resources/iota/iota_groups.json and
// iota_islands.json, bundled into every installer as app resources (see NOTICE: included with the
// written permission of IOTA Ltd and the RSGB).
//
// Source — the address IOTA Ltd publishes for software on its Developers page
// (https://www.iota-world.org/iota-directory/developers.html):
//   https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=groups.json
//   https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=islands.json
// The same address, client rules and limits as the app's own monthly check
// (crates/propagation/src/live/iota.rs): HTTPS, the app's User-Agent, no redirect followed,
// nothing over 2 MB read.
//
// Retry: iota-world.org answers HTTP 202 with no file while it is rebuilding groups.json (a
// release once built in that window and shipped without the list). A 202 or a 5xx is asked again
// after 15 s, 30 s and 60 s, and so is a network failure with no answer at all (a timeout, DNS, a
// reset); after the fourth attempt the run fails, and release.yml makes that failure visible on
// the run's summary. Any other status, and a file refused for its size, fails at once.
//
// Fallback (--fallback <dir>): a list whose download (or file) fails, or fails the checks below,
// is taken from a copy an earlier run left in <dir>, and a line on stderr says so ("using the
// cached copy"). release.yml's iota-lists job restores the last good lists into <dir> from the
// GitHub Actions cache (the lists are never committed), so <dir> is untrusted input, read once and
// checked as a whole before any of it is used (`checkCacheDir`): exactly the three files this script writes
// (iota_groups.json, iota_islands.json, iota_groups.manifest.json) and nothing else, each a
// regular file (not a link) within its size limit; both lists pass the checks below and the
// manifest is a JSON object with a whole-number `fetchedAt`. Anything else is a cache miss, and
// the reason is logged. Only the checked bytes are written to outDir; <dir> itself is never
// copied. The manifest then carries the cached copy's `fetchedAt`, not now, so the app knows how
// old the bundled lists are. No usable cached copy: the run fails as it would without --fallback.
//
// Handed over (--checked <dir>): no download at all. <dir> holds the three files another job
// staged (release.yml's installer jobs download them as a workflow artifact from iota-lists); it
// is checked exactly as a cache directory is, and the run fails when it does not pass. The
// manifest keeps <dir>'s `fetchedAt`.
//
// The checks — any miss exits 1 and writes nothing, so a short or changed file never replaces
// a copy already staged:
//   - the body is a JSON array of objects carrying `refno` and `name`;
//   - at least 900 entries have a valid reference (XX-NNN, one of the seven continent codes)
//     and a name — the app's own floor (propagation::iota::MIN_PLAUSIBLE_GROUPS);
//   - no name is longer than 200 characters (propagation::iota::NAME_MAX_CHARS; the app would
//     cut it, but a real name is a few words, so a longer one means the shape changed);
//   - two groups that will never change: AF-001 and EU-005.
// And for islands.json (the islands a log form offers to pick from for a group, IOTA_ISLAND_ID):
//   - a JSON array of objects carrying `id` (a whole number, 1 to 99999999: ADIF's IOTA_ISLAND_ID),
//     `refno` and `name`;
//   - at least 10,000 islands with a valid id, reference and name (13,020 in November 2026; the
//     app's floor, propagation::iota::MIN_PLAUSIBLE_ISLANDS), no two with one id;
//   - no name longer than 200 characters, and islands in AF-001 and EU-005;
//   - no larger than 6 MB (1.28 MB in November 2026).
// The groups are written first: an islands download that fails leaves them staged, and the run
// still exits 1, so the caller's warning shows.
//
// Output (all in outDir):
//   iota_groups.json          — the downloaded BYTES verbatim, the format the app's own download
//                               stores, so one parser reads both.
//   iota_islands.json         — likewise, for islands.json.
//   iota_groups.manifest.json — {source, fetched, fetchedAt, groups, bytes, sha256}, and with the
//                               islands staged {islandsSource, islands, islandsBytes, islandsSha256}. The app reads
//                               `fetchedAt` (Unix seconds) as the bundled copy's date: a copy it
//                               downloads itself replaces the bundled one only when newer, and the
//                               monthly check counts from the newer of the two.
//
// Run:  node scripts/gen-iota-groups.mjs [--download | <groups.json>] [outDir] [--islands <islands.json>] [--fallback <dir>] [--dry-run]
//       node scripts/gen-iota-groups.mjs --checked <dir> [outDir] [--dry-run]
//   Default: --download into src-tauri/resources/iota (both lists). A file argument re-checks a
//   groups copy already on disk (no network) and dates it now, with `--islands` an islands copy too.
//   --dry-run checks and prints the manifest, writes nothing.

import { readFileSync, writeFileSync, mkdirSync, lstatSync, readdirSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const GROUPS_URL = 'https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=groups.json'
const ISLANDS_URL = 'https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=islands.json'
const UA = 'Nexus (amateur radio logbook; https://hamradiotools.io; kd9taw@protonmail.com)'
const MAX_BYTES = 2 * 1024 * 1024
const MIN_GROUPS = 900
const NAME_MAX_CHARS = 200
const CANARIES = ['AF-001', 'EU-005']
const REF = /^(AF|AN|AS|EU|NA|OC|SA)-\d{3}$/
const ISLANDS_MAX_BYTES = 6 * 1024 * 1024
const MIN_ISLANDS = 10_000
const ISLAND_ID_MAX = 99_999_999

/** The waits before the second, third and fourth attempt. */
export const RETRY_DELAYS_MS = [15_000, 30_000, 60_000]

/** A failure that passes on its own: a 202 or 5xx status, or no answer at all. */
const passing = (e) => e.network === true || e.status === 202 || (e.status >= 500 && e.status <= 599)

/**
 * `fetchOnce()` until it returns the file, asking again after each of `delays` when it fails in a
 * passing way: a thrown error's `status` 202 or 5xx, or `network` (no HTTP answer). Anything else
 * fails at once.
 */
export async function downloadWithRetry({
  fetchOnce = downloadOnce,
  sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
  delays = RETRY_DELAYS_MS,
  log = (line) => console.error(line),
} = {}) {
  for (let attempt = 1; ; attempt++) {
    try {
      return await fetchOnce()
    } catch (e) {
      if (!passing(e)) throw e
      const wait = delays[attempt - 1]
      if (wait === undefined) throw new Error(`${e.message}, after ${attempt} attempts`)
      log(`gen-iota-groups: attempt ${attempt}: ${e.message}; asking again in ${wait / 1000} s`)
      await sleep(wait)
    }
  }
}

/** `e` (a fetch or body-stream failure: timeout, DNS, reset) as a network failure. */
const unreachable = (e) =>
  Object.assign(new Error(`the IOTA list server could not be reached (${e.cause?.code ?? e.name}: ${e.message})`), {
    network: true,
  })

async function downloadOnce(url = GROUPS_URL, maxBytes = MAX_BYTES) {
  const resp = await fetch(url, {
    headers: { 'User-Agent': UA },
    redirect: 'manual',
    signal: AbortSignal.timeout(60_000),
  }).catch((e) => {
    throw unreachable(e)
  })
  if (resp.status !== 200) {
    throw Object.assign(
      new Error(`the IOTA list server answered HTTP ${resp.status} instead of the file`),
      { status: resp.status },
    )
  }
  const len = Number(resp.headers.get('content-length') ?? 0)
  if (len > maxBytes) throw new Error(`the IOTA list is ${len} bytes, larger than ${maxBytes}`)
  const chunks = []
  let total = 0
  const reader = resp.body[Symbol.asyncIterator]()
  for (;;) {
    // A reset or timeout part-way through the file is a network failure too; the size refusal
    // below is not.
    const { value: chunk, done } = await reader.next().catch((e) => {
      throw unreachable(e)
    })
    if (done) break
    total += chunk.length
    if (total > maxBytes) throw new Error(`the IOTA list is larger than ${maxBytes} bytes`)
    chunks.push(chunk)
  }
  return Buffer.concat(chunks)
}

/** The checks in the header; returns the number of valid groups or throws. */
export function check(bytes) {
  if (bytes.length > MAX_BYTES) throw new Error(`the IOTA list is larger than ${MAX_BYTES} bytes`)
  const list = JSON.parse(bytes.toString('utf8'))
  if (!Array.isArray(list)) throw new Error('the IOTA list is not a JSON array')
  const refs = new Set()
  for (const g of list) {
    const ref = typeof g?.refno === 'string' ? g.refno.trim().toUpperCase() : ''
    const name = typeof g?.name === 'string' ? g.name.trim() : ''
    if (!REF.test(ref) || !name) continue
    if ([...name].length > NAME_MAX_CHARS) throw new Error(`${ref}'s name is longer than ${NAME_MAX_CHARS} characters`)
    refs.add(ref)
  }
  if (refs.size < MIN_GROUPS) throw new Error(`only ${refs.size} island groups, fewer than ${MIN_GROUPS}`)
  for (const c of CANARIES) if (!refs.has(c)) throw new Error(`${c} is missing`)
  return refs.size
}

/** The islands checks in the header; returns the number of valid islands or throws. */
export function checkIslands(bytes) {
  if (bytes.length > ISLANDS_MAX_BYTES) throw new Error(`the IOTA islands list is larger than ${ISLANDS_MAX_BYTES} bytes`)
  const list = JSON.parse(bytes.toString('utf8'))
  if (!Array.isArray(list)) throw new Error('the IOTA islands list is not a JSON array')
  const ids = new Set()
  const refs = new Set()
  for (const i of list) {
    const id = Number(typeof i?.id === 'string' ? i.id.trim() : i?.id)
    const ref = typeof i?.refno === 'string' ? i.refno.trim().toUpperCase() : ''
    const name = typeof i?.name === 'string' ? i.name.trim() : ''
    if (!Number.isInteger(id) || id < 1 || id > ISLAND_ID_MAX || !REF.test(ref) || !name) continue
    if ([...name].length > NAME_MAX_CHARS) throw new Error(`island ${id}'s name is longer than ${NAME_MAX_CHARS} characters`)
    if (ids.has(id)) throw new Error(`island id ${id} is listed twice`)
    ids.add(id)
    refs.add(ref)
  }
  if (ids.size < MIN_ISLANDS) throw new Error(`only ${ids.size} islands, fewer than ${MIN_ISLANDS}`)
  for (const c of CANARIES) if (!refs.has(c)) throw new Error(`no island in ${c}`)
  return ids.size
}

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex')

/** The files a cache directory may hold, each with its size limit: what this script writes. */
export const CACHE_FILES = {
  'iota_groups.json': MAX_BYTES,
  'iota_islands.json': ISLANDS_MAX_BYTES,
  'iota_groups.manifest.json': 64 * 1024,
}

/**
 * The cached copy in `dir`, checked as a whole: `{groups, islands, fetchedAt}` (the bytes, read
 * once), or `{miss}` naming why it cannot be used. See "Fallback" in the header.
 */
export function checkCacheDir(dir) {
  try {
    let st
    try {
      st = lstatSync(dir)
    } catch (e) {
      if (e.code === 'ENOENT') return { miss: `no cached copy in ${dir}` }
      throw e
    }
    if (!st.isDirectory()) return { miss: `${dir} is not a directory` }
    const names = readdirSync(dir)
    const extra = names.filter((n) => !Object.hasOwn(CACHE_FILES, n))
    if (extra.length) return { miss: `unexpected entries in ${dir}: ${extra.join(', ')}` }
    if (names.length === 0) return { miss: `no cached copy in ${dir}` }
    const bytes = {}
    for (const [name, max] of Object.entries(CACHE_FILES)) {
      const path = join(dir, name)
      let f
      try {
        f = lstatSync(path)
      } catch (e) {
        if (e.code === 'ENOENT') return { miss: `${name} is missing from ${dir}` }
        throw e
      }
      if (!f.isFile()) return { miss: `${name} in ${dir} is not a regular file` }
      if (f.size > max) return { miss: `${name} in ${dir} is ${f.size} bytes, over ${max}` }
      bytes[name] = readFileSync(path)
      if (bytes[name].length > max) return { miss: `${name} in ${dir} is over ${max} bytes` }
    }
    check(bytes['iota_groups.json'])
    checkIslands(bytes['iota_islands.json'])
    const manifest = JSON.parse(bytes['iota_groups.manifest.json'].toString('utf8'))
    if (manifest === null || typeof manifest !== 'object' || Array.isArray(manifest)) {
      return { miss: `the cached manifest is not a JSON object` }
    }
    if (!Number.isInteger(manifest.fetchedAt) || manifest.fetchedAt <= 0) {
      return { miss: `the cached manifest has no valid fetchedAt` }
    }
    return { groups: bytes['iota_groups.json'], islands: bytes['iota_islands.json'], fetchedAt: manifest.fetchedAt }
  } catch (e) {
    return { miss: `the cached copy in ${dir} failed its checks: ${e.message}` }
  }
}

/**
 * `get()`'s bytes once `checkFn` passes them; when either throws, `cached` (bytes already
 * checked by `checkCacheDir`) instead, if there are any. Returns `{bytes, cached}`; with no
 * cached bytes, throws the failure with `missWhy` (why there is no usable copy) added.
 */
export async function orCached(get, { file, checkFn, cached = null, missWhy = null, log = (line) => console.error(line) }) {
  try {
    const bytes = await get()
    checkFn(bytes)
    return { bytes, cached: false }
  } catch (e) {
    if (!cached) throw missWhy ? new Error(`${e.message}; no usable cached copy (${missWhy})`) : e
    log(`gen-iota-groups: ${e.message}; using the cached copy of ${file} instead`)
    return { bytes: cached, cached: true }
  }
}

async function main() {
  const argv = process.argv.slice(2)
  const dryRun = argv.includes('--dry-run')
  const valued = new Set()
  const option = (flag) => {
    const at = argv.indexOf(flag)
    if (at < 0) return null
    valued.add(at).add(at + 1)
    return argv[at + 1]
  }
  const islandsFile = option('--islands')
  const fallbackDir = option('--fallback')
  const checkedDir = option('--checked')
  const args = argv.filter((a, i) => a !== '--dry-run' && !valued.has(i))
  if (checkedDir) args.unshift('--checked')
  const [src = '--download', outDir = 'src-tauri/resources/iota'] = args
  const download = src === '--download'
  if (checkedDir && (fallbackDir || islandsFile)) throw new Error('--checked takes no --fallback or --islands')
  // The cache is read and checked once, up front; a miss is logged and the downloads decide.
  let cache = null
  let missWhy = null
  if (fallbackDir) {
    cache = checkCacheDir(fallbackDir)
    if (cache.miss) {
      missWhy = cache.miss
      console.error(`gen-iota-groups: cache miss: ${missWhy}`)
      cache = null
    } else {
      console.error(`gen-iota-groups: cached copy in ${fallbackDir} checked (fetched ${new Date(cache.fetchedAt * 1000).toISOString()})`)
    }
  }
  if (checkedDir) {
    cache = checkCacheDir(checkedDir)
    if (cache.miss) throw new Error(`the handed-over IOTA lists cannot be used: ${cache.miss}`)
    console.error(`gen-iota-groups: handed-over lists in ${checkedDir} checked (fetched ${new Date(cache.fetchedAt * 1000).toISOString()})`)
  }
  const fromCache = []
  const take = async (file, get, checkFn, cached) => {
    if (checkedDir) {
      fromCache.push(file)
      return cached
    }
    const got = await orCached(get, { file, checkFn, cached, missWhy })
    if (got.cached) fromCache.push(file)
    return got.bytes
  }
  const bytes = await take('iota_groups.json', () => (download ? downloadWithRetry() : readFileSync(src)), check, cache?.groups)
  const groups = check(bytes)
  // The islands, checked before anything is written; a failure is kept for after the groups are.
  let islandBytes = null
  let islandsFailed = null
  if (download || islandsFile || checkedDir) {
    try {
      islandBytes = await take(
        'iota_islands.json',
        () =>
          download
            ? downloadWithRetry({ fetchOnce: () => downloadOnce(ISLANDS_URL, ISLANDS_MAX_BYTES) })
            : readFileSync(islandsFile),
        checkIslands,
        cache?.islands,
      )
    } catch (e) {
      islandBytes = null
      islandsFailed = e
    }
  }
  // A list from the cache dates the whole manifest as that copy, so the app never takes it as new.
  const now = fromCache.length ? new Date(cache.fetchedAt * 1000) : new Date()
  const manifest = {
    source: GROUPS_URL,
    fetched: now.toISOString(),
    fetchedAt: Math.floor(now.getTime() / 1000),
    groups,
    bytes: bytes.length,
    sha256: sha256(bytes),
    ...(islandBytes && {
      islandsSource: ISLANDS_URL,
      islands: checkIslands(islandBytes),
      islandsBytes: islandBytes.length,
      islandsSha256: sha256(islandBytes),
    }),
  }
  if (dryRun) {
    console.log(JSON.stringify(manifest, null, 2))
  } else {
    mkdirSync(outDir, { recursive: true })
    writeFileSync(join(outDir, 'iota_groups.json'), bytes)
    if (islandBytes) writeFileSync(join(outDir, 'iota_islands.json'), islandBytes)
    writeFileSync(join(outDir, 'iota_groups.manifest.json'), JSON.stringify(manifest, null, 2) + '\n')
    console.log(`IOTA list: ${groups} island groups, ${bytes.length} bytes -> ${outDir}`)
    if (islandBytes) console.log(`IOTA islands: ${manifest.islands} islands, ${islandBytes.length} bytes -> ${outDir}`)
    if (fromCache.length && !checkedDir) console.log(`IOTA lists from the cached copy (fetched ${manifest.fetched}): ${fromCache.join(', ')}`)
  }
  if (islandsFailed) throw new Error(`the island groups were staged, the islands list was not: ${islandsFailed.message}`)
}

if (import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((e) => {
    console.error(`gen-iota-groups: ${e.message}`)
    process.exit(1)
  })
}
