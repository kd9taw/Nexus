import type { StationAction } from './station-operation'

/** The operation lane's own ladder. v5 adds nothing a browser can SEND: it is the station's
 * `operationEvent` - a settled control's outcome and a fresh state, pushed the moment the radio
 * loop finishes it, where v4 left the browser to poll `state` and `result` for both. A v5 page
 * against a v4 station polls as before; a v5 station never pushes to a page below v5. */
export const OPERATION_VERSIONS = [1, 2, 3, 4, 5] as const
export type OperationVersion = (typeof OPERATION_VERSIONS)[number]
export function parseOperationVersion(raw: unknown): OperationVersion | null {
  return OPERATION_VERSIONS.includes(raw as OperationVersion) ? raw as OperationVersion : null
}

/** Keep the original v2 advertisement for older cloud/browser builds. New
 * peers explicitly opt into the expanded version; absence preserves v1/v2. */
export function advertisedOperationVersion(base: unknown, maximum?: unknown, ft?: unknown, push?: unknown): OperationVersion | 0 {
  const legacy = base === 1 ? 1 : base === 2 ? 2 : 0
  if (legacy !== 2 || maximum !== 3) return legacy
  return ft === 1 ? (push === 1 ? 5 : 4) : 3
}

export function controlVersion(action: StationAction): 2 | 3 | 4 {
  if (action.action === 'ft.runtime' || action.action === 'ft.setting' || action.action === 'qso.logCurrent' || action.action === 'qso.confirm' || action.action === 'qso.discard' || action.action === 'ft.message' || action.action === 'ft.exchange' || action.action === 'ft.call' || action.action === 'ft.cq' || action.action === 'ft.txEnabled') return 4
  return ['radio.level', 'radio.band', 'radio.filterWidth', 'radio.function', 'radio.agc', 'radio.phoneMode', 'radio.workSpot', 'radio.workDigitalSpot', 'radio.workRttySpot', 'radio.repeater', 'radio.memoryRecall', 'radio.aprsTune', 'rotator.point', 'rotator.pointAtCall', 'rotator.stop', 'sstv.deleteImage', 'radio.scope', 'radio.subLevel', 'radio.frequency', 'radio.mode', 'radio.tier', 'radio.workspace', 'radio.select', 'amplifier.followBand', 'decoder.js8Speed', 'decoder.msk144Period', 'decoder.depth', 'receiver.rxOffset', 'receiver.rxGain', 'decoder.aiCw', 'decoder.redecode', 'radio.split', 'radio.xit', 'radio.vfo', 'radio.rit',
    'satellite.track', 'satellite.stopTrack', 'satellite.transponder', 'satellite.doppler',
    'satellite.uplinkMap', 'satellite.peg', 'satellite.elements'].includes(action.action) ? 3 : 2
}
