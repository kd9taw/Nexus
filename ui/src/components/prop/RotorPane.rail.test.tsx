// @vitest-environment jsdom
//
// THE ROTOR BOX IN THE DASHBOARD RAIL IS LAID OUT COMPACTLY, so the live bearing (and the elevation),
// ■ STOP and line 2 (→ CALL bearing distance, Point) are in view without scrolling in the stock four-box
// rail at its default split. jsdom lays nothing out, so these are the cascade winners (cssCascade.testkit)
// of the decisions that make it fit; the geometry was measured in Chrome over the full app, where the
// box's body is 139 px at 1366×768 and 1600×900 and the pane was 288 px (317 with an elevation axis, 33
// more with line 2), so STOP stood a scroll down inside the box. Everywhere else the pane keeps its own
// layout (a Connect box, a box in a cockpit's columns): none of the rail's rules reach it there.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { PropagationSnapshot } from '../../types'
import { atToken, css, loadSheets } from '../../cssCascade.testkit'
import settingsFixture from '../__fixtures__/defaultSettings.json'

vi.mock('../../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    // A G-5500 at 123° / 45°, and the station's bearing for the call in the log entry.
    getSettings: vi.fn(async () => ({ ...settingsFixture, mygrid: 'EN52', rotatorModel: 603 })),
    readRotator: vi.fn(async () => 123),
    readRotatorState: vi.fn(async () => ({ azDeg: 123, reading: 'position', elDeg: 45, elRange: [0, 180] })),
    rotatorBearingToCall: vi.fn(async () => ({ pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 })),
    getSatTrackStatus: vi.fn(async () => null),
    getDeclination: vi.fn(async () => null),
    getGettingOut: vi.fn(async () => ({ count: 0, maxKm: 0, reports: [] })),
    getBandOutlook: vi.fn(async () => ({ bands: [], mufNow: 0 })),
    getSpaceWxScales: vi.fn(async () => ({ scales: { r: 0, s: 0, g: 0 }, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getXrayNow: vi.fn(async () => ({ flux: 1e-7, asOf: 0 })),
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getDxpedWindows: vi.fn(async () => []),
    getPathOutlook: vi.fn(async () => null),
    getDxccEntityLocations: vi.fn(async () => []),
  }
})

import { OwnedDashRail } from '../DashRail.testkit'
import { RotorPane } from './RotorPane'

// THE BUDGET: the slowest case here takes ~0.5 s on one core; a loaded full suite on this box has run cases
// up to 20 times slower than one core, past vitest's 5 s default. 15 s is the house budget.
vi.setConfig({ testTimeout: 15_000 })

const LIVE = {
  advisory: { headline: 'Bands are fair', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'live',
  asOf: Math.floor(Date.now() / 1000),
} as unknown as PropagationSnapshot

/** The rail as Chrome measured it: the Rotor box first of the four, beside FT, with EC1DD in the entry. */
const mountRail = () => {
  localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: { rail1: 'rotor', rail2: 'clock', rail3: 'spacewx', rail4: 'getout' } }))
  return render(
    <OwnedDashRail
      section="operate"
      myGrid="EN52"
      theme="dark"
      stations={[]}
      prop={LIVE}
      needByCall={new Map()}
      entryCall="EC1DD"
      onHide={() => {}}
    />,
  )
}

/** The rotor pane once it has its reading, its elevation axis and line 2's bearing. */
const drawn = (scope: string) =>
  vi.waitFor(() => {
    const pane = document.querySelector<HTMLElement>(`${scope} .rotor-pane`)
    if (!pane?.querySelector('.rotor-el') || !pane.querySelector('.rotor-aim-point')) throw new Error(`the Rotor pane in ${scope} has not drawn its reading and line 2 yet`)
    return pane
  })

const parts = (pane: HTMLElement) => {
  const q = (s: string) => {
    const el = pane.querySelector<HTMLElement>(s)
    if (!el) throw new Error(`the Rotor pane has no ${s}`)
    return el
  }
  return {
    row: q('.rotor-row'),
    rose: q('.rotor-rose'),
    side: q('.rotor-side'),
    az: q('.rotor-az'),
    el: q('.rotor-el'),
    entry: q('.rotor-entry'),
    input: q('.rotor-entry input'),
    stop: q('.rotor-stop'),
    aim: q('.rotor-aim'),
    to: q('.rotor-aim-to'),
    point: q('.rotor-aim-point'),
    hint: q('.rotor-hint'),
  }
}

