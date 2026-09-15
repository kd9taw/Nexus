import { createContext, useContext } from 'react'
import { initialState, type MonitorState } from '../remote-monitor/session'
import type { MonitorFrame } from '../remote-monitor/protocol'
import { useStationData, useStationHeld } from '../stationAccess'
import type { ControlContext } from './station-operation'

/** How long one of the station's own hardware readings answers for. */
export const READING_CURRENT_MS = 1000

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

/**
 * THE OBSERVED RIG, READY FOR A COMMAND: connected, not keyed, not busy, on a PTT reading the
 * station took recently. The one place that question is answered — the amplifier strip, the
 * decode/receive settings and the RX gain slider all read it here.
 *
 * Held station control (`held`) carries it through a brief lapse, exactly as it carries a
 * capability (operator decision 2026-09-14): an observation poll landing late used to disable the
 * amplifier buttons and the decode-depth chips under the operator, every second or so on a real
 * WAN. What still refuses at once: stale application readings, a station state that no longer shows
 * this browser controlling, and the frame itself, which drops a reading — and `rigKeyed` with it —
 * at MEASUREMENT_STALE_MS. The send is never enabled by this: the station re-checks its own
 * measurements before it acts, and OperationClient waits for current control or refuses as not sent.
 */
export function rigReady(radio: MonitorFrame['station']['radio'] | undefined, held: boolean): boolean {
  return !!(radio?.catConnected && radio.rigKeyed === false && !radio.nexusBusy &&
    radio.readings.ptt && (held || radio.readings.ptt.ageMs < READING_CURRENT_MS))
}

/** `useRemoteStation` plus that answer. Deliberately a SEPARATE hook: it subscribes to the station
 * operation view, and only the controls that ask this question should pay for that — wheel tuning
 * and the scope press read `useRemoteStation` on every gesture and must not gain a subscription. */
export function useRemoteRig(radioId: number | undefined) {
  const observation = useRemoteStation(radioId)
  const held = useStationHeld()
  return { ...observation, ready: rigReady(observation.station?.radio, held) }
}

export function useRemoteAmplifier(radioId: number | undefined) {
  const { station, context, ready } = useRemoteRig(radioId)
  const amp = station?.amplifier ?? null
  const idle = !!(ready && amp?.linked && amp.reading &&
    amp.transmitting !== true && (amp.outputWatts === null || amp.outputWatts === 0))
  return { amp, context, idle }
}
