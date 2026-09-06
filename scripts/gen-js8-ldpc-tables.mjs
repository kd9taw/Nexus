#!/usr/bin/env node
// Emit crates/js8/src/phy/ldpc_tables.rs — the LDPC(174,87) tables JS8 uses — from the two
// upstream Fortran sources, pinned by sha256, cross-checked BEFORE anything is written.
//
// WHY A GENERATOR AND NOT A HAND TRANSCRIPTION. 87 × 22 hex chars, 174 colorder entries,
// 174 × 3 + 87 × 7 parity-check integers: a single mistyped integer fails SILENTLY as decoder
// under-performance, and a wrong generator entry makes every Nexus frame undecodable by
// JS8Call while our own loopback stays green. So the numbers are parsed from the upstream
// files, the algebra (G·Hᵀ = 0, colorder a permutation, Nm ≡ Mn) is proved here in JS, and
// crates/js8/tests/tables.rs proves it AGAIN in Rust against the committed file.
//
// SOURCES (two different origins — that is the point: they agree only if both are right):
//   * G + colorder:  WSJT-X lib/ft8/ldpc_174_87_params.f90 (GPLv3, K1JT / WSJT Development
//     Group). Read from the phase2 WSJT-X tree on this box (--wsjtx-params <path>), and the
//     byte-identical copy JS8Call carries is fetched and compared.
//   * Mn / Nm / nrw: JS8Call lib/ft8/bpdecode174.f90 @ a7ff1be0 (fetched). This file is
//     WSJT-X 1.9.1's bpdecode174.f90 minus the `apmask` argument — the DATA blocks are
//     identical; pass --check-wsjtx-origin to fetch the 1.9.1 original from SourceForge and
//     assert that (network to sourceforge.net; optional because SF is flaky).
//
// NO NETWORK AT TEST TIME (the gen-wsjtx-callsign-oracle.mjs rule): this runs when a
// maintainer regenerates; the committed .rs is what CI reads. NOTHING UPSTREAM IS COMMITTED:
// the Fortran is parsed in memory; only integer tables reach the tree. Output is deterministic
// and rustfmt'd, so re-running on an unchanged tree is a no-op diff.
//
// Run:  node scripts/gen-js8-ldpc-tables.mjs [--wsjtx-params <path>] [--check-wsjtx-origin]
// Out:  crates/js8/src/phy/ldpc_tables.rs
// Read by: crates/js8/src/phy/ldpc.rs (encode87 / decode174), crates/js8/tests/tables.rs

import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const out = join(repo, 'crates/js8/src/phy/ldpc_tables.rs')

const JS8CALL_COMMIT = 'a7ff1be0b389d287fdc56e2ea0d06962aa68127d'
const RAW = (p) => `https://raw.githubusercontent.com/js8call/js8call/${JS8CALL_COMMIT}/${p}`
const WSJTX_191 = (p) => `https://sourceforge.net/p/wsjt/wsjtx/ci/wsjtx-1.9.1/tree/${p}?format=raw`

// sha256 of each source, verified 2026-09-05. A moved pin means upstream changed a table —
// that is a protocol change, not a regeneration: STOP and read the diff.
const PIN = {
  wsjtxParams: '98f802adb997b856801366c12567d9af0c089fdc96b1523e012508adb05f8650',
  js8callBpdecode: 'c185d6e5716b7f18c5010e29361d46df47708bc383aabd9f36b8dd1b086a6c5f',
  wsjtx191Bpdecode: '1959ad7f2f08bcf70a50db9c32edd68d22bf69dd52c113b8021b263625557f2e',
}

const argv = process.argv.slice(2)
const argAfter = (name) => {
  const i = argv.indexOf(name)
  return i >= 0 ? argv[i + 1] : undefined
}
const paramsPath =
  argAfter('--wsjtx-params') ??
  join(homedir(), 'work/ft1/research/phase2/wsjtx-source/lib/ft8/ldpc_174_87_params.f90')
const checkOrigin = argv.includes('--check-wsjtx-origin')

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex')

function pinned(name, buf, want) {
  const got = sha256(buf)
  if (got !== want) throw new Error(`${name}: sha256 ${got} != pinned ${want}`)
  return buf.toString('utf8')
}

async function fetchBytes(url) {
  const r = await fetch(url, { redirect: 'follow' })
  if (!r.ok) throw new Error(`GET ${url} → ${r.status}`)
  return Buffer.from(await r.arrayBuffer())
}

// ---------------------------------------------------------------- parse the Fortran DATA blocks

function dataBlock(src, name) {
  const m = src.match(new RegExp(`data\\s+${name}\\s*/([\\s\\S]*?)/`))
  if (!m) throw new Error(`no 'data ${name}/…/' block`)
  return m[1]
}
const ints = (block) => (block.match(/\d+/g) ?? []).map(Number)

