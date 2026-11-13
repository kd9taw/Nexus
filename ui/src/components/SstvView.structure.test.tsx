// @vitest-environment jsdom
//
// SSTV SHELL STRUCTURE (2026-07-30 layout assessment; census growers.md #9/#10).
//
// SSTV follows RTTY's shape of the cockpit shell contract (2026-11-04): a PINNED header (the
// dial, the band, ⊞ Panels, power and CAT never scroll out of the window), the RX stage, the TX
// strip, ONE pane region with a single column — so it never has a track it cannot fill, and it
// fills at every width (useRegionCols `oneGroup`) — and the TX bar. The roles fix census #10 directly: the Transmit
// composer is a bounded strip (fit="content" — a drop zone cannot use surplus), the
// Gallery is the fill grower beside the RX stage. The `.sstv-lower` 50/50 wrapper
// (each pane took half the region whatever it held) is deleted, not restyled.
//
// The RX stage (.sstv-canvas) stays the shell's own bounded flex child, NOT a frame:
// it is canvas-centric (the live decode / the band waterfall), its sizing context is
// the shell's flex share (1.1) with the band's self-disarming floor, and a frame's
// .pane-body scroller would hand its height to content measurement instead.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup, fireEvent, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'
import { SSTV_PANELS, coercePanelLayout, panelStateIn, type PanelLayoutApi, type SstvPanelId } from '../features/panelState'
import { chainOf, parseRules, winnerAt } from '../cssCascade'

// THE BUDGET (2026-10-09). The slowest case here, "the RF scope pane ships hidden; ticked, it is one more…", takes
// 0.27 s and 0.19 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

// The RF scope pane's picture is a renderer canvas jsdom cannot draw; its frame and place are what
// these cases check.
vi.mock('./PhoneScope', () => ({ PhoneScope: (p: { feed?: string }) => <div data-testid="rfscope-stub" data-feed={p.feed} /> }))
vi.mock('./Waterfall', () => ({
  // Capture the cadence prop: the liveliness pin below asserts the SSTV band waterfall runs at
  // the live-instrument 50 ms cadence, not the FT surfaces' 120 ms default.
  Waterfall: (p: { rowMs?: number }) => <div data-testid="band-waterfall" data-rowms={p.rowMs} />,
}))
// The rotor strip has its own suite (SstvView.rotor.test.tsx), and it reaches the rotator on mount.
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(async () => []),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>

