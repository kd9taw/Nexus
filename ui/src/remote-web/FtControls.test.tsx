// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { OperateQsoStrip } from '../components/OperateQsoStrip'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import type { QsoStatus, RadioStatus } from '../types'
const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers() })
function fixture(granted = true) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const sent: any[] = [], c = new OperationClient(w => sent.push(JSON.parse(w)), true, () => 1000, undefined, 4)
  clients.push(c); c.open()
  c.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: { stationBootId: crypto.randomUUID(), allowed: true,
    phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1,
    leaseRemainingMs: 5000, actions: [], txArmed: false, transmitEpoch: granted ? '0000000000000001' : null,
    controls: { context: { radioId: 0, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: granted ? ['ftOperate'] : ['decoder'] } } })
  return { c, sent }
}
function strip(c: OperationClient, available = true) {
  const cq = vi.fn(), tx = vi.fn(), halt = vi.fn(), other = vi.fn()
  const content = (data: boolean) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={data}><RemoteOperationsContext.Provider value={c}>
    <OperateQsoStrip qso={{ running: false, state: 'Idle', dxcall: null, txNow: null } as QsoStatus}
      radio={{ txEnabled: false, tuning: false, holdTxFreq: false, catOk: true } as RadioStatus}
      onCallCq={cq} onSetTxEnabled={tx} onHaltTx={halt} onSetMode={other} onResend={other} onFreetext={other} onLog={other} onSetTune={other} onSetHoldTxFreq={other} />
  </RemoteOperationsContext.Provider></StationDataContext.Provider></StationControlContext.Provider>
  const view = render(content(available))
  return { cq, tx, halt, other, view, stale: () => view.rerender(content(false)) }
}
const button = (name: RegExp) => screen.getByRole('button', { name }) as HTMLButtonElement
it('uses the existing CQ and TX latch while retaining independent Stop outside removable panes', () => {
  const h = fixture(), ui = strip(h.c)
  expect(ui.cq).not.toHaveBeenCalled(); expect(ui.tx).not.toHaveBeenCalled()
  expect(button(/call cq/i).disabled).toBe(false); fireEvent.click(button(/call cq/i)); expect(ui.cq).toHaveBeenCalledOnce()
  expect(button(/^tx off$/i).disabled).toBe(false); fireEvent.click(button(/^tx off$/i)); expect(ui.tx).toHaveBeenCalledExactlyOnceWith(true)
  expect(button(/^tune$/i).disabled).toBe(true)
  expect(button(/stop tx/i).closest('[data-pane-id]')).toBeNull()
  ui.stale()
  expect(button(/call cq/i).disabled).toBe(true)
  expect(button(/stop tx/i).disabled).toBe(false); fireEvent.click(button(/stop tx/i)); expect(ui.halt).toHaveBeenCalledOnce()
  act(() => h.c.disconnected())
  expect(button(/stop tx/i).disabled).toBe(true)
  expect(ui.other).not.toHaveBeenCalled()
})
it('receiver permission alone does not enable FT operation or Stop', () => {
  const h = fixture(false); strip(h.c)
  for (const name of [/call cq/i, /^tx off$/i, /stop tx/i]) expect(button(name).disabled).toBe(true)
})
