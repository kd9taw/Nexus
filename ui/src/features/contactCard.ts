// THE CONTACT CARD'S EDIT — what the Logbook's contact card sends when the operator presses Save.
// Pure: no React, no IO.
//
// ⭐ NO SILENT CHANGE. The edit is the contact AS STORED (`stored`, the same projection the backend
// computes the edit key over: `QsoEdit::project`) with only the boxes the operator changed laid over
// it. A box left alone is never re-read from its text, so nothing the text cannot say exactly — a
// frequency to more places than the box shows, a time's seconds, a comment imported with a line
// break, a call with padding — is rounded, trimmed or reformatted on the way back. The fields kept
// as written (`extra`) go the same way: only a changed one is sent (`FieldEdit`), each with the value
// it was changed from.
//
// ⚠️ BLANK MEANS "LEAVE ALONE" on several fields, in the backend's one edit policy
// (`Logbook::update_record`): the state, the end time, the park references when both are blank,
// the IOTA reference, your own grid and rig. Clearing one of those boxes would save, report success,
// and change nothing. So the card refuses it, naming the field, instead of saving a change it
// cannot make.
import type { FieldEdit, QsoEdit } from '../api'
import type { LoggedQso } from '../types'
import { parseUtcTime, utcDate, utcDateTimeToUnix, utcTime } from './utcLog'

/** Every box the card edits, as typed. Strings, so one setter serves them all. */
export interface CardDraft {
  call: string
  date: string
  timeOn: string
  timeOff: string
  band: string
  freq: string
  mode: string
  rstSent: string
  rstRcvd: string
  txPower: string
  name: string
  qth: string
  grid: string
  state: string
  theirProgram: string
  theirRef: string
  myProgram: string
  myRef: string
  iota: string
  myGrid: string
  myRig: string
  /** '' not sent, 'B' | 'D' | 'E', or 'SENT' for a stored mark with no method. */
  qslSentVia: string
  /** '1' a paper card arrived, '' none. */
  qslCard: string
  comment: string
  notes: string
}

/** One field kept as written, as the card shows it: the value it opened with (`orig`, null for a
 *  field added on the card) and the value now. `removed` takes it off on Save. */
export interface CardField {
  tag: string
  orig: string | null
  value: string
  removed: boolean
}

/** The programmes a park or summit reference can be in, as the log stores them. */
export const PROGRAMS = ['POTA', 'SOTA', 'WWFF'] as const

/** The fields kept as written that Nexus or a service keeps for itself: shown, never edited
 *  (`FieldEdit::check` refuses them too). */
export function ownField(tag: string): boolean {
  return tag.startsWith('APP_NEXUS_') || tag.startsWith('APP_TEMPO_') || tag === 'APP_EQSL_AG'
}

/** The ADIF fields whose type is a multi-line string. */
const MULTILINE_TAGS: ReadonlySet<string> = new Set([
  'ADDRESS', 'ADDRESS_INTL', 'NOTES', 'NOTES_INTL', 'QSLMSG', 'QSLMSG_INTL', 'QSLMSG_RCVD', 'RIG', 'RIG_INTL',
])

/** Whether a field kept as written is edited in a box that holds lines: an ADIF multi-line field,
 *  or any value that holds a line break. A one-line box drops line breaks from what it shows, so
 *  the first keystroke in it would glue the lines together. */
export function multiLine(f: CardField): boolean {
  return MULTILINE_TAGS.has(f.tag.trim().toUpperCase()) || /[\r\n]/.test(f.orig ?? '') || /[\r\n]/.test(f.value)
}

/** What a multi-line box typed, with the line breaks the field was written with: a browser's box
 *  gives back bare line feeds. While the text is the stored text (typed away and back included),
 *  it is the stored value exactly, mixed breaks and all; once edited, a field written with CR LF
 *  (ADIF's own) keeps CR LF, and one written with bare CRs keeps those. */
export function typedLines(f: CardField, typed: string): string {
  const orig = f.orig ?? ''
  if (f.orig !== null && typed === orig.replace(/\r\n?/g, '\n')) return orig
  if (orig.includes('\r\n')) return typed.replace(/\r?\n/g, '\r\n')
  if (orig.includes('\r')) return typed.replace(/\r?\n/g, '\r')
  return typed
}

const IOTA_RE = /^(AF|AN|AS|EU|NA|OC|SA)-\d{3}$/

