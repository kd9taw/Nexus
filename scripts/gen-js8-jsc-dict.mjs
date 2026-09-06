#!/usr/bin/env node
// scripts/gen-js8-jsc-dict.mjs — build the JSC dictionary blob crates/js8 ships.
//
// WHY A GENERATOR. JS8Call's dense text coder (jsc.cpp, GPLv3) indexes a 262,144-word table:
// index i on the air MEANS word i, so a single transposition garbles every dense data frame
// both ways while every unit test still passes. The table is 6.3 MB of C++ source twice over
// (jsc_list.cpp / jsc_map.cpp). This script fetches both at the pinned commit, verifies their
// sha256, PROVES the structural properties the Rust lookup relies on, and writes a compact
// canonical blob plus its sha256 pin. Nexus commits the blob and the pin, never the source.
// The Rust test `proto::jsc::tests::blob_matches_its_pin` re-hashes the inflated blob against
// dict.sha256 every run, so a swapped blob is a red test. Same shape as
// scripts/gen-wsjtx-callsign-oracle.mjs: fetch here, by hand; the test job makes no network
// call and must not start.
//
// WHAT THE RUST SIDE ASSUMES (each proven below, each a loud error if upstream ever changes):
//   P1  map[i].index == i; map[i].size is the CONSUME count (differs from strlen for exactly
//       {81:"@ALLCALL":7, 262143:"ROSIDS":1} — wire behaviour, so the blob carries it).
//   P2  list is a permutation of map (same (str,index) pairs), words unique.
//   P3  JSC::lookup(b) takes the FIRST prefix entry whose byte == b[0]; a single-entry group
//       returns its entry without comparing; otherwise it scans the group's list range for
//       the first entry with strncmp(b, str, size) == 0. Group ranges overlap, so an entry is
//       REACHABLE iff some group whose byte equals the entry's first byte covers it; the only
//       unreachable one upstream is "@ALLCALL" at list position 256644.
//   P4  within a group, every reachable proper prefix of a reachable word sits AFTER it, so
//       first-match == longest-match, and a longest-prefix lookup over reachable words is
//       wire-identical to the scan.
//
// Blob format (canonical, then raw-deflate level 9): for i in 0..262144:
//   u8 strlen · strlen bytes (Latin-1) · u8 consume · u8 reachable(0|1)
// Pin: sha256 of the CANONICAL (inflated) bytes, hex, one line, in dict.sha256.
//
// Run:  node scripts/gen-js8-jsc-dict.mjs            (network; ~20 s)
// Out:  crates/js8/src/proto/jsc/dict.bin, crates/js8/src/proto/jsc/dict.sha256
import { createHash } from 'node:crypto'
import { writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { deflateRawSync } from 'node:zlib'

const COMMIT = 'a7ff1be0b389d287fdc56e2ea0d06962aa68127d'
const SRC = {
  'jsc_list.cpp': '677b6a874849bdf1dd668bb9ab95b28bd6880c0b928d2c52db0e11cc466ccc6b',
  'jsc_map.cpp': '63dcea037badcdc49c5fc29a23438b66953fbfefcc1e354331f58c7dec232927',
}
const SIZE = 262144
const PREFIX_SIZE = 103
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const outDir = join(repo, 'crates/js8/src/proto/jsc')

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex')

async function fetchPinned(name) {
  const url = `https://raw.githubusercontent.com/js8call/js8call/${COMMIT}/${name}`
  const r = await fetch(url)
  if (!r.ok) throw new Error(`GET ${url} → ${r.status}`)
  const buf = Buffer.from(await r.arrayBuffer())
  const got = sha256(buf)
  if (got !== SRC[name]) throw new Error(`${name}: sha256 ${got} != pinned ${SRC[name]} — upstream moved; re-read the deep read before re-pinning`)
  return buf.toString('latin1')
}

// C string literal → bytes. Only the escapes upstream actually uses.
function unescapeC(lit) {
  const out = []
  for (let i = 0; i < lit.length; i++) {
    const c = lit[i]
    if (c !== '\\') { out.push(c.charCodeAt(0)); continue }
    const n = lit[++i]
    if (n === '\\') out.push(0x5c)
    else if (n === '"') out.push(0x22)
    else if (n === 'n') out.push(0x0a)
    else if (n === 'x') { out.push(parseInt(lit.slice(i + 1, i + 3), 16)); i += 2 }
    else throw new Error(`unknown escape \\${n} in ${JSON.stringify(lit)}`)
  }
  return Buffer.from(out)
}

function parseTable(src, name, count) {
  const start = src.indexOf(`${name}[${count}] = {`)
  if (start < 0) throw new Error(`${name} not found`)
  const body = src.slice(start)
  const re = /\{"((?:[^"\\]|\\.)*)"(?:\s*\/\*[^*]*\*\/)?,\s*(\d+),\s*(\d+)\}/g
  const rows = []
  let m
  while ((m = re.exec(body)) && rows.length < count) {
    rows.push({ str: unescapeC(m[1]), size: +m[2], index: +m[3] })
  }
  if (rows.length !== count) throw new Error(`${name}: parsed ${rows.length} rows, want ${count}`)
  return rows
}

