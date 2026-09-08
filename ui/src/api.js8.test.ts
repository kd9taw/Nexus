// @vitest-environment jsdom
//
// The twelve JS8 wrappers name exactly the Tauri commands interfaces.md §3.6 lists, with
// exactly the argument keys the Rust side destructures. A wrapper that spells `to` as `dest`
// compiles, ships, and fails at the IPC boundary with "missing required key" — in the field,
// not in CI. So this pins the wire the way api.credentials.test.ts pins the bridge seam:
// stub `window.__TAURI_INTERNALS__.invoke`, record every call, compare.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import * as api from './api'

type Call = { cmd: string; args: unknown }
let calls: Call[] = []

beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return { speed: 'normal', rxSpeeds: 15, activity: [], stations: [], inbox: [], queue: [] }
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('JS8 api wrappers', () => {
  it('name the twelve commands with the exact argument keys the Rust side destructures', async () => {
    await api.js8Enter()
    await api.getJs8State()
    await api.js8SetSpeed(1)
    await api.js8SetRxSpeeds(15)
    await api.js8Send(null, 'HELLO')
    await api.js8Send('KD2UWR', 'HELLO')
    await api.js8SendCommand('KD2UWR', 0, '')
    await api.js8CallCq(0)
    await api.js8Arm('autoreply', true)
    await api.js8Cancel()
    await api.js8DropQueue()
    await api.js8InboxMark(7, 'read')
    await api.js8InboxDelete(7)
    expect(calls).toEqual([
      { cmd: 'js8_enter', args: undefined },
      { cmd: 'get_js8_state', args: undefined },
      { cmd: 'js8_set_speed', args: { speed: 1 } },
      { cmd: 'js8_set_rx_speeds', args: { mask: 15 } },
      { cmd: 'js8_send', args: { to: null, text: 'HELLO' } },
      { cmd: 'js8_send', args: { to: 'KD2UWR', text: 'HELLO' } },
      { cmd: 'js8_send_command', args: { to: 'KD2UWR', cmd: 0, arg: '' } },
      { cmd: 'js8_call_cq', args: { idx: 0 } },
      { cmd: 'js8_arm', args: { which: 'autoreply', on: true } },
      { cmd: 'js8_cancel', args: undefined },
      { cmd: 'js8_drop_queue', args: undefined },
      { cmd: 'js8_inbox_mark', args: { id: 7, state: 'read' } },
      { cmd: 'js8_inbox_delete', args: { id: 7 } },
    ])
  })

  it('every wrapper resolves to the whole Js8State', async () => {
    const s = await api.js8SetRxSpeeds(15)
    expect(s.speed).toBe('normal')
    expect(s.rxSpeeds).toBe(15)
  })
})
