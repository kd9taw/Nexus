// @vitest-environment jsdom
//
// A paper card is queued from the contact's own QSL menu in the Logbook (ADIF QSL_SENT=Q), and
// taken out again from the same menu; a contact already marked sent offers neither, since its
// card went out. "Cards to send" prints the queue (QslCardsToSend).
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup, fireEvent } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'
import { queueQslCardById } from '../api'
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
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: noop(),
    logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]),
    setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    queueQslCardById: vi.fn(async () => ({ kind: 'busy' })),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function logRow(extra: [string, string][], sent = false) {
  return {
    id: 'id-K0ABC', call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: { sent, via: sent ? 'B' : null, dateUnix: null }, ota: null, upload: undefined,
    extra,
  }
}

async function qslMenu(extra: [string, string][], sent = false) {
  engineLog.mockResolvedValue([logRow(extra, sent)])
  const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  const aria = t('logbook.row.qslSent.aria', { call: 'K0ABC' })
  await waitFor(() => expect(container.querySelector(`select[aria-label="${aria}"]`)).not.toBeNull())
  const select = container.querySelector(`select[aria-label="${aria}"]`) as HTMLSelectElement
  const options = [...select.options].map((o) => [o.value, o.textContent])
  return { select, options }
}

describe('queueing a paper card from the Logbook', () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  it('offers to queue a card for a contact not sent, and queues it', async () => {
    const { select, options } = await qslMenu([])
    expect(options).toContainEqual(['Q', t('logbook.row.qslQueue.add')])
    expect(options.map((o) => o[0])).not.toContain('q')
    fireEvent.change(select, { target: { value: 'Q' } })
    await waitFor(() => expect(queueQslCardById).toHaveBeenCalledTimes(1))
    expect(vi.mocked(queueQslCardById).mock.calls[0][0]).toMatchObject({ id: 'id-K0ABC' })
    expect(vi.mocked(queueQslCardById).mock.calls[0][1]).toBe(true)
  })

  it('offers to take a queued or requested card out, and takes it out', async () => {
    for (const v of ['Q', 'R']) {
      const { select, options } = await qslMenu([['QSL_SENT', v]])
      expect(options).toContainEqual(['q', t('logbook.row.qslQueue.remove')])
      expect(options.map((o) => o[0])).not.toContain('Q')
      fireEvent.change(select, { target: { value: 'q' } })
      await waitFor(() => expect(queueQslCardById).toHaveBeenCalledTimes(1))
      expect(vi.mocked(queueQslCardById).mock.calls[0][1]).toBe(false)
      cleanup()
      vi.clearAllMocks()
    }
  })

  it('offers neither once the card is marked sent', async () => {
    const { options } = await qslMenu([['QSL_SENT', 'Q']], true)
    expect(options.map((o) => o[0]).filter((v) => v === 'Q' || v === 'q')).toEqual([])
  })
})
