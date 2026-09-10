// Manual logging has a separate grammar from application observation. An
// authenticated relay can route a request; it cannot issue a native grant.
import { object, finite, integer, text } from './display-validation'
export const OPERATION_REQUEST_BYTES = 6144
export const OPERATION_RESPONSE_BYTES = 4096
export type ManualRecord = {
  call: string
  grid: string | null
  country: string | null
  state: string | null
  band: string
  freqMhz: number
  mode: string
  rstSent: string | null
  rstRcvd: string | null
  name: string | null
  qth: string | null
  comment: string | null
  notes: string | null
  whenUnix: number | null
  confirmed: false
  awardConfirmed: false
  ota?: { theirProgram: 'POTA' | 'SOTA'; theirRef: string }
}
export type OperationRequest =
  | { type: 'state'; requestId: string }
  | { type: 'acquire'; requestId: string; stationBootId: string }
  | { type: 'heartbeat' | 'release'; requestId: string; leaseId: string }
  | { type: 'result'; requestId: string; operationId: string }
  | {
      type: 'logManual'
      requestId: string
      stationBootId: string
      leaseId: string
      expectedRevision: number
      commandWindowId: string
      clientSequence: number
      record: ManualRecord
    }
export type OperationState = {
  stationBootId: string
  allowed: boolean
  phase: 'controlling' | 'occupied' | 'available' | 'localPermissionRequired'
  leaseId: string | null
  revision: number
  commandWindowId: string | null
  nextSequence: number | null
  leaseRemainingMs: number | null
  actions: 'log.manual'[]
  txArmed: false
}
export type OperationOutcome =
  | { outcome: 'applied'; evidence: 'fileSynced'; uploads: 'stationPipeline'; operationId: string }
  | {
      outcome: 'unknown' | 'rejected'
      reason: 'persistenceUnconfirmed' | 'alreadyPresent'
      operationId: string
    }
export type OperationResponse =
  | { type: 'operationResponse'; requestId: string; value: OperationState | OperationOutcome }
  | { type: 'operationResponse'; requestId: string; error: string }
export const operationId = (v: unknown): v is string =>
  typeof v === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v)