/** A px length the sheet writes as arithmetic — px numbers, + − × ÷, brackets, calc() and clamp(), and
 *  nothing else (anything else throws) — evaluated. */
function evalPx(decl: string): number {
  const toks = decl.match(/\d+(?:\.\d+)?(?:px)?|clamp|calc|[-+*/(),]|\S/g) ?? []
  let i = 0
  const take = (want?: string) => {
    const tk = toks[i++]
    if (tk === undefined || (want !== undefined && tk !== want)) throw new Error(`not px arithmetic at "${tk}": ${decl}`)
    return tk
  }
  const factor = (): number => {
    const tk = take()
    if (tk === '-') return -factor()
    if (tk === 'calc') take('(')
    if (tk === '(' || tk === 'calc') return close(sum())
    if (tk === 'clamp') {
      take('(')
      const lo = sum()
      take(',')
      const v = sum()
      take(',')
      return close(Math.max(lo, Math.min(v, sum())))
    }
    if (/^\d/.test(tk)) return Number(tk.replace('px', ''))
    throw new Error(`not px arithmetic at "${tk}": ${decl}`)
  }
  const close = (v: number) => {
    take(')')
    return v
  }
  const product = () => {
    let v = factor()
    while (toks[i] === '*' || toks[i] === '/') v = take() === '*' ? v * factor() : v / factor()
    return v
  }
  const sum = () => {
    let v = product()
    while (toks[i] === '+' || toks[i] === '-') v = take() === '+' ? v + product() : v - product()
    return v
  }
  const v = sum()
  if (i !== toks.length) throw new Error(`not px arithmetic at "${toks[i]}": ${decl}`)
  return v
}

/** The rose's width in the rail, in px, with the rail `railPx` wide: the winning declaration with the rail's
 *  own width put in, evaluated. */
const roseWidthAt = (rose: Element, railPx: number) => {
  const decl = atToken('--dash-rail-w', `${railPx}px`, () => css(rose, 'width'))
  if (!decl) throw new Error('nothing sizes the rose in the rail: it stays 148 px')
  return evalPx(decl)
}

