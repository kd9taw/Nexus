// Closed operating vocabulary. These are station intents, never Tauri command
// names. The station rechecks the lease and hardware binding before execution.
import { finite, integer, object as displayObject } from './display-validation'

export type Receiver = 'rtty' | 'psk' | 'sstv' | 'aprs'
export type TextReceiver = 'cw' | 'rtty' | 'psk'
export const RECEIVER_FUNCTIONS = ['nb', 'nr', 'notch', 'manualNotch'] as const
export type ReceiverFunction = (typeof RECEIVER_FUNCTIONS)[number]
export const AGC_SPEEDS = ['auto', 'fast', 'mid', 'slow', 'off'] as const
export type AgcSpeed = (typeof AGC_SPEEDS)[number]
export type StationAction =
  | { action: 'radio.frequency'; dialMhz: number; band: string; sideband: 'USB' | 'LSB' | 'FM' | 'AM' }
  | { action: 'radio.band'; band: string; mode: 'cw' | 'phone' }
  | { action: 'radio.filterWidth'; mode: 'cw' | 'phone'; expectedHz: number; hz: number }
  | { action: 'radio.function'; mode: 'cw' | 'phone'; func: ReceiverFunction; expectedOn: boolean; on: boolean }
  | { action: 'radio.agc'; mode: 'cw' | 'phone'; expectedSpeed: AgcSpeed; speed: AgcSpeed }
  | { action: 'radio.mode'; mode: 'digital' | 'phone' | 'cw' | 'rtty' | 'keyboard'; followFrequency: boolean }
  | { action: 'radio.tier'; tier: string }
  | { action: 'radio.workspace'; workspace: 'ft' | 'tempo' | 'js8' }
  | { action: 'radio.select'; radioId: number }
  | { action: 'radio.disarm' }
  | { action: 'decoder.arm'; receiver: Receiver; on: boolean }
  | { action: 'decoder.clear'; receiver: TextReceiver }
  | { action: 'decoder.afcReset'; receiver: 'rtty' | 'psk' }
  | { action: 'decoder.net'; receiver: 'rtty' | 'psk'; hz: number }
  | { action: 'decoder.pskMode'; mode: 'PSK31' | 'QPSK31'; reverse: boolean }
  | { action: 'decoder.js8Speed'; expectedSpeed: number; speed: number }
  | { action: 'decoder.msk144Period'; expectedPeriodSecs: number; periodSecs: number }
  | { action: 'decoder.depth'; expectedTier: string; expectedDepth: number; depth: number }
  | { action: 'receiver.rxOffset'; expectedTier: string; expectedHz: number; hz: number }
  | { action: 'receiver.rxGain'; radioId: number; expectedSettingsRevision: string; expectedGain: number; gain: number }
  | { action: 'amplifier.operate'; expectedOperate: boolean; operate: boolean }
  | { action: 'amplifier.band'; expectedBand: string; direction: -1 | 1 }
  | { action: 'amplifier.followBand'; radioId: number; expectedSettingsRevision: string; expectedFollow: boolean; follow: boolean }

