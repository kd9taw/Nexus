// @vitest-environment jsdom
//
// THE ROTOR BOX IN THE DASHBOARD RAIL GROWS INTO A TALLER OR WIDER BOX, centred (rotorFit.ts measures the box
// and stamps the pane; styles.css draws what is stamped). jsdom lays nothing out, so the boxes Chrome measured
// are handed to it here, element by element (the arithmetic itself is rotorFit.test.ts), and the drawing is
// the cascade winners (cssCascade.testkit). The geometry was measured in Chrome over the real rail.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import type { PropagationSnapshot } from '../../types'
import { atToken, css, loadSheets, pxOf } from '../../cssCascade.testkit'
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
    getMoonStatus: vi.fn(async () => null),
    startMoonTrack: vi.fn(async () => ({ state: 'following', elevation: true, azDeg: null, elDeg: null })),
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

vi.setConfig({ testTimeout: 15_000 })

const LIVE = {
  advisory: { headline: 'Bands are fair', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'live',
  asOf: Math.floor(Date.now() / 1000),
} as unknown as PropagationSnapshot

// ── THE LAYOUT jsdom does not do: Chrome's numbers in the rail at Normal text ─────────────────────────────
// The box's body (its content area, plus its 8 px padding); the bearing and elevation boxes 38.5 px and STOP
// 75, 26 tall, 4 px apart; the hint two lines (26); the compact rose 56 px (the 324 px rail's); the bearing
// at 1.25em (17.5 px) and the elevation at 14; the pane grid's 8 px column and 4 px row gaps.
const PAD = 8
let body = { w: 271, h: 255 }
const inRail = (el: Element) => el.classList.contains('pane-body') && el.closest('.dash-boxes') !== null
/** Every body the test hands a size, in the rail or out of it. */
const isBody = (el: Element) => el.classList.contains('pane-body')
const width = (el: Element): number => {
  if (isBody(el)) return body.w + 2 * PAD
  if (el instanceof HTMLInputElement && el.closest('.rotor-entry')) return 38.5
  if (el.classList.contains('rotor-stop')) return 75
  return 0
}
/** Line 2's height: one line beside Point, or two where its words wrap (a narrower rail, a longer call). */
let line2 = 25
const height = (el: Element): number => {
  if (isBody(el)) return body.h + 2 * PAD
  if (el.closest('.rotor-entry') && el.parentElement?.classList.contains('rotor-entry')) return 26
  if (el.classList.contains('rotor-hint')) return 26
  if (el.classList.contains('rotor-aim')) return line2
  return 0
}
/** Where each part stands, for the measure of the arrangement's height: in the compact layout (and 'beside')
 *  the bearing, the elevation and the boxes and STOP beside the rose, 70 px of them; 'under', the boxes and
 *  STOP under the rose (157 + 4 + 26 = 187 px tall in the operator's box). */
const rect = (el: Element): DOMRect => {
  const at = (top: number, h: number, w = 0) => ({ top, bottom: top + h, left: 0, right: w, width: w, height: h, x: 0, y: top, toJSON() {} }) as DOMRect
  if (isBody(el)) return at(0, body.h + 2 * PAD, body.w + 2 * PAD)
  const c = el.classList
  const pane = el.closest<HTMLElement>('.rotor-pane')
  const fit = pane?.getAttribute('data-fit')
  const rose = (fit && Number.parseFloat(pane!.style.getPropertyValue('--rotor-rose'))) || 56
  if (c?.contains('rotor-rose')) return at(0, rose, rose)
  if (fit === 'under') {
    if (c?.contains('rotor-az')) return at(0, 40)
    if (c?.contains('rotor-el')) return at(44, 26)
    if (c?.contains('rotor-entry')) return at(rose + 4, 26)
  } else {
    if (c?.contains('rotor-az')) return at(0, 20)
    if (c?.contains('rotor-el')) return at(24, 16)
    if (c?.contains('rotor-entry')) return at(44, 26)
  }
  return at(0, 0)
}
const styleOf = (el: Element): Partial<CSSStyleDeclaration> | null => {
  const c = el.classList
  if (isBody(el)) return { paddingLeft: `${PAD}px`, paddingRight: `${PAD}px`, paddingTop: `${PAD}px`, paddingBottom: `${PAD}px` }
  if (c.contains('rotor-row')) return { columnGap: '8px', rowGap: '4px' }
  if (c.contains('rotor-entry')) return { columnGap: '4px' }
  if (c.contains('rotor-az')) return { fontSize: '17.5px' }
  if (c.contains('rotor-el')) return { fontSize: '14px' }
  return null
}

/** The resize observers that watch the rotor's box, so a test can drag its divider. */
let boxObservers: Array<() => void> = []

beforeAll(() => loadSheets())
beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.units', 'metric')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  boxObservers = []
  line2 = 25
  globalThis.ResizeObserver = class {
    constructor(private cb: ResizeObserverCallback) {}
    observe(t: Element) {
      if (inRail(t)) boxObservers.push(() => this.cb([], this as unknown as ResizeObserver))
    }
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
    return width(this)
  })
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
    return height(this)
  })
  vi.spyOn(Element.prototype, 'clientWidth', 'get').mockImplementation(function (this: Element) {
    return width(this)
  })
  vi.spyOn(Element.prototype, 'clientHeight', 'get').mockImplementation(function (this: Element) {
    return height(this)
  })
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    return rect(this)
  })
  const real = window.getComputedStyle.bind(window)
  vi.spyOn(window, 'getComputedStyle').mockImplementation((el: Element, pseudo?: string | null) => {
    const style = real(el, pseudo)
    const own = styleOf(el)
    if (!own) return style
    return new Proxy(style, {
      get: (t, k) => {
        if (k in own) return own[k as keyof typeof own]
        const v = (t as unknown as Record<string | symbol, unknown>)[k]
        return typeof v === 'function' ? v.bind(t) : v
      },
    })
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The rail as the operator has it: the Rotor box first, beside FT, with EC1DD in the entry. */
const mountRail = (entryCall: string | null = 'EC1DD') => {
  localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: { rail1: 'rotor', rail2: 'clock', rail3: 'spacewx', rail4: 'getout' } }))
  const props = { section: 'operate', myGrid: 'EN52', theme: 'dark' as const, stations: [], prop: LIVE, needByCall: new Map(), onHide: () => {} }
  const r = render(<OwnedDashRail {...props} entryCall={entryCall} />)
  return { ...r, call: (c: string | null) => r.rerender(<OwnedDashRail {...props} entryCall={c} />) }
}

