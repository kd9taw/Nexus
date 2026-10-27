// THE INBOX, as JS8Call 3.0.3's message panel (MessagePanel.cpp): the messages to me and the ones I
// hold for others, for every station or one ("Messages: All" or a call; the call list's "Show message
// inbox" picks one), sorted by a column (newest first to begin with), several picked at once and
// deleted together, read the moment one is picked, and Reply, which puts "<path> MSG [MESSAGE]" in the
// compose box for the operator to finish and send.
//
// Nothing here keys: Reply fills the compose box and sends nothing; read and delete are inbox edits.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): calls, times and message text are data;
// every word is in the catalog.
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { t } from '../i18n'
import type { Js8InboxEntry, Js8InboxState, Js8InboxTooLong } from '../types'
import { utcClock } from '../js8Vocab'
import { Js8InboxTooLongNote } from './Js8InboxTooLong'
import {
  JS8_INBOX_DEFAULT_SORT,
  JS8_MARK_GLYPH,
  inboxFromLabel,
  js8InboxFor,
  sortInbox,
  type Js8InboxSort,
  type Js8InboxSortKey,
} from '../features/js8Lists'

/** Literal keys per state, so the orphan guard sees each of them referenced. */
export function inboxStateLabel(s: Js8InboxState): string {
  switch (s) {
    case 'unread':
      return t('js8.inbox.state.unread')
    case 'read':
      return t('js8.inbox.state.read')
    case 'store':
      return t('js8.inbox.state.store')
    case 'delivered':
      return t('js8.inbox.state.delivered')
  }
}

/** The calls the filter offers: each sender of my messages and each station I hold one for. */
function filterCalls(inbox: readonly Js8InboxEntry[]): string[] {
  const calls = inbox.map((e) => (e.state === 'store' || e.state === 'delivered' ? e.to : e.from).trim().toUpperCase())
  return [...new Set(calls.filter(Boolean))].sort()
}

