// @vitest-environment jsdom
//
// The Logbook's "Check eQSL sent marks" button: shown only while the station lists a contact
// holding the sent mark an eQSL card left, naming how many, and the only way into the list.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup, fireEvent, screen, within, act } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'
import { clearEqslSentMarks, eqslSentMarkReview, type EqslSentMark } from '../api'
import { t } from '../i18n'

// The house budget (see Logbook.confirmations.test.tsx): a loaded full suite runs cases far slower
// than one core.
vi.setConfig({ testTimeout: 15_000 })

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(), editQsoById: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(), markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
    parkStateReview: vi.fn(async () => []), applyParkStates: noop(),
    startConfirmationCheck: noop(), confirmationCheck: noop(), applyConfirmationCheck: noop(),
    cancelConfirmationCheck: vi.fn(async () => {}),
    eqslSentMarkReview: vi.fn(), clearEqslSentMarks: noop(), restoreEqslSentMarks: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

const review = vi.mocked(eqslSentMarkReview)

function contact(call: string, when: number) {
  return {
    id: `id-${call}`, call, grid: null, band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', state: null, whenUnix: when,
    confirmed: true, awardConfirmed: false, qslRcvd: null,
    qslSent: { sent: true, via: 'E', dateUnix: null }, ota: null, upload: undefined,
  }
}
const LOG = [contact('W1AW', 1_700_000_600), contact('K1ABC', 1_700_000_500)]
const MARKS: EqslSentMark[] = [
  { id: 'id-W1AW', call: 'W1AW', whenUnix: 1_700_000_600, band: '20m', mode: 'FT8' },
  { id: 'id-K1ABC', call: 'K1ABC', whenUnix: 1_700_000_500, band: '20m', mode: 'FT8' },
]

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** The Logbook on the desktop, once its rows show and the station has answered the review. */
async function shown() {
  engineLog.mockResolvedValue(LOG)
  const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() => expect(container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
  await waitFor(() => expect(review).toHaveBeenCalled())
  await act(async () => {
    await Promise.resolve()
  })
  return container
}

describe('Check eQSL sent marks in the Logbook', () => {
  it('is not offered while the station lists none', async () => {
    review.mockResolvedValue([])
    await shown()
    expect(screen.queryByTitle(t('logbook.eqslSentMarks.buttonTitle'))).toBeNull()
  })

  it('names how many the station lists, and opens the list from its button only', async () => {
    review.mockResolvedValue(MARKS)
    await shown()
    expect(screen.queryByRole('dialog', { name: t('logbook.eqslSentMarks.title') })).toBeNull()
    const button = screen.getByRole('button', { name: t('logbook.eqslSentMarks.button', { count: 2 }) })
    expect(button.title).toBe(t('logbook.eqslSentMarks.buttonTitle'))
    fireEvent.click(button)
    const dialog = await screen.findByRole('dialog', { name: t('logbook.eqslSentMarks.title') })
    expect(['W1AW', 'K1ABC'].map((call) => within(dialog).queryByText(call) != null)).toEqual([true, true])
  })

  it('reads the list again after a clear, and goes when none is left', async () => {
    review.mockResolvedValueOnce(MARKS).mockResolvedValue([])
    vi.mocked(clearEqslSentMarks).mockResolvedValue({ ids: ['id-W1AW', 'id-K1ABC'], clearedUnix: 1_780_000_000 })
    await shown()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.eqslSentMarks.button', { count: 2 }) }))
    const dialog = await screen.findByRole('dialog', { name: t('logbook.eqslSentMarks.title') })
    fireEvent.click(within(dialog).getByRole('button', { name: t('logbook.eqslSentMarks.clear', { count: 2 }) }))
    await waitFor(() => expect(screen.queryByTitle(t('logbook.eqslSentMarks.buttonTitle'))).toBeNull())
    expect(vi.mocked(clearEqslSentMarks).mock.calls).toEqual([[['id-W1AW', 'id-K1ABC']]])
    expect(review.mock.calls.length, 'read when the Logbook opened, and again after the clear').toBe(2)
    expect(screen.queryByRole('dialog', { name: t('logbook.eqslSentMarks.title') })).toBeNull()
  })
})
