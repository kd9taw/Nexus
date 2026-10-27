// @vitest-environment jsdom
//
// Exporting the rows selected in the Logbook, beside the whole-log and date-range exports.
//
// Ctrl-click (⌘ on a Mac) picks a row, Shift-click a run of rows, Space the focused one, Esc
// clears. The export sends the picked contacts' ids to the station, which writes each one as the
// whole-log export writes it (crates/tempo-app logstore tests pin the bytes). A file with no
// contact in it is not written: the Logbook says so instead.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import { Toasts } from './Toasts'
import { t } from '../i18n'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'
import type { LoggedQso } from '../types'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

const HEADER = 'Nexus logbook\n<ADIF_VER:5>3.1.8\n<PROGRAMID:5>Nexus\n<EOH>\n'

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    exportGeneralLog: vi.fn(async () => ({ text: '<CALL:5>W1AAA<EOR>\n', saving: 0, held: 0 })),
    exportSelectedLog: vi.fn(async () => ({ text: '<CALL:5>W1AAA<EOR>\n', saving: 0, held: 0 })),
    saveTextToDownloads: vi.fn(async () => '/tmp/nexus-log.adi'),
    deleteQsoById: noop(), importAdif: noop(), editQsoById: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: noop(),
    logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})

/** Four contacts, a minute apart; the list shows newest first. */
const contact = (call: string, minute: number): LoggedQso =>
  ({
    id: `id-${call}`, call, grid: null, band: '40m', freqMhz: 7.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: null, whenUnix: Math.floor(Date.UTC(2026, 8, 14, 1, minute) / 1000),
    confirmed: false, awardConfirmed: false,
  }) as LoggedQso
const LOG = [contact('W1AAA', 1), contact('W2BBB', 2), contact('W3CCC', 3), contact('W4DDD', 4)]

async function openLogbook() {
  engineLog.mockResolvedValue(LOG)
  render(
    <>
      <Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />
      <Toasts />
    </>,
  )
  await waitFor(() => expect(rowOf('W1AAA')).toBeTruthy())
}

const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.logbook-row:not(.head)')].find((r) =>
    r.textContent?.includes(call),
  )!
const selectedCalls = () =>
  [...document.querySelectorAll<HTMLElement>('.logbook-row[aria-selected="true"]')].map(
    (r) => LOG.find((q) => r.textContent?.includes(q.call))!.call,
  )
const selectedButton = () =>
  screen.getByRole('button', { name: /^Export selected/ }) as HTMLButtonElement
const sentIds = () => [...(vi.mocked(api.exportSelectedLog).mock.calls[0]?.[0] ?? [])].sort()

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

