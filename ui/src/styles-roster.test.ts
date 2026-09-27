import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Guards the v0.5.0 Call Roster overlap fix (tester report: the translucent Zone need-chip
// painted over the callsign, making it look blurry). The fix is per-cell containment:
// `.or-need` and `.or-call` must keep `min-width: 0` + `overflow: hidden` so excess chips
// clip inside their own grid track instead of bleeding across. That containment is what keeps
// header and data rows aligned (content can't push a track), so `.or-row`'s first (Call) column
// only needs a DEFINED MINIMUM width — a bare `px` track or a `minmax(<px>, …)` — never a
// content-sized track that could collapse or drift. (Call leads the row now; the Need column that
// follows it is the widest track so it fits all the need chips + the 💎 rarity pill.)
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

  it('.or-row gives the first track a defined minimum width (header/data alignment)', () => {
    const b = block('.or-row')
    // First (Call) track must start with a px minimum — a bare `<px>` track OR `minmax(<px>, …)`,
    // either of which may be written scaled by the root's text size (`calc(<px> * var(--text-scale))`,
    // #215): that is the same number in every row, so it is still content-independent. Combined
    // with the containment above, that keeps header and data rows aligned and prevents the first
    // column from collapsing.
    expect(b).toMatch(
      /grid-template-columns:\s*(?:minmax\(\s*)?(?:\d+px|calc\(\s*\d+px\s*\*\s*var\(--text-scale\)\s*\))/,
    )
  })

  it('.or-row: the Call floor holds a worked call with its marks AND the QRZ link', () => {
    // The 88px floor (2026-07-13) was sized for "VE2OPR-length calls + the B4/L marks". The ↗
    // QRZ link joined the same cell a week later (2026-07-20) and the floor never moved, so at
    // the 1024×768 floor — where the column sits ON its floor — `.or-call`'s overflow:hidden
    // cut the link off every worked station (29px past the edge, measured in Chrome at Normal
    // text). MEASURED, not reasoned: the widest call cell over every roster row — a six-character
    // worked call with B4, L and ↗ (`JH7NOP B4 L ↗`) — is 121.5px at Normal text in Chrome at
    // zoom 1 (130.2 at Large, 139.6 at Larger). The floor scales with Text size along with the
    // cell's contents, so this is checked at Normal.
    const MEASURED_CALL_CELL = 122
    const first = /grid-template-columns:\s*minmax\(\s*(?:calc\(\s*)?(\d+(?:\.\d+)?)px/.exec(block('.or-row'))
    expect(first, 'the Call track has no px floor to read').not.toBeNull()
    expect(
      Number(first![1]),
      `the Call floor (${first![1]}px) is under what a worked call and its QRZ link need ` +
        `(${MEASURED_CALL_CELL}px) — the ↗ is cut off at the 1024×768 floor`,
    ).toBeGreaterThanOrEqual(MEASURED_CALL_CELL)
  })
})
