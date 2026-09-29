// @vitest-environment jsdom
//
// THE TWO CONSTANTS BEHIND THE TEMPO RAIL PAIR, pinned to the sheet (layout L2, from L1's gap).
//
// usePaneWidths.fitRails keeps the conversation between the two rails at least CENTER_MIN wide by
// sharing `effective width − RAIL_CHROME − CENTER_MIN` between them (and index.html's preseed carries
// the same two numbers). Both were measured in Chrome and written down as numbers, which means the
// guarantee rests on the sheet still agreeing with them — and nothing checked that it did:
//   · RAIL_CHROME is everything across the window that is neither rail nor conversation. It was
//     first written as 161 (the navigation rail's border counted on top of its 88 px, where the
//     sheet sizes every box border-box). Widen the rail, the layout's padding or its gap, or give a
//     divider a width of its own, and the pair's fit silently leaves the conversation less than
//     CENTER_MIN — the 0 px conversation L1-1 fixed, coming back by a few px at a time.
//   · CENTER_MIN is only the conversation's floor if the grid renders the widths the hook fits: no
//     sheet floor on the conversation's own track or cell above it, and no sheet floor on a rail
//     above the hook's own (a rail the grid draws wider than the hook fitted takes the difference
//     out of the conversation).
// So each is recomputed here from the rules that decide it, with the real class chain App renders
// (App.tsx `threePane`), at every viewport tier where the rails sit side by side (md and up; sm/xs
// stack the three panes in one column, where neither constant is read by the layout).
//
// jsdom lays nothing out: these read the CASCADE (cssCascade.testkit — the real selector engine,
// importance → specificity → source order), not the screen. What the screen does with them was
// measured in Chrome by L1 (the three panes share exactly effective width − 160 at 1366×768 and
// 1920×1080; End on both rails at 1920 leaves the conversation exactly 360 px).
import { describe, it, expect, beforeAll, beforeEach } from 'vitest'
import { loadSheets, css, pxOf, atToken } from './cssCascade.testkit'
import { CENTER_MIN, LEFT_MIN, RAIL_CHROME, RIGHT_MIN } from './usePaneWidths'

beforeAll(() => loadSheets())

/** App's shell around the Tempo three-pane workspace, with its five grid children in order. */
function mountThreePane(viewport: string) {
  document.documentElement.setAttribute('data-viewport', viewport)
  document.body.innerHTML =
    '<div id="root"><div class="app"><div class="shell">' +
    '<nav class="mode-nav"></nav>' +
    '<main class="layout" data-three-pane>' +
    '<div class="grid-stations"></div>' +
    '<div class="pane-splitter left" role="separator"></div>' +
    '<div class="grid-center"></div>' +
    '<div class="pane-splitter right" role="separator"></div>' +
    '<div class="grid-waterfall"></div>' +
    '</main></div></div></div>'
  const q = (sel: string) => document.querySelector(sel)!
  return {
    nav: q('.mode-nav'),
    main: q('main.layout'),
    splitters: [q('.pane-splitter.left'), q('.pane-splitter.right')],
    center: q('.grid-center'),
    chain: [q('.app'), q('.shell'), q('main.layout')],
  }
}

/** Top-level tracks of a resolved grid-template-columns value (commas and spaces inside
 *  `minmax(…)` / `min(…)` stay with their track). */
function tracks(template: string): string[] {
  const out: string[] = []
  let depth = 0
  let buf = ''
  for (const ch of template.replace(/\s+/g, ' ').trim()) {
    if (ch === '(') depth++
    else if (ch === ')') depth--
    if (ch === ' ' && depth === 0) {
      if (buf) out.push(buf)
      buf = ''
    } else buf += ch
  }
  if (buf) out.push(buf)
  return out
}

/** The template as the browser resolves it at this tier: `--rail-min` is declared on the layout
 *  itself where a tier overrides it (sm/xs do, which is the positive control below), and the
 *  testkit substitutes only `:root` tokens, so an element-scoped value is folded in here. */
function templateAt(main: Element): string[] {
  const railMin = css(main, '--rail-min')
  const read = () => css(main, 'grid-template-columns') ?? ''
  return tracks(railMin ? atToken('--rail-min', railMin, read) : read())
}

const SIDE_BY_SIDE = ['md', 'lg', 'xl'] as const

beforeEach(() => {
  document.body.innerHTML = ''
})

