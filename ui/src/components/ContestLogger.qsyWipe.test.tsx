// @vitest-environment jsdom
//
// "Wipe an unlogged call when I QSY" ON THE CONTEST SCREEN AND IN THE LOGGER WINDOW (operator yes):
// the large log line (`ContestLargeEntry`) offers the same QSY wipe as the cockpits' strips, by the
// same rules — a move held for QSY_HOLD_MS, never an over — with the contest notice's wording: a
// contest contact logs on the frequency the radio is on. Never while the logger window says a
// satellite pass's transponder is held (the dial follows Doppler).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, fireEvent } from '@testing-library/react'
import { ContestLargeEntry } from './ContestLogger'
import { passHoldsDial, setQsyWipe } from '../features/qsyWipe'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return { ...auto, contestLogManualRows: vi.fn(async () => []), searchParks: vi.fn(async () => []), askLog: vi.fn(() => new Promise(() => {})) }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action().catch(() => null)),
}))

const party = (): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    event: 'ilqp',
    role: 'in_state',
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

/** The station on `dialMhz`, the rig's own PTT read back keyed or not; `shared`, the one entry the
 *  two windows show while the logger window is open (`contestEntry`). */
const at = (dialMhz: number, rigKeyed = false, shared?: { rev: number; call: string }) =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: { band: '20m', dialMhz, sideband: 'CW', operatingMode: 'cw', rigKeyed },
    link: { tier: 'CW' },
    hunt: null,
    bandChangeTick: 1,
    ...(shared ? { contestEntry: { rev: shared.rev, contact: 0, call: shared.call, fields: {}, marks: null } } : {}),
  }) as unknown as AppSnapshot

const entry = (snap: AppSnapshot, logger: boolean, satellite: string | null = null, dopplerTracking = false, fd = party()) => (
  <ContestLargeEntry
    snap={snap}
    fieldDay={fd}
    fdActive
    fdRuleset={null}
    title="Contest"
    sharedEntry={logger ? 'logger' : undefined}
    operator=""
    onSaveOperator={() => {}}
    satellite={satellite}
    dopplerTracking={dopplerTracking}
    entryOnly
  />
)
const callBox = () => document.querySelector<HTMLInputElement>('input.le-fd-input-call')!
const notice = () => document.querySelector('.le-band-cleared')?.textContent ?? ''
const notices = () => document.querySelectorAll('.le-band-cleared').length
const wait = (ms: number) => act(async () => { vi.advanceTimersByTime(ms) })

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] })
  setQsyWipe(true)
})
afterEach(() => {
  setQsyWipe(false)
  vi.useRealTimers()
})

describe('the QSY wipe on the Contest screen and in the logger window', () => {
  for (const [where, logger] of [['the Contest screen', false], ['the logger window', true]] as const) {
    it(`${where}: a held QSY sets the line aside, saying a contest contact logs where the radio is`, async () => {
      const view = render(entry(at(14.025), logger))
      fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
      view.rerender(entry(at(14.0255), logger))
      await wait(5000)
      expect(callBox().value, 'within 1 kHz on CW').toBe('K9AAA')
      view.rerender(entry(at(14.03), logger))
      await wait(1000)
      expect(callBox().value, 'not before the move holds').toBe('K9AAA')
      await wait(1500)
      expect(callBox().value).toBe('')
      expect(notice()).toContain('radio')
    })

    it(`${where}: an over read at the split TX VFO never wipes`, async () => {
      const view = render(entry(at(14.025), logger))
      fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
      view.rerender(entry(at(14.027), logger))
      await wait(1000)
      view.rerender(entry(at(14.027, true), logger))
      await wait(3000)
      view.rerender(entry(at(14.025), logger))
      await wait(5000)
      expect(callBox().value).toBe('K9AAA')
    })
  }

  it('the logger window holding a satellite pass leaves the line alone: the dial follows Doppler', async () => {
    const view = render(entry(at(435.8), true, 'SO-50'))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    view.rerender(entry(at(435.81), true, 'SO-50'))
    await wait(5000)
    expect(callBox().value).toBe('K9AAA')
  })

  it('the main window\'s Contest screen during a Doppler-tracked pass leaves the line alone', async () => {
    const view = render(entry(at(435.8), false, null, true))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    view.rerender(entry(at(435.81), false, null, true))
    await wait(5000)
    expect(callBox().value).toBe('K9AAA')
  })

  it('any satellite pass up holds the wipe, Doppler on or not (App reads this from its pass poll)', () => {
    const track = (dopplerDownlink: boolean) =>
      ({ name: 'SO-50', state: 'armed', mode: 'rotor-only', dopplerDownlink, dopplerUplink: false }) as unknown as Parameters<typeof passHoldsDial>[0]
    expect(passHoldsDial(null)).toBe(false)
    expect(passHoldsDial(track(false))).toBe(true)
    expect(passHoldsDial(track(true))).toBe(true)
  })

  it('TWO WINDOWS, ONE LINE: only the logger window runs the wipe, so one notice, one Restore', async () => {
    const both = (dialMhz: number) => (
      <>
        {entry(at(dialMhz, false, { rev: 1, call: 'K9AAA' }), false)}
        {entry(at(dialMhz, false, { rev: 1, call: 'K9AAA' }), true)}
      </>
    )
    const view = render(both(14.025))
    await wait(0)
    const calls = () => [...document.querySelectorAll<HTMLInputElement>('input.le-fd-input-call')].map((b) => b.value)
    expect(calls()).toEqual(['K9AAA', 'K9AAA'])
    view.rerender(both(14.03))
    await wait(2500)
    expect(notices(), 'one window set the line aside, not both').toBe(1)
    expect(calls(), 'the main window waits for the shared line to empty').toEqual(['K9AAA', ''])
  })

  it('a notice ends when its line takes up a call typed in the other window: its Restore can never overwrite it', async () => {
    const view = render(entry(at(14.025), false))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    view.rerender(entry(at(14.03), false))
    await wait(2500)
    expect(notices()).toBe(1)
    // The logger window opens and the operator there types the next station.
    view.rerender(entry(at(14.03, false, { rev: 4, call: 'W1BBB' }), false))
    await wait(0)
    expect(callBox().value).toBe('W1BBB')
    expect(notices(), 'the stale Restore is gone').toBe(0)
  })

  it('a notice ends when that contact is logged from the other window', async () => {
    const view = render(entry(at(14.025), false))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    view.rerender(entry(at(14.03), false))
    await wait(2500)
    expect(notices()).toBe(1)
    const logged = { ...party(), qsoCount: 1, log: [{ call: 'K9AAA', band: '20m', mode: 'CW', whenUnix: 1_792_342_800, rcvd: ['599', 'KANE'] }] } as unknown as FieldDayStatus
    view.rerender(entry(at(14.03), false, null, false, logged))
    await wait(0)
    expect(notices(), 'no Restore of a contact already in the log').toBe(0)
  })

  it('is off by default', async () => {
    setQsyWipe(false)
    const view = render(entry(at(14.025), false))
    fireEvent.change(callBox(), { target: { value: 'K9AAA' } })
    view.rerender(entry(at(14.03), false))
    await wait(5000)
    expect(callBox().value).toBe('K9AAA')
  })
})
