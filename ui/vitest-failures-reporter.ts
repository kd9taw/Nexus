// A red vitest run leaves its failing tests' names behind — in a file, and as the
// LAST thing printed. Added 2026-09-16 after a bare `npx vitest run` went 2/5929 red,
// green on rerun, and nobody could say which two: vitest's default reporter names
// them in a "Failed Tests" block that sits above 5,900 lines of per-file output,
// and its closing summary is counts only, so a reader of the tail learns "2 failed"
// and nothing else. scripts/gates reads the same names out of its own log; this is
// for the runs that do not go through it.
//
// Not a replacement for the default reporter — it sits beside it (vite.config.ts).
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import type { Reporter } from 'vitest/node'

// ui/test-results/failures.txt — gitignored. Written on EVERY run, so a stale red
// file from an earlier run can never be read as this run's verdict.
const OUT = fileURLToPath(new URL('./test-results/failures.txt', import.meta.url))

export const failuresReporter: Reporter = {
  onTestRunEnd(modules, unhandledErrors) {
    const lines: string[] = []
    for (const m of modules) {
      const file = path.relative(process.cwd(), m.moduleId)
      for (const t of m.children.allTests('failed')) {
        lines.push(`${file} > ${t.fullName}`)
        const r = t.result()
        for (const e of (r.errors ?? []).slice(0, 1)) {
          lines.push(`    ${String(e.message ?? '').split('\n')[0]}`)
        }
      }
    }
    for (const e of unhandledErrors) {
      lines.push(`unhandled error: ${String((e as { message?: string }).message ?? e).split('\n')[0]}`)
    }
    const head = lines.length
      ? `failing test(s), ${lines.filter((l) => !l.startsWith('    ')).length}:`
      : 'failing test(s), 0'
    fs.mkdirSync(path.dirname(OUT), { recursive: true })
    fs.writeFileSync(OUT, `${head}\n${lines.join('\n')}\n`)
    if (lines.length) {
      process.stdout.write(`\n${head}\n${lines.map((l) => `  ${l}`).join('\n')}\n(also in ${OUT})\n`)
    }
  },
}
