// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { OperateQsoStrip } from '../components/OperateQsoStrip'
import { TxPanel } from '../components/TxPanel'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import type { QsoStatus, RadioStatus } from '../types'
const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers() })
function fixture(granted = true, exchange = false) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const sent: any[] = [], c = new OperationClient(w => sent.push(JSON.parse(w)), true, () => 1000, undefined, 4)
  clients.push(c); c.open()
  c.receive({ type: 'operationResponse', requestId: sent[0].request.requestId, value: { stationBootId: crypto.randomUUID(), allowed: true,
    phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1,
    leaseRemainingMs: 5000, actions: [], txArmed: false, transmitEpoch: granted ? '0000000000000001' : null,
    controls: { context: { radioId: 0, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: granted ? ['ftOperate', ...(exchange ? ['ftExchange', 'ftMessages'] : [])] : ['decoder'] } } })
  return { c, sent }
}
function strip(c: OperationClient, available = true, send?: (text: string) => Promise<boolean>) {
  const cq = vi.fn(), tx = vi.fn(), halt = vi.fn(), other = vi.fn()
  const content = (data: boolean) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={data}><RemoteOperationsContext.Provider value={c}>
    <OperateQsoStrip qso={{ running: false, state: 'Idle', dxcall: 'W1AW', txNow: 'W1AW KD9TAW EN52' } as QsoStatus}
      radio={{ txEnabled: false, tuning: false, holdTxFreq: false, catOk: true } as RadioStatus}
      onCallCq={cq} onSetTxEnabled={tx} onHaltTx={halt} onSetMode={other} onResend={other} onFreetext={send ?? other} onLog={other} onSetTune={other} onSetHoldTxFreq={other} />
  </RemoteOperationsContext.Provider></StationDataContext.Provider></StationControlContext.Provider>
  const view = render(content(available))
  return { cq, tx, halt, other, view, stale: () => view.rerender(content(false)) }
}
const button = (name: RegExp) => screen.getByRole('button', { name }) as HTMLButtonElement
it('uses the existing Tx panel for deliberate message choices and keeps drafts editable without transmit authority', () => {
  const h = fixture(true, true), send = vi.fn(), edit = vi.fn()
  const content = (available: boolean) => <StationControlContext.Provider value={false}><StationDataContext.Provider value={available}><RemoteOperationsContext.Provider value={h.c}>
    <TxPanel dxCall="W1AW" dxGrid="FN31" onDxCall={edit} onDxGrid={edit}
      messages={{ tx1: 'W1AW KD9TAW EN52', tx2: 'W1AW KD9TAW -10', tx3: 'W1AW KD9TAW R-10', tx4: 'W1AW KD9TAW RR73', tx5: '73', tx6: 'CQ KD9TAW EN52' }}
      tx5="73" tx6="CQ KD9TAW EN52" onTx5={edit} onTx6={edit} nextIndex={null}
      onTx={send} onGenerate={edit} onClear={edit} qsoMacros={[]} />
  </RemoteOperationsContext.Provider></StationDataContext.Provider></StationControlContext.Provider>
  const view = render(content(true))
  expect(send).not.toHaveBeenCalled()
  for (let n = 1; n <= 6; n++) { const e = button(new RegExp(`^Tx ${n}$`)); expect(e.disabled).toBe(false); fireEvent.click(e); expect(send).toHaveBeenLastCalledWith(n) }
  view.rerender(content(false))
  for (let n = 1; n <= 6; n++) expect(button(new RegExp(`^Tx ${n}$`)).disabled).toBe(true)
  const input = document.querySelector('.txp-dx input') as HTMLInputElement
  expect(input.disabled).toBe(false)
  fireEvent.change(input, { target: { value: 'K2ABC' } }); expect(edit).toHaveBeenLastCalledWith('K2ABC')
  expect(send).toHaveBeenCalledTimes(6)
})
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
it('disables ordinary FT gestures when request capacity is unavailable and keeps Stop usable', () => {
  const h = fixture()
  vi.spyOn(h.c, 'getSnapshot').mockReturnValue({ ...h.c.getSnapshot(), requestReady: false })
  const ui = strip(h.c)
  for (const name of [/call cq/i, /^tx off$/i]) {
    expect(button(name).disabled).toBe(true)
    fireEvent.click(button(name))
  }
  expect(ui.cq).not.toHaveBeenCalled(); expect(ui.tx).not.toHaveBeenCalled()
  expect(button(/stop tx/i).disabled).toBe(false)
  fireEvent.click(button(/stop tx/i)); expect(ui.halt).toHaveBeenCalledOnce()
})


it('uses existing exchange controls and clears only a confirmed Remote free-text draft', async () => {
  let confirm!: (accepted: boolean) => void
  const send = vi.fn(() => new Promise<boolean>(resolve => { confirm = resolve }))
  const h = fixture(true, true), ui = strip(h.c, true, send)
  const input = screen.getByRole('textbox') as HTMLInputElement
  fireEvent.change(input, { target: { value: 'TNX 73' } })
  fireEvent.submit(input.closest('form')!)
  expect(send).toHaveBeenCalledExactlyOnceWith('TNX 73', expect.objectContaining({ dxcall: 'W1AW', txNow: 'W1AW KD9TAW EN52' }))
  expect(input.value).toBe('TNX 73')
  await act(async () => confirm(false))
  expect(input.value).toBe('TNX 73')
  fireEvent.submit(input.closest('form')!)
  await act(async () => confirm(true))
  expect(input.value).toBe('')
  fireEvent.click(document.querySelector('.cq-resend')!)
  expect(ui.other).toHaveBeenLastCalledWith(expect.objectContaining({ dxcall: 'W1AW' }))
  fireEvent.click(document.querySelectorAll('.cq-role')[1])
  expect(ui.other).toHaveBeenLastCalledWith('qso-monitor', expect.objectContaining({ dxcall: 'W1AW' }))
})
