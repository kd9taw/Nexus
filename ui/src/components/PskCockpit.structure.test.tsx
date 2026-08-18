// @vitest-environment jsdom
//
// PSK COCKPIT SHELL STRUCTURE (Keyboard Modes Phase 2 — RX + TX).
//
// PSK follows RTTY's region-less shape of the pane-grid contract: one operator-content
// block (the decoded stream) through CockpitPaneFrame, deliberately NO pane region, and
// — since Phase 2 — the pinned .cockpit-txdock hosting every transmit control (macros,
// the continuous-TX latch, Esc/Stop, the compose bar, the ALC drive hint). THE STOP LINE
// is no longer held "by construction" (the Phase 1 no-TX census is gone, as that census's
// own header demanded): it is held the way RTTY holds it — Stop TX + the TX-enable latch
// in the header, the Esc/Stop macro in the dock, none with a ⊞ id — and the WIRING sweep
// for that lives in stop-line.test.tsx's PSK case. What THIS file pins is the shell
// census, the dock placement (transmit controls never inside a pane), and the view-entry
// auto-arm wiring (engine-owned policy, called once per activation edge).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { PskCockpit } from './PskCockpit'
import * as api from '../api'
import type { AppSnapshot, PskState } from '../types'
import type { PanelLayoutApi, PskPanelId } from '../features/panelState'

const state: { current: PskState } = {
  current: {
    armed: true,
    afcHz: 0,
    signal: false,
    centerHz: 1000,
    text: 'CQ CQ de KD9TAW',
    charConf: [],
    sending: false,
    latched: false,
    keyerError: null,
  },
}

vi.mock('../api', () => ({
  getPskState: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  pskArm: vi.fn(async () => state.current),
  pskAutoArm: vi.fn(async () => state.current),
  pskClear: vi.fn(async () => state.current),
  pskAfcReset: vi.fn(async () => state.current),
  pskNet: vi.fn(async () => state.current),
  pskSend: vi.fn(async () => state.current),
  pskSetLatched: vi.fn(async () => state.current),
  pskType: vi.fn(async () => state.current),
  pskStop: vi.fn(async () => state.current),
  haltTx: vi.fn(async () => ({})),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({
  // Capture the cadence prop: the liveliness pin below asserts PSK runs the waterfall
  // at the live-instrument 50 ms cadence (the RTTY value), not the FT default.
  Waterfall: (p: { rowMs?: number }) => <div className="waterfall-wrap" data-rowms={p.rowMs} />,
}))

const pskAutoArm = api.pskAutoArm as ReturnType<typeof vi.fn>
const pskArm = api.pskArm as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.07,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    txAllowed: true,
  },
} as unknown as AppSnapshot