const snap = {
  radio: {
    dialMhz: 14.23,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const NO_HEALTH: SstvHealth = {
  armed: false,
  audioPeak: 0,
  lastAudioUnix: null,
  drains: 0,
  visSeen: 0,
  lastVisUnix: null,
  unknownVis: 0,
  lastUnknownVisCode: null,
  lastUnknownVisUnix: null,
  images: 0,
  lastImageUnix: null,
  transmitting: false,
}

const IDLE: SstvState = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: NO_HEALTH,
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
}

function fakePanels(removed: SstvPanelId[] = [], setPanelState: PanelLayoutApi<SstvPanelId>['setPanelState'] = () => {}): PanelLayoutApi<SstvPanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState,
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

async function renderView(props: Partial<Parameters<typeof SstvView>[0]> = {}) {
  const r = render(<SstvView snap={snap} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  // Entering the view starts the receiver (`sstv_auto_arm`). These cases are about
  // the shell's structure, so it just resolves to the same state.
  ;(api.sstvAutoArm as ReturnType<typeof vi.fn>).mockReset().mockResolvedValue(IDLE)
})
afterEach(cleanup)

describe('SstvView pane shell', () => {
  it('the RF scope pane ships hidden; ticked, it is one more frame, FIRST under the TX strip, so the strip never moves', async () => {
    await renderView()
    expect(document.querySelector('[data-pane="rfScope"]'), 'the RF scope pane shipped visible').toBeNull()
    cleanup()
    await renderView({ panels: fakePanels() })
    const shell = document.querySelector('main.layout.single.sstv-view')!
    const frames = Array.from(shell.querySelectorAll(':scope > .cockpit-panes > .cockpit-col > .pane-frame')).map((f) => f.getAttribute('data-pane'))
    // fakePanels docks every id, so the large TX meters pane stands too: under the RF scope pane, over the
    // gallery, with the composer last (#479).
    expect(frames).toEqual(['rfScope', 'txMetersPane', 'gallery', 'txcompose'])
    const strip = shell.querySelector(':scope > .cockpit-txstrip')!
    expect(strip.previousElementSibling?.matches('.pane-splitter'), 'the TX strip left its place under the stage').toBe(true)
    expect(strip.nextElementSibling?.matches('.cockpit-panes'), 'the pane region is not directly under the TX strip').toBe(true)
    expect(shell.querySelector('.cockpit-col')!.firstElementChild?.getAttribute('data-pane'), 'the RF scope pane is not first in the region').toBe('rfScope')
    const pane = shell.querySelector('[data-pane="rfScope"]')!
    expect(pane.getAttribute('data-fit')).toBe('fill')
    expect(pane.querySelector('[data-testid="rfscope-stub"]')?.getAttribute('data-feed')).toBe('rf')
    // The RX stage is untouched by it: the band (or a picture) stays the stage.
    expect(shell.querySelector(':scope > .sstv-canvas')).not.toBeNull()
  })

  it('the shell holds no child kinds beyond the census (header, RX stage and its divider, TX strip, region, TX bar)', async () => {
    await renderView()
    const shell = document.querySelector('main.layout.single.sstv-view')!
    expect(shell).not.toBeNull()
    // The stage's divider (layout L6) is a shell child, as the scope dividers are in Phone/CW/JS8.
    // The frames are in the ONE pane region's column (2026-11-04), never shell children.
    const ALLOWED = ['.cockpit-header', '.sstv-canvas', '.pane-splitter', '.cockpit-txstrip', '.cockpit-panes', '.sstv-tx-bar']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(shell.querySelectorAll(':scope > .pane-frame').length, 'a frame is a shell child again').toBe(0)
    expect(shell.querySelectorAll(':scope > .cockpit-panes > .cockpit-col > .pane-frame').length).toBe(2)
    // The TX bar is still the LAST shell child: bottom-sticky, it parks in the scrollport at every scroll.
    expect(shell.lastElementChild?.matches('.sstv-tx-bar'), 'the TX bar is not the last shell child').toBe(true)
    // THE TX STRIP (2026-10-01): exactly one, a shell child directly under the scope (after
    // its divider), holding the stop controls the header used to hold — and the header none.
    const strips = shell.querySelectorAll(':scope > .cockpit-txstrip')
    expect(strips.length, 'no TX strip in the shell').toBe(1)
    expect(strips[0].previousElementSibling?.matches('.pane-splitter'), 'the TX strip is not directly under the scope').toBe(true)
    const named = (root: Element, re: RegExp) => [...root.querySelectorAll('button')].filter((b) => re.test(b.textContent!.trim()))
    expect(named(strips[0], /^stop tx$/i).length, 'Stop TX is not in the TX strip').toBe(1)
    expect(named(strips[0], /^tune$/i).length, 'Tune is not in the TX strip').toBe(1)
    expect(named(shell.querySelector('.cockpit-header')!, /^stop tx$|^tune$|^tuning…$|^atu$|tx (on|off)$/i), 'the header still draws a transmit control').toEqual([])
  })

  it('the Transmit composer renders through a frame with fit="content" (a drop zone cannot stretch)', async () => {
    await renderView()
    const pane = document.querySelector('[data-pane="txcompose"]')
    expect(pane, 'the composer is not framed').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.getAttribute('data-fit')).toBe('content')
    // Its content is inside the frame body, not a shell-level sibling.
    expect(pane!.querySelector('.pane-body .sstv-tx-drop')).not.toBeNull()
    expect(pane!.querySelector('.pane-body input[type=file]')).not.toBeNull()
  })

  it('the Gallery renders through a fill frame (the one lower grower beside the RX stage)', async () => {
    await renderView()
    const pane = document.querySelector('[data-pane="gallery"]')
    expect(pane, 'the gallery is not framed').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.getAttribute('data-fit')).toBe('fill')
    expect(pane!.querySelector('.pane-body .sstv-gallery-grid')).not.toBeNull()
  })

  it('one pane region with ONE column, filling at every width; the header is pinned; the .sstv-lower 50/50 wrapper is gone', async () => {
    // jsdom measures every region at 0 px wide: a region that could stack would stamp 'stack' here.
    await renderView()
    const shell = document.querySelector('main.layout.single.sstv-view')!
    const regions = shell.querySelectorAll(':scope > .cockpit-panes')
    expect(regions.length, 'not exactly one pane region').toBe(1)
    expect(regions[0].querySelectorAll(':scope > .cockpit-col').length, 'more than the one column').toBe(1)
    expect([regions[0].getAttribute('data-cols'), regions[0].getAttribute('data-flow')]).toEqual(['1', 'fill'])
    const head = shell.firstElementChild!
    expect(head.classList.contains('cockpit-header'), 'the header is not the first shell child').toBe(true)
    expect(head.classList.contains('cockpit-header-pinned'), 'the header is not pinned: it scrolls away with the stage').toBe(true)
    expect(
      document.querySelectorAll('.sstv-lower').length,
      '.sstv-lower is back — the wrapper whose 50/50 split is census finding #10',
    ).toBe(0)
  })

  it('the RX stage is the shell-owned bounded stage, never framed', async () => {
    await renderView()
    const stage = document.querySelector('.sstv-canvas')
    expect(stage).not.toBeNull()
    expect(stage!.closest('.pane-frame')).toBeNull()
  })

  // ── THE BAND WATERFALL IS A PANEL NOW (operator, 2026-08-16) ──────────────────────
  // "add the waterfall in each window as an option to remove in the panels section — leave
  // it ON by default, give me the option to turn it off." SSTV is the awkward one: the tick
  // reaches only HALF of a region that is the band until a VIS lands and the PICTURE after
  // it. Three cases, because the interesting one is the third.
  it('the band waterfall is SHOWN by default — the ⊞ entry is an option, not a new default', async () => {
    await renderView()
    expect(document.querySelector('[data-testid="band-waterfall"]')).not.toBeNull()
    expect(document.querySelector('.sstv-canvas')).not.toBeNull()
  })

  it("⊞ 'Waterfall' unticked drops the whole idle stage, not just the canvas inside it", async () => {
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('[data-testid="band-waterfall"]'), 'the waterfall survived its own hide').toBeNull()
    // AND the section with it. `.sstv-canvas` is `flex: 1.1 1 0` with a 16em floor: keep it
    // and empty it and the operator trades a waterfall for a 16em bordered void that hoards
    // the height he ticked the box to reclaim. This is the assertion that would catch the
    // tempting one-line version of this change (gating the <Waterfall> alone).
    expect(document.querySelector('.sstv-canvas'), 'an empty 16em stage is left hoarding the height').toBeNull()
    // The lower panes are still there to receive it — the gallery is the shell's fill grower.
    expect(document.querySelector('[data-pane="gallery"]')).not.toBeNull()
    expect(document.querySelector('[data-pane="txcompose"]')).not.toBeNull()
    // And the TX bar, which is where Stop lives, is untouched by any tick (THE STOP LINE).
    expect(document.querySelector('.sstv-tx-stop')).not.toBeNull()
  })

  it('a picture in flight takes the stage even with the waterfall unticked', async () => {
    // The tick hides the BAND, not the decode. An operator who unticked the waterfall last
    // week has not asked to stop seeing pictures — and this view exists to show them.
    getSstvState.mockResolvedValue({
      ...IDLE,
      armed: true,
      mode: 'Robot 36',
      linesDone: 1,
      linesTotal: 240,
      previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
      previewWidth: 2,
      previewHeight: 1,
    })
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.sstv-canvas'), 'the RX stage went with the band waterfall').not.toBeNull()
    expect(document.querySelector('.sstv-live'), 'no picture on the stage while one is decoding').not.toBeNull()
    expect(document.querySelector('[data-testid="band-waterfall"]'), 'the band is back under a picture').toBeNull()
  })

  it('a picture arriving onto a HIDDEN-waterfall stage is still measured and upscaled', async () => {
    // The stage is not permanent any more, and the measurement has to follow it. With the
    // waterfall unticked the section does not exist while idle and mounts the instant a VIS
    // lands — so a mount-only ResizeObserver would attach when there was nothing to observe
    // and never again, and the picture would fall back to the sheet's 480px 3× default with
    // the integer upscale silently never applied. Same fixture as the measurement case below,
    // one id hidden: 1000×800 stage, native 2×1, 6× cap ⇒ 12px.
    const cw = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
    const ch = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
    Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => 1000 })
    Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => 800 })
    try {
      getSstvState.mockResolvedValue({
        ...IDLE,
        armed: true,
        mode: 'Robot 36',
        linesDone: 1,
        linesTotal: 240,
        previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
        previewWidth: 2,
        previewHeight: 1,
      })
      await renderView({ panels: fakePanels(['scope']) })
      const canvas = document.querySelector('.sstv-live-canvas') as HTMLElement
      expect(canvas, 'no live canvas rendered with the waterfall hidden').not.toBeNull()
      expect(
        canvas.style.getPropertyValue('--sstv-img-w'),
        'the stage was never measured — the picture fell back to the sheet default',
      ).toBe('12px')
    } finally {
      if (cw) Object.defineProperty(Element.prototype, 'clientWidth', cw)
      if (ch) Object.defineProperty(Element.prototype, 'clientHeight', ch)
    }
  })

  it('the band waterfall polls at the live-instrument cadence (50 ms), not the FT default', async () => {
    // The idle RX stage IS the band ("what's on the frequency right now") — a live instrument
    // like the rig scope, not a slot-synchronous FT surface. The producer makes a fresh row
    // every 20 ms; the FT 120 ms poll would discard 5 of 6 rows (operator report 2026-07-30).
    await renderView()
    const wf = document.querySelector('[data-testid="band-waterfall"]')
    expect(wf, 'idle band waterfall did not render — cadence untested').not.toBeNull()
    expect(wf!.getAttribute('data-rowms')).toBe('50')
  })

  it('the TX bar is the LAST shell child — parked in the deficit valve (dock discipline)', async () => {
    await renderView()
    const shell = document.querySelector('main.layout.single.sstv-view')!
    expect(
      shell.lastElementChild!.classList.contains('sstv-tx-bar'),
      'the bar must be the final child: sticky bottom parks it at the scrollport bottom, so ' +
        'when the shell valve scrolls, Send/Stop never leave reach. Mid-column, sticky ' +
        'bottom cannot stop Stop scrolling off the TOP on the way down to the gallery.',
    ).toBe(true)
  })

  it('TX controls (Send / Stop / mode) live in the pinned bar, never inside a pane', async () => {
    await renderView()
    for (const sel of ['.sstv-tx-send', '.sstv-tx-stop', '.sstv-tx-mode']) {
      const el = document.querySelector(sel)
      expect(el, `${sel} missing`).not.toBeNull()
      expect(el!.closest('.sstv-tx-bar'), `${sel} is not in the TX bar`).not.toBeNull()
      expect(
        el!.closest('.pane-frame'),
        `${sel} is inside a pane frame — a pane scrolls, transmit controls must not`,
      ).toBeNull()
    }
  })

  it("⊞ Panels 'removed' hides exactly the named pane; Send/Stop have no id to gate", async () => {
    await renderView({ panels: fakePanels(['txcompose']) })
    expect(document.querySelector('[data-pane="txcompose"]')).toBeNull()
    expect(document.querySelector('[data-pane="gallery"]')).not.toBeNull()
    expect(document.querySelector('.sstv-tx-send')).not.toBeNull()
    expect(document.querySelector('.sstv-tx-stop')).not.toBeNull()
  })

  it('stamps an integer-multiple --sstv-img-w on the live canvas from the measured stage', async () => {
    // jsdom reports clientWidth/Height 0 (the 0×0 keep-alive guard would keep the last
    // real size); give every element a fake layout so the stage measure sees 1000×800.
    // Decode native is 2×1 (the existing preview fixture): avail ≈ 968×736 allows far
    // more than the 6× cap ⇒ k = 6 ⇒ width 12px. Fractional would be e.g. 484px.
    const cw = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
    const ch = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
    Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => 1000 })
    Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => 800 })
    try {
      getSstvState.mockResolvedValue({
        ...IDLE,
        armed: true,
        mode: 'Robot 36',
        linesDone: 1,
        linesTotal: 240,
        previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
        previewWidth: 2,
        previewHeight: 1,
      })
      await renderView()
      const canvas = document.querySelector('.sstv-live-canvas') as HTMLElement
      expect(canvas, 'no live canvas rendered').not.toBeNull()
      expect(canvas.style.getPropertyValue('--sstv-img-w')).toBe('12px')
    } finally {
      if (cw) Object.defineProperty(Element.prototype, 'clientWidth', cw)
      if (ch) Object.defineProperty(Element.prototype, 'clientHeight', ch)
    }
  })
})

