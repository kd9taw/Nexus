// @vitest-environment jsdom
//
// THE LOG STRIP TELLS ITS COCKPIT WHAT A MACRO'S {RST} AND {HISNAME} READ (`onMacroFields`): the
// report this contact will log as SENT and the name in its Name box. A macro that keyed 599 while
// the strip said 549 put one report on the air and another in the log.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
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

describe('the log strip reports what a macro keys as {RST} and {HISNAME}', () => {
  it('the mode’s report until one is typed, then the typed one; the Name box as it stands', async () => {
    const report = vi.fn()
    render(<LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} onMacroFields={report} />)
    await waitFor(() => expect(report).toHaveBeenLastCalledWith({ rstSent: '599', name: '' }))
    fireEvent.change(screen.getByTitle('Signal report you SENT them').querySelector('input')!, { target: { value: '549' } })
    await waitFor(() => expect(report).toHaveBeenLastCalledWith({ rstSent: '549', name: '' }))
    fireEvent.change(screen.getByPlaceholderText('Name'), { target: { value: 'Bob' } })
    await waitFor(() => expect(report).toHaveBeenLastCalledWith({ rstSent: '549', name: 'Bob' }))
  })
})