function fakePanels(removed: PskPanelId[] = []): PanelLayoutApi<PskPanelId> {
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

async function renderCockpit(props: Partial<Parameters<typeof PskCockpit>[0]> = {}) {
  const r = render(<PskCockpit snap={snap} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

beforeEach(() => {
  state.current = {
    ...state.current,
    armed: true,
    signal: false,
    sending: false,
    latched: false,
    keyerError: null,
  }
  pskAutoArm.mockClear()
  pskArm.mockClear()
})
afterEach(cleanup)

describe('PskCockpit pane shell', () => {
  it('the shell holds no child kinds beyond the census', async () => {
    // PSK's sanctioned kinds since Phase 2: header chrome, the waterfall, the
    // keyer-error banner, the ONE content frame, and the TX dock — RTTY's
    // census exactly. A new shell-level sibling updates this deliberately or
    // does not ship.
    state.current = { ...state.current, keyerError: 'the rig didn’t accept PTT' }
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.psk-cockpit')!
    expect(shell).not.toBeNull()
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.cw-keyer-warn', '.pane-frame', '.cockpit-txdock']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(document.querySelector('.cw-keyer-warn'), 'warn banner did not render — census untested').not.toBeNull()
    expect(shell.querySelectorAll(':scope > .pane-frame').length).toBe(1)
    expect(shell.querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
  })

  it('the waterfall polls at the live-instrument cadence (50 ms), not the FT default', async () => {
    await renderCockpit()
    expect(document.querySelector('.waterfall-wrap')!.getAttribute('data-rowms')).toBe('50')
  })

  it('the decoded stream renders through a CockpitPaneFrame with its decoder controls', async () => {
    await renderCockpit()
    const pane = document.querySelector('[data-pane="stream"]')
    expect(pane, 'the stream is not framed').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.querySelector('.cw-decode-text')).not.toBeNull()
    expect(pane!.querySelector('.rtty-arm')).not.toBeNull()
  })

  it('introduces no pane region (one content pane cannot fill a multi-track template)', async () => {
    await renderCockpit()
    expect(document.querySelectorAll('.cockpit-panes').length).toBe(0)
    expect(document.querySelectorAll('.cockpit-col').length).toBe(0)
  })

  it('every transmit control lives in the pinned TX dock — never inside a pane', async () => {
    await renderCockpit()
    const dock = document.querySelector('.cockpit-txdock')
    expect(dock, 'no .cockpit-txdock').not.toBeNull()
    for (const sel of [
      '.psk-macros', // F-key macros + their-call + TX latch + Stop
      '.psk-tx-latch', // the continuous-TX latch (a SENDER, not a stop)
      '.psk-stop', // the Esc/Stop abort specifically (on the stop-line census)
      '.cw-send', // compose bar
      '.cw-type',
      '.cw-send-btn',
      '.rtty-hiscall',
      '.psk-drive-hint', // the ALC/IMD drive sentence (the plan's dock hint)
    ]) {
      const el = document.querySelector(sel)
      expect(el, `${sel} missing`).not.toBeNull()
      expect(el!.closest('.cockpit-txdock'), `${sel} is not in the TX dock`).not.toBeNull()
      expect(
        el!.closest('.pane-frame'),
        `${sel} is inside a pane frame — a pane scrolls, transmit controls must not`,
      ).toBeNull()
    }
    expect(dock!.querySelector('.pane-frame')).toBeNull()
    // The dock comes AFTER the stream pane in the DOM (pinned at the bottom).
    const pane = document.querySelector('[data-pane="stream"]')!
    expect(pane.compareDocumentPosition(dock!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('the Esc/Stop macro is live from the LATCH, not only from sending audio', async () => {
    // The RTTY rule, pinned on PSK's own dock: `sending` is stamped by the
    // radio loop from audio in flight, so it is false in the tick between the
    // latch going up and the first chunk keying — a census stop control that
    // is mounted and disabled then is the same loss as one that is gone.
    state.current = { ...state.current, sending: false, latched: false }
    await renderCockpit()
    let stop = document.querySelector('.psk-stop') as HTMLButtonElement
    expect(stop.disabled, 'idle: Stop has nothing to stop').toBe(true)
    cleanup()
    state.current = { ...state.current, sending: false, latched: true }
    await renderCockpit()
    stop = document.querySelector('.psk-stop') as HTMLButtonElement
    expect(stop.disabled, 'latched but not yet keying: Stop must already be live').toBe(false)
    cleanup()
    state.current = { ...state.current, sending: true, latched: false }
    await renderCockpit()
    stop = document.querySelector('.psk-stop') as HTMLButtonElement
    expect(stop.disabled, 'an over on the air: Stop is live').toBe(false)
  })

  it("⊞ 'Waterfall' unticked leaves the transcript to take the height, and vice versa", async () => {
    await renderCockpit({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.waterfall-wrap')).toBeNull()
    expect(document.querySelector('[data-pane="stream"]'), 'nothing left to take the freed height').not.toBeNull()
    cleanup()
    await renderCockpit({ panels: fakePanels(['stream']) })
    expect(document.querySelector('[data-pane="stream"]')).toBeNull()
    expect(document.querySelector('.waterfall-wrap'), 'the waterfall is a separate entry').not.toBeNull()
    // Stop and Send are not gated by the menu — they have no id to gate.
    expect(document.querySelector('.cockpit-txdock .psk-stop')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock .cw-send-btn')).not.toBeNull()
  })

  it('survives the keep-alive hide/show round trip with its structure intact', async () => {
    const { rerender } = await renderCockpit({ active: true })
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={false} />)
    })
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={true} />)
    })
    expect(document.querySelector('[data-pane="stream"]')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock')).not.toBeNull()
  })
})

describe('PSK view-entry auto-arm (the APRS/SSTV pattern, operator ruling 2026-08-17)', () => {
  it('entering the view calls the ENGINE policy exactly once per activation edge', async () => {
    const { rerender } = await renderCockpit({ active: true })
    expect(pskAutoArm).toHaveBeenCalledTimes(1)
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={true} theme="dark" />)
    })
    expect(pskAutoArm).toHaveBeenCalledTimes(1)
    // Leave and re-enter: a fresh edge, a fresh policy call (the engine decides
    // whether it arms — a session decline makes it a no-op THERE, not here).
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={false} />)
    })
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={true} />)
    })
    expect(pskAutoArm).toHaveBeenCalledTimes(2)
  })

  it('never auto-arms while the view is hidden (the keep-alive host renders it inactive)', async () => {
    await renderCockpit({ active: false })
    expect(pskAutoArm).not.toHaveBeenCalled()
  })

  it("the pane's Arm button is the EXPLICIT stop/start the engine remembers", async () => {
    state.current = { ...state.current, armed: true }
    await renderCockpit()
    const btn = document.querySelector('.rtty-arm')! as HTMLButtonElement
    expect(btn.textContent).toBe('RX armed')
    await act(async () => {
      fireEvent.click(btn)
    })
    expect(pskArm).toHaveBeenCalledWith(false)
    // And starting again is the explicit arm that retires the decline (engine-side).
    state.current = { ...state.current, armed: false }
    cleanup()
    await renderCockpit()
    const btn2 = document.querySelector('.rtty-arm')! as HTMLButtonElement
    expect(btn2.textContent).toBe('Arm RX')
    await act(async () => {
      fireEvent.click(btn2)
    })
    expect(pskArm).toHaveBeenCalledWith(true)
  })
})