function parseParams(src) {
  const g = [...dataBlock(src, 'g').matchAll(/"([0-9a-f]{22})"/g)].map((m) => m[1])
  if (g.length !== 87) throw new Error(`expected 87 generator rows, got ${g.length}`)
  const colorder = ints(dataBlock(src, 'colorder'))
  if (colorder.length !== 174) throw new Error(`expected 174 colorder entries, got ${colorder.length}`)
  return { g, colorder }
}

function parseBpdecode(src) {
  const mn = ints(dataBlock(src, 'Mn'))
  const nm = ints(dataBlock(src, 'Nm'))
  const nrw = ints(dataBlock(src, 'nrw'))
  const colorder = ints(dataBlock(src, 'colorder'))
  if (mn.length !== 174 * 3) throw new Error(`Mn: ${mn.length} ints`)
  if (nm.length !== 87 * 7) throw new Error(`Nm: ${nm.length} ints`)
  if (nrw.length !== 87) throw new Error(`nrw: ${nrw.length} ints`)
  if (colorder.length !== 174) throw new Error(`colorder: ${colorder.length} ints`)
  const Mn = Array.from({ length: 174 }, (_, i) => mn.slice(3 * i, 3 * i + 3))
  const Nm = Array.from({ length: 87 }, (_, j) => nm.slice(7 * j, 7 * j + 7))
  return { Mn, Nm, nrw, colorder }
}

// ---------------------------------------------------------------- cross-checks (all 1-based here)

function check(params, bp) {
  const sorted = [...params.colorder].sort((a, b) => a - b)
  if (sorted.some((v, i) => v !== i)) throw new Error('colorder is not a permutation of 0..173')
  if (params.colorder.some((v, i) => v !== bp.colorder[i])) throw new Error('colorder differs between the two files')
  for (let j = 0; j < 87; j++) {
    const used = bp.Nm[j].filter((v) => v !== 0)
    if (used.length !== bp.nrw[j]) throw new Error(`nrw[${j}] = ${bp.nrw[j]} but Nm row has ${used.length} entries`)
    if (bp.Nm[j].slice(bp.nrw[j]).some((v) => v !== 0)) throw new Error(`Nm row ${j} has a non-zero past nrw`)
  }
  const hNm = Array.from({ length: 87 }, () => new Uint8Array(174))
  const hMn = Array.from({ length: 87 }, () => new Uint8Array(174))
  bp.Nm.forEach((row, j) => row.slice(0, bp.nrw[j]).forEach((b) => (hNm[j][b - 1] = 1)))
  bp.Mn.forEach((checks, i) => checks.forEach((c) => (hMn[c - 1][i] = 1)))
  for (let j = 0; j < 87; j++) for (let i = 0; i < 174; i++) if (hNm[j][i] !== hMn[j][i]) throw new Error(`H(Nm) != H(Mn) at (${j},${i})`)

  const gen = params.g.map((row) => {
    const bits = []
    for (let j = 0; j < 11; j++) {
      const byte = parseInt(row.slice(2 * j, 2 * j + 2), 16)
      for (let k = 0; k < 8; k++) bits.push((byte >> (7 - k)) & 1)
    }
    return bits.slice(0, 87)
  })
  const encode = (msg) => {
    const itmp = gen.map((row) => row.reduce((acc, g, j) => acc ^ (g & msg[j]), 0)).concat(msg)
    const cw = new Array(174)
    params.colorder.forEach((c, i) => (cw[c] = itmp[i]))
    return cw
  }
  const isCodeword = (cw) => hNm.every((row) => row.reduce((acc, h, i) => acc ^ (h & cw[i]), 0) === 0)
  for (let k = 0; k < 87; k++) {
    const msg = new Array(87).fill(0)
    msg[k] = 1
    if (!isCodeword(encode(msg))) throw new Error(`G·Hᵀ != 0 for unit message e_${k}`)
  }
  let state = 0x2545F491n
  for (let t = 0; t < 200; t++) {
    const msg = Array.from({ length: 87 }, () => {
      state = (state * 6364136223846793005n + 1442695040888963407n) & ((1n << 64n) - 1n)
      return Number((state >> 33n) & 1n)
    })
    if (!isCodeword(encode(msg))) throw new Error(`G·Hᵀ != 0 for random message ${t}`)
  }
  return gen
}

// ---------------------------------------------------------------- emit

