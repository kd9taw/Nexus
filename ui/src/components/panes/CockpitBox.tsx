// A BOX IN A COCKPIT (any pane in any area, 2026-10-07): one entry of the shared list
// (features/sharedPanes) standing in a cockpit's column, or on Phone's left side, wherever ⊞ Panels ▸
// Arrange put it. Which entry it shows, whether it is on screen and where it stands are the cockpit's
// panel record (features/panelState, THE BOXES); this draws one.
//
// THE FRAME IS THE COCKPIT'S (CockpitPaneFrame), as for every pane there, with the role the shared list
// gives the entry — a reading is its own height, a list fills by its weight — and never a size. The
// picker is a head control in the frame's `actions` slot, so the frame gains no picker of its own: it
// lists the whole shared list in the Conditions picker's groups and order, marks an entry already on
// screen elsewhere, and choosing one moves it here (the record's `setBox`). The ✕ hides the box; that
// hide ends nothing, so it carries no note. The picker is memoised: a cockpit renders on every snapshot,
// and six boxes' twenty-nine options are rebuilt only when what they show, mark or say changes.
//
// THE BODY is the Conditions box's own (BoxBody, mounted only while the box is). In the stacking flow a
// list box is capped and scrolls inside itself (`box-body--stacked`, styles.css), so a long list never
// pushes the log form a screen down; at the bounded tiers the pane body scrolls it.
//
// THE DASHBOARD RAIL'S BOXES stand here too on a window too small for the rail (`foldedRailBoxes`): the same
// frame and picker, acting on the rail's record.
//
// THE STOP LINE is not near any of this: a box shows only an entry of the shared list, none of which
// hosts a control that starts or stops a transmission, and the cockpit's own senders never stand in one.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { t } from '../../i18n'
import { useLocale } from '../../i18n/useLocale'
import { CockpitPaneFrame } from './CockpitPaneFrame'
import { BoxBody } from './BoxBody'
import { PANES, PANE_CATEGORIES, PANE_CATEGORY_LABEL, paneById } from '../connect/panes'
import { sharedPaneById, sharedPaneOf, type SharedPane } from '../../features/sharedPanes'
import { BOX_IDS, type BoxId } from '../../features/panelState'
import type { DashSlotId, RailBox } from '../../features/dashRail'
import { surfaceGet } from '../../features/windowScope'
import type { DashRailProps } from '../DashRail'
import type { MapIntent } from '../MapView'

/** THE DASHBOARD RAIL BESIDE THE COCKPIT, as its boxes see it (App computes it, where both records live: the
 *  operator's "Once per screen" across the cockpit and its rail). */
export interface RailLink {
  /** The shared-list entries the rail shows on this screen: no box shows one of them too. */
  shows: readonly string[]
  /** A box's picker chose one of them (`entry`): the rail's slot that showed it takes what the box showed
   *  (`give`) in exchange, as two boxes swap. */
  take: (entry: string, give: string | null) => void
  /** ON A WINDOW TOO SMALL FOR THE RAIL (the operator's "They move into the columns", 2026-10-07): the rail's
   *  boxes on screen, which the cockpit stands at the foot of the column its own boxes stand in until placed,
   *  until the window is wide enough again (`foldedRailBoxes`). The rail's record is never rewritten by a
   *  window's width. */
  folded?: {
    boxes: readonly RailBox[]
    /** A folded box's picker: that entry in that slot of the rail, moved there if it is on screen. */
    pick: (slot: DashSlotId, entry: string) => void
  }
}

/** What the window lends a cockpit's boxes: exactly what it lends the dashboard rail (App), and the rail
 *  beside the cockpit where it stands on screen. */
export type BoxSource = Pick<
  DashRailProps,
  | 'myGrid'
  | 'theme'
  | 'stations'
  | 'prop'
  | 'needByCall'
  | 'needAlerts'
  | 'amp'
  | 'rigBand'
  | 'onWorkSpot'
  | 'onPoint'
  | 'spotsFeed'
  | 'otaBoard'
  | 'neededBoard'
> & {
  /** The dashboard rail beside this cockpit, while it is on screen (App lends it the cockpit on screen only). */
  rail?: RailLink
}

/** A box's picker: `entry` in `box` (the record's `setBox`), and, where the rail beside the cockpit shows it,
 *  the rail's slot takes what the box showed (`was`) in exchange — once per screen across the two. */
export function pickForBox<P extends string>(
  setBox: ((box: P, entry: string) => void) | undefined,
  source: BoxSource | undefined,
  box: P,
  entry: string,
  was: string | undefined,
): void {
  setBox?.(box, entry)
  if (source?.rail?.shows.includes(entry)) source.rail.take(entry, was ?? null)
}

/** A cockpit's box selection, the box highlight a band tile's click sets, and Connect's goal. */
export interface BoxSelection {
  selectedCall: string | null
  onSelectCall: (call: string | null) => void
  focusBand: string | null
  toggleFocusBand: (band: string) => void
  intent: MapIntent
}

const INTENTS: readonly MapIntent[] = ['dx', 'pota', 'casual', 'vhf']

