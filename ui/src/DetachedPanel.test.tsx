// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { selectPeer, workSpot, setFrequency, getNeedAlerts } from './api'
import { readEnabledModes } from './useFeatures'
import type { NeedAlert } from './types'

// Stub the heavy map view — we only need the deselect seam it exposes. In the real
// map, an empty-space click, re-clicking the selected dot, and the Selection pane's
// ✕ all funnel through onSelectCall(null); this button stands in for that path.
vi.mock('./components/ConnectView', () => ({
  ConnectView: ({
    onSelectCall,
    needByCall,
  }: {
    onSelectCall: (c: string | null) => void
    needByCall?: Map<string, string>
  }) => (
    <>
      <button data-testid="deselect" onClick={() => onSelectCall(null)}>
        deselect
      </button>
      {/* The colour the POP-OUT computed, surfaced so a test can see it. */}
      <span data-testid="need-dl1abc">{needByCall?.get('DL1ABC') ?? '-'}</span>
    </>
  ),
}))

// Stub the Needed board — expose ONLY its onWork seam. The button fires a freq-less
// Phone need on 20m; DetachedPanel.onWork routes it via workSpot (rig mode switch) or,
// when the Phone cockpit is a DISABLED feature, a plain QSY (setFrequency) — the bug.
vi.mock('./components/NeededPanel', () => ({
  NeededPanel: ({ onWork }: { onWork: (a: NeedAlert) => void }) => (
    <>
      <button
        data-testid="work"
        onClick={() =>
          onWork({ call: 'DX1ABC', band: '20m', mode: 'Phone', freqMhz: null } as NeedAlert)
        }
      >
        work
      </button>
      <button
        data-testid="work-ft4"
        onClick={() =>
          onWork({ call: 'DX2DEF', band: '20m', mode: 'FT4', freqMhz: 14.083 } as NeedAlert)
        }
      >
        work ft4
      </button>
    </>
  ),
}))

// Control the enabled-modes source the guard reads — the SAME source the docked board
// (App.tsx handleWorkNeeded / nav-hint effect) derives cwEnabled/phoneEnabled from.
vi.mock('./useFeatures', () => ({
  readEnabledModes: vi.fn(() => ({ cw: true, phone: true })),
}))

// Stub the waterfall strip — the branch under test is its WRAPPER (class list / Toasts
// placement), not the canvas/spectrum plumbing.
vi.mock('./components/Waterfall', () => ({
  Waterfall: () => <canvas data-testid="wf" />,
}))

// Engine calls under test are selectPeer, workSpot, and setFrequency; the other mount-time
// pollers just need to resolve to something harmless so the effects settle.
vi.mock('./api', () => ({
  subscribeSnapshot: vi.fn(() => () => {}),
  selectPeer: vi.fn(() => Promise.resolve(null)),
  // A populated 20m channel so qsyBand can resolve a dial (the guard's QSY path).
  getBandPlan: vi.fn(() =>
    Promise.resolve([
      { band: '20m', group: 'HF', dialMhz: 14.074, mode: 'USB', label: '', note: '' },
    ]),
  ),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([])),
  getSettings: vi.fn(() => Promise.resolve(null)),
  pointRotatorAtCall: vi.fn(() => Promise.resolve(null)),
  workSpot: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
}))

const mockedSelectPeer = vi.mocked(selectPeer)
const mockedWorkSpot = vi.mocked(workSpot)
const mockedSetFrequency = vi.mocked(setFrequency)
const mockedReadEnabledModes = vi.mocked(readEnabledModes)

beforeEach(() => {
  mockedSelectPeer.mockClear()
  mockedWorkSpot.mockClear()
  mockedSetFrequency.mockClear()
  mockedReadEnabledModes.mockReset()
  mockedReadEnabledModes.mockReturnValue({ cw: true, phone: true })
})

// Unmount between cases — this project runs vitest without globals, so RTL's
// auto-cleanup isn't registered; without it a second render duplicates testids.
afterEach(() => cleanup())

