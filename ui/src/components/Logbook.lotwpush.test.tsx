// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, screen, cleanup, fireEvent } from '@testing-library/react'
import { Logbook } from './Logbook'
import type { LogQuestion } from '../features/logAnswers'
import { uploadLotwReportByIds } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

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
afterEach(() => {
  cleanup()
  vi.mocked(uploadLotwReportByIds).mockReset()
  vi.mocked(pushToast).mockReset()
})

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: vi.fn(() => Promise.resolve({})),
    markQslCardById: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: noop(), uploadLotwReportByIds: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (run: () => Promise<unknown>, fallback: string) => {
    try {
      return await run()
    } catch (e) {
      // The real one's words: the fallback, then the reason (toast.ts failureText).
      ;(await import('../toast')).pushToast(`${fallback}: ${e instanceof Error ? e.message : String(e)}`, 'error')
      return null
    }
  }),
}))

function oneContact(over: Record<string, unknown> = {}) {
  return [
    {
      call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000,
      confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
      ...over,
    },
  ]
}

async function lotwButton(over: Record<string, unknown> = {}) {
  engineLog.mockResolvedValue(oneContact(over))
  const { container } = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() => expect(container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
  // Positive control: the sibling ClubLog push renders on the same row, so a missing LoTW
  // button below is a real absence, not a row that failed to render.
  expect(screen.getByRole('button', { name: 'Push K0ABC to ClubLog' }).textContent).toBe('CL')
  return screen.findByRole('button', { name: t('logbook.row.pushLotw.aria', { call: 'K0ABC' }) })
}

/** The key the row asked by: the id the Logbook's page answer handed it for this contact. */
async function rowId(): Promise<string> {
  const { answerAs } = await import('../features/logAnswers.testkit')
  const { DEFAULT_LOG_QUERY } = await import('../features/logQuery')
  return answerAs({ kind: 'page', query: DEFAULT_LOG_QUERY, offset: 0, limit: 1 }, await engineLog()).keys[0]
}

describe('Logbook row — upload one contact to LoTW', () => {
  it('sits with the other services, labelled LoTW, and uploads that contact by its id', async () => {
    vi.mocked(uploadLotwReportByIds).mockResolvedValue({ dispatched: 1, outcome: 'pending' })
    const btn = await lotwButton()
    expect(btn.textContent).toBe('LoTW')
    // Beside the other uploads in the cluster: right after WRL.
    expect(btn.previousElementSibling?.textContent).toBe('WRL')
    fireEvent.click(btn)
    await waitFor(() => expect(uploadLotwReportByIds).toHaveBeenCalledTimes(1))
    const sent = vi.mocked(uploadLotwReportByIds).mock.calls[0][0]
    expect(sent).toHaveLength(1)
    expect(sent[0]).toBe(await rowId())
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('logbook.push.lotw.ok', { call: 'K0ABC' }), 'success', expect.any(Number)),
    )
  })

  it('says LoTW already had it when TQSL answers duplicate', async () => {
    vi.mocked(uploadLotwReportByIds).mockResolvedValue({ dispatched: 1, outcome: 'duplicate' })
    fireEvent.click(await lotwButton())
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('logbook.push.lotw.duplicate', { call: 'K0ABC' }), 'success', expect.any(Number)),
    )
  })

  it("says the bulk upload's words when LoTW refuses or cannot be reached", async () => {
    vi.mocked(uploadLotwReportByIds).mockResolvedValue({ dispatched: 1, outcome: 'authfail', detail: 'bad cert' })
    fireEvent.click(await lotwButton())
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('logbook.lotw.upload.authFailedDetail', { detail: 'bad cert' }), 'error'),
    )
    cleanup()
    vi.mocked(uploadLotwReportByIds).mockResolvedValue({ dispatched: 1, outcome: 'retry' })
    fireEvent.click(await lotwButton())
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith(t('logbook.lotw.upload.retry'), 'error'))
  })

  it("passes the station's own refusal through in the bulk upload's words (no Station Location)", async () => {
    const why = 'Set your LoTW Station Location in Settings before uploading.'
    vi.mocked(uploadLotwReportByIds).mockRejectedValue(why)
    fireEvent.click(await lotwButton())
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(`${t('logbook.lotw.upload.failed')}: ${why}`, 'error'),
    )
  })

  it('names the mode TQSL cannot take yet when the station held the contact back', async () => {
    vi.mocked(uploadLotwReportByIds).mockResolvedValue({ dispatched: 0, outcome: 'none', heldMode: 1 })
    fireEvent.click(await lotwButton())
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('logbook.push.lotw.none', { call: 'K0ABC' }), 'info', expect.any(Number)),
    )
    expect(pushToast).toHaveBeenCalledWith(t('logbook.lotw.held', { count: 1 }), 'info', expect.any(Number))
  })

  it('refuses without starting TQSL: already uploaded, already confirmed, no time of day', async () => {
    for (const [over, key] of [
      [{ upload: { lotw: { outcome: 'accepted', whenUnix: 1_700_000_100 } } }, 'logbook.push.lotw.already'],
      [{ upload: { lotw: { outcome: 'pending', whenUnix: 1_700_000_100 } } }, 'logbook.push.lotw.already'],
      [{ awardConfirmed: true }, 'logbook.push.lotw.confirmed'],
      [{ timeKnown: false }, 'logbook.push.lotw.timeless'],
    ] as const) {
      fireEvent.click(await lotwButton(over))
      await waitFor(() => expect(pushToast).toHaveBeenCalledWith(t(key, { call: 'K0ABC' }), 'info', expect.any(Number)))
      expect(uploadLotwReportByIds, key).not.toHaveBeenCalled()
      cleanup()
      vi.mocked(pushToast).mockReset()
    }
  })

  it('sends a contact LoTW bounced again', async () => {
    vi.mocked(uploadLotwReportByIds).mockResolvedValue({ dispatched: 1, outcome: 'pending' })
    fireEvent.click(await lotwButton({ upload: { lotw: { outcome: 'rejected', whenUnix: 1_700_000_100 } } }))
    await waitFor(() => expect(uploadLotwReportByIds).toHaveBeenCalledTimes(1))
  })
})
