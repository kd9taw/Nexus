// Task 1 of the POTA map plan: working a spot from the Connect map must tag the hunt
// target when the spot carries a POTA/SOTA program+reference — the plumbing a later task
// needs so double-clicking a park marker both QSYs to it AND credits the activator.
//
// Parses App.tsx rather than rendering it: the whole app is not mountable in jsdom (the
// App.rigmode.test.ts / App.logtoast.test.ts precedent, same honest limit).
//
// ⚠️ WHAT THIS FILE CANNOT SEE, and it cost a real defect. A regex over the source proves a call
// is WRITTEN, never that its result is handled: every assertion below passed for months while the
// call was `void setHuntTarget(...).catch(() => {})`, so a hunt the station refused was discarded
// in silence and the contact logged with no park. It also could not see that the tag ran BEFORE
// the work path's own bail-out, arming a pend for a QSY that never happened. Both are pinned by
// `App.mapHunt.test.tsx`, which mounts the real App and runs the handler. Keep this file for the
// cheap structural facts; do not read a green here as the behaviour being covered.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const APP = readFileSync(resolve(process.cwd(), 'src/App.tsx'), 'utf8')

/** The body of `handleWorkMapSpot`, up to the closing of its useCallback dependency list. */
function handlerBody(): string {
  const start = APP.indexOf('const handleWorkMapSpot')
  expect(start, 'handleWorkMapSpot must exist').toBeGreaterThan(-1)
  const end = APP.indexOf('[handleWorkNeeded', start)
  expect(end, 'handleWorkMapSpot must still depend on handleWorkNeeded').toBeGreaterThan(start)
  return APP.slice(start, end)
}

describe('handleWorkMapSpot tags the POTA hunt target (map work-spot path)', () => {
  it('imports setHuntTarget from the api', () => {
    expect(/^\s*setHuntTarget,\s*$/m.test(APP) || /\bsetHuntTarget\b/.test(APP)).toBe(true)
  })

  it('widens the onWorkSpot payload with the optional park identity', () => {
    const body = handlerBody()
    const sig = body.slice(0, body.indexOf('=>'))
    expect(sig).toContain('program?: string')
    expect(sig).toContain('reference?: string')
  })

  it('calls setHuntTarget with the SAME call/program/reference the spot carried, gated on both being present', () => {
    const body = handlerBody()
    expect(
      /if\s*\(spot\.program\s*&&\s*spot\.reference\b/.test(body),
      'must be gated — a plain spot with no park identity must not tag the hunt',
    ).toBe(true)
    expect(
      /setHuntTarget\(spot\.call,\s*program,\s*reference\)/.test(body),
      'must tag the hunt target with the spot’s own call/program/reference',
    ).toBe(true)
  })

  // The swallow this file could not see. Structural, and that is the point: the one shape that is
  // always wrong here is a discarded rejection, so name it rather than trust a future reader to.
  it('does not discard the result of setHuntTarget', () => {
    const body = handlerBody()
    expect(
      /\.catch\(\s*\(\s*\)\s*=>\s*\{\s*\}\s*\)/.test(body),
      'a swallowed rejection logs the contact with no park and tells the operator nothing',
    ).toBe(false)
  })
})
