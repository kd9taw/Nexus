// @vitest-environment jsdom
//
// POINT-AT-CALL AIMS AT THE STATION, NOT ITS COUNTRY (a tester's report, 2026-09-29).
//
// The rotator's "→ CALL" used to aim at the centre of the station's DXCC entity: 207° for EC1DD
// from JO21EV, where QRZ (and the station's own grid) say 227°. It now aims at the best location
// Nexus already has for the call, and one of those is this form: the grid it holds for the call it
// is logging, filled in from the callbook or — on a satellite exchange, the one strip with a Grid
// box — typed by the operator. These tests hold the hand-off to the engine to its rules: a real
// square only, keyed to its call, and nothing at all while a call is being typed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { qrzLookup, setLogFormGrid } from '../api'
import type { AppSnapshot } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "costs nothing while a call is typed with no square to…", takes
// 1.22 s and 1.22 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const mockedQrz = vi.mocked(qrzLookup)
const mockedSetGrid = vi.mocked(setLogFormGrid)

const snap = {
  radio: { band: '20m', dialMhz: 14.25 },
  hunt: null,
} as unknown as AppSnapshot

beforeEach(() => {
  vi.clearAllMocks()
  mockedQrz.mockResolvedValue(null as never)
})
afterEach(() => cleanup())

const callBox = () => screen.getByPlaceholderText('Call')

describe('the log form tells the rotator where the station is', () => {
  it('hands over the callbook grid it filled in, keyed to the call (the tester’s Phone case)', async () => {
    mockedQrz.mockResolvedValue({ call: 'EC1DD', name: 'Pepe', grid: 'IN52TK' } as never)
    render(<LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} />)
    fireEvent.change(callBox(), { target: { value: 'EC1DD' } })
    await waitFor(() => expect(mockedSetGrid).toHaveBeenCalledWith('EC1DD', 'IN52TK'), { timeout: 3000 })
    expect(mockedSetGrid).toHaveBeenCalledTimes(1)
  })

  it('costs nothing while a call is typed with no square to go with it', async () => {
    render(<LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} />)
    for (const partial of ['E', 'EC', 'EC1', 'EC1D', 'EC1DD']) {
      fireEvent.change(callBox(), { target: { value: partial } })
    }
    // Past the form's own 700 ms lookup debounce, whose answer here is "no record".
    await new Promise((r) => setTimeout(r, 1200))
    expect(mockedSetGrid).not.toHaveBeenCalled()
  })

  it('a square typed into the satellite strip reaches it, and clearing the box forgets it', async () => {
    render(<LogEntry snap={snap} mode="FM" defaultRst="59" exchange="satellite" titled={false} />)
    fireEvent.change(callBox(), { target: { value: 'EC1DD' } })
    const grid = screen.getByPlaceholderText('Grid')
    fireEvent.change(grid, { target: { value: 'IN5' } })
    expect(mockedSetGrid, 'a half-typed square is not a location').not.toHaveBeenCalled()
    fireEvent.change(grid, { target: { value: 'IN52' } })
    await waitFor(() => expect(mockedSetGrid).toHaveBeenLastCalledWith('EC1DD', 'IN52'))
    fireEvent.change(grid, { target: { value: 'IN52TK' } })
    await waitFor(() => expect(mockedSetGrid).toHaveBeenLastCalledWith('EC1DD', 'IN52TK'))
    fireEvent.change(grid, { target: { value: '' } })
    await waitFor(() => expect(mockedSetGrid).toHaveBeenLastCalledWith('EC1DD', ''))
  })
})
