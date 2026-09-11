// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SessionStatus } from './SessionStatus'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import type { OperationState } from './operation-protocol'

const clients: OperationClient[] = []
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.disconnected()); vi.useRealTimers() })
function fixture(loggingOnly = false) {
  vi.useFakeTimers()
  const sent: { request: { type: string; requestId: string; operationId?: string } }[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value) }, removeItem: (key: string) => { values.delete(key) } }
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined,
    loggingOnly ? 1 : 3, pendingControlStorage(() => storage, 'session-test', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: loggingOnly ? ['log.manual'] : [], txArmed: false,
    ...(loggingOnly ? {} : { controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['amplifier' as const] } })
  }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent.at(-1)!.request.requestId, value })
  client.open(); reply(state)
  const disconnect = vi.fn()
  const view = (stale = false) => <SessionStatus client={client} stale={stale} disconnect={disconnect} />
  return { ...render(view()), view, client, state, sent, reply, disconnect }
}
const toggle = () => screen.getByRole('button', { name: 'Remote access' })

it('keeps the current authority outside optional help and releases only on an explicit click', async () => {
  const h = fixture()
  expect(screen.getByText('Station control active').closest('.remote-session-info')).toBeNull()
  const release = screen.getByRole('button', { name: 'Release station control' })
  expect(release.closest('.remote-session-info')).toBeNull()
  expect(screen.queryByRole('button', { name: 'Disconnect and return to stations' })).toBeNull()
  const messages = h.sent.length, lease = h.client.getSnapshot().state?.leaseId
  fireEvent.click(toggle())
  expect(toggle().getAttribute('aria-expanded')).toBe('true')
  expect(screen.getByRole('button', { name: 'Disconnect and return to stations' })).toBeTruthy()
  fireEvent.click(toggle())
  h.rerender(h.view())
  expect(toggle().getAttribute('aria-expanded')).toBe('false')
  expect(h.client.getSnapshot().state?.leaseId).toBe(lease)
  expect(h.sent).toHaveLength(messages)
  fireEvent.click(release)
  expect(h.sent.at(-1)?.request.type).toBe('release')
  act(() => h.reply({ ...h.state, phase: 'available', leaseId: null, commandWindowId: null, nextSequence: null, leaseRemainingMs: null }))
  await act(async () => {})
  expect(screen.getByRole('button', { name: 'Take station control' })).toBeTruthy()
  expect(h.disconnect).not.toHaveBeenCalled()
})

it('keeps data loss and command recovery visible while help stays closed, without resending a command', async () => {
  const h = fixture()
  let pending!: ReturnType<OperationClient['control']>
  await act(async () => {
    pending = h.client.control({ action: 'amplifier.operate', expectedOperate: false, operate: true })
    await Promise.resolve()
  })
  const operationId = h.sent.at(-1)!.request.requestId
  await act(async () => {
    h.reply({ operation: 'stationControl', operationId, outcome: 'unknown', reason: 'hardwareUnconfirmed' })
    await pending
  })
  h.rerender(h.view(true))
  expect(screen.getByRole('alert').closest('.remote-session-info')).toBeNull()
  expect(screen.getByRole('alert').textContent).toContain('Station data unavailable')
  for (const name of ['Check command result', 'I checked the station — finish this check']) {
    expect(screen.getByRole('button', { name }).closest('.remote-session-info')).toBeNull()
  }
  expect(toggle().getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(screen.getByRole('button', { name: 'Check command result' }))
  expect(h.sent.at(-1)?.request).toMatchObject({ type: 'result', operationId })
  await act(async () => h.reply({ operation: 'stationControl', operationId, outcome: 'unknown', reason: 'hardwareUnconfirmed' }))
  expect(h.sent.filter(message => message.request.type === 'stationControl')).toHaveLength(1)
  act(() => h.client.disconnected())
  expect(screen.getByText('Station control disconnected').closest('.remote-session-info')).toBeNull()
})

it('retains the older logging-only wording and disconnect action', () => {
  const h = fixture(true)
  expect(screen.getByRole('button', { name: 'Release logging control' })).toBeTruthy()
  expect(screen.queryByText('Station control active')).toBeNull()
  fireEvent.click(toggle())
  expect(screen.getByText('Station display with optional manual logging. Radio and transmit controls remain unavailable.')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Disconnect and return to stations' }))
  expect(h.disconnect).toHaveBeenCalledOnce()
})

it.each([null, false])('identifies an observation-only session without offering authority (%s)', enabled => {
  const client = enabled === null ? null : new OperationClient(() => { throw Error('observer sent a command') }, enabled)
  render(<SessionStatus client={client} stale={false} disconnect={() => {}} />)
  expect(screen.getByRole('status').textContent).toBe('Monitoring only')
  expect(screen.getAllByRole('button')).toHaveLength(1)
  fireEvent.click(toggle())
  expect(screen.getByText('Monitoring only. Operating controls are not available in this preview.')).toBeTruthy()
})
