#!/usr/bin/env node
// Generate the table of Hamlib models that cannot send CW over CAT (no `send_morse`).
//
// WHY. The CAT keyer keys CW with rigctld `b <text>`, Hamlib's `rig_send_morse`, which answers
// `-RIG_ENAVAIL` (`RPRT -11`) at once on a model whose driver has no `send_morse`
// (`src/rig.c`, `rig_send_morse`). On such a model the CAT keyer can never key, so Settings ▸ CW
// and the CW screen say so and name the keyers that can (`rigmodels::CAT_CW_NO_MORSE_RIGS`).
// Every Xiegu model of Hamlib 4.7.1 is one.
//
// Source of truth: the BUNDLED Hamlib (`src-tauri/resources/hamlib`, 4.7.1), and its own answer:
// the `Can send Morse:` line of `rigctl -m <model> -u` (`--dump-caps`), which `dumpcaps.c` prints
// `Y` when the driver's caps carry a `send_morse` and `N` when they do not. Models: every one
// `rigctl -l` lists, not only the picker's catalog, because "Show all" can pick any of them.
//
// Output (deterministic, no timestamp):
//   crates/tempo-audio/tests/fixtures/hamlib_send_morse.json
//     { hamlib, models, noMorse: [{ model, name }, …], unread }
//   `rigmodels.rs` pins `CAT_CW_NO_MORSE_RIGS` to `noMorse`.
//
// Run:  node scripts/gen-hamlib-send-morse.mjs [path/to/rigctl]
//   Default binary is the one `scripts/fetch-hamlib-unix.sh` (`rigctl`) or `scripts/fetch-hamlib.sh`
//   (`rigctl.exe`, a Windows program) stages in `src-tauri/resources/hamlib`. Re-run and diff
//   after any Hamlib bump.

import { existsSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const staged = join(repo, 'src-tauri/resources/hamlib')
const rigctl =
  process.argv[2] ??
  (existsSync(join(staged, 'rigctl')) ? join(staged, 'rigctl') : join(staged, 'rigctl.exe'))
const out = join(repo, 'crates/tempo-audio/tests/fixtures/hamlib_send_morse.json')

const run = (args) => {
  try {
    return execFileSync(rigctl, args, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: 10_000,
    })
  } catch (e) {
    // A non-zero exit is normal for some models; keep whatever it printed.
    return e.stdout ?? ''
  }
}

const hamlib = run(['--version']).match(/Hamlib\s+(.+)$/m)?.[0].trim() ?? ''
if (!hamlib.startsWith('Hamlib ')) throw new Error(`no Hamlib version from ${rigctl}`)

// `rigctl -l`: a header line, then ` <model>  <mfg>  <name>  <version>  <status>  <macro>`.
const list = run(['-l'])
  .split('\n')
  .slice(1)
  .map((l) => l.match(/^\s*(\d+)\s+(.*?)\s{2,}(.*?)\s{2,}/))
  .filter(Boolean)
  .map((m) => ({ model: Number(m[1]), name: `${m[2].trim()} ${m[3].trim()}` }))
if (list.length < 200) throw new Error(`only ${list.length} models from rigctl -l — check the pattern`)

const noMorse = []
const unread = []
for (const { model, name } of list) {
  const caps = run(['-m', String(model), '-u'])
  const value = caps.match(/^Can send Morse:\s*(\S)/m)?.[1]
  if (!value) {
    unread.push(model)
    continue
  }
  if (value === 'N') noMorse.push({ model, name })
}

writeFileSync(
  out,
  JSON.stringify({ hamlib, models: list.length, noMorse, unread }, null, 2) + '\n',
)
console.log(`${out}: ${list.length} models read, ${noMorse.length} with no send_morse (${hamlib})`)
console.log(`noMorse: [${noMorse.map((r) => r.model).join(', ')}]`)
if (unread.length) console.log(`no "Can send Morse" line (${unread.length}): ${unread.join(' ')}`)
