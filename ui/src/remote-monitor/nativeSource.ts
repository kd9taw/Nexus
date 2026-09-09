import { getRemoteMonitorFrame, isTauri } from '../api'
import type { MonitorSource } from './session'

export const nativeSource: MonitorSource = {
  id: 'native-station',
  kind: 'native',
  read: async (signal) => {
    if (signal.aborted) throw new Error('monitorReadCancelled')
    if (!isTauri()) throw new Error('nativeMonitorUnavailable')
    return getRemoteMonitorFrame()
  },
}
