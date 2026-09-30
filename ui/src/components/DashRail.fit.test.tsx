// @vitest-environment jsdom
//
// THE RAIL AT ITS 200 PX FLOOR, computed over the real sheets. jsdom lays nothing out, so these are the
// cascade winners (cssCascade.testkit) of the decisions that make the floor fit; the geometry itself is
// measured in Chrome (the real-browser census), which is where both defects below were found:
//   - the head: in German, Spanish and French, "⊞ Panels · 1 hidden" could not fit beside the title in
//     a 200 px rail, so the title stood one letter per line, the ✕ went past the window's right edge and
//     the page scrolled sideways (every language once a box was hidden and the label grew);
//   - Getting Out: its direction line kept its place beside the 108 px rose and ran past the box's
//     right edge, cut off.
// Space Wx is Connect's box and needs Connect's fit here too: its gauge strip is also a `.panel`, whose
// flex column comes later in the sheet than the strip's grid, so without a rule of the box's own the
// gauges stand one per row and the 30-day lines start under the box's fold (measured on Connect).
// The controls: a cockpit's ⊞ keeps its one-line label, and the direction line keeps a basis, so it
// drops under the rose only where it cannot stand beside it.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { PropagationSnapshot } from '../types'
import { css, loadSheets } from '../cssCascade.testkit'

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getGettingOut: vi.fn(async () => ({
      count: 2,
      maxKm: 3200,
      reports: [
        { call: 'K1ABC', octant: 'E', km: 1500, band: '20m', snr: -12 },
        { call: 'G4XYZ', octant: 'NE', km: 3200, band: '20m', snr: -18 },
      ],
    })),
    getBandOutlook: vi.fn(async () => ({ bands: [], mufNow: 0 })),
    getSpaceWxScales: vi.fn(async () => ({ scales: { r: 0, s: 0, g: 0 }, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getXrayNow: vi.fn(async () => ({ flux: 1e-7, asOf: 0 })),
    getDxpedWindows: vi.fn(async () => []),
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getPathOutlook: vi.fn(async () => null),
    getDxccEntityLocations: vi.fn(async () => []),
  }
})

import { DashRail } from './DashRail'
import { PanelsMenu } from './PanelsMenu'

const LIVE = {
  advisory: { headline: 'Bands are fair', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'live',
  asOf: Math.floor(Date.now() / 1000),
} as unknown as PropagationSnapshot

const mountRail = (prop: PropagationSnapshot = LIVE) =>
  render(
    <DashRail
      section="cw"
      myGrid="EN52"
      theme="dark"
      stations={[]}
      prop={prop}
      needByCall={new Map()}
      onHide={() => {}}
    />,
  )

beforeAll(() => loadSheets())
beforeEach(() => {
  localStorage.clear()
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('the rail’s head fits its 200 px floor in every language', () => {
  it('the ⊞ and ✕ travel together, and go under the title rather than past the rail', () => {
    mountRail()
    const head = document.querySelector<HTMLElement>('.dash-rail .dash-rail-head')!
    const menuBtn = head.querySelector<HTMLElement>('.panels-menu-btn')!
    const close = head.querySelector<HTMLElement>('.pane-close')!
    expect(css(head, 'flex-wrap'), 'the head is one line whatever it holds: its ✕ runs past the rail').toBe('wrap')
    const acts = head.lastElementChild as HTMLElement
    expect(acts.contains(menuBtn) && acts.contains(close), 'the ⊞ and ✕ can wrap apart, leaving ✕ alone on a line').toBe(true)
    expect(acts.contains(head.querySelector('.dash-rail-title')), 'the title is inside the controls’ group').toBe(false)
    expect(css(acts, 'margin-left'), 'on a line of their own the controls leave the rail’s right edge').toBe('auto')
  })

  it('the rail’s ⊞ label may take two lines; a cockpit’s ⊞ keeps its one', () => {
    mountRail()
    const railBtn = document.querySelector<HTMLElement>('.dash-rail .panels-menu-btn')!
    expect(css(railBtn, 'white-space'), '“⊞ Bereiche · 1 ausgeblendet” is wider than the rail and cannot wrap').toBe('normal')
    // A label that may wrap still takes its one-line width while the menu around it refuses to shrink
    // (`.panels-menu { flex: 0 0 auto }`): Chrome measured the German button 209 px wide in a 175 px row.
    expect(css(railBtn.closest('.panels-menu')!, 'flex-shrink'), 'the menu keeps the label’s one-line width').toBe('1')
    cleanup()
    // THE CONTROL: the same menu in a cockpit's header is untouched.
    render(
      <div className="cockpit-header">
        <PanelsMenu items={[{ id: 'a', label: 'A', state: 'removed' }]} onToggle={() => {}} onUndo={() => {}} canUndo={false} onReset={() => {}} />
      </div>,
    )
    expect(css(document.querySelector('.panels-menu-btn')!, 'white-space')).toBe('nowrap')
    expect(css(document.querySelector('.panels-menu')!, 'flex-shrink')).toBe('0')
  })
})

describe('Getting Out at the rail’s floor', () => {
  it('the direction line drops under the rose where it cannot stand beside it, and keeps a basis where it can', async () => {
    mountRail()
    const wrap = await vi.waitFor(() => {
      const el = document.querySelector<HTMLElement>('.dash-rail .getout-rose-wrap')
      if (!el) throw new Error('the Getting Out box has not drawn its rose yet')
      return el
    })
    const dir = wrap.querySelector<HTMLElement>('.getout-dir')!
    expect(css(wrap, 'flex-wrap'), 'the rose and its line stay on one row and the line runs past the box').toBe('wrap')
    // A basis, not auto: with `auto` the line's basis is its whole sentence, so it would wrap under the
    // rose at EVERY width, in Connect's wide boxes too.
    expect(css(dir, 'flex-basis'), 'the line has no basis of its own').toBe('8em')
    expect(css(dir, 'flex-grow'), 'beside the rose the line no longer fills the row').toBe('1')
  })
})

describe('Space Wx at the rail’s floor', () => {
  it('the gauges stand two to a row, as in a Connect box, and the solar-wind speed’s unit may drop under its value', () => {
    const windy = {
      ...LIVE,
      spaceWx: { ...LIVE.spaceWx, solarWind: { bzNt: -3.4, btNt: 6.1, speedKms: 487, density: 5.2, timeUnix: Math.floor(Date.now() / 1000) } },
    } as PropagationSnapshot
    mountRail(windy)
    const strip = document.querySelector<HTMLElement>('.dash-rail .swx-strip')
    expect(strip, 'control: the rail draws the Space Wx gauges').not.toBeNull()
    expect(css(strip!, 'display'), 'the panel’s column wins: one gauge per row').toBe('grid')
    expect(css(strip!, 'grid-template-columns')).toBe('repeat(2, 1fr)')
    const unit = strip!.querySelector<HTMLElement>('.swx-vu')
    expect(unit, 'control: the solar-wind speed is drawn with its unit').not.toBeNull()
    // Two to a row at the 200 px floor, a column is ~77 px: the speed and its unit do not fit side by side.
    expect(css(unit!, 'flex-wrap'), 'the unit runs past its column’s edge').toBe('wrap')
    expect(css(unit!, 'justify-content')).toBe('flex-end')
  })
})
