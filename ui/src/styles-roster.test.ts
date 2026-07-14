import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Guards the v0.5.0 Call Roster overlap fix (tester report: the translucent Zone need-chip
// painted over the callsign, making it look blurry). The fix is per-cell containment:
// `.or-need` and `.or-call` must keep `min-width: 0` + `overflow: hidden` so excess chips
// clip inside their own grid track instead of bleeding across. That containment is what keeps
// header and data rows aligned (content can't push a track), so `.or-row`'s first (Need) column
// only needs a DEFINED MINIMUM width — a bare `px` track or a `minmax(<px>, …)` — never a
// content-sized track that could collapse or drift. (It's now `minmax(140px, 1.5fr)` so the Need
// column widens to fit all the need chips + the 💎 rarity pill.)
describe('styles.css call-roster overlap containment', () => {
  const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
  const block = (selector: string): string => {
    const m = css.match(new RegExp(`(?:^|\\n)\\${selector}\\s*\\{([^}]*)\\}`))
    expect(m, `${selector} rule block missing from styles.css`).toBeTruthy()
    return m![1]
  }

  it('.or-need clips its chips inside the Need column', () => {
    const b = block('.or-need')
    expect(b).toMatch(/min-width:\s*0/)
    expect(b).toMatch(/overflow:\s*hidden/)
  })

  it('.or-call cannot be painted over by a neighboring cell', () => {
    const b = block('.or-call')
    expect(b).toMatch(/min-width:\s*0/)
    expect(b).toMatch(/overflow:\s*hidden/)
  })

  it('.or-row gives the Need track a defined minimum width (header/data alignment)', () => {
    const b = block('.or-row')
    // First (Need) track must start with a px minimum — a bare `<px>` track OR `minmax(<px>, …)`.
    // Combined with the containment above, that keeps header and data rows aligned and prevents
    // the Need column from collapsing.
    expect(b).toMatch(/grid-template-columns:\s*(?:\d+px|minmax\(\s*\d+px)/)
  })
})
