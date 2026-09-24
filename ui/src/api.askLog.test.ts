// @vitest-environment jsdom
//
// `askLog` is the transport C17b's asking source takes (SPEC-2 v3 C17a): the question goes to the
// engine as it is, under the one key the Rust command destructures (`q`), and the answer comes
// back as it is. Pinned the way the other api wrappers are: stub the bridge, record the call.
import { afterEach, beforeEach, expect, it } from 'vitest'
import { askLog } from './api'
import type { LogTransport } from './features/askingLogSource'
import { DEFAULT_LOG_QUERY } from './features/logQuery'

type Call = { cmd: string; args: unknown }
let calls: Call[] = []

beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return 24
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

it('sends the question as it is to ask_log, and is the asking source’s transport', async () => {
  // A type-level pin: the asking source accepts it as its transport.
  const transport: LogTransport = askLog
  const page = { kind: 'page', query: DEFAULT_LOG_QUERY, offset: 0, limit: 50 } as const
  await transport({ kind: 'logSize' })
  await askLog(page)
  expect(calls).toEqual([
    { cmd: 'ask_log', args: { q: { kind: 'logSize' } } },
    { cmd: 'ask_log', args: { q: page } },
  ])
})
