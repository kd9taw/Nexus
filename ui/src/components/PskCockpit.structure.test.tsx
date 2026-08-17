// @vitest-environment jsdom
//
// PSK COCKPIT SHELL STRUCTURE (Keyboard Modes Phase 1 — RECEIVE ONLY).
//
// PSK follows RTTY's region-less shape of the pane-grid contract: one operator-content
// block (the decoded stream) through CockpitPaneFrame, deliberately NO pane region —
// and, unlike RTTY, deliberately NO TX DOCK, because no PSK transmit path exists in the
// engine this phase. THE STOP LINE therefore holds BY CONSTRUCTION (the APRS way): with
// nothing on the screen able to start a transmission there is nothing a stop control
// would stop, and the no-TX-control census below is what pins that claim to the DOM.
// When Phase 2 brings PSK TX, it must replace that census with the RTTY-style dock +
// latch + Esc structure AND a stop-line.test.tsx sweep entry — not amend it.
//
// The other half of what these tests pin: the view-entry AUTO-ARM wiring (operator
// ruling 2026-08-17) — entry calls pskAutoArm exactly once per activation edge (the
// engine owns the decline memory and the Settings opt-out; the view must call the
// policy, never reimplement it), and the pane's Arm button is the explicit
// pskArm(false) stop the engine remembers.
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
  },
}

vi.mock('../api', () => ({
  getPskState: vi.fn(async () => state.current),
  pskArm: vi.fn(async () => state.current),
  pskAutoArm: vi.fn(async () => state.current),
  pskClear: vi.fn(async () => state.current),
  pskAfcReset: vi.fn(async () => state.current),
  pskNet: vi.fn(async () => state.current),
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
    txEnabled: false,
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
  state.current = { ...state.current, armed: true, signal: false }
  pskAutoArm.mockClear()
  pskArm.mockClear()
})
afterEach(cleanup)

describe('PskCockpit pane shell', () => {
  it('the shell holds no child kinds beyond the census — and NO TX dock', async () => {
    // PSK's sanctioned kinds: header chrome, the waterfall, the ONE content frame.
    // No .cockpit-txdock: this cockpit is receive-only and a dock is where TX
    // controls would live. A new shell-level sibling updates this census
    // deliberately or does not ship.
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.psk-cockpit')!
    expect(shell).not.toBeNull()
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.pane-frame']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(shell.querySelectorAll(':scope > .pane-frame').length).toBe(1)
    expect(document.querySelector('.cockpit-txdock'), 'an RX-only cockpit grew a TX dock').toBeNull()
  })

  it('renders NO control that starts or stops a transmission (THE STOP LINE, by construction)', async () => {
    // The DOM half of the panelState.ts PSK vocabulary comment: no send box, no
    // macros, no TX latch, no PTT, no Stop TX — nothing to key the rig and
    // therefore nothing whose reachability the stop-line sweeps must guard.
    // Two layers, so a renamed class cannot slip a sender past the census:
    await renderCockpit()
    for (const sel of ['.cw-send', '.cw-type', '.cw-send-btn', '.rtty-stop', '.rtty-tx-latch', '.cw-macros']) {
      expect(document.querySelector(sel), `${sel} is TX furniture and must not render here`).toBeNull()
    }
    // …and by accessible name over every rendered button (the header is mocked
    // out, but its census is CockpitHeader's own: this cockpit passes it no
    // onStopTx / onSetTxEnabled / onTune / power, pinned by the source check below).
    for (const btn of Array.from(document.querySelectorAll('button'))) {
      const name = (btn.getAttribute('aria-label') ?? btn.textContent ?? '').trim()
      expect(
        /send|transmit|ptt|macro|\btx\b|\btune\b/i.test(name),
        `button "${name}" reads as a TX control in a receive-only cockpit`,
      ).toBe(false)
    }
  })

  it('passes the header none of the TX affordances (source pin — the header is mocked above)', async () => {
    // The census test mocks CockpitHeader, so this reads the component source the
    // way the shells suite reads AprsCockpit's class list: the RX-only cockpit
    // must not hand the real header a Stop TX, a TX-enable latch, a Tune key or
    // a power block, and must keep the TX/RX pill off.
    const { readFileSync } = await import('node:fs')
    const { resolve } = await import('node:path')
    // cwd is ui/ under vitest; import.meta.url is not a file: URL in the jsdom env.
    const src = readFileSync(resolve(process.cwd(), 'src/components/PskCockpit.tsx'), 'utf8')
    // Scope to the CockpitHeader element (up to the waterfall block): the
    // Waterfall's own onTune is click-to-NET — an RX control — and must not
    // trip a whole-file scan.
    const start = src.indexOf('<CockpitHeader')
    const end = src.indexOf('THE BAND WATERFALL')
    expect(start, 'PskCockpit no longer renders CockpitHeader — rescope this pin').toBeGreaterThan(-1)
    expect(end, 'the waterfall landmark comment moved — rescope this pin').toBeGreaterThan(start)
    const header = src.slice(start, end)
    expect(header).toContain('txState={false}')
    for (const forbidden of ['onStopTx', 'onSetTxEnabled', 'onTune', 'onAtuTune', 'power=']) {
      expect(
        header.includes(forbidden),
        `PskCockpit passes ${forbidden} to CockpitHeader — TX furniture in an RX-only cockpit`,
      ).toBe(false)
    }
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

  it("⊞ 'Waterfall' unticked leaves the transcript to take the height, and vice versa", async () => {
    await renderCockpit({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.waterfall-wrap')).toBeNull()
    expect(document.querySelector('[data-pane="stream"]'), 'nothing left to take the freed height').not.toBeNull()
    cleanup()
    await renderCockpit({ panels: fakePanels(['stream']) })
    expect(document.querySelector('[data-pane="stream"]')).toBeNull()
    expect(document.querySelector('.waterfall-wrap'), 'the waterfall is a separate entry').not.toBeNull()
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
    expect(document.querySelector('.cockpit-txdock')).toBeNull()
  })
})

describe('PSK view-entry auto-arm (the APRS/SSTV pattern, operator ruling 2026-08-17)', () => {
  it('entering the view calls the ENGINE policy exactly once per activation edge', async () => {
    // The engine owns the decline memory + the Settings opt-out; the view's whole
    // job is to report the entry. Once per rising edge — a re-render while active
    // must not hammer the command.
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
    // Disarm goes through pskArm(false) — the act the engine latches as the
    // session decline — never through some local state that a remount would lose.
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
