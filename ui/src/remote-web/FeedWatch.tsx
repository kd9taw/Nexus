import { useSyncExternalStore } from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { t } from '../i18n'
import type { FeedControl } from './client'

/** The feed's own control: whether this tab keeps watching while it is in the background, and -
 *  once, on return - why the feed has a gap in it.
 *
 *  It has its own module because it belongs to BOTH remote surfaces. The full workspace shows it
 *  in the session row beside Listen; the observer-only browser shows it in the monitor header,
 *  and that browser is the one this control is most for - an operator who is only ever watching.
 *  Importing it from `SessionStatus` would have dragged the lazily-loaded workspace status bar
 *  into the observer's initial bundle to get it.
 *
 *  The label does not change with the state; the pressed state does, which is what a toggle is. A
 *  button whose word flips between "keep" and "pause" reads as an instruction and leaves nobody
 *  sure which one is current. Styles live in `remote.css`, which both surfaces load. */
export function FeedWatch({ feed }: { feed: FeedControl }) {
  const view = useSyncExternalStore(feed.subscribe, feed.getSnapshot)
  return <span className="remote-feed">
    <button type="button" className="remote-button remote-feed-toggle"
      aria-pressed={view.keepWatching} aria-label={t('remote.feed.keep')}
      title={view.keepWatching ? t('remote.feed.keepOn.title') : t('remote.feed.keepOff.title')}
      onClick={() => feed.keepWatching(!view.keepWatching)}>
      {view.keepWatching ? <Eye size={18} aria-hidden="true" /> : <EyeOff size={18} aria-hidden="true" />}
      <span>{t('remote.feed.keep')}</span>
    </button>
    {/* The gap is explained at the one moment the operator is there to read it: on the way back.
        It clears itself when the feed is actually live again, not when it was merely asked for. */}
    {view.resumed && <span className="remote-feed-state" role="status">{t('remote.feed.resumed')}</span>}
  </span>
}
