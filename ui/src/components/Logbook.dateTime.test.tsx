// @vitest-environment jsdom
//
// #239 — THE CONTACT'S DATE AND TIME ARE THEIR OWN COLUMNS. The log had one "Time (UTC)"
// column printing a whole instant, `2026-09-14 00:58Z`, in a single cell: you could not sort
// the eye down a column of dates, and the time of day sat wherever the date's width left it.
// swinn asked for "the time and dates in their own fields".
//
// The split also makes an existing lie visible and has to answer it. A record imported from a
// source that carried no time of day is stored with `timeKnown: false` and `whenUnix` anchored
// at midnight FOR ORDERING ONLY — the combined column printed "00:58Z"-shaped text for it and
// read as fact, which is the same mistake `QsoDetail` already refuses to make. A Time column of
// its own cannot print 00:00 for a contact whose time nobody recorded.
//
// jsdom lays nothing out: nothing here asserts a column WIDTH or that a cell fits. The track
// count against the rendered cell count is computed in styles-logbook-actions.test.tsx, which
// is the guard that catches a template left one track short.
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
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
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSentById: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

/** 00:58 UTC — the #280 instant, which a 12-hour locale drew as "12:58 AM". */
const WHEN = Date.UTC(2026, 8, 14, 0, 58) / 1000

function qso(over: Record<string, unknown> = {}) {
  return {
    call: 'K1ABC', grid: 'FN31', band: '40m', freqMhz: 7.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: WHEN, confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    ...over,
  }
}

async function table(rows: Record<string, unknown>[]): Promise<HTMLElement> {
  engineLog.mockResolvedValue(rows)
  const { container } = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  return waitFor(() => {
    const r = container.querySelector('.logbook-row:not(.head)')
    expect(r, 'no data row rendered').not.toBeNull()
    return container as HTMLElement
  })
}

/** The header labels, in render order, with the sort arrow the active column appends stripped. */
const headers = (c: HTMLElement) =>
  [...c.querySelectorAll('.logbook-row.head > .log-cell')].map((el) =>
    (el.textContent ?? '').replace(/\s*[▲▼]\s*$/, '').trim(),
  )

/** The data row's cells, in render order. */
const cells = (c: HTMLElement) =>
  [...(c.querySelector('.logbook-row:not(.head)') as HTMLElement).querySelectorAll(':scope > .log-cell')]

describe('the log shows date and time as separate columns (#239)', () => {
  it('gives each its own header, side by side, in that order', async () => {
    const c = await table([qso()])
    const h = headers(c)
    const date = h.indexOf('Date (UTC)')
    const time = h.indexOf('Time (UTC)')
    expect(date, 'there is no Date column').toBeGreaterThanOrEqual(0)
    expect(time, 'there is no Time column').toBeGreaterThanOrEqual(0)
    expect(time - date, 'the two must be adjacent, date first — they read as one field').toBe(1)
  })

  it('puts the date in one cell and the time of day in the next, neither carrying the other', async () => {
    const c = await table([qso()])
    const h = headers(c)
    const cs = cells(c)
    expect(cs.length, 'a cell per header, or the indices below address the wrong column').toBe(h.length)

    const date = (cs[h.indexOf('Date (UTC)')].textContent ?? '').trim()
    const time = (cs[h.indexOf('Time (UTC)')].textContent ?? '').trim()
    // The exact strings: the old single column printed "2026-09-14 00:58Z" into ONE cell, so
    // an equality on each half is what tells a real split from a relabelled header.
    expect(date).toBe('2026-09-14')
    expect(time).toBe('00:58')
    expect(date, 'the date cell is still carrying a time of day').not.toMatch(/:/)
    expect(time, 'the time cell is still carrying a date').not.toMatch(/-/)
  })

  it('says nothing about a time of day nobody recorded, and still prints the date', async () => {
    // `timeKnown: false` = an imported date-only record; whenUnix anchors the day for ordering
    // and its clock reading is an artefact, not an observation.
    const c = await table([qso({ timeKnown: false })])
    const h = headers(c)
    const cs = cells(c)
    expect((cs[h.indexOf('Date (UTC)')].textContent ?? '').trim()).toBe('2026-09-14')
    expect(
      (cs[h.indexOf('Time (UTC)')].textContent ?? '').trim(),
      'a date-only record must not print a clock reading it never had',
    ).toBe('—')
  })

  it('control: the SAME instant with the time known does print it', async () => {
    // Without this the test above cannot tell "suppressed because unknown" from "the Time
    // column is always a dash": same whenUnix, one field different, two different readings.
    const c = await table([qso({ timeKnown: true })])
    const h = headers(c)
    expect((cells(c)[h.indexOf('Time (UTC)')].textContent ?? '').trim()).toBe('00:58')
  })
})