/** The rotor pane once it has its reading and its elevation axis (and line 2's bearing, with a call). */
const drawn = (call = true) =>
  vi.waitFor(() => {
    const pane = document.querySelector<HTMLElement>('.dash-rail .rotor-pane')
    if (!pane?.querySelector('.rotor-el') || (call && !pane.querySelector('.rotor-aim-point'))) throw new Error('the Rotor pane has not drawn yet')
    return pane
  })

const stamps = (pane: HTMLElement) => ({
  fit: pane.getAttribute('data-fit'),
  rose: pane.style.getPropertyValue('--rotor-rose'),
  az: pane.style.getPropertyValue('--rotor-az'),
  el: pane.style.getPropertyValue('--rotor-el'),
  top: pane.style.getPropertyValue('--rotor-top'),
})

describe('the Rotor box in the dashboard rail grows into its box', () => {
  it('in the operator’s box (a 324 px rail, a 270 px body): a 157 px rose, the readings at 31 and 21 px, the boxes and STOP under them', async () => {
    body = { w: 271, h: 255 }
    mountRail()
    // Centred in the 195 px left above line 2 and the hint: (195 − 187) / 2.
    expect(stamps(await drawn())).toEqual({ fit: 'under', rose: '157px', az: '31px', el: '21px', top: '4px' })
  })

  it('in the stock rail’s short box (139 px at 1366×768): the boxes stay beside the rose, which grows only to the readings’ height', async () => {
    body = { w: 247, h: 123 }
    mountRail()
    expect(stamps(await drawn())).toMatchObject({ fit: 'beside', rose: '63px', az: '17px', el: '14px' })
  })

  it('in a box that holds no bigger rose than the compact one: the compact layout, unstamped', async () => {
    body = { w: 247, h: 100 }
    mountRail()
    expect(stamps(await drawn())).toEqual({ fit: null, rose: '', az: '', el: '', top: '' })
  })

  it('a box made taller (its divider dragged, which renders nothing) is fitted again', async () => {
    body = { w: 247, h: 123 }
    mountRail()
    const pane = await drawn()
    expect(stamps(pane).fit).toBe('beside')
    body = { w: 247, h: 436 }
    expect(boxObservers.length, 'nothing watches the box').toBeGreaterThan(0)
    act(() => boxObservers.forEach((fire) => fire()))
    expect(stamps(pane)).toMatchObject({ fit: 'under', rose: '143px', az: '28px', el: '20px' })
  })

  it('a line 2 on two lines takes its room once, and keeps it when the call goes: STOP does not move with every QSO', async () => {
    body = { w: 271, h: 255 }
    line2 = 40
    const rail = mountRail(null)
    const pane = await drawn(false)
    expect(stamps(pane), 'before any call').toMatchObject({ fit: 'under', rose: '157px' })
    rail.call('EC1DD')
    await drawn()
    const held = stamps(pane)
    expect(held, 'its second line comes out of the rose').toMatchObject({ fit: 'under', rose: '151px', az: '30px' })
    rail.call(null)
    await vi.waitFor(() => expect(pane.querySelector('.rotor-aim')).toBeNull())
    expect(stamps(pane), 'the call gone').toEqual(held)
    rail.call('EC1DD')
    await drawn()
    expect(stamps(pane), 'the next call').toEqual(held)
  })

  it('line 2 coming and going changes nothing: the room under the arrangement is held for it', async () => {
    body = { w: 271, h: 255 }
    const rail = mountRail(null)
    const pane = await drawn(false)
    const without = stamps(pane)
    expect(without.fit, 'the operator’s box with no call').toBe('under')
    rail.call('EC1DD')
    await drawn()
    expect(stamps(pane)).toEqual(without)
    rail.call(null)
    await vi.waitFor(() => expect(pane.querySelector('.rotor-aim')).toBeNull())
    expect(stamps(pane)).toEqual(without)
  })
})

