// @vitest-environment jsdom
//
// A PARK'S CONTACT TAKES THE PARK'S STATE. The callbook's state is the licensee's address, where
// the activator lives and not where they are operating, so an Ohio ham in a North Dakota park was
// logged OH and Worked All States credited Ohio. The strip puts the park's own state in the STATE
// box, where the operator sees it and can change it, and tells the station where the box's value
// came from: at the station a park's state outranks a callbook's, and the operator's outranks both.
//
// Every fixture gives the callbook and the park DIFFERENT states, so a test can only pass when the
// right one wins.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor, act, within } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso, lookupPark, qrzLookup } from '../api'
import type { AppSnapshot } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "replaces the callbook’s state when the hunt arrives…", takes
// 0.74 s and 0.74 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
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
    // The logbook never answers: the recall card is not what this file is about.
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const mockedQrz = vi.mocked(qrzLookup)
const mockedPark = vi.mocked(lookupPark)
const mockedLog = vi.mocked(logQso)

type Hunt = { program: string; reference: string; call: string; states?: string[] }
const snapWith = (hunt: Hunt | null) =>
  ({ mycall: 'KD9TAW', radio: { band: '20m', dialMhz: 14.25 }, hunt }) as unknown as AppSnapshot

/** An Ohio-licensed activator in a North Dakota park. */
const W8OH: Hunt = { program: 'POTA', reference: 'US-0001', call: 'W8OH', states: ['US-ND'] }

function strip(hunt: Hunt | null, work: { call: string; ts: number } | null) {
  return (
    <LogEntry snap={snapWith(hunt)} mode="SSB" defaultRst="59" exchange="terrestrial" pendingWork={work} />
  )
}
const stateBox = () => document.querySelector('input.le-state') as HTMLInputElement
const parkBox = () => document.querySelector('input.le-park-ref') as HTMLInputElement
const logButton = () => screen.getByRole('button', { name: /^log$/i })
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })

/** The callbook answers with the licensee's address: Ohio. */
function callbookSaysOhio() {
  mockedQrz.mockImplementation(async (call: string) => ({ call, name: 'Olive', qth: 'Columbus', state: 'OH' }) as never)
}

/** What the strip sent: the record's state and where the strip says it came from. */
async function logged(): Promise<{ state: string | null; source: unknown }> {
  fireEvent.click(logButton())
  await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
  const [rec, source] = mockedLog.mock.calls[0] as unknown as [{ state: string | null }, unknown]
  return { state: rec.state, source }
}

beforeEach(() => {
  mockedQrz.mockReset().mockImplementation(async () => null as never)
  mockedPark.mockReset().mockImplementation(async () => null as never)
  mockedLog.mockClear()
})
afterEach(cleanup)

