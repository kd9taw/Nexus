// @vitest-environment jsdom
//
// #280: "LOGGED clock time off by 12 hours". The stored time was right. The edit form showed it
// through a native `datetime-local` control, which WebView2 renders in the OS locale — so on a
// 12-hour Windows PC a contact at 00:58 UTC read "12:58 AM", the operator "corrected" it, and a
// right record moved 12 hours. Typing 00 into that control's hour went straight back to 12.
// The time is now a plain 24-hour UTC box (HH:MM or HH:MM:SS) that no locale formats, beside
// the date.
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
    // The shared log store (big-log fix) reads through get_log_delta; answered with the whole
    // log from `getLog`, so this file holds with or without it.
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

/** 2026-09-14 at the given UTC time — the reporter's 00:58, and one with seconds. */
const at = (h: number, m: number, s = 0) => Math.floor(Date.UTC(2026, 8, 14, h, m, s) / 1000)

async function openEdit(whenUnix: number) {
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue([
    {
      call: 'VE3ABC', grid: 'FN03', band: '40m', freqMhz: 7.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'Canada', whenUnix, confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    },
  ])
  const { container } = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  await waitFor(() => expect(container.querySelector('button[aria-label="Edit VE3ABC"]')).not.toBeNull())
  fireEvent.click(container.querySelector('button[aria-label="Edit VE3ABC"]') as HTMLButtonElement)
  await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())
  return container.querySelector('.logbook-form') as HTMLElement
}

/** The box that holds the contact's time of day: whichever input's value carries 00:58/12:58. */
function timeBox(form: HTMLElement): HTMLInputElement {
  const box = [...form.querySelectorAll('input')].find((i) => /\d{2}:\d{2}/.test(i.value))
  expect(box, 'no input in the form holds the time').toBeTruthy()
  return box as HTMLInputElement
}
const save = (form: HTMLElement) => fireEvent.click(within(form).getByRole('button', { name: /save/i }))
const savedWhen = () => (api.editQsoById as ReturnType<typeof vi.fn>).mock.calls[0][1].whenUnix as number

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  vi.restoreAllMocks()
  localStorage.clear()
})

describe('the Logbook edit form shows and takes the time as 24-hour UTC (#280)', () => {
  it('shows 00:58 UTC as "00:58" in a plain text box, whatever the OS locale', async () => {
    // A 12-hour OS locale, as far as the page can see one. Nothing may reach the box through it.
    vi.spyOn(Date.prototype, 'toLocaleTimeString').mockReturnValue('12:58 AM')
    vi.spyOn(Date.prototype, 'toLocaleString').mockReturnValue('9/14/2026, 12:58:00 AM')
    const form = await openEdit(at(0, 58))
    const box = timeBox(form)
    // Not a native time control: those draw in the OS locale, which is the whole bug.
    expect(box.type, 'the time is a native control the OS locale renders').toBe('text')
    expect(box.value).toBe('00:58')
    expect(within(form).getByDisplayValue('2026-09-14')).toBeTruthy()
  })

  it('saves the instant it opened with when the time is left alone — seconds included', async () => {
    const form = await openEdit(at(0, 58, 37))
    expect(timeBox(form).value).toBe('00:58:37')
    save(form)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    expect(savedWhen()).toBe(at(0, 58, 37))
  })

  it('moves only the hour when the hour is what changed — the date and the seconds stay', async () => {
    const form = await openEdit(at(0, 58, 37))
    fireEvent.change(timeBox(form), { target: { value: '12:58:37' } })
    save(form)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    expect(savedWhen()).toBe(at(12, 58, 37))
  })

  it('takes a 24-hour time typed as HH:MM', async () => {
    const form = await openEdit(at(0, 58, 37))
    fireEvent.change(timeBox(form), { target: { value: '18:05' } })
    save(form)
    await waitFor(() => expect(api.editQsoById).toHaveBeenCalled())
    expect(savedWhen()).toBe(at(18, 5))
  })

  it('refuses 25:00 and saves nothing', async () => {
    const form = await openEdit(at(0, 58))
    fireEvent.change(timeBox(form), { target: { value: '25:00' } })
    save(form)
    await waitFor(() => expect(within(form).getByRole('alert').textContent).toMatch(/HH:MM/))
    expect(api.editQsoById).not.toHaveBeenCalled()
  })
})
