import type { FieldDayObservation } from '../fieldDayObservation'
import type { QueryPage } from './application-query-protocol'

export const FIELD_DAY_TTL_MS = 60_000
const integer = (v: unknown) => Number.isSafeInteger(v) && Number(v) >= 0
const text = (v: unknown): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= 1024 && !/[\uD800-\uDFFF]/u.test(v)
const texts = (v: unknown, max: number) => Array.isArray(v) && v.length <= max && v.every(text)
// The rules file is DATA and it grows: it already carries 15 events (ARRL FD and Winter FD, the
// Sweepstakes and VHF runnings, CQ WW and WPX, and the state QSO parties), and each new contest
// adds another. Matching a hardcoded pair here rejected the whole Field Day payload the moment a
// station ran anything but ARRL FD or WFD - thirteen of the fifteen. Validate the SHAPE of the id
// instead, which is what fd_rules.rs guarantees; the ruleset itself is the authority on which ids
// are real, and it is not reachable from the browser.
const eventId = (v: unknown) => typeof v === 'string' && /^[a-z][a-z0-9_]{1,31}$/.test(v)
function object(v: unknown, keys: string[], optional: string[] = []): Record<string, unknown> {
  if (!v || typeof v !== 'object' || Array.isArray(v) || !keys.every(k => Object.prototype.hasOwnProperty.call(v, k)) ||
    Object.keys(v).some(k => !keys.includes(k) && !optional.includes(k))) throw new Error('invalidFieldDay')
  return v as Record<string, unknown>
}
function status(v: unknown): void {
  const f = object(v, ['myClass','mySection','running','state','dxcall','qsoCount','sections','workedSections','points','event',
    'poweredPoints','bonusPoints','totalScore','eventStartUnix','eventEndUnix','rulesYear','rulesGenerated','assistanceOn','log'], ['club'])
  if (![f.myClass,f.mySection,f.state,f.rulesGenerated].every(text) || (f.dxcall !== null && !text(f.dxcall)) ||
    typeof f.running !== 'boolean' || !eventId(f.event) ||
    ![f.qsoCount,f.sections,f.points,f.poweredPoints,f.bonusPoints,f.totalScore,f.eventStartUnix,f.eventEndUnix,f.rulesYear].every(integer) ||
    Number(f.eventEndUnix) <= Number(f.eventStartUnix) || !texts(f.workedSections,2048) || !texts(f.assistanceOn,64) ||
    !Array.isArray(f.log) || f.log.length > 2048 || f.qsoCount !== f.log.length || f.sections !== (f.workedSections as unknown[]).length) throw new Error('invalidFieldDay')
  for (const raw of f.log) {
    const q = object(raw,['call','class','section','band','mode','submode','whenUnix'])
    if (![q.call,q.class,q.section,q.band,q.submode].every(text) || !['CW','PH','DIG'].includes(String(q.mode)) || !integer(q.whenUnix)) throw new Error('invalidFieldDay')
  }
  if (f.club !== undefined && f.club !== null) {
    const c = object(f.club,['syncState','queued','offlineSinceUnix','hosting','event','hostCall','score','qsos','sections','skewSecs','dupes','board'],['lastError'])
    if (!['disabled','offline','behind','synced'].includes(String(c.syncState)) || typeof c.hosting !== 'boolean' || ![c.event,c.hostCall].every(text) ||
      ![c.queued,c.offlineSinceUnix,c.score,c.qsos,c.sections].every(integer) || !Number.isSafeInteger(c.skewSecs) ||
      (c.lastError !== undefined && c.lastError !== null && !text(c.lastError)) || !Array.isArray(c.dupes) || c.dupes.length > 4096 ||
      !c.dupes.every(d => Array.isArray(d) && d.length === 3 && d.every(text)) || !Array.isArray(c.board) || c.board.length > 128) throw new Error('invalidFieldDay')
    for (const raw of c.board) {
      const r = object(raw,['posid','posName','band','mode','operator','qsos','rate','lastSeenSecs'])
      if (![r.posid,r.posName,r.band,r.mode,r.operator].every(text) || ![r.qsos,r.rate,r.lastSeenSecs].every(integer)) throw new Error('invalidFieldDay')
    }
  }
}
export function parseFieldDay(page: QueryPage): FieldDayObservation & { capturedAgeMs: number } {
  const meta = page.meta as Record<string,unknown> | null
  if (page.collection !== 'fieldDay' || page.offset !== 0 || page.total !== 0 || page.retained !== 0 || page.rows.length !== 0 || page.nextCursor !== null ||
    !meta || !integer(meta.capturedAgeMs) || Number(meta.capturedAgeMs) >= FIELD_DAY_TTL_MS) throw new Error('invalidFieldDay')
  const value = object(meta.source,['active','fieldDay','settings','ruleset'])
  if (new TextEncoder().encode(JSON.stringify(value)).length > 224*1024 || typeof value.active !== 'boolean' || (!value.active && value.fieldDay !== null)) throw new Error('invalidFieldDay')
  const s = object(value.settings,['fdOperator','fdPowerMult','fdBonuses','fdBonusesPlanned'])
  if (!text(s.fdOperator) || ![1,2,5].includes(Number(s.fdPowerMult)) || typeof s.fdPowerMult !== 'number' || !texts(s.fdBonuses,64) || !texts(s.fdBonusesPlanned,64)) throw new Error('invalidFieldDay')
  const r = object(value.ruleset,['event','rulesYear','bannedModes','spottingAllowed','clusterAllowed','enforcement'])
  if (!['arrlfd','wfd'].includes(String(r.event)) || !integer(r.rulesYear) || !texts(r.bannedModes,64) || typeof r.spottingAllowed !== 'boolean' ||
    typeof r.clusterAllowed !== 'boolean' || !text(r.enforcement)) throw new Error('invalidFieldDay')
  if (value.fieldDay !== null) status(value.fieldDay)
  return {...value as unknown as FieldDayObservation, capturedAgeMs:Number(meta.capturedAgeMs)}
}
