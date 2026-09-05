#!/usr/bin/env node
// Fetch + validate AD1C's cty.dat country file and emit it with a manifest, for the weekly
// refresh pipeline (.github/workflows/cty.yml → the rolling `cty` Release →
// hamradiotools.io/nexus/cty.{dat,json} → the in-app download, src-tauri cty_download_if_newer).
//
// Source — the canonical stable URL, verified against country-files.com on 2026-08-29:
//   https://www.country-files.com/bigcty/cty.dat
//   BIG CTY, not the classic /cty/cty.dat, and the choice is load-bearing: the vendored seed
//   (crates/propagation/data/cty.dat, 358 KB, ~21k exact calls) IS the BIG CTY variant — the
//   classic file (102 KB, ~2.7k exact calls) lacks thousands of exact-call exceptions the
//   resolver's behavior is pinned on (e.g. `=KG4BBX`→Alaska in dxcc.rs's KG4 tests), so
//   serving classic would silently shrink coverage at the first refresh. Verified live on
//   2026-08-29: both stable URLs answered 200; /bigcty/cty.dat carried =VER20260826,
//   346 entities, 22,879 exact calls including =KG4BBX, matching the newest dated release
//   (bigcty-20260826.zip) by Last-Modified.
//   ⚠️ The server answers a bare/robot User-Agent with an HTML page instead of the file, so
//   both curl below and the sanity gate send a browser-ish UA and validation refuses HTML.
//
// Validation (the sanity gate the workflow re-runs): parse with the same rules as
// crates/propagation/src/dxcc.rs::parse_cty (8-field `:`-separated headers, indented alias
// continuations, `;` terminator, `=`-prefixed exact calls, zone annotations stripped), then
// require entities ≥ 340, prefixes ≥ 1000, exact calls ≥ 500, a `=VERyyyymmdd` marker, and
// two spot resolves on names that will never respell (W1AW → United States, JA1XYZ → Japan).
// Any miss exits 1 — refusing to publish a short/garbage file (the fcc-states.yml gate's
// shape). The runtime loader (dxcc::init_from) applies its own, looser floor (≥ 330) — the
// publisher gate is deliberately the stricter of the two.
//
// Output:
//   cty.dat  — the source BYTES verbatim (no newline normalization; the Rust parser handles
//              both endings).
//   cty.json — manifest {format, generated, source, ver, entities, prefixes, bytes, sha256}.
//              The in-app client keys its "is newer" compare on `ver` (the content-derived
//              AD1C date), NOT `generated` — a weekly re-publish of identical content must
//              not force every install to re-download.
//
// Run:  node scripts/gen-cty.mjs <cty.dat | --download> [outDir] [--dry-run]
//   --download fetches the current file first (needs `curl` on PATH — present on CI runners).
//   --dry-run  validates and prints the manifest to stdout, writes nothing (what the tests
//              run against the vendored crates/propagation/data/cty.dat — no network).

