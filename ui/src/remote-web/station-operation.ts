// Closed operating vocabulary. These are station intents, never Tauri command
// names. The station rechecks the lease and hardware binding before execution.
import { finite, integer, object as displayObject } from './display-validation'

export type Receiver = 'rtty' | 'psk' | 'sstv' | 'aprs'
export type TextReceiver = 'cw' | 'rtty' | 'psk'
export const RECEIVER_FUNCTIONS = ['nb', 'nr', 'notch', 'manualNotch'] as const
export type ReceiverFunction = (typeof RECEIVER_FUNCTIONS)[number]
export const AGC_SPEEDS = ['auto', 'fast', 'mid', 'slow', 'off'] as const
export type AgcSpeed = (typeof AGC_SPEEDS)[number]
export const PHONE_MODES = ['auto', 'USB', 'LSB', 'FM', 'AM'] as const
export type PhoneMode = (typeof PHONE_MODES)[number]
/** Which VFO carries the uplink during a pass — `Settings.satVfoMap`, kebab for kebab the Rust
 * enum (`tempo_app::settings::SatVfoMap`). Literal here, like the APRS channels below: this
 * grammar is compiled into the relay, which must not pull in the app's type module. */
export const SAT_VFO_MAPS = ['off', 'downlink-only', 'uplink-only', 'a-down-b-up', 'a-up-b-down',
  'main-down-sub-up', 'main-up-sub-down'] as const
