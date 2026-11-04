// @vitest-environment jsdom
//
// The scope frame reads on the wire. The CW and Phone audio view asks `get_scope_frame` with
// `audioOnly: true`: without it the backend hands back the radio's RF row while one streams, and the
// audio half silently becomes a second copy of the radio's scope. The rig scope's own read must not
// carry the flag, or it would never show the radio's scope.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import * as api from './api'

type Call = { cmd: string; args: unknown }
let calls: Call[] = []

beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return null
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('the scope frame reads', () => {
  it("the audio view's read asks for the audio only", async () => {
    await api.getAudioScopeFrame(200, 3000, 'hidef', 7)
    expect(calls).toEqual([
      { cmd: 'get_scope_frame', args: { loHz: 200, hiHz: 3000, window: 'hidef', lastSeq: 7, audioOnly: true } },
    ])
  })

  it("control: the rig scope's read carries no audio-only flag, with or without an RF window", async () => {
    await api.getScopeFrame(200, 3000, 'balanced', 3)
    await api.getScopeFrame(200, 3000, 'balanced', 3, { loHz: 14_000_000, hiHz: 14_100_000 })
    expect(calls.map((c) => c.cmd)).toEqual(['get_scope_frame', 'get_scope_frame'])
    for (const c of calls) expect(c.args).not.toHaveProperty('audioOnly')
  })
})
