// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, waitFor, fireEvent } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'

// Same jsdom shims the sibling Logbook suite needs: react-virtual measures the scroll
// element and rows via offsetHeight + a ResizeObserver, neither of which jsdom implements.
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    getLog: vi.fn(),
    deleteQso: noop(), editQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: vi.fn(() => Promise.resolve({})),
    markQslCard: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

// One contact whose QSL request is ALREADY marked sent — the state a mis-click leaves behind.
function sentLog() {
  return [
    {
      call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000,
      confirmed: false, awardConfirmed: false,
      qslRcvd: null,
      qslSent: { sent: true, via: 'D', dateUnix: 1_700_000_000 },
      ota: null, upload: undefined,
    },
  ]
}

async function renderWithSentQsl() {
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(sentLog())
  const { container } = render(
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
  )
  await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())
  const select = container.querySelector('.logbook-row:not(.head) select') as HTMLSelectElement
  expect(select).toBeTruthy()
  return { container, select }
}

// #180 (rgoiko): the RECEIVED side has a clear entry ("r") the moment a card is on the
// record, so a mis-tick is reversible. The SENT side has none — once `qslSent.sent` is
// true the three SEND options vanish and nothing puts the row back. From the operator's
// chair a mis-click on Bureau/Direct/Electronic is permanent.
describe('QSL sent — undoing a mis-click (#180)', () => {
  it('offers a clear entry while the request is marked sent', async () => {
    const { select } = await renderWithSentQsl()

    // Positive control: the RECEIVED side's send-independent entry IS here, so the menu
    // really rendered and a missing SENT clear is a real absence, not a dead query.
    const values = [...select.options].map((o) => o.value)
    expect(values).toContain('R')
    // The three SEND options are correctly hidden — you cannot send twice.
    expect(values).not.toContain('B')
    expect(values).not.toContain('D')
    expect(values).not.toContain('E')

    // ...and the clear must be reachable in exactly this state. A clear that is itself
    // hidden once sent is the same bug with extra steps.
    expect(values).toContain('s')
  })

  it('clears the sent mark when that entry is chosen', async () => {
    const { select } = await renderWithSentQsl()
    fireEvent.change(select, { target: { value: 's' } })
    await waitFor(() =>
      expect(api.markQslSent as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(0, null),
    )
  })

  // The other direction of the same guard: an undo offered before there is anything to undo
  // is noise in a menu an operator uses one-handed with a stack of cards. Shown alone, the
  // assertion above would also pass on an option that is simply always there.
  it('does not offer the clear before anything has been sent', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(
      sentLog().map((r) => ({ ...r, qslSent: null })),
    )
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())
    const select = container.querySelector('.logbook-row:not(.head) select') as HTMLSelectElement
    const values = [...select.options].map((o) => o.value)
    // The three sends are back — so this really is the not-yet-sent state.
    expect(values).toEqual(expect.arrayContaining(['B', 'D', 'E']))
    expect(values).not.toContain('s')
  })
})

