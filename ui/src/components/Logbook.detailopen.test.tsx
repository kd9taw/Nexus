// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'

vi.mock('../api', () => {
  const noop = () => vi.fn()
  const getLog = vi.fn()
  return {
    getLog,
    // The Logbook reads the shared log store, which asks get_log_delta. Every answer here is
    // the whole log (a valid answer), stocked through `getLog` as before.
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: await getLog() })),
    deleteQso: vi.fn(() => Promise.resolve({})),
    editQso: vi.fn(() => Promise.resolve({ call: 'K1ABC', whenUnix: 1_700_000_100 })),
    exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTag: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: noop(), markQslCard: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

// HOW A CONTACT IS OPENED (#313). The detail view exists; this is the affordance that
// reaches it, and it is INVISIBLE — so it needs pinning harder than a button would.
//
// ⛔ It is a double-click, and that is a MEASURED decision rather than a preference. A tenth
// button in the row-action cluster was built first and the layout harness refused it: ten
// controls need ~318 px of button plus nine gaps against a 336 px track floor, `fits: false`
// at EVERY viewport including the 1024 support floor. Widening the track would take that
// width from the columns at the narrowest size the app claims to support.
//
// ⭐ And single click must stay free: operators select and copy callsigns out of the log.

afterEach(cleanup)
// The log table is virtualised; jsdom has no ResizeObserver and no layout. Same stubs
// the sibling Logbook suites use.
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

const ROW = {
  call: 'DL1ABC', grid: 'JO31AB', band: '20m', freqMhz: 14.074, mode: 'FT8',
  rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
  country: 'Germany', whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false,
  qslRcvd: null, qslSent: null, ota: null, upload: undefined,
}

async function row() {
  vi.mocked(api.getLog).mockResolvedValue([ROW] as never)
  const { container } = render(
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
  )
  await waitFor(() =>
    expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull(),
  )
  return container.querySelector('.logbook-row:not(.head)') as HTMLElement
}

const open = () => document.body.textContent?.includes('JO31AB') ?? false

describe('opening a contact from the log', () => {
  it('opens the detail view on a DOUBLE click', async () => {
    const r = await row()
    expect(open(), 'precondition: nothing is open before the gesture').toBe(false)
    fireEvent.doubleClick(r)
    await waitFor(() => expect(open()).toBe(true))
  })

  it('does NOT open on a single click, so a callsign can still be selected', async () => {
    const r = await row()
    fireEvent.click(r)
    // Deliberately asserted immediately AND after a settle: a dialog that opened one tick
    // later would pass a bare synchronous check.
    expect(open()).toBe(false)
    await new Promise((res) => setTimeout(res, 30))
    expect(open(), 'a single click must leave the row alone').toBe(false)
  })

  it('adds no tenth control to the action cluster', async () => {
    // The guard in styles-logbook-actions.test.tsx owns the width budget; this states the
    // intent next to the feature that would otherwise have broken it.
    const r = await row()
    const cluster = r.querySelector('.log-rowactions')
    expect(cluster, 'precondition: there is a cluster to count').not.toBeNull()
    const labels = [...cluster!.querySelectorAll('.log-rowbtn')].map((e) => e.textContent ?? '')
    expect(labels, 'the detail view must not have grown the cluster').not.toContain('🔍')
  })
})
