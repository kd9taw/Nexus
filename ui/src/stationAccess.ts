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
/** STATION CONTROL HELD by this browser: connected, the latest station state showing this browser
 * controlling, with nothing unresolved and no command in flight. It is the part of
 * useStationCapability that names no capability, and it is THE ONE ANSWER a control's enabled
 * state follows (operator decision 2026-09-14) — so it is deliberately lapse-tolerant: a
 * heartbeat reply landing after the 1.2 s freshness window (most seconds on a real WAN) leaves
 * this true, and a control does not disable under the operator. The send never borrows it:
 * OperationClient waits for current control, at most CONTROL_RESUME_MS, and otherwise refuses as
 * not sent. Stale application readings (StationDataContext) still refuse here at once. */
export function useStationHeld(): boolean {
  const local = useStationControl(), available = useStationData()
  const client = useContext(RemoteOperationsContext)
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleSnapshot)
  return local || !!(available && view?.connected && view.requestReady !== false && !view.unresolved &&
    !view.controlPending && view.state?.phase === 'controlling')
}

/** A permission for an explicit gesture. Never enables native-only TX controls
 * or the effects which automatically arm receivers on local view entry.
 * Held control (above) carries it through a brief lapse, so an open dropdown is not closed; a
 * transmit action is still refused at once. Pass `lapse = false` where the gesture must begin
 * on current control (a scope press captures its command window at pointer-down). */
export function useStationCapability(capability: ControlCapability, lapse = true): boolean {
  const local = useStationControl(), held = useStationHeld()
  const client = useContext(RemoteOperationsContext)
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleSnapshot)
  const expanded = ['frequency', 'mode', 'tier', 'workspace', 'ampFollowBand', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp', 'phoneMode', 'workSpot', 'radioLevels', 'radioSelection', 'fmTuning', 'fmReceiver'].includes(capability)
  return local || !!(held && (!['ftRuntime', 'ftSettings', 'qsoLogging', 'ftOperate', 'ftCall', 'ftExchange', 'ftMessages'].includes(capability) || (client?.operationVersion ?? 0) >= 4) && (!expanded || (client?.operationVersion ?? 0) >= 3) && (lapse || view?.fresh) &&
    (!['frequency', 'mode', 'tier', 'workspace', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp', 'phoneMode', 'workSpot', 'radioLevels', 'radioSelection', 'fmTuning', 'fmReceiver'].includes(capability) || !view?.state?.txArmed) && view?.state?.controls?.capabilities.includes(capability))
}

/** Local tier changes retain their native conditions. Remote selection needs
 * a settled digital receiver and the specific station capability. */
export function useStationTierControl(radio: RadioStatus): boolean {
  const local = useStationControl(), allowed = useStationCapability('tier')
  return local || !!(allowed && radio.operatingMode?.toLowerCase() === 'digital' &&
    radio.catOk === true && !radio.txEnabled && !radio.transmitting && !radio.rigKeyed &&
    !radio.tuning && !radio.txBusyReason)
}

/** Stop is independent of stale observations, receipts and ordinary commands. */
export function useStationStopControl(): boolean {
  const local = useStationControl(), client = useContext(RemoteOperationsContext)
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleSnapshot)
  return local || !!(view?.connected && view.stopAvailable && !view.stopSending)
}
