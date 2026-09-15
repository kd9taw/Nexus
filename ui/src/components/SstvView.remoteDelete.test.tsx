// @vitest-environment jsdom
// Deleting a received picture from the browser. It is irreversible and it is the operator's only
// copy, so the ✕ is live only while the station offers its gallery verb, it confirms first, and
// what it sends names the gallery ROW — never a station path.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { SstvView } from './SstvView'
import { getSstvState, sstvDeleteImage, sstvDeleteImageRemote } from '../api'
import { confirmDialog } from '../confirm'
import { useSstvImage } from '../remote-web/useSstvImage'
import { parseSstvSample } from '../remote-web/sstv'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from '../remote-web/operation-client'
import { pendingControlStorage } from '../remote-web/control-storage'
import { pendingLogStorage, type ReceiptLock } from '../remote-web/operation-storage'
import type { OperationState } from '../remote-web/operation-protocol'
import type { ControlCapability } from '../remote-web/station-operation'
import sstvFixture from '../remote-web/__fixtures__/sstv.json'
import type { AppSnapshot } from '../types'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => true) }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(async () => null),
  getLicensedBandPlan: vi.fn(async () => []),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  sstvDeleteImage: vi.fn(),
  sstvDeleteImageRemote: vi.fn(),
  setOperatingMode: vi.fn(),
  setRfPower: vi.fn(async () => {}),
}))
vi.mock('../remote-web/useSstvImage', () => ({ useSstvImage: vi.fn() }))
afterEach(() => { cleanup(); vi.clearAllMocks() })

const snap = { mycall: 'N0CALL', radio: { dialMhz: 14.23, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, tuning: false, txAllowed: true } } as unknown as AppSnapshot
const gallery = (sstvFixture as { state: { gallery: { path: string; mode: string; finishedUtc: string }[] } }).state.gallery

function operations(capabilities: ControlCapability[]) {
  const values = new Map<string, string>(), sent: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const client = new OperationClient(s => sent.push(s), true, () => 1000, pendingLogStorage(() => data, 'station', lock), 3, pendingControlStorage(() => data, 'station', lock))
  client.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } }
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: state })
  return client
}

function browser(capabilities: ControlCapability[], children: ReactNode = <SstvView snap={snap} active />) {
  vi.mocked(getSstvState).mockImplementation(async () => parseSstvSample(structuredClone(sstvFixture), 0))
  vi.mocked(useSstvImage).mockReturnValue({ url: 'blob:nexus-station-image', failed: false, retry: vi.fn() })
  const source = { client: { supports: () => true } } as unknown as RemoteCollections
  return render(
    <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
      <RemoteOperationsContext.Provider value={operations(capabilities)}>
        <RemoteCollectionsContext.Provider value={source}>{children}</RemoteCollectionsContext.Provider>
      </RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>)
}

const buttons = (view: ReturnType<typeof render>) => [...view.container.querySelectorAll<HTMLButtonElement>('.sstv-thumb-del')]

it('confirms, then deletes the row it was shown, naming no station path', async () => {
  expect(gallery.length).toBeGreaterThan(0)
  const view = browser(['sstvGallery'])
  await waitFor(() => expect(buttons(view)).toHaveLength(gallery.length))
  expect(buttons(view)[0].disabled).toBe(false)
  fireEvent.click(buttons(view)[0])
  await waitFor(() => expect(sstvDeleteImageRemote).toHaveBeenCalled())
  expect(confirmDialog).toHaveBeenCalledTimes(1)
  expect(sstvDeleteImageRemote).toHaveBeenCalledWith(gallery[0].finishedUtc, gallery[0].mode)
  // The desktop's path-taking command is not what a browser reaches.
  expect(sstvDeleteImage).not.toHaveBeenCalled()
})

it('deletes nothing when the operator says no', async () => {
  vi.mocked(confirmDialog).mockResolvedValueOnce(false)
  const view = browser(['sstvGallery'])
  await waitFor(() => expect(buttons(view)).toHaveLength(gallery.length))
  fireEvent.click(buttons(view)[0])
  await waitFor(() => expect(confirmDialog).toHaveBeenCalledTimes(1))
  expect(sstvDeleteImageRemote).not.toHaveBeenCalled()
  expect(sstvDeleteImage).not.toHaveBeenCalled()
})

it('is dead for a browser the station does not offer the gallery verb to', async () => {
  const view = browser(['decoder', 'rotator', 'workSpot'])
  await waitFor(() => expect(buttons(view)).toHaveLength(gallery.length))
  for (const button of buttons(view)) expect(button.disabled).toBe(true)
  fireEvent.click(buttons(view)[0])
  await Promise.resolve()
  expect(confirmDialog).not.toHaveBeenCalled()
  expect(sstvDeleteImageRemote).not.toHaveBeenCalled()
  cleanup()
  // Positive control: the same gallery with the hint has the same buttons live.
  const allowed = browser(['sstvGallery'])
  await waitFor(() => expect(buttons(allowed)).toHaveLength(gallery.length))
  expect(buttons(allowed).every(b => !b.disabled)).toBe(true)
})
