#!/usr/bin/env node
// Generate the table of Hamlib models whose CAT PTT can choose the radio's MIC or DATA input.
//
// WHY. Settings ▸ Radio ▸ Rig & CAT offers "Transmit audio source (CAT PTT): Front/Mic |
// Rear/Data" (#381) only on a radio that can do both, because Rear/Data keys with rigctld `T 3`
// (`RIG_PTT_ON_DATA`), and only a backend whose caps say `RIG_PTT_RIG_MICDATA` sends that as a
// DATA transmit (`TX1;` on a Kenwood: "DATA SEND (ACC2/USB input)"). WSJT-X asks the same caps
// field (`rig_get_caps_int(model, RIG_CAPS_PTT_TYPE)`) before it offers its own Rear/Data choice.
//
// ⚠️ NOT `--dump-caps`. `rigctl.c` writes its own `-P` option (default None) into
// `caps->ptt_type` before dumping, so every model's dump reads `PTT type: None`. The caps value
// survives in the PTT PORT's type, which `rig_init` copies from the caps and only `-P` replaces,
// and `-L` (list the config parameters) prints it: `ptt_type: … Default: RIG, Value: RIGMICDATA`.
//
// Source of truth: the BUNDLED Hamlib (`src-tauri/resources/hamlib`, 4.7.1). Models: every one
// `rigctl -l` lists, not only the picker's catalog, because "Show all" can pick any of them.
//
// Output (deterministic, no timestamp):
//   crates/tempo-audio/tests/fixtures/hamlib_ptt_micdata.json
//     { hamlib, models, micData: [{ model, name }, …], unread }
//   `rigmodels.rs` pins `PTT_MIC_DATA_RIGS` to `micData` less `PTT_DATA_NEVER_KEYS` (a model
//   whose caps say mic/data but whose driver's DATA key cannot key, named there with the reason).
//
// Run:  node scripts/gen-hamlib-ptt-micdata.mjs [path/to/rigctl]
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
const out = join(repo, 'crates/tempo-audio/tests/fixtures/hamlib_ptt_micdata.json')

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
// After printing the list, rigctl opens the radio: a port that cannot exist makes that fail at
// once (≈0.5 s a model instead of ≈2 s at the default port), and the list is already printed.
const NO_PORT = '/nonexistent/nexus-ptt-probe'

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

const micData = []
const unread = []
for (const { model, name } of list) {
  const conf = run(['-m', String(model), '-r', NO_PORT, '-L'])
  const block = conf.split(/^(?=\S)/m).find((b) => b.startsWith('ptt_type:'))
  const value = block?.match(/Value:\s*(\S+)/)?.[1]
  if (!value) {
    unread.push(model)
    continue
  }
  if (value === 'RIGMICDATA') micData.push({ model, name })
}

writeFileSync(
  out,
  JSON.stringify({ hamlib, models: list.length, micData, unread }, null, 2) + '\n',
)
console.log(`${out}: ${list.length} models read, ${micData.length} with mic/data PTT (${hamlib})`)
console.log(`micData: [${micData.map((r) => r.model).join(', ')}] (PTT_MIC_DATA_RIGS is this less PTT_DATA_NEVER_KEYS)`)
if (unread.length) console.log(`no ptt_type in -L (${unread.length}): ${unread.join(' ')}`)