const listSrc = await fetchPinned('jsc_list.cpp')
const mapSrc = await fetchPinned('jsc_map.cpp')
const map = parseTable(mapSrc, 'JSC::map', SIZE)
const list = parseTable(listSrc, 'JSC::list', SIZE)
const prefix = parseTable(listSrc, 'JSC::prefix', PREFIX_SIZE)

// P1
const quirks = []
map.forEach((r, i) => {
  if (r.index !== i) throw new Error(`P1: map[${i}].index = ${r.index}`)
  if (r.size !== r.str.length) quirks.push(`${i}:${r.str.toString('latin1')}:${r.size}`)
  if (r.str.length === 0 || r.str.length > 255 || r.size < 0 || r.size > 255) throw new Error(`P1: map[${i}] bad length`)
})
if (quirks.join(',') !== '81:@ALLCALL:7,262143:ROSIDS:1') throw new Error(`P1: consume quirks changed: ${quirks.join(',')}`)
// P2
const byWord = new Map()
map.forEach((r) => {
  const k = r.str.toString('latin1')
  if (byWord.has(k)) throw new Error(`P2: duplicate word ${JSON.stringify(k)}`)
  byWord.set(k, r.index)
})
list.forEach((r, pos) => {
  const k = r.str.toString('latin1')
  if (byWord.get(k) !== r.index) throw new Error(`P2: list[${pos}] ${JSON.stringify(k)} index ${r.index} != map ${byWord.get(k)}`)
})
// P3
const groupOf = new Map()
for (const p of prefix) {
  if (p.str.length !== 1) throw new Error('P3: prefix entry not 1 byte')
  if (!groupOf.has(p.str[0])) groupOf.set(p.str[0], p)
}
const reachable = new Array(SIZE).fill(false)
const marked = new Array(SIZE).fill(false)
const listPos = new Map()
for (const p of groupOf.values()) {
  for (let i = p.index; i < p.index + p.size; i++) {
    const r = list[i]
    if (r.str[0] !== p.str[0]) continue
    if (p.size === 1 && r.str.length !== 1) throw new Error(`P3: single-entry group ${p.str[0]} holds ${r.str}`)
    if (r.size !== r.str.length) throw new Error(`P3: reachable list[${i}] size ${r.size} != strlen ${r.str.length}`)
    const k = r.str.toString('latin1')
    if (listPos.has(k)) throw new Error(`P3: reachable key ${JSON.stringify(k)} listed twice`)
    listPos.set(k, i)
    reachable[r.index] = true
    marked[i] = true
  }
}
const unreachable = []
list.forEach((r, i) => { if (!marked[i]) unreachable.push(`${r.str.toString('latin1')}@${i}`) })
if (unreachable.join(',') !== '@ALLCALL@256644') throw new Error(`P3: unreachable set changed: ${unreachable.join(',')}`)
// P4
let checks = 0
for (const [b, posB] of listPos) {
  for (let l = 1; l < b.length; l++) {
    const posA = listPos.get(b.slice(0, l))
    if (posA === undefined) continue
    checks++
    if (posA < posB) throw new Error(`P4: prefix ${JSON.stringify(b.slice(0, l))} (pos ${posA}) precedes ${JSON.stringify(b)} (pos ${posB})`)
  }
}

// Canonical blob + pin.
const parts = []
for (const r of map) { parts.push(Buffer.from([r.str.length]), r.str, Buffer.from([r.size, reachable[r.index] ? 1 : 0])) }
const canonical = Buffer.concat(parts)
const pin = sha256(canonical)
const deflated = deflateRawSync(canonical, { level: 9 })
writeFileSync(join(outDir, 'dict.bin'), deflated)
writeFileSync(join(outDir, 'dict.sha256'), pin + '\n')
let maxLen = 0
for (const r of map) maxLen = Math.max(maxLen, r.str.length)
console.error(`P1-P4 ok (${checks} prefix-order checks, max word len ${maxLen}); canonical ${canonical.length} B sha256 ${pin}; dict.bin ${deflated.length} B`)
