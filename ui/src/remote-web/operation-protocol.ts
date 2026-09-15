// Manual logging has a separate grammar from application observation. An
// authenticated relay can route a request; it cannot issue a native grant.
import { object, finite, integer, text } from './display-validation'
import { CLUSTER_SPOT_RESULTS, POTA_SPOT_RESULTS, type SelfSpotReport } from '../selfSpot'
import { controlContext, controlOutcome, stationAction, CONTROL_CAPABILITIES, type ControlCapability, type ControlContext, type ControlOutcome, type StationAction } from './station-operation'
import { SETTINGS_SHAPES, WRITABLE_CONTROL_SETTINGS_KEYS, WRITABLE_LOGGING_SETTINGS_KEYS } from './configuration-schema'
export const OPERATION_REQUEST_BYTES = 6144
export const OPERATION_RESPONSE_BYTES = 4096
/** A chunk of the one activation file this browser asked for is the only operation reply allowed
 * past OPERATION_RESPONSE_BYTES: 32 KiB of file as base64 plus its envelope. */
export const OPERATION_EXPORT_RESPONSE_BYTES = 48 * 1024
export const ACTIVATION_EXPORT_MAX_BYTES = 1024 * 1024
export const ACTIVATION_EXPORT_CHUNK_BYTES = 32 * 1024
export const ACTIVATION_EXPORT_LISTED = 128
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
/** A log row as the station's log page sent it. The station finds the row again by this key and
 * never by a position: a position is stale the moment anything else changes the log. */
export type LogTarget = { call: string; whenUnix: number; key: string }
export type LogChange =
  | { kind: 'edit'; target: LogTarget; record: ManualRecord }
  | { kind: 'delete'; target: LogTarget }
  /** `via` is the ADIF QSL_SENT_VIA letter; only `null` withdraws the mark. */
  | { kind: 'qslSent'; target: LogTarget; via: 'B' | 'D' | 'E' | null }
  | { kind: 'qslCard'; target: LogTarget; received: boolean }
  /** Station context, not a row: the station tags its next contact with `call` with this reference. */
  | { kind: 'hunt'; call: string; program: 'POTA' | 'SOTA'; reference: string }
  | { kind: 'clearHunt' }
  /** Station context too: while it is on, the station stamps your reference on every contact it logs. */
  | { kind: 'activation'; program: 'POTA' | 'SOTA'; reference: string }
  | { kind: 'clearActivation' }
  /** A public DX cluster spot of the station's own call. Carries the reference and dial the confirm
   * showed, so the station refuses it if either has moved since. */
  | { kind: 'selfSpot'; reference: string; dialHz: number }
  /** Operating preferences from the station's allow-list, against the Settings document revision the
   * page showed. Station control writes station preferences and the logging grant writes logging
   * ones; a change carrying both needs both. */
  | { kind: 'settings'; revision: string; values: Record<string, unknown> }
/** Station hints for log changes. They ride in `controls.capabilities`, which every hosted page
 * since operation v3 filters, so a newer station can offer them without breaking an older page. */
export const LOG_CAPABILITIES = ['logEdit', 'qslMarks', 'otaHunt', 'otaActivation', 'selfSpot', 'activationExport',
  'settingsControl', 'settingsLogging'] as const
export type LogCapability = (typeof LOG_CAPABILITIES)[number]
const loggingPreference = (key: string) => (WRITABLE_LOGGING_SETTINGS_KEYS as readonly string[]).includes(key)
export const logChangeCapability = (change: LogChange): LogCapability =>
  change.kind === 'settings'
    ? Object.keys(change.values).every(loggingPreference) ? 'settingsLogging' : 'settingsControl'
    : ({ edit: 'logEdit', delete: 'logEdit', qslSent: 'qslMarks', qslCard: 'qslMarks', hunt: 'otaHunt', clearHunt: 'otaHunt',
      activation: 'otaActivation', clearActivation: 'otaActivation', selfSpot: 'selfSpot' } as const)[change.kind]
/** Every hint a change needs. Only a settings change carrying both kinds of preference needs two. */
export const logChangeCapabilities = (change: LogChange): LogCapability[] =>
  change.kind !== 'settings' ? [logChangeCapability(change)]
    : [...(Object.keys(change.values).some(loggingPreference) ? ['settingsLogging' as const] : []),
      ...(Object.keys(change.values).some(k => !loggingPreference(k)) ? ['settingsControl' as const] : [])]
