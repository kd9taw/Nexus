#!/usr/bin/env node
// Fetch + check IOTA Ltd's island-group list (groups.json) and write the copy that ships with
// Nexus: src-tauri/resources/iota/iota_groups.json, bundled into every installer as an app
// resource (see NOTICE: included with IOTA Ltd's permission).
//
// Source — the address IOTA Ltd publishes for software on its Developers page
// (https://www.iota-world.org/iota-directory/developers.html):
//   https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=groups.json
// The same address, client rules and limits as the app's own monthly check
// (crates/propagation/src/live/iota.rs): HTTPS, the app's User-Agent, no redirect followed,
// nothing over 2 MB read.
//
// The checks — any miss exits 1 and writes nothing, so a short or changed file never replaces
// a copy already staged:
//   - the body is a JSON array of objects carrying `refno` and `name`;
//   - at least 900 entries have a valid reference (XX-NNN, one of the seven continent codes)
//     and a name — the app's own floor (propagation::iota::MIN_PLAUSIBLE_GROUPS);
//   - no name is longer than 200 characters (propagation::iota::NAME_MAX_CHARS; the app would
//     cut it, but a real name is a few words, so a longer one means the shape changed);
//   - two groups that will never change: AF-001 and EU-005.
//
// Output (both in outDir):
//   iota_groups.json          — the downloaded BYTES verbatim, the format the app's own download
//                               stores, so one parser reads both.
//   iota_groups.manifest.json — {source, fetched, fetchedAt, groups, bytes, sha256}. The app reads
//                               `fetchedAt` (Unix seconds) as the bundled copy's date: a copy it
//                               downloads itself replaces the bundled one only when newer, and the
//                               monthly check counts from the newer of the two.
//
// Run:  node scripts/gen-iota-groups.mjs [--download | <groups.json>] [outDir] [--dry-run]
//   Default: --download into src-tauri/resources/iota. A file argument re-checks a copy already
//   on disk (no network) and dates it now. --dry-run checks and prints the manifest, writes nothing.

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const GROUPS_URL = 'https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=groups.json'
const UA = 'Nexus (amateur radio logbook; https://hamradiotools.io; kd9taw@protonmail.com)'
const MAX_BYTES = 2 * 1024 * 1024
const MIN_GROUPS = 900
const NAME_MAX_CHARS = 200
const CANARIES = ['AF-001', 'EU-005']
const REF = /^(AF|AN|AS|EU|NA|OC|SA)-\d{3}$/

async function download() {
  const resp = await fetch(GROUPS_URL, {
    headers: { 'User-Agent': UA },
    redirect: 'manual',
    signal: AbortSignal.timeout(60_000),
  })
  if (resp.status !== 200) throw new Error(`the IOTA list server answered HTTP ${resp.status} instead of the file`)
  const len = Number(resp.headers.get('content-length') ?? 0)
  if (len > MAX_BYTES) throw new Error(`the IOTA list is ${len} bytes, larger than ${MAX_BYTES}`)
  const chunks = []
  let total = 0
  for await (const chunk of resp.body) {
    total += chunk.length
    if (total > MAX_BYTES) throw new Error(`the IOTA list is larger than ${MAX_BYTES} bytes`)
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

async function main() {
  const args = process.argv.slice(2).filter((a) => a !== '--dry-run')
  const dryRun = process.argv.includes('--dry-run')
  const [src = '--download', outDir = 'src-tauri/resources/iota'] = args
  const bytes = src === '--download' ? await download() : readFileSync(src)
  const groups = check(bytes)
  const now = new Date()
  const manifest = {
    source: GROUPS_URL,
    fetched: now.toISOString(),
    fetchedAt: Math.floor(now.getTime() / 1000),
    groups,
    bytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  }
  if (dryRun) {
    console.log(JSON.stringify(manifest, null, 2))
    return
  }
  mkdirSync(outDir, { recursive: true })
  writeFileSync(join(outDir, 'iota_groups.json'), bytes)
  writeFileSync(join(outDir, 'iota_groups.manifest.json'), JSON.stringify(manifest, null, 2) + '\n')
  console.log(`IOTA list: ${groups} island groups, ${bytes.length} bytes -> ${outDir}`)
}

if (import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((e) => {
    console.error(`gen-iota-groups: ${e.message}`)
    process.exit(1)
  })
}
