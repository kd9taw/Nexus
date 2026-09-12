// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { TopBar } from '../components/TopBar'
import { TempoHeader } from '../components/TempoHeader'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import type { AppSnapshot, Tier } from '../types'
import type { ControlCapability } from './station-operation'
import type { OperationVersion } from './operation-version'

const clients: OperationClient[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => { cleanup(); clients.splice(0).forEach(c => c.disconnected()); vi.useRealTimers() })
const snapshot = (radio = {}) => ({ mycall: 'N0CALL', mygrid: 'AA00', link: { tier: 'FT8', dtSec: 0 },
  radio: { operatingMode: 'digital', source: 'native', dialMhz: 14.074, sideband: 'USB', band: '20m', catOk: true, txEnabled: false, txLevel: 0.5, ...radio }
}) as AppSnapshot
function authority(capabilities: ControlCapability[] = ['tier'], version: OperationVersion = 3) {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const frames: any[] = []
  const client = new OperationClient(s => frames.push(JSON.parse(s)), true, () => 1000, undefined, version)
  clients.push(client); client.open()
  client.receive({ type: 'operationResponse', requestId: frames[frames.length - 1].request.requestId, value: {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: [], txArmed: false, controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities }
  } })
  return client
}
const noop = () => {}
function content(kind: 'advanced' | 'tempo', onTierChange: (tier: Tier) => void, radio = {}) {
  const snap = snapshot(radio), shared = { onTierChange, bandPlan: [], onSetFrequency: noop }
  return kind === 'advanced' ? <TopBar {...shared} tier="FT8" mycall={snap.mycall} mygrid={snap.mygrid} radio={snap.radio} link={snap.link}
    onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} onOpenGuide={noop} />
    : <TempoHeader {...shared} snap={snap} tier="TempoFast" onSetTxLevel={noop} onToggleCqRun={noop} onResumeCqRun={noop} />
}
function wrapped(kind: 'advanced' | 'tempo', onTierChange: (tier: Tier) => void, client: OperationClient | null, radio = {}, available = true, local = false) {
  return <StationControlContext.Provider value={local}><StationDataContext.Provider value={available}>
    <RemoteOperationsContext.Provider value={client}>{content(kind, onTierChange, radio)}</RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>
}
const cases = [['advanced', 'Q65', '.tier-toggle:not(.tx-period) .tier-btn'], ['tempo', 'TempoDeep', '.cockpit-mode']] as const
it.each(cases)('the existing %s buttons stay passive until an explicit tier gesture', (kind, target, selector) => {
  const onTier = vi.fn(), client = authority(), view = render(wrapped(kind, onTier, client))
  view.rerender(wrapped(kind, onTier, client))
  expect(onTier).not.toHaveBeenCalled()
  const button = [...view.container.querySelectorAll<HTMLButtonElement>(selector)].find(e => e.textContent!.includes(target === 'TempoDeep' ? 'Deep' : target))!
  expect(button.disabled).toBe(false)
  fireEvent.click(button)
  expect(onTier).toHaveBeenCalledExactlyOnceWith(target)
})
it.each(cases)('the %s buttons require their own current authority and an idle digital receiver', (kind, _target, selector) => {
  const onTier = vi.fn(), client = authority(), view = render(wrapped(kind, onTier, null))
  const refused = () => {
    const buttons = [...view.container.querySelectorAll<HTMLButtonElement>(selector)]
    expect(buttons.length).toBeGreaterThan(1)
    for (const button of buttons) { expect(button.disabled).toBe(true); fireEvent.click(button) }
    expect(onTier).not.toHaveBeenCalled()
  }
  refused()
  for (const other of [authority(['mode', 'decoder']), authority(['tier'], 2)]) {
    view.rerender(wrapped(kind, onTier, other)); refused()
  }
  for (const radio of [{ operatingMode: 'cw' }, { catOk: false }, { txEnabled: true }, { transmitting: true }, { rigKeyed: true }, { tuning: true }, { txBusyReason: 'Mic PTT is held' }]) {
    view.rerender(wrapped(kind, onTier, client, radio)); refused()
  }
  view.rerender(wrapped(kind, onTier, client, {}, false)); refused()
  view.rerender(wrapped(kind, onTier, client))
  expect([...view.container.querySelectorAll<HTMLButtonElement>(selector)].every(e => !e.disabled)).toBe(true)
  act(() => client.disconnected()); refused()
})
it.each(cases)('local %s tier selection keeps its existing armed behavior', (kind, target, selector) => {
  const onTier = vi.fn(), view = render(wrapped(kind, onTier, null, { txEnabled: true }, true, true))
  const button = [...view.container.querySelectorAll<HTMLButtonElement>(selector)].find(e => e.textContent!.includes(target === 'TempoDeep' ? 'Deep' : target))!
  expect(button.disabled).toBe(false); fireEvent.click(button)
  expect(onTier).toHaveBeenCalledExactlyOnceWith(target)
})