describe('RAIL_CHROME is what the sheet puts across the window besides the rails and the conversation', () => {
  for (const vp of SIDE_BY_SIDE) {
    it(`[data-viewport='${vp}']: rail + padding + gaps + dividers = ${RAIL_CHROME} px`, () => {
      const { nav, main, splitters } = mountThreePane(vp)
      // The navigation rail, border included: every box is border-box (styles.css `*`).
      expect(css(nav, 'box-sizing'), 'the rail is no longer border-box: its border adds to its width').toBe('border-box')
      const rail = pxOf(nav, 'width')
      const pad = pxOf(main, 'padding-left') + pxOf(main, 'padding-right')
      const gap = pxOf(main, 'gap')
      const n = templateAt(main).length
      // Each divider rides an `auto` track, sized by its margin box: 8 px wide, −4 px either side.
      const dividers = splitters.reduce(
        (sum, s) => sum + pxOf(s, 'width') + pxOf(s, 'margin-left') + pxOf(s, 'margin-right'),
        0,
      )
      const chrome = rail + pad + (n - 1) * gap + dividers
      expect(
        chrome,
        `at ${vp} the sheet puts ${chrome} px across the window besides the rails and the conversation ` +
          `(rail ${rail} + padding ${pad} + ${n - 1} gaps of ${gap} + dividers ${dividers}), but ` +
          `usePaneWidths.RAIL_CHROME is ${RAIL_CHROME}: the pair's fit now leaves the conversation ` +
          `${RAIL_CHROME - chrome} px off CENTER_MIN. Re-measure in Chrome and change RAIL_CHROME ` +
          'AND index.html\'s preseed together (index-preseed.test.ts holds them to each other).',
      ).toBe(RAIL_CHROME)
    })
  }

  it('control: the dividers really are two `auto` tracks between the rails and the conversation', () => {
    // If a divider moved off its `auto` track, its margin box would no longer be its track's width
    // and the sum above would be counting the wrong thing.
    const t = templateAt(mountThreePane('lg').main)
    expect(t.length, `the three-pane template is no longer five tracks: ${t.join(' | ')}`).toBe(5)
    expect([t[1], t[3]]).toEqual(['auto', 'auto'])
  })

  it('control: at sm the panes stack in one column (where RAIL_CHROME is not read by the layout)', () => {
    // The tier the constant does NOT speak for — and the positive control for templateAt's
    // element-scoped --rail-min read (sm declares one of its own).
    const { main, nav } = mountThreePane('sm')
    expect(css(main, '--rail-min'), 'the sm rail floor the template read relies on is gone').toBe('190px')
    expect(templateAt(main)).toEqual(['1fr'])
    expect(css(nav, 'width')).toBe('100%')
  })
})

describe(`CENTER_MIN (${CENTER_MIN} px) is the conversation's only floor, and the grid draws the widths the hook fits`, () => {
  for (const vp of SIDE_BY_SIDE) {
    it(`[data-viewport='${vp}']: the conversation's track and cell carry no floor of their own`, () => {
      const { main, center } = mountThreePane(vp)
      const t = templateAt(main)
      const m = /^minmax\(\s*([^,]+),\s*(.+)\)$/.exec(t[2])
      expect(m, `the conversation's track is \`${t[2]}\`, not a minmax() with a floor to read`).not.toBeNull()
      expect(
        parseFloat(m![1]),
        `the conversation's track floors at ${m![1]}: above 0 the grid, not CENTER_MIN, decides how narrow ` +
          'it gets, and a floor above CENTER_MIN pushes the rails off the window',
      ).toBe(0)
      expect(
        pxOf(center, 'min-width'),
        'the conversation cell has a min-width: its content can then widen it past its track',
      ).toBe(0)
    })

    it(`[data-viewport='${vp}']: each rail track draws the published width, floored no higher than the hook's floor`, () => {
      const { main, chain } = mountThreePane(vp)
      // The hook publishes the widths inline on <html>; a rule between <html> and the layout that
      // declared them would replace them before the grid read them.
      for (const el of chain) {
        for (const v of ['--left-rail-w', '--right-rail-w']) {
          expect(css(el, v), `.${el.className.split(' ')[0]} redeclares ${v} at ${vp}: the hook's width never reaches the grid`).toBeNull()
        }
      }
      // The tracks follow the published widths: move each and the track moves with it.
      const at = (name: string, px: string) => atToken(name, px, () => templateAt(main))
      expect(at('--left-rail-w', '777px')[0]).toMatch(/,\s*777px\)$/)
      expect(at('--right-rail-w', '555px')[4]).toBe('555px')
      // The stations rail's sheet floor (`--rail-min`) must not exceed the hook's own floor, or the
      // grid draws that rail wider than the hook fitted it and the conversation pays.
      const lf = /^minmax\(\s*([\d.]+)px,/.exec(templateAt(main)[0])
      expect(lf, `the stations rail track is \`${templateAt(main)[0]}\`: no px floor to compare`).not.toBeNull()
      expect(
        Number(lf![1]),
        `the stations rail's sheet floor is ${lf![1]} px, above LEFT_MIN (${LEFT_MIN}): the grid draws it wider than ` +
          'usePaneWidths fits it and the conversation loses the difference below CENTER_MIN',
      ).toBeLessThanOrEqual(LEFT_MIN)
      // The waterfall rail's track is the published width alone: its floor is not raised above
      // RIGHT_MIN and a wide one is not capped.
      expect(at('--right-rail-w', `${RIGHT_MIN}px`)[4]).toBe(`${RIGHT_MIN}px`)
      expect(at('--right-rail-w', '5000px')[4]).toBe('5000px')
    })
  }
})
