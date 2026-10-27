// @vitest-environment jsdom
// Every press on the shack's Remote settings reaches the station stamped with where it was made: at the
// shack, or through a stream (`isStreamInput`, read before anything awaits). The station takes nothing but
// a read through a stream, so a device driving the shack's screen never approves, allows or removes itself.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import * as api from './api'

const stream = vi.hoisted(() => ({ on: false }))
vi.mock('./remote-native/stream-input', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  isStreamInput: () => stream.on,
}))

let sent: unknown[] = []
beforeEach(() => {
  sent = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => { sent.push([cmd, args]); return {} },
  }
})
afterEach(() => {
  stream.on = false
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

it('a press through a stream is stamped as the stream’s', async () => {
  stream.on = true
  const pending = api.remoteStationAction({ type: 'allowTransmit', deviceId: 'd' })
  stream.on = false
  await pending
  expect(sent).toEqual([['remote_station_action', { action: { type: 'allowTransmit', deviceId: 'd' }, origin: 'stream' }]])
})

it('CONTROL: a press at the shack is stamped as the shack’s', async () => {
  await api.remoteStationAction({ type: 'allowTransmit', deviceId: 'd' })
  expect(sent).toEqual([['remote_station_action', { action: { type: 'allowTransmit', deviceId: 'd' }, origin: 'shack' }]])
})
