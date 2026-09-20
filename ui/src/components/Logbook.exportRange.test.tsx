// @vitest-environment jsdom
//
// The export date range is UTC too (logging-lens review, 2026-09-19).
//
// It bounds the ADIF and CSV exports by UTC QSO date (#98), and those files are what an operator
// hands to an awards submission or uploads to a service. The two boxes were native `type="date"`
// controls, which WebView2 draws in the OS locale and fills from a "Today" button that means the
// LOCAL day — so a range could silently start or end a day off and leave contacts out of what
// was sent. The control also discards what it cannot parse, so a mistyped bound looked like no
// bound at all: the widest possible failure, a whole log where a slice was meant.
//
// Same treatment as the log's own date boxes: plain UTC text (YYYY-MM-DD), marked while it is
// not a date, and the export held rather than run on a half-understood range.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
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
    exportGeneralLog: vi.fn(async () => '<eor>\n'),
    saveTextToDownloads: vi.fn(async () => '/tmp/nexus-log.adi'),
    deleteQso: noop(), importAdif: noop(), editQso: vi.fn(async () => ({})),
    logOperators: vi.fn(async () => [] as string[]), exportLogForOperator: noop(),
    logActivations: vi.fn(async () => []), exportLogForActivation: noop(),
    logQso: vi.fn(async () => ({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: noop(), markQslCard: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})

async function openLogbook() {
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue([
    {
      call: 'VE3ABC', grid: 'FN03', band: '40m', freqMhz: 7.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'Canada', whenUnix: Math.floor(Date.UTC(2026, 8, 14, 0, 58) / 1000),
      confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null,
    },
  ])
  const { container } = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  await waitFor(() => expect(container.querySelectorAll('.log-export-date').length).toBe(2))
  const [from, to] = [...container.querySelectorAll('.log-export-date')] as HTMLInputElement[]
  return { from, to }
}
const adifButton = () => screen.getByRole('button', { name: 'Export ADIF' }) as HTMLButtonElement

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

describe('the export date range is typed in UTC', () => {
  it('takes the dates as plain text, not through a control the OS locale draws', async () => {
    const { from, to } = await openLogbook()
    expect(from.type, 'the range start is a native date control').toBe('text')
    expect(to.type, 'the range end is a native date control').toBe('text')
  })

  it('exports the UTC days that were typed', async () => {
    const { from, to } = await openLogbook()
    fireEvent.change(from, { target: { value: '2026-09-01' } })
    fireEvent.change(to, { target: { value: '2026-09-19' } })
    fireEvent.click(adifButton())
    await waitFor(() => expect(api.exportGeneralLog).toHaveBeenCalled())
    expect((api.exportGeneralLog as ReturnType<typeof vi.fn>).mock.calls[0]).toEqual([
      'adif', '2026-09-01', '2026-09-19',
    ])
  })

  it('holds the export on a date that does not exist, and keeps what was typed', async () => {
    const { from } = await openLogbook()
    fireEvent.change(from, { target: { value: '2026-02-30' } })
    // The native control threw the typing away, which reads as "no bound" — and exporting then
    // hands over the WHOLE log where a slice was meant.
    expect(from.value, 'the typed bound was discarded instead of refused').toBe('2026-02-30')
    expect(from.getAttribute('aria-invalid')).toBe('true')
    expect(adifButton().disabled, 'the export stayed armed on a range it cannot read').toBe(true)
    fireEvent.click(adifButton())
    expect(api.exportGeneralLog).not.toHaveBeenCalled()
  })

  it('still exports the whole log when both bounds are blank', async () => {
    await openLogbook()
    expect(adifButton().disabled).toBe(false)
    fireEvent.click(adifButton())
    await waitFor(() => expect(api.exportGeneralLog).toHaveBeenCalled())
    expect((api.exportGeneralLog as ReturnType<typeof vi.fn>).mock.calls[0]).toEqual(['adif', '', ''])
  })
})
