// @vitest-environment jsdom
//
// THE STOP LINE, COMPUTED, OVER JS8'S ARRANGEMENTS — one of the stop-line sweep files (2026-10-07).
// The rule, what a case's `stopControls` may hold and what the sweeps do not prove are in the header of
// stop-line.test.tsx. This file runs JS8's arrangement sweep (stop-line.testkit.tsx `arrangementRun`:
// fifty seeded placements of its panes, each with every id hidden singly and all at once, every stop
// control on screen and no more disabled than with nothing hidden), on a worker of its own.
// stop-line.test.tsx checks that this file exists and sweeps this cockpit.
import { describe, it, vi, beforeEach, afterEach } from 'vitest'
import { cleanup } from '@testing-library/react'
import { ARRANGEMENT_RUN_BUDGET_MS, arrangementRun, arrangementRuns, js8 } from './stop-line.testkit'

// The api answers every stop-line sweep file shares (stop-line.api.testkit.ts says why).
vi.mock('../api', async (importOriginal) =>
  (await import('./stop-line.api.testkit')).stopLineApi(await importOriginal<Record<string, unknown>>()),
)

vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas/scope children only. CockpitHeader is DELIBERATELY REAL — see the file header.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
// A BOX's body is a Conditions box, and none of those holds a transmit control (DashRail.test.tsx
// places every box in the registry and finds none). Stubbed, so each mount measures the cockpit
// rather than twenty-nine feeds; the boxes' frames, pickers and ✕ are real, and so is where they stand.
vi.mock('./panes/BoxBody', () => ({ BoxBody: () => <div data-testid="box-body-stub" /> }))

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('THE ARRANGEMENT SWEEP: no placement of the panes gates a control that stops a transmission', () => {
  // Fifty placements in five runs of ten, each its own test with its own budget (ARRANGEMENT_RUN_BUDGET_MS).
  it.each(arrangementRuns(js8))(
    '%s: %s, every id hidden singly and all at once, every stop control where it was',
    async (_name, _what, c, k) => arrangementRun(c, k),
    ARRANGEMENT_RUN_BUDGET_MS,
  )
})