beforeAll(() => loadSheets())
beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.units', 'metric')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('the Rotor box in the dashboard rail', () => {
  it('draws a smaller rose beside the readings and the boxes, and line 2 under them at the box’s full width', async () => {
    mountRail()
    const p = parts(await drawn('.dash-rail'))
    // The drawing scales with the size the sheet gives it: with no viewBox a smaller box would crop it.
    expect(p.rose.getAttribute('viewBox'), 'a smaller rose would show only its top-left corner').toBe('0 0 148 148')
    expect(css(p.row, 'display'), 'the rose stands over the readings, and STOP is a scroll down inside the box').toBe('grid')
    expect(css(p.side, 'display'), 'line 2 stays in the column beside the rose, two or three lines tall').toBe('contents')
    expect(css(p.rose, 'grid-row'), 'the rose sets the height of one row and pushes everything under it').toBe('1 / span 3')
    for (const [name, el] of [['the bearing', p.az], ['the elevation', p.el], ['the boxes and STOP', p.entry]] as const)
      expect(css(el, 'grid-column'), `${name} leave the column beside the rose`).toBe('2')
    expect(css(p.aim, 'grid-column'), 'line 2 is squeezed beside the rose').toBe('1 / -1')
    expect(css(p.hint, 'grid-column'), 'the hint is squeezed beside the rose').toBe('1 / -1')
  })

  it('sizes the rose by the rail’s own width: 56 px at its 300 px default, 36 px at its 200 px floor', async () => {
    mountRail()
    const p = parts(await drawn('.dash-rail'))
    // The rail writes its width on itself; a size read off the BOX would change when the box grows a
    // scrollbar for line 2, and move STOP sideways the moment a call is entered.
    const rail = p.rose.closest<HTMLElement>('.dash-rail')!
    expect(rail.style.getPropertyValue('--dash-rail-w'), 'the rail no longer writes the width the rose is sized by').toMatch(/^\d+px$/)
    expect(roseWidthAt(p.rose, 300), 'the stock rail’s rose').toBe(56)
    // At the floor a box's column is 147 px: a 36 px rose and the gap leave the bearing ("123°T 121°M",
    // 97 px in Chrome) its one line; a wider rose puts the magnetic heading on a line of its own.
    expect(roseWidthAt(p.rose, 200), 'the floor’s rose').toBe(36)
    expect(roseWidthAt(p.rose, 250), 'between the two the rose grows a fifth of every px the rail is wider').toBe(46)
    expect(roseWidthAt(p.rose, 400), 'a wider rail’s rose grows past the size its box can hold').toBe(56)
    expect(css(p.rose, 'height'), 'the rose keeps its 148 px height under a smaller width').toBe('auto')
  })

  it('tightens the lines: the boxes are STOP’s height, and Point stands beside line 2’s words where they keep 6em', async () => {
    mountRail()
    const p = parts(await drawn('.dash-rail'))
    expect(css(p.input, 'min-height'), 'the bearing box keeps the 44 px touch floor, the tallest thing in the row').toBe('0px')
    // Beside Point the words wrap rather than push it under them: at the floor "→ EC1DD 227° (951 mi)" is wider
    // than the box, and under them Point cost a line of its own.
    expect(css(p.to, 'flex-basis'), 'line 2’s words claim their one-line width, and Point drops under them').toBe('6em')
    expect(css(p.to, 'flex-grow'), 'line 2’s words leave the rest of the line empty').toBe('1')
    expect(css(p.to, 'min-width'), 'line 2’s words cannot wrap beside Point, and run past the box').toBe('0px')
    // …but a Point too wide to leave them 6em ("Ausrichten" at the floor) still goes under them, where the
    // words take two lines instead of five.
    expect(css(p.aim, 'flex-wrap'), 'Point squeezes line 2’s words to a word a line').toBe('wrap')
  })

  it('■ STOP is the same button, enabled, with nothing between it and the box that could hide it', async () => {
    mountRail()
    const pane = await drawn('.dash-rail')
    const { stop } = parts(pane)
    expect(stop.tagName).toBe('BUTTON')
    expect((stop as HTMLButtonElement).disabled).toBe(false)
    expect(stop.closest('details'), 'STOP sits behind a disclosure').toBeNull()
    for (let el: HTMLElement | null = stop; el && el !== pane.parentElement; el = el.parentElement) {
      expect(css(el, 'display'), `${el.className} hides STOP`).not.toBe('none')
      expect(css(el, 'visibility'), `${el.className} hides STOP`).not.toBe('hidden')
    }
  })
})

describe('everywhere else the pane keeps its own layout', () => {
  // The two other hosts' chains, as they render: a Connect box, and a box in a cockpit's own columns (where
  // the rail's boxes stand below `lg`, components/panes/CockpitBox).
  const hosts = {
    'a Connect box': (pane: JSX.Element) => (
      <div className="connect">
        <section className="pane-frame" data-slot="right2">
          <div className="pane-body">{pane}</div>
        </section>
      </div>
    ),
    'a box in a cockpit’s columns': (pane: JSX.Element) => (
      <section className="pane-frame">
        <div className="pane-body">
          <div className="box-body">{pane}</div>
        </div>
      </section>
    ),
  }
  for (const [name, host] of Object.entries(hosts)) {
    it(`in ${name} none of the rail’s rules reach it`, async () => {
      render(<div className="host">{host(<RotorPane entryCall="EC1DD" />)}</div>)
      const p = parts(await drawn('.host'))
      expect(css(p.row, 'display')).toBe('flex')
      expect(css(p.side, 'display')).toBe('flex')
      expect(css(p.rose, 'width'), 'the rose is drawn smaller here too').toBeNull()
      expect(css(p.rose, 'grid-row')).toBeNull()
      expect(css(p.aim, 'grid-column')).toBeNull()
      expect(css(p.input, 'min-height')).toBe('44px')
      expect(css(p.aim, 'flex-wrap')).toBe('wrap')
      expect(css(p.az, 'font-size')).toBe('1.6em')
      // The drawing is the same at its own size: a viewBox of the size it is drawn at changes nothing.
      expect([p.rose.getAttribute('width'), p.rose.getAttribute('height')]).toEqual(['148', '148'])
    })
  }
})
