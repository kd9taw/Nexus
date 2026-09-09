import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { parseRules, specificity, cmpSpec } from './cssCascade'

// CSS-text guards (same technique as styles-spacing.test.ts) for the Connect layout
// invariants: the rail must not clip silently, and the map insight overlay must mirror
// the established .map-path overlay contract.
const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')

const RULES = parseRules(css)

/** ⚠️ `parseRules` DOES NOT STRIP COMMENTS — it hands back the comment above a rule glued to
 *  that rule's selector, and a dangling tail (`'2026-07-31). * / .connect .pane-body
 *  .swx-strip'`) when the comment wraps. A matcher that did not strip them would quietly stop
 *  matching the moment somebody wrote a comment over a rule: the guard goes green while seeing
 *  nothing, which is the exact failure this file's style exists to prevent. The `.swx-strip`
 *  control below carries such a comment, so this stripping is under test rather than assumed. */
function cleanSelector(selector: string): string {
  return selector
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/^[\s\S]*\*\//, ' ')
    .trim()
}

/** Does `selector` match an element at the end of `chain`? Descendant combinators only —
 *  which is every selector these panes are styled by. The last compound must match the
 *  element itself; earlier compounds must match ancestors in order. */
function matchesChain(selector: string, chain: Array<Set<string>>): boolean {
  const sel = cleanSelector(selector)
  if (/[>+~,]/.test(sel)) return false
  const parts = sel.split(/\s+/)
  const classesOf = (c: string) => (c.match(/\.[\w-]+/g) ?? []).map((x) => x.slice(1))
  const ok = (c: string, have: Set<string>) => {
    if (/[#:[]/.test(c)) return false
    const cls = classesOf(c)
    return cls.length > 0 && cls.every((x) => have.has(x))
  }
  if (!ok(parts[parts.length - 1], chain[chain.length - 1])) return false
  let i = chain.length - 2
  for (let p = parts.length - 2; p >= 0; p--) {
    while (i >= 0 && !ok(parts[p], chain[i])) i--
    if (i < 0) return false
    i--
  }
  return true
}

/** The cascade WINNER of `prop` for the element at the end of `chain` — highest specificity,
 *  then latest in source order, exactly as the browser resolves it. */
function winningDecl(
  chain: Array<Set<string>>,
  prop: string,
): { value: string; selector: string } | null {
  let win: { value: string; selector: string; spec: readonly [number, number, number]; order: number } | null =
    null
  for (const r of RULES) {
    if (!matchesChain(r.selector, chain)) continue
    const d = r.decls.filter((x) => x.prop === prop).pop()
    if (!d) continue
    const spec = specificity(cleanSelector(r.selector))
    if (!win || cmpSpec(spec, win.spec) > 0 || (cmpSpec(spec, win.spec) === 0 && r.order >= win.order)) {
      win = { value: d.value, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

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

  // ⭐ COMPUTED, NOT PRESENCE-MATCHED. A regex that finds `.amp-grid { grid-template-columns:
  // … }` somewhere in the sheet says nothing about whether that rule WINS for an .amp-grid
  // inside a Connect pane body — a later or more specific rule silently taking over is the
  // dead-fix mechanism this file's deleted scrollbar test was removed for.
  it('the amplifier readout cannot widen a Connect pane (computed cascade winner)', () => {
    const chain: Array<Set<string>> = [
      new Set(['app']),
      new Set(['connect']),
      new Set(['pane-frame']),
      new Set(['pane-body']),
      new Set(['amp-grid']),
    ]
    const cols = winningDecl(chain, 'grid-template-columns')
    expect(cols, 'nothing decides the amplifier grid at all').not.toBeNull()
    // auto-fit + minmax: the readout adapts to the ~250px rail AND the wider bottom strip
    // with no viewport rule of its own, and the minmax floor is what stops a long value
    // pushing the track — and the frame — wider than the pane.
    expect(cols!.value).toMatch(/repeat\(\s*auto-fit\s*,\s*minmax\(/)
    // The cell itself must be allowed to shrink; a grid item's default min-width is `auto`,
    // which is exactly how a four-digit wattage widens a bounded column.
    const cellChain: Array<Set<string>> = [...chain, new Set(['amp-cell'])]
    expect(winningDecl(cellChain, 'min-width')?.value).toBe('0')
    expect(winningDecl([...cellChain, new Set(['amp-v'])], 'min-width')?.value).toBe('0')

    // POSITIVE CONTROL — the resolver is really walking this chain and really discriminating.
    // A class no rule mentions must come back null; if it did not, every assertion above
    // would pass on a sheet that decides nothing.
    expect(winningDecl([...chain.slice(0, 4), new Set(['amp-grid-nope'])], 'grid-template-columns'))
      .toBeNull()
    // …and the neighbouring gauge grid, which is NOT auto-fit, must not match the pattern —
    // proving the assertion above reads .amp-grid's own winner and not some ambient default.
    const swx = winningDecl([...chain.slice(0, 4), new Set(['swx-strip'])], 'grid-template-columns')
    expect(swx).not.toBeNull()
    expect(swx!.value).not.toMatch(/auto-fit/)
  })

  // ⭐ COMPUTED ACROSS THE [data-viewport] RULES, which `winningDecl` above cannot see: its
  // `ok()` rejects any compound containing `[`, so an attribute rule is silently skipped —
  // fine for the pane chains it arbitrates, useless for a grid whose competitors are exactly
  // `[data-viewport='sm'|'xs'] .connect`. The full-screen collapse has to beat those at every
  // viewport class, and it does it on SPECIFICITY (0,3,0 vs 0,2,0) rather than on source
  // order, so the resolver below scores attributes and classes together and fails loudly on
  // any selector shape it cannot judge — a skipped rule is how this check would go green
  // while the sheet restacked a full-screen map into six rows.
  describe('full screen collapses the Connect grid to one cell, at every viewport class', () => {
    // Comments stripped BEFORE parsing: `parseRules` splits its selector on commas, and a
    // prose comment above a rule (which it does not strip) therefore arrives as several
    // garbage "selectors" — exactly what fail-loud matching must not choke on.
    const CLEAN = parseRules(css.replace(/\/\*[\s\S]*?\*\//g, ' '))
    type El = { classes: string[]; attrs: Record<string, string> }

    function matchCompound(compound: string, el: El): boolean {
      const attrs = [...compound.matchAll(/\[([\w-]+)='([^']*)'\]/g)]
      const rest = compound.replace(/\[[^\]]*\]/g, '')
      const classes = [...rest.matchAll(/\.([\w-]+)/g)].map((m) => m[1])
      const leftover = rest.replace(/\.[\w-]+/g, '').trim()
      if (leftover !== '') throw new Error(`resolver cannot judge compound: ${compound}`)
      return (
        attrs.every(([, k, v]) => el.attrs[k] === v) && classes.every((c) => el.classes.includes(c))
      )
    }

    /** Descendant combinators only — every rule competing for this element uses them. */
    function applies(selector: string, chain: El[]): boolean {
      if (/[>+~]/.test(selector)) return false
      const parts = selector.split(' ').filter(Boolean)
      if (!matchCompound(parts[parts.length - 1], chain[chain.length - 1])) return false
      let i = chain.length - 2
      for (let p = parts.length - 2; p >= 0; p--) {
        while (i >= 0 && !matchCompound(parts[p], chain[i])) i--
        if (i < 0) return false
        i--
      }
      return true
    }

    /** The cascade winner for `prop` on the last element of `chain`: highest specificity,
     *  then latest in source order — the browser's own arbitration. */
    function winner(chain: El[], prop: string) {
      let win: { value: string; selector: string; spec: readonly [number, number, number]; order: number } | null =
        null
      for (const r of CLEAN) {
        // Only rules that could possibly target this element are judged — everything else
        // in a 4000-line sheet is noise, and shapes this resolver rejects (`:hover`, `>`)
        // are not competitors for a grid template on `.connect`.
        if (!/\.connect(\s|$|\.)/.test(r.selector)) continue
        const d = r.decls.filter((x) => x.prop === prop).pop()
        if (!d) continue
        if (!applies(r.selector, chain)) continue
        if (!win || cmpSpec(r.spec, win.spec) > 0 || (cmpSpec(r.spec, win.spec) === 0 && r.order >= win.order)) {
          win = { value: d.value, selector: r.selector, spec: r.spec, order: r.order }
        }
      }
      return win
    }

    const shell = (full: boolean): El => ({
      classes: full ? ['connect-shell', 'map-full'] : ['connect-shell'],
      attrs: {},
    })
    const chainAt = (vp: string, full: boolean): El[] => [
      { classes: ['app'], attrs: { 'data-viewport': vp } },
      shell(full),
      { classes: ['connect'], attrs: {} },
    ]

    it.each(['xl', 'lg', 'md', 'sm', 'xs'])('at %s the map cell is the only track', (vp) => {
      const rows = winner(chainAt(vp, true), 'grid-template-rows')
      expect(rows, 'nothing decides the full-screen grid rows at all').not.toBeNull()
      expect(rows!.value).toBe('minmax(0, 1fr)')
      expect(winner(chainAt(vp, true), 'grid-template-columns')!.value).toBe('minmax(0, 1fr)')
      // The one remaining child is the map cell; a stale area map would strand it.
      expect(winner(chainAt(vp, true), 'grid-template-areas')!.value).toBe("'center'")
      // A "full screen" map inside 12px of padding is not full screen.
      expect(winner(chainAt(vp, true), 'padding')!.value).toBe('0')
      expect(winner(chainAt(vp, true), 'gap')!.value).toBe('0')
    })

    it('POSITIVE CONTROL — without the class the viewport rules still win their layouts', () => {
      // The resolver really is seeing the [data-viewport] competitors: at xs the stacked
      // six-row template wins, at lg the two-rail one does. Without this, the assertions
      // above would pass on a resolver that had silently skipped every rival rule.
      expect(winner(chainAt('xs', false), 'grid-template-rows')!.value).toContain('280px')
      expect(winner(chainAt('lg', false), 'grid-template-rows')!.value).toBe(
        'minmax(0, 1fr) minmax(0, 1fr) auto',
      )
      expect(winner(chainAt('sm', false), 'grid-template-columns')!.value).toContain('--cn-rail')
    })

    it('POSITIVE CONTROL — the resolver refuses a shape it cannot judge', () => {
      // The fail-loud property itself. A rule this resolver quietly skipped would be a rule
      // that could be beating the collapse in a real browser.
      expect(() => matchCompound('#connect', { classes: ['connect'], attrs: {} })).toThrow()
    })
  })

  it('the map insight overlay mirrors .map-path (right edge, absolute, z 3–5)', () => {
    const block = css.match(/\.map-insights\s*\{([^}]*)\}/)?.[1] ?? ''
    expect(block).toMatch(/position:\s*absolute/)
    expect(block).toMatch(/right:\s*var\(--space-3\)/)
    expect(block).toMatch(/z-index:\s*[345]\b/)
  })
})
