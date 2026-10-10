// @vitest-environment jsdom
//
// `workSpot` names a Needed row's park ONLY when there is one. The hosted page's control transport
// (remote-web/control-transport.ts) refuses a work_spot carrying any key but mode, freqMhz, band, call
// and tier, so a `park: null` on every call would turn every browser's Work into "unsupported". The
// wire is pinned the way api.js8.test.ts pins its own: stub the bridge, record every call, compare.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import * as api from './api'

type Call = { cmd: string; args: unknown }
let calls: Call[] = []

beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return {}
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('workSpot', () => {
  it('sends the five keys it always sent when there is no park', async () => {
    await api.workSpot('phone', 14.25, '20m', 'W9XYZ')
    await api.workSpot('digital', 14.074, '20m', 'K1ABC', 'FT8', null)
    // Strict: a `park: undefined` key is still a key to the transport's `Object.keys` check.
    expect(calls).toStrictEqual([
      { cmd: 'work_spot', args: { mode: 'phone', freqMhz: 14.25, band: '20m', call: 'W9XYZ', tier: null } },
      { cmd: 'work_spot', args: { mode: 'digital', freqMhz: 14.074, band: '20m', call: 'K1ABC', tier: 'FT8' } },
    ])
  })

  it('adds the park when a Needed row names one', async () => {
    await api.workSpot('phone', 14.285, '20m', 'K9ABC', undefined, { program: 'POTA', reference: 'US-1000' })
    expect(calls).toStrictEqual([
      {
        cmd: 'work_spot',
        args: {
          mode: 'phone', freqMhz: 14.285, band: '20m', call: 'K9ABC', tier: null,
          park: { program: 'POTA', reference: 'US-1000' },
        },
      },
    ])
  })
})
