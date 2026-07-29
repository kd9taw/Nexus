import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// CSS-text guards (same technique as connectLayout.test.ts / styles-spacing.test.ts) for the
// APRS section's layout invariants.
//
// THE BUG THIS EXISTS FOR, reported on the 0.20.3 test build: the APRS map "just keeps slowly
// expanding infinitely down". `.layout.single` is `display:block; overflow-y:auto`, so a child
// has NO definite height to resolve against. MapView sizes its canvas from the measured
// container; the taller canvas makes the container taller; the next measurement is larger
// again. A feedback loop, not a sizing mistake — which is why no min/max-height on the map
// itself fixes it. The ANCESTOR has to be bounded, exactly as Connect already does.
//
// Comments are stripped before matching: a comment that merely DESCRIBES a rule (this file is
// full of them) would otherwise satisfy the assertions and the guard would pass on prose.
const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  '',
)

describe('APRS layout invariants', () => {
  it('the APRS view flex-FILLS its .single layout instead of block-scrolling', () => {
    // Without this the map has no definite height and grows without bound.
    expect(css).toMatch(/\.layout\.single:has\(>\s*\.aprs-body\)/)
    const rule = /\.layout\.single:has\(>\s*\.aprs-body\)[^{]*\{([^}]*)\}/.exec(css)
    expect(rule, 'the :has(> .aprs-body) rule must exist').not.toBeNull()
    expect(rule![1]).toMatch(/display:\s*flex/)
    expect(rule![1]).toMatch(/flex-direction:\s*column/)
  })

  it('the map cell can shrink: bounded row track and no hard min-height floor', () => {
    const body = /\.aprs-body\s*\{([^}]*)\}/.exec(css)
    expect(body, '.aprs-body rule must exist').not.toBeNull()
    // A bare `1fr` row keeps a min-content floor, which is the other way this overflows.
    expect(body![1]).toMatch(/grid-template-rows:\s*minmax\(0, 1fr\)/)
    expect(body![1]).toMatch(/min-height:\s*0/)

    // A hard px floor on the map would push the grid past its parent on a short window —
    // the additive-floor clipping this codebase has already been bitten by (see
    // cockpit-floors.test.ts). `min(320px, 100%)` floors only when there is room.
    const map = /\.aprs-map\s*\{([^}]*)\}/.exec(css)
    expect(map, '.aprs-map rule must exist').not.toBeNull()
    expect(map![1]).not.toMatch(/min-height:\s*[\d.]+px/)
  })

  it('the rail scrolls internally rather than forcing the section wide or tall', () => {
    const rail = /\.aprs-rail\s*\{([^}]*)\}/.exec(css)
    expect(rail, '.aprs-rail rule must exist').not.toBeNull()
    expect(rail![1]).toMatch(/min-height:\s*0/)
    expect(rail![1]).toMatch(/overflow-y:\s*auto/)
  })

  it('the map canvas is OUT OF FLOW so its device-px size cannot grow the page', () => {
    // The 0.20.4 bug: canvas.height is set to h * devicePixelRatio. In flow that
    // attribute is the canvas's intrinsic size, so on a display scaled above 100%
    // (dpr > 1) it exceeds the box that measured it -> parent grows -> the
    // ResizeObserver re-fires larger -> geometric runaway. dpr == 1 hides it
    // entirely, which is exactly why it reached an operator. `.waterfall-canvas`
    // already carries this fix and documents the same reasoning.
    const rule = /\.map-canvas-wrap\s*>\s*canvas\s*\{([^}]*)\}/.exec(css)
    expect(rule, '.map-canvas-wrap > canvas rule must exist').not.toBeNull()
    expect(rule![1]).toMatch(/position:\s*absolute/)
    expect(rule![1]).toMatch(/inset:\s*0/)
    // And the wrap must be able to shrink, or an in-flow child grows it instead.
    const wrap = /\.map-canvas-wrap\s*\{([^}]*)\}/.exec(css)
    expect(wrap![1]).toMatch(/min-height:\s*0/)
  })

  it('restacks on narrow via [data-viewport], never a zoom-blind @media', () => {
    // UI zoom lives on the non-root `.app`, so a raw max-width query fires against the
    // UNZOOMED width and mis-fires at every zoom level.
    expect(css).toMatch(/\[data-viewport='(narrow|phone)'\]\s*\.aprs-body/)
  })
})