/** The card's boxes, filled from the contact as stored. */
export function draftOf(q: LoggedQso): CardDraft {
  return {
    call: q.call,
    date: utcDate(q.whenUnix),
    // A date-only contact has no time of day: its box is empty, and left empty it stays so.
    timeOn: q.timeKnown === false ? '' : utcTime(q.whenUnix),
    timeOff: q.timeOffUnix != null ? utcTime(q.timeOffUnix) : '',
    band: q.band,
    freq: String(q.freqMhz),
    mode: q.mode,
    rstSent: q.rstSent ?? '',
    rstRcvd: q.rstRcvd ?? '',
    txPower: q.txPower != null ? String(q.txPower) : '',
    name: q.name ?? '',
    qth: q.qth ?? '',
    grid: q.grid ?? '',
    state: q.state ?? '',
    theirProgram: q.ota?.theirProgram ?? '',
    theirRef: q.ota?.theirRef ?? '',
    myProgram: q.ota?.myProgram ?? '',
    myRef: q.ota?.myRef ?? '',
    iota: q.ota?.iota ?? '',
    myGrid: q.myGrid ?? '',
    myRig: q.myRig ?? '',
    qslSentVia: q.qslSent?.sent ? (q.qslSent.via ?? 'SENT') : '',
    qslCard: q.qslRcvd?.card ? '1' : '',
    comment: q.comment ?? '',
    notes: q.notes ?? '',
  }
}

/** The fields kept as written, as the card lists them. */
export function fieldsOf(q: LoggedQso): CardField[] {
  return (q.extra ?? []).map(([tag, value]) => ({ tag, orig: value, value, removed: false }))
}

/** The contact as the backend projects it for an edit (`QsoEdit::project`): what Save sends for
 *  every box the operator did not change. */
export function storedEdit(q: LoggedQso): QsoEdit {
  return {
    call: q.call,
    grid: q.grid ?? null,
    state: q.state ?? null,
    band: q.band,
    freqMhz: q.freqMhz,
    mode: q.mode,
    rstSent: q.rstSent ?? null,
    rstRcvd: q.rstRcvd ?? null,
    name: q.name ?? null,
    qth: q.qth ?? null,
    comment: q.comment ?? null,
    notes: q.notes ?? null,
    txPower: q.txPower ?? null,
    whenUnix: q.whenUnix,
    timeOffUnix: q.timeOffUnix ?? null,
    ota: {
      myProgram: q.ota?.myProgram ?? null,
      myRef: q.ota?.myRef ?? null,
      theirProgram: q.ota?.theirProgram ?? null,
      theirRef: q.ota?.theirRef ?? null,
      iota: q.ota?.iota ?? null,
    },
    myGrid: q.myGrid ?? null,
    myRig: q.myRig ?? null,
    qslSentVia: q.qslSent?.sent ? ((q.qslSent.via ?? 'SENT') as QsoEdit['qslSentVia']) : null,
    qslCard: q.qslRcvd?.card ?? false,
  }
}

/** Whether anything on the card differs from what it opened with. */
export function cardDirty(start: CardDraft, draft: CardDraft, fields: readonly CardField[]): boolean {
  return (
    (Object.keys(start) as (keyof CardDraft)[]).some((k) => start[k] !== draft[k]) ||
    fieldChanges(fields).length > 0
  )
}

/** The changes to fields kept as written: a corrected, removed or added one — never one left alone.
 *
 *  ⚠️ A field is a removal only when the OPERATOR removed or emptied it. An import keeps a field
 *  written empty (`<QSLMSG:0>`) as it was, so an empty value the card opened with is a field left
 *  alone, never one to take off. A typed value is trimmed, as the edit form trims its boxes; one
 *  that trims to what the field held is no change. */
export function fieldChanges(fields: readonly CardField[]): FieldEdit[] {
  const out: FieldEdit[] = []
  for (const f of fields) {
    const typed = f.value.trim()
    if (f.orig === null) {
      if (!f.removed && typed) out.push({ tag: f.tag.trim().toUpperCase(), old: null, new: typed })
    } else if (f.removed || (f.value !== f.orig && !typed)) {
      out.push({ tag: f.tag, old: f.orig, new: null })
    } else if (f.value !== f.orig && typed !== f.orig) {
      out.push({ tag: f.tag, old: f.orig, new: typed })
    }
  }
  return out
}

/** A field kept as written that says where YOUR station was, or whose station it was. */
function ownStationTag(tag: string): boolean {
  const t = tag.trim().toUpperCase()
  return t.startsWith('MY_') || t === 'OWNER_CALLSIGN'
}

