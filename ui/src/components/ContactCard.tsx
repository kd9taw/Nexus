// THE CONTACT CARD — one logged contact, opened from the Logbook (double-click a row, or Enter on
// the row the keyboard is on), where every field it holds can be corrected.
//
// "When you open a specific entry, it pops a card where you can edit details, see the QRZ default
// picture, link to QRZ bio for the call, etc.. The card should show recent QSOs that are logged,
// band, freq, mode, RST, etc." (operator, 2026-10-26.)
//
// ⭐ ONE WRITER. The card is a second place to edit a contact, not a second way to write one: its
// Save is the Logbook edit form's (`editQsoById`, handed in as `save`), so it is refused the same
// way when the contact changed elsewhere, recorded as one change in the contact's history, and
// undone by that change's Undo (the History section below). What it sends is the contact as stored
// with only the boxes the operator changed laid over it (`features/contactCard`), so a field left
// alone is written back exactly as it was.
//
// Nothing is asked of the network on open: the callbook answer and photo are the ones this window
// already has (`features/callbookAnswers`), and Look up asks only when pressed. The QRZ button opens
// the call's page in the browser, as the station card's does.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; ADIF field names, band, mode, RST and the QSL letters are wire
// formats and shown as they are.
import './ContactCard.css'
import { useId, useMemo, useState, useSyncExternalStore } from 'react'
import type { FieldEdit, QsoEdit, RowAnswer, RowRef } from '../api'
import { qrzLookup } from '../api'
import type { LoggedQso } from '../types'
import { Dialog } from './ui/Dialog'
import { QsoHistory } from './QsoHistory'
import { IotaReferenceNote } from './Iota'
import { CallbookPhoto, qrzPageOf } from './CallCard'
import { confirmDialog } from '../confirm'
import { useFocusReturn } from '../focusReturn'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { oneLineTyped } from '../typedInput'
import { callbookAnswer, subscribeCallbookAnswers } from '../features/callbookAnswers'
import { useLogAnswer } from '../features/logSource'
import { emptyAnswer } from '../features/logAnswers'
import { fmtUtc } from '../features/logQuery'
import { UTC_DATE_FORMAT, UTC_TIME_FORMATS } from '../features/utcLog'
import { addedColumnText, columnName, logColumn, uploadMark, uploadOf, uploadWord } from '../features/logColumns'
import {
  PROGRAMS,
  cardDirty,
  cardSave,
  clearedByCallFix,
  draftOf,
  fieldsOf,
  multiLine,
  ownField,
  ownStationNotResent,
  typedLines,
  type CallFixClear,
  type CardDraft,
  type CardError,
  type CardField,
  type Unclearable,
} from '../features/contactCard'

/** The mark on a contact whose IOTA reference a spot's comment filled (`offer::SPOT_FILLED_FIELD`). */
const SPOT_FILLED_FIELD = 'APP_NEXUS_SPOT_FILLED'

/** Service names and the QRZ button: proper nouns, not words. */
const QRZ_PAGE = 'QRZ ↗'
/** A value a corrected call takes off, as the card names it: a place under its Logbook column's
 *  name, a confirmation in words. */
function callFixLabel(c: CallFixClear): string {
  if (c === 'lotw') return t('contactCard.callFix.lotw')
  if (c === 'eqsl') return t('contactCard.callFix.eqsl')
  const column = logColumn({ CQZ: 'cqz', ITUZ: 'ituz', CONT: 'cont', CNTY: 'county' }[c])
  return column ? columnName(column) : c
}

/** How many of the call's contacts the card lists. */
export const RECENT = 10

/** The contact the card is open on: the row as the list read it, and the reference a save sends
 *  (null on a Remote browser, which edits nothing from here). */
export interface CardContact {
  row: LoggedQso
  ref: RowRef | null
}

