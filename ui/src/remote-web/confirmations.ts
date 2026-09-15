// Confirmation diagnostics as the station's Awards view computes them, cut to the rows the
// panel lists. Validated completely before the existing Awards panel renders it. Bucket QSO
// indices only feed the desktop's upload buttons and are never sent to a browser.
import type { DiagAction, DiagnosticsReport, OneAway, QsoDiagnosis } from '../types'
import type { QueryPage } from './application-query-protocol'

export type Confirmations = { report: DiagnosticsReport; logCount: number; capturedAgeMs: number }
const fail = (): never => { throw new Error('invalidConfirmations') }
const count = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) >= 0
const text = (v: unknown): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= 1024 && !/[\uD800-\uDFFF]/u.test(v)
function record(v: unknown, keys: string[], optional: string[] = []): Record<string, unknown> {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return fail()
  const own = Object.keys(v)
  if (!keys.every(k => own.includes(k)) || !own.every(k => keys.includes(k) || optional.includes(k))) fail()
  return v as Record<string, unknown>
}
const list = (v: unknown, max: number): unknown[] => Array.isArray(v) && v.length <= max ? v : fail()
const ACTION_TEXT = ['source', 'detail', 'field', 'found', 'expected', 'logged', 'suggested', 'call']
const ACTION_NUMBER = ['otherIndex', 'untilUnix']
function action(v: unknown): DiagAction {
  const a = record(v, ['kind'], [...ACTION_TEXT, ...ACTION_NUMBER])
  if (!text(a.kind) || !a.kind || !ACTION_TEXT.every(k => a[k] === undefined || text(a[k])) ||
    !ACTION_NUMBER.every(k => a[k] === undefined || Number.isSafeInteger(a[k]))) fail()
  return a as unknown as DiagAction
}
function diagnosis(v: unknown): QsoDiagnosis {
  const d = record(v, ['index', 'award', 'status', 'reasons'])
  if (!count(d.index) || !text(d.award) || !text(d.status)) fail()
  const reasons = list(d.reasons, 8).map(raw => {
    const r = record(raw, ['code', 'confidence', 'explanation', 'action'])
    if (![r.code, r.confidence, r.explanation].every(text)) fail()
    return { code: r.code as string, confidence: r.confidence as string, explanation: r.explanation as string, action: action(r.action) }
  })
  return { index: d.index as number, award: d.award as string, status: d.status as string, reasons }
}
export function parseConfirmations(page: QueryPage): Confirmations {
  const meta = page.meta as Record<string, unknown> | null
  if (page.collection !== 'confirmations' || page.offset !== 0 || page.total !== 0 || page.retained !== 0 || page.rows.length !== 0 ||
    page.nextCursor !== null || !meta || !count(meta.capturedAgeMs) || Number(meta.capturedAgeMs) >= 60_000) fail()
  const s = record(meta!.source, ['diagnoses', 'buckets', 'oneAway', 'waitingOnPartner', 'pendingLag', 'logCount'])
  if (!count(s.waitingOnPartner) || !count(s.pendingLag) || !count(s.logCount)) fail()
  const buckets = list(s.buckets, 64).map(raw => {
    const b = record(raw, ['kind', 'count', 'qsoIndices'])
    if (!text(b.kind) || !count(b.count)) fail()
    list(b.qsoIndices, 0)
    return { kind: b.kind as string, count: b.count as number, qsoIndices: [] }
  })
  const oneAway = list(s.oneAway, 512).map((raw): OneAway => {
    const o = record(raw, ['entity', 'bands', 'newEntity'])
    if (!text(o.entity) || typeof o.newEntity !== 'boolean' || !list(o.bands, 64).every(text)) fail()
    return { entity: o.entity as string, bands: o.bands as string[], newEntity: o.newEntity as boolean }
  })
  return {
    report: { diagnoses: list(s.diagnoses, 50).map(diagnosis), buckets, oneAway, waitingOnPartner: s.waitingOnPartner as number, pendingLag: s.pendingLag as number },
    logCount: s.logCount as number, capturedAgeMs: Number(meta!.capturedAgeMs) + page.ageMs,
  }
}
