// UI affordances follow the session's station authority. Enforcement remains in
// the transport and station; hiding/disabling a button never grants permission.
// The installed desktop keeps its existing authority and transmit guards.
import { createContext, useContext, useSyncExternalStore } from 'react'
import type { OperationClient } from './remote-web/operation-client'
import { TUNE_CAPABILITIES, TX_IDLE_CAPABILITIES, type ControlCapability } from './remote-web/station-operation'
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
  const expanded = ['frequency', 'mode', 'tier', 'workspace', 'ampFollowBand', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp', 'phoneMode', 'workSpot', 'radioLevels', 'radioSelection', 'fmTuning', 'fmReceiver', ...TUNE_CAPABILITIES].includes(capability)
  return local || !!(held && (!['ftRuntime', 'ftSettings', 'qsoLogging', 'ftOperate', 'ftCall', 'ftExchange', 'ftMessages'].includes(capability) || (client?.operationVersion ?? 0) >= 4) && (!expanded || (client?.operationVersion ?? 0) >= 3) && (lapse || view?.fresh) &&
    (!['frequency', 'mode', 'tier', 'workspace', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp', 'phoneMode', 'workSpot', 'radioLevels', 'radioSelection', 'fmTuning', 'fmReceiver', ...TX_IDLE_CAPABILITIES].includes(capability) || !view?.state?.txArmed) && view?.state?.controls?.capabilities.includes(capability))
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

/** How far a REMOTE Stop has actually got — 'idle' on the desktop, where the call is the act.
 *
 * The station returns Ok on ACCEPTANCE, not on RF: when its Engine is held (a radio-loop tick,
 * another command) the halt runs afterwards on its own thread with no deadline. So an accepted Stop
 * says `sent`, never `stopped`, and only the station's OWN reading of the transmitter turns it into
 * `stopped`. Saying "stopped" while the rig is still keyed is the failure this exists to prevent;
 * saying "stop sent" a moment longer than necessary costs nothing. With no reading to go on
 * (`radio` absent) it stays `sent` — the safe direction, never an assertion nobody made. */
export function useStationStopProgress(radio?: RadioStatus | null): 'idle' | 'sending' | 'sent' | 'stopped' {
  const local = useStationControl(), client = useContext(RemoteOperationsContext)
  const view = useSyncExternalStore(client?.subscribe ?? idleSubscribe, client?.getSnapshot ?? idleSnapshot)
  if (local || !view) return 'idle'
  if (view.stopSending) return 'sending'
  if (!view.stopAccepted) return 'idle'
  return radio && !radio.transmitting && !radio.tuning && !radio.rigKeyed && !radio.txEnabled ? 'stopped' : 'sent'
}
