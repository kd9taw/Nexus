#!/usr/bin/env node
// Generate the amateur-satellite TLE mirror payload (tles.json + tles.txt).
//
// WHY A MIRROR: Celestrak's 2026 policies (a 2 h update cycle, one-download-
// per-cycle 403s on popular groups, per-IP caps, and developers held
// responsible for their apps' fleet behavior) make every-install-fetches-
// directly a scaling liability. This script runs in CI every 6 h
// (.github/workflows/tles.yml), fetches Celestrak ONCE, and publishes a
// rolling GitHub Release asset that hamradiotools.io/nexus/tles.json 302s to.
// Nexus installs refresh against the mirror with conditional GETs (the
// release asset serves an ETag; an unchanged set costs a ~300-byte 304) and
// touch Celestrak directly only as a narrow client-side fallback.
//
// SANITY GATES — ANY failure publishes NOTHING (exit 1; the Release keeps its
// last good asset, which clients re-validate on their side anyway; the Rust
// twin of these gates is propagation::live::tle::validate_tles):
//   1. HTTP 200, and no "No GP data found" (Celestrak's cold-cache 200 body —
//      it must never read as "zero satellites").
//   2. Bird count >= max(60, ceil(0.85 × previous)) — the live amateur group
//      is ~95 birds; the ratchet against the previously published manifest
//      catches a silent mass shrink a fixed floor would wave through.
//   3. Bird count <= 400 (a wrong or merged group would balloon).
//   4. Every NORAD numeric and unique.
//   5. NORAD 25544 (ISS) present — the canary that this IS the amateur group.
//   6. Every element line exactly 69 chars with a valid mod-10 checksum (all
//      ~190 live lines pass — a free truncation/corruption gate).
//   7. Epoch freshness: median age <= 7 d AND p90 <= 14 d. Median/p90, NEVER
//      max — AO-10 is legitimately old and must not fail a fresh set.
//   8. Round-trip: re-serializing the parsed set and re-parsing it yields
//      identical elements (parser/format drift dies here, not on clients).
//
// Output:
//   tles.json — manifest-is-payload (~16 KB): {schema, generated, source,
//               count, medianEpochAgeDays, elements:[{name,line1,line2}]}
//               (element keys match crates/propagation sat::Tle serde).
//   tles.txt  — the raw 3LE text exactly as fetched.
//
// Run:  node scripts/gen-tles.mjs [outDir]
//   TLES_SOURCE_URL — override the source (https://…, file://…, or a bare
//                     path). THE TEST SEAM: point it at a fixture.
//   TLES_PREV       — path to the previously published tles.json (the count
//                     ratchet's baseline); missing/unreadable = fixed floor.
//   TLES_NOW        — override "now" (unix seconds or ISO date) for the
//                     freshness gate, so a committed fixture tests the same
//                     way forever.

import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const CELESTRAK_URL = 'https://celestrak.org/NORAD/elements/gp.php?GROUP=amateur&FORMAT=tle'
const UA = 'nexus-tles-mirror/1.0 (+https://hamradiotools.io; ham radio satellite tracking; CI, 4x/day)'

const fail = (msg) => {
  console.error(`GATE FAILED: ${msg}`)
  process.exit(1)
}

// TLE mod-10 line checksum (column 69): sum of digits, '-' counts 1, over the
// first 68 columns. Matches propagation::live::tle::tle_line_checksum_ok.
function checksumOk(line) {
  let sum = 0
  for (const c of line.slice(0, 68)) {
    if (c >= '0' && c <= '9') sum += +c
    else if (c === '-') sum += 1
  }
  return sum % 10 === +line[68]
}

// TLE epoch (line 1 cols 19–32: 2-digit year + fractional day-of-year) → unix
// seconds. Matches propagation::sat::tle_age_days' year window (57 pivot).
function epochUnix(line1) {
  const yy = parseInt(line1.slice(18, 20), 10)
  const doy = parseFloat(line1.slice(20, 32))
  if (!Number.isFinite(yy) || !Number.isFinite(doy) || doy < 1) return null
  const year = yy < 57 ? 2000 + yy : 1900 + yy
  return Date.UTC(year, 0, 1) / 1000 + (doy - 1) * 86400
}

