#!/usr/bin/env node
// Generate the Hamlib XIT capability table for every rig the picker offers.
//
// WHY THIS IS A SCRIPT AND NOT A HAND-WRITTEN LIST. `settings::NO_XIT_RIGS` decides which radios
// Nexus offers no XIT on: the XIT buttons are hidden, a request is refused, and the licence gate
// stops adding an XIT offset to the frequency it judges. It began as the IC-9700 alone, found by
// reading Icom's CI-V reference, and a radio missing from a deny-list keeps the phantom offset —
// the offset the station believes in, draws on the transmit line and judges the licence by,
// while the radio transmits without it. The fact comes from ONE line Hamlib states outright per
// model, `Can set XIT: Y|N`, and this script is where it comes from so nobody re-types it.
//
// Source of truth: `rigctl --dump-caps -m <model>` from the BUNDLED Hamlib
// (`src-tauri/resources/hamlib`, 4.7.1) — the build that ships in the installer, so the table
// the test checks is what the operator's copy reports. Model list: the two catalog tiers in
// `crates/tempo-audio/src/rigmodels.rs`, i.e. exactly the rigs the picker lists.
//
// Output (deterministic — no timestamp, so regenerating an unchanged tree is a no-op diff):
//   crates/tempo-audio/tests/fixtures/hamlib_xit.json
//     { hamlib, catalog, rigs: [{ model, name, setXit, getXit, xitFunc }, …], missing,
//       needsDaemon }
//   `setXit` is `Can set XIT:` (the offset, Hamlib's `set_xit`); `xitFunc` is whether XIT is
//   among the rig's settable functions (the on/off switch Nexus sends first, `U XIT`).
//   `rigmodels.rs` pins `NO_XIT_RIGS` to exactly the rigs with `setXit: false`.
//
// Run:  node scripts/gen-hamlib-xit.mjs [path/to/rigctl]
//   Default binary is the bundled `rigctl.exe`, which runs under WSL interop on the dev box;
//   pass a native `rigctl` (Hamlib 4.7.1) elsewhere. Re-run and diff after any Hamlib bump or
//   catalog change: a row that moves is a rig whose XIT just appeared or went away.

import { readFileSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const rigctl = process.argv[2] ?? join(repo, 'src-tauri/resources/hamlib/rigctl.exe')
const catalog = join(repo, 'crates/tempo-audio/src/rigmodels.rs')
const out = join(repo, 'crates/tempo-audio/tests/fixtures/hamlib_xit.json')

// The same catalog pattern `gen-hamlib-serial-speeds.mjs` reads: `(model, "Friendly name"),`
// rows anchored at line start, so a number inside a comment or a name cannot be mistaken for one.
const models = [...readFileSync(catalog, 'utf8').matchAll(/^\s*\(\s*(\d+)\s*,/gm)]
  .map((m) => Number(m[1]))
const uniq = [...new Set(models)].sort((a, b) => a - b)
if (uniq.length < 100) throw new Error(`only ${uniq.length} models parsed from ${catalog} — check the pattern`)

const yes = (caps, label) => {
  const m = caps.match(new RegExp(`^${label}:\\s*([YN])\\b`, 'm'))
  if (!m) throw new Error(`no "${label}:" line in a caps dump — the format moved`)
  return m[1] === 'Y'
}

let hamlib = ''
const rigs = []
const missing = []
const needsDaemon = []
for (const model of uniq) {
  // As in the serial-speeds script: a non-zero exit is normal for NET rigctl and for a model this
  // Hamlib does not carry, so keep what it wrote and let the missing caps header mark it.
  let caps = ''
  let err = ''
  try {
    caps = execFileSync(rigctl, ['-m', String(model), '--dump-caps'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  } catch (e) {
    caps = e.stdout ?? ''
    err = e.stderr ?? ''
  }
  if (!caps.includes('Caps dump for model')) {
    ;(err.includes('Unknown rig num') ? missing : needsDaemon).push(model)
    continue
  }
  hamlib ||= caps.match(/^Hamlib version:\s*(.+)$/m)?.[1].trim() ?? ''
  const name = `${caps.match(/^Mfg name:\s*(.+)$/m)?.[1].trim() ?? ''} ${caps.match(/^Model name:\s*(.+)$/m)?.[1].trim() ?? ''}`
  const setFuncs = caps.match(/^Set functions:(.*)$/m)?.[1].trim().split(/\s+/) ?? []
  rigs.push({
    model,
    name: name.trim(),
    setXit: yes(caps, 'Can set XIT'),
    getXit: yes(caps, 'Can get XIT'),
    xitFunc: setFuncs.includes('XIT'),
  })
}

writeFileSync(
  out,
  JSON.stringify({ hamlib, catalog: uniq.length, rigs, missing, needsDaemon }, null, 2) + '\n',
)
const noXit = rigs.filter((r) => !r.setXit)
const noFunc = rigs.filter((r) => r.setXit && !r.xitFunc)
console.log(`${out}: ${rigs.length} rigs dumped, ${noXit.length} with Can set XIT: N (${hamlib})`)
console.log(`NO_XIT_RIGS: [${noXit.map((r) => r.model).join(', ')}]`)
if (noFunc.length) console.log(`XIT offset but no XIT function (${noFunc.length}): ${noFunc.map((r) => r.model).join(' ')}`)
if (needsDaemon.length) console.log(`present but needs a live daemon to dump: ${needsDaemon.join(' ')}`)
if (missing.length) console.log(`not in this Hamlib: ${missing.join(' ')}`)