export type ControlContext = {
  radioId: number
  radioConnection: number | null
  ampConnection: number | null
  ampReadSequence: number | null
}
export const CONTROL_CAPABILITIES = ['decoder', 'radio', 'amplifier', 'frequency', 'mode', 'tier', 'ampFollowBand', 'workspace', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp'] as const
export type ControlCapability = (typeof CONTROL_CAPABILITIES)[number]
// A new action cannot silently inherit a broader capability by its prefix.
const ACTION_CAPABILITY: Record<StationAction['action'], ControlCapability> = {
  'radio.disarm': 'radio', 'radio.select': 'radio', 'radio.frequency': 'frequency',
  'radio.band': 'bandSelection', 'radio.mode': 'mode', 'radio.tier': 'tier', 'radio.workspace': 'workspace',
  'radio.filterWidth': 'receiverFilter',
  'radio.function': 'receiverDsp', 'radio.agc': 'receiverDsp',
  'decoder.arm': 'decoder', 'decoder.clear': 'decoder', 'decoder.afcReset': 'decoder',
  'decoder.net': 'decoder', 'decoder.pskMode': 'decoder',
  'decoder.js8Speed': 'decoderSettings', 'decoder.msk144Period': 'decoderSettings',
  'decoder.depth': 'receiverSettings', 'receiver.rxOffset': 'receiverSettings',
  'receiver.rxGain': 'receiverGain',
  'amplifier.operate': 'amplifier', 'amplifier.band': 'amplifier', 'amplifier.followBand': 'ampFollowBand'
}
export const actionCapability = (action: StationAction): ControlCapability => ACTION_CAPABILITY[action.action]
export const CONTROL_TIERS = [
  'FT8', 'FT4', 'FT2', 'Q65', 'MSK144', 'JT65', 'FST4', 'FST4W', 'WSPR', 'JS8',
  'TempoFast', 'TempoDeep'
] as const
const BANDS = ['2190m', '630m', '160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '1.25m', '70cm', '33cm', '23cm', '13cm', '9cm', '6cm', '3cm', '1.25cm', '6mm', '4mm', '2.5mm', '2mm', '1mm']
const invalid = (): never => { throw Error('invalidOperation') }
function object(raw: unknown, keys: string[]): Record<string, unknown> {
  try { return displayObject(raw, keys) } catch { return invalid() }
}
const oneOf = (v: unknown, choices: readonly string[]) => typeof v === 'string' && choices.includes(v)

export function controlContext(raw: unknown): ControlContext {
  const c = object(raw, ['radioId', 'radioConnection', 'ampConnection', 'ampReadSequence'])
  if (!integer(c.radioId) || c.radioId < 0 || c.radioId > 0xffffffff) invalid()
  for (const key of ['radioConnection', 'ampConnection', 'ampReadSequence']) {
    const v = c[key]
    if (v !== null && (!integer(v) || v < 1)) invalid()
  }
  if ((c.ampConnection === null) !== (c.ampReadSequence === null)) invalid()
  return raw as ControlContext
}

export function stationAction(raw: unknown): StationAction {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const a = raw as Record<string, unknown>
  switch (a.action) {
    case 'radio.frequency':
      object(a, ['action', 'dialMhz', 'band', 'sideband'])
      if (!finite(a.dialMhz) || a.dialMhz <= 0 || a.dialMhz > 250000 || (a.band !== '' && !oneOf(a.band, BANDS)) || !oneOf(a.sideband, ['USB', 'LSB', 'FM', 'AM'])) invalid()
      break
    case 'radio.band':
      object(a, ['action', 'band', 'mode'])
      if (!oneOf(a.band, BANDS) || !oneOf(a.mode, ['cw', 'phone'])) invalid()
      break
    case 'radio.filterWidth':
      object(a, ['action', 'mode', 'expectedHz', 'hz'])
      if (!oneOf(a.mode, ['cw', 'phone']) || !integer(a.expectedHz) || a.expectedHz < 1 || a.expectedHz > 0xffffffff ||
        !integer(a.hz) || a.hz < (a.mode === 'cw' ? 50 : 300) || a.hz > (a.mode === 'cw' ? 2000 : 4000)) invalid()
      break
    case 'radio.function':
      object(a, ['action', 'mode', 'func', 'expectedOn', 'on'])
      if (!oneOf(a.mode, ['cw', 'phone']) || !oneOf(a.func, RECEIVER_FUNCTIONS) || typeof a.expectedOn !== 'boolean' || typeof a.on !== 'boolean') invalid()
      break
    case 'radio.agc':
      object(a, ['action', 'mode', 'expectedSpeed', 'speed'])
      if (!oneOf(a.mode, ['cw', 'phone']) || !oneOf(a.expectedSpeed, AGC_SPEEDS) || !oneOf(a.speed, AGC_SPEEDS)) invalid()
      break
    case 'radio.mode':
      object(a, ['action', 'mode', 'followFrequency'])
      if (!oneOf(a.mode, ['digital', 'phone', 'cw', 'rtty', 'keyboard']) || typeof a.followFrequency !== 'boolean') invalid()
      break
    case 'radio.workspace':
      object(a, ['action', 'workspace'])
      if (!oneOf(a.workspace, ['ft', 'tempo', 'js8'])) invalid()
      break
    case 'radio.tier':
      object(a, ['action', 'tier'])
      if (!oneOf(a.tier, CONTROL_TIERS)) invalid()
      break
    case 'radio.select':
      object(a, ['action', 'radioId'])
      if (!integer(a.radioId) || a.radioId < 0 || a.radioId > 0xffffffff) invalid()
      break
    case 'radio.disarm':
      object(a, ['action'])
      break
    case 'decoder.arm':
      object(a, ['action', 'receiver', 'on'])
      if (!oneOf(a.receiver, ['rtty', 'psk', 'sstv', 'aprs']) || typeof a.on !== 'boolean') invalid()
      break
    case 'decoder.clear':
      object(a, ['action', 'receiver'])
      if (!oneOf(a.receiver, ['cw', 'rtty', 'psk'])) invalid()
      break
    case 'decoder.afcReset':
      object(a, ['action', 'receiver'])
      if (!oneOf(a.receiver, ['rtty', 'psk'])) invalid()
      break
    case 'decoder.net':
      object(a, ['action', 'receiver', 'hz'])
      if (!oneOf(a.receiver, ['rtty', 'psk']) || !finite(a.hz) || a.hz < 300 || a.hz > 3700) invalid()
      break
    case 'decoder.pskMode':
      object(a, ['action', 'mode', 'reverse'])
      if (!oneOf(a.mode, ['PSK31', 'QPSK31']) || typeof a.reverse !== 'boolean') invalid()
      break
    case 'decoder.js8Speed':
      object(a, ['action', 'expectedSpeed', 'speed'])
      if (!integer(a.expectedSpeed) || !integer(a.speed) || a.expectedSpeed < 0 || a.expectedSpeed > 3 || a.speed < 0 || a.speed > 3 || a.expectedSpeed === a.speed) invalid()
      break
    case 'decoder.msk144Period':
      object(a, ['action', 'expectedPeriodSecs', 'periodSecs'])
      if (!integer(a.expectedPeriodSecs) || !integer(a.periodSecs) || ![5, 10, 15, 30].includes(a.expectedPeriodSecs) || ![5, 10, 15, 30].includes(a.periodSecs) || a.expectedPeriodSecs === a.periodSecs) invalid()
      break
    case 'decoder.depth':
      object(a, ['action', 'expectedTier', 'expectedDepth', 'depth'])
      if (!oneOf(a.expectedTier, CONTROL_TIERS) || !integer(a.expectedDepth) || a.expectedDepth < 1 || a.expectedDepth > 3 ||
        !integer(a.depth) || a.depth < 1 || a.depth > 3 || a.expectedDepth === a.depth) invalid()
      break
    case 'receiver.rxOffset':
      object(a, ['action', 'expectedTier', 'expectedHz', 'hz'])
      if (!oneOf(a.expectedTier, CONTROL_TIERS) || !finite(a.expectedHz) || !finite(a.hz) || a.hz < 200 || a.hz > 4000 || a.expectedHz === a.hz) invalid()
      break
    case 'receiver.rxGain':
      object(a, ['action', 'radioId', 'expectedSettingsRevision', 'expectedGain', 'gain'])
      if (!integer(a.radioId) || a.radioId < 0 || a.radioId > 0xffffffff ||
        typeof a.expectedSettingsRevision !== 'string' || !/^[0-9a-f]{64}$/.test(a.expectedSettingsRevision) ||
        !finite(a.expectedGain) || !finite(a.gain) || a.gain < 1 || a.gain > 8 || a.expectedGain === a.gain) invalid()
      break
    case 'amplifier.operate':
      object(a, ['action', 'expectedOperate', 'operate'])
      if (typeof a.expectedOperate !== 'boolean' || typeof a.operate !== 'boolean' || a.expectedOperate === a.operate) invalid()
      break
    case 'amplifier.band':
      object(a, ['action', 'expectedBand', 'direction'])
      if (!oneOf(a.expectedBand, BANDS.slice(2, 14)) || ![-1, 1].includes(Number(a.direction)) || typeof a.direction !== 'number') invalid()
      break
    case 'amplifier.followBand':
      object(a, ['action', 'radioId', 'expectedSettingsRevision', 'expectedFollow', 'follow'])
      if (!integer(a.radioId) || a.radioId < 0 || a.radioId > 0xffffffff ||
        typeof a.expectedSettingsRevision !== 'string' || !/^[0-9a-f]{64}$/.test(a.expectedSettingsRevision) ||
        typeof a.expectedFollow !== 'boolean' || typeof a.follow !== 'boolean' || a.expectedFollow === a.follow) invalid()
      break
    default: invalid()
  }
  return raw as StationAction
}

export type ControlOutcome = { operation: 'stationControl'; operationId: string } & (
  | { outcome: 'pending' }
  | { outcome: 'applied'; evidence: 'receiverState' | 'radioReadback' | 'amplifierReadback' | 'stationState' | 'settingsSaved' }
  | { outcome: 'rejected' | 'unknown'; reason: string }
)
const REASONS = ['authorityExpired', 'contextChanged', 'readingUnavailable', 'stationBusy', 'hardwareUnavailable', 'hardwareUnconfirmed', 'unsupportedAction', 'invalidAction', 'persistenceFailed']
export function controlOutcome(raw: unknown): ControlOutcome {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const v = raw as Record<string, unknown>
  object(v, ['operation', 'operationId', 'outcome', ...(v.outcome === 'applied' ? ['evidence'] : v.outcome === 'pending' ? [] : ['reason'])])
  if (v.operation !== 'stationControl' || typeof v.operationId !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v.operationId)) invalid()
  if (v.outcome === 'applied') {
    if (!oneOf(v.evidence, ['receiverState', 'radioReadback', 'amplifierReadback', 'stationState', 'settingsSaved'])) invalid()
  } else if (v.outcome !== 'pending' && (!oneOf(v.outcome, ['rejected', 'unknown']) || !oneOf(v.reason, REASONS))) invalid()
  return raw as ControlOutcome
}