/** What a callsign correction takes off, among the values the card shows. */
export type CallFixClear = 'CQZ' | 'ITUZ' | 'CONT' | 'CNTY' | 'lotw' | 'eqsl'

/** The shown values a save that corrects the call will take off with the old call (the backend's
 *  edit policy): the zones, continent and county the contact holds, where this save leaves them as
 *  they are — one changed in the same save is the operator's and stays — and the LoTW and eQSL
 *  confirmations. Empty when the call is not corrected: the backend compares it trimmed, in any
 *  capitals, and so does this. */
export function clearedByCallFix(q: LoggedQso, d: CardDraft, fields: readonly CardField[]): CallFixClear[] {
  if (d.call.trim().toUpperCase() === q.call.trim().toUpperCase()) return []
  const left = (tag: string) =>
    fields.some((f) => f.orig !== null && f.tag.trim().toUpperCase() === tag && !f.removed && f.value === f.orig)
  const out: CallFixClear[] = (['CQZ', 'ITUZ', 'CONT', 'CNTY'] as const).filter(left)
  if (q.qslRcvd?.lotw || left('LOTW_QSLRDATE')) out.push('lotw')
  if (q.qslRcvd?.eqsl || left('EQSL_QSLRDATE')) out.push('eqsl')
  return out
}

/** The own-station fields this save changes on a contact LoTW has already taken, by ADIF name:
 *  your grid's box and any `MY_…` or `OWNER_CALLSIGN` field kept as written.
 *
 *  ⚠️ They are saved, and NOT sent to LoTW again: a contact's upload is queued again only when its
 *  call is corrected (the edit policy), and LoTW keeps the contact it was sent. So the card says
 *  so, rather than let the operator believe LoTW now holds the correction. Empty when LoTW has not
 *  taken the contact, or the call is corrected too (then everything goes again). */
export function ownStationNotResent(q: LoggedQso, start: CardDraft, d: CardDraft, fields: readonly CardField[]): string[] {
  const lotw = q.upload?.lotw?.outcome
  if (lotw !== 'accepted' && lotw !== 'duplicate') return []
  if (d.call.trim().toUpperCase() !== start.call.trim().toUpperCase()) return []
  const out: string[] = []
  if (d.myGrid !== start.myGrid && d.myGrid.trim().toUpperCase() !== (q.myGrid ?? '')) out.push('MY_GRIDSQUARE')
  for (const c of fieldChanges(fields)) if (ownStationTag(c.tag)) out.push(c.tag)
  return out
}

/** A box whose empty value the backend reads as "leave it" (or both park boxes at once). */
export type Unclearable = 'state' | 'timeOff' | 'iota' | 'myGrid' | 'myRig' | 'parks'

/** Why Save cannot be made. */
export type CardError =
  | { code: 'cannotClear'; field: Unclearable }
  | { code: 'call' | 'when' | 'end' | 'freq' | 'power' | 'iota' }
  | { code: 'tag' | 'twice' | 'contest'; tag: string }

/** The edit and field changes Save sends — or why it cannot. */
export type CardSave = { ok: true; edit: QsoEdit; fields: FieldEdit[] } | { ok: false; error: CardError }

const blankToNull = (v: string) => (v.trim() === '' ? null : v.trim())