const CHANGE_EVIDENCE = ['fileSynced', 'stationState', 'spotPosted', 'settingsSaved'] as const
/** `clusterUnavailable` is gone: a self-spot now reports each target in `spot`, so the one refusal
 * that named the cluster alone has no producer left. */
const CHANGE_REFUSALS = ['contextChanged', 'invalidChange', 'spotNotPosted'] as const
/** A self-spot's outcome, and only a self-spot's, carries `spot`: what pota.app and the cluster each
 * did. `spotPosted` means at least one took it, `spotNotPosted` neither. */
export type LogChangeOutcome = { operation: 'logChange'; operationId: string } & (
  | { outcome: 'applied'; evidence: 'fileSynced' | 'stationState' | 'settingsSaved' }
  | { outcome: 'applied'; evidence: 'spotPosted'; spot: SelfSpotReport }
  | { outcome: 'rejected'; reason: 'contextChanged' | 'invalidChange' }
  | { outcome: 'rejected'; reason: 'spotNotPosted'; spot: SelfSpotReport }
  | { outcome: 'unknown'; reason: 'persistenceUnconfirmed' })
export type OperationRequest =
  | { type: 'state'; requestId: string }
  | { type: 'stopTransmit'; requestId: string; stationBootId: string; leaseId: string; transmitEpoch: string }
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
  | {
      type: 'stationControl'
      requestId: string
      stationBootId: string
      leaseId: string
      expectedRevision: number
      commandWindowId: string
      clientSequence: number
      context: ControlContext
      action: StationAction
    }
  | {
      type: 'logChange'
      requestId: string
      stationBootId: string
      leaseId: string
      expectedRevision: number
      commandWindowId: string
      clientSequence: number
      change: LogChange
    }
  /** A read under this browser's logging lease: the list of activations (no selection), or one chunk
   * of one activation file. It spends no command sequence and changes nothing at the station. */
  | {
      type: 'activationExport'
      requestId: string
      stationBootId: string
      leaseId: string
      selection: ActivationSelection | null
      index: number
    }
/** One activation, named the way the desktop's per-activation export names it: your park or summit,
 * the UTC day and the callsign you signed. A range, a search or the whole log cannot be expressed. */
export type ActivationSelection = { reference: string; dayStartUnix: number; callsign: string | null }
export type ActivationEntry = ActivationSelection & { program: string | null; date: string; qsos: number }
export type ActivationExportValue = { operation: 'activationExport' } & (
  | { activations: ActivationEntry[] }
  | { file: { byteLength: number; sha256: string; chunks: number }; index: number; base64: string }
  | { refused: 'notFound' | 'tooLarge' })
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
  txArmed: boolean
  transmitEpoch?: string | null
  controls?: { context: ControlContext; capabilities: (ControlCapability | LogCapability)[] }
}
export type OperationOutcome =
  | { outcome: 'applied'; evidence: 'fileSynced'; uploads: 'stationPipeline'; operationId: string }
  | {
      outcome: 'unknown' | 'rejected'
      reason: 'persistenceUnconfirmed' | 'alreadyPresent'
      operationId: string
    }
export type StopOutcome = { stop: 'accepted' }
export type OperationValue = OperationState | OperationOutcome | ControlOutcome | StopOutcome | LogChangeOutcome | ActivationExportValue
export const transmitEpoch = (v: unknown): v is string => typeof v === 'string' && /^[0-9a-f]{16}$/.test(v)
export type OperationResponse =
  | { type: 'operationResponse'; requestId: string; value: OperationValue }
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
/** The exact bytes a row key is the SHA-256 of. Mirrored by `row_canonical` in src-tauri's
 * remote_service logging module; a shared vector in both test suites holds them together.
 * Numbers are compared in millionths so a float and the same integer agree on both sides. */
