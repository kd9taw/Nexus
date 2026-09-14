// @vitest-environment jsdom
// Remote log entry reads the station's offline park directory, and only that. The live POTA
// lookup never runs from a browser, and an older station that offers no directory gets no reads.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import { lookupPark, lookupParkLive, searchParks } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(async () => ({})),
  contestLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  getLog: vi.fn(async () => []),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
}))
afterEach(() => { cleanup(); vi.clearAllMocks() })

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null, mygrid: 'FN31', stations: [] } as unknown as AppSnapshot
const acadia = { reference: 'US-0001', name: 'Acadia National Park', grid: 'FN54', location: 'US-ME', latitude: null, longitude: null }
const remote = { submit: vi.fn(async () => {}), canSubmit: false, busy: false, resetKey: 0, recall: () => null }
function view(parks: boolean) {
  const source = { client: { supports: (c: string) => parks && c === 'get_remote_parks' } } as unknown as RemoteCollections
  return render(<RemoteCollectionsContext.Provider value={source}>
    <LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" remote={remote} />
  </RemoteCollectionsContext.Provider>)
}
const type = (container: HTMLElement, value: string) =>
  fireEvent.change(container.querySelector('.le-park-ref')!, { target: { value } })

it('suggests parks and shows the detail chip from the station directory, without the live lookup', async () => {
  vi.mocked(searchParks).mockResolvedValue([acadia])
  vi.mocked(lookupPark).mockResolvedValue(acadia)
  const { container } = view(true)
  type(container, 'us-00')
  await waitFor(() => expect(searchParks).toHaveBeenCalledWith('US-00', 8))
  await waitFor(() => expect(container.querySelectorAll('.le-park-suggest li')).toHaveLength(1))
  type(container, 'US-0001')
  await waitFor(() => expect(lookupPark).toHaveBeenCalledWith('US-0001'))
  await waitFor(() => expect(container.querySelector('.le-park-detail-name')?.textContent).toBe('Acadia National Park'))
  // Not in the station's list: the desktop falls back to the live directory; the browser does not.
  vi.mocked(lookupPark).mockResolvedValue(null)
  type(container, 'US-0002')
  await waitFor(() => expect(lookupPark).toHaveBeenCalledWith('US-0002'))
  await new Promise(resolve => setTimeout(resolve, 50))
  expect(lookupParkLive).not.toHaveBeenCalled()
  expect(container.querySelector('.le-park-detail-name')).toBeNull()
})

it('makes no park reads for a station that does not offer its directory', async () => {
  const { container } = view(false)
  type(container, 'US-0001')
  // Positive control on timing: the same wait is long enough for both debounced reads above.
  await new Promise(resolve => setTimeout(resolve, 400))
  expect(searchParks).not.toHaveBeenCalled()
  expect(lookupPark).not.toHaveBeenCalled()
  expect(lookupParkLive).not.toHaveBeenCalled()
})
