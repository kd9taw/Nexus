// @vitest-environment jsdom
//
// A NEW PARK CLICKED IN NEEDED FILLS THE STRIP, AND NOTHING IS HUNTED. The operator: "There's no way
// to clear a 'POTA - Hunted' tag if you click through from 'Needed'. It should instead simply populate
// the call data (and the park number) without setting it as 'Hunted'." So the Needed click hands the
// row's park to this strip with its call (`pendingWork.park`), and the contact logged from the strip
// carries that park on the record itself: no pending hunt is there for the engine to tag it with.
//
// The rest of the file is what a hunted park already does in this strip, held for a clicked one: it
// survives the call change that clears the previous station's callbook fill, it leaves with its
// station, Clear really clears it, and a hunt pending for the same station at another park does not
// replace it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso, qrzLookup } from '../api'
import type { AppSnapshot, LoggedQso } from '../types'

// THE BUDGET: the house 15 s, as LogEntry.huntPrefill.test.tsx has it (the same strip, the same waits).
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (LogEntry.huntPrefill's pattern); the overrides are
  // the callbook, the park directory and the log write this file reads.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setCwPeerInfo: vi.fn(async () => {}),
    setLogFormGrid: vi.fn(async () => {}),
    logQso: vi.fn(async (rec: unknown) => rec),
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const mockedQrz = vi.mocked(qrzLookup)
const mockedLog = vi.mocked(logQso)

type Hunt = { program: string; reference: string; call: string }
type Work = { call: string; ts: number; park?: { program: string; reference: string } | null }
const snapWith = (hunt: Hunt | null) =>
  ({ mycall: 'KD9TAW', radio: { band: '20m', dialMhz: 14.25 }, hunt }) as unknown as AppSnapshot
const pota = (reference: string) => ({ program: 'POTA', reference })

function strip(hunt: Hunt | null, work: Work | null) {
  return <LogEntry snap={snapWith(hunt)} mode="SSB" defaultRst="59" exchange="terrestrial" pendingWork={work} />
}
const park = () => document.querySelector('input.le-park-ref') as HTMLInputElement
const callBox = () => document.querySelector('input.le-call') as HTMLInputElement
const logButton = () => screen.getByRole('button', { name: /^log$/i })
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
/** The record the Log button wrote. */
async function logged(): Promise<LoggedQso> {
  fireEvent.click(logButton())
  await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
  return mockedLog.mock.calls[0][0] as LoggedQso
}

beforeEach(() => {
  mockedQrz.mockReset().mockImplementation(async () => null as never)
  mockedLog.mockClear()
})
afterEach(cleanup)

describe('a park clicked in Needed fills the strip with its call', () => {
  it('fills the call and the park, with no hunt, and the record carries the park', async () => {
    render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(callBox().value).toBe('K9XYZ'))
    await settle()
    expect(park().value, 'the clicked park never reached the strip').toBe('US-1000')
    expect((await logged()).ota, 'the contact went out without its park').toEqual({ theirProgram: 'POTA', theirRef: 'US-1000' })
  })

  it('shows no hunt tag for it', async () => {
    render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    expect(document.querySelector('.le-hunt-chip')).toBeNull()
  })

  // CONTROL — a strip that filled a park on every handoff would pass both tests above.
  it('a station handed over with no park fills no park, and its record names none', async () => {
    render(strip(null, { call: 'W9XYZ', ts: 1, park: null }))
    await waitFor(() => expect(callBox().value).toBe('W9XYZ'))
    await settle()
    expect(park().value).toBe('')
    expect((await logged()).ota).toBeUndefined()
  })

  // The race the hunted park had (LogEntry.huntPrefill): the click lands a NEW call, and in that
  // render the call-change effect clears the previous station's callbook fill and its park box.
  it('fills it when the previous station had a callbook answer', async () => {
    mockedQrz.mockImplementation(async (call: string) => ({ call, name: 'Pat', qth: 'Springfield' }) as never)
    const view = render(strip(null, { call: 'W1AAA', ts: 1, park: null }))
    await waitFor(() => expect((screen.getByDisplayValue('Pat') as HTMLInputElement).value).toBe('Pat'))
    view.rerender(strip(null, { call: 'K9XYZ', ts: 2, park: pota('US-3000') }))
    await waitFor(() => expect(callBox().value).toBe('K9XYZ'))
    await settle()
    expect(park().value, 'the call change emptied the clicked park').toBe('US-3000')
  })

  it('takes a re-click of the same station at its new park', async () => {
    const view = render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    view.rerender(strip(null, { call: 'K9XYZ', ts: 2, park: pota('US-2000') }))
    await settle()
    expect(park().value).toBe('US-2000')
  })

  // App's own work hint repeats a docked click as a second handoff of the same call with no park.
  it('keeps the park through a second handoff of the same station with no park', async () => {
    const view = render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    view.rerender(strip(null, { call: 'K9XYZ', ts: 2, park: null }))
    await settle()
    expect(park().value).toBe('US-1000')
    expect((await logged()).ota).toEqual({ theirProgram: 'POTA', theirRef: 'US-1000' })
  })
})