export interface ContactCardProps {
  open: CardContact | null
  onClose: () => void
  /** Open another of the call's contacts on the card, by its id. */
  onSwitch: (id: string) => void
  /** The Logbook form's save: one change through `editQsoById`, with its toasts. Resolves with the
   *  engine's answer, or null when it failed (the toast has said why). */
  save: (ref: RowRef, edit: QsoEdit, fields: FieldEdit[], call: string) => Promise<RowAnswer | null>
  /** A change was made (a save, or an undo from the history): the list reads the log again. */
  onChanged: () => void
  /** Read the contact `id` again onto the card, if the card is still open on it: after an Undo or
   *  Redo in its History, which change the contact behind the card's back. */
  onReload: (id: string) => void
  logTick?: number
}

/** The box an empty value cannot clear, by the name the card shows it under. */
function unclearableLabel(f: Unclearable): string {
  switch (f) {
    case 'state':
      return t('logbook.field.state.label')
    case 'timeOff':
      return t('logbook.field.end.label')
    case 'iota':
      return t('logbook.field.iota.label')
    case 'myGrid':
      return t('logbook.field.myGrid.label')
    case 'myRig':
      return t('logbook.field.myRig.label')
    case 'parks':
      return t('contactCard.parks')
  }
}

/** Why Save was not made, in the operator's words. */
function errorText(e: CardError): string {
  switch (e.code) {
    case 'cannotClear':
      return t('contactCard.error.cannotClear', { label: unclearableLabel(e.field) })
    case 'call':
      return t('logbook.form.callRequired')
    case 'when':
      return t('contactCard.error.when')
    case 'end':
      return t('contactCard.error.end')
    case 'freq':
      return t('contactCard.error.freq')
    case 'power':
      return t('contactCard.error.power')
    case 'iota':
      return t('logbook.form.iotaInvalid')
    case 'tag':
      return t('contactCard.error.tag', { tag: e.tag })
    case 'twice':
      return t('contactCard.error.twice', { tag: e.tag })
    case 'contest':
      return t('contactCard.error.contest', { tag: e.tag })
  }
}

export function ContactCard({ open, onClose, onSwitch, save, onChanged, onReload, logTick }: ContactCardProps) {
  const returnFocus = useFocusReturn(!!open)
  if (!open) return null
  return (
    <CardBody
      // A new contact is a new card: nothing typed on one carries to the next. So is the same
      // contact as it now is — after a save, or a refusal that reads it again. The edit key does not
      // cover the fields kept as written (a confirmation download writes there), so the key is the
      // whole row the card is filled from: anything it shows changed, it is filled again.
      key={`${open.row.id ?? ''}|${open.ref?.editKey ?? ''}|${JSON.stringify(open.row)}`}
      contact={open}
      onClose={onClose}
      onSwitch={onSwitch}
      save={save}
      onChanged={onChanged}
      onReload={onReload}
      logTick={logTick}
      returnFocus={returnFocus}
    />
  )
}

