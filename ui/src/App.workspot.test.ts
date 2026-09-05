// Task 1 of the POTA map plan: working a spot from the Connect map must tag the hunt
// target when the spot carries a POTA/SOTA program+reference — the plumbing a later task
// needs so double-clicking a park marker both QSYs to it AND credits the activator.
//
// Parses App.tsx rather than rendering it: the whole app is not mountable in jsdom (the
// App.rigmode.test.ts / App.logtoast.test.ts precedent, same honest limit).
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const APP = readFileSync(resolve(process.cwd(), 'src/App.tsx'), 'utf8')

/** The body of `handleWorkMapSpot`, up to its closing `[handleWorkNeeded])`. */
function handlerBody(): string {
  const start = APP.indexOf('const handleWorkMapSpot')
  expect(start, 'handleWorkMapSpot must exist').toBeGreaterThan(-1)
  return APP.slice(start, APP.indexOf('[handleWorkNeeded]', start))
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
      /if\s*\(t\.program\s*&&\s*t\.reference\)/.test(body),
      'must be gated — a plain spot with no park identity must not tag the hunt',
    ).toBe(true)
    expect(
      /setHuntTarget\(t\.call,\s*t\.program,\s*t\.reference\)/.test(body),
      'must tag the hunt target with the spot’s own call/program/reference',
    ).toBe(true)
  })
})