describe('exporting the rows selected in the Logbook', () => {
  it('exports exactly the rows picked with Ctrl-click and ⌘-click', async () => {
    await openLogbook()
    expect(selectedButton().disabled, 'nothing picked, nothing to export').toBe(true)
    fireEvent.click(rowOf('W2BBB'), { ctrlKey: true })
    fireEvent.click(rowOf('W4DDD'), { metaKey: true })
    expect(selectedCalls().sort()).toEqual(['W2BBB', 'W4DDD'])
    expect(selectedButton().textContent).toBe(t('logbook.export.selected.label', { count: 2 }))
    fireEvent.click(selectedButton())
    await waitFor(() => expect(api.saveTextToDownloads).toHaveBeenCalled())
    expect(sentIds()).toEqual(['id-W2BBB', 'id-W4DDD'])
    expect(vi.mocked(api.saveTextToDownloads).mock.calls[0]?.[1]).toBe('<CALL:5>W1AAA<EOR>\n')
    expect(api.exportGeneralLog).not.toHaveBeenCalled()
  })

  it('a second Ctrl-click unpicks the row, and a plain click picks nothing', async () => {
    await openLogbook()
    fireEvent.click(rowOf('W2BBB'), { ctrlKey: true })
    fireEvent.click(rowOf('W3CCC'), { ctrlKey: true })
    fireEvent.click(rowOf('W2BBB'), { ctrlKey: true })
    fireEvent.click(rowOf('W1AAA'))
    expect(selectedCalls()).toEqual(['W3CCC'])
  })

  it('Shift-click picks the run of rows from the last one picked', async () => {
    await openLogbook()
    // Newest first: W4DDD, W3CCC, W2BBB, W1AAA.
    fireEvent.click(rowOf('W4DDD'), { ctrlKey: true })
    fireEvent.click(rowOf('W2BBB'), { shiftKey: true })
    expect(selectedCalls().sort()).toEqual(['W2BBB', 'W3CCC', 'W4DDD'])
  })

  it('a Ctrl-click on one of the row\'s own controls is that control\'s, not a pick', async () => {
    await openLogbook()
    const call = rowOf('W2BBB').querySelector('button.qrz-link-call') as HTMLButtonElement
    fireEvent.click(call, { ctrlKey: true })
    expect(selectedCalls()).toEqual([])
  })

  it('a run that reaches rows still loading is not picked, and says why', async () => {
    const many = Array.from({ length: 300 }, (_, i) => contact(`K${i}AA`, i % 60))
    engineLog.mockResolvedValue(many)
    render(
      <>
        <Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />
        <Toasts />
      </>,
    )
    const first = await waitFor(() => {
      const r = document.querySelector<HTMLElement>('.logbook-row[data-index="0"]:not(.placeholder)')
      expect(r).toBeTruthy()
      return r!
    })
    fireEvent.click(first, { ctrlKey: true })
    fireEvent.keyDown(first, { key: 'End' })
    const last = await waitFor(() => {
      const r = document.querySelector<HTMLElement>('.logbook-row[data-index="299"]:not(.placeholder)')
      expect(r).toBeTruthy()
      return r!
    })
    fireEvent.keyDown(last, { key: ' ', shiftKey: true })
    expect(await screen.findByText(t('logbook.export.selected.notLoaded'))).toBeTruthy()
    // Still the one row picked before the run.
    expect(selectedButton().textContent).toBe(t('logbook.export.selected.label', { count: 1 }))
  })

  it('Space picks the focused row and Esc clears the selection', async () => {
    await openLogbook()
    fireEvent.keyDown(rowOf('W3CCC'), { key: ' ' })
    expect(selectedCalls()).toEqual(['W3CCC'])
    fireEvent.keyDown(rowOf('W3CCC'), { key: 'Escape' })
    expect(selectedCalls()).toEqual([])
  })

  it('a new search clears the selection, so no row the list no longer shows is exported', async () => {
    await openLogbook()
    fireEvent.click(rowOf('W2BBB'), { ctrlKey: true })
    fireEvent.change(screen.getByPlaceholderText(t('logbook.search.placeholder')), { target: { value: 'W3' } })
    await waitFor(() => expect(selectedButton().disabled).toBe(true))
  })

  it('says so and writes no file when the selection holds no contact the station has', async () => {
    vi.mocked(api.exportSelectedLog).mockResolvedValueOnce({ text: HEADER, saving: 0, held: 0 })
    await openLogbook()
    fireEvent.click(rowOf('W2BBB'), { ctrlKey: true })
    fireEvent.click(selectedButton())
    expect(await screen.findByText(t('logbook.export.nothing'))).toBeTruthy()
    expect(api.saveTextToDownloads).not.toHaveBeenCalled()
  })

  it('says so and writes no file when the date range holds no contact', async () => {
    vi.mocked(api.exportGeneralLog).mockResolvedValueOnce({ text: HEADER, saving: 0, held: 0 })
    await openLogbook()
    const adif = screen.getByRole('button', { name: 'Export ADIF' }) as HTMLButtonElement
    await waitFor(() => expect(adif.disabled).toBe(false))
    fireEvent.click(adif)
    expect(await screen.findByText(t('logbook.export.nothing'))).toBeTruthy()
    expect(api.saveTextToDownloads).not.toHaveBeenCalled()
  })
})
