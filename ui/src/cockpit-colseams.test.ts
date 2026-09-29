// @vitest-environment jsdom
//
// A GRID COCKPIT'S COLUMN DIVIDER SITS ON THE GAP BEFORE ITS TRACK (layout L2), computed over BOTH
// sheets. The divider element carries the shared `.pane-splitter` classes as well as its own
// `.cockpit-colseam` placement, and styles.css sizes `.pane-splitter` (8 px wide, −4 px margins,
// in-flow). The placement is only real if cockpit-panes.css WINS every property that places it —
// so this resolves the winner through the real selector engine (cssCascade.testkit), with the
// class chain the cockpits render, for each grid cockpit, both bounded tiers and both kinds of
// divider. jsdom lays nothing out: the real-browser measurement of where it lands is the
// harness's (every divider's rect inside the gap, at 1024/1366/1920 and pinned 175 %).
import { describe, it, expect, beforeAll } from 'vitest'
import { loadSheets, css, pxOf } from './cssCascade.testkit'

beforeAll(() => loadSheets())

/** The split divider (three columns) and the width divider, as PaneSeam renders their classes. */
const DIVIDERS = {
  split: (at: number) => `pane-splitter col-seam seam cockpit-colseam cockpit-colseam-${at}`,
  width: (at: number) => `pane-splitter cockpit-colseam cockpit-colseam-${at}`,
}

function mount(shell: string, cols: 2 | 3, divider: string, viewport = 'lg') {
  document.documentElement.setAttribute('data-viewport', viewport)
  document.body.innerHTML =
    `<div class="app"><div class="shell"><main class="layout single ${shell}">` +
    `<div class="cockpit-panes" data-cols="${cols}" data-flow="fill">` +
    '<div class="cockpit-col"></div><div class="cockpit-col"></div>' +
    `<div class="${divider}" role="separator"></div>` +
    '</div></main></div></div>'
  return {
    region: document.querySelector('.cockpit-panes')!,
    seam: document.querySelector('[role=separator]')!,
  }
}

/** `calc(<a>px / <b> - <c>px)`, the one shape the placement's `left` is written in. */
function calcPx(v: string): number {
  const m = /^calc\((-?[\d.]+)px \/ (-?[\d.]+) - ([\d.]+)px\)$/.exec(v.replace(/\s+/g, ' ').trim())
  expect(m, `the divider's \`left\` is \`${v}\`, not the calc() this test evaluates`).not.toBeNull()
  return Number(m![1]) / Number(m![2]) - Number(m![3])
}

const CASES: Array<[string, 2 | 3, keyof typeof DIVIDERS, number]> = [
  ['phone-cockpit', 2, 'width', 2],
  ['phone-cockpit', 3, 'split', 2],
  ['phone-cockpit', 3, 'width', 3],
  ['cw-cockpit', 2, 'width', 2],
  ['cw-cockpit', 3, 'split', 2],
  ['cw-cockpit', 3, 'width', 3],
  ['js8-cockpit', 2, 'width', 2],
  ['js8-cockpit', 3, 'split', 2],
  ['js8-cockpit', 3, 'width', 3],
]

describe('a column divider is taken out of the flow and laid over the gap before its track', () => {
  for (const [shell, cols, kind, at] of CASES) {
    it(`.${shell} · ${cols} columns · the ${kind} divider on track ${at}`, () => {
      const { region, seam } = mount(shell, cols, DIVIDERS[kind](at))
      expect(css(region, 'position'), 'the region is not the dividers’ containing block').toBe('relative')
      expect(css(seam, 'position'), 'an in-flow divider takes a track or a cell of its own').toBe('absolute')
      expect(css(seam, 'grid-column'), 'the divider is not placed on its track').toBe(`${at} / ${at + 1}`)
      expect([css(seam, 'top'), css(seam, 'bottom')], 'the divider does not run the region’s height').toEqual(['0px', '0px'])
      // styles.css `.pane-splitter { margin: 0 -4px }` must lose, or the divider drifts off the gap.
      expect([pxOf(seam, 'margin-left'), pxOf(seam, 'margin-right')]).toEqual([0, 0])
      const width = pxOf(seam, 'width')
      const gap = pxOf(region, 'gap')
      const left = calcPx(css(seam, 'left')!)
      // Exactly the gap: from the end of the track before it to the start of its own.
      expect(width, `the divider is ${width} px over a ${gap} px gap`).toBe(gap)
      expect(left, `the divider starts ${left} px from its track, over a ${gap} px gap`).toBe(-gap)
      expect(css(seam, 'display'), 'hidden at a tier where the region is divided').not.toBe('none')
    })
  }

  it('control: the narrow window hides every divider the collapse hides (sm)', () => {
    const { seam } = mount('phone-cockpit', 2, DIVIDERS.width(2), 'sm')
    expect(css(seam, 'display')).toBe('none')
  })
})
