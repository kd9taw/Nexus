import { createContext, useContext, useSyncExternalStore } from 'react'
import { useStationCapability } from '../stationAccess'
import type { WheelTuning } from './wheel-tuning'

export const RemoteWheelTuningContext = createContext<WheelTuning | null>(null)
const idleSubscribe = () => () => {}
const idleSnapshot = () => false
const idleProvisional = () => null
export function useRemoteWheelTuning() {
  const controller = useContext(RemoteWheelTuningContext), capability = useStationCapability('frequency', false)
  // `input`: wheel and digit steps outlive a brief control lapse AND a command already in flight;
  // WheelTuning sends their burst once control is current again, or queues it behind that command
  // and sends it when the station reads the radio back. Nothing is dropped here, so nothing gates
  // on `pending`. `allowed` (a scope press) still needs current control and an idle controller:
  // an absolute target cannot be queued behind a dial it does not know.
  const held = useStationCapability('frequency', true)
  const pending = useSyncExternalStore(controller?.subscribe ?? idleSubscribe, controller?.getPending ?? idleSnapshot)
  // DISPLAY ONLY, and a plain number: the dial this browser has asked for, until the station's own
  // reading shows it. It is read off the controller and passed down as a prop — it is deliberately
  // not in `AppSnapshot`, `OperationState` or any context value, so nothing that decides a
  // privilege, a band, a mode, TX enable or an S-meter can reach it (wheel-tuning.ts's class
  // comment, and `optimistic-dial.test.ts` holds it).
  const provisionalHz = useSyncExternalStore(controller?.subscribe ?? idleSubscribe, controller?.getProvisionalHz ?? idleProvisional)
  return { controller, allowed: !!(controller && capability && !pending), input: !!(controller && held),
    provisionalMhz: provisionalHz === null ? undefined : provisionalHz / 1e6 }
}