function emit(params, bp) {
  const hexRow = (row) => Array.from({ length: 11 }, (_, j) => `0x${row.slice(2 * j, 2 * j + 2)}`).join(', ')
  const lines = []
  lines.push('//! GENERATED by scripts/gen-js8-ldpc-tables.mjs — do not edit by hand; re-run the script.')
  lines.push('//!')
  lines.push('//! LDPC(174,87) tables transcribed as FACTS (see NOTICE, "JS8Call — protocol facts").')
  lines.push('//! Generator rows + column order: WSJT-X lib/ft8/ldpc_174_87_params.f90 (GPLv3), sha256 in')
  lines.push('//! [`SOURCE_WSJTX_PARAMS_SHA256`]. Parity-check Mn/Nm/nrw: JS8Call lib/ft8/bpdecode174.f90 @')
  lines.push(`//! ${JS8CALL_COMMIT.slice(0, 8)} (WSJT-X 1.9.1's file, GPLv3), sha256 in [\`SOURCE_JS8CALL_BPDECODE_SHA256\`].`)
  lines.push('//! All indices are 0-BASED here (the Fortran is 1-based). Unused `NM` slots past `NRW[j]`')
  lines.push('//! hold `u8::MAX` so an off-by-one read panics on index instead of silently using bit 0.')
  lines.push('//! `crates/js8/tests/tables.rs` proves G·Hᵀ = 0 against this file on every `cargo test`.')
  lines.push('')
  lines.push(`pub const SOURCE_WSJTX_PARAMS_SHA256: &str = "${PIN.wsjtxParams}";`)
  lines.push(`pub const SOURCE_JS8CALL_BPDECODE_SHA256: &str = "${PIN.js8callBpdecode}";`)
  lines.push('')
  lines.push('/// Generator row i as 11 bytes; message bit j is `(GENERATOR[i][j / 8] >> (7 - j % 8)) & 1`, j < 87.')
  lines.push('pub const GENERATOR: [[u8; 11]; 87] = [')
  for (const row of params.g) lines.push(`    [${hexRow(row)}],`)
  lines.push('];')
  lines.push('')
  lines.push('/// On-air position of `[parity(87) | message(87)][i]`: `cw[COLORDER[i]] = itmp[i]`.')
  lines.push(`pub const COLORDER: [u8; 174] = [${params.colorder.join(', ')}];`)
  lines.push('')
  lines.push('/// The three checks each bit takes part in (column weight 3).')
  lines.push('pub const MN: [[u8; 3]; 174] = [')
  for (const checks of bp.Mn) lines.push(`    [${checks.map((c) => c - 1).join(', ')}],`)
  lines.push('];')
  lines.push('')
  lines.push('/// The bits each check covers; only the first `NRW[j]` entries are valid.')
  lines.push('pub const NM: [[u8; 7]; 87] = [')
  for (let j = 0; j < 87; j++) {
    const row = bp.Nm[j].map((b, k) => (k < bp.nrw[j] ? b - 1 : 255))
    lines.push(`    [${row.join(', ')}],`)
  }
  lines.push('];')
  lines.push('')
  lines.push('/// Number of valid entries in each `NM` row (5, 6 or 7).')
  lines.push(`pub const NRW: [u8; 87] = [${bp.nrw.join(', ')}];`)
  lines.push('')
  return lines.join('\n')
}

// ---------------------------------------------------------------- main

const localParams = pinned('local WSJT-X ldpc_174_87_params.f90', readFileSync(paramsPath), PIN.wsjtxParams)
const js8callParams = pinned('JS8Call lib/ft8/ldpc_174_87_params.f90', await fetchBytes(RAW('lib/ft8/ldpc_174_87_params.f90')), PIN.wsjtxParams)
if (localParams !== js8callParams) throw new Error('JS8Call and WSJT-X ldpc_174_87_params.f90 differ (same sha256?!)')
const bpdecodeSrc = pinned('JS8Call lib/ft8/bpdecode174.f90', await fetchBytes(RAW('lib/ft8/bpdecode174.f90')), PIN.js8callBpdecode)

const params = parseParams(localParams)
const bp = parseBpdecode(bpdecodeSrc)

if (checkOrigin) {
  const origin = pinned('WSJT-X 1.9.1 lib/ft8/bpdecode174.f90', await fetchBytes(WSJTX_191('lib/ft8/bpdecode174.f90')), PIN.wsjtx191Bpdecode)
  const o = parseBpdecode(origin)
  const same = JSON.stringify([o.Mn, o.Nm, o.nrw, o.colorder]) === JSON.stringify([bp.Mn, bp.Nm, bp.nrw, bp.colorder])
  if (!same) throw new Error('WSJT-X 1.9.1 bpdecode174.f90 tables differ from JS8Call\'s copy')
  console.log('ok: WSJT-X 1.9.1 origin tables identical to JS8Call\'s copy')
}

check(params, bp)
writeFileSync(out, emit(params, bp))
execFileSync('rustfmt', ['--edition', '2021', out], { stdio: 'inherit' })
console.log(`wrote ${out}: 87 generator rows, 174 colorder, 174×3 Mn, 87×7 Nm, 87 nrw — G·Hᵀ = 0 verified`)
