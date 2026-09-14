// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SplitControl } from '../components/SplitControl'
import { TuningStrip } from '../components/TuningStrip'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient, OperationFailure } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { AppSnapshot } from '../types'
import type { ControlCapability } from './station-operation'
import { t } from '../i18n'

// Remote parity batch 1: SPLIT, XIT and the VFO buttons are live for a browser holding the
// splitTuning hint and RIT for ritTuning. The transmit-frequency controls stay off while this
// browser's transmission is armed; RIT is receive-only and stays live.

vi.mock('../api', () => ({
  setSplit: vi.fn(async () => null), setRit: vi.fn(async () => null), setXit: vi.fn(async () => null),
  setVfo: vi.fn(async () => null), setFrequency: vi.fn(async () => null)
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
import { setRit, setSplit, setVfo, setXit } from '../api'
import { pushToast } from '../toast'

const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.clearAllMocks() })
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve() })

function station(capabilities: ControlCapability[], txArmed: boolean) {
  const sent: any[] = [], values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000 + performance.now(), undefined, 4,
    pendingControlStorage(() => storage, 'split-gestures', async (_key, run) => run()))
  clients.push(client)
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value: {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed,
    transmitEpoch: txArmed ? '0123456789abcdef' : null,
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null }, capabilities } } })
  return client
}

const snap = { radio: { dialMhz: 14.03, band: '20m', sideband: 'USB', catOk: true, source: 'native', splitTxMhz: null,
  ritHz: 0, xitHz: 0, activeVfo: 'A', txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txAllowed: true } } as unknown as AppSnapshot

function mount(capabilities: ControlCapability[], { local = false, txArmed = false } = {}) {
  const client = station(capabilities, txArmed)
  const onError = vi.fn()
  const ui = render(<StationControlContext.Provider value={local}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={local ? null : client}>
      <SplitControl snap={snap} onError={onError}/>
      <TuningStrip snap={snap} showReadout={false}/>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  const button = (name: string) => screen.getByRole('button', { name })
  const clar = (which: 'rit' | 'xit') => {
    const reset = screen.getByTitle(t(`cockpit.tuning.${which}.title`)).closest('span')!
    return [...reset.querySelectorAll('button')] as HTMLButtonElement[]
  }
  const vfo = (letter: 'A' | 'B') => screen.getByTitle(t('cockpit.tuning.vfo.title', { vfo: letter })) as HTMLButtonElement
  return { ui, onError, button, clar, vfo }
}

it('SPLIT, XIT and VFO are live with splitTuning and send the operator choice', async () => {
  const h = mount(['splitTuning'])
  expect((h.button('SPLIT') as HTMLButtonElement).disabled).toBe(false)
  fireEvent.click(h.button('SPLIT')); await flush()
  expect(vi.mocked(setSplit).mock.calls[0][0]).toBeCloseTo(14.035, 6)
  expect(h.vfo('B').disabled).toBe(false)
  fireEvent.click(h.vfo('B')); await flush()
  expect(setVfo).toHaveBeenCalledWith('B')
  const [, , xitUp] = h.clar('xit')
  expect(xitUp.disabled).toBe(false)
  fireEvent.click(xitUp); await flush()
  expect(setXit).toHaveBeenCalledWith(10)
  // RIT needs its own hint.
  for (const b of h.clar('rit')) expect(b.disabled).toBe(true)
})

it('RIT is live with ritTuning alone, and a refused remote change says why', async () => {
  const h = mount(['ritTuning'])
  const [, , ritUp] = h.clar('rit')
  expect(ritUp.disabled).toBe(false)
  vi.mocked(setRit).mockRejectedValueOnce(new OperationFailure('stationBusy', true, true))
  fireEvent.click(ritUp); await flush()
  expect(setRit).toHaveBeenCalledWith(10)
  expect(pushToast).toHaveBeenCalledWith(t('remote.controlBusy'), 'error')
  expect((h.button('SPLIT') as HTMLButtonElement).disabled).toBe(true)
  expect(h.vfo('B').disabled).toBe(true)
})

it('a split outside the licence is reported as that, not as a generic failure', async () => {
  const h = mount(['splitTuning'])
  vi.mocked(setSplit).mockRejectedValueOnce(new Error('outsidePrivileges'))
  fireEvent.click(h.button('SPLIT')); await flush()
  expect(h.onError).toHaveBeenCalledWith(t('remote.b1.outsidePrivileges'))
})

it('without a hint nothing is live and nothing is sent; the local desktop keeps every control', async () => {
  const h = mount(['frequency'])
  fireEvent.click(h.button('SPLIT'))
  fireEvent.click(h.vfo('B'))
  for (const b of [...h.clar('rit'), ...h.clar('xit')]) { expect(b.disabled).toBe(true); fireEvent.click(b) }
  await flush()
  for (const fn of [setSplit, setVfo, setRit, setXit]) expect(fn).not.toHaveBeenCalled()
  cleanup()
  const local = mount([], { local: true })
  expect((local.button('SPLIT') as HTMLButtonElement).disabled).toBe(false)
  expect(local.vfo('B').disabled).toBe(false)
  for (const b of [...local.clar('rit'), ...local.clar('xit')]) expect(b.disabled).toBe(false)
})

it('while this browser transmission is armed, split, XIT and VFO stay off and RIT stays live', async () => {
  const h = mount(['splitTuning', 'ritTuning'], { txArmed: true })
  expect((h.button('SPLIT') as HTMLButtonElement).disabled).toBe(true)
  expect(h.vfo('B').disabled).toBe(true)
  for (const b of h.clar('xit')) expect(b.disabled).toBe(true)
  for (const b of h.clar('rit')) expect(b.disabled).toBe(false)
})
