// @vitest-environment jsdom
//
// A park or summit contact with no state counts for no state in Worked All States. A park on a
// state line is logged that way on purpose (Nexus never guesses which state the activator was
// in), so the Logbook marks every such contact where a state applies, and says why.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup, fireEvent, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'
import { parkStateReview } from '../api'
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
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function contact(call: string, when: number, over: Record<string, unknown>) {
  return {
    id: `id-${call}`, call, grid: null, band: '20m', freqMhz: 14.285, mode: 'SSB',
    rstSent: '59', rstRcvd: '59', name: null, qth: null, comment: null, notes: null,
    country: 'United States', state: null, whenUnix: when,
    confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    ...over,
  }
}
const park = (ref: string) => ({ theirProgram: 'POTA', theirRef: ref })

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('a park contact with no state is marked in the Logbook', () => {
  it('marks it only where a state applies and none is logged', async () => {
    engineLog.mockResolvedValue([
      contact('W8LINE', 1_700_000_600, { ota: park('US-0003') }),
      contact('W8ND', 1_700_000_500, { ota: park('US-0001'), state: 'ND' }),
      contact('DL1ABC', 1_700_000_400, { ota: park('DE-0001'), country: 'Fed. Rep. of Germany' }),
      contact('W8HOME', 1_700_000_300, {}),
      contact('K0UNK', 1_700_000_200, { ota: park('US-0002'), country: null }),
      contact('VE3XYZ', 1_700_000_100, { ota: park('CA-0001'), country: 'Canada' }),
    ])
    const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
    await waitFor(() => expect(container.querySelectorAll('.logbook-row:not(.head):not(.placeholder)').length).toBe(6))
    const marked = Array.from(container.querySelectorAll('.logbook-row:not(.head):not(.placeholder)'))
      .filter((row) => row.querySelector('.log-park-nostate'))
      .map((row) => ['W8LINE', 'W8ND', 'DL1ABC', 'W8HOME', 'K0UNK', 'VE3XYZ'].find((c) => row.textContent?.includes(c)))
    expect(marked.sort()).toEqual(['K0UNK', 'VE3XYZ', 'W8LINE'])
    const mark = container.querySelector('.log-park-nostate') as HTMLElement
    expect(mark.getAttribute('role')).toBe('img')
    expect(mark.getAttribute('aria-label')).toMatch(/no state/i)
  })
})

describe('Check park states', () => {
  it('opens only from its button, and reading the check is all it does until Apply', async () => {
    engineLog.mockResolvedValue([contact('W8LINE', 1_700_000_600, { ota: park('US-0003') })])
    const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
    await waitFor(() => expect(container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
    expect(screen.queryByRole('dialog', { name: t('logbook.parkStates.title') })).toBeNull()
    expect(vi.mocked(parkStateReview)).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.parkStates.button') }))
    expect(await screen.findByRole('dialog', { name: t('logbook.parkStates.title') })).toBeTruthy()
    await waitFor(() => expect(vi.mocked(parkStateReview)).toHaveBeenCalledTimes(1))
  })
})
