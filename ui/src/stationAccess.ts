// UI affordances follow the session's station authority. Enforcement remains in
// the transport and station; hiding/disabling a button never grants permission.
// The installed desktop keeps its existing authority and transmit guards.
import { createContext, useContext, useSyncExternalStore } from 'react'
import type { OperationClient } from './remote-web/operation-client'
import type { ControlCapability } from './remote-web/station-operation'

export const StationControlContext = createContext(true)
export function useStationControl(): boolean { return useContext(StationControlContext) }
export const StationDataContext = createContext(true)
export function useStationData(): boolean { return useContext(StationDataContext) }
export const RemoteOperationsContext = createContext<OperationClient | null>(null)
const idleSubscribe = () => () => {}
const idleSnapshot = () => null
/** A permission for an explicit gesture. Never enables native-only TX controls
 * or the effects which automatically arm receivers on local view entry. */
export function useStationCapability(capability: ControlCapability): boolean {
  const local = useStationControl(), available = useStationData()
  const client = useContext(RemoteOperationsContext)
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleSnapshot)
  return local || !!(available && view?.fresh && view.connected && !view.unresolved && !view.controlPending &&
    view.state?.phase === 'controlling' && (!['frequency', 'mode'].includes(capability) || !view.state.txArmed) && view.state.controls?.capabilities.includes(capability))
}
