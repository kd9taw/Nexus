// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the prefixes, grid squares and entity names the
// placeholders offer as EXAMPLES — they are technical tokens, so they live below as
// WATCH_EXAMPLES (the rule is in `i18n/index.ts`) — and `DXCC`, a programme's name.
import { useEffect, useState } from 'react'
import {
  loadWatchlist,
  saveWatchlist,
  newWatchFilter,
  WATCHLIST_CHANGED,
  type WatchFilter,
  type WatchKind,
} from '../watchlist'
import { t } from '../i18n'
import { T } from '../i18n/T'

/** Example values, every one a token: two callsign wildcards, two grid wildcards, and a
 * DXCC entity name. A locale may swap the entity for one its operators chase; the wildcards
 * are syntax and never change. */
const WATCH_EXAMPLES = {
  callPrefix: 'VP8*',
  call: '3Y0J',
  grid: 'FN31',
  gridPrefix: 'EM7*',
  entity: 'Bouvet',
}

/** The award programme's name — three letters in every language. */
const DXCC_PROGRAM = 'DXCC'

/**
 * #390 — one entry's note, edited in place on its row. Kept as a draft while the operator types
 * and saved when the field is left, or on Enter — not on every keystroke, because every save is
 * announced and App answers each announcement by sending the station the list and reading the
 * Needed board again. A field left unchanged saves nothing; an emptied one removes the note.
 */
function EntryNotes({ entry, onSave }: { entry: WatchFilter; onSave: (notes: string) => void }) {
  const stored = entry.notes ?? ''
  const [draft, setDraft] = useState(stored)
  // The stored note changed under the field (a save of this one, trimmed, or another writer):
  // show what is stored now.
  useEffect(() => setDraft(stored), [stored])
  const save = () => {
    const next = draft.trim()
    if (next !== stored) onSave(next)
  }
  return (
    <input
      className="watchlist-notes"
      value={draft}
      placeholder={t('watchlist.item.notes.placeholder')}
      title={t('watchlist.notes.title')}
      aria-label={t('watchlist.item.notes.aria', { value: entry.value })}
      autoComplete="off"
      onChange={(e) => setDraft(e.target.value)}
      onBlur={save}
      onKeyDown={(e) => {
        if (e.key === 'Enter') save()
      }}
    />
  )
}

/**
 * Manage the user watch list — "alert me loudly when THIS shows up." Self-contained: it
 * persists to localStorage and dispatches `nexus:watchlist-changed` so the live decode
 * alerter (App) re-syncs immediately. Generalizes the DXpedition chase-star to any
 * operator-defined target (a call/prefix, a whole DXCC entity, or a grid square). Each entry
 * can carry the operator's own note (#390): why it is there, and when it can come off.
 */
export function WatchlistPanel() {
  const [list, setList] = useState<WatchFilter[]>(() => loadWatchlist())
  // The list as it is NOW, not as it was when this opened: it has another writer (the one-time
  // fold of the retired wanted list at startup), and an edit here saves this copy whole — a stale
  // one would delete whatever that writer added.
  useEffect(() => {
    const resync = () => setList(loadWatchlist())
    window.addEventListener(WATCHLIST_CHANGED, resync)
    return () => window.removeEventListener(WATCHLIST_CHANGED, resync)
  }, [])
  const [kind, setKind] = useState<WatchKind>('call')
  const [value, setValue] = useState('')
  const [cqOnly, setCqOnly] = useState(false)
  const [notes, setNotes] = useState('')

  const commit = (next: WatchFilter[]) => {
    setList(next)
    saveWatchlist(next)
    window.dispatchEvent(new Event(WATCHLIST_CHANGED))
  }
  const add = () => {
    const v = value.trim()
    if (!v) return
    const note = notes.trim()
    commit([...list, newWatchFilter(kind, v, { ...(cqOnly ? { cqOnly: true } : {}), ...(note ? { notes: note } : {}) })])
    setValue('')
    setCqOnly(false)
    setNotes('')
  }
  const remove = (id: string) => commit(list.filter((f) => f.id !== id))
  // One entry's note set, or emptied and so removed. Every other field of every entry is kept.
  const setEntryNotes = (id: string, note: string) =>
    commit(
      list.map((f) => {
        if (f.id !== id) return f
        const next = { ...f }
        if (note) next.notes = note
        else delete next.notes
        return next
      }),
    )

  return (
    <div className="watchlist">
      <div className="watchlist-hint">
        <T k="watchlist.hint" tags={{ code: <code /> }} />
      </div>
      {list.length > 0 && (
        <ul className="watchlist-items">
          {list.map((f) => (
            <li key={f.id} className="watchlist-item">
              <span className={`watchlist-kind watchlist-kind-${f.kind}`}>
                {f.kind === 'call'
                  ? t('watchlist.item.kind.call')
                  : f.kind === 'grid'
                    ? t('watchlist.item.kind.grid')
                    : DXCC_PROGRAM}
              </span>
              <span className="watchlist-value">
                {f.kind === 'dxcc' ? f.value : f.value.toUpperCase()}
              </span>
              {f.cqOnly && <span className="watchlist-flag">{t('watchlist.item.cqOnly')}</span>}
              <EntryNotes entry={f} onSave={(note) => setEntryNotes(f.id, note)} />
              <button
                type="button"
                className="watchlist-remove"
                onClick={() => remove(f.id)}
                title={t('watchlist.item.remove.title')}
                aria-label={t('watchlist.item.remove.aria', { value: f.value })}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="watchlist-add">
        <select
          className="settings-input watchlist-kind-select"
          value={kind}
          onChange={(e) => setKind(e.target.value as WatchKind)}
          aria-label={t('watchlist.add.kind.aria')}
        >
          <option value="call">{t('watchlist.add.kind.call')}</option>
          <option value="dxcc">{t('watchlist.add.kind.dxcc')}</option>
          <option value="grid">{t('watchlist.add.kind.grid')}</option>
        </select>
        <input
          className="settings-input"
          value={value}
          placeholder={
            kind === 'call'
              ? t('watchlist.add.value.placeholder.call', {
                  first: WATCH_EXAMPLES.callPrefix,
                  second: WATCH_EXAMPLES.call,
                })
              : kind === 'grid'
                ? t('watchlist.add.value.placeholder.grid', {
                    first: WATCH_EXAMPLES.grid,
                    second: WATCH_EXAMPLES.gridPrefix,
                  })
                : t('watchlist.add.value.placeholder.dxcc', { entity: WATCH_EXAMPLES.entity })
          }
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add()
          }}
          autoComplete="off"
          aria-label={t('watchlist.add.value.aria')}
        />
        <label className="watchlist-cqonly" title={t('watchlist.add.cqOnly.title')}>
          <input type="checkbox" checked={cqOnly} onChange={(e) => setCqOnly(e.target.checked)} />{' '}
          {t('watchlist.add.cqOnly.label')}
        </label>
        <input
          className="settings-input watchlist-notes-new"
          value={notes}
          placeholder={t('watchlist.add.notes.placeholder')}
          title={t('watchlist.notes.title')}
          onChange={(e) => setNotes(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add()
          }}
          autoComplete="off"
          aria-label={t('watchlist.add.notes.aria')}
        />
        <button type="button" className="watchlist-add-btn" onClick={add} disabled={!value.trim()}>
          {t('watchlist.add.submit')}
        </button>
      </div>
    </div>
  )
}