export function Js8InboxPanel({
  inbox,
  call,
  onCall,
  onRead,
  onDelete,
  onReply,
  tooLong,
  notes,
}: {
  inbox: readonly Js8InboxEntry[]
  /** The station shown, or null for all of them. */
  call: string | null
  onCall: (call: string | null) => void
  /** Mark a message read (picking an unread one). */
  onRead: (id: number) => void
  /** Delete these messages. */
  onDelete: (ids: number[]) => void
  /** Reply to this message (the compose box takes JS8Call's reply text). */
  onReply: (entry: Js8InboxEntry) => void
  /** `Js8State.inboxTooLong`: the held messages too long to hand over at this speed, each noted on
   *  its own row (Js8InboxTooLong). */
  tooLong?: Js8InboxTooLong[]
  /** Shown above the rows: the notes of what a cap took out (Js8InboxDropped). */
  notes?: ReactNode
}) {
  const [sort, setSort] = useState<Js8InboxSort>(JS8_INBOX_DEFAULT_SORT)
  const [picked, setPicked] = useState<number[]>([])
  const [current, setCurrent] = useState<number | null>(null)
  const shown = useMemo(() => sortInbox(js8InboxFor(inbox, call), sort), [inbox, call, sort])
  // A message deleted, or filtered out, is no longer picked.
  const ids = shown.map((e) => e.id).join(' ')
  useEffect(() => {
    const live = new Set(ids.split(' ').filter(Boolean).map(Number))
    setPicked((p) => (p.every((id) => live.has(id)) ? p : p.filter((id) => live.has(id))))
    setCurrent((c) => (c != null && !live.has(c) ? null : c))
  }, [ids])
  const calls = useMemo(() => {
    const all = filterCalls(inbox)
    return call && !all.includes(call) ? [...all, call] : all
  }, [inbox, call])
  const currentEntry = shown.find((e) => e.id === current) ?? null

  /** Pick one message, as a click on a row picks it in JS8Call: it is read the moment it is picked. */
  const pick = (e: Js8InboxEntry) => {
    setCurrent(e.id)
    setPicked([e.id])
    if (e.state === 'unread') onRead(e.id)
  }
  const toggle = (e: Js8InboxEntry, on: boolean) => {
    setPicked((p) => (on ? [...p.filter((id) => id !== e.id), e.id] : p.filter((id) => id !== e.id)))
    if (on) {
      setCurrent(e.id)
      if (e.state === 'unread') onRead(e.id)
    }
  }
  const remove = () => {
    if (picked.length === 0) return
    onDelete(picked)
    setPicked([])
  }
  const sortBy = (key: Js8InboxSortKey) =>
    setSort((s) => (s.key === key ? { key, descending: !s.descending } : { key, descending: key === 'time' || key === 'id' }))
  const ariaSort = (key: Js8InboxSortKey) => (sort.key === key ? (sort.descending ? 'descending' : 'ascending') : 'none')
  const head = (key: Js8InboxSortKey, label: string) => (
    <th scope="col" aria-sort={ariaSort(key)}>
      <button type="button" className="js8-sort" onClick={() => sortBy(key)} title={t('js8.inbox.sort.title', { column: label })}>
        {label}
        {sort.key === key && <span aria-hidden="true">{sort.descending ? ' ▾' : ' ▴'}</span>}
      </button>
    </th>
  )

  return (
    <div className="js8-inbox">
      <div className="js8-inbox-bar" role="toolbar" aria-label={t('js8.inbox.toolbar.aria')}>
        <label className="js8-inbox-filter">
          <span>{t('js8.inbox.filter.label')}</span>
          <select value={call ?? ''} onChange={(e) => onCall(e.target.value || null)}>
            <option value="">{t('js8.inbox.filter.all')}</option>
            {calls.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="cw-macro js8-inbox-act js8-inbox-reply"
          disabled={currentEntry == null}
          onClick={() => currentEntry && onReply(currentEntry)}
          title={t('js8.inbox.reply.title')}
        >
          {t('js8.inbox.reply.label')}
        </button>
        <button
          type="button"
          className="cw-macro js8-inbox-act js8-inbox-delete"
          disabled={picked.length === 0}
          onClick={remove}
          title={t('js8.inbox.deleteSelected.title')}
        >
          {t('js8.inbox.deleteSelected.label', { count: Math.max(1, picked.length) })}
        </button>
      </div>
      {notes}
      {shown.length === 0 ? (
        <div className="cw-decode-idle">{t('js8.inbox.empty')}</div>
      ) : (
        <table
          className="js8-table js8-inbox-table"
          onKeyDown={(e) => {
            // JS8Call's Delete shortcut on the table (MessagePanel.cpp:45-50).
            if (e.key === 'Delete') {
              e.preventDefault()
              remove()
            }
          }}
        >
          <thead>
            <tr>
              <th scope="col" aria-label={t('js8.inbox.col.pick')} />
              <th scope="col" aria-label={t('js8.inbox.col.unread')} />
              {head('id', t('js8.inbox.col.id'))}
              {head('time', t('js8.inbox.col.time'))}
              {head('from', t('js8.inbox.col.from'))}
              {head('to', t('js8.inbox.col.to'))}
              <th scope="col">{t('js8.inbox.col.text')}</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((m) => (
              <tr
                key={m.id}
                className={`js8-inbox-row state-${m.state}${m.id === current ? ' selected' : ''}`}
                onClick={() => pick(m)}
                aria-selected={picked.includes(m.id)}
              >
                <td className="js8-cell" onClick={(e) => e.stopPropagation()}>
                  <input
                    type="checkbox"
                    checked={picked.includes(m.id)}
                    onChange={(e) => toggle(m, e.target.checked)}
                    aria-label={t('js8.inbox.pick.aria', { from: m.from, time: utcClock(m.atMs) })}
                  />
                </td>
                <td className="js8-cell js8-mark" title={inboxStateLabel(m.state)}>
                  {m.state === 'unread' ? JS8_MARK_GLYPH.message : ''}
                </td>
                <td className="js8-cell">{m.id}</td>
                <td className="js8-cell js8-time" title={new Date(m.atMs).toISOString().slice(0, 10)}>
                  {utcClock(m.atMs)}
                </td>
                <td className="js8-cell js8-call">{inboxFromLabel(m)}</td>
                <td className="js8-cell js8-call">{m.to}</td>
                <td className="js8-cell js8-text">
                  {m.text}
                  {(m.state === 'store' || m.state === 'delivered') && <span className="js8-chip">{inboxStateLabel(m.state)}</span>}
                  <Js8InboxTooLongNote id={m.id} tooLong={tooLong} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
