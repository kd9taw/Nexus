// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the prefixes, grid squares and entity names the
// placeholders offer as EXAMPLES — they are technical tokens, so they live below as
// WATCH_EXAMPLES (the rule is in `i18n/index.ts`) — and `DXCC`, a programme's name.
import { useState } from 'react'
import {
  loadWatchlist,
  saveWatchlist,
  newWatchFilter,
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
 * Manage the user watch list — "alert me loudly when THIS shows up." Self-contained: it
 * persists to localStorage and dispatches `nexus:watchlist-changed` so the live decode
 * alerter (App) re-syncs immediately. Generalizes the DXpedition chase-star to any
 * operator-defined target (a call/prefix, a whole DXCC entity, or a grid square).
 */
export function WatchlistPanel() {
  const [list, setList] = useState<WatchFilter[]>(() => loadWatchlist())
  const [kind, setKind] = useState<WatchKind>('call')
  const [value, setValue] = useState('')
  const [cqOnly, setCqOnly] = useState(false)

  const commit = (next: WatchFilter[]) => {
    setList(next)
    saveWatchlist(next)
    window.dispatchEvent(new Event('nexus:watchlist-changed'))
  }
  const add = () => {
    const v = value.trim()
    if (!v) return
    commit([...list, newWatchFilter(kind, v, cqOnly ? { cqOnly: true } : undefined)])
    setValue('')
    setCqOnly(false)
  }
  const remove = (id: string) => commit(list.filter((f) => f.id !== id))

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
        <button type="button" className="watchlist-add-btn" onClick={add} disabled={!value.trim()}>
          {t('watchlist.add.submit')}
        </button>
      </div>
    </div>
  )
}
