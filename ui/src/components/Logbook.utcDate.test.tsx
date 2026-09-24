// @vitest-environment jsdom
//
// The date half of #280 (logging-lens review, 2026-09-19). The TIME became a plain 24-hour UTC
// box; the DATE was still a native `type="date"` control, and WebView2 draws that one in the OS
// locale too — MM/DD/YYYY order, and a "Today" button that fills the LOCAL date. An operator west
// of Greenwich hand-logging after 0000Z would file the contact a UTC day early, which is the same
// class of error #280 exists to end, and the native control also silently EMPTIES anything it
// cannot parse, so a mistyped date looks like a blank one.
//
// The date is now a plain UTC text box (YYYY-MM-DD) beside the time, with the same validation and
// the same refusal: what you typed stays on screen, it is marked, and saving says why.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, within } from '@testing-library/react'
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
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(() => Promise.resolve({})),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: vi.fn(() => Promise.resolve({})), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})

/** The reporter's contact: 2026-09-14 at 00:58 UTC — the hour that reads "12:58 AM" on a
 *  12-hour PC, and the hour where a local-date "Today" lands on the wrong UTC day. */
const WHEN = Math.floor(Date.UTC(2026, 8, 14, 0, 58) / 1000)

async function openEdit() {
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue([
    {
      call: 'VE3ABC', grid: 'FN03', band: '40m', freqMhz: 7.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'Canada', whenUnix: WHEN, confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    },
  ])
  const { container } = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  await waitFor(() => expect(container.querySelector('button[aria-label="Edit VE3ABC"]')).not.toBeNull())
  fireEvent.click(container.querySelector('button[aria-label="Edit VE3ABC"]') as HTMLButtonElement)
  await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())
  return container.querySelector('.logbook-form') as HTMLElement
}

const dateBox = (form: HTMLElement) => within(form).getByLabelText('Date (UTC)') as HTMLInputElement
const timeBox = (form: HTMLElement) => within(form).getByLabelText('Time (UTC)') as HTMLInputElement
const save = (form: HTMLElement) => fireEvent.click(within(form).getByRole('button', { name: /save/i }))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  vi.restoreAllMocks()
  localStorage.clear()
})

describe('the Logbook edit form takes the date as UTC text', () => {
  it('shows the contact UTC day in a plain text box, not a control the OS locale draws', async () => {
    const form = await openEdit()
    const box = dateBox(form)
    expect(box.type, 'the date is a native control the OS locale draws and dates itself').toBe('text')
    expect(box.value).toBe('2026-09-14')
  })

  it('saves the UTC day that was typed', async () => {
    const form = await openEdit()
    fireEvent.change(dateBox(form), { target: { value: '2026-09-19' } })
    fireEvent.change(timeBox(form), { target: { value: '00:58' } })
    save(form)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    const saved = (api.editQsoById as ReturnType<typeof vi.fn>).mock.calls[0][1].whenUnix as number
    expect(saved).toBe(Math.floor(Date.UTC(2026, 8, 19, 0, 58) / 1000))
  })

  it('keeps an impossible date on screen, marks it, and saves nothing', async () => {
    const form = await openEdit()
    fireEvent.change(dateBox(form), { target: { value: '2026-02-30' } })
    // A native date control throws the typing away and shows a blank box, which reads as
    // "I cleared it" rather than "that is not a date".
    expect(dateBox(form).value, 'the typed date was discarded instead of refused').toBe('2026-02-30')
    expect(dateBox(form).getAttribute('aria-invalid')).toBe('true')
    save(form)
    await waitFor(() => expect(within(form).getByRole('alert').textContent).toMatch(/YYYY-MM-DD/))
    expect(api.editQsoById).not.toHaveBeenCalled()
  })
})
