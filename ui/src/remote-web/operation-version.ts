import type { StationAction } from './station-operation'

export const OPERATION_VERSIONS = [1, 2, 3] as const
export type OperationVersion = (typeof OPERATION_VERSIONS)[number]
export function parseOperationVersion(raw: unknown): OperationVersion | null {
  return OPERATION_VERSIONS.includes(raw as OperationVersion) ? raw as OperationVersion : null
}

/** Keep the original v2 advertisement for older cloud/browser builds. New
 * peers explicitly opt into the expanded version; absence preserves v1/v2. */
export function advertisedOperationVersion(base: unknown, maximum?: unknown): OperationVersion | 0 {
  const legacy = base === 1 ? 1 : base === 2 ? 2 : 0
  return legacy === 2 && maximum === 3 ? 3 : legacy
}

export function controlVersion(action: StationAction): 2 | 3 {
  return ['radio.band', 'radio.filterWidth', 'radio.function', 'radio.agc', 'radio.phoneMode', 'radio.frequency', 'radio.mode', 'radio.tier', 'radio.workspace', 'radio.select', 'amplifier.followBand', 'decoder.js8Speed', 'decoder.msk144Period', 'decoder.depth', 'receiver.rxOffset', 'receiver.rxGain'].includes(action.action) ? 3 : 2
}