describe('DetachedPanel need colours match the docked window', () => {
  // The stranded-fix regression, at the surface that had it: the pop-out built
  // its own need map with the pre-fix last-wins loop, and the backend hands
  // alerts out priority-DESCENDING, so a call that is a NEW ENTITY on 20 m and
  // merely wants a confirmation on 40 m was painted the dim Confirm colour on
  // the torn-off map while the docked window showed NewEntity. Asserting
  // through the RENDERED pop-out is what makes this fail if the hand-rolled
  // map ever comes back — a helper-only test cannot see it.
  it('colours a multi-band call from its STRONGEST need', async () => {
    const alerts = [
      { call: 'DL1ABC', band: '20m', mode: 'Ft8', tags: ['NewEntity'], priority: 100 },
      { call: 'DL1ABC', band: '40m', mode: 'Ft8', tags: ['Confirm'], priority: 10 },
    ] as unknown as NeedAlert[]
    vi.mocked(getNeedAlerts).mockResolvedValueOnce(alerts)
    await act(async () => {
      render(<DetachedPanel panel="connect" />)
    })
    expect(screen.getByTestId('need-dl1abc').textContent).toBe('NewEntity')
  })
})

describe('DetachedPanel selection forwarding', () => {
  // Regression: the pop-out onSelect had an `if (call)` guard that silently swallowed
  // null, so a deselect in a torn-off Connect window was impossible — and because the
  // selection is engine-shared, it stayed stuck in the main window too.
  it('forwards a null (deselect) to the shared engine, not just non-null picks', () => {
    render(<DetachedPanel panel="connect" />)
    fireEvent.click(screen.getByTestId('deselect'))
    expect(mockedSelectPeer).toHaveBeenCalledWith(null)
  })
})

describe('DetachedPanel waterfall branch', () => {
  // Regression: this was the ONE pop-out branch missing the `app` class — `zoom` lives
  // on `.app` (styles.css), so the torn-off waterfall ignored the operator's UI scale
  // entirely, and its toast viewport measured a --vh-eff computed for a zoom that never
  // applied (a 430px toast cap in a 300px window).
  it('wraps the torn-off waterfall in the zoomed .app tree, Toasts inside the same subtree', () => {
    const { container } = render(<DetachedPanel panel="waterfall" />)
    const root = container.firstElementChild as HTMLElement
    expect(root.className).toBe('app detached detached-waterfall')
    // Toasts sit INSIDE the zoomed tree, so they scale with the UI like every other branch.
    expect(root.querySelector('.ui-toast-viewport')).not.toBeNull()
  })
})

describe('DetachedPanel Needed board work-guard', () => {
  // Regression: the docked board only QSYs (no rig mode switch) when the target cockpit
  // is a DISABLED feature — otherwise the main window's nav-hint effect refuses to follow
  // and the rig silently enters a hidden mode with no UI. The detached board's onWork had
  // no such guard; it must mirror App.tsx handleWorkNeeded.
  it('phone-disabled: QSYs to the spot instead of switching the rig into the hidden Phone cockpit', async () => {
    mockedReadEnabledModes.mockReturnValue({ cw: true, phone: false })
    render(<DetachedPanel panel="needed" />)
    // Let the mount pollers settle (getBandPlan → setBandPlan) so qsyBand can resolve the dial.
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work'))
    expect(mockedWorkSpot).not.toHaveBeenCalled()
    expect(mockedSetFrequency).toHaveBeenCalled()
  })

  it('phone-enabled: works the spot (band + mode + freq) as before', async () => {
    mockedReadEnabledModes.mockReturnValue({ cw: true, phone: true })
    render(<DetachedPanel panel="needed" />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work'))
    // 5th arg: no tier for a Phone spot — the tier rides only digital FT8/FT4 clicks.
    expect(mockedWorkSpot).toHaveBeenCalledWith('phone', 14.25, '20m', 'DX1ABC', undefined)
    expect(mockedSetFrequency).not.toHaveBeenCalled()
  })

  // The spot-click double-retune (operator report, 2026-08-09: "hitting a default first,
  // then switching"): a digital spot's FT8/FT4 protocol must ride the SAME atomic workSpot
  // call. The pop-out previously passed no tier at all — an FT4 click left the decoder on
  // FT8 — and adding it as a separate set_tier call would recreate the main window's gap,
  // where the radio loop commands the tier's default dial before the spot's frequency.
  it('an FT4 spot carries its tier inside the one atomic workSpot call', async () => {
    render(<DetachedPanel panel="needed" />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    fireEvent.click(screen.getByTestId('work-ft4'))
    expect(mockedWorkSpot).toHaveBeenCalledWith('digital', 14.083, '20m', 'DX2DEF', 'FT4')
    expect(mockedSetFrequency).not.toHaveBeenCalled()
  })
})
