// A scratch folder under the system temp directory that is removed however the process ends.
//
// WHY. On 2026-10-30 /tmp on the development box, a RAM disk, filled to 100% and every tool on it
// failed. Among what filled it were the scratch folders of these script tests, made with
// mkdtempSync and never removed: dozens per pass, thousands over a few days of agents running
// gates. Removing them at the end of a test is not enough. A red ends the run early, and Ctrl-C,
// `timeout`, a closed terminal or the test runner stopping its child all signal the process,
// which by default dies there and runs no cleanup at all.
//
// So every folder made here is removed on 'exit', and SIGINT, SIGTERM and SIGHUP are turned into
// an ordinary exit, with the code a shell reports for that signal, so that 'exit' runs for them
// too. SIGKILL cannot be caught; nothing in a process can clean up after it.
//
// Tested in scripts/scratch-dir.test.mjs.

import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const owned = []

/** A new folder `<tmpdir>/<prefix>XXXXXX`, removed with everything in it when this process ends. */
export function scratchDir(prefix) {
  const dir = mkdtempSync(join(tmpdir(), prefix))
  if (!owned.length) {
    process.on('exit', () => {
      for (const d of owned) rmSync(d, { recursive: true, force: true })
    })
    for (const [signal, number] of [['SIGINT', 2], ['SIGTERM', 15], ['SIGHUP', 1]]) {
      process.on(signal, () => process.exit(128 + number))
    }
  }
  owned.push(dir)
  return dir
}
