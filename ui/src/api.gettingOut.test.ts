// @vitest-environment jsdom
//
// Getting Out's Start fresh time reaches the station as the `since` the Rust command destructures.
// Every box test stands in for `getGettingOut`, so this pins the wire itself: stub
// `window.__TAURI_INTERNALS__.invoke`, record the call, compare.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import * as api from './api'

type Call = { cmd: string; args: unknown }
let calls: Call[] = []

beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return { count: 0, maxKm: 0, reports: [] }
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('getGettingOut', () => {
  it('sends the start time as `since`, and nothing without one', async () => {
    await api.getGettingOut(1_793_369_130)
    await api.getGettingOut()
    await api.getGettingOut(null)
    expect(calls.map((c) => c.cmd)).toEqual(['get_getting_out', 'get_getting_out', 'get_getting_out'])
    expect(calls[0].args).toEqual({ since: 1_793_369_130 })
    expect(calls[1].args ?? {}).toEqual({})
    expect(calls[2].args ?? {}).toEqual({})
  })
})
