// @vitest-environment jsdom
//
// THE DUPE BADGE — a contest tester: "would like to see a visible DUPE thing show up on main entry
// window when a dup happens". The own-dupe verdict was an 11 px faint sentence under the boxes; it
// is now a DUPE badge on the Call box saying when the station was worked, the box itself coloured,
// both from the session's own dupe check (`contestDupeRow`, the row `contestDupe` judged by). And
// since every shipped contest LOGS its dupes, Enter on one said "Logged" exactly as for a new
// contact: the toast now says the row went in as a dupe, from the engine's own mark on the row.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManualRows: vi.fn(() => Promise.resolve([])),
  contestLogSatellite: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  contestPlace: vi.fn(() => Promise.resolve(null)),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
const { pushToast } = (await import('../toast')) as unknown as { pushToast: ReturnType<typeof vi.fn> }

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

/** 2026-06-27 at hh:mm UTC, as the engine stamps a row. */
const at = (hh: number, mm: number) => Date.UTC(2026, 5, 27, hh, mm) / 1000

const row = (over: Partial<FieldDayQso>): FieldDayQso =>
  ({ call: 'W1AW', class: '1D', section: 'CT', band: '20m', mode: 'PH', submode: '', ...over }) as FieldDayQso

const fd = (log: FieldDayQso[], extra: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    composing: [
      { key: 'CLASS', raw: '3A' },
      { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
    ],
    running: true,
    state: '',
    qsoCount: log.length,
    sections: 1,
    points: 1,
    log,
    club: { dupes: [['K1ABC', '20m', 'PH']], board: [] },
    ...extra,
  }) as unknown as FieldDayStatus

const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const badge = () => document.querySelector('.le-fd-field-call .le-fd-dupe') as HTMLElement
const toasts = (kind: string) =>
  pushToast.mock.calls.filter(([, k]) => (k ?? 'error') === kind).map(([text]) => text)

function typeCall(fieldDay: FieldDayStatus, call: string) {
  render(<LogEntry snap={snap} mode="PH" defaultRst="59" exchange="terrestrial" fieldDay={fieldDay} fdMode="PH" />)
  fireEvent.change(callBox(), { target: { value: call } })
}

afterEach(() => {
  cleanup()
  pushToast.mockClear()
  api.contestLogManual.mockClear()
})

describe('the DUPE badge on the Call box', () => {
  it('shows DUPE and when the station was worked, colours the Call box, and is a live region', () => {
    typeCall(fd([row({ whenUnix: at(14, 32) })]), 'w1aw')
    expect(badge().textContent).toBe('DUPE 14:32 20m PH')
    expect(badge().getAttribute('aria-live')).toBe('polite')
    expect(callBox().classList.contains('le-fd-input-dupe')).toBe(true)
  })

  it('names the FIRST row the dupe check matched: not another band, not the later logged dupe', () => {
    typeCall(
      fd([
        row({ band: '40m', whenUnix: at(13, 0) }),
        row({ whenUnix: at(14, 32) }),
        row({ whenUnix: at(15, 5), dupe: true }),
      ]),
      'w1aw',
    )
    expect(badge().textContent).toBe('DUPE 14:32 20m PH')
  })

  it("names the row's own band and mode under a rule that ignores them, never the dial's", () => {
    // Sweepstakes works a station once: the 40 m CW row is a dupe of a 20 m phone contact.
    const once = {
      byCall: true,
      byBand: false,
      byModeClass: false,
      byFields: [],
      bySentFields: [],
      modeClassGroups: [],
    }
    typeCall(
      fd([row({ band: '40m', mode: 'CW', whenUnix: at(9, 7) })], { dupeRule: once } as unknown as Partial<FieldDayStatus>),
      'W1AW',
    )
    expect(badge().textContent).toBe('DUPE 09:07 40m CW')
  })

  it('stays empty, and the box uncoloured, for a new call and for a club-only dupe', () => {
    typeCall(fd([row({ whenUnix: at(14, 32) })]), 'n0xyz')
    expect(badge().textContent).toBe('')
    expect(callBox().classList.contains('le-fd-input-dupe')).toBe(false)
    fireEvent.change(callBox(), { target: { value: 'K1ABC' } })
    expect(screen.getByText(/^Club dupe:/)).toBeTruthy()
    expect(badge().textContent).toBe('')
    expect(callBox().classList.contains('le-fd-input-dupe')).toBe(false)
  })
})

describe('Enter on a dupe the contest logs', () => {
  const enterWith = async (logged: FieldDayQso) => {
    api.contestLogManual.mockImplementationOnce(() =>
      Promise.resolve({ fieldDay: fd([row({ whenUnix: at(14, 32) }), logged]) }),
    )
    typeCall(fd([row({ whenUnix: at(14, 32) })]), 'W1AW')
    const [cls, sec] = [...document.querySelectorAll('.le-fd-input-code')] as HTMLInputElement[]
    fireEvent.change(cls, { target: { value: '3A' } })
    fireEvent.change(sec, { target: { value: 'WI' } })
    fireEvent.keyDown(sec, { key: 'Enter' })
    await vi.waitFor(() => expect(pushToast).toHaveBeenCalledTimes(1))
  }

  it('says the contact went in AS A DUPE when the engine marked its row one', async () => {
    await enterWith(row({ whenUnix: at(15, 5), dupe: true }))
    expect(toasts('success')).toEqual([])
    expect(toasts('info')).toEqual(['Logged W1AW 3A WI (PH) as a DUPE: worked before, it scores no points'])
  })

  it('says plain "Logged" when the engine did not mark it (control)', async () => {
    await enterWith(row({ whenUnix: at(15, 5), dupe: false }))
    expect(toasts('info')).toEqual([])
    expect(toasts('success')).toEqual(['Logged W1AW 3A WI (PH)'])
  })
})
