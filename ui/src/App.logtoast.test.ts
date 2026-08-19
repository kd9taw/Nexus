// #100 — the Log toast that lied.
//
// `Engine::log_current_qso` returns false when nothing was loggable (already logged, no
// active QSO, no report exchanged) — every refusal deliberate, each a logbook-integrity
// guard. The command discarded that bool and App toasted "Logged QSO" unconditionally, so
// a double-click after auto-log, or a click during a bare CQ, showed a green success for a
// write that never happened. The operator's logbook then "lost" a QSO the app claimed.
//
// Parses App.tsx rather than rendering it (the whole app is not mountable in jsdom — the
// App.rigmode.test.ts precedent, same honest limit): what is asserted is that the handler
// consults the command's `logged` verdict before claiming success. The engine's refusal
// behavior itself is pinned in engine.rs; the api shape in api.ts.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const APP = readFileSync(resolve(process.cwd(), 'src/App.tsx'), 'utf8')

/** The body of `handleLogCurrent`, up to its closing `}, [deps])`. */
function handlerBody(): string {
  const start = APP.indexOf('const handleLogCurrent')
  expect(start, 'handleLogCurrent must exist').toBeGreaterThan(-1)
  return APP.slice(start, APP.indexOf('}, [', start))
}

describe('the Log QSO toast tells the truth (#100)', () => {
  it('gates the success toast on the logged verdict, not on the call returning', () => {
    const body = handlerBody()
    // The handler must read the verdict the command now returns…
    expect(
      /\.logged\b/.test(body),
      'handleLogCurrent must consult the logged flag — toasting on mere return is the bug',
    ).toBe(true)
    // …and the success toast must be inside a branch on it, not unconditional.
    // The WORDS moved into the English catalog (i18n phase 2, `i18n/en.ts`), so the key is
    // what the handler names now and the key is what this guard follows. What is asserted is
    // unchanged: a success toast exists, and it sits behind the verdict.
    const successAt = body.indexOf("'shell.toast.logged'")
    expect(successAt, 'the success toast exists').toBeGreaterThan(-1)
    const beforeSuccess = body.slice(0, successAt)
    expect(
      /if\s*\([^)]*\.logged\b/.test(beforeSuccess),
      "the 'Logged QSO' toast must sit behind an if on the verdict",
    ).toBe(true)
  })

  it('says something honest on a refusal instead of nothing (or worse, success)', () => {
    const body = handlerBody()
    expect(
      /'shell\.toast\.nothingToLog'/.test(body),
      'a refused log must toast an honest explanation, not silence',
    ).toBe(true)
  })
})
