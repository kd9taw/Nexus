// A BOX'S BODY IN A COCKPIT (any pane in any area, 2026-10-07): its pane context, and the one renderer
// every Conditions box's body goes through (connect/PaneFrame `PaneBody`), so a box reads the same
// beside the log as it does on Conditions and in the dashboard rail.
//
// A COMPONENT OF ITS OWN, MOUNTED ONLY WHILE ITS BOX IS ON SCREEN, and that is load-bearing: its context
// subscribes the window's Conditions feeds (connect/usePaneContext → features/connectFeeds), which are
// polled only while some surface wants them. A cockpit with no box asks for nothing it did not ask for
// before, and several boxes share each feed's one poll.
//
// ⛔ THE BOX'S SELECTION, NEVER THE WINDOW'S (the dashboard rail's rule, components/DashRail). Its
// Selection and Outlook read the cockpit's box selection (CockpitBox `useBoxSelection`), and the Spots
// and Needed boards are lent with that selection in place of the app's, so a click in a box never
// re-targets the station a CW macro's `!` sends. Their Work is the boards' own: it moves the rig and
// opens a cockpit, and keys nothing.
import { useMemo } from 'react'
import { PaneBody } from '../connect/PaneFrame'
import { resolveSelection, usePaneContext } from '../connect/usePaneContext'
import type { PaneId } from '../../features/connectConfig'
import type { BoxSelection, BoxSource } from './CockpitBox'

export function BoxBody({ pane, source, selection }: { pane: PaneId; source: BoxSource; selection: BoxSelection }) {
  const { selectedCall, onSelectCall } = selection
  const resolved = useMemo(
    () => resolveSelection(selectedCall, source.stations, source.prop),
    [selectedCall, source.stations, source.prop],
  )
  const spotsFeed = source.spotsFeed && {
    rows: source.spotsFeed.rows,
    board: { ...source.spotsFeed.board, selectedCall, onSelect: onSelectCall },
  }
  const neededBoard = source.neededBoard && { ...source.neededBoard, selectedCall, onSelect: onSelectCall }
  const { ctx } = usePaneContext({
    myGrid: source.myGrid,
    theme: source.theme,
    intent: selection.intent,
    prop: source.prop,
    needByCall: source.needByCall,
    needAlerts: source.needAlerts,
    amp: source.amp,
    rigBand: source.rigBand ?? null,
    selectedCall,
    selection: resolved,
    onSelectCall,
    onWorkSpot: source.onWorkSpot,
    onPoint: source.onPoint,
    focusBand: selection.focusBand,
    toggleFocusBand: selection.toggleFocusBand,
    remote: null,
    spotsFeed,
    otaBoard: source.otaBoard,
    neededBoard,
    entryCall: source.entryCall,
  })
  return <PaneBody pane={pane} ctx={ctx} />
}
