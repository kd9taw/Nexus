// @vitest-environment jsdom
//
// #222 — the callbook-lookup toast now carries the short-path bearing from the operator's
// grid to the looked-up station, the same figure the StationCard already shows after the
// country. Before the fix the detail line stopped at the state.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { qrzLookup } from '../api'
import { pushToast } from '../toast'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((fn: () => unknown) => fn()),
}))

const mockedQrz = vi.mocked(qrzLookup)
const mockedToast = vi.mocked(pushToast)

// EN52 (WI) → FN31 (CT): a real, non-zero short-path bearing to the east.
const snap = {
  radio: { band: '20m', dialMhz: 14.05 },
  hunt: null,
  mygrid: 'EN52',
} as unknown as AppSnapshot

const QRZ_ANSWER = { call: 'K9ABC', name: 'Dale', grid: 'FN31', state: 'NY', country: 'United States' }

beforeEach(() => {
  vi.clearAllMocks()
  mockedQrz.mockResolvedValue(QRZ_ANSWER as never)
})
afterEach(() => cleanup())

describe('the callbook lookup detail carries the bearing', () => {
  it('appends the short-path azimuth after name/grid/state', async () => {
    render(<LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} />)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'K9ABC' } })
    fireEvent.click(screen.getByRole('button', { name: /^lookup$/i }))
    await waitFor(() => expect(mockedToast).toHaveBeenCalled(), { timeout: 3000 })
    const msg = String(mockedToast.mock.calls[0][0])
    // The only source of a degree figure in the detail is the new bearing.
    expect(msg, msg).toMatch(/\d+°/)
  })
})
