// @vitest-environment jsdom
//
// The Logbook's "Check confirmations" button: the only way into the check. It never opens or runs
// by itself, opening it downloads nothing until the operator presses Check, and a Remote browser,
// which reads the log from the station, is not offered it.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup, fireEvent, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'
import { confirmationCheck } from '../api'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import type { QueryPage } from '../remote-web/application-query-protocol'
import { t } from '../i18n'

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
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function contact(call: string, when: number) {
  return {
    id: `id-${call}`, call, grid: null, band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', state: null, whenUnix: when,
    confirmed: true, awardConfirmed: true, qslRcvd: null, qslSent: null, ota: null, upload: undefined,
  }
}
const LOG = [contact('W1AW', 1_700_000_600), contact('K1ABC', 1_700_000_500)]

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('Check confirmations in the Logbook', () => {
  it('opens only from its button, and downloads nothing until Check is pressed', async () => {
    engineLog.mockResolvedValue(LOG)
    const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
    expect(screen.queryByRole('dialog', { name: t('logbook.confirmations.title') })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.confirmations.button') }))
    const dialog = await screen.findByRole('dialog', { name: t('logbook.confirmations.title') })
    expect(dialog.textContent).toContain(t('logbook.confirmations.intro'))
    expect(vi.mocked(confirmationCheck)).not.toHaveBeenCalled()
  })

  it('is not offered to a Remote browser, even one holding the station’s control', async () => {
    const page = (): QueryPage => ({ type: 'applicationPage', requestId: 'r', collection: 'log', snapshotId: 's', offset: 0,
      total: LOG.length, retained: LOG.length, nextCursor: null, ageMs: 0, meta: {},
      rows: LOG as unknown as QueryPage['rows'] })
    const { container } = render(
      <StationControlContext.Provider value={true}>
        <RemoteCollectionsContext.Provider value={{ page: async () => page() } as unknown as RemoteCollections}>
          <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
        </RemoteCollectionsContext.Provider>
      </StationControlContext.Provider>,
    )
    expect(
      await screen.findByRole('button', { name: t('remote.refreshCollection') }),
      'the Remote log is the one on screen',
    ).toBeTruthy()
    expect(container.querySelector('.log-actions'), 'and so is the toolbar the button sits in').not.toBeNull()
    expect(screen.queryByRole('button', { name: t('logbook.confirmations.button') })).toBeNull()
  })
})
