import type { AppSnapshot, Tier } from '../types'
import type { StationAction } from './station-operation'
import { useSavedReceiverSetting } from './useDecoderSettings'

type ReceiverSetting = Extract<StationAction, { action: 'decoder.depth' | 'receiver.rxOffset' }>

/** Receive gestures bind the displayed tier and value. In particular a plain
 * waterfall click never turns into a TX or combined-marker action. */
export function useReceiverSettings(snap: AppSnapshot | null | undefined, tier: Tier | undefined) {
  const station = useSavedReceiverSetting<ReceiverSetting>(snap, tier, 'receiverSettings')
  const depth = snap?.radio.decodeDepth, hz = snap?.radio.rxOffsetHz
  const depthAllowed = station.allowed && Number.isInteger(depth) && depth! >= 1 && depth! <= 3
  const rxAllowed = station.allowed && typeof hz === 'number' && Number.isFinite(hz)
  const setDepth = (value: number) => {
    if (depthAllowed && tier && value !== depth && Number.isInteger(value) && value >= 1 && value <= 3)
      station.change({ action: 'decoder.depth', expectedTier: tier, expectedDepth: depth!, depth: value })
  }
  const tuneRx = (value: number) => {
    if (!rxAllowed || !tier || !Number.isFinite(value)) return
    const next = Math.max(200, Math.min(4000, value))
    if (next !== hz) station.change({ action: 'receiver.rxOffset', expectedTier: tier, expectedHz: hz!, hz: next })
  }
  return { depthAllowed, rxAllowed, setDepth, tuneRx }
}