// ── THE STAGE'S DIVIDER (layout L6) ──────────────────────────────────────────────────────────
// The stage is the column's grower — the one shape that follows this view at every window, and
// the picture's integer upscale rides on it — so its divider paints NOTHING until the operator
// moves it; moved, the stage is marked `data-sized` (its sheet rule's key) and the size is stored
// per surface; a reset gives the grower back. jsdom lays nothing out, so the shell and the stage
// are stubbed: the stock stage stands at 300 px of a 1000 px shell, and the sized one lays out like
// its rule (the painted basis, the 16em floor, 70 % of the window) at jsdom's 16 px / 768 px.
describe('the SSTV stage divider', () => {
  function layOut() {
    const real = HTMLElement.prototype.getBoundingClientRect
    const rect = (height: number) =>
      ({ top: 0, left: 0, width: 800, height, right: 800, bottom: height, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('main.sstv-view')) return rect(1000)
      if (this.matches('.sstv-canvas')) {
        if (!this.hasAttribute('data-sized')) return rect(300)
        const v = shell().style.getPropertyValue('--sstv-stage-h')
        const basis = v === '' ? 350 : v.endsWith('%') ? parseFloat(v) * 10 : parseFloat(v)
        return rect(Math.min(0.7 * 768, Math.max(16 * 16, basis)))
      }
      return real.call(this)
    })
  }
  const shell = () => document.querySelector<HTMLElement>('main.sstv-view')!
  const stage = () => document.querySelector<HTMLElement>('.sstv-canvas')!
  const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))
  const divider = () => screen.queryByRole('separator', { name: 'waterfall and picture height' })
  const KEY = 'nexus.split.sstv.stage'
  const LIVE: SstvState = {
    ...IDLE,
    armed: true,
    mode: 'Robot 36',
    linesDone: 1,
    linesTotal: 240,
    previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
    previewWidth: 2,
    previewHeight: 1,
  }
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('--vh-eff')
    document.documentElement.style.removeProperty('--ui-zoom')
  })
  afterEach(() => vi.restoreAllMocks())

  it('sits right under the stage, a shell child outside every pane, and the TX bar stays last', async () => {
    await renderView()
    const sep = divider()
    expect(sep, 'no divider under the stage').not.toBeNull()
    expect(sep!.previousElementSibling, 'the divider is not under the stage').toBe(stage())
    expect(sep!.parentElement).toBe(shell())
    expect(sep!.closest('.pane-frame')).toBeNull()
    expect(sep!.tabIndex).toBe(0)
    expect(shell().lastElementChild!.classList.contains('sstv-tx-bar')).toBe(true)
  })

  it('stock: the stage keeps the size the sheet gives it — nothing painted, marked or stored', async () => {
    layOut()
    await renderView()
    expect(shell().style.getPropertyValue('--sstv-stage-h')).toBe('')
    expect(stage().hasAttribute('data-sized')).toBe(false)
    expect(localStorage.getItem(KEY)).toBeNull()
    // The stage as it stands; the 16em floor at 16 px; 70 % of the 768 px window.
    expect(aria(divider()!)).toEqual(['300', '256', '538'])
  })

  it('a key sizes the stage from where it stands, a reset gives the grower back', async () => {
    layOut()
    await renderView()
    const sep = divider()!
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(shell().style.getPropertyValue('--sstv-stage-h')).toBe(`${(316 / 1000) * 100}%`)
    expect(stage().hasAttribute('data-sized')).toBe(true)
    expect(localStorage.getItem(KEY)).toBe(String((316 / 1000) * 100))
    fireEvent.keyDown(sep, { key: 'End' })
    expect(sep.getAttribute('aria-valuenow')).toBe('538')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(shell().style.getPropertyValue('--sstv-stage-h')).toBe('')
    expect(stage().hasAttribute('data-sized')).toBe(false)
    expect(sep.getAttribute('aria-valuenow')).toBe('300')
  })

  it('a stored height is restored sized, clamped against this window, and kept for a bigger one', async () => {
    localStorage.setItem(KEY, '75')
    layOut()
    await renderView()
    expect(stage().hasAttribute('data-sized')).toBe(true)
    expect(divider()!.getAttribute('aria-valuenow')).toBe('538')
    expect(localStorage.getItem(KEY), 'the clamp is apply-side only').toBe('75')
  })

  it('goes with the stage when the waterfall is unticked and nothing is decoding', async () => {
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.sstv-canvas')).toBeNull()
    expect(divider()).toBeNull()
  })

  it('stays with the stage while a picture is in flight, the waterfall unticked or not', async () => {
    getSstvState.mockResolvedValue(LIVE)
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.sstv-live'), 'no picture on the stage').not.toBeNull()
    expect(divider(), 'the picture lost its divider with the waterfall').not.toBeNull()
    expect(divider()!.previousElementSibling).toBe(stage())
  })
})

