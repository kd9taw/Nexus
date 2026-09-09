// Version 2 adds demand-driven samples, never a general command dispatcher.
// Every hop answers a one-use credit. Its round-trip time is included in age,
// so buffered network data cannot acquire a fresh measurement timestamp.
import { APPLICATION_COMMANDS, APPLICATION_ERRORS, APPLICATION_MAX_BYTES, readApplicationReply } from './application-protocol'
import type { ApplicationReply, ApplicationErrorCode } from './application-protocol'

export const STREAM_VERSION = 2
export const STREAM_TOPICS = [...APPLICATION_COMMANDS, 'get_scope_snapshot', 'get_cw_state'] as const
export type StreamTopic = typeof STREAM_TOPICS[number]
export type StreamSample = ApplicationReply<StreamTopic>
export type StreamError = { type: 'applicationError'; requestId: string; command: StreamTopic; error: ApplicationErrorCode }
export type StreamUpdate = StreamSample | StreamError
export const STREAM_INTEREST_MS = 2500
export const STREAM_INTERVAL: Record<StreamTopic, number> = {
  get_snapshot: 500, get_settings: 1000, get_band_plan: 1000, get_spectrum_row: 100,
  get_meters: 200, get_scope_snapshot: 100, get_cw_state: 200,
}
export const streamTopic = (v: unknown): v is StreamTopic => STREAM_TOPICS.includes(v as StreamTopic)
export const streamId = (v: unknown): v is string => typeof v === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v)
export function streamExact(v: Record<string, unknown>, keys: string[]): void {
  if (Object.keys(v).length !== keys.length || keys.some(k => !Object.prototype.hasOwnProperty.call(v, k))) throw new Error('invalidApplicationMessage')
}
export function streamTopics(v: unknown): StreamTopic[] {
  if (!Array.isArray(v) || v.length > STREAM_TOPICS.length || !v.every(streamTopic) || new Set(v).size !== v.length) throw new Error('invalidApplicationTopics')
  return v
}
export function streamUpdates(v: unknown, requestId: string): StreamUpdate[] {
  if (!Array.isArray(v) || !v.length || v.length > STREAM_TOPICS.length ||
    new TextEncoder().encode(JSON.stringify(v)).length > APPLICATION_MAX_BYTES - 256) throw new Error('invalidApplicationUpdates')
  const seen = new Set<StreamTopic>()
  return v.map(item => {
    if (!item || typeof item !== 'object' || item.requestId !== requestId || !streamTopic(item.command) || seen.has(item.command)) throw new Error('invalidApplicationSample')
    seen.add(item.command)
    if (item.type === 'applicationError') {
      streamExact(item, ['type', 'requestId', 'command', 'error'])
      if (!APPLICATION_ERRORS.includes(item.error)) throw new Error('invalidApplicationError')
      return item as StreamError
    }
    return readApplicationReply(item, streamTopic)
  })
}
