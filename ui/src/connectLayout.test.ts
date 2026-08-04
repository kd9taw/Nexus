import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// CSS-text guards (same technique as styles-spacing.test.ts) for the Connect layout
// invariants: the rail must not clip silently, and the map insight overlay must mirror
// the established .map-path overlay contract.
const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')

describe('connect layout invariants', () => {
  it('the pane grid restacks to one column via [data-viewport=xs], not a zoom-blind @media', () => {
    expect(css).toMatch(/\[data-viewport='xs'\]\s*\.connect\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\)/)
    // No raw-px breakpoint may drive the Connect layout — that mis-fires at every UI zoom.
    expect(css).not.toMatch(/@media\s*\(max-width:\s*900px\)\s*\{\s*\/\*[^}]*bottom sheet/i)
  })

  it('the globe cell keeps a definite size (center minmax(0,1fr) + min-width:0, never runaway)', () => {
    // The map canvas is 100%-width; a bare 1fr column would let it grow unbounded. The
    // center column must be minmax(0,1fr), and .connect-map keeps the min-*:0 chain.
    expect(css).toMatch(/\.connect\s*\{[^}]*grid-template-columns:[^;]*minmax\(0, 1fr\)/)
    expect(css).toMatch(/\.connect-map\s*\{[^}]*min-width:\s*0/)
  })

  it('a CONNECT pane body scopes the wide gauge grid to 2 columns (no horizontal clip)', () => {
    // .connect-scoped deliberately: the pane-frame family is shared with the cockpit
    // pane grids, and this narrow-rail column-strip must not leak onto them.
    expect(css).toMatch(/\.connect\s+\.pane-body\s+\.swx-strip\s*\{[^}]*grid-template-columns:\s*repeat\(2/)
  })

  // ('a pane body declares a visible scrollbar affordance' lived here — a regex-PRESENCE
  //  match on `scrollbar-width: thin`, the exact form CLAUDE.md forbids: a dead selector
  //  passes it, and it said nothing at all about the property the box actually turns on,
  //  its `overflow`. Both are now cascade-COMPUTED for Connect's rail chain AND its bottom
  //  strip chain, alongside every cockpit host of the shared frame family, in
  //  cockpit-shells.test.ts — 'the shared pane body is the first legal fate of vertical
  //  deficit'.)

  it('the map insight overlay mirrors .map-path (right edge, absolute, z 3–5)', () => {
    const block = css.match(/\.map-insights\s*\{([^}]*)\}/)?.[1] ?? ''
    expect(block).toMatch(/position:\s*absolute/)
    expect(block).toMatch(/right:\s*var\(--space-3\)/)
    expect(block).toMatch(/z-index:\s*[345]\b/)
  })
})
