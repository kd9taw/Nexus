// THE SHARED LIST — every pane an area may show besides a cockpit's own (the operator's pick of
// 2026-10-07: any pane in any area). Pure, like connectConfig.ts, so every rule unit-tests without React.
//
// ONE SOURCE FOR WHAT CAN BE PICKED. A box shows one entry of this list and nothing else, and the list
// is the Conditions boxes (connectConfig PANE_IDS), each exactly once. Whatever offers an entry,
// records which one a box shows or draws its body reads it from here; the body itself is drawn by the
// one renderer every Conditions box uses (components/connect/PaneFrame `PaneBody`), so a box reads the
// same wherever it stands.
//
// WHAT AN ENTRY CARRIES
//   · `id` — its name beside a cockpit's own panes: what a box records that it shows. It is its
//     Conditions id, except where a cockpit already uses that id for a pane of its own: Band Scope
//     (`scope` is every cockpit's spectrum strip), Activity Matrix (`activity` is JS8's decode window),
//     and Spots and Needed (`spots` and `needed` are Phone's and CW's own Spots and Needed panes). A box
//     recording one of those would read as that cockpit's pane, so those four have names of their own.
//   · `pane` — the Conditions box it is. Its title, its body, its one-line state and its group in a
//     picker (`category`) are the registry's (components/connect/panes), so there is one grouping.
//   · `role` — how it shares a column, the cockpit pane frame's two roles: `content` is exactly its own
//     height (a reading, a gauge, a chart, a table of fixed rows), and `fill` shares the column's
//     surplus by `weight` (a list or a board that grows with what it holds and scrolls in its box).
//     Set from what each box draws (2026-10-07).
//   · `remote` — whether the hosted Remote page has everything the box draws: its copies of the
//     Conditions feeds (connect/usePaneContext `remoteFeeds`), the station's collections
//     (remote-web/collections), the streamed snapshot and spectrum, or the clock. False for a box that
//     asks the station for something none of those carry (the Kp outlook, the openings log, the
//     satellite passes, the contest calendar) and for POTA/SOTA, whose hunt wiring a browser does not
//     get. Read off the code on 2026-10-07; a box offered on the Remote page is checked there first.
//     Selection and Chase Feed lack only the expedition windows there, and Space Wx its trend lines.
//
// THE STOP LINE, BY CONSTRUCTION. Every entry is a Conditions box, and no Conditions box hosts a
// control that stops a transmission or one that starts it: ▶ Work and HUNT move the rig and open a
// cockpit, and key nothing (connect/panes). A cockpit's own senders — FT's decode panes, its rosters
// and its Tx messages, Phone's voice keyer — are not Conditions boxes, so no box can show one; they
// move only within their own cockpit (the operator, 2026-10-07: "No, only within their cockpit").
// sharedPanes.test.ts holds the rest: no id is named for a stop control, none is any cockpit's own id,
// and the list is not shaped like a panel vocabulary — it says what a box may SHOW, never what a
// cockpit may hide.
import type { PaneId } from './connectConfig'

interface SharedPaneBase {
  readonly id: string
  readonly pane: PaneId
  readonly remote: boolean
}

/** One pane a box may show (see the header). */
export type SharedPane =
  | (SharedPaneBase & { readonly role: 'content' })
  | (SharedPaneBase & { readonly role: 'fill'; readonly weight: number })

/** Every Conditions box once, in the registry's id order. */
export const SHARED_PANES: readonly SharedPane[] = [
  { id: 'advisory', pane: 'advisory', role: 'content', remote: true },
  { id: 'bandAdvisor', pane: 'bandAdvisor', role: 'content', remote: true },
  { id: 'selection', pane: 'selection', role: 'content', remote: true },
  { id: 'outlook', pane: 'outlook', role: 'content', remote: true },
  { id: 'openings', pane: 'openings', role: 'content', remote: true },
  { id: 'openingsLog', pane: 'openingsLog', role: 'fill', weight: 1, remote: false },
  { id: 'spacewx', pane: 'spacewx', role: 'content', remote: true },
  // Every receiver that hears you, most distant first: a list under the compass.
  { id: 'getout', pane: 'getout', role: 'fill', weight: 1, remote: true },
  { id: 'bestband', pane: 'bestband', role: 'content', remote: true },
  { id: 'activityMatrix', pane: 'activity', role: 'content', remote: true },
  { id: 'beacons', pane: 'beacons', role: 'content', remote: true },
  { id: 'insights', pane: 'insights', role: 'fill', weight: 1, remote: true },
  { id: 'chase', pane: 'chase', role: 'fill', weight: 1, remote: true },
  { id: 'greyline', pane: 'greyline', role: 'content', remote: true },
  { id: 'bandHours', pane: 'bandHours', role: 'content', remote: true },
  { id: 'esNowcast', pane: 'esNowcast', role: 'content', remote: true },
  // The ten freshest ionosondes at most: a short table of its own height.
  { id: 'measuredMuf', pane: 'measuredMuf', role: 'content', remote: true },
  { id: 'chaseFeed', pane: 'chaseFeed', role: 'fill', weight: 1, remote: true },
  { id: 'satPasses', pane: 'satPasses', role: 'fill', weight: 1, remote: false },
  { id: 'rotor', pane: 'rotor', role: 'content', remote: true },
  { id: 'contests', pane: 'contests', role: 'fill', weight: 1, remote: false },
  { id: 'bandScope', pane: 'scope', role: 'content', remote: true },
  { id: 'amp', pane: 'amp', role: 'content', remote: true },
  { id: 'kpOutlook', pane: 'kpOutlook', role: 'content', remote: false },
  { id: 'bandTiles', pane: 'bandTiles', role: 'content', remote: true },
  { id: 'clock', pane: 'clock', role: 'content', remote: true },
  { id: 'spotsBoard', pane: 'spots', role: 'fill', weight: 1, remote: true },
  { id: 'pota', pane: 'pota', role: 'fill', weight: 1, remote: false },
  // The hosted page has the station's needs (its `needs` collection), as Phone's own Needed pane shows.
  { id: 'neededBoard', pane: 'needed', role: 'fill', weight: 1, remote: true },
]

const BY_ID = new Map<string, SharedPane>(SHARED_PANES.map((e) => [e.id, e]))
const BY_PANE = new Map<PaneId, SharedPane>(SHARED_PANES.map((e) => [e.pane, e]))

/** The entry a box records (its `id`), or undefined for one the list does not have. */
export function sharedPaneById(id: string): SharedPane | undefined {
  return BY_ID.get(id)
}

/** The entry that is this Conditions box. */
export function sharedPaneOf(pane: PaneId): SharedPane | undefined {
  return BY_PANE.get(pane)
}