// STRICT 3LE parse: name / line1 / line2 triples and nothing else. The client
// parser is tolerant (skips malformed birds); the mirror publishes only
// perfect data, so any structural surprise is a gate failure, by line number.
function parseStrict(text) {
  const lines = text.split(/\r?\n/).filter((l) => l.trim() !== '')
  if (lines.length % 3 !== 0) fail(`line count ${lines.length} is not a multiple of 3 — truncated or reformatted feed`)
  const els = []
  for (let i = 0; i < lines.length; i += 3) {
    const name = lines[i].replace(/^0 /, '').trim()
    const l1 = lines[i + 1].trimEnd()
    const l2 = lines[i + 2].trimEnd()
    if (name.startsWith('1 ') || name.startsWith('2 ')) fail(`line ${i + 1}: expected a name line, got an element line`)
    if (!l1.startsWith('1 ')) fail(`line ${i + 2}: expected TLE line 1`)
    if (!l2.startsWith('2 ')) fail(`line ${i + 3}: expected TLE line 2`)
    for (const [l, which] of [[l1, 1], [l2, 2]]) {
      if (l.length !== 69) fail(`${name}: line ${which} is ${l.length} chars, not 69`)
      if (!checksumOk(l)) fail(`${name}: line ${which} fails the mod-10 checksum`)
    }
    els.push({ name, line1: l1, line2: l2 })
  }
  return els
}

function parseNow(env) {
  if (!env) return Date.now() / 1000
  const n = Number(env)
  if (Number.isFinite(n) && n > 0) return n
  const t = Date.parse(env)
  if (Number.isFinite(t)) return t / 1000
  fail(`TLES_NOW=${env} is neither unix seconds nor an ISO date`)
}

async function main() {
  const outDir = process.argv[2] || process.cwd()
  const src = process.env.TLES_SOURCE_URL || CELESTRAK_URL

  // Gate 1: fetch (the ONE Celestrak touch per CI run) — or read the fixture.
  let text
  if (/^https?:/.test(src)) {
    console.error(`fetching ${src} …`)
    const resp = await fetch(src, { headers: { 'user-agent': UA } })
    if (!resp.ok) fail(`HTTP ${resp.status} from ${src}`)
    text = await resp.text()
  } else {
    text = readFileSync(src.replace(/^file:\/\//, ''), 'utf8')
  }
  if (/No GP data found/i.test(text)) fail('Celestrak answered "No GP data found" (cold cache) — not zero satellites')

  // Gate 6 runs inside the strict parse (shape + checksum, by line).
  const els = parseStrict(text)

  // Gates 2 + 3: plausible count, ratcheted against the last published set.
  let prev = 0
  try {
    prev = JSON.parse(readFileSync(process.env.TLES_PREV, 'utf8')).count | 0
  } catch {
    console.error('no previous manifest — count gate uses the fixed floor only')
  }
  const floor = Math.max(60, Math.ceil(prev * 0.85))
  if (els.length < floor) fail(`only ${els.length} birds (< max(60, 0.85×${prev}) = ${floor})`)
  if (els.length > 400) fail(`${els.length} birds — that is not the amateur group`)

  // Gates 4 + 5: NORADs numeric + unique; the ISS canary present.
  const norads = new Set()
  for (const e of els) {
    const raw = e.line1.slice(2, 7).trim()
    if (!/^\d+$/.test(raw)) fail(`${e.name}: NORAD "${raw}" is not numeric`)
    if (norads.has(raw)) fail(`${e.name}: duplicate NORAD ${raw}`)
    norads.add(raw)
  }
  if (!norads.has('25544')) fail('canary NORAD 25544 (ISS) missing — wrong group or mangled payload')

  // Gate 7: epoch freshness — median/p90, never max (AO-10 is legitimately old).
  const now = parseNow(process.env.TLES_NOW)
  const ages = els
    .map((e) => {
      const ep = epochUnix(e.line1)
      if (ep === null) fail(`${e.name}: unparseable epoch`)
      return (now - ep) / 86400
    })
    .sort((a, b) => a - b)
  const median = ages[Math.floor(ages.length / 2)]
  const p90 = ages[Math.min(ages.length - 1, Math.floor(ages.length * 0.9))]
  if (median > 7) fail(`median epoch age ${median.toFixed(1)} d (> 7 d) — the feed has gone stale`)
  if (p90 > 14) fail(`p90 epoch age ${p90.toFixed(1)} d (> 14 d) — too much of the set is stale`)

  // Gate 8: round-trip — canonical serialization re-parses to the same set.
  const canonical = els.map((e) => `${e.name}\n${e.line1}\n${e.line2}`).join('\n') + '\n'
  if (JSON.stringify(parseStrict(canonical)) !== JSON.stringify(els)) fail('round-trip re-parse mismatch — parser drift')

  // Publish payload. `generated` is the real publication stamp (TLES_NOW only
  // steers the freshness gate); elements stay compact — this file is the
  // payload clients download.
  const manifest = {
    schema: 1,
    generated: new Date().toISOString(),
    source: 'celestrak amateur group',
    count: els.length,
    medianEpochAgeDays: +median.toFixed(2),
    elements: els,
  }
  writeFileSync(join(outDir, 'tles.json'), JSON.stringify(manifest) + '\n')
  writeFileSync(join(outDir, 'tles.txt'), text)
  console.error(`ok: ${els.length} birds, median epoch age ${median.toFixed(2)} d, p90 ${p90.toFixed(2)} d`)
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
