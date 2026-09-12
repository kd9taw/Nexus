import { createContext, useContext } from 'react'
import { initialState, type MonitorState } from '../remote-monitor/session'
import { useStationData } from '../stationAccess'
import type { ControlContext } from './station-operation'

// Reuse the authenticated observation stream already carried by this hosted
// connection. The displayed amp and its opaque hardware binding come together.
export const RemoteObservationContext = createContext<MonitorState>(initialState)
export function useRemoteStation(radioId: number | undefined) {
  const observation = useContext(RemoteObservationContext), available = useStationData()
  const frame = observation.frame
  const station = available && observation.status === 'current' && frame && frame.station.radio.id === radioId
    ? frame.station : null
  const radio = station?.radio, amp = station?.amplifier ?? null
  const context: ControlContext | null = station ? {
    radioId: station.radio.id,
    radioConnection: radio?.readings.cat?.connectionGeneration ?? null,
    ampConnection: amp?.reading?.connectionGeneration ?? null,
    ampReadSequence: amp?.reading?.readSequence ?? null
  } : null
  return { station, context }
}

export function useRemoteAmplifier(radioId: number | undefined) {
  const { station, context } = useRemoteStation(radioId)
  const radio = station?.radio, amp = station?.amplifier ?? null
  const idle = !!(radio?.catConnected && radio.rigKeyed === false && !radio.nexusBusy &&
    radio.readings.ptt && radio.readings.ptt.ageMs < 1000 && amp?.linked && amp.reading &&
    amp.transmitting !== true && (amp.outputWatts === null || amp.outputWatts === 0))
  return { amp, context, idle }
}