/** The edit Save sends: the stored contact, with the changed boxes laid over it. */
export function cardSave(q: LoggedQso, start: CardDraft, d: CardDraft, fields: readonly CardField[]): CardSave {
  const edit = storedEdit(q)
  const changed = (k: keyof CardDraft) => d[k] !== start[k]
  // A box whose blank the backend reads as "leave it": clearing it would save nothing.
  const cleared = (k: keyof CardDraft) => changed(k) && d[k].trim() === '' && start[k].trim() !== ''
  const no = (error: CardError): CardSave => ({ ok: false, error })
  for (const k of ['state', 'timeOff', 'iota', 'myGrid', 'myRig'] as const) {
    if (cleared(k)) return no({ code: 'cannotClear', field: k })
  }
  if ((cleared('theirRef') || cleared('myRef')) && !d.theirRef.trim() && !d.myRef.trim()) {
    return no({ code: 'cannotClear', field: 'parks' })
  }

  if (changed('call')) {
    const call = d.call.trim().toUpperCase()
    if (!call) return no({ code: 'call' })
    edit.call = call
  }
  if (changed('date') || changed('timeOn')) {
    const when = utcDateTimeToUnix(d.date, d.timeOn.trim() === '' ? '00:00' : d.timeOn)
    if (when === null) return no({ code: 'when' })
    edit.whenUnix = when
  }
  if (changed('timeOff') || (edit.whenUnix !== q.whenUnix && d.timeOff.trim() !== '')) {
    const tod = parseUtcTime(d.timeOff)
    if (!tod) return no({ code: 'end' })
    // On the contact's own UTC date; earlier in the day than the start is the rollover (#329).
    const midnight = Math.floor(edit.whenUnix / 86400) * 86400
    const end = midnight + tod.h * 3600 + tod.m * 60 + (tod.s ?? 0)
    edit.timeOffUnix = end < edit.whenUnix ? end + 86400 : end
  }
  if (changed('band')) edit.band = d.band.trim()
  if (changed('freq')) {
    const f = Number(d.freq)
    if (!d.freq.trim() || !Number.isFinite(f) || f <= 0) return no({ code: 'freq' })
    edit.freqMhz = f
  }
  if (changed('mode')) edit.mode = d.mode.trim().toUpperCase()
  if (changed('rstSent')) edit.rstSent = blankToNull(d.rstSent)
  if (changed('rstRcvd')) edit.rstRcvd = blankToNull(d.rstRcvd)
  if (changed('txPower')) {
    const p = d.txPower.trim() === '' ? null : Number(d.txPower)
    if (p !== null && (!Number.isFinite(p) || p < 0)) return no({ code: 'power' })
    edit.txPower = p
  }
  if (changed('name')) edit.name = blankToNull(d.name)
  if (changed('qth')) edit.qth = blankToNull(d.qth)
  if (changed('grid')) edit.grid = blankToNull(d.grid)?.toUpperCase() ?? null
  if (changed('state')) edit.state = d.state.trim().toUpperCase()
  if (changed('comment')) edit.comment = blankToNull(d.comment)
  // Notes keep their lines; only a box left empty is no note.
  if (changed('notes')) edit.notes = d.notes.trim() === '' ? null : d.notes
  if (changed('theirRef') || changed('theirProgram')) {
    const ref = blankToNull(d.theirRef)?.toUpperCase() ?? null
    edit.ota.theirRef = ref
    edit.ota.theirProgram = ref ? d.theirProgram || 'POTA' : null
  }
  if (changed('myRef') || changed('myProgram')) {
    const ref = blankToNull(d.myRef)?.toUpperCase() ?? null
    edit.ota.myRef = ref
    edit.ota.myProgram = ref ? d.myProgram || 'POTA' : null
  }
  if (changed('iota')) {
    const iota = d.iota.trim().toUpperCase()
    if (!IOTA_RE.test(iota)) return no({ code: 'iota' })
    edit.ota.iota = iota
  }
  if (changed('myGrid')) edit.myGrid = d.myGrid.trim().toUpperCase()
  if (changed('myRig')) edit.myRig = d.myRig.trim()
  if (changed('qslSentVia')) edit.qslSentVia = (d.qslSentVia || null) as QsoEdit['qslSentVia']
  if (changed('qslCard')) edit.qslCard = d.qslCard === '1'

  const changes = fieldChanges(fields)
  const tags = changes.map((c) => c.tag)
  for (const c of changes) {
    if (!/^[A-Z0-9_]+$/.test(c.tag)) return no({ code: 'tag', tag: c.tag })
  }
  const dup = tags.find((tag, i) => tags.indexOf(tag) !== i)
  const held = fields.filter((f) => f.orig !== null).map((f) => f.tag)
  const clash = changes.find((c) => c.old === null && held.includes(c.tag))
  if (dup || clash) return no({ code: 'twice', tag: (dup ?? clash?.tag) as string })
  // A merged contest contact's exchange keeps its own ADIF columns (a QSO party's CNTY, a section,
  // a zone), and exports, uploads and awards read them from there: a change to one beside it would
  // be saved and reach none of them (`FieldEdit::check_on` refuses it too). A column holding only
  // blanks holds it as well: the writer writes every non-empty column in place of a field beside it.
  const exchange = (q.contest?.adif ?? []).filter(([, v]) => v !== '').map(([k]) => k.toUpperCase())
  const inExchange = changes.find((c) => exchange.includes(c.tag))
  if (inExchange) return no({ code: 'contest', tag: inExchange.tag })
  return { ok: true, edit, fields: changes }
}
