import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import type { OperationState } from './operation-protocol'

afterEach(() => vi.useRealTimers())

function fixture(control = false) {
  vi.useFakeTimers()
  let now = 1000, unlock!: () => void
  const gate = new Promise<void>(resolve => { unlock = resolve })
  const exclusive = async <T,>(run: () => T | Promise<T>): Promise<T> => { await gate; return run() }
  const operationId = crypto.randomUUID(), sent: { request: { type: string; requestId: string; operationId?: string } }[] = []
  let saved: string | null = operationId
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => now,
    control ? undefined : { read: () => saved, write: value => { saved = value }, exclusive }, 3,
    control ? { read: () => ({ operationId, action: { action: 'amplifier.operate', expectedOperate: false, operate: true } }), write: () => {}, exclusive } : undefined)
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'available', leaseId: null,
    revision: 1, commandWindowId: null, nextSequence: null, leaseRemainingMs: null, actions: ['log.manual'], txArmed: false }
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value })
  client.open(); reply(state)
  const resolve = () => control ? client.refreshControl() : client.resolve()
  return { client, sent, operationId, unlock, reply, resolve, state,
    advance: async () => { now += 1000; await vi.advanceTimersByTimeAsync(1000) },
    outcome: control ? { operation: 'stationControl', operationId, outcome: 'applied', evidence: 'amplifierReadback' }
      : { operationId, outcome: 'applied', evidence: 'fileSynced', uploads: 'stationPipeline' } }
}

it.each([false, true])('keeps a background poll from overtaking the explicit result check while its browser lock opens (control=%s)', async control => {
  const h = fixture(control), checked = h.resolve()
  void checked.catch(() => {})
  try {
    await h.advance()
    expect(h.sent.map(w => w.request.type)).toEqual(['state'])
    expect(h.client.getSnapshot().busy).toBe(true)
    await expect(h.resolve()).rejects.toThrow('remoteBusy')
    h.unlock(); await Promise.resolve(); await Promise.resolve()
    expect(h.sent.map(w => w.request.type)).toEqual(['state', 'result'])
    expect(h.sent[1]!.request.operationId).toBe(h.operationId)
    h.reply(h.outcome)
    expect(await checked).toMatchObject({ outcome: 'applied', operationId: h.operationId })
    expect(h.client.getSnapshot().busy).toBe(false)
  } finally { h.client.disconnected(); h.unlock(); await checked.catch(() => {}) }
})

it.each([false, true])('does not send a delayed result gesture through a reopened connection (control=%s)', async control => {
  const h = fixture(control), checked = h.resolve()
  void checked.catch(() => {})
  try {
    h.client.disconnected(); await h.advance(); h.client.open(); h.reply(h.state)
    h.unlock(); await Promise.resolve(); await Promise.resolve()
    expect(h.sent.map(w => w.request.type)).toEqual(['state', 'state'])
    await expect(checked).rejects.toThrow('stationUnavailable')
    expect(h.client.getSnapshot().busy).toBe(false)
  } finally { h.client.disconnected(); h.unlock(); await checked.catch(() => {}) }
})
