// Versioned, closed application data contract. This is not a Tauri dispatcher.
// Changes are top-level replacements against an exact revision, never JSON paths
// or executable instructions. Missing bases require a fresh full response.
export const APPLICATION_VERSION = 1
export const APPLICATION_MAX_BYTES = 768 * 1024
export const APPLICATION_REQUEST_BYTES = 512
export const APPLICATION_TIMEOUT_MS = 3000
export const APPLICATION_COMMANDS = ['get_snapshot', 'get_settings', 'get_band_plan', 'get_spectrum_row', 'get_meters'] as const
export type ApplicationCommand = typeof APPLICATION_COMMANDS[number]
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }
export type ApplicationRequest = { type: 'applicationRead'; requestId: string; command: ApplicationCommand; revision: number | null }
export type ApplicationReply<C extends string = ApplicationCommand> = { type: 'applicationResult'; requestId: string; command: C;
  revision: number; baseRevision: number | null; ageMs: number; data: Json; removed: string[] }
export type ApplicationValue = { revision: number; value: Json }
export const APPLICATION_ERRORS = ['stationUpdateRequired', 'applicationBusy', 'applicationUnavailable', 'applicationTooLarge', 'applicationUnsupported'] as const
export type ApplicationErrorCode = typeof APPLICATION_ERRORS[number]

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const safeKey = (key: string) => !['__proto__', 'prototype', 'constructor'].includes(key)
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value)
const positive = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) > 0
export const applicationCommand = (value: unknown): value is ApplicationCommand => APPLICATION_COMMANDS.includes(value as ApplicationCommand)
function exact(value: unknown, keys: string[]): asserts value is Record<string, unknown> {
  if (!record(value) || Object.keys(value).length !== keys.length || keys.some(key => !Object.prototype.hasOwnProperty.call(value, key))) throw new Error('invalidApplicationMessage')
}
export function validApplicationJson(value: unknown, depth = 0): value is Json {
  if (depth > 32) return false
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return true
  if (typeof value === 'number') return Number.isFinite(value)
  if (Array.isArray(value)) return value.every(item => validApplicationJson(item, depth + 1))
  return record(value) && Object.entries(value).every(([key, item]) => safeKey(key) && validApplicationJson(item, depth + 1))
}
export function applicationRequest(value: unknown): ApplicationRequest {
  exact(value, ['type', 'requestId', 'command', 'revision'])
  if (value.type !== 'applicationRead' || typeof value.requestId !== 'string' || !uuid.test(value.requestId) ||
    !applicationCommand(value.command) || (value.revision !== null && !positive(value.revision))) throw new Error('invalidApplicationRequest')
  return value as ApplicationRequest
}
export function applicationReply(value: unknown): ApplicationReply {
  return readApplicationReply(value, applicationCommand)
}
export function readApplicationReply<C extends string>(value: unknown, command: (value: unknown) => value is C): ApplicationReply<C> {
  exact(value, ['type', 'requestId', 'command', 'revision', 'baseRevision', 'ageMs', 'data', 'removed'])
  if (value.type !== 'applicationResult' || typeof value.requestId !== 'string' || !uuid.test(value.requestId) ||
    !command(value.command) || !positive(value.revision) ||
    (value.baseRevision !== null && (!positive(value.baseRevision) || value.baseRevision > value.revision)) ||
    !Number.isSafeInteger(value.ageMs) || Number(value.ageMs) < 0 || Number(value.ageMs) >= APPLICATION_TIMEOUT_MS ||
    !Array.isArray(value.removed) || value.removed.length > 512 ||
    !value.removed.every(key => typeof key === 'string' && safeKey(key)) || !validApplicationJson(value.data) ||
    (value.baseRevision === null && value.removed.length !== 0) || (value.baseRevision !== null && !record(value.data))) {
    throw new Error('invalidApplicationReply')
  }
  return value as ApplicationReply<C>
}
export function applyApplicationReply(previous: ApplicationValue | null, reply: ApplicationReply<string>): ApplicationValue {
  if (reply.baseRevision === null) return { revision: reply.revision, value: reply.data }
  if (!previous || previous.revision !== reply.baseRevision || !record(previous.value) || !record(reply.data)) throw new Error('applicationBaseMissing')
  const next = { ...previous.value, ...reply.data }
  for (const key of reply.removed) delete next[key]
  return { revision: reply.revision, value: next as Json }
}
