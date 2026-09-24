// @vitest-environment jsdom
//
// An export the logbook database had not caught up with. The operator's ruling (SPEC-2 C15),
// verbatim: "Write what the database holds and say how many recent changes aren't in the file
// yet (they keep retrying). Export still works as a rescue when the disk is failing." So the
// station writes the file whatever the database holds and counts what it lacks — still being
// saved, or refused by the database for good — and the Logbook says so beside the export's own
// toast. An export that lacks nothing says nothing more.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { Toasts } from './Toasts'
import { dismissToast, subscribeToasts, type Toast } from '../toast'
import { t } from '../i18n'
import type { LogExport, LoggedQso } from '../types'
import * as api from '../api'

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
  const getLog = vi.fn()
  return {
    getLog,
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: await getLog() })),
    exportGeneralLog: vi.fn(),
    saveTextToDownloads: vi.fn(async () => '/tmp/nexus-log.adi'),
    deleteQso: noop(), importAdif: noop(), editQso: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: vi.fn(),
    logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTag: vi.fn(async () => ({})),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: noop(), markQslCard: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})

/** One contact, so the Logbook offers its exports. Written the way the station sends it (nulls
 *  where Rust has `None`), hence the cast. */
const oneContact = [
  {
    call: 'VE3ABC', grid: 'FN03', band: '40m', freqMhz: 7.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'Canada', whenUnix: 1_789_000_000,
    confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null,
  },
] as unknown as LoggedQso[]

const TWO = '<CALL:6>VE3ABC <EOR>\n<CALL:5>K1ABC <EOR>\n'
const file = (saving: number, held: number): LogExport => ({ text: TWO, saving, held })

let toasts: Toast[] = []
let unsubscribe: () => void = () => {}
beforeEach(() => {
  unsubscribe = subscribeToasts((now) => {
    toasts = now
  })
  vi.mocked(api.getLog).mockResolvedValue(oneContact)
})
afterEach(() => {
  cleanup()
  for (const toast of toasts) dismissToast(toast.id)
  unsubscribe()
  vi.clearAllMocks()
})

function open() {
  render(
    <>
      <Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />
      <Toasts />
    </>,
  )
}

/** Export the whole log as ADIF, the station answering `exported`, and wait for the export's own
 *  toast — the control: the file was written and the toast column is live. */
async function exportAdif(exported: LogExport) {
  vi.mocked(api.exportGeneralLog).mockResolvedValue(exported)
  open()
  fireEvent.click(await screen.findByRole('button', { name: 'Export ADIF' }))
  await screen.findByText(t('logbook.export.done', { count: 2, path: '/tmp/nexus-log.adi' }))
}

/** Toasts about what an export lacks, by the words only they say. */
const lackToasts = () => toasts.filter((x) => x.message.includes('not in the export'))

describe('an export the logbook database had not caught up with', () => {
  it('writes the file, and says how many recent changes it lacks and that they are still being saved', async () => {
    await exportAdif(file(2, 0))
    expect(vi.mocked(api.saveTextToDownloads).mock.calls[0]?.[1], 'the file is written').toBe(TWO)
    const said = t('logbook.export.lacks.saving', { count: 2 })
    expect(await screen.findByText(said)).toBeTruthy()
    // The English, read as the operator reads it.
    expect(said).toBe('2 recent changes are not in the export: Nexus is still saving them to the logbook.')
    expect(lackToasts()).toHaveLength(1)
  })

  it('says a change the database refused for good apart — it will not be saved by waiting', async () => {
    await exportAdif(file(0, 1))
    const said = t('logbook.export.lacks.held', { count: 1 })
    expect(await screen.findByText(said)).toBeTruthy()
    expect(said).toMatch(/^1 change the logbook refused is not in the export\. /)
    expect(said).not.toMatch(/still saving/)
    expect(lackToasts()).toHaveLength(1)
  })

  it('says nothing more when the file lacks nothing', async () => {
    await exportAdif(file(0, 0))
    expect(vi.mocked(api.saveTextToDownloads)).toHaveBeenCalledTimes(1)
    expect(lackToasts()).toHaveLength(0)
  })

  it('says it for the CSV export too', async () => {
    vi.mocked(api.exportGeneralLog).mockResolvedValue(file(1, 0))
    open()
    fireEvent.click(await screen.findByRole('button', { name: 'Export CSV' }))
    // CSV counts its rows minus the header line.
    await screen.findByText(t('logbook.export.done', { count: 1, path: '/tmp/nexus-log.adi' }))
    expect(lackToasts().map((x) => x.message)).toEqual([t('logbook.export.lacks.saving', { count: 1 })])
  })

  it('says it for the one-activation export — the file POTA is sent', async () => {
    vi.mocked(api.logActivations).mockResolvedValue([
      { program: 'POTA', reference: 'US-1234', dayStartUnix: 1_788_998_400, date: '2026-09-09', callsign: 'K1ABC', qsos: 12 },
    ])
    vi.mocked(api.exportLogForActivation).mockResolvedValue(file(0, 2))
    open()
    const pick = (await screen.findByRole('option', { name: /US-1234/ })) as HTMLOptionElement
    fireEvent.change(pick.closest('select')!, { target: { value: pick.value } })
    fireEvent.click(screen.getByRole('button', { name: t('logbook.export.activation.button') }))
    await screen.findByText(t('logbook.export.done', { count: 2, path: '/tmp/nexus-log.adi' }))
    expect(lackToasts().map((x) => x.message)).toEqual([t('logbook.export.lacks.held', { count: 2 })])
  })

  it('says it once for the per-operator files, by the most any of them lacked', async () => {
    vi.mocked(api.logOperators).mockResolvedValue(['K1AAA', 'K1BBB'])
    vi.mocked(api.exportLogForOperator)
      .mockResolvedValueOnce(file(0, 0))
      .mockResolvedValueOnce(file(3, 0))
    vi.mocked(api.exportGeneralLog).mockResolvedValue(file(1, 0))
    open()
    fireEvent.click(await screen.findByRole('button', { name: t('logbook.export.perOperator.label') }))
    await screen.findByText(t('logbook.export.perOperator.done', { count: 3 }))
    await waitFor(() => expect(lackToasts()).toHaveLength(1))
    expect(lackToasts()[0].message).toBe(t('logbook.export.lacks.saving', { count: 3 }))
  })
})
