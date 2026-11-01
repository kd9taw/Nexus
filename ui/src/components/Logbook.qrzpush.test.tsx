// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'
import { t } from '../i18n'
import { menuItem, openRowMenu } from './Logbook.rowMenu.testkit'

// Same jsdom shims the sibling Logbook suites need: react-virtual measures the scroll
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
// Each test's Logbook unmounted when the test ends. Left mounted, it stays subscribed to the log
// answers still on their way, and one landing after this file's window is gone re-renders it
// there: "window is not defined", counted as an uncaught error against the run.
afterEach(cleanup)

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: vi.fn(() => Promise.resolve({})),
    markQslCardById: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function oneContact() {
  return [
    {
      call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000,
      confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    },
  ]
}

// #270 (ddobbins1686): the per-row QRZ push existed in 1.10.3, but its only visible mark was a
// bare ↥ arrow, so an operator hunting for "Upload to QRZ" reported the control as gone.
describe('Logbook row — the QRZ push is recognisable as QRZ (#270)', () => {
  it('names QRZ Logbook in the row menu', async () => {
    engineLog.mockResolvedValue(oneContact())
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())

    // In the row's ⋯ menu, by the service's full name, so an operator looking for "Upload to
    // QRZ" finds it in words.
    const menu = await openRowMenu('K0ABC')
    const push = menuItem(menu, t('logbook.row.menu.qrz'))
    // Positive control: the sibling Club Log upload is in the same menu with its own name, so a
    // missing QRZ item below is a real absence, not a menu that failed to render.
    expect(menuItem(menu, t('logbook.row.menu.clublog')).textContent).toContain('Club Log')

    expect(push.textContent).toContain('QRZ Logbook')
  })
})