const invalid = (): never => {
  throw new Error('invalidOperation')
}
export function manualRecord(raw: unknown): ManualRecord {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const r = raw as Record<string, unknown>
  object(r, [
    'call',
    'grid',
    'country',
    'state',
    'band',
    'freqMhz',
    'mode',
    'rstSent',
    'rstRcvd',
    'name',
    'qth',
    'comment',
    'notes',
    'whenUnix',
    'confirmed',
    'awardConfirmed',
    ...('ota' in r ? ['ota'] : [])
  ])
  if (
    typeof r.call !== 'string' ||
    !/^[A-Z0-9/]{3,32}$/.test(r.call) ||
    !text(r.band, 16) ||
    !r.band ||
    !text(r.mode, 32) ||
    !r.mode ||
    !finite(r.freqMhz) ||
    r.freqMhz <= 0 ||
    r.freqMhz > 250000 ||
    (r.whenUnix !== null &&
      (!integer(r.whenUnix) || r.whenUnix <= 0 || r.whenUnix > 253402300799)) ||
    r.confirmed !== false ||
    r.awardConfirmed !== false
  )
    invalid()
  for (const [key, max] of Object.entries({
    grid: 16,
    country: 96,
    state: 16,
    rstSent: 16,
    rstRcvd: 16,
    name: 128,
    qth: 256,
    comment: 512,
    notes: 1024
  }))
    if (r[key] !== null && !text(r[key], max)) invalid()
  if (r.ota !== undefined) {
    const o = object(r.ota, ['theirProgram', 'theirRef'])
    if (
      !['POTA', 'SOTA'].includes(String(o.theirProgram)) ||
      typeof o.theirRef !== 'string' ||
      !/^[A-Za-z0-9/-]{1,32}$/.test(o.theirRef)
    )
      invalid()
  }
  return raw as ManualRecord
}
export function operationRequest(raw: unknown): OperationRequest {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const r = raw as Record<string, unknown>
  const extra = {
    state: [],
    acquire: ['stationBootId'],
    heartbeat: ['leaseId'],
    release: ['leaseId'],
    result: ['operationId'],
    logManual: [
      'stationBootId',
      'leaseId',
      'expectedRevision',
      'commandWindowId',
      'clientSequence',
      'record'
    ]
  }[String(r.type)]
  if (!extra) throw new Error('invalidOperation')
  object(r, ['type', 'requestId', ...extra])
  if (!operationId(r.requestId)) invalid()
  for (const key of ['stationBootId', 'leaseId', 'operationId', 'commandWindowId'])
    if (key in r && !operationId(r[key])) invalid()
  if (r.type === 'logManual') {
    if (
      !integer(r.expectedRevision) ||
      r.expectedRevision < 0 ||
      !integer(r.clientSequence) ||
      r.clientSequence < 1
    )
      invalid()
    manualRecord(r.record)
  }
  if (new TextEncoder().encode(JSON.stringify(r)).length > 4096) invalid()
  return raw as OperationRequest
}
export function operationValue(raw: unknown): OperationState | OperationOutcome {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const v = raw as Record<string, unknown>
  if ('outcome' in v) {
    object(
      v,
      v.outcome === 'applied'
        ? ['outcome', 'evidence', 'uploads', 'operationId']
        : ['outcome', 'reason', 'operationId']
    )
    if (!operationId(v.operationId)) invalid()
    if (v.outcome === 'applied') {
      if (v.evidence !== 'fileSynced' || v.uploads !== 'stationPipeline') invalid()
    } else if (
      !(v.outcome === 'unknown' && v.reason === 'persistenceUnconfirmed') &&
      !(v.outcome === 'rejected' && v.reason === 'alreadyPresent')
    )
      invalid()
    return raw as OperationOutcome
  }
  object(v, [
    'stationBootId',
    'allowed',
    'phase',
    'leaseId',
    'revision',
    'commandWindowId',
    'nextSequence',
    'leaseRemainingMs',
    'actions',
    'txArmed'
  ])
  if (
    !operationId(v.stationBootId) ||
    typeof v.allowed !== 'boolean' ||
    !['controlling', 'occupied', 'available', 'localPermissionRequired'].includes(
      String(v.phase)
    ) ||
    !integer(v.revision) ||
    v.revision < 0 ||
    v.txArmed !== false ||
    !Array.isArray(v.actions) ||
    v.actions.length > 1 ||
    v.actions.some((a) => a !== 'log.manual')
  )
    invalid()
  const owned = v.phase === 'controlling'
  if (
    owned
      ? !v.allowed ||
        !operationId(v.leaseId) ||
        !operationId(v.commandWindowId) ||
        !integer(v.nextSequence) ||
        v.nextSequence < 1 ||
        !integer(v.leaseRemainingMs) ||
        v.leaseRemainingMs < 0 ||
        v.leaseRemainingMs > 5000
      : [v.leaseId, v.commandWindowId, v.nextSequence, v.leaseRemainingMs].some((x) => x !== null)
  )
    invalid()
  if (!v.allowed && (v.actions as unknown[]).length) invalid()
  return raw as OperationState
}
export const OPERATION_ERRORS = [
  'invalidRequest',
  'remoteBusy',
  'stationBusy',
  'authorityUnavailable',
  'localPermissionRequired',
  'staleStation',
  'controllerBusy',
  'leaseExpired',
  'notController',
  'resultExpired',
  'requestConflict',
  'sequenceConflict',
  'staleContext',
  'windowExpired',
  'invalidRecord',
  'fieldDayUnsupported',
  'recordingUnsupported',
  'staleConnection',
  'stationUnsupported',
  'stationUnavailable',
  'operationUnknown'
] as const
export function operationResponse(raw: unknown): OperationResponse {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const r = raw as Record<string, unknown>
  object(r, ['type', 'requestId', ...('error' in r ? ['error'] : ['value'])])
  if (r.type !== 'operationResponse' || !operationId(r.requestId)) invalid()
  if ('error' in r) {
    if (!OPERATION_ERRORS.includes(r.error as (typeof OPERATION_ERRORS)[number])) invalid()
  } else operationValue(r.value)
  return raw as OperationResponse
}
