import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Guards the Workspace-scaling report fixes (2026-07):
//
// 1. Settings tab rail must be reachable at ANY zoom/resolution/aspect combo —
//    tabs WRAP as the effective width shrinks (65% zoom floor = content 154% of
//    the window; high OS display scaling; narrow windows), the rail is never
//    clipped (`overflow: hidden` would silently amputate whole settings
//    sections), and the column flex can't squash it (`flex-shrink: 0`); the
//    content pane absorbs all the vertical squeeze and scrolls instead.
//
// 2. The Density control (Comfortable/Compact) must actually be consumed by the
//    data rows. It shipped as a dead switch: data-density → --density-scale →
//    row tokens that NOTHING referenced, so toggling it changed zero pixels
//    (part of the "scaling settings are not visually doing anything" report).
describe('styles.css settings tab-rail resilience', () => {
  const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
  const block = (selector: string): string => {
    const m = css.match(new RegExp(`(?:^|\\n)\\${selector}\\s*\\{([^}]*)\\}`))
    expect(m, `${selector} rule block missing from styles.css`).toBeTruthy()
    // Strip comments so prose (e.g. "never overflow:hidden here") can't trip
    // the negative assertions — match DECLARATIONS only.
    return m![1].replace(/\/\*[\s\S]*?\*\//g, '')
  }

  it('.settings-tabs wraps and is never clipped or squashed', () => {
    const b = block('.settings-tabs')
    expect(b).toMatch(/flex-wrap:\s*wrap/)
    expect(b).toMatch(/flex-shrink:\s*0/)
    expect(b).not.toMatch(/overflow(?:-[xy])?:\s*hidden/)
  })

  it('.settings-scroll absorbs the vertical squeeze and scrolls', () => {
    const b = block('.settings-scroll')
    expect(b).toMatch(/overflow-y:\s*auto/)
    expect(b).toMatch(/min-height:\s*0/)
    expect(b).toMatch(/flex:\s*1/)
  })

  // BUG (operator screenshot): the Manual UI-scale strip has 11 chips (65…175%).
  // .theme-switcher is the rounded "pill" container; without wrapping, a nowrap
  // flex row overflows and chips past 110% escape the pill to the right —
  // unstyled and un-clickable. The pill background/border live on THIS container,
  // so wrapping is what keeps every chip inside it. Never let children overflow.
  it('.theme-switcher wraps its chips inside the pill (never overflow-escape)', () => {
    const b = block('.theme-switcher')
    expect(b).toMatch(/flex-wrap:\s*wrap/)
    // The rounded background/border must be on the wrapping container itself so it
    // grows to enclose every wrapped row (not a fixed-width element chips escape).
    expect(b).toMatch(/border-radius:\s*999px/)
    expect(b).toMatch(/background:/)
    // A nowrap escape hatch here would re-introduce the overflow bug.
    expect(b).not.toMatch(/flex-wrap:\s*nowrap/)
  })
})

describe('styles.css density is actually consumed by data rows', () => {
  const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
  const block = (selector: string): string => {
    const m = css.match(new RegExp(`(?:^|\\n)\\${selector}\\s*\\{([^}]*)\\}`))
    expect(m, `${selector} rule block missing from styles.css`).toBeTruthy()
    return m![1]
  }

  it.each(['.decode-row', '.or-row', '.log-row'])(
    '%s vertical padding rides --density-scale',
    (sel) => {
      expect(block(sel)).toMatch(/padding:\s*calc\([^)]*var\(--density-scale\)/)
    },
  )

  it('the three density levels set distinct --density-scale values', () => {
    for (const level of ['guided', 'standard', 'dense']) {
      const m = css.match(
        new RegExp(`\\[data-density='${level}'\\]\\s*\\{([^}]*)\\}`),
      )
      expect(m, `[data-density='${level}'] block missing`).toBeTruthy()
      expect(m![1]).toMatch(/--density-scale:\s*[\d.]+/)
    }
  })
})
