// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { CockpitHeader } from '../components/CockpitHeader'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { setOperatingMode } from '../api'
import type { AppSnapshot } from '../types'
import type { ControlCapability } from './station-operation'

vi.mock('../api', () => ({ setFrequency: vi.fn(), setOperatingMode: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
const clients: OperationClient[] = []
afterEach(() => {
  cleanup()
  clients.splice(0).forEach(c => c.disconnected())
  vi.useRealTimers()
  vi.clearAllMocks()
})
const snapshot = (radio = {}) => ({ radio: { operatingMode: 'digital', dialMhz: 14.074, sideband: 'USB', catOk: true, txEnabled: false, ...radio } }) as AppSnapshot
function authority(capabilities: ControlCapability[] = ['mode']) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const frames: any[] = []
  const client = new OperationClient(s => frames.push(JSON.parse(s)), true, () => 1000, undefined, 2)
  clients.push(client)
  client.open()
  client.receive({ type: 'operationResponse', requestId: frames[frames.length - 1].request.requestId, value: {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: [], txArmed: false, controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities }
  } })
  return client
}
function header(client: OperationClient | null, snap = snapshot(), onSnap = vi.fn(), available = true, local = false) {
  return <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
    <RemoteOperationsContext.Provider value={client}>
      <CockpitHeader snap={snap} remoteMode="cw" modeIndicator={<span>CW</span>} bandControl={<span>20m</span>} onSnap={onSnap} />
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
}

it('mounting and revisiting the actual cockpit header is passive; only the mode button submits', async () => {
  const client = authority(), onSnap = vi.fn(), later = snapshot({ operatingMode: 'cw', dialMhz: 14.030 })
  let finish!: (s: AppSnapshot) => void
  vi.mocked(setOperatingMode).mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const view = render(header(client, snapshot(), onSnap))
  view.rerender(header(client, snapshot(), onSnap))
  expect(setOperatingMode).not.toHaveBeenCalled()
  expect(view.container.querySelector('.ch-identity .remote-mode-entry')).not.toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Use this mode' }))
  expect(setOperatingMode).toHaveBeenCalledExactlyOnceWith('cw', true)
  expect(onSnap).not.toHaveBeenCalled()
  await act(async () => finish(later))
  await waitFor(() => expect(onSnap).toHaveBeenCalledExactlyOnceWith(later))
  view.rerender(header(client, later, onSnap))
  expect(view.queryByRole('button', { name: 'Use this mode' })).toBeNull()
})

it('observation, missing capability, stale data and lost authority cannot switch modes', () => {
  const client = authority(), view = render(header(null))
  const refused = () => {
    const button = view.getByRole('button', { name: 'Use this mode' }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    fireEvent.click(button)
    expect(setOperatingMode).not.toHaveBeenCalled()
  }
  refused()
  view.rerender(header(authority(['frequency', 'decoder'])))
  refused()
  view.rerender(header(client, snapshot(), vi.fn(), false))
  refused()
  view.rerender(header(client))
  expect((view.getByRole('button', { name: 'Use this mode' }) as HTMLButtonElement).disabled).toBe(false)
  act(() => client.disconnected())
  refused()
})

it.each([
  { catOk: false }, { txEnabled: true }, { transmitting: true }, { tuning: true },
  { rigKeyed: true }, { txBusyReason: 'Mic PTT is held' }
])('does not enter a different mode while station state is %j', radio => {
  const view = render(header(authority(), snapshot(radio)))
  const button = view.getByRole('button', { name: 'Use this mode' }) as HTMLButtonElement
  expect(button.disabled).toBe(true)
  fireEvent.click(button)
  expect(setOperatingMode).not.toHaveBeenCalled()
})

it('leaves the local header unchanged even when its section differs from the snapshot', () => {
  const view = render(header(null, snapshot(), vi.fn(), true, true))
  expect(view.queryByRole('button', { name: 'Use this mode' })).toBeNull()
  expect(setOperatingMode).not.toHaveBeenCalled()
})
