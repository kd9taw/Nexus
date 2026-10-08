// THE DASHBOARD RAIL'S SWITCH, as a cockpit's ⊞ Panels menu offers it (the operator's pick: "one
// click from ⊞ Panels or the NOW bar"). App owns the rail; the menus that offer its switch are drawn
// by every cockpit.
//
// A MODULE STORE, NOT A PROP (bandConditions.ts's reasoning, for the same shape of problem): the ⊞
// menu is drawn inside eight cockpits, and threading the switch through each would touch every one of
// them — and their keep-alive hosts — for one row. App publishes the switch for the section on screen
// (null where the rail does not stand: Connect, Settings, a pop-out, the hosted Remote page), and
// PanelsMenu subscribes. Only the cockpit on screen can open its menu, so the section on screen is the
// only one a menu ever needs. The switch sits OUTSIDE the rail, so the rail can always be turned off
// from the cockpit, whatever the rail itself shows.
import { useSyncExternalStore } from 'react'

export interface DashRailSwitch {
  /** The operator has the rail on for the section on screen. */
  on: boolean
  /** This window is large enough to show it (`lg` and up). Below that the row still records the
   *  choice and says why nothing appears (the menu's rule: a note explains, it never refuses). */
  fits: boolean
  /** Below `lg` the rail's boxes stand in this cockpit's columns instead (features/dashRail
   *  `DASH_RAIL_FOLDS`), and the row says so. */
  folds?: boolean
  set: (on: boolean) => void
}

let current: DashRailSwitch | null = null
const listeners = new Set<() => void>()

/** Publish the switch for the section on screen, or null where there is no rail. App calls it. */
export function publishDashRailSwitch(sw: DashRailSwitch | null): void {
  current = sw
  for (const l of [...listeners]) l()
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

/** The switch a ⊞ menu offers, or null when there is none to offer. */
export function useDashRailSwitch(): DashRailSwitch | null {
  return useSyncExternalStore(subscribe, () => current, () => current)
}

/** Tests: nothing published, after every test (src/test-setup.ts). */
export function __resetDashRailSwitchForTests(): void {
  current = null
}
;(globalThis as { __nexusTestResets?: Set<() => void> }).__nexusTestResets?.add(__resetDashRailSwitchForTests)
