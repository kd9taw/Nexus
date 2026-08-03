// @vitest-environment jsdom
//
// SSTV SHELL STRUCTURE (2026-07-30 layout assessment; census growers.md #9/#10).
//
// SSTV follows RTTY's shape of the pane-grid contract: CockpitPaneFrame for the
// operator-content blocks, deliberately NO .cockpit-panes region — with two content
// blocks every multi-column tier is a 2-or-3-track template with one filled column,
// i.e. manufactured dead space. The roles fix census #10 directly: the Transmit
// composer is a bounded strip (fit="content" — a drop zone cannot use surplus), the
// Gallery is the fill grower beside the RX stage. The `.sstv-lower` 50/50 wrapper
// (each pane took half the region whatever it held) is deleted, not restyled.
//
// The RX stage (.sstv-canvas) stays the shell's own bounded flex child, NOT a frame:
// it is canvas-centric (the live decode / the band waterfall), its sizing context is
// the shell's flex share (1.1) with the band's self-disarming floor, and a frame's
// .pane-body scroller would hand its height to content measurement instead.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'
import type { PanelLayoutApi, SstvPanelId } from '../features/panelState'

vi.mock('./Waterfall', () => ({
  // Capture the cadence prop: the liveliness pin below asserts the SSTV band waterfall runs at
  // the live-instrument 50 ms cadence, not the FT surfaces' 120 ms default.
  Waterfall: (p: { rowMs?: number }) => <div data-testid="band-waterfall" data-rowms={p.rowMs} />,
}))
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

function fakePanels(removed: SstvPanelId[] = []): PanelLayoutApi<SstvPanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
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
  it('the shell holds no child kinds beyond the census (header, RX stage, TX bar, frames)', async () => {
    await renderView()
    const shell = document.querySelector('main.layout.single.sstv-view')!
    expect(shell).not.toBeNull()
    const ALLOWED = ['.cockpit-header', '.sstv-canvas', '.sstv-tx-bar', '.pane-frame']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(shell.querySelectorAll(':scope > .pane-frame').length).toBe(2)
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

  it('introduces no pane region and the .sstv-lower 50/50 wrapper is gone', async () => {
    await renderView()
    expect(
      document.querySelectorAll('.cockpit-panes').length,
      'SSTV grew a pane region: two content blocks cannot fill a multi-track template',
    ).toBe(0)
    expect(document.querySelectorAll('.cockpit-col').length).toBe(0)
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

// (The computed-winner check that the sheet's `.sstv-live-canvas` width actually consumes
// the stamp lives in cockpit-shells.test.ts, beside the file's other cascade computers —
// this jsdom file cannot read styles.css: import.meta.url is not file-scheme here.)
