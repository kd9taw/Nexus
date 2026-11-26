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
import { dopplerDrivesDial, setQsyWipe } from '../features/qsyWipe'
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

/** The station on `dialMhz`, the rig's own PTT read back keyed or not. */
const at = (dialMhz: number, rigKeyed = false) =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: { band: '20m', dialMhz, sideband: 'CW', operatingMode: 'cw', rigKeyed },
    link: { tier: 'CW' },
    hunt: null,
    bandChangeTick: 1,
  }) as unknown as AppSnapshot

const entry = (snap: AppSnapshot, logger: boolean, satellite: string | null = null, dopplerTracking = false) => (
  <ContestLargeEntry
    snap={snap}
    fieldDay={party()}
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

  it('a track drives the dial only while Doppler corrects a leg (App reads this from its pass poll)', () => {
    const track = (dopplerDownlink: boolean, dopplerUplink: boolean) =>
      ({ name: 'SO-50', state: 'tracking', mode: 'rotor+doppler', dopplerDownlink, dopplerUplink }) as unknown as Parameters<typeof dopplerDrivesDial>[0]
    expect(dopplerDrivesDial(null)).toBe(false)
    expect(dopplerDrivesDial(track(false, false))).toBe(false)
    expect(dopplerDrivesDial(track(true, false))).toBe(true)
    expect(dopplerDrivesDial(track(false, true))).toBe(true)
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