describe('the sheet draws what is stamped', () => {
  const parts = (pane: HTMLElement) => {
    const q = (s: string) => pane.querySelector<HTMLElement>(s)!
    return { row: q('.rotor-row'), rose: q('.rotor-rose'), az: q('.rotor-az'), el: q('.rotor-el'), entry: q('.rotor-entry'), aim: q('.rotor-aim'), hint: q('.rotor-hint'), needle: q('.rotor-needle:not(.target)'), ring: q('.rotor-ring'), tick: q('.rotor-tick') }
  }

  it('centred: the rose and the column beside it between two equal spacers, moved down by the stamp', async () => {
    body = { w: 271, h: 255 }
    mountRail()
    const p = parts(await drawn())
    expect(css(p.row, 'grid-template-columns')).toBe('minmax(0, 1fr) auto auto minmax(0, 1fr)')
    expect(css(p.rose, 'grid-column')).toBe('2')
    for (const el of [p.az, p.el]) expect(css(el, 'grid-column')).toBe('3')
    for (const el of [p.aim, p.hint]) expect(css(el, 'grid-column'), 'line 2 and the hint keep the box’s full width').toBe('1 / -1')
    expect(atToken('--rotor-top', '4px', () => css(p.row, 'padding-top'))).toBe('4px')
  })

  it('under: the rose spans the two readings, the boxes and STOP take a centred line of their own under them', async () => {
    body = { w: 271, h: 255 }
    mountRail()
    const p = parts(await drawn())
    expect(p.rose.closest('.rotor-pane')!.getAttribute('data-fit')).toBe('under')
    expect(css(p.rose, 'grid-row')).toBe('1 / span 2')
    expect(css(p.entry, 'grid-column')).toBe('1 / -1')
    expect(css(p.entry, 'justify-self')).toBe('center')
    // The readings gather at the middle of the rose's height.
    expect([css(p.az, 'align-self'), css(p.el, 'align-self')]).toEqual(['end', 'start'])
  })

  it('beside: the compact placements, the boxes and STOP in the column beside the rose', async () => {
    body = { w: 247, h: 123 }
    mountRail()
    const p = parts(await drawn())
    expect(css(p.rose, 'grid-row')).toBe('1 / span 3')
    expect(css(p.entry, 'grid-column')).toBe('3')
  })

  it('the rose takes the stamped size and its lines thicken with it; the readings take theirs, never under the compact floors', async () => {
    body = { w: 271, h: 255 }
    mountRail()
    const p = parts(await drawn())
    const at = <T,>(vars: Record<string, string>, fn: () => T): T =>
      Object.entries(vars).reduce<() => T>((f, [k, v]) => () => atToken(k, v, f), fn)()
    expect(at({ '--rotor-rose': '157px' }, () => pxOf(p.rose, 'min-width'))).toBe(157)
    // On a 395 px rose (a 720 px rail) the needle is 6.7 px, not the compact rose's 2.5.
    expect(at({ '--rotor-rose': '395px' }, () => pxOf(p.needle, 'stroke-width'))).toBeCloseTo(6.715, 3)
    expect(at({ '--rotor-rose': '395px' }, () => pxOf(p.ring, 'stroke-width'))).toBeCloseTo(3.95, 3)
    expect(at({ '--rotor-rose': '60px' }, () => pxOf(p.needle, 'stroke-width')), 'a small rose keeps the pane’s own widths').toBe(2.5)
    // The station's line with them, and its dots apart in step, or a big rose's would run into a solid line.
    const line = await vi.waitFor(() => {
      const l = p.rose.querySelector<SVGElement>('.rotor-bearing-line')
      if (!l) throw new Error('the station’s line is not drawn yet')
      return l
    })
    const gapAt = (rose: string) => {
      const v = at({ '--rotor-rose': rose }, () => css(line, 'stroke-dasharray')) ?? ''
      const arms = /^0 max\((.+)\)$/.exec(v)?.[1].split(',').map((a) => parseFloat(a)) ?? []
      return arms.length === 2 ? Math.max(...arms) : NaN
    }
    expect(at({ '--rotor-rose': '395px' }, () => pxOf(line, 'stroke-width'))).toBeCloseTo(3.95, 3)
    expect(at({ '--rotor-rose': '60px' }, () => pxOf(line, 'stroke-width')), 'a small rose keeps the pane’s own width').toBe(1.5)
    expect(gapAt('395px')).toBeCloseTo(10.665, 3)
    expect(gapAt('60px'), 'a small rose keeps the pane’s own dots').toBe(4)
    expect(at({ '--rotor-az': '31px' }, () => pxOf(p.az, 'font-size'))).toBe(31)
    expect(at({ '--rotor-el': '21px' }, () => pxOf(p.el, 'font-size'))).toBe(21)
    expect(at({ '--rotor-az': '10px' }, () => pxOf(p.az, 'font-size')), 'the bearing’s floor is the compact 1.25em').toBe(17.5)
    expect(at({ '--rotor-el': '10px' }, () => pxOf(p.el, 'font-size')), 'the elevation’s floor is the body size').toBe(14)
  })

  it('the box holds its scrollbar’s gutter while the pane is grown, so nothing centred moves sideways when it scrolls', async () => {
    body = { w: 271, h: 255 }
    mountRail()
    const pane = await drawn()
    expect(css(pane.closest('.pane-body')!, 'scrollbar-gutter')).toBe('stable')
    // A compact pane is the compact layout to the pixel: no gutter of its own.
    pane.removeAttribute('data-fit')
    expect(css(pane.closest('.pane-body')!, 'scrollbar-gutter')).toBeNull()
  })
})

describe('everywhere else nothing is stamped, however tall the box', () => {
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
    it(`in ${name}`, async () => {
      body = { w: 667, h: 590 }
      render(<div className="host">{host(<RotorPane entryCall="EC1DD" />)}</div>)
      const pane = await vi.waitFor(() => {
        const p = document.querySelector<HTMLElement>('.host .rotor-pane')
        if (!p?.querySelector('.rotor-el')) throw new Error('not drawn yet')
        return p
      })
      expect(stamps(pane)).toEqual({ fit: null, rose: '', az: '', el: '', top: '' })
      expect(css(pane.closest('.pane-body')!, 'scrollbar-gutter')).toBeNull()
      expect(boxObservers, 'nothing watches a box outside the rail').toEqual([])
    })
  }
})
