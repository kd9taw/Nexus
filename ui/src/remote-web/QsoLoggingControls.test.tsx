// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { LogConfirm } from '../components/LogConfirm'
import { OperateQsoStrip } from '../components/OperateQsoStrip'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import type { LoggedQso, QsoStatus, RadioStatus } from '../types'

const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers() })
function fixture(granted: boolean, stop = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const sent: any[] = [], client = new OperationClient(w => sent.push(JSON.parse(w)), true, () => 1000, undefined, 4)
  clients.push(client); client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, ...(stop ? { transmitEpoch: '0000000000000001' } : {}), actions: granted ? ['log.manual'] : [], txArmed: false,
    controls: { context: { radioId: 0, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities: granted ? ['qsoLogging'] : ['decoder'] }
  } })
  return client
}
const pending = { call: 'W9XYZ', grid: 'EN37', rstSent: '-07', rstRcvd: '-10', band: '20m', mode: 'FT8', whenUnix: 1700000000 } as LoggedQso
const button = (name: RegExp) => screen.getByRole('button', { name }) as HTMLButtonElement

it('the existing confirmation dialog requires logging permission and retains edits when readings become stale', () => {
  const denied = fixture(false), allowed = fixture(true), confirm = vi.fn(), discard = vi.fn()
  const content = (client: OperationClient, fresh = true) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={fresh}><RemoteOperationsContext.Provider value={client}>
    <LogConfirm record={pending} onConfirm={confirm} onDiscard={discard} />
  </RemoteOperationsContext.Provider></StationDataContext.Provider></StationControlContext.Provider>
  const ui = render(content(denied))
  expect(button(/log qso/i).disabled).toBe(true)
  expect(button(/discard/i).disabled).toBe(true)
  ui.rerender(content(allowed))
  const call = screen.getAllByRole('textbox')[0]
  fireEvent.change(call, { target: { value: 'K2ABC' } })
  expect(button(/log qso/i).disabled).toBe(false)
  ui.rerender(content(allowed, false))
  expect(button(/log qso/i).disabled).toBe(true)
  expect((call as HTMLInputElement).value).toBe('K2ABC')
  fireEvent.click(button(/log qso/i)); expect(confirm).not.toHaveBeenCalled()
  ui.rerender(content(allowed))
  fireEvent.click(button(/log qso/i)); expect(confirm).toHaveBeenCalledWith({ ...pending, call: 'K2ABC' })
  expect(discard).not.toHaveBeenCalled()
})

it('the existing Log QSO button uses logging permission without radio or transmit authority', () => {
  const client = fixture(true), log = vi.fn(), other = vi.fn()
  render(<StationControlContext.Provider value={false}><RemoteOperationsContext.Provider value={client}>
    <OperateQsoStrip qso={{ running: false, state: 'AwaitRoger', dxcall: 'W9XYZ', txNow: 'W9XYZ K2DEF R-07' } as QsoStatus}
      radio={{ txEnabled: false, tuning: false, holdTxFreq: false, catOk: false } as RadioStatus}
      onCallCq={other} onSetTxEnabled={other} onHaltTx={other} onSetMode={other} onResend={other} onFreetext={other}
      onLog={log} onSetTune={other} onSetHoldTxFreq={other} />
  </RemoteOperationsContext.Provider></StationControlContext.Provider>)
  expect(button(/^log$/i).disabled).toBe(false)
  fireEvent.click(button(/^log$/i)); expect(log).toHaveBeenCalledOnce()
  expect(button(/call cq/i).disabled).toBe(true)
  expect(other).not.toHaveBeenCalled()
})

it('keeps the existing Stop TX reachable from a stale remote confirmation dialog', () => {
  const client = fixture(true, true), stop = vi.fn(), confirm = vi.fn()
  render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={false}><RemoteOperationsContext.Provider value={client}>
    <LogConfirm record={pending} onConfirm={confirm} onDiscard={confirm} onStop={stop} />
  </RemoteOperationsContext.Provider></StationDataContext.Provider></StationControlContext.Provider>)
  expect(button(/log qso/i).disabled).toBe(true)
  expect(button(/stop tx/i).disabled).toBe(false)
  expect(button(/stop tx/i).getAttribute('data-remote-stop')).toBe('true')
  fireEvent.click(button(/stop tx/i)); expect(stop).toHaveBeenCalledOnce()
  expect(confirm).not.toHaveBeenCalled()
})
