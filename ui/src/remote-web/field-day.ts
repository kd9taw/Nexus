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
  // The station sends its whole FieldDayStatus, so this list has to track that struct. It drifted
  // badly: `myClass`/`mySection` were DELETED from the DTO on purpose (two interop emitters were
  // reading a session-level exchange inside their per-QSO loops and stamping it onto every row),
  // while the contest programme added multCount, scoreNoteKey, upload, receives, composing, role
  // and boards. Requiring two fields nobody sends and refusing seven that everybody does meant
  // every Field Day payload was rejected - the view was blank through Remote for every event.
  // The additions are OPTIONAL so this validates both directions: an older station that does not
  // send them still passes, and a newer one is no longer refused by an older browser.
  const f = object(v, ['running','state','dxcall','qsoCount','sections','workedSections','points','event',
    'poweredPoints','bonusPoints','totalScore','eventStartUnix','eventEndUnix','rulesYear','rulesGenerated',
    'assistanceOn','log'],
    ['club','multCount','scoreNoteKey','upload','receives','composing','role','boards','myClass','mySection'])
  if (![f.state,f.rulesGenerated].every(text) || (f.dxcall !== null && !text(f.dxcall)) ||
    (f.myClass !== undefined && !text(f.myClass)) || (f.mySection !== undefined && !text(f.mySection)) ||
    (f.scoreNoteKey !== undefined && !text(f.scoreNoteKey)) || (f.role !== undefined && !text(f.role)) ||
    (f.multCount !== undefined && f.multCount !== null && !integer(f.multCount)) ||
    // `composing` is a VECTOR by design, never a preformatted exchange string - a row's own sent
    // exchange is its `mex`. Bounded here rather than re-modelled: the real Nexus app is what
    // consumes these, and this gate exists to cap size and shape, not to duplicate the DTO.
    [f.receives,f.composing,f.boards].some(x => x !== undefined && (!Array.isArray(x) || x.length > 512)) ||
    (f.upload !== undefined && (!f.upload || typeof f.upload !== 'object' || Array.isArray(f.upload))) ||
    typeof f.running !== 'boolean' || !eventId(f.event) ||
    ![f.qsoCount,f.sections,f.points,f.poweredPoints,f.bonusPoints,f.totalScore,f.eventStartUnix,f.eventEndUnix,f.rulesYear].every(integer) ||
    Number(f.eventEndUnix) <= Number(f.eventStartUnix) || !texts(f.workedSections,2048) || !texts(f.assistanceOn,64) ||
    !Array.isArray(f.log) || f.log.length > 2048 || f.qsoCount !== f.log.length || f.sections !== (f.workedSections as unknown[]).length) throw new Error('invalidFieldDay')
  for (const raw of f.log) {
    // `mex` is the row's OWN sent exchange, which is where a per-QSO exchange belongs now that
    // the session-level class/section pair is gone from the status struct. Optional so a log
    // written by an older station still validates.
    const q = object(raw,['call','class','section','band','mode','submode','whenUnix'],['mex'])
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
  // Same drift as the status struct above, and this is the one that actually fired: the contest
  // programme gave the ruleset an `exchange` (the ExchangeSpec that replaced the hardcoded
  // class/section pair), a `problem` and a `role`. Optional for both-direction compatibility.
  const r = object(value.ruleset,['event','rulesYear','bannedModes','spottingAllowed','clusterAllowed','enforcement'],
    ['exchange','problem','role'])
  if (!['arrlfd','wfd'].includes(String(r.event)) || !integer(r.rulesYear) || !texts(r.bannedModes,64) || typeof r.spottingAllowed !== 'boolean' ||
    typeof r.clusterAllowed !== 'boolean' || !text(r.enforcement) ||
    (r.role !== undefined && !text(r.role)) ||
    (r.problem !== undefined && r.problem !== null && !text(r.problem)) ||
    // The exchange is a spec object, bounded here rather than re-modelled - the real Nexus app
    // is what renders it, and this gate caps shape and size, it does not duplicate the DTO.
    (r.exchange !== undefined && r.exchange !== null &&
      (typeof r.exchange !== 'object' || (Array.isArray(r.exchange) && r.exchange.length > 64)))) throw new Error('invalidFieldDay')
  if (value.fieldDay !== null) status(value.fieldDay)
  return {...value as unknown as FieldDayObservation, capturedAgeMs:Number(meta.capturedAgeMs)}
}
