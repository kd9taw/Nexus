// The Satellites view must not be handed the DIGITAL (FT8) frequency control.
//
// Operator field report: "my dropdowns for frequencies are still showing FT8
// frequencies under satellite". TopBar's FrequencyControl is fed `bandPlan` from
// `get_band_plan` — the tier-aware FT8/FT1 watering holes — so on the Satellites
// view the top of the window offered 14.074 / 7.074 / … beside a bird on 435 MHz.
// Phone, CW, RTTY, SSTV and APRS each fixed this the same way: they own their own
// frequency surface, so they opt out of the top control. Satellites owns its own
// too (the transponder cards, the passband strip, the binding line).
//
// COMPUTED, not presence-matched: this extracts the actual membership of both
// opt-out lists from App.tsx and asserts on the SET. A test that merely grepped
// for the word 'sats' would pass on a comment.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const app = readFileSync(fileURLToPath(new URL('./App.tsx', import.meta.url)), 'utf8')

/** The views listed in a `<prop>={ effectiveView === 'a' || … }` opt-out. */
function optOutViews(prop: string): string[] {
  const start = app.indexOf(`${prop}={`)
  if (start < 0) throw new Error(`${prop} prop not found in App.tsx`)
  // Balance braces from the `{` that opens the expression. The scan STARTS on
  // that brace (depth 1 after it), so its closer is the `}` that brings the
  // depth back to 0 — checking depth before decrementing over-captured through
  // the rest of the component, letting one list answer for the other.
  let depth = 0
  let end = start + prop.length + 1
  for (let i = end; i < app.length; i++) {
    if (app[i] === '{') depth++
    else if (app[i] === '}') {
      depth--
      if (depth === 0) {
        end = i
        break
      }
    }
  }
  const expr = app.slice(start, end).replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '')
  return [...expr.matchAll(/effectiveView\s*===\s*'([^']+)'/g)].map((m) => m[1])
}

describe('the Satellites view owns its own frequency surface', () => {
  it("hides the TopBar FrequencyControl, like phone/cw and the free-running modes", () => {
    const views = optOutViews('hideFrequencyControl')
    expect(views).toContain('sats')
    // Guard the guard: the extraction must really be reading the list.
    expect(views).toEqual(expect.arrayContaining(['phone', 'cw', 'aprs']))
  })

  it('hides the digital chrome (tier tiles / slot clock / DT) too', () => {
    // Same reason RTTY/SSTV/APRS do: slot-sync furniture means nothing on a bird,
    // and the comment on that very list says the top control is fed the FT8 plan.
    const views = optOutViews('hideDigitalChrome')
    expect(views).toContain('sats')
    expect(views).toEqual(expect.arrayContaining(['rtty', 'sstv', 'aprs']))
  })
})
