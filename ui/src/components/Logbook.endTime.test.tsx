// @vitest-environment jsdom
//
// #329: "TIME_OFF / end time missing". The field was written at log time and exported all
// along, and 1.14.0 stopped a confirmed prompt-to-log contact reaching Ham Radio Deluxe with
// 00:00 on it — but nothing in the interface ever READ it. An operator could not see an end
// time, so a wrong one could not be noticed, let alone repaired, which is what the reporter
// meant by "time off does not appear at all".
//
// It appears in two places now and both are here: the "More columns" table shows it, and the
// edit form takes it as a 24-hour UTC time beside the start, through the same plain text box
// and the same parser as the start (#280 — a native time control is drawn in the OS locale).
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'

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

/** 2026-09-14 at the given UTC time. */
const at = (h: number, m: number, s = 0) => Math.floor(Date.UTC(2026, 8, 14, h, m, s) / 1000)

function seedLog(whenUnix: number, timeOffUnix: number | null) {
  engineLog.mockResolvedValue([
    {
      call: 'VE3ABC', grid: 'FN03', band: '40m', freqMhz: 7.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'Canada', whenUnix, confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
      // `undefined` is what an older payload and the absent case both look like on the wire;
      // the test seeds `null` deliberately for the "never recorded" row so the component is
      // exercised on the nullish check it actually has to make.
      ...(timeOffUnix === null ? {} : { timeOffUnix }),
    },
  ])
}

/** The wide table ("More columns") is read from localStorage on mount, so it is seeded. */
async function renderRows(wide: boolean) {
  window.localStorage.setItem('nexus.logbook.moreColumns', wide ? '1' : '0')
  const { container } = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
  return container
}

async function openEdit(whenUnix: number, timeOffUnix: number | null) {
  seedLog(whenUnix, timeOffUnix)
  const container = await renderRows(false)
  fireEvent.click(container.querySelector('button[aria-label="Edit VE3ABC"]') as HTMLButtonElement)
  await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())
  return container.querySelector('.logbook-form') as HTMLElement
}

/** The End box, found the way an operator finds it: by its label, not by a class. */
function endBox(form: HTMLElement): HTMLInputElement {
  return within(form).getByTitle(/when the contact ended/i) as HTMLInputElement
}
const save = (form: HTMLElement) => fireEvent.click(within(form).getByRole('button', { name: /save/i }))
const saved = () => (api.editQsoById as ReturnType<typeof vi.fn>).mock.calls[0][1] as { timeOffUnix: number | null }

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  vi.restoreAllMocks()
  localStorage.clear()
})

describe('the Logbook shows the contact end time (#329)', () => {
  it('prints it in the More-columns table, and an em dash when it was never recorded', async () => {
    seedLog(at(14, 32), at(14, 35, 20))
    const withEnd = await renderRows(true)
    const cells = [...withEnd.querySelectorAll('.logbook-row:not(.head) .log-cell')].map((c) => c.textContent)
    expect(cells, 'the end time is on the row, seconds and all').toContain('14:35:20')
    cleanup()

    // THE CONTROL that makes the assertion above mean something: the same row, same table,
    // with no end time. Without it, a cell that always printed the same thing would pass.
    seedLog(at(14, 32), null)
    const without = await renderRows(true)
    const bare = [...without.querySelectorAll('.logbook-row:not(.head) .log-cell')].map((c) => c.textContent)
    expect(bare).not.toContain('14:35:20')
    expect(bare.filter((c) => c === '—').length, 'the cell falls back to an em dash').toBeGreaterThan(0)
  })

  it('does not print it in the compact table — More columns is what it belongs to', async () => {
    seedLog(at(14, 32), at(14, 35, 20))
    const compact = await renderRows(false)
    const cells = [...compact.querySelectorAll('.logbook-row:not(.head) .log-cell')].map((c) => c.textContent)
    expect(cells).not.toContain('14:35:20')
  })
})

describe('the Logbook edit form takes the contact end time (#329)', () => {
  it('opens with the stored end time in a 24-hour UTC box', async () => {
    const form = await openEdit(at(14, 32), at(14, 35))
    expect(endBox(form).value).toBe('14:35')
  })

  it('opens empty when the contact has no end time', async () => {
    const form = await openEdit(at(14, 32), null)
    expect(endBox(form).value).toBe('')
  })

  it('saves a corrected end time as an instant on the contact date', async () => {
    const form = await openEdit(at(14, 32), at(14, 35))
    fireEvent.change(endBox(form), { target: { value: '14:41:07' } })
    save(form)
    await waitFor(() => expect((api.editQsoById as ReturnType<typeof vi.fn>).mock.calls.length).toBe(1))
    expect(saved().timeOffUnix).toBe(at(14, 41, 7))
  })

  it('reads an end EARLIER in the day as the contact running past midnight', async () => {
    // 23:58 to 00:03 is five minutes, not 23h55m backwards. This is the case ADIF gives
    // QSO_DATE_OFF its own field for, and the one a single time box has to get right.
    const start = at(23, 58)
    const form = await openEdit(start, null)
    fireEvent.change(endBox(form), { target: { value: '00:03' } })
    save(form)
    await waitFor(() => expect((api.editQsoById as ReturnType<typeof vi.fn>).mock.calls.length).toBe(1))
    expect(saved().timeOffUnix).toBe(start + 5 * 60)
  })

  it('sends no end time at all when the box is left blank, so a stored one is not wiped', async () => {
    // ⚠️ `null`, never a time: in an edit (`QsoEdit.timeOffUnix`) null is LEAVE ALONE. The backend
    // restores the stored TIME_OFF when the incoming record has none (`Logbook::update_record`),
    // which is what keeps every producer that knows nothing about the field from dropping one —
    // and it is also why clearing an end time is deliberately not offered here.
    const form = await openEdit(at(14, 32), at(14, 35))
    fireEvent.change(endBox(form), { target: { value: '' } })
    save(form)
    await waitFor(() => expect((api.editQsoById as ReturnType<typeof vi.fn>).mock.calls.length).toBe(1))
    expect(saved().timeOffUnix).toBeNull()
  })

  it('refuses a time that is not a 24-hour UTC one rather than guessing at it', async () => {
    const form = await openEdit(at(14, 32), null)
    fireEvent.change(endBox(form), { target: { value: '2:35 PM' } })
    expect(endBox(form).getAttribute('aria-invalid')).toBe('true')
    save(form)
    expect((api.editQsoById as ReturnType<typeof vi.fn>).mock.calls.length).toBe(0)
  })
})
