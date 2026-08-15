// @vitest-environment jsdom
//
// CW CONTACTS LOGGED WITH NO GRID (operator, 2026-08-15).
//
// The grid is not a field the CW operator can type — `asksForGrid` is satellite-only, so the
// Phone and CW strips carry no Grid box at all and `logGrid` is a write-only autofill stash.
// The ONLY thing that fills it is the callbook lookup.
//
// And that lookup only ever ran for calls the operator TYPED. The debounced auto-lookup is gated
// on `humanCallEditRef`, and both machine fills clear it — the CW decoder's, and a clicked spot's.
// In the CW cockpit the call is never typed: the decoder fills it and focus goes straight to RST,
// so the on-blur lookup never fires either (and it is further gated on an empty Name, which the
// decoder also fills). Net effect: every CW QSO reached `logQso` with `grid: null`.
//
// These tests drive the seam that was actually broken — a machine-filled call must reach the
// callbook — and then the consequence, that the grid lands on the record.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso, qrzLookup } from '../api'
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

const mockedQrz = vi.mocked(qrzLookup)
const mockedLogQso = vi.mocked(logQso)

const snap = {
  radio: { band: '20m', dialMhz: 14.05 },
  hunt: null,
} as unknown as AppSnapshot

/** What QRZ answers for the worked station — the grid is the part under test. */
const QRZ_ANSWER = {
  call: 'K9ABC',
  name: 'Dale',
  grid: 'EN52XA',
  state: 'WI',
  country: 'United States',
  qth: 'Madison',
}

beforeEach(() => {
  vi.clearAllMocks()
  mockedQrz.mockResolvedValue(QRZ_ANSWER as never)
})
afterEach(() => cleanup())

/** The CW cockpit's own props, with the decoder's live fill. */
function renderCw(cwLive: { call: string | null; confirmed: boolean }) {
  render(
    <LogEntry
      snap={snap}
      mode="CW"
      defaultRst="599"
      exchange="terrestrial"
      titled={false}
      cwLive={{ call: cwLive.call, rst: null, name: null, confirmed: cwLive.confirmed }}
    />,
  )
}

describe('a CW contact carries the callbook grid', () => {
  it('looks up a call the DECODER filled, not only one the operator typed', async () => {
    renderCw({ call: 'K9ABC', confirmed: true })
    // The lookup is debounced (700 ms) and fires on a timer, so wait for the effect rather
    // than asserting synchronously.
    await waitFor(() => expect(mockedQrz).toHaveBeenCalledWith('K9ABC'), { timeout: 3000 })
  })

  it('puts that grid on the logged record', async () => {
    renderCw({ call: 'K9ABC', confirmed: true })
    await waitFor(() => expect(mockedQrz).toHaveBeenCalled(), { timeout: 3000 })
    // The grid is invisible on this strip by design, so the assertion has to be made where the
    // operator actually feels it: the record handed to the backend.
    await waitFor(() => expect(screen.getByDisplayValue('Dale')).toBeTruthy(), { timeout: 3000 })

    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await waitFor(() => expect(mockedLogQso).toHaveBeenCalled(), { timeout: 3000 })
    const rec = mockedLogQso.mock.calls[0][0] as unknown as { call: string; grid: string | null }
    expect(rec.call.toUpperCase()).toBe('K9ABC')
    expect(rec.grid, 'the CW QSO must log with the callbook grid, not null').toBe('EN52XA')
  })

  it('does NOT look up an unconfirmed best guess', async () => {
    // The decoder walks K9 → K9A → K9ABC. Enriching each step would spend the operator's QRZ
    // quota on calls nobody worked, which is why the fix keys on `confirmed` and not merely on
    // "the field is non-empty". This is the half that keeps the fix honest.
    renderCw({ call: 'K9AB', confirmed: false })
    await new Promise((r) => setTimeout(r, 1200))
    expect(mockedQrz).not.toHaveBeenCalled()
  })
})
