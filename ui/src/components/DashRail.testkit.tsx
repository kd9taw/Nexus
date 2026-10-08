// THE DASHBOARD RAIL AS APP MOUNTS IT, for the tests that render the rail on its own: over the records App
// owns (`useDashRail`, one per cockpit), for the section it stands beside.
import { DashRail, dashRailRecords, useDashRail, type DashRailProps } from './DashRail'
import { isDashRailSection } from '../features/dashRail'

export function OwnedDashRail(p: Omit<DashRailProps, 'rail'>) {
  const rail = useDashRail()
  if (!isDashRailSection(p.section)) throw new Error(`no dashboard rail beside ${p.section}`)
  return <DashRail {...p} rail={dashRailRecords(rail, p.section)} />
}
