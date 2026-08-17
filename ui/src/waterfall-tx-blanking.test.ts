// THE TX DARK BAND IS AN FT-SURFACE BEHAVIOR, NOT A WATERFALL BEHAVIOR (field reports,
// 2026-08-17: "the waterfall stops").
//
// One `Waterfall` component draws five surfaces. While keyed it zeroes the row so the display
// is honest about having no receiver — which is right for FT8/FT4, where an over is 13 seconds
// and the black band reads as "that was us". It is wrong everywhere else: an RTTY over has no
// fixed length and SSTV Scottie DX is ~4.5 MINUTES, and a waterfall that scrolls solid black
// for minutes is indistinguishable from one that has died. Operators reported exactly that.
// So the blanking moved behind an opt-in prop that defaults OFF, and this file pins WHICH
// surfaces opt in — the one thing about the fix that can silently regress, because a new
// caller gets the safe default and a copied-in `txBlanks` gets the dark band.
//
// ⚠️ WHAT THIS FILE CANNOT PROVE, stated rather than implied. jsdom has no 2D canvas
// (`getContext('2d')` returns null and the render effect bails before drawing), so nothing
// here paints or reads a pixel — the blanking itself is not observable in this environment.
// This is a CENSUS over the call sites and the fill site, the same guard-test idiom as
// `display-liveliness.test.ts`: it pins the wiring and the pairing, not the picture.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'

/** Source with comments removed, so prose ABOUT `txBlanks` can never read as passing it —
 *  the RTTY and SSTV call sites both carry a comment saying not to add the prop. */
function sourceOf(rel: string): string {
  return readFileSync(new URL(rel, import.meta.url), 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '') // block + JSX comments
    .replace(/(^|[^:])\/\/.*$/gm, '$1') // line comments, sparing `https://`
}

/** Every `<Waterfall … />` element in `src`, as the raw attribute text of each. */
function waterfallCallSites(src: string): string[] {
  const out: string[] = []
  for (let i = src.indexOf('<Waterfall'); i >= 0; i = src.indexOf('<Waterfall', i + 1)) {
    // Scan to this element's own `/>`, ignoring anything nested inside a `{…}` expression.
    let depth = 0
    for (let j = i; j < src.length; j++) {
      const c = src[j]
      if (c === '{') depth++
      else if (c === '}') depth--
      else if (depth === 0 && c === '/' && src[j + 1] === '>') {
        out.push(src.slice(i, j))
        break
      }
    }
  }
  return out
}

/** Every surface that mounts a Waterfall, and whether its over is short enough to go dark.
 *  A new caller must be classified HERE — that is the point of the exhaustiveness check. */
const SURFACES: { file: string; blanks: boolean; why: string }[] = [
  { file: './App.tsx', blanks: true, why: 'the Operate right-rail FT waterfall — 13 s overs' },
  { file: './DetachedPanel.tsx', blanks: true, why: 'the same FT waterfall, torn off' },
  { file: './components/OperateCockpit.tsx', blanks: true, why: 'the FT cockpit — 13 s overs' },
  {
    file: './components/RttyCockpit.tsx',
    blanks: false,
    why: 'an RTTY over has no fixed length and a latched one runs to the 10-minute ceiling',
  },
  {
    file: './components/SstvView.tsx',
    blanks: false,
    why: 'Scottie DX is ~4.5 minutes of continuous keying',
  },
]

describe('the TX dark band reaches the FT surfaces and only the FT surfaces', () => {
  it.each(SURFACES)('$file — $why', ({ file, blanks }) => {
    const sites = waterfallCallSites(sourceOf(file))
    expect(sites.length, `no <Waterfall> found in ${file} — did the call site move?`).toBeGreaterThan(
      0,
    )
    for (const site of sites) {
      expect(
        /\btxBlanks\b/.test(site),
        blanks
          ? `${file} must pass txBlanks — its over is short enough for a dark band to read as "that was us"`
          : `${file} must NOT pass txBlanks — a black band for the length of ITS over reads as a dead waterfall`,
      ).toBe(blanks)
    }
  })

  it('every surface that mounts a Waterfall is classified above', () => {
    // Without this, a sixth cockpit could mount one and inherit whichever default happened to
    // be in force, which is precisely how the shared component grew a behavior nobody chose
    // for four of the five surfaces it draws.
    const known = new Set(SURFACES.map((s) => s.file))
    const searched = [
      './App.tsx',
      './DetachedPanel.tsx',
      './components/OperateCockpit.tsx',
      './components/RttyCockpit.tsx',
      './components/SstvView.tsx',
      './components/PhoneCockpit.tsx',
      './components/CwCockpit.tsx',
      './components/AprsCockpit.tsx',
    ]
    for (const file of searched) {
      let src: string
      try {
        src = sourceOf(file)
      } catch {
        continue // the cockpit set moves; a missing file is not a failure of this rule
      }
      if (waterfallCallSites(src).length > 0) {
        expect(known.has(file), `${file} mounts a Waterfall but is not classified in SURFACES`).toBe(
          true,
        )
      }
    }
  })

  it('defaults to OFF, so a new caller cannot inherit the dark band by omission', () => {
    const src = sourceOf('./components/Waterfall.tsx')
    expect(src).toMatch(/txBlanks\s*=\s*false/)
  })

  it('the fill and the AGC freeze stay on ONE condition', () => {
    // Splitting them is how the post-TX red band comes back: the freeze is what stops the
    // zeroed row from dragging the EMA to digital silence, so a surface that does not blank
    // must not freeze, and a surface that blanks must do both. Pinned structurally because
    // the pixels are not observable here.
    const src = sourceOf('./components/Waterfall.tsx')
    expect(src, 'the blanking condition should be computed once and reused').toMatch(
      /const blanking = txBlanksRef\.current && txRef\.current/,
    )
    expect(src, 'the row fill must read that condition').toMatch(/if \(blanking\) frow\.fill\(0\)/)
    expect(src, 'the AGC freeze must read the SAME condition, not `txRef` on its own').toMatch(
      /if \(!blanking\) \{/,
    )
  })
})
