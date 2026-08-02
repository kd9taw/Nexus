// DISPLAY-LIVELINESS PINS (2026-08-01) — meters may not smear real data.
//
// The operator's report ("the S meter … feels accurate, but slow") had a CSS contributor: the
// meter fills carried a `transition: width …`, which eased every REAL sample by another
// 80–120 ms on top of the backend's ballistics and the poll cadence. Color transitions are
// cosmetic (zone changes) and stay; WIDTH is the measurement itself and must move the instant
// the value does.
//
// Census style (the codebase's guard-test idiom): this COMPUTES over every styles.css rule
// that targets the meter fills — including any later rule that would silently win the cascade —
// rather than grepping for one fixed declaration.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'

const css = readFileSync(new URL('./styles.css', import.meta.url), 'utf8')
  // Strip comments first so prose about the trap can never satisfy (or trip) the census.
  .replace(/\/\*[\s\S]*?\*\//g, '')

/** Every `selector { body }` block whose selector mentions `cls`. */
function rulesFor(cls: string): { selector: string; body: string }[] {
  const out: { selector: string; body: string }[] = []
  const re = /([^{}]+)\{([^{}]*)\}/g
  for (let m = re.exec(css); m; m = re.exec(css)) {
    if (m[1].includes(cls)) out.push({ selector: m[1].trim(), body: m[2] })
  }
  return out
}

describe('meter fills never transition width (the width IS the measurement)', () => {
  it.each(['.level-fill', '.ph-scope-smeter-fill'])('%s', (cls) => {
    const rules = rulesFor(cls)
    expect(rules.length, `no styles.css rule targets ${cls} — did the class move?`).toBeGreaterThan(0)
    for (const { selector, body } of rules) {
      const transition = /transition[^;]*;?/.exec(body)?.[0] ?? ''
      expect(
        /(^|[\s:,])width/.test(transition),
        `${selector} eases width — every real meter sample is smeared by the ease:\n${transition}`,
      ).toBe(false)
    }
  })
})
