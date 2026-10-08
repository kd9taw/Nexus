// CONNECT'S NEEDED BOX — the Needed board in a Connect slot or in the dashboard rail. It is the REAL
// board (NeededPanel, the Needed view's own component, hosted as a pane) with the wiring its window
// gives the Needed view, so a row's Work is that view's act and nothing else: no second list, no second
// filter, no second work path. As a pane it leaves out what is the view's (its heading, open at launch,
// the rotator widget) and opens as the view does, on every mode, in a filter record of its own, so a
// chip in the box never moves the view's or a cockpit pane's.
//
// It keys nothing. Working a row QSYs and opens a cockpit through the board's own handler, exactly as
// from the Needed view, and that handler keys no transmitter.
import { NeededPanel } from '../NeededPanel'
import type { NeededBoard } from './paneContext'

/** The box's filter record: per surface, like the view's `neededFilters` and the cockpit panes' own. */
const CONNECT_NEEDED_FILTERS = 'nexus.connect.neededFilters'

export function NeededBox({ board }: { board: NeededBoard }) {
  return (
    <div className="cn-needed">
      <NeededPanel {...board} pane={{ filterKey: CONNECT_NEEDED_FILTERS }} />
    </div>
  )
}
