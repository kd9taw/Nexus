// UI affordances follow the session's station authority. Enforcement remains in
// the transport and station; hiding/disabling a button never grants permission.
// The installed desktop keeps its existing authority and transmit guards.
import { createContext, useContext, useSyncExternalStore } from 'react'
import type { OperationClient } from './remote-web/operation-client'
import type { ControlCapability } from './remote-web/station-operation'
import type { RadioStatus } from './types'

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
  const expanded = ['frequency', 'mode', 'tier', 'workspace', 'ampFollowBand', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp'].includes(capability)
  return local || !!(available && (!expanded || (client?.operationVersion ?? 0) >= 3) && view?.fresh && view.connected && !view.unresolved && !view.controlPending &&
    view.state?.phase === 'controlling' && (!['frequency', 'mode', 'tier', 'workspace', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp'].includes(capability) || !view.state.txArmed) && view.state.controls?.capabilities.includes(capability))
}

/** Local tier changes retain their native conditions. Remote selection needs
 * a settled digital receiver and the specific station capability. */
export function useStationTierControl(radio: RadioStatus): boolean {
  const local = useStationControl(), allowed = useStationCapability('tier')
  return local || !!(allowed && radio.operatingMode?.toLowerCase() === 'digital' &&
    radio.catOk === true && !radio.txEnabled && !radio.transmitting && !radio.rigKeyed &&
    !radio.tuning && !radio.txBusyReason)
}
