// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The word and the tooltip
// come from the catalog; the entry the tooltip names (a call, prefix, entity or grid, or the
// operator's own label for it) is data.
//
// THE WATCH TILE — one element for the three lists that mark a station on the operator's watch
// list (the Call Roster, the Stations list, Spots), so a watched station looks the same on all of
// them: the need-chip pill in a colour of its own (`--need-watch`, lime, which no need tier and no
// row state uses in either theme), and a tooltip naming the entry that matched.
//
// Each host puts it FIRST among its chips. The roster's Need cell clips what does not fit and a
// spot's comment cell ellipsizes, so first is the one place the tile can never be cut off.
import { t } from '../i18n'
import { watchLabel, type WatchFilter } from '../watchlist'

export function WatchTile({ entry }: { entry: WatchFilter }) {
  return (
    <span className="need-chip need-watch" title={t('watchlist.tile.title', { what: watchLabel(entry) })}>
      {t('watchlist.tile.label')}
    </span>
  )
}
