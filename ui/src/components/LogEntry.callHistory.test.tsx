// @vitest-environment jsdom
//
// CALL HISTORY FILLS THE CONTEST STRIP, rendered with the props `PhoneCockpit` gives it and an
// Illinois QSO Party session shaped as the engine serialises it. The rules under test are the
// operator's: the file fills the boxes as the call is typed; only a code the contest accepts goes
// in; a filled box is marked; what the operator typed is never written over; nothing is guessed.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestLogManual: vi.fn(() => Promise.resolve({})),
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
  scpEnsure: vi.fn(() => Promise.resolve({ fetchedAt: 0, checkedAt: 0, nextCheckAt: 0, count: 0 })),
  getScpCalls: vi.fn(() => Promise.resolve([])),
  getCallHistory: vi.fn(() =>
    Promise.resolve({
      contest: 'ilqp',
      fileName: 'il-2026.txt',
      entries: {
        K9AAA: { Name: 'BOB', Loc1: 'COOK' },
        K9BBB: { Loc1: 'CHICAGO' },
        W9CCC: { State: 'WI' },
      },
    }),
  ),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null } as unknown as AppSnapshot

/** The Illinois QSO Party as the engine sends it: RST, then the one QTH box. */
const ilqp = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
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
    ...over,
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

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const qthBox = () => document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')[1]
const qthMarked = () =>
  qthBox().classList.contains('le-fd-input-history') &&
  (qthBox().closest('label')!.querySelector('.le-fd-from-history')?.textContent ?? '') === 'history'
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('call history fills the contest strip', () => {
  it('fills the box as the call is typed, marked as from call history', async () => {
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9AA')
    expect(qthBox().value).toBe('')
    type(callBox(), 'K9AAA')
    expect(qthBox().value).toBe('COOK')
    expect(qthMarked()).toBe(true)
    expect(qthBox().title).toContain('il-2026.txt')
    // The report is what you hear: never from the file.
    expect(document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')[0].value).toBe('59')
    expect(api.getCallHistory).toHaveBeenCalledTimes(1)
  })

  it('fills a state for a station outside Illinois, and leaves out what the contest would refuse', async () => {
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'W9CCC')
    expect(qthBox().value).toBe('WI')
    type(callBox(), 'K9BBB') // the file says CHICAGO: a city, not a county
    expect(qthBox().value).toBe('')
    expect(qthMarked()).toBe(false)
  })

  it('never writes over what the operator typed', async () => {
    render(strip(ilqp()))
    await settle()
    type(qthBox(), 'LAKE')
    type(callBox(), 'K9AAA')
    expect(qthBox().value).toBe('LAKE')
    expect(qthMarked()).toBe(false)
  })

  it('gives the box to the operator once they type over a fill', async () => {
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9AAA')
    type(qthBox(), 'DUPG')
    expect(qthMarked()).toBe(false)
    type(callBox(), 'K9AAAB')
    type(callBox(), 'K9AAA')
    expect(qthBox().value).toBe('DUPG')
  })

  it('takes a fill back out when the call stops being the one it was for', async () => {
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9AAA')
    expect(qthBox().value).toBe('COOK')
    type(callBox(), 'K9AAAB')
    expect(qthBox().value).toBe('')
    expect(qthMarked()).toBe(false)
  })

  it('uses a file only for the contest it was imported for', async () => {
    render(strip(ilqp({ event: 'nyqp' } as Partial<FieldDayStatus>)))
    await settle()
    type(callBox(), 'K9AAA')
    expect(qthBox().value).toBe('')
  })

  it('is off in Unassisted mode: nothing filled, and the file is not even read', async () => {
    render(strip(ilqp({ assistanceOn: [] })))
    await settle()
    type(callBox(), 'K9AAA')
    expect(qthBox().value).toBe('')
    expect(api.getCallHistory).not.toHaveBeenCalled()
  })

  it('logs the code it filled, and Clear on an abandoned contact gives the box back', async () => {
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9AAA')
    await act(async () => {
      fireEvent.click(document.querySelector('.le-fd-log-btn')!)
    })
    expect(api.contestLogManual).toHaveBeenCalledWith(
      'K9AAA',
      [
        ['RST', '59'],
        ['QTH', 'COOK'],
      ],
      'PH',
      undefined,
    )
    cleanup()
    render(strip(ilqp()))
    await settle()
    type(callBox(), 'K9AAA')
    expect(qthBox().value).toBe('COOK')
    fireEvent.click(document.querySelector('.le-fd-big .le-qrz')!)
    expect(qthBox().value).toBe('')
    expect(qthMarked()).toBe(false)
  })
})