function CardBody({
  contact,
  onClose,
  onSwitch,
  save,
  onChanged,
  onReload,
  logTick,
  returnFocus,
}: Omit<ContactCardProps, 'open'> & { contact: CardContact; returnFocus: (e: Event) => void }) {
  const q = contact.row
  const start = useMemo(() => draftOf(q), [q])
  const [draft, setDraft] = useState<CardDraft>(start)
  const [fields, setFields] = useState<CardField[]>(() => fieldsOf(q))
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [looking, setLooking] = useState(false)
  const dirty = cardDirty(start, draft, fields)
  const notResent = ownStationNotResent(q, start, draft, fields)
  const canSave = contact.ref !== null
  const id = useId()
  const book = useSyncExternalStore(subscribeCallbookAnswers, () => callbookAnswer(q.call))

  // The call's contacts the log holds, newest first — the recall card's own question.
  const question = { kind: 'callHistory', call: q.call, band: q.band, mode: q.mode, matchMode: false } as const
  const hist = useLogAnswer(question, logTick) ?? emptyAnswer(question)
  const recent = [...hist.qsos].sort((a, b) => b.whenUnix - a.whenUnix).slice(0, RECENT)

  const set = (k: keyof CardDraft, v: string) => {
    setErr(null)
    setDraft((d) => ({ ...d, [k]: v }))
  }
  const setField = (i: number, patch: Partial<CardField>) => {
    setErr(null)
    setFields((fs) => fs.map((f, j) => (j === i ? { ...f, ...patch } : f)))
  }

  /** Leave this contact — closing the card, or opening another — only once the operator has said
   *  what becomes of what they typed. */
  const leave = async (then: () => void) => {
    if (
      dirty &&
      !(await confirmDialog({
        title: t('contactCard.discard.title', { call: q.call }),
        body: t('contactCard.discard.body'),
        confirmLabel: t('contactCard.discard.confirm'),
        cancelLabel: t('contactCard.discard.keep'),
      }))
    ) {
      return
    }
    then()
  }

  const onSave = async (e?: React.FormEvent) => {
    e?.preventDefault()
    if (!contact.ref || busy) return
    const made = cardSave(q, start, draft, fields)
    if (!made.ok) {
      setErr(errorText(made.error))
      return
    }
    // A corrected call takes off what belonged to the old one: say which of the values on the card
    // go before they do, and save only once the operator says so.
    const clears = clearedByCallFix(q, draft, fields)
    if (
      clears.length &&
      !(await confirmDialog({
        title: t('contactCard.callFix.title', { old: q.call, call: made.edit.call }),
        body: t('contactCard.callFix.body', { old: q.call, fields: clears.map(callFixLabel).join(', ') }),
        confirmLabel: t('contactCard.callFix.confirm'),
        cancelLabel: t('contactCard.discard.keep'),
      }))
    ) {
      return
    }
    setBusy(true)
    const answer = await save(contact.ref, made.edit, made.fields, q.call)
    setBusy(false)
    // A refusal the toast has explained; `changed` re-opens the card on the contact as it now is.
    if (answer?.kind === 'applied' || answer?.kind === 'changed') {
      onChanged()
      if (q.id) onSwitch(q.id)
    }
  }

  // An IOTA reference a spot's comment filled is kept in the log and sent to no service until the
  // operator confirms it (`tempo_core::logbook::offer`). Confirm takes the mark off: the one
  // Nexus-kept field a card may change, and only by removing it.
  const spotMark = q.extra?.find(([k]) => k === SPOT_FILLED_FIELD)?.[1] ?? null
  const spotHeld = !!spotMark?.split(',').some((tag) => tag.trim() === 'IOTA')
  const confirmIota = async () => {
    if (!contact.ref || busy || !spotMark) return
    const made = cardSave(q, start, start, fieldsOf(q))
    if (!made.ok) return
    setBusy(true)
    const answer = await save(contact.ref, made.edit, [...made.fields, { tag: SPOT_FILLED_FIELD, old: spotMark, new: null }], q.call)
    setBusy(false)
    if (answer?.kind === 'applied' || answer?.kind === 'changed') {
      onChanged()
      if (q.id) onSwitch(q.id)
    }
  }

  const lookUp = () => {
    setLooking(true)
    void withErrorToast(() => qrzLookup(q.call), t('callbook.lookupFailed')).finally(() => setLooking(false))
  }

  const name = (book ? book.nickname || book.name || '' : (q.name ?? '')).trim()
  const where = [book ? book.qth : q.qth, book ? book.state : q.state, book?.country ?? q.entity ?? q.country]
    .map((s) => s?.trim())
    .filter(Boolean)
    .join(' · ')

  const box = (k: keyof CardDraft, label: string, extra: React.InputHTMLAttributes<HTMLInputElement> = {}) => (
    <label className="logbook-field">
      <span>{label}</span>
      <input
        className="settings-input"
        value={draft[k]}
        onChange={(e) => set(k, e.target.value)}
        autoComplete="off"
        spellCheck={false}
        readOnly={!canSave}
        {...extra}
      />
    </label>
  )
  const program = (k: 'theirProgram' | 'myProgram', label: string) => (
    <label className="logbook-field">
      <span>{label}</span>
      <select className="settings-input" value={draft[k]} onChange={(e) => set(k, e.target.value)} disabled={!canSave}>
        <option value="">—</option>
        {PROGRAMS.map((p) => (
          <option key={p} value={p}>
            {p}
          </option>
        ))}
        {draft[k] && !(PROGRAMS as readonly string[]).includes(draft[k]) && <option value={draft[k]}>{draft[k]}</option>}
      </select>
    </label>
  )

  // What the record holds that this card does not edit: shown so a correction is made with the whole
  // contact in view. Absent is omitted, never printed blank.
  const fact = (label: string, value: unknown): [string, string] | null =>
    value === null || value === undefined || value === '' || (Array.isArray(value) && !value.length)
      ? null
      : [label, Array.isArray(value) ? value.join(' · ') : String(value)]
  const col = (cid: string) => {
    const c = logColumn(cid)
    return c ? fact(columnName(c), addedColumnText(q, cid)) : null
  }
  // One service's upload stamp: its state, and when the service answered.
  const upload = (cid: string) => {
    const c = logColumn(cid)
    const st = uploadOf(q, cid)
    if (!c || !st) return null
    const word = uploadWord(uploadMark(st))
    return fact(columnName(c), st.whenUnix ? `${word} · ${fmtUtc(st.whenUnix)}` : word)
  }
  const rcvd = q.qslRcvd
  const sent = q.qslSent
  const yes = t('qso.detail.qsl.yes')
  const facts = [
    fact(t('logbook.column.country'), q.entity ?? q.country),
    fact('DXCC', q.dxcc),
    fact(t('logbook.column.operator'), q.operator),
    fact(t('qso.detail.stationCallsign'), q.stationCallsign),
    fact(t('contactCard.freqRx'), q.freqRxMhz),
    fact(t('qso.detail.propMode'), q.propMode),
    fact(t('qso.detail.satName'), q.satName),
    fact(
      t('qso.detail.qsl.received'),
      [rcvd?.card && t('qso.detail.qsl.card'), rcvd?.lotw && 'LoTW', rcvd?.eqsl && 'eQSL', rcvd?.qrz && 'QRZ'].filter(Boolean).join(' · '),
    ),
    // The card sent, by which route and when (the box above sets the route; the date is the mark's).
    fact(t('qso.detail.qsl.sent'), sent?.sent ? [sent.via, sent.dateUnix ? fmtUtc(sent.dateUnix) : null].filter(Boolean).join(' · ') || yes : null),
    fact(t('qso.detail.qsl.confirmed'), q.confirmed ? yes : null),
    fact(t('qso.detail.qsl.awardConfirmed'), q.awardConfirmed ? yes : null),
    col('qslRcvdDate'),
    col('lotwRcvdDate'),
    col('eqslRcvdDate'),
    fact(t('qso.detail.credit.granted'), q.creditGranted),
    fact(t('qso.detail.credit.submitted'), q.creditSubmitted),
    // Where the station was, under the names the Logbook's columns give these ADIF fields (each is
    // also under More fields above, as written, to correct).
    col('county'),
    col('cqz'),
    col('ituz'),
    col('cont'),
    col('distance'),
    col('contest'),
    col('exchSent'),
    col('exchRcvd'),
    // The contest exchange's own ADIF columns: what every export, upload and award reads for those
    // fields, so shown here, read only (a change beside them is refused).
    ...(q.contest?.adif ?? []).map(([tag, value]) => fact(tag, value)),
    upload('upLotw'),
    upload('upEqsl'),
    upload('upQrz'),
    upload('upClublog'),
  ].filter((f): f is [string, string] => f !== null)

  const marks = (r: LoggedQso) =>
    [r.qslRcvd?.lotw && 'L', r.qslRcvd?.eqsl && 'E', r.qslRcvd?.qrz && 'Q', r.qslRcvd?.card && 'C'].filter(Boolean).join('')

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) void leave(onClose)
      }}
      title={t('contactCard.title', { call: q.call })}
      hideTitle
      className="contact-card-dialog"
      onCloseAutoFocus={returnFocus}
    >
      <form className="contact-card" onSubmit={onSave}>
        <div className="callcard-head contact-card-head">
          <CallbookPhoto call={q.call} book={book} />
          <div className="callcard-id">
            <div className="callcard-title">
              <span className="callcard-call mono">{q.call}</span>
              {name && <span className="callcard-name">{name}</span>}
            </div>
            {where && <div className="callcard-where">{where}</div>}
            <div className="callcard-actions">
              {!book && (
                <button type="button" className="callcard-lookup" onClick={lookUp} disabled={looking} title={t('logEntry.lookup.title')}>
                  {looking ? t('callCard.looking') : t('logEntry.lookup.label')}
                </button>
              )}
              <button type="button" className="callcard-qrz" onClick={qrzPageOf(q.call)} title={t('callbook.qrzPage.title', { call: q.call })}>
                {QRZ_PAGE}
              </button>
            </div>
          </div>
        </div>

        <section className="qso-detail-section">
          <h3 className="qso-detail-heading">{t('qso.detail.section.contact.radio')}</h3>
          <div className="logbook-form-grid">
            {box('call', t('logbook.field.call.label'))}
            {box('date', t('logbook.field.date.label'), { placeholder: UTC_DATE_FORMAT, maxLength: 10, className: 'settings-input mono' })}
            {box('timeOn', t('logbook.field.time.label'), { placeholder: UTC_TIME_FORMATS.short, maxLength: 8, className: 'settings-input mono', title: q.timeKnown === false ? t('contactCard.dateOnly') : undefined })}
            {box('timeOff', t('logbook.field.end.label'), { placeholder: UTC_TIME_FORMATS.short, maxLength: 8, className: 'settings-input mono' })}
            {box('band', t('logbook.field.band.label'))}
            {box('freq', t('logbook.field.freq.label'), { inputMode: 'decimal', className: 'settings-input mono' })}
            {box('mode', t('logbook.field.mode.label'))}
            {box('rstSent', t('logbook.field.rstSent.label'))}
            {box('rstRcvd', t('logbook.field.rstRcvd.label'))}
            {box('txPower', t('logbook.field.txPower.label'), { inputMode: 'decimal' })}
          </div>
        </section>

        <section className="qso-detail-section">
          <h3 className="qso-detail-heading">{t('qso.detail.section.contact')}</h3>
          <div className="logbook-form-grid">
            {box('name', t('logbook.field.name.label'))}
            {box('qth', t('logbook.field.qth.label'))}
            {box('grid', t('logbook.field.grid.label'))}
            {box('state', t('logbook.field.state.label'), { maxLength: 2 })}
            {program('theirProgram', t('contactCard.program.theirs'))}
            {box('theirRef', t('logbook.field.parkTheirs.label'))}
            {box('iota', t('logbook.field.iota.label'), { maxLength: 6 })}
            {(draft.iota.trim() !== '' || spotHeld) && (
              <div className="logbook-field logbook-field-wide iota-card-note">
                <IotaReferenceNote reference={draft.iota.trim().toUpperCase()} />
                {spotHeld && (
                  <span className="iota-card-held">
                    <span className="settings-hint">{t('iota.card.spotHeld')}</span>
                    <button type="button" className="settings-test-btn" onClick={() => void confirmIota()} disabled={!canSave || busy || dirty}>
                      {t('iota.card.confirm')}
                    </button>
                  </span>
                )}
              </div>
            )}
            {program('myProgram', t('contactCard.program.mine'))}
            {box('myRef', t('logbook.field.parkMine.label'))}
            {box('myGrid', t('logbook.field.myGrid.label'))}
            {box('myRig', t('logbook.field.myRig.label'))}
            <div className="logbook-field">
              <label htmlFor={`${id}-qsl`}>{t('logbook.field.qslSent.label')}</label>
              <select id={`${id}-qsl`} className="settings-input" value={draft.qslSentVia} onChange={(e) => set('qslSentVia', e.target.value)} disabled={!canSave}>
                <option value="">{t('logbook.field.qslSent.none')}</option>
                {draft.qslSentVia === 'SENT' && <option value="SENT">{t('logbook.field.qslSent.sent')}</option>}
                <option value="B">{t('logbook.qsl.via.bureau')}</option>
                <option value="D">{t('logbook.qsl.via.direct')}</option>
                <option value="E">{t('logbook.qsl.via.electronic')}</option>
              </select>
            </div>
            <label className="logbook-field">
              <span>{t('logbook.field.qslCard.label')}</span>
              <input type="checkbox" checked={draft.qslCard === '1'} onChange={(e) => set('qslCard', e.target.checked ? '1' : '')} disabled={!canSave} />
            </label>
            {/* The shared comment: full width, directly above Notes. It wraps over lines but holds
                ONE, as ADIF's COMMENT does — the log strip's and the edit form's rule. */}
            <label className="logbook-field logbook-field-wide">
              <span>{t('logbook.field.comment.label')}</span>
              <textarea
                className="settings-input logbook-comment"
                value={draft.comment}
                onChange={(e) => set('comment', oneLineTyped(e.target))}
                onKeyDown={(e) => {
                  if (e.key !== 'Enter') return
                  e.preventDefault()
                  e.currentTarget.form?.requestSubmit()
                }}
                rows={2}
                readOnly={!canSave}
              />
            </label>
            <label className="logbook-field logbook-field-wide">
              <span>{t('logbook.field.notes.label')}</span>
              <textarea className="settings-input logbook-notes" value={draft.notes} onChange={(e) => set('notes', e.target.value)} rows={3} readOnly={!canSave} />
            </label>
          </div>
        </section>

        <details className="qso-detail-section contact-card-more">
          <summary className="qso-detail-heading">{t('contactCard.more')}</summary>
          <p className="qso-detail-bare">{t('contactCard.more.hint')}</p>
          <div className="contact-card-fields">
            {fields.map((f, i) => {
              const own = ownField(f.tag) && f.orig !== null
              return (
                <div key={i} className={`contact-card-field${f.removed ? ' removed' : ''}`}>
                  {f.orig === null ? (
                    <input
                      className="settings-input mono"
                      value={f.tag}
                      onChange={(e) => setField(i, { tag: e.target.value.toUpperCase() })}
                      placeholder={t('contactCard.field.tag')}
                      aria-label={t('contactCard.field.tag')}
                      spellCheck={false}
                      autoComplete="off"
                    />
                  ) : (
                    <span className="mono contact-card-tag">{f.tag}</span>
                  )}
                  {multiLine(f) ? (
                    // A field that holds lines (an address, a QSL message): edited with its lines.
                    <textarea
                      className="settings-input mono"
                      value={f.value}
                      onChange={(e) => setField(i, { value: typedLines(f, e.target.value) })}
                      readOnly={own || f.removed || !canSave}
                      aria-label={f.tag || t('contactCard.field.value')}
                      spellCheck={false}
                      rows={Math.min(6, Math.max(2, f.value.split('\n').length))}
                    />
                  ) : (
                    <input
                      className="settings-input mono"
                      value={f.value}
                      onChange={(e) => setField(i, { value: e.target.value })}
                      readOnly={own || f.removed || !canSave}
                      aria-label={f.tag || t('contactCard.field.value')}
                      spellCheck={false}
                      autoComplete="off"
                    />
                  )}
                  {!own && canSave && (
                    <button
                      type="button"
                      className="log-rowbtn"
                      onClick={() =>
                        f.orig === null
                          ? setFields((fs) => fs.filter((_, j) => j !== i))
                          : setField(i, { removed: !f.removed })
                      }
                      title={f.removed ? t('contactCard.field.keep', { tag: f.tag }) : t('contactCard.field.remove', { tag: f.tag })}
                      aria-label={f.removed ? t('contactCard.field.keep', { tag: f.tag }) : t('contactCard.field.remove', { tag: f.tag })}
                    >
                      {f.removed ? '↺' : '✕'}
                    </button>
                  )}
                </div>
              )
            })}
          </div>
          {canSave && (
            <button
              type="button"
              className="export-btn"
              onClick={() => setFields((fs) => [...fs, { tag: '', orig: null, value: '', removed: false }])}
            >
              {t('contactCard.field.add')}
            </button>
          )}
          {facts.length > 0 && (
            <dl className="qso-detail-fields contact-card-facts">
              {facts.map(([label, value], i) => (
                <div key={`${i}-${label}`} className="qso-detail-field">
                  <dt>{label}</dt>
                  <dd className="mono">{value}</dd>
                </div>
              ))}
            </dl>
          )}
        </details>

        <section className="qso-detail-section">
          <h3 className="qso-detail-heading">{t('contactCard.recent')}</h3>
          <table className="contact-card-recent mono">
            <thead>
              <tr>
                <th>{t('logbook.column.time')}</th>
                <th>{t('logbook.column.band')}</th>
                <th>{t('logbook.column.freq')}</th>
                <th>{t('logbook.column.mode')}</th>
                <th>{t('logbook.column.sent')}</th>
                <th>{t('logbook.column.rcvd')}</th>
                <th>{t('contactCard.recent.qsl')}</th>
              </tr>
            </thead>
            <tbody>
              {recent.map((r) => {
                const here = !!r.id && r.id === q.id
                return (
                  <tr
                    key={r.id ?? `${r.whenUnix}`}
                    className={here ? 'current' : undefined}
                    aria-current={here || undefined}
                    tabIndex={here || !r.id ? -1 : 0}
                    title={here ? undefined : t('contactCard.recent.open')}
                    onClick={() => !here && r.id && void leave(() => onSwitch(r.id as string))}
                    onKeyDown={(e) => {
                      if (e.key !== 'Enter' || here || !r.id) return
                      e.preventDefault()
                      void leave(() => onSwitch(r.id as string))
                    }}
                  >
                    <td>{fmtUtc(r.whenUnix)}</td>
                    <td>{r.band}</td>
                    <td>{r.freqMhz.toFixed(3)}</td>
                    <td>{r.mode}</td>
                    <td>{r.rstSent ?? ''}</td>
                    <td>{r.rstRcvd ?? ''}</td>
                    <td>{marks(r)}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </section>

        {/* An Undo here changes the contact the card shows: the card reads it again. */}
        {q.id && (
          <QsoHistory
            id={q.id}
            onChanged={() => {
              onChanged()
              if (q.id) onReload(q.id)
            }}
          />
        )}

        <div className="contact-card-actions">
          {err && (
            <span className="settings-error" role="alert">
              {err}
            </span>
          )}
          {!canSave && <span className="logbook-editing-note">{t('contactCard.readOnly')}</span>}
          {/* Your own station's fields are saved, and LoTW keeps the contact it was sent: say so. */}
          {canSave && notResent.length > 0 && (
            <span className="logbook-editing-note contact-card-lotw-note" role="status">
              {t('contactCard.lotwNotResent', { fields: notResent.join(', ') })}
            </span>
          )}
          <button type="button" className="logconfirm-discard" onClick={() => void leave(onClose)}>
            {t('contactCard.cancel')}
          </button>
          <button type="submit" className="settings-save" disabled={!canSave || !dirty || busy}>
            {t('logbook.form.save')}
          </button>
        </div>
      </form>
    </Dialog>
  )
}
