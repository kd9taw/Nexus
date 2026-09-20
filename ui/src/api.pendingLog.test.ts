// @vitest-environment jsdom
//
// Answering the prompt-to-log popup must name the hold it is answering.
//
// Operator ruling, 2026-09-19: when a second contact completes while the popup is still open,
// QUEUE it — the open popup keeps the first contact, and the next one appears after you log or
// discard it. The engine holds that queue and refuses a confirm or discard whose `expectedKey`
// is not the head's, so an answer can never be applied to a contact the operator was not
// looking at. That key was sent only under a Remote transport; a desktop answer carried nothing
// to check, which with the queue in place is refused outright (fails closed — nothing is logged
// wrongly, but the popup cannot be cleared).
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import * as api from './api'
import type { LoggedQso } from './types'

const calls: { cmd: string; args: Record<string, unknown> | undefined }[] = []

beforeEach(() => {
  calls.length = 0
  // The real seam (see api.credentials.test.ts): stub the bridge, not a test-only hook.
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: Record<string, unknown> | undefined) => {
      calls.push({ cmd, args })
      return {}
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const record = { call: 'VE3ABC', band: '20m', mode: 'FT8' } as unknown as LoggedQso

describe('confirming or discarding a held contact names it, on every transport', () => {
  it('sends the key the popup was shown with when confirming', async () => {
    await api.confirmPendingLog(record, 'hold-1')
    expect(calls).toHaveLength(1)
    expect(calls[0].cmd).toBe('confirm_pending_log')
    expect(calls[0].args, 'a desktop confirm carried no key for the engine to check').toEqual({
      record,
      expectedKey: 'hold-1',
    })
  })

  it('sends it when discarding too', async () => {
    await api.discardPendingLog('hold-1')
    expect(calls).toHaveLength(1)
    expect(calls[0].cmd).toBe('discard_pending_log')
    expect(calls[0].args, 'a desktop discard carried no key for the engine to check').toEqual({
      expectedKey: 'hold-1',
    })
  })
})