// (The computed-winner check that the sheet's `.sstv-live-canvas` width actually consumes
// the stamp lives in cockpit-shells.test.ts, beside the file's other cascade computers —
// this jsdom file cannot read styles.css: import.meta.url is not file-scheme here.)

// #479: THE COMPOSER AT THE BOTTOM, THE LARGE TX METERS PANE, AND "OPEN FOLDER". The composer stands last in
// the pane region, directly above the Send/Stop bar, with the gallery above it taking the spare height; it lays
// out sideways (picture left, options right) and wraps by its own width. The large TX meters pane joins SSTV's
// vocabulary as FT's, Phone's and CW's: hidden until ticked, and one click from the strip's meters. Nothing in the
// strip or the bar moves, and neither gains or loses a control.
describe('#479: the composer at the bottom, the meters pane, Open folder', () => {
  const sheet = (name: string) =>
    readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
  const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
  const win = (el: Element, prop: string) => winnerAt(RULES, 'dark', chainOf(el), prop)?.value.trim() ?? null
  const frames = () =>
    [...document.querySelectorAll('main.sstv-view > .cockpit-panes > .cockpit-col > .pane-frame')].map((f) => f.getAttribute('data-pane'))
  afterEach(() => document.documentElement.removeAttribute('data-viewport'))

  it('the composer is the LAST pane, directly above the Send/Stop bar; the gallery stands above it', async () => {
    await renderView()
    expect(frames()).toEqual(['gallery', 'txcompose'])
    const shell = document.querySelector('main.sstv-view')!
    const col = shell.querySelector(':scope > .cockpit-panes > .cockpit-col')!
    expect(col.lastElementChild?.getAttribute('data-pane'), 'the composer is not at the foot of the region').toBe('txcompose')
    expect(shell.querySelector(':scope > .cockpit-panes')!.nextElementSibling?.matches('.sstv-tx-bar'), 'the bar is not right under the region').toBe(true)
    // Still the content-height strip it was, and still holding no transmit control.
    expect(document.querySelector('[data-pane="txcompose"]')!.getAttribute('data-fit')).toBe('content')
    for (const sel of ['.sstv-tx-send', '.sstv-tx-stop', '.sstv-tx-mode']) expect(document.querySelector(sel)!.closest('.pane-frame'), sel).toBeNull()
  })

  it('the composer lays out sideways and wraps by its own width: the picture side, then the options', async () => {
    await renderView()
    const row = document.querySelector('[data-pane="txcompose"] .sstv-compose-row')
    expect(row, 'no composer row').not.toBeNull()
    expect(win(row!, 'display')).toBe('flex')
    expect(win(row!, 'flex-wrap')).toBe('wrap')
    const stage = row!.firstElementChild!
    expect(stage.classList.contains('sstv-compose-stage'), 'the picture side does not lead the row').toBe(true)
    expect(stage.querySelector('.sstv-tx-drop'), 'the drop zone left the picture side').not.toBeNull()
    expect(stage.querySelector('input[type=file]'), 'the file picker left the picture side').not.toBeNull()
    // Nothing loaded: the drop zone takes the whole row, as it did.
    expect(win(stage, 'flex')).toBe('1 1 auto')
    expect(win(stage, 'min-width')).toBe('0')
    // Loaded, the picture side is the preview's own width, and yields before it overflows.
    cleanup()
    const c = render(
      <div className="sstv-compose-row">
        <div className="sstv-compose-stage sstv-compose-stage-packed" />
        <div className="sstv-compose-opts" />
      </div>,
    ).container
    const packed = c.querySelector('.sstv-compose-stage')!
    expect(win(packed, 'flex')).toBe('0 1 auto')
    expect(win(packed, 'min-width')).toBe('0')
    const opts = c.querySelector('.sstv-compose-opts')!
    // A basis in em, so the wrap point follows the text size and the zoom; capped so a wide window
    // never gets a window-wide text field; shrinkable to nothing before the row overflows sideways.
    expect(win(opts, 'flex')).toBe('1 1 24em')
    expect(win(opts, 'min-width')).toBe('0')
    expect(win(opts, 'max-width')).toBe('64em')
  })

  it('the large TX meters pane ships hidden, and ticked it is a fill pane over the gallery', async () => {
    await renderView()
    expect(document.querySelector('[data-pane="txMetersPane"]'), 'shown with no panel record').toBeNull()
    cleanup()
    await renderView({ panels: fakePanels(['rfScope']) })
    expect(frames()).toEqual(['txMetersPane', 'gallery', 'txcompose'])
    expect(document.querySelector('[data-pane="txMetersPane"]')!.getAttribute('data-fit')).toBe('fill')
    expect(document.querySelector('[data-pane="txMetersPane"] .txmeters-pane'), 'the pane holds no meters').not.toBeNull()
  })

  it('from md up the strip opens it beside its meters; below md the strip has no meters and no toggle', async () => {
    const set = vi.fn()
    document.documentElement.setAttribute('data-viewport', 'lg')
    await renderView({ panels: fakePanels(['txMetersPane'], set) })
    const strip = document.querySelector('main.sstv-view > .cockpit-txstrip')!
    const toggle = strip.querySelector<HTMLButtonElement>('.txmeters-pane-toggle')
    expect(toggle, 'no way to the large meters from the strip').not.toBeNull()
    expect(toggle!.previousElementSibling?.className).toContain('cockpit-txstrip-meters')
    fireEvent.click(toggle!)
    expect(set).toHaveBeenCalledWith('txMetersPane', 'docked')
    cleanup()
    document.documentElement.setAttribute('data-viewport', 'sm')
    await renderView({ panels: fakePanels(['txMetersPane']) })
    expect(document.querySelector('.txmeters-pane-toggle')).toBeNull()
  })

  it('an SSTV layout saved before the change opens with every pane as it was and the meters hidden, writing nothing', () => {
    const saved = { v: 2, state: { scope: 'docked', rfScope: 'docked', txcompose: 'docked', gallery: 'removed' }, share: {} }
    const layout = coercePanelLayout(SSTV_PANELS, saved)
    expect(panelStateIn(SSTV_PANELS, layout, 'txMetersPane')).toBe('removed')
    expect(panelStateIn(SSTV_PANELS, layout, 'rfScope')).toBe('docked')
    expect(panelStateIn(SSTV_PANELS, layout, 'txcompose')).toBe('docked')
    expect(panelStateIn(SSTV_PANELS, layout, 'gallery')).toBe('removed')
    // The read is the insertion: no entry is added for the new pane.
    expect(Object.keys(layout.state).sort()).toEqual(['gallery', 'rfScope', 'scope', 'txcompose'])
  })

  it('✎ Edit & resend brings the composer into view once the picture is loaded: it stands below the gallery now', async () => {
    // The composer moved under the gallery, so on a short window ✎ loaded a picture into a composer below the
    // fold and nothing on screen changed. Once the load settles the composer is scrolled to, the nearest way,
    // and focus stays where it was.
    const w = window as unknown as { __TAURI_INTERNALS__?: { convertFileSrc: (p: string) => string } }
    w.__TAURI_INTERNALS__ = { convertFileSrc: (p: string) => `asset://localhost${p}` }
    const realFetch = globalThis.fetch
    globalThis.fetch = vi.fn(async () => ({ arrayBuffer: async () => new ArrayBuffer(8) })) as unknown as typeof fetch
    // jsdom loads no picture: the decoder answers "nothing decoded", so the load settles on its notice, which
    // stands in the composer too and is as worth seeing.
    const g = globalThis as { createImageBitmap?: unknown }
    const realCib = g.createImageBitmap
    g.createImageBitmap = async () => ({ width: 0, height: 0 })
    const scrolled: Array<{ el: Element; arg: unknown }> = []
    const real = Element.prototype.scrollIntoView
    Element.prototype.scrollIntoView = function (this: Element, arg?: unknown) {
      scrolled.push({ el: this, arg })
    }
    try {
      getSstvState.mockResolvedValue({
        ...IDLE,
        gallery: [{ path: '/pics/a.png', mode: 'Scottie 1', finishedUtc: '2026-11-08T12:00:00Z', freqMhz: 14.23, lines: 256 }],
      })
      await renderView()
      const edit = document.querySelector<HTMLButtonElement>('.sstv-thumb-edit')!
      const focusBefore = document.activeElement
      await act(async () => {
        fireEvent.click(edit)
        // The load settles, then the next frame scrolls (jsdom's frames are ~16 ms).
        for (let i = 0; i < 40 && !scrolled.some((x) => x.el.closest('[data-pane="txcompose"]')); i++) await new Promise((r) => setTimeout(r, 10))
      })
      const toComposer = scrolled.filter((x) => x.el.closest('[data-pane="txcompose"]') != null)
      expect(toComposer.length, 'the composer was never scrolled into view').toBeGreaterThan(0)
      expect(toComposer[toComposer.length - 1].arg).toEqual({ block: 'nearest' })
      expect(document.activeElement).toBe(focusBefore)
    } finally {
      Element.prototype.scrollIntoView = real
      globalThis.fetch = realFetch
      g.createImageBitmap = realCib
      delete w.__TAURI_INTERNALS__
    }
  })

  it('the gallery button says what it does: Open folder', async () => {
    await renderView()
    const btn = document.querySelector<HTMLButtonElement>('.sstv-gallery-reveal')!
    expect(btn.textContent).toBe('Open folder')
  })
})
