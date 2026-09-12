import { createContext, useContext, useSyncExternalStore } from 'react'
import { useStationCapability } from '../stationAccess'
import type { WheelTuning } from './wheel-tuning'

export const RemoteWheelTuningContext = createContext<WheelTuning | null>(null)
const idleSubscribe = () => () => {}
const idleSnapshot = () => false
export function useRemoteWheelTuning() {
  const controller = useContext(RemoteWheelTuningContext), capability = useStationCapability('frequency')
  const pending = useSyncExternalStore(controller?.subscribe ?? idleSubscribe, controller?.getPending ?? idleSnapshot)
  return { controller, allowed: !!(controller && capability && !pending) }
}
