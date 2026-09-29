// @vitest-environment jsdom
//
// THE LOGBOOK'S GLOBE | TABLE DIVIDER (layout L7): the globe band, fixed at 320 px, gets a divider
// under it. jsdom lays nothing out, so the scroller and the band get stubbed boxes, and the band's
// box follows the height the divider paints the way the sheet does (`.log-globe-band[data-sized]
// { height: var(--log-globe-h) }`, a % of the scroller): 320 px until it is sized. The real
// rectangles, and the rows starting where the band now ends, are measured in Chrome by the layout
// harness.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, waitFor, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import { setLogbookGlobeShown } from '../features/logbookGlobe'
import type { LogQuestion } from '../features/logAnswers'

const KEY = 'nexus.split.logbook.globe'
const SCROLL_H = 500
const BAND_H = 320

/** Every ResizeObserver made, with its callback and the elements it watches. */
let observers: Array<{ cb: () => void; els: Element[] }> = []
/** Reads of the rows' offset (the Logbook's list-offset measurement is the only reader). */
let offsetReads = 0
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => true }))
vi.mock('./QsoGlobe', () => ({ default: () => <div data-testid="qso-globe" /> }))
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSentById: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

const qso = {
  call: 'K1ABC', grid: 'FN31', band: '40m', freqMhz: 7.074, mode: 'FT8', rstSent: '-10',
  rstRcvd: '-12', name: null, qth: null, comment: null, notes: null, country: 'United States',
  whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null,
  ota: null, upload: undefined,
}

async function mount() {
  engineLog.mockResolvedValue([qso])
  const view = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  await waitFor(() => expect(view.container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
  await waitFor(() => expect(view.container.querySelector('.log-globe-band')).not.toBeNull())
  return view.container
}

const scroller = (c: HTMLElement) => c.querySelector<HTMLElement>('.log-scroll')!
const band = (c: HTMLElement) => c.querySelector<HTMLElement>('.log-globe-band')
const divider = () => screen.getByRole('separator', { name: 'Globe height' })
const painted = (c: HTMLElement) => scroller(c).style.getPropertyValue('--log-globe-h')
const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => Number(el.getAttribute(a)))

const realRect = HTMLElement.prototype.getBoundingClientRect
beforeEach(() => {
  localStorage.clear()
  observers = []
  offsetReads = 0
  globalThis.ResizeObserver = class {
    entry: { cb: () => void; els: Element[] }
    constructor(cb: () => void) {
      this.entry = { cb, els: [] }
      observers.push(this.entry)
    }
    observe(el: Element) {
      this.entry.els.push(el)
    }
    unobserve() {}
    disconnect() {
      this.entry.els = []
    }
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get(this: HTMLElement) {
      if (this.classList.contains('log-rows')) offsetReads++
      return 0
    },
  })
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const box = (height: number) => ({ x: 0, y: 0, left: 0, top: 0, width: 900, height, right: 900, bottom: height, toJSON() {} }) as DOMRect
    if (this.classList.contains('log-scroll')) return box(SCROLL_H)
    if (this.classList.contains('log-globe-band')) {
      const v = this.parentElement?.style.getPropertyValue('--log-globe-h') ?? ''
      if (this.hasAttribute('data-sized') && v.endsWith('%')) return box((parseFloat(v) / 100) * SCROLL_H)
      if (this.hasAttribute('data-sized') && v.endsWith('px')) return box(parseFloat(v))
      return box(BAND_H)
    }
    return realRect.call(this)
  }
})
afterEach(() => {
  cleanup()
  HTMLElement.prototype.getBoundingClientRect = realRect
})

describe('the Logbook’s globe | table divider', () => {
  it('sits between the band and the search/header block, and announces the band’s own 320 px', async () => {
    const c = await mount()
    const d = divider()
    expect(d.previousElementSibling).toBe(band(c))
    expect(d.nextElementSibling?.className).toBe('log-sticky')
    expect(d.getAttribute('aria-orientation')).toBe('horizontal')
    // An 8em floor (16 px in jsdom) to 90 % of the scroller.
    expect(aria(d)).toEqual([BAND_H, 128, 450])
    // Nothing painted or marked: the stock band is the sheet's until the operator moves it.
    expect(painted(c)).toBe('')
    expect(band(c)!.hasAttribute('data-sized')).toBe(false)
  })

  it('moving it down grows the band; Home/End go to its ends; Backspace gives the stock 320 px back', async () => {
    const c = await mount()
    const d = divider()
    fireEvent.keyDown(d, { key: 'ArrowDown' })
    expect(painted(c)).toBe(`${((BAND_H + 16) / SCROLL_H) * 100}%`)
    expect(band(c)!.hasAttribute('data-sized')).toBe(true)
    expect(Number(localStorage.getItem(KEY))).toBeCloseTo(((BAND_H + 16) / SCROLL_H) * 100, 6)
    fireEvent.keyDown(d, { key: 'Home' })
    expect(painted(c)).toBe(`${(128 / SCROLL_H) * 100}%`)
    fireEvent.keyDown(d, { key: 'End' })
    expect(painted(c)).toBe('90%')
    fireEvent.keyDown(d, { key: 'Backspace' })
    expect(painted(c)).toBe('')
    expect(band(c)!.hasAttribute('data-sized')).toBe(false)
    expect(localStorage.getItem(KEY)).toBe('')
  })

  it('a stored height is fitted into this scroller on load, and the stored preference is never rewritten', async () => {
    localStorage.setItem(KEY, '95')
    const c = await mount()
    expect(painted(c)).toBe('90%')
    expect(localStorage.getItem(KEY)).toBe('95')
  })

  it('the rows’ offset is re-measured when the band alone changes size', async () => {
    const c = await mount()
    // What a divider move is to the page: the band's box changes, the scroller's does not. Only
    // the observers watching the band fire. (The divider's own observer watches the band too, and
    // does not measure the rows: counting a watcher would not tell the two apart, a read does.)
    offsetReads = 0
    act(() => observers.filter((o) => o.els.includes(band(c)!)).forEach((o) => o.cb()))
    expect(offsetReads, 'a band resized by its divider would leave the list offset stale').toBeGreaterThan(0)
  })

  it('no band, no divider: the globe switched off takes both', async () => {
    const c = await mount()
    act(() => setLogbookGlobeShown(false))
    expect(band(c)).toBeNull()
    expect(screen.queryByRole('separator', { name: 'Globe height' })).toBeNull()
    act(() => setLogbookGlobeShown(true))
  })
})