import { readFileSync, writeFileSync, mkdtempSync, existsSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const CTY_URL = 'https://www.country-files.com/bigcty/cty.dat' // BIG CTY — see header
// See the header: a bare UA gets an HTML page, not the file.
const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36'

/** Parse a cty.dat text with dxcc.rs::parse_cty's rules; returns counts + the version marker
 * + the entity index the spot resolves walk. */
function parseCty(text) {
  const entities = [] // entity names, in file order
  const exact = new Map() // CALL → entity index
  const prefixes = new Map() // prefix → entity index
  let cur = null
  let buf = ''
  let ver = null
  for (const line of text.split('\n')) {
    if (!line.trim()) continue
    const header = !(line[0] === ' ' || line[0] === '\t')
    if (header) {
      const parts = line.split(':')
      if (parts.length < 8) continue
      entities.push(parts[0].trim())
      cur = entities.length - 1
      buf = ''
    } else if (cur !== null) {
      buf += line.trim()
      const semi = buf.indexOf(';')
      if (semi >= 0) {
        for (const tok of buf.slice(0, semi).split(',')) {
          const t = tok.trim()
          if (!t) continue
          const isExact = t.startsWith('=')
          const body = isExact ? t.slice(1) : t
          // Cut at the first annotation char: (cq) [itu] {cont} <lat/lon> ~tz~.
          const cut = body.search(/[([{<~]/)
          const key = (cut < 0 ? body : body.slice(0, cut)).trim().toUpperCase()
          if (!key) continue
          if (isExact) {
            const m = /^VER(\d{8})$/.exec(key)
            if (m) ver = m[1]
            exact.set(key, cur)
          } else {
            prefixes.set(key, cur)
          }
        }
        buf = ''
      }
    }
  }
  return { entities, exact, prefixes, ver }
}

/** Exact-then-longest-prefix walk (dxcc.rs::spot_entity), for the validation spot resolves. */
function spotEntity(parsed, call) {
  if (parsed.exact.has(call)) return parsed.entities[parsed.exact.get(call)]
  for (let n = call.length; n > 0; n--) {
    const p = call.slice(0, n)
    if (parsed.prefixes.has(p)) return parsed.entities[parsed.prefixes.get(p)]
  }
  return null
}

/** The publish gate. Throws with a specific reason on any miss. */
function validate(parsed) {
  if (parsed.entities.length < 340)
    throw new Error(`only ${parsed.entities.length} entities (< 340) — refusing a short file`)
  if (parsed.prefixes.size < 1000)
    throw new Error(`only ${parsed.prefixes.size} prefixes (< 1000) — refusing a short file`)
  if (parsed.exact.size < 500)
    throw new Error(`only ${parsed.exact.size} exact calls (< 500) — refusing a short file`)
  if (!parsed.ver) throw new Error('no =VERyyyymmdd marker — not a release file')
  for (const [call, want] of [
    ['W1AW', 'United States'],
    ['JA1XYZ', 'Japan'],
  ]) {
    const got = spotEntity(parsed, call)
    if (got !== want) throw new Error(`spot resolve ${call} → ${JSON.stringify(got)}, want ${want}`)
  }
}

async function main() {
  const args = process.argv.slice(2).filter((a) => a !== '--dry-run')
  const dryRun = process.argv.includes('--dry-run')
  let src = args[0]
  const outDir = args[1] || process.cwd()
  if (!src) {
    console.error('usage: gen-cty.mjs <cty.dat | --download> [outDir] [--dry-run]')
    process.exit(2)
  }
  let source = src
  if (src === '--download') {
    const work = mkdtempSync(join(tmpdir(), 'cty-'))
    src = join(work, 'cty.dat')
    source = CTY_URL
    console.error(`downloading ${CTY_URL} …`)
    execFileSync('curl', ['-fsSL', '-A', UA, '-o', src, CTY_URL], {
      stdio: ['ignore', 'ignore', 'inherit'],
    })
  }
  if (!existsSync(src)) throw new Error(`no such file: ${src}`)

  const bytes = readFileSync(src) // verbatim bytes — this is what ships
  const text = bytes.toString('latin1') // cty.dat is ASCII; latin1 keeps byte-parity
  if (/^\s*</.test(text))
    throw new Error('got HTML, not cty.dat — the server rejected the User-Agent?')
  const parsed = parseCty(text)
  validate(parsed)

  const manifest = {
    format: 'AD1C-CTY1',
    generated: new Date().toISOString(),
    source,
    ver: parsed.ver,
    entities: parsed.entities.length,
    prefixes: parsed.prefixes.size,
    bytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  }
  console.error(
    `ok: =VER${parsed.ver} · ${parsed.entities.length} entities · ${parsed.prefixes.size} prefixes · ${parsed.exact.size} exact calls · ${bytes.length} bytes`,
  )
  if (dryRun) {
    console.log(JSON.stringify(manifest, null, 2))
    return
  }
  writeFileSync(join(outDir, 'cty.dat'), bytes)
  writeFileSync(join(outDir, 'cty.json'), JSON.stringify(manifest, null, 2) + '\n')
  console.error(`wrote ${join(outDir, 'cty.dat')} + cty.json`)
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
