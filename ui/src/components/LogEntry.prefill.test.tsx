// @vitest-environment jsdom
//
// ⭐ THE CONTEST ENTRY'S PRE-FILL, FROM THREE SOURCES IN THE OPERATOR'S ORDER: on a call, the
// county/state box takes this contest's last contact with the station, else the call-history
// file, else the most recent contact anywhere in the log (asked of the station, which reads that
// one call's contacts through the log's per-call index). The box says which, in its caption
// ("this contest" / "call history" / "log 2025"); it is never written over what the operator
// typed; and Space into it selects it, so typing replaces it.
//
// The Illinois QSO Party as the engine sends it, rendered with the props `PhoneCockpit` gives.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  contestPlace: vi.fn(() => Promise.resolve(null)),
  contestLoggedQth: vi.fn(() => Promise.resolve(null)),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
  scpEnsure: vi.fn(() => Promise.resolve({ fetchedAt: 0, checkedAt: 0, nextCheckAt: 0, count: 0 })),
  getScpCalls: vi.fn(() => Promise.resolve([])),
  getCallHistory: vi.fn(() =>
    Promise.resolve({ contest: 'ilqp', fileName: 'il-2026.txt', entries: { K9AAA: { Loc1: 'DUPAGE' }, K9HHH: { Loc1: 'WILL' } } }),
  ),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null } as unknown as AppSnapshot

/** 2025-10-18 12:00 UTC: a contact a year back, for the "log 2025" tag. */
const LAST_YEAR = 1_760_788_800

const row = (call: string, county: string): FieldDayQso => ({ call, class: '', section: '', band: '40m', mode: 'PH', rcvd: ['59', county] })

const ilqp = (log: FieldDayQso[] = []): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    role: '',
    event: 'ilqp',
    receives: [
      { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '59' },
      { key: 'QTH', raw: 'LAKE', domain: 'il_counties' },
    ],
    assistanceOn: ['Super Check Partial', 'Call history'],
  }) as unknown as FieldDayStatus

const strip = (fieldDay: FieldDayStatus) => (
  <LogEntry
    snap={snap}
    mode="SSB"
    defaultRst="59"
    exchange="terrestrial"
    titled={false}
    onSpot={() => {}}
    pendingWork={null}
    onConsumeWork={() => {}}
    fieldDay={fieldDay}
    fdMode="PH"
  />
)

const settle = (ms = 0) => act(async () => new Promise((r) => setTimeout(r, ms)))
const callBox = () => screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const qthBox = () => document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')[1]
/** The box's value and the word its caption shows ('' when unmarked). */
const qth = () => [qthBox().value, qthBox().closest('label')!.querySelector('.le-fd-from-history')?.textContent ?? '']
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the contest entry pre-fills the county/state box', () => {
  it("from this contest's last contact with the station first, over the call-history file", async () => {
    render(strip(ilqp([row('K9AAA', 'KANE'), row('K9AAA', 'COOK')])))
    await settle()
    type(callBox(), 'K9AAA')
    expect(qth()).toEqual(['COOK', 'this contest'])
  })

  it('from the call-history file when this contest has not worked them', async () => {
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9HHH')
    expect(qth()).toEqual(['WILL', 'call history'])
  })

  it('from the most recent contact anywhere in the log last, tagged with its year', async () => {
    api.contestLoggedQth.mockImplementation((call: string) =>
      Promise.resolve(call === 'K9LOG' ? { value: 'Cook', whenUnix: LAST_YEAR } : null),
    )
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9LOG')
    await settle(260)
    expect(api.contestLoggedQth).toHaveBeenLastCalledWith('K9LOG', 'QTH')
    // "Cook" as logged becomes the code the box takes.
    expect(qth()).toEqual(['COOK', 'log 2025'])
    // Asked once for the settled call, never per keystroke of it.
    expect(api.contestLoggedQth).toHaveBeenCalledTimes(1)
  })

  it('the log is the last source: this contest and the file still win over it', async () => {
    api.contestLoggedQth.mockImplementation(() => Promise.resolve({ value: 'KANE', whenUnix: LAST_YEAR }))
    render(strip(ilqp([row('K9AAA', 'COOK')])))
    await settle()
    type(callBox(), 'K9AAA')
    await settle(260)
    expect(qth()).toEqual(['COOK', 'this contest'])
    type(callBox(), 'K9HHH')
    await settle(260)
    expect(qth()).toEqual(['WILL', 'call history'])
  })

  it('a value from the log that the box would refuse is not filled', async () => {
    api.contestLoggedQth.mockImplementation(() => Promise.resolve({ value: 'XX', whenUnix: LAST_YEAR }))
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9LOG')
    await settle(260)
    expect(qth()).toEqual(['', ''])
  })

  it('a station already worked on this band and mode takes no fill from this contest or the log', async () => {
    // What it sends NOW decides dupe or a rover's new county; the box waits for the operator.
    api.contestLoggedQth.mockImplementation(() => Promise.resolve({ value: 'KANE', whenUnix: LAST_YEAR }))
    render(strip(ilqp([{ ...row('K9AAA', 'COOK'), band: '20m' }, { ...row('K9LOG', 'COOK'), band: '20m' }])))
    await settle()
    type(callBox(), 'K9LOG')
    await settle(260)
    expect(qth()).toEqual(['', ''])
    // The call-history file is the operator's own and fills as it always did.
    type(callBox(), 'K9AAA')
    expect(qth()).toEqual(['DUPG', 'call history'])
  })

  it('never writes over what the operator typed', async () => {
    render(strip(ilqp([row('K9AAA', 'COOK')])))
    await settle()
    type(qthBox(), 'LAKE')
    type(callBox(), 'K9AAA')
    expect(qth()).toEqual(['LAKE', ''])
  })

  it('a moving station is not filled from any source', async () => {
    api.contestLoggedQth.mockImplementation(() => Promise.resolve({ value: 'KANE', whenUnix: LAST_YEAR }))
    render(strip(ilqp([row('K9AAA/M', 'COOK')])))
    await settle()
    type(callBox(), 'K9AAA/M')
    await settle(260)
    expect(qth()).toEqual(['', ''])
  })

  it('Space into the filled box selects it, so typing replaces it', async () => {
    render(strip(ilqp([row('K9AAA', 'COOK')])))
    await settle()
    type(callBox(), 'K9AAA')
    callBox().focus()
    // Space walks Call → RST → County; each box it reaches is selected whole.
    fireEvent.keyDown(callBox(), { key: ' ', code: 'Space' })
    const rst = document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')[0]
    fireEvent.keyDown(rst, { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(qthBox())
    expect([qthBox().selectionStart, qthBox().selectionEnd]).toEqual([0, 4])
  })

  it('a fill landing in the box that has the caret selects it', async () => {
    api.contestLoggedQth.mockImplementation(() => Promise.resolve({ value: 'KANE', whenUnix: LAST_YEAR }))
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9LOG')
    qthBox().focus()
    await settle(260)
    await settle(20)
    expect(qth()).toEqual(['KANE', 'log 2025'])
    expect([qthBox().selectionStart, qthBox().selectionEnd]).toEqual([0, 4])
  })
})
