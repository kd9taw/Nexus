#!/usr/bin/env node
// Generate the Hamlib `Serial speed: <min>..<max>` table for every rig the picker offers.
//
// WHY THIS IS A SCRIPT AND NOT A HAND-WRITTEN TABLE. `RIG_FIXED_BAUD` (ui SettingsPanel.tsx)
// imposes a CAT rate when an operator picks a rig, and four rounds of it were built by
// transcribing baud menus out of hardware manuals across 61 models. That has the failure rate
// hand transcription always has: the IC-746 row shipped `[9600, 19200]` against a manual (Set
// Mode item 27) that reads 300/1200/4800/9600/19200/AUTO, so an operator running one at 4800
// was silently clobbered to 19200 — the exact bug the whole feature was written to fix. The
// table is now derived from ONE fact the backend states outright, `serial_rate_min ==
// serial_rate_max`, and this script is where that fact comes from so nobody re-types it.
//
// Source of truth: `rigctl --dump-caps -m <model>` from the BUNDLED Hamlib
// (`src-tauri/resources/hamlib`) — the same build that ships in the installer, so the caps the
// test checks are the caps the operator's copy will report. Model list: the two catalog tiers in
// `crates/tempo-audio/src/rigmodels.rs`, i.e. exactly the rigs the picker lists.
//
// Output (deterministic — no timestamp, so regenerating an unchanged tree is a no-op diff):
//   ui/src/components/__fixtures__/hamlibSerialSpeeds.json
//     { hamlib: "<version banner>", rigs: [{ model, name, min, max }, …] }
//   Rigs with no serial port at all (Dummy / NET / FLRig / TRXManager / Flex) declare no
//   `Serial speed:` line and are omitted — an entry for one of those is not admissible either.
//
// Run:  node scripts/gen-hamlib-serial-speeds.mjs [path/to/rigctl]
//   Default binary is the bundled `rigctl.exe`, which runs under WSL interop on the dev box;
//   pass a native `rigctl` (Hamlib 4.7.1) elsewhere. Re-run and diff after any Hamlib bump or
//   catalog change: a row that moves is a rig whose rate rule just changed.

import { readFileSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const rigctl = process.argv[2] ?? join(repo, 'src-tauri/resources/hamlib/rigctl.exe')
const catalog = join(repo, 'crates/tempo-audio/src/rigmodels.rs')
const out = join(repo, 'ui/src/components/__fixtures__/hamlibSerialSpeeds.json')

// `(model, "Friendly name"),` rows of the two `vec![…]` catalogs. Anchored at line start so a
// model number quoted inside a comment or a rig's own name ("FT-847") cannot be mistaken for one.
const models = [...readFileSync(catalog, 'utf8').matchAll(/^\s*\(\s*(\d+)\s*,/gm)]
  .map((m) => Number(m[1]))
const uniq = [...new Set(models)].sort((a, b) => a - b)
if (uniq.length < 100) throw new Error(`only ${uniq.length} models parsed from ${catalog} — check the pattern`)

let hamlib = ''
const rigs = []
const undumpable = []
for (const model of uniq) {
  // Non-zero exit is normal here: NET rigctl tries to reach a live rigctld and fails, and a
  // model the bundled Hamlib does not carry prints "Unknown rig num". Keep whatever it wrote
  // and let the missing caps header be what marks it, so a genuine failure is loud, not absent.
  // stdio 'ignore' on stdin: rigctl reads it, and inheriting the loop's would eat the model list.
  let caps = ''
  try {
    caps = execFileSync(rigctl, ['-m', String(model), '--dump-caps'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
  } catch (e) {
    caps = e.stdout ?? ''
  }
  if (!caps.includes('Caps dump for model')) {
    undumpable.push(model)
    continue
  }
  hamlib ||= caps.match(/^Hamlib version:\s*(.+)$/m)?.[1].trim() ?? ''
  const speed = caps.match(/^Serial speed:\s*(\d+)\.\.(\d+)\s*baud/m)
  if (!speed) continue // no serial port on this backend
  const name = `${caps.match(/^Mfg name:\s*(.+)$/m)?.[1].trim() ?? ''} ${caps.match(/^Model name:\s*(.+)$/m)?.[1].trim() ?? ''}`
  rigs.push({ model, name: name.trim(), min: Number(speed[1]), max: Number(speed[2]) })
}

writeFileSync(out, JSON.stringify({ hamlib, rigs }, null, 2) + '\n')
const fixed = rigs.filter((r) => r.min === r.max)
console.log(`${out}: ${rigs.length} serial rigs, ${fixed.length} with min == max (${hamlib})`)
if (undumpable.length) console.log(`no caps at all (network-only or not in this Hamlib): ${undumpable.join(' ')}`)