describe('the clicked park belongs to its station', () => {
  it('leaves when the operator types another call over it', async () => {
    render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    fireEvent.change(callBox(), { target: { value: 'W1ABC' } })
    await settle()
    expect(park().value, 'the clicked station’s park rode onto another call').toBe('')
    expect((await logged()).ota).toBeUndefined()
  })

  it('leaves when the operator erases the call', async () => {
    render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    fireEvent.change(callBox(), { target: { value: '' } })
    await settle()
    expect(park().value, 'the clicked park stayed in a strip with no call').toBe('')
  })

  // A hunt pending for another station still fills for that station once its call is typed.
  it('gives way to another station’s hunt when its call is typed', async () => {
    render(strip({ program: 'POTA', reference: 'US-5000', call: 'W1ABC' }, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    fireEvent.change(callBox(), { target: { value: 'W1ABC' } })
    await settle()
    expect(park().value).toBe('US-5000')
  })

  // CONTROL — the same station under a portable suffix is still the station clicked.
  it('stays when the call only gains a /P', async () => {
    render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    fireEvent.change(callBox(), { target: { value: 'K9XYZ/P' } })
    await settle()
    expect(park().value).toBe('US-1000')
  })

  it('Clear empties it, and it does not come back for the same call typed again', async () => {
    render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    fireEvent.click(screen.getByRole('button', { name: /^clear$/i }))
    await settle()
    expect(park().value).toBe('')
    fireEvent.change(callBox(), { target: { value: 'K9XYZ' } })
    await settle()
    expect(park().value, 'a cleared park came back').toBe('')
  })
})

describe('a hunt pending from the POTA/SOTA view', () => {
  it('does not replace the park clicked for the same station at another park', async () => {
    render(strip({ program: 'POTA', reference: 'US-1111', call: 'K9XYZ' }, { call: 'K9XYZ', ts: 1, park: pota('US-2222') }))
    await waitFor(() => expect(callBox().value).toBe('K9XYZ'))
    await settle()
    expect(park().value).toBe('US-2222')
    expect((await logged()).ota).toEqual({ theirProgram: 'POTA', theirRef: 'US-2222' })
  })

  // Two activators at one park: the hunt is for K9XYZ, the click is for W1ABC there. The engine's
  // auto-tag matches by call and would never tag W1ABC, so the strip has to send the park itself.
  it('a second activator at the hunted park is logged with the park', async () => {
    render(strip({ program: 'POTA', reference: 'US-1000', call: 'K9XYZ' }, { call: 'W1ABC', ts: 1, park: pota('US-1000') }))
    await waitFor(() => expect(callBox().value).toBe('W1ABC'))
    await settle()
    expect(park().value).toBe('US-1000')
    expect((await logged()).ota, 'the second activator’s park was left to a hunt that is not theirs').toEqual({
      theirProgram: 'POTA',
      theirRef: 'US-1000',
    })
  })

  // The operator's latest act wins: a HUNT on the POTA/SOTA view for the same station, at the park
  // it has moved to, after the Needed click. App hands that over as the call alone.
  it('a hunt set afterwards for the same station at another park takes over', async () => {
    const view = render(strip(null, { call: 'K9XYZ', ts: 1, park: pota('US-2222') }))
    await waitFor(() => expect(park().value).toBe('US-2222'))
    view.rerender(strip({ program: 'POTA', reference: 'US-3333', call: 'K9XYZ' }, { call: 'K9XYZ', ts: 2, park: null }))
    await settle()
    expect(park().value, 'the clicked park outlived the hunt set after it').toBe('US-3333')
    expect((await logged()).ota).toBeUndefined()
  })

  // CONTROL — the hunted station's own prefill is still left to the engine's auto-tag, which also
  // ends the hunt: the strip must not send it as a park of its own.
  it('the hunted station’s own park is still left to the hunt', async () => {
    render(strip({ program: 'POTA', reference: 'US-1000', call: 'K9XYZ' }, { call: 'K9XYZ', ts: 1, park: null }))
    await waitFor(() => expect(park().value).toBe('US-1000'))
    expect(document.querySelector('.le-hunt-chip.match')).toBeTruthy()
    expect((await logged()).ota).toBeUndefined()
  })
})
