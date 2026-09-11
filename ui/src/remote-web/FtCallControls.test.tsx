// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { OperateDecodes } from '../components/OperateDecodes'
import { OperateRoster } from '../components/OperateRoster'
import { StationCard } from '../components/StationCard'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import type { DecodeRow, Station } from '../types'

vi.mock('../api', () => ({ getDeclination: vi.fn(async () => 0), openQrzPage: vi.fn() }))
const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers(); localStorage.clear() })
function owner(capability: 'ftCall' | 'ftOperate') {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const sent: any[] = [], client = new OperationClient(w => sent.push(JSON.parse(w)), true, () => 1000, undefined, 4)
  clients.push(client); client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: [], txArmed: false, transmitEpoch: '0000000000000001',
    controls: { context: { radioId: 0, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: [capability] }
  } })
  return client
}
const station: Station = { call: 'W1AW', grid: 'FN31', snr: -12, lastHeardSlot: 100,
  heardCount: 1, presence: 'active', worked: false, tier: 'FT8', freqHz: 1250 }
const decode: DecodeRow = { from: 'W1AW', message: 'CQ W1AW FN31', snr: -12, freqHz: 1250,
  dtSec: 0.1, isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: 0 }

it.each(['ftCall', 'ftOperate'] as const)('existing decode, roster and card gestures require %s to support calling', capability => {
  const client = owner(capability), call = vi.fn(), select = vi.fn(), ignore = vi.fn(), rx = vi.fn(), spot = vi.fn()
  const children = <>
    <OperateDecodes decodes={[decode]} slot={100} rxOffsetHz={1250} band="20m" tier="FT8"
      harqRescues={0} onCall={call} onSelectDecode={select} onToggleIgnore={ignore} onSetRx={rx} />
    <OperateRoster stations={[station]} myGrid="EN52" currentSlot={100} needByCall={new Map()}
      selectedCall="W1AW" onSelect={select} onCall={call} onToggleIgnore={ignore} onSpot={spot} />
    <StationCard station={station} myGrid="EN52" currentSlot={100} selected={false}
      unread={0} need={null} needAll={[]} onSelect={select} onCall={call} />
  </>
  const tree = (fresh: boolean) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={fresh}>
    <RemoteOperationsContext.Provider value={client}>{children}</RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
  const view = render(tree(true))
  const decoded = view.container.querySelector('.decode-row')!, roster = view.container.querySelector('.or-row[aria-selected]')!
  expect(decoded).not.toBeNull(); expect(roster).not.toBeNull()
  fireEvent.dblClick(decoded)
  fireEvent.focus(decoded)
  fireEvent.keyDown(decoded, { key: 'Enter', shiftKey: true })
  fireEvent.dblClick(roster)
  fireEvent.focus(roster)
  fireEvent.keyDown(roster, { key: 'Enter', shiftKey: true })
  fireEvent.dblClick(view.container.querySelector('.station-card')!)
  if (capability === 'ftCall') {
    expect(call).toHaveBeenCalledTimes(5)
    expect(call).toHaveBeenNthCalledWith(1, 'W1AW', undefined, 'CQ W1AW FN31', -12, 1250)
    expect(call).toHaveBeenNthCalledWith(3, 'W1AW', 'FN31', undefined, undefined, 1250)
    expect(call).toHaveBeenNthCalledWith(5, 'W1AW', 'FN31', undefined, undefined, 1250, 'FT8')
  } else expect(call).not.toHaveBeenCalled()
  call.mockClear()
  fireEvent.dblClick(decoded, { altKey: true }); fireEvent.dblClick(decoded, { ctrlKey: true })
  fireEvent.dblClick(roster, { altKey: true }); fireEvent.click(view.container.querySelector('.or-spot')!)
  expect(call).not.toHaveBeenCalled(); expect(ignore).not.toHaveBeenCalled(); expect(rx).not.toHaveBeenCalled(); expect(spot).not.toHaveBeenCalled()
  view.rerender(tree(false))
  fireEvent.dblClick(decoded); fireEvent.dblClick(roster); fireEvent.dblClick(view.container.querySelector('.station-card')!)
  expect(call).not.toHaveBeenCalled()
})
