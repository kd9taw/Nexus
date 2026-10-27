// The Inbox's notes of what a cap took out. JS8Call keeps every message until the operator deletes
// it; Nexus keeps the same, inside two caps (100 messages and 64 KB for the operator's own mail,
// the same again for mail held for other stations), and when a cap takes one out the Inbox says
// which message went and why, here, above the rows, until the operator clears the notes. The
// station keeps the last ten and journals them (`Station::inbox_dropped`).
import type { Js8InboxDrop, Js8InboxState } from '../types'
import { t } from '../i18n'
import { utcClock } from '../js8Vocab'

/** The row's state when it went, as the Inbox labels it. One literal key per state, so the
 *  catalog guards can read every call site. */
function stateLabel(s: Js8InboxState): string {
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

/** Why it went: which cap, and whose mail (the operator's, or mail held for another station). */
function reason(d: Js8InboxDrop): string {
  const mine = d.entry.state === 'unread' || d.entry.state === 'read'
  if (d.why === 'count') return mine ? t('js8.inbox.dropped.count.mine') : t('js8.inbox.dropped.count.held')
  return mine ? t('js8.inbox.dropped.bytes.mine') : t('js8.inbox.dropped.bytes.held')
}

export function Js8InboxDropped({
  drops,
  onClear,
}: {
  /** `Js8State.inboxDropped`: absent while there are none. */
  drops: Js8InboxDrop[] | undefined
  onClear: () => void
}) {
  if (!drops || drops.length === 0) return null
  return (
    <div className="js8-inbox-dropped" role="status">
      {drops.map((d) => (
        <div key={`${d.entry.id}-${d.atMs}`} className="js8-inbox-row state-dropped">
          <span className="js8-chip">{t('js8.inbox.dropped.chip')}</span>
          <span className="js8-cell js8-time">{utcClock(d.atMs)}</span>
          <span className="js8-chip">{stateLabel(d.entry.state)}</span>
          <span className="js8-cell js8-call">
            {d.entry.from} → {d.entry.to}
          </span>
          <span className="js8-cell js8-text">{d.entry.text}</span>
          <span className="js8-cell js8-path">{reason(d)}</span>
        </div>
      ))}
      <button
        type="button"
        className="cw-macro js8-inbox-act"
        onClick={onClear}
        title={t('js8.inbox.dropped.clear.title')}
      >
        {t('js8.inbox.dropped.clear.label')}
      </button>
    </div>
  )
}