describe('a hunted park puts its own state in the STATE box', () => {
  it('fills the box with the park’s state and logs it as the park’s', async () => {
    render(strip(W8OH, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(stateBox().value).toBe('ND'))
    expect(await logged()).toEqual({ state: 'ND', source: 'park' })
  })

  it('replaces the callbook’s state when the hunt arrives after the lookup', async () => {
    callbookSaysOhio()
    const view = render(strip(null, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(stateBox().value).toBe('OH'))
    view.rerender(strip(W8OH, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(stateBox().value).toBe('ND'))
    expect(await logged()).toEqual({ state: 'ND', source: 'park' })
  })

  it('is not replaced by the callbook’s state when the lookup answers after it', async () => {
    callbookSaysOhio()
    render(strip(W8OH, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(stateBox().value).toBe('ND'))
    // The lookup has answered: its name is in the strip, and its state is not.
    await waitFor(() => expect(screen.getByDisplayValue('Olive')).toBeTruthy())
    await settle()
    expect(stateBox().value).toBe('ND')
    expect(await logged()).toEqual({ state: 'ND', source: 'park' })
  })

  it('leaves the recall card the callbook’s town and state, never the town with the park’s state', async () => {
    // The card says where the station lives ("Columbus, OH"); the box says where the contact
    // counts. "Columbus, ND" would be neither.
    callbookSaysOhio()
    render(strip(W8OH, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('Olive')).toBeTruthy())
    await settle()
    expect(stateBox().value).toBe('ND')
    expect(screen.queryByText(/Columbus, ND/), 'the town with the park’s state').toBeNull()
    expect(screen.getByText(/Columbus, OH/)).toBeTruthy()
  })

  it('never replaces a state the operator typed, before the hunt or after it', async () => {
    const view = render(strip(null, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('W8OH')).toBeTruthy())
    fireEvent.change(stateBox(), { target: { value: 'MN' } })
    view.rerender(strip(W8OH, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(parkBox().value).toBe('US-0001'))
    await settle()
    expect(stateBox().value).toBe('MN')
    expect(await logged()).toEqual({ state: 'MN', source: 'operator' })
  })

  it('leaves the box alone for a hunt the park cannot place, so the callbook’s state stays a callbook’s', async () => {
    callbookSaysOhio()
    render(strip({ ...W8OH, states: [] }, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(stateBox().value).toBe('OH'))
    expect(await logged()).toEqual({ state: 'OH', source: 'callbook' })
  })
})

describe('a park the operator types', () => {
  it('puts its looked-up state in the box, and takes it away again when the park goes', async () => {
    mockedPark.mockImplementation(async (ref: string) =>
      (ref === 'US-0004' ? { reference: 'US-0004', name: 'Lake', grid: '', location: 'US-SD', states: ['US-SD'] } : null) as never,
    )
    render(strip(null, { call: 'K0ABC', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('K0ABC')).toBeTruthy())
    fireEvent.change(parkBox(), { target: { value: 'US-0004' } })
    await waitFor(() => expect(stateBox().value).toBe('SD'))
    fireEvent.change(parkBox(), { target: { value: '' } })
    await waitFor(() => expect(stateBox().value).toBe(''))
  })
})

describe('a park on a state line', () => {
  const LINE: Hunt = { program: 'POTA', reference: 'US-0003', call: 'W8OH', states: ['US-MT', 'US-ND'] }
  const pick = () => screen.getByRole('group', { name: /state line/i })
  const choice = (code: string) => within(pick()).getByRole('button', { name: code })

  it('asks which of its states, empties the callbook’s, and logs no state until one is picked', async () => {
    callbookSaysOhio()
    const view = render(strip(null, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(stateBox().value).toBe('OH'))
    view.rerender(strip(LINE, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(choice('MT')).toBeTruthy())
    expect(choice('ND')).toBeTruthy()
    expect(stateBox().value, 'the licence’s Ohio is no pick').toBe('')
    expect(choice('MT').getAttribute('aria-pressed')).toBe('false')
    expect(await logged()).toEqual({ state: null, source: 'park' })
  })

  it('keeps the callbook’s state out when the lookup answers after the park, so the box shows what logs', async () => {
    // The order a spot click makes: the hunt and the call land together, and the lookup answers
    // after. The empty box is the park's undecided answer, not a blank for the callbook to fill.
    callbookSaysOhio()
    render(strip(LINE, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(choice('MT')).toBeTruthy())
    await waitFor(() => expect(screen.getByDisplayValue('Olive')).toBeTruthy())
    await settle()
    expect(stateBox().value, 'the licence’s Ohio is no pick').toBe('')
    expect(await logged()).toEqual({ state: null, source: 'park' })
  })

  it('logs the state the operator picks', async () => {
    render(strip(LINE, { call: 'W8OH', ts: 1 }))
    await waitFor(() => expect(choice('ND')).toBeTruthy())
    fireEvent.click(choice('ND'))
    expect(stateBox().value).toBe('ND')
    expect(choice('ND').getAttribute('aria-pressed')).toBe('true')
    expect(choice('MT').getAttribute('aria-pressed')).toBe('false')
    expect(await logged()).toEqual({ state: 'ND', source: 'operator' })
  })

  it('asks for a typed park whose lookup names two states, and not for one in a single state', async () => {
    mockedPark.mockImplementation(async (ref: string) =>
      (ref === 'US-0003'
        ? { reference: 'US-0003', name: 'Line', grid: '', location: 'US-MT,US-ND', states: ['US-MT', 'US-ND'] }
        : ref === 'US-0004'
          ? { reference: 'US-0004', name: 'Lake', grid: '', location: 'US-SD', states: ['US-SD'] }
          : null) as never,
    )
    render(strip(null, { call: 'K0ABC', ts: 1 }))
    await waitFor(() => expect(screen.getByDisplayValue('K0ABC')).toBeTruthy())
    fireEvent.change(parkBox(), { target: { value: 'US-0003' } })
    await waitFor(() => expect(choice('MT')).toBeTruthy())
    fireEvent.change(parkBox(), { target: { value: 'US-0004' } })
    await waitFor(() => expect(stateBox().value).toBe('SD'))
    expect(screen.queryByRole('group', { name: /state line/i })).toBeNull()
  })
})