export type SatVfoMap = (typeof SAT_VFO_MAPS)[number]
export const RADIO_LEVELS = ['power', 'micGain', 'nr', 'compression', 'notch'] as const
export type RadioLevel = (typeof RADIO_LEVELS)[number]
export type FtCallSelection = { call: string; grid: string | null; message: string | null; snr: number | null; freq: number | null }
export type FtExchangeContext = { dxcall: string | null; state: string; txNow: string | null; cqRunning: boolean }
export type FtExchangeChange = { kind: 'resend' | 'monitor' } | { kind: 'freeText'; text: string }
export type PendingLogEdits = { call: string; grid: string | null; rstSent: string | null; rstRcvd: string | null }
export type FtSettingsContext = { key: string; txOffsetHz: number; rxOffsetHz: number; holdTxFreq: boolean; txEven: boolean; txCycleAuto: boolean }
export type FtSettingChange = { kind: 'txOffset' | 'bothOffsets'; hz: number } | { kind: 'hold'; on: boolean } | { kind: 'even'; even: boolean } | { kind: 'auto'; auto: boolean }
export type FtRuntimeContext = { settings: FtSettingsContext; skipTx1: boolean }
export type FtRuntimeChange = { kind: 'rxOffset'; hz: number } | { kind: 'skipTx1'; on: boolean }
export type StationAction =
  | { action: 'ft.runtime'; expectedTier: 'FT8' | 'FT4'; transmitEpoch: string; expected: FtRuntimeContext; change: FtRuntimeChange }
  | { action: 'ft.setting'; expectedTier: 'FT8' | 'FT4'; transmitEpoch: string; expected: FtSettingsContext; change: FtSettingChange }
  | { action: 'qso.logCurrent'; expectedKey: string; expectedTier: string; expectedQso: FtExchangeContext }
  | { action: 'qso.confirm'; expectedKey: string; edits: PendingLogEdits }
  | { action: 'qso.discard'; expectedKey: string }
  | { action: 'ft.message'; expectedTier: 'FT8' | 'FT4'; transmitEpoch: string; expectedQso: FtExchangeContext; call: string; grid: string | null; text: string }
  | { action: 'ft.exchange'; expectedTier: 'FT8' | 'FT4'; transmitEpoch: string; expectedQso: FtExchangeContext; change: FtExchangeChange }
  | { action: 'ft.call'; expectedTier: 'FT8' | 'FT4'; transmitEpoch: string; selection: FtCallSelection }
  | { action: 'ft.cq'; expectedTier: 'FT8' | 'FT4'; transmitEpoch: string; direction: string | null }
  | { action: 'ft.txEnabled'; expectedTier: 'FT8' | 'FT4'; transmitEpoch: string; on: boolean }
  | { action: 'radio.level'; mode: 'digital' | 'phone' | 'cw' | 'rtty' | 'keyboard'; level: RadioLevel; expected: number; value: number }
  | { action: 'radio.workSpot'; mode: 'cw' | 'phone'; dialMhz: number; band: string; call: string }
  // RTTY Work names itself, for the same reason: an older desktop parses workSpot's mode against
  // cw/phone exactly, so a third word there would be refused rather than understood.
  | { action: 'radio.workRttySpot'; dialMhz: number; band: string; call: string }
  // FT8/FT4 Work names its tier in its own action: an older desktop parses workSpot exactly.
  | { action: 'radio.workDigitalSpot'; tier: 'FT8' | 'FT4'; dialMhz: number; band: string; call: string }
  // An FM repeater: the output, the shift and offset the rig keys (0 = band convention) and the tone.
  | { action: 'radio.repeater'; outputMhz: number; shift: 'simplex' | 'plus' | 'minus'; offsetHz: number; toneHz: number }
  // The APRS channel pick: one of the regional 2 m APRS channels, FM simplex at the station.
  | { action: 'radio.aprsTune'; dialMhz: number }
  // ⭐ The satellite section, by operator gesture only. Arming a track is the one gesture in the
  // app that leaves the station steering the dial, the split and the mast by itself for minutes,
  // so it is a click and never a timer, an alarm or a spot — exactly as on the desktop — and the
  // track the station arms for it ends when this browser does (station side: `SatTrack`).
  | { action: 'satellite.track'; name: string; aosUnix: number }
  | { action: 'satellite.stopTrack' }
  // The transponder handed to the Doppler engine, by its raw row in the list the `satellite`
  // detail page returned; null hands the dial back. `auto` marks a pick the "Work this pass"
  // chain made rather than a click on a card, which the station judges differently mid-pass.
  | { action: 'satellite.transponder'; name: string; index: number | null; auto: boolean }
  // The readiness rail's two fixes: the Doppler switch, and the uplink VFO mapping together with
  // the consent that it is the mapping for the radio Doppler would drive. A null `map` confirms
  // the mapping already in force — the page's copy of it is poll-time state, so the station
  // resolves it at write time; `radioId` names the rig the page's rail SHOWED, so a radio switch
  // between the poll and the click can never consent for a rig the operator never saw named.
  | { action: 'satellite.doppler'; on: boolean }
  | { action: 'satellite.uplinkMap'; map: SatVfoMap | null; radioId: number | null }
  // Peg-lock from the satellite radio-binding line: the app-wide "don't auto-switch radios".
  | { action: 'satellite.peg'; on: boolean }
  // "Update elements": one manual TLE refresh attempt. The station keeps every policy gate.
  | { action: 'satellite.elements' }
  // The rotator, by operator gesture only: an azimuth, a callsign's entity, or stop.
  | { action: 'rotator.point'; azimuthDeg: number }
  | { action: 'rotator.pointAtCall'; call: string }
  | { action: 'rotator.stop' }
  // ⛔ Delete one received SSTV picture, permanently. The station finds the row; this page names
  // only what its own gallery row showed, never a path.
  | { action: 'sstv.deleteImage'; finishedUtc: string; mode: string }
  // The native panadapter: one closed setting and exactly its own field. The station judges which
  // scope family is live, never this page.
  | { action: 'radio.scope'; setting: 'span'; hz: number }
  | { action: 'radio.scope'; setting: 'ref'; tenthsDb: number }
  | { action: 'radio.scope'; setting: 'position'; position: 'center' | 'cursor' | 'fix' }
  | { action: 'radio.scope'; setting: 'panSpan'; hz: number }
  | { action: 'radio.scope'; setting: 'panRef'; refDbm: number | null }
  // A station memory: its section and exact dial, plus its own sideband (Phone) or its FM machine.
  | { action: 'radio.memoryRecall'; section: 'cw' | 'phone' | 'digital'; dialMhz: number; band: string; sideband: 'USB' | 'LSB' | null
      fm?: { shift: 'simplex' | 'plus' | 'minus'; offsetHz: number; toneHz: number } }
  | { action: 'radio.frequency'; dialMhz: number; band: string; sideband: 'USB' | 'LSB' | 'FM' | 'AM' }
  | { action: 'radio.band'; band: string; mode: 'cw' | 'phone' }
  | { action: 'radio.filterWidth'; mode: 'cw' | 'phone'; expectedHz: number; hz: number }
  | { action: 'radio.function'; mode: 'cw' | 'phone'; func: ReceiverFunction; expectedOn: boolean; on: boolean }
  | { action: 'radio.agc'; mode: 'cw' | 'phone'; expectedSpeed: AgcSpeed; speed: AgcSpeed }
  | { action: 'radio.phoneMode'; expectedMode: PhoneMode; mode: PhoneMode }
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
  | { action: 'decoder.aiCw'; expectedOn: boolean; on: boolean }
  | { action: 'decoder.redecode'; expectedTier: 'FT8' | 'FT4' }
  | { action: 'radio.split'; expectedTxMhz: number | null; txMhz: number | null }
  | { action: 'radio.xit'; expectedHz: number; hz: number }
  | { action: 'radio.vfo'; expectedVfo: 'A' | 'B'; vfo: 'A' | 'B' }
  | { action: 'radio.rit'; expectedHz: number; hz: number }
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
export const CONTROL_CAPABILITIES = ['ftRuntime', 'ftSettings', 'qsoLogging', 'ftOperate', 'ftCall', 'ftExchange', 'ftMessages', 'decoder', 'radio', 'amplifier', 'frequency', 'mode', 'tier', 'ampFollowBand', 'workspace', 'decoderSettings', 'receiverSettings', 'receiverGain', 'bandSelection', 'receiverFilter', 'receiverDsp', 'phoneMode', 'workSpot', 'radioLevels', 'radioSelection', 'fmTuning', 'fmReceiver',
  // Remote parity batch 1. A station advertises each one only with its action, so an older
  // station never names them and this page never sends their actions to it.
  'aiCw', 'redecode', 'rigScope', 'workDigitalSpot', 'splitTuning', 'ritTuning', 'repeaterTuning', 'memoryRecall', 'aprsTuning', 'rotator',
  // The parity leftovers. Same rule: a station advertises each one only with its action.
  'workRttySpot', 'sstvGallery',
  // The satellite section: one hint for the whole of it, because its verbs are one operator act.
  // Deliberately NOT in TX_IDLE_CAPABILITIES: an operator works an FM bird by transmitting DURING
  // the pass the same controls armed, and the rail's Stop — which ends that pass — must stay live
  // exactly when a transmission is armed rather than going dead at the moment it is wanted most.
  'satellite',
  // Listening to the station's receive audio. The one hint here that names no ACTION: it
  // is answered on the audio lane, not by a station control, so it appears in no action
  // map. It rides this list because it is given under the same station-control grant, and
  // because a station that does not name it must never be offered the control.
  'audioListen'] as const