export function logRowCanonical(v: unknown): string {
  if (v === null) return 'z'
  if (typeof v === 'boolean') return v ? 't' : 'f'
  if (typeof v === 'number') {
    if (!Number.isFinite(v)) invalid()
    const micro = Math.round(Math.abs(v) * 1e6)
    return `n${v < 0 && micro !== 0 ? '-' : ''}${BigInt(micro)};`
  }
  if (typeof v === 'string') return `s${new TextEncoder().encode(v).length}:${v}`
  if (Array.isArray(v)) return `a${v.length}[${v.map(logRowCanonical).join('')}]`
  if (typeof v === 'object') {
    const keys = Object.keys(v).sort()
    return `o${keys.length}{${keys.map(k => logRowCanonical(k) + logRowCanonical((v as Record<string, unknown>)[k])).join('')}}`
  }
  return invalid()
}
export async function logTarget(row: { call: string; whenUnix: number }): Promise<LogTarget> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(logRowCanonical(row)))
  return { call: row.call, whenUnix: row.whenUnix, key: [...new Uint8Array(digest)].map(b => b.toString(16).padStart(2, '0')).join('') }
}
export function logChange(raw: unknown): LogChange {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const c = raw as Record<string, unknown>
  const shapes: Record<string, string[]> = { edit: ['kind', 'target', 'record'], delete: ['kind', 'target'],
    qslSent: ['kind', 'target', 'via'], qslCard: ['kind', 'target', 'received'],
    hunt: ['kind', 'call', 'program', 'reference'], clearHunt: ['kind'],
    activation: ['kind', 'program', 'reference'], clearActivation: ['kind'],
    selfSpot: ['kind', 'reference', 'dialHz'], settings: ['kind', 'revision', 'values'] }
  if (typeof c.kind !== 'string' || !Object.prototype.hasOwnProperty.call(shapes, c.kind)) invalid()
  object(c, shapes[c.kind as string])
  // A row change names the exact row (its shape requires the target); a hunt names none.
  if ('target' in c) {
    const t = object(c.target, ['call', 'whenUnix', 'key'])
    if (!text(t.call, 32) || !t.call || !integer(t.whenUnix) || t.whenUnix > 253402300799 ||
      typeof t.key !== 'string' || !/^[0-9a-f]{64}$/.test(t.key))
      invalid()
  }
  if (c.kind === 'hunt' && (typeof c.call !== 'string' || !/^[A-Z0-9/]{3,32}$/.test(c.call)))
    invalid()
  // The wire grammar only; the station normalizes the reference for its program and may refuse it.
  if ((c.kind === 'hunt' || c.kind === 'activation') && ((c.program !== 'POTA' && c.program !== 'SOTA') ||
    typeof c.reference !== 'string' || !/^[A-Za-z0-9/-]{1,32}$/.test(c.reference)))
    invalid()
  if (c.kind === 'selfSpot' && (typeof c.reference !== 'string' || !/^[A-Za-z0-9/-]{1,32}$/.test(c.reference) ||
    !integer(c.dialHz) || c.dialHz < 1 || c.dialHz > 250_000_000_000))
    invalid()
  // An edit states when the contact happened; "station time" only means something for a new entry.
  if (c.kind === 'edit' && manualRecord(c.record).whenUnix === null) invalid()
  // The empty string is the QSL menu's placeholder, a non-choice: never read it as a withdrawal.
  if (c.kind === 'qslSent' && !(c.via === null || c.via === 'B' || c.via === 'D' || c.via === 'E')) invalid()
  if (c.kind === 'qslCard' && typeof c.received !== 'boolean') invalid()
  // Only settings on the allow-list, each in the type the Settings document declares. The station
  // decides again: it denies any other key and refuses a value the setting cannot hold.
  if (c.kind === 'settings') {
    if (typeof c.revision !== 'string' || !/^[0-9a-f]{64}$/.test(c.revision) || !c.values || typeof c.values !== 'object' ||
      Array.isArray(c.values) || Object.getPrototypeOf(c.values) !== Object.prototype)
      invalid()
    const values = c.values as Record<string, unknown>, keys = Object.keys(values)
    if (keys.length < 1 || keys.length > 32) invalid()
    for (const key of keys) {
      if (![...WRITABLE_CONTROL_SETTINGS_KEYS, ...WRITABLE_LOGGING_SETTINGS_KEYS].includes(key as never)) invalid()
      const shape: string = SETTINGS_SHAPES[key as keyof typeof SETTINGS_SHAPES], value = values[key]
      if (shape === 'number' ? !finite(value) : shape === 'string' ? !text(value, 256) : shape === 'boolean' ? typeof value !== 'boolean'
        : shape !== 'object' || !value || typeof value !== 'object' || Array.isArray(value))
        invalid()
    }
  }
  return raw as LogChange
}
const EXPORT_REFERENCE = /^[A-Z0-9/-]{1,32}$/
const EXPORT_CALLSIGN = /^[A-Z0-9/]{3,32}$/
/** The wire grammar for one activation. The station still refuses a selection its log does not list. */
export function activationSelection(raw: unknown): ActivationSelection {
  const s = object(raw, ['reference', 'dayStartUnix', 'callsign'])
  if (typeof s.reference !== 'string' || !EXPORT_REFERENCE.test(s.reference) || !integer(s.dayStartUnix) ||
    s.dayStartUnix > 253402214400 || s.dayStartUnix % 86400 !== 0 ||
    !(s.callsign === null || (typeof s.callsign === 'string' && EXPORT_CALLSIGN.test(s.callsign))))
    invalid()
  return raw as ActivationSelection
}
function activationExportValue(v: Record<string, unknown>): ActivationExportValue {
  if ('activations' in v) {
    object(v, ['operation', 'activations'])
    if (!Array.isArray(v.activations) || v.activations.length > ACTIVATION_EXPORT_LISTED) invalid()
    for (const raw of v.activations as unknown[]) {
      const a = object(raw, ['program', 'reference', 'dayStartUnix', 'date', 'callsign', 'qsos'])
      activationSelection({ reference: a.reference, dayStartUnix: a.dayStartUnix, callsign: a.callsign })
      if (!(a.program === null || text(a.program, 16)) || typeof a.date !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(a.date) ||
        !integer(a.qsos))
        invalid()
    }
  } else if ('refused' in v) {
    object(v, ['operation', 'refused'])
    if (v.refused !== 'notFound' && v.refused !== 'tooLarge') invalid()
  } else {
    object(v, ['operation', 'file', 'index', 'base64'])
    const f = object(v.file, ['byteLength', 'sha256', 'chunks'])
    if (!integer(f.byteLength) || f.byteLength < 1 || f.byteLength > ACTIVATION_EXPORT_MAX_BYTES ||
      typeof f.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(f.sha256) ||
      f.chunks !== Math.ceil(f.byteLength / ACTIVATION_EXPORT_CHUNK_BYTES) || !integer(v.index) || v.index >= Number(f.chunks) ||
      typeof v.base64 !== 'string' || v.base64.length % 4 !== 0 ||
      v.base64.length > Math.ceil(ACTIVATION_EXPORT_CHUNK_BYTES / 3) * 4 || !/^[A-Za-z0-9+/]*={0,2}$/.test(v.base64))
      invalid()
  }
  return v as ActivationExportValue
}
function logChangeOutcome(v: Record<string, unknown>): LogChangeOutcome {
  const spot = v.evidence === 'spotPosted' || v.reason === 'spotNotPosted'
  object(v, ['operation', 'operationId', 'outcome', v.outcome === 'applied' ? 'evidence' : 'reason', ...(spot ? ['spot'] : [])])
  if (v.operation !== 'logChange' || !operationId(v.operationId) ||
    !(v.outcome === 'applied' ? CHANGE_EVIDENCE.includes(v.evidence as never)
      : v.outcome === 'rejected' ? CHANGE_REFUSALS.includes(v.reason as never)
        : v.outcome === 'unknown' && v.reason === 'persistenceUnconfirmed'))
    invalid()
  if (spot) {
    const s = object(v.spot, ['pota', 'cluster'])
    if (!POTA_SPOT_RESULTS.includes(s.pota as never) || !CLUSTER_SPOT_RESULTS.includes(s.cluster as never)) invalid()
  }
  return v as LogChangeOutcome
}
export function operationRequest(raw: unknown): OperationRequest {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const r = raw as Record<string, unknown>
  const extra = {
    state: [],
    stopTransmit: ['stationBootId', 'leaseId', 'transmitEpoch'],
    acquire: ['stationBootId'],
    heartbeat: ['leaseId'],
    release: ['leaseId'],
    result: ['operationId'],
    stationControl: ['stationBootId', 'leaseId', 'expectedRevision', 'commandWindowId', 'clientSequence', 'context', 'action'],
    logChange: ['stationBootId', 'leaseId', 'expectedRevision', 'commandWindowId', 'clientSequence', 'change'],
    activationExport: ['stationBootId', 'leaseId', 'selection', 'index'],
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
  if (r.type === 'stopTransmit' && !transmitEpoch(r.transmitEpoch)) invalid()
  // One activation by park, UTC day and callsign, or none for the list, which has a single chunk.
  if (r.type === 'activationExport') {
    if (r.selection !== null) activationSelection(r.selection)
    if (!integer(r.index) || r.index >= ACTIVATION_EXPORT_MAX_BYTES / ACTIVATION_EXPORT_CHUNK_BYTES ||
      (r.selection === null && r.index !== 0))
      invalid()
  }
  if (r.type === 'logManual' || r.type === 'stationControl' || r.type === 'logChange') {
    if (
      !integer(r.expectedRevision) ||
      r.expectedRevision < 0 ||
      !integer(r.clientSequence) ||
      r.clientSequence < 1
    )
      invalid()
    if (r.type === 'logManual') manualRecord(r.record)
    else if (r.type === 'logChange') logChange(r.change)
    else { controlContext(r.context); stationAction(r.action) }
  }
  if (new TextEncoder().encode(JSON.stringify(r)).length > 4096) invalid()
  return raw as OperationRequest
}
export function operationValue(raw: unknown): OperationValue {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const v = raw as Record<string, unknown>
  if ('stop' in v) {
    object(v, ['stop'])
    if (v.stop !== 'accepted') invalid()
    return raw as StopOutcome
  }
  if ('operation' in v)
    return v.operation === 'logChange' ? logChangeOutcome(v) : v.operation === 'activationExport' ? activationExportValue(v) : controlOutcome(v)
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
    'txArmed',
    ...('transmitEpoch' in v ? ['transmitEpoch'] : []),
    ...('controls' in v ? ['controls'] : [])
  ])
  if ('controls' in v) {
    const controls = object(v.controls, ['context', 'capabilities'])
    controlContext(controls.context)
    if (!Array.isArray(controls.capabilities) || controls.capabilities.length > 64 || new Set(controls.capabilities).size !== controls.capabilities.length || controls.capabilities.some(c => typeof c !== 'string' || !/^[a-z][a-zA-Z0-9]{0,31}$/.test(c))) invalid()
    if (!v.allowed && (controls.capabilities as unknown[]).length) invalid()
  }
  if (
    !operationId(v.stationBootId) ||
    typeof v.allowed !== 'boolean' ||
    !['controlling', 'occupied', 'available', 'localPermissionRequired'].includes(
      String(v.phase)
    ) ||
    !integer(v.revision) ||
    v.revision < 0 ||
    typeof v.txArmed !== 'boolean' ||
    !Array.isArray(v.actions) ||
    v.actions.length > 32 ||
    new Set(v.actions).size !== v.actions.length ||
    v.actions.some((a) => typeof a !== 'string' || !/^[a-z][a-zA-Z0-9]{0,31}(\.[a-z][a-zA-Z0-9]{0,31})?$/.test(a))
  )
    invalid()
  const owned = v.phase === 'controlling'
  if (v.txArmed && (!owned || !transmitEpoch(v.transmitEpoch))) invalid()
  if ('transmitEpoch' in v && v.transmitEpoch !== null && (!owned || !transmitEpoch(v.transmitEpoch))) invalid()
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
  // A bounded action a newer station names is a hint for a newer page, exactly like an unknown
  // capability: drop it. Refusing the whole state for it broke control for every older page the
  // moment a station learned a second action.
  const actions = (v.actions as string[]).filter((a): a is 'log.manual' => a === 'log.manual')
  if ('controls' in v) {
    const controls = v.controls as { context: ControlContext; capabilities: string[] }
    // An unknown bounded capability is only a hint for a newer UI. Ignore it;
    // never widen the closed action grammar or reject existing capabilities.
    return { ...v, actions, controls: { ...controls, capabilities: controls.capabilities.filter(c => CONTROL_CAPABILITIES.includes(c as ControlCapability) || LOG_CAPABILITIES.includes(c as LogCapability)) } } as OperationState
  }
  return { ...v, actions } as OperationState
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
  } else return { type: 'operationResponse', requestId: r.requestId as string, value: operationValue(r.value) }
  return raw as OperationResponse
}
