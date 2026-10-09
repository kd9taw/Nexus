// @vitest-environment jsdom
//
// THE LOG STRIP TELLS ITS COCKPIT THE CALL IT WOULD LOG (`onEntryCall`), for the Rotor box beside the
// cockpit: once the call has stood for ENTRY_SETTLE_MS, so a call typed a key at a time is one report
// and not one per keystroke (each report re-renders the window and asks the station for a bearing);
// a call filled from outside (a decoder, a click-to-work) is reported like a typed one; and the strip
// withdraws its call the moment it goes, since a strip that is not on screen has nothing in it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { ENTRY_SETTLE_MS, LogEntry } from './LogEntry'
import type { AppSnapshot } from '../types'

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
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((fn: () => unknown) => fn()),
}))

vi.setConfig({ testTimeout: 15_000 })

const snap = { radio: { band: '20m', dialMhz: 14.05 }, hunt: null, mygrid: 'EN52' } as unknown as AppSnapshot

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

const settle = () => new Promise((r) => setTimeout(r, ENTRY_SETTLE_MS + 100))

describe('the log strip reports the call it would log', () => {
  it('once, when the typing stops, trimmed and uppercased', async () => {
    const report = vi.fn()
    render(<LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} onEntryCall={report} />)
    const call = screen.getByPlaceholderText('Call')
    for (const v of ['e', 'ec', 'ec1', 'ec1d', 'ec1dd']) fireEvent.change(call, { target: { value: v } })
    await waitFor(() => expect(report).toHaveBeenCalledWith('EC1DD'))
    await settle()
    expect(report.mock.calls).toEqual([['EC1DD']])
  })

  it('a call filled from outside the strip is reported like a typed one', async () => {
    const report = vi.fn()
    const { rerender } = render(
      <LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} onEntryCall={report} />,
    )
    rerender(
      <LogEntry
        snap={snap}
        mode="CW"
        defaultRst="599"
        exchange="terrestrial"
        titled={false}
        onEntryCall={report}
        cwLive={{ call: 'AA1AA', rst: null, name: null, confirmed: true }}
      />,
    )
    await waitFor(() => expect(report).toHaveBeenLastCalledWith('AA1AA'))
  })

  it('clearing the call reports none, and so does the strip going', async () => {
    const report = vi.fn()
    const { unmount } = render(
      <LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} onEntryCall={report} />,
    )
    const call = screen.getByPlaceholderText('Call')
    fireEvent.change(call, { target: { value: 'EC1DD' } })
    await waitFor(() => expect(report).toHaveBeenLastCalledWith('EC1DD'))
    fireEvent.change(call, { target: { value: '' } })
    await waitFor(() => expect(report).toHaveBeenLastCalledWith(''))
    fireEvent.change(call, { target: { value: 'AA1AA' } })
    await waitFor(() => expect(report).toHaveBeenLastCalledWith('AA1AA'))
    unmount()
    expect(report).toHaveBeenLastCalledWith('')
  })
})