/** The hints added after operation v3 froze — batch 1 and the parity leftovers after it. An older
 * page does not know these names and drops them as hints. */
export const TUNE_CAPABILITIES = ['aiCw', 'redecode', 'rigScope', 'workDigitalSpot', 'splitTuning', 'ritTuning', 'repeaterTuning', 'memoryRecall', 'aprsTuning', 'rotator', 'workRttySpot', 'sstvGallery', 'satellite'] as const satisfies readonly ControlCapability[]
/** Batch-1 controls that move the transmit frequency, start a retune or feed the FT sequencer.
 * They stay disabled while this browser's transmission is armed; the station refuses them too. */
export const TX_IDLE_CAPABILITIES = ['redecode', 'workDigitalSpot', 'workRttySpot', 'splitTuning', 'repeaterTuning', 'memoryRecall', 'aprsTuning'] as const satisfies readonly ControlCapability[]
export type ControlCapability = (typeof CONTROL_CAPABILITIES)[number]
// A new action cannot silently inherit a broader capability by its prefix.
const ACTION_CAPABILITY: Record<StationAction['action'], ControlCapability> = {
  'qso.logCurrent': 'qsoLogging', 'qso.confirm': 'qsoLogging', 'qso.discard': 'qsoLogging',
  'ft.runtime': 'ftRuntime',
  'ft.setting': 'ftSettings',
  'ft.message': 'ftMessages',
  'ft.exchange': 'ftExchange',
  'ft.call': 'ftCall', 'ft.cq': 'ftOperate', 'ft.txEnabled': 'ftOperate',
  'radio.level': 'radioLevels',
  'radio.disarm': 'radio', 'radio.select': 'radioSelection', 'radio.frequency': 'frequency',
  'radio.band': 'bandSelection', 'radio.mode': 'mode', 'radio.tier': 'tier', 'radio.workspace': 'workspace',
  'radio.filterWidth': 'receiverFilter',
  'radio.function': 'receiverDsp', 'radio.agc': 'receiverDsp',
  'radio.phoneMode': 'phoneMode', 'radio.workSpot': 'workSpot', 'radio.workDigitalSpot': 'workDigitalSpot',
  'radio.workRttySpot': 'workRttySpot',
  'radio.repeater': 'repeaterTuning',
  'radio.aprsTune': 'aprsTuning',
  'rotator.point': 'rotator', 'rotator.pointAtCall': 'rotator', 'rotator.stop': 'rotator',
  'satellite.track': 'satellite', 'satellite.stopTrack': 'satellite', 'satellite.transponder': 'satellite',
  'satellite.doppler': 'satellite', 'satellite.uplinkMap': 'satellite', 'satellite.peg': 'satellite',
  'satellite.elements': 'satellite',
  'sstv.deleteImage': 'sstvGallery',
  'radio.scope': 'rigScope',
  'radio.memoryRecall': 'memoryRecall',
  'decoder.arm': 'decoder', 'decoder.clear': 'decoder', 'decoder.afcReset': 'decoder',
  'decoder.net': 'decoder', 'decoder.pskMode': 'decoder',
  'decoder.aiCw': 'aiCw', 'decoder.redecode': 'redecode',
  // Split, XIT and VFO decide where the transmitter goes; RIT only moves the receiver.
  'radio.split': 'splitTuning', 'radio.xit': 'splitTuning', 'radio.vfo': 'splitTuning', 'radio.rit': 'ritTuning',
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
// The regional 2 m APRS channels in Hz, channel for channel the cockpit picker's list (aprsBeacon.ts).
// Literal here: this grammar is compiled into the relay, which must not pull in the grid code.
const APRS_CHANNELS_HZ = [144390000, 144800000, 145175000, 144575000, 144660000, 144930000, 145570000]
const invalid = (): never => { throw Error('invalidOperation') }
function object(raw: unknown, keys: string[]): Record<string, unknown> {
  try { return displayObject(raw, keys) } catch { return invalid() }
}
const oneOf = (v: unknown, choices: readonly string[]) => typeof v === 'string' && choices.includes(v)
/** A bird name as the wire admits it: the page's own schedule row, bounded and printable. Naming
 * the bird is the station's job — it resolves the string against its element set and its alias
 * table, so a name this admits is still refused there if no such bird exists. */
const satelliteName = (v: unknown) => typeof v === 'string' && v.length >= 1 && v.length <= 64 && !/[^ -~]/.test(v)
/** An FM machine: a closed shift, a whole offset (0 = band convention) and a tone that is 0 (none)
 * or in the CTCSS range to a tenth of a hertz. */
const repeaterMachine = (shift: unknown, offsetHz: unknown, toneHz: unknown) =>
  oneOf(shift, ['simplex', 'plus', 'minus']) && Number.isSafeInteger(offsetHz) && (offsetHz as number) >= 0 && (offsetHz as number) <= 20000000 &&
  finite(toneHz) && (toneHz === 0 || (toneHz >= 60 && toneHz <= 260 && Math.abs(Math.round(toneHz * 10) - toneHz * 10) <= 1e-6))

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

function exchangeContext(raw: unknown): void {
  const q = object(raw, ['dxcall', 'state', 'txNow', 'cqRunning'])
  if (typeof q.state !== 'string' || !q.state || q.state.length > 32 || /[^ -~]/.test(q.state) || typeof q.cqRunning !== 'boolean' ||
        (q.dxcall !== null && (typeof q.dxcall !== 'string' || !/^[A-Z0-9/]{3,32}$/.test(q.dxcall))) ||
        (q.txNow !== null && (typeof q.txNow !== 'string' || q.txNow.length > 128 || /[^ -~]/.test(q.txNow)))) invalid()
}

/** Every rig-scope span the cockpits offer: the Icom CI-V chips (± half-width) and the FT-710
 * rungs (half of the full span), nothing between. The station checks the live family's own ladder. */
const SCOPE_SPANS_HZ = [500, 1_000, 2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000]
const SCOPE_FIELD: Record<string, string> = { span: 'hz', ref: 'tenthsDb', position: 'position', panSpan: 'hz', panRef: 'refDbm' }
export function stationAction(raw: unknown): StationAction {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const a = raw as Record<string, unknown>
  switch (a.action) {
    case 'ft.runtime': case 'ft.setting': {
      object(a, ['action', 'expectedTier', 'transmitEpoch', 'expected', 'change'])
      if (!oneOf(a.expectedTier, ['FT8', 'FT4']) || typeof a.transmitEpoch !== 'string' || !/^[0-9a-f]{16}$/.test(a.transmitEpoch)) invalid()
      const runtime = a.action === 'ft.runtime' ? object(a.expected, ['settings', 'skipTx1']) : null
      if (runtime && typeof runtime.skipTx1 !== 'boolean') invalid()
      const e = object(runtime ? runtime.settings : a.expected, ['key', 'txOffsetHz', 'rxOffsetHz', 'holdTxFreq', 'txEven', 'txCycleAuto'])
      if (typeof e.key !== 'string' || !/^[0-9a-f]{32}$/.test(e.key) || !finite(e.txOffsetHz) || !finite(e.rxOffsetHz) ||
        typeof e.holdTxFreq !== 'boolean' || typeof e.txEven !== 'boolean' || typeof e.txCycleAuto !== 'boolean') invalid()
      if (!a.change || typeof a.change !== 'object') invalid()
      const kind = (a.change as Record<string, unknown>).kind
      const field = kind === 'txOffset' || kind === 'bothOffsets' || kind === 'rxOffset' ? 'hz' : kind === 'hold' || kind === 'skipTx1' ? 'on' : kind === 'even' ? 'even' : 'auto'
      const c = object(a.change, ['kind', field])
      if (!oneOf(kind, runtime ? ['rxOffset', 'skipTx1'] : ['txOffset', 'bothOffsets', 'hold', 'even', 'auto'])) invalid()
      if (field === 'hz' ? !finite(c.hz) || c.hz < 200 || c.hz > 4000 : typeof c[field] !== 'boolean') invalid()
      break
    }
    case 'qso.logCurrent': case 'qso.confirm': case 'qso.discard': {
      object(a, ['action', 'expectedKey', ...(a.action === 'qso.logCurrent' ? ['expectedTier', 'expectedQso'] : a.action === 'qso.confirm' ? ['edits'] : [])])
      if (typeof a.expectedKey !== 'string' || !(a.action === 'qso.logCurrent' ? /^[0-9a-f]{32}$/ : /^[0-9a-f]{16}$/).test(a.expectedKey)) invalid()
      if (a.action === 'qso.logCurrent') {
        if (!oneOf(a.expectedTier, CONTROL_TIERS)) invalid()
        exchangeContext(a.expectedQso)
      } else if (a.action === 'qso.confirm') {
        const e = object(a.edits, ['call', 'grid', 'rstSent', 'rstRcvd'])
        if (typeof e.call !== 'string' || !/^[A-Z0-9/]{3,32}$/.test(e.call) ||
          (e.grid !== null && (typeof e.grid !== 'string' || !/^[A-Za-z0-9]{0,16}$/.test(e.grid)))) invalid()
        for (const k of ['rstSent', 'rstRcvd']) if (e[k] !== null && (typeof e[k] !== 'string' || (e[k] as string).length > 16 || /[^ -~]/.test(e[k] as string))) invalid()
      }
      break
    }
    case 'ft.message': case 'ft.exchange': {
      object(a, ['action', 'expectedTier', 'transmitEpoch', 'expectedQso', ...(a.action === 'ft.message' ? ['call', 'grid', 'text'] : ['change'])])
      if (!oneOf(a.expectedTier, ['FT8', 'FT4']) || typeof a.transmitEpoch !== 'string' || !/^[0-9a-f]{16}$/.test(a.transmitEpoch)) invalid()
      exchangeContext(a.expectedQso)
      if (a.action === 'ft.message') {
        if (typeof a.call !== 'string' || !/^[A-Z0-9/]{3,32}$/.test(a.call) ||
          (a.grid !== null && (typeof a.grid !== 'string' || !/^[A-Za-z0-9]{0,16}$/.test(a.grid))) ||
          typeof a.text !== 'string' || !a.text.trim() || a.text.length > 128 || /[^ -~]/.test(a.text)) invalid()
        break
      }
      if (!a.change || typeof a.change !== 'object') invalid()
      const kind = (a.change as Record<string, unknown>).kind
      const change = object(a.change, ['kind', ...(kind === 'freeText' ? ['text'] : [])])
      if (!oneOf(kind, ['resend', 'monitor', 'freeText'])) invalid()
      if (kind === 'freeText' && (typeof change.text !== 'string' || !change.text.trim() || change.text.length > 13 || /[^ -~]/.test(change.text))) invalid()
      break
    }
    case 'ft.call': {
      object(a, ['action', 'expectedTier', 'transmitEpoch', 'selection'])
      if (!oneOf(a.expectedTier, ['FT8', 'FT4']) || typeof a.transmitEpoch !== 'string' || !/^[0-9a-f]{16}$/.test(a.transmitEpoch)) invalid()
      const s = object(a.selection, ['call', 'grid', 'message', 'snr', 'freq'])
      if (typeof s.call !== 'string' || !/^[A-Z0-9/]{3,32}$/.test(s.call) ||
        (s.grid !== null && (typeof s.grid !== 'string' || !/^[A-Za-z0-9]{0,16}$/.test(s.grid))) ||
        (s.message !== null && (typeof s.message !== 'string' || !s.message || s.message.length > 128 || /[^ -~]/.test(s.message))) ||
        (s.snr !== null && (!finite(s.snr) || !Number.isInteger(s.snr) || s.snr < -2147483648 || s.snr > 2147483647)) ||
        (s.freq !== null && !finite(s.freq))) invalid()
      if (s.message === null ? s.snr !== null : s.grid !== null || s.snr === null || s.freq === null) invalid()
      break
    }
    case 'ft.cq': case 'ft.txEnabled':
      object(a, ['action', 'expectedTier', 'transmitEpoch', a.action === 'ft.cq' ? 'direction' : 'on'])
      if (!oneOf(a.expectedTier, ['FT8', 'FT4']) || typeof a.transmitEpoch !== 'string' || !/^[0-9a-f]{16}$/.test(a.transmitEpoch)) invalid()
      if (a.action === 'ft.cq') {
        if (a.direction !== null && (typeof a.direction !== 'string' || !/^(?:[A-Z]{1,4}|[0-9]{3})$/.test(a.direction))) invalid()
      } else if (typeof a.on !== 'boolean') invalid()
      break
    case 'radio.workSpot':
      object(a, ['action', 'mode', 'dialMhz', 'band', 'call'])
      if (!oneOf(a.mode, ['cw', 'phone']) || !finite(a.dialMhz) || a.dialMhz <= 0 || a.dialMhz > 250000 || !oneOf(a.band, BANDS) || typeof a.call !== 'string' || !/^[A-Za-z0-9/]{1,32}$/.test(a.call)) invalid()
      break
    case 'radio.workRttySpot':
      object(a, ['action', 'dialMhz', 'band', 'call'])
      if (!finite(a.dialMhz) || a.dialMhz <= 0 || a.dialMhz > 250000 || !oneOf(a.band, BANDS) || typeof a.call !== 'string' || !/^[A-Za-z0-9/]{1,32}$/.test(a.call)) invalid()
      break
    case 'radio.workDigitalSpot':
      object(a, ['action', 'tier', 'dialMhz', 'band', 'call'])
      if (!oneOf(a.tier, ['FT8', 'FT4']) || !finite(a.dialMhz) || a.dialMhz <= 0 || a.dialMhz > 250000 || !oneOf(a.band, BANDS) || typeof a.call !== 'string' || !/^[A-Za-z0-9/]{1,32}$/.test(a.call)) invalid()
      break
    case 'radio.frequency':
      object(a, ['action', 'dialMhz', 'band', 'sideband'])
      if (!finite(a.dialMhz) || a.dialMhz <= 0 || a.dialMhz > 250000 || (a.band !== '' && !oneOf(a.band, BANDS)) || !oneOf(a.sideband, ['USB', 'LSB', 'FM', 'AM'])) invalid()
      break
    case 'radio.band':
      object(a, ['action', 'band', 'mode'])
      if (!oneOf(a.band, BANDS) || !oneOf(a.mode, ['cw', 'phone'])) invalid()
      break
    case 'radio.level':
      object(a, ['action', 'mode', 'level', 'expected', 'value'])
      if (!oneOf(a.mode, ['digital', 'phone', 'cw', 'rtty', 'keyboard']) || !oneOf(a.level, RADIO_LEVELS) ||
        !finite(a.expected) || !finite(a.value) || a.expected < 0 ||
        (a.level === 'notch' ? a.value < 300 || a.value > 3400 : a.expected > 1 || a.value < 0 || a.value > 1)) invalid()
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
    case 'radio.phoneMode':
      object(a, ['action', 'expectedMode', 'mode'])
      if (!oneOf(a.expectedMode, PHONE_MODES) || !oneOf(a.mode, PHONE_MODES)) invalid()
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
    case 'decoder.aiCw':
      object(a, ['action', 'expectedOn', 'on'])
      if (typeof a.expectedOn !== 'boolean' || typeof a.on !== 'boolean' || a.expectedOn === a.on) invalid()
      break
    case 'decoder.redecode':
      object(a, ['action', 'expectedTier'])
      if (!oneOf(a.expectedTier, ['FT8', 'FT4'])) invalid()
      break
    case 'radio.memoryRecall': {
      object(a, ['action', 'section', 'dialMhz', 'band', 'sideband', ...('fm' in a ? ['fm'] : [])])
      // Only a Phone memory names a sideband; an FM machine is Phone voice at or above 29 MHz.
      if (!oneOf(a.section, ['cw', 'phone', 'digital']) || !finite(a.dialMhz) || a.dialMhz <= 0 || a.dialMhz > 250000 || !oneOf(a.band, BANDS) ||
        (a.sideband !== null && (a.section !== 'phone' || !oneOf(a.sideband, ['USB', 'LSB'])))) invalid()
      if ('fm' in a) {
        const machine = object(a.fm, ['shift', 'offsetHz', 'toneHz'])
        if (a.section !== 'phone' || a.sideband !== null || (a.dialMhz as number) < 29 || !repeaterMachine(machine.shift, machine.offsetHz, machine.toneHz)) invalid()
      }
      break
    }
    case 'radio.repeater': {
      object(a, ['action', 'outputMhz', 'shift', 'offsetHz', 'toneHz'])
      // FM voice starts at 29 MHz.
      if (!finite(a.outputMhz) || a.outputMhz < 29 || a.outputMhz > 250000 || !repeaterMachine(a.shift, a.offsetHz, a.toneHz)) invalid()
      break
    }
    case 'radio.aprsTune':
      object(a, ['action', 'dialMhz'])
      // Only a regional APRS channel: this is the APRS pick, never a general 2 m tune.
      if (!finite(a.dialMhz) || !APRS_CHANNELS_HZ.includes(Math.round((a.dialMhz as number) * 1e6))) invalid()
      break
    case 'radio.scope': {
      const field = typeof a.setting === 'string' && Object.prototype.hasOwnProperty.call(SCOPE_FIELD, a.setting) ? SCOPE_FIELD[a.setting] : null
      if (!field) invalid()
      object(a, ['action', 'setting', field as string])
      const whole = (v: unknown, lo: number, hi: number) => Number.isSafeInteger(v) && (v as number) >= lo && (v as number) <= hi
      if (a.setting === 'span' ? !Number.isSafeInteger(a.hz) || !SCOPE_SPANS_HZ.includes(a.hz as number)
        : a.setting === 'ref' ? !whole(a.tenthsDb, -200, 200)
        : a.setting === 'position' ? !oneOf(a.position, ['center', 'cursor', 'fix'])
        : a.setting === 'panSpan' ? !whole(a.hz, 5_000, 14_000_000)
        : a.refDbm !== null && !whole(a.refDbm, -160, 20)) invalid()
      break
    }
    case 'rotator.point':
      object(a, ['action', 'azimuthDeg'])
      // An azimuth the rotctld line carries exactly: 0 ≤ az < 360, to a tenth of a degree.
      if (!finite(a.azimuthDeg) || a.azimuthDeg < 0 || a.azimuthDeg >= 360 || Math.abs(a.azimuthDeg * 10 - Math.round(a.azimuthDeg * 10)) > 1e-6) invalid()
      break
    case 'rotator.pointAtCall':
      object(a, ['action', 'call'])
      if (typeof a.call !== 'string' || !/^[A-Z0-9/]{3,32}$/.test(a.call)) invalid()
      break
    case 'rotator.stop':
      object(a, ['action'])
      break
    case 'satellite.track':
      object(a, ['action', 'name', 'aosUnix'])
      // The station resolves the bird against its own element set (aliases included) and refuses
      // anything it cannot name; this only keeps the wire value bounded and printable. `aosUnix`
      // is the schedule row's own AOS — the station matches a pass to it within ±3 min.
      if (!satelliteName(a.name) || !integer(a.aosUnix) || a.aosUnix < 1 || a.aosUnix > 253402300799) invalid()
      break
    case 'satellite.stopTrack': case 'satellite.elements':
      object(a, ['action'])
      break
    case 'satellite.transponder':
      object(a, ['action', 'name', 'index', 'auto'])
      // `index` indexes the list the detail page returned, dead rows included — the station
      // indexes that same list and refuses a dead pick by name rather than shifting past it.
      // null hands the dial back. The cap is the wire's, not the bird's: the station knows how
      // many rows it has.
      if (!satelliteName(a.name) || typeof a.auto !== 'boolean' ||
        (a.index !== null && (!integer(a.index) || a.index > 4095))) invalid()
      break
    case 'satellite.doppler': case 'satellite.peg':
      object(a, ['action', 'on'])
      if (typeof a.on !== 'boolean') invalid()
      break
    case 'satellite.uplinkMap':
      object(a, ['action', 'map', 'radioId'])
      // null map = confirm the mapping already in force, which the station resolves at write time.
      // null radioId = the radio active at the station then; a number is the rig the rail NAMED.
      if ((a.map !== null && !oneOf(a.map, SAT_VFO_MAPS)) ||
        (a.radioId !== null && (!integer(a.radioId) || a.radioId > 0xffffffff))) invalid()
      break
    case 'sstv.deleteImage':
      object(a, ['action', 'finishedUtc', 'mode'])
      // Exactly the two display fields a gallery row carries, bounded; the station matches them
      // against its own gallery and refuses anything but one row.
      if (typeof a.finishedUtc !== 'string' || !a.finishedUtc || a.finishedUtc.length > 64 || /[^ -~]/.test(a.finishedUtc) ||
        typeof a.mode !== 'string' || !a.mode || a.mode.length > 80 || /[^ -~]/.test(a.mode)) invalid()
      break
    case 'radio.split':
      // null is simplex. Both values are always named: a missing one is not simplex by omission.
      object(a, ['action', 'expectedTxMhz', 'txMhz'])
      for (const v of [a.expectedTxMhz, a.txMhz]) if (v !== null && (!finite(v) || v <= 0 || v > 250000)) invalid()
      if (a.expectedTxMhz === a.txMhz) invalid()
      break
    case 'radio.xit': case 'radio.rit':
      object(a, ['action', 'expectedHz', 'hz'])
      // Offsets are signed: the shared `integer` helper is non-negative by design.
      if (!Number.isSafeInteger(a.expectedHz) || !Number.isSafeInteger(a.hz) || (a.expectedHz as number) < -9999 || (a.expectedHz as number) > 9999 || (a.hz as number) < -9999 || (a.hz as number) > 9999 || a.expectedHz === a.hz) invalid()
      break
    case 'radio.vfo':
      object(a, ['action', 'expectedVfo', 'vfo'])
      if (!oneOf(a.expectedVfo, ['A', 'B']) || !oneOf(a.vfo, ['A', 'B']) || a.expectedVfo === a.vfo) invalid()
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
  | { outcome: 'applied'; evidence: 'receiverState' | 'radioReadback' | 'amplifierReadback' | 'stationState' | 'settingsSaved' | 'fileSynced' | 'pendingConfirmationSynced' | 'pendingDiscarded' }
  | { outcome: 'rejected' | 'unknown'; reason: string }
)
const REASONS = ['authorityExpired', 'contextChanged', 'readingUnavailable', 'stationBusy', 'hardwareUnavailable', 'hardwareUnconfirmed', 'unsupportedAction', 'invalidAction', 'persistenceFailed', 'noEligibleContact', 'alreadyPresent', 'pendingConfirmationRequired',
  // Only batch-1 transmit-frequency actions emit this, so a page that predates it never receives it.
  'outsidePrivileges']
export function controlOutcome(raw: unknown): ControlOutcome {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) invalid()
  const v = raw as Record<string, unknown>
  object(v, ['operation', 'operationId', 'outcome', ...(v.outcome === 'applied' ? ['evidence'] : v.outcome === 'pending' ? [] : ['reason'])])
  if (v.operation !== 'stationControl' || typeof v.operationId !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v.operationId)) invalid()
  if (v.outcome === 'applied') {
    if (!oneOf(v.evidence, ['receiverState', 'radioReadback', 'amplifierReadback', 'stationState', 'settingsSaved', 'fileSynced', 'pendingConfirmationSynced', 'pendingDiscarded'])) invalid()
  } else if (v.outcome !== 'pending' && (!oneOf(v.outcome, ['rejected', 'unknown']) || !oneOf(v.reason, REASONS))) invalid()
  return raw as ControlOutcome
}