/** The cockpit's own box selection, shared by its boxes and never the window's (BoxBody). */
export function useBoxSelection(): BoxSelection {
  const [selectedCall, setSelectedCall] = useState<string | null>(null)
  const [focusBand, setFocusBand] = useState<string | null>(null)
  const toggleFocusBand = useCallback((band: string) => setFocusBand((f) => (f === band ? null : band)), [])
  // Connect's goal chips live on Connect; a box reads the one this window last picked there, as the
  // dashboard rail does.
  const [intent] = useState<MapIntent>(() => {
    const v = surfaceGet('nexus.connect.intent')
    return v != null && (INTENTS as readonly string[]).includes(v) ? (v as MapIntent) : 'dx'
  })
  return useMemo(
    () => ({ selectedCall, onSelectCall: setSelectedCall, focusBand, toggleFocusBand, intent }),
    [selectedCall, focusBand, toggleFocusBand, intent],
  )
}

/** An entry's name: its Conditions box's. */
function entryTitle(entry: SharedPane): string {
  return paneById(entry.pane)?.title ?? ''
}

/** The boxes' names for ⊞ Arrange: what each box on screen shows, else plainly "Box". */
export function boxLabels(entries: Partial<Record<BoxId, string>>): Record<BoxId, string> {
  const out = {} as Record<BoxId, string>
  for (const b of BOX_IDS) {
    const e = entries[b] != null ? sharedPaneById(entries[b]!) : undefined
    out[b] = e ? entryTitle(e) : t('panels.box.name')
  }
  return out
}

export interface CockpitBoxProps {
  /** The box (its vocabulary id): the frame's `data-pane`. */
  box: string
  /** The entry of the shared list it shows. */
  entry: string
  source: BoxSource
  selection: BoxSelection
  /** Whether an entry is on screen elsewhere: in another box, or as the cockpit's own pane. */
  onScreen: (entry: string) => boolean
  /** The picker's choice (the record's `setBox`). */
  onPick: (entry: string) => void
  /** Hide this box. */
  onRemove?: () => void
  /** The region stacks (its flow is 'stack'), where a list box is capped (see the header). */
  stacked: boolean
}

/** Every entry in the Conditions picker's groups and order. */
const PICKER_GROUPS = PANE_CATEGORIES.map((cat) => ({
  cat,
  items: PANES.filter((p) => p.category === cat).flatMap((p) => sharedPaneOf(p.id) ?? []),
})).filter((g) => g.items.length > 0)

/** A box's picker. `marked` is the entries on screen elsewhere, space-joined: a string, so the memo
 *  compares it by value. */
const BoxPicker = memo(function BoxPicker({
  entry,
  title,
  marked,
  onPick,
}: {
  entry: string
  title: string
  marked: string
  onPick: (entry: string) => void
}) {
  // The options' words are the catalog's: a change of language rebuilds them, as it re-renders the rest.
  useLocale()
  const marks = new Set(marked.split(' '))
  return (
    <select
      className="pane-pick"
      value={entry}
      aria-label={t('panels.box.pick.aria', { title })}
      title={t('panels.box.pick.title')}
      onChange={(e) => onPick(e.target.value)}
    >
      {PICKER_GROUPS.map(({ cat, items }) => (
        <optgroup key={cat} label={PANE_CATEGORY_LABEL[cat]()}>
          {items.map((x) => (
            <option key={x.id} value={x.id}>
              {marks.has(x.id) ? t('panels.box.pick.onScreen', { title: entryTitle(x) }) : entryTitle(x)}
            </option>
          ))}
        </optgroup>
      ))}
    </select>
  )
})

export function CockpitBox({ box, entry, source, selection, onScreen, onPick, onRemove, stacked }: CockpitBoxProps) {
  // The cockpit hands a new `onPick` on every render; the picker gets one that never changes.
  const pickNow = useRef(onPick)
  useEffect(() => {
    pickNow.current = onPick
  })
  const pick = useCallback((e: string) => pickNow.current(e), [])
  const shown = sharedPaneById(entry)
  if (!shown) return null
  const title = entryTitle(shown)
  const fill = shown.role === 'fill'
  const marked = PICKER_GROUPS.flatMap((g) => g.items)
    .filter((x) => x.id !== entry && onScreen(x.id))
    .map((x) => x.id)
    .join(' ')
  return (
    <CockpitPaneFrame
      title={title}
      paneId={box}
      fit={fill ? undefined : 'content'}
      weight={shown.role === 'fill' ? shown.weight : undefined}
      onRemove={onRemove}
      actions={<BoxPicker entry={entry} title={title} marked={marked} onPick={pick} />}
    >
      <div className={`box-body${stacked && fill ? ' box-body--stacked' : ''}`} data-box={entry}>
        <BoxBody pane={shown.pane} source={source} selection={selection} />
      </div>
    </CockpitPaneFrame>
  )
}

/**
 * THE RAIL'S BOXES IN THE COCKPIT, on a window too small for the rail (RailLink `folded`): each in the
 * cockpit's frame as a box is, keyed by its slot, its picker acting on the rail. They carry no ✕: a slot
 * closed here could be brought back only from the rail's own ⊞ Panels, which a window this size does not
 * show, so they leave the screen with the rail's own switch (⊞ Panels ▸ Dashboard rail). The cockpit stands
 * them at the foot of the column its own boxes stand in until placed.
 */
export function foldedRailBoxes(
  source: BoxSource | undefined,
  selection: BoxSelection,
  onScreen: (entry: string) => boolean,
  stacked: boolean,
): ReactNode[] {
  const folded = source?.rail?.folded
  if (!source || !folded) return []
  return folded.boxes.map((b) => (
    <CockpitBox
      key={b.slot}
      box={b.slot}
      entry={b.entry}
      source={source}
      selection={selection}
      onScreen={onScreen}
      onPick={(e) => folded.pick(b.slot, e)}
      stacked={stacked}
    />
  ))
}
