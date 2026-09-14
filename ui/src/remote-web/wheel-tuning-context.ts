import { createContext, useContext, useSyncExternalStore } from 'react'
import { useStationCapability } from '../stationAccess'
import type { WheelTuning } from './wheel-tuning'

export const RemoteWheelTuningContext = createContext<WheelTuning | null>(null)
const idleSubscribe = () => () => {}
const idleSnapshot = () => false
export function useRemoteWheelTuning() {
  const controller = useContext(RemoteWheelTuningContext), capability = useStationCapability('frequency')
  // `input`: wheel and digit steps outlive a brief control lapse; WheelTuning sends their burst only
  // once control is current again. `allowed` (a scope press) still needs current control.
  const held = useStationCapability('frequency', true)
  const pending = useSyncExternalStore(controller?.subscribe ?? idleSubscribe, controller?.getPending ?? idleSnapshot)
  return { controller, allowed: !!(controller && capability && !pending), input: !!(controller && held && !pending) }
}
