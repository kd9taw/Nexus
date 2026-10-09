// @vitest-environment jsdom
//
// THE STOP LINE, COMPUTED, OVER PHONE'S ARRANGEMENTS — one of the stop-line sweep files (2026-10-07).
// The rule, what a case's `stopControls` may hold and what the sweeps do not prove are in the header of
// stop-line.test.tsx. This file runs Phone's arrangement sweep (stop-line.testkit.tsx `arrangementRun`:
// fifty seeded placements of its panes, each with every id hidden singly and all at once, every stop
// control on screen and no more disabled than with nothing hidden), and its left side sweep, on a worker
// of its own. stop-line.test.tsx checks that this file exists and sweeps this cockpit.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup } from '@testing-library/react'
import { ALL_PANEL_VOCABULARIES, PHONE_PANEL_IDS } from '../features/panelState'
import type { PanelLayoutApi } from '../features/panelState'
import type { ArrangeSpec } from '../features/panelPlace'
import {
  ARRANGEMENT_RUN_BUDGET_MS,
  arrangementRun,
  arrangementRuns,
  panelsWith,
  phone,
  randomArrangements,
  settle,
  stopsOnScreen,
} from './stop-line.testkit'

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
  it.each(arrangementRuns(phone))(
    '%s: %s, every id hidden singly and all at once, every stop control where it was',
    async (_name, _what, c, k) => arrangementRun(c, k),
    ARRANGEMENT_RUN_BUDGET_MS,
  )
})

describe('THE LEFT SIDE SWEEP (2026-10-03): no arrangement with Phone’s left side gates a control that stops a transmission', () => {
  // The side stands between the header and the dock, beside the scope, the TX strip and the region;
  // the strip moves into the stage beside it. Swept on a window wide enough for the side, with every
  // id hidden singly and all at once, as the arrangement sweep above does for the columns.
  afterEach(() => document.documentElement.style.removeProperty('--vw-eff'))

  // Thirty arrangements in three runs of ten, each its own test (the arrangement sweep's reason).
  it.each([0, 1, 2].map((k) => [k * 10 + 1, k * 10 + 10, k] as const))(
    'Phone: random arrangements %i–%i of 30 on a wide window, every id hidden singly and all at once, every stop control where it was',
    async (_from, _to, k) => {
      document.documentElement.style.setProperty('--vw-eff', '1600px')
      const spec = ALL_PANEL_VOCABULARIES.find((v) => v.view === 'phone')!.arrange! as ArrangeSpec<string>
      phone.render(panelsWith<(typeof PHONE_PANEL_IDS)[number]>([]))
      await settle()
      const shown = stopsOnScreen(phone.stopControls)
      const baseline = new Map(phone.stopControls.map(([label]) => [label, shown.get(label)!.some((e) => !e.disabled)]))
      cleanup()
      let sided = 0
      for (const [j, arr] of randomArrangements(spec, 30, 20261003).slice(k * 10, k * 10 + 10).entries()) {
        const i = k * 10 + j
        const combos: Array<readonly string[]> = [[], ...phone.ids.map((id: string) => [id]), [...phone.ids]]
        for (const removed of combos) {
          ;(phone.render as (p: PanelLayoutApi<string>) => void)(panelsWith(removed, arr.place, arr.leftSide))
          await settle()
          if (removed.length === 0 && document.querySelector('.cockpit-left')) sided++
          const on = stopsOnScreen(phone.stopControls)
          for (const [label] of phone.stopControls) {
            const els = on.get(label)!
            const where = `Phone, arrangement #${i} ${JSON.stringify(arr)}, hiding {${removed.join(', ')}}`
            expect(els.length, `${where} took "${label}" with it`).toBeGreaterThan(0)
            expect(els.some((e) => !e.disabled), `${where} left "${label}" on screen but DISABLED`).toBe(baseline.get(label))
            // …and none of them is on the side.
            expect(els.some((e) => e.closest('.cockpit-left') != null), `${where} put "${label}" on the left side`).toBe(false)
          }
          cleanup()
        }
      }
      expect(sided, 'the random arrangements never put the side on screen — the sweep is reading nothing').toBeGreaterThan(3)
    },
    // A run of ten, measured with the arrangement sweep: 7.8 s alone, 44 s at a fifth of a CPU and
    // 97 s at a tenth.
    240_000,
  )
})
