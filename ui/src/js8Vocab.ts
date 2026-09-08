// The JS8 cockpit's INVARIANT vocabulary and its pure helpers — the `pskModes.ts` pattern.
//
// Everything here is a technical token or a number: the mode's own name, the JS8Call submode
// names and their ALL.TXT letters, the 32 directed-command texts and the 8 CQ variants (both
// transcribed from JS8Call's varicode.cpp as PROTOCOL FACTS — credited in NOTICE; the Rust
// side, js8::proto::command, is the wire authority and this table must agree with it), and
// invariant formatters. None of it is prose and none of it goes through the catalog
// (i18n/index.ts, the invariant-token rule): a translated command name would not be answered.
import type { Js8Speed } from './types'

export const JS8 = 'JS8'
export const HB = 'HB'
export const CQ = 'CQ'
export const ALLCALL = '@ALLCALL'
export const HZ = 'Hz'
export const RX_PLATE = 'RX ▼'
export const TX_PLATE = 'TX ▲'

/** JS8Call's own names for its three persisted automatic-origin switches — the chips carry
 *  them verbatim (a JS8Call operator reads AUTOREPLY as a control name, not as a word). */
export const AUTOREPLY = 'AUTOREPLY'
export const RELAY = 'RELAY'
export const HB_ACK = 'HB ACK'

export interface Js8SpeedInfo {
  key: Js8Speed
  /** Index into `Speed::ALL` — the `js8_speed` setting and `js8_set_speed`'s argument. */
  idx: number
  /** The submode's own name (Slow/Normal/Fast/Turbo — an invariant token). */
  label: string
  /** ALL.TXT submode letter (E/A/B/C). */
  letter: string
  periodS: number
  /** §97.119 airtime cap: frames × period < 600 s (compose::max_frames). */
  maxFrames: number
}

export const JS8_SPEEDS: Record<Js8Speed, Js8SpeedInfo> = {
  slow: { key: 'slow', idx: 0, label: 'Slow', letter: 'E', periodS: 30, maxFrames: 19 },
  normal: { key: 'normal', idx: 1, label: 'Normal', letter: 'A', periodS: 15, maxFrames: 39 },
  fast: { key: 'fast', idx: 2, label: 'Fast', letter: 'B', periodS: 10, maxFrames: 59 },
  turbo: { key: 'turbo', idx: 3, label: 'Turbo', letter: 'C', periodS: 6, maxFrames: 99 },
}
/** Display / index order (= Speed::ALL). */
export const JS8_SPEED_LIST: readonly Js8SpeedInfo[] = [
  JS8_SPEEDS.slow, JS8_SPEEDS.normal, JS8_SPEEDS.fast, JS8_SPEEDS.turbo,
]

/** The CQ variants a heartbeat-with-isAlt frame carries, by `cqs` index (varicode.cpp:283-292). */
export const JS8_CQS: readonly string[] = [
  'CQ CQ CQ', 'CQ DX', 'CQ QRP', 'CQ CONTEST', 'CQ FIELD', 'CQ FD', 'CQ CQ', 'CQ',
]

/** The 32 directed commands, id = wire value, text = JS8Call's exact wire text with its
 *  leading space (varicode.cpp:46-84). `label` is the trimmed text for a button. */
export interface Js8Command {
  id: number
  text: string
  label: string
}
const CMD_TEXTS: readonly string[] = [
  ' SNR?', ' DIT DIT', ' NACK', ' HEARING?', ' GRID?', '>', ' STATUS?', ' STATUS', ' HEARING',
  ' MSG', ' MSG TO:', ' QUERY', ' QUERY MSGS', ' QUERY CALL', ' ACK', ' GRID', ' INFO?', ' INFO',
  ' FB', ' HW CPY?', ' SK', ' RR', ' QSL?', ' QSL', ' CMD', ' SNR', ' NO', ' YES', ' 73',
  ' HEARTBEAT SNR', ' AGN?', ' ',
]
export const JS8_COMMANDS: readonly Js8Command[] = CMD_TEXTS.map((text, id) => ({
  id,
  text,
  label: text.trim(),
}))
/** The five one-click asks on a station row: SNR? GRID? INFO? HEARING? QUERY MSGS. */
export const JS8_QUICK_QUERIES: readonly Js8Command[] = [0, 4, 16, 3, 12].map((id) => JS8_COMMANDS[id])

/** JS8Call's SNR rendering: sign always, two digits (`+07`, `-12`). */
export function fmtSnr(db: number): string {
  const v = Math.max(-60, Math.min(60, Math.round(db)))
  const mag = String(Math.abs(v)).padStart(2, '0')
  return `${v < 0 ? '-' : '+'}${mag}`
}

/** "4s" / "2m" / "2h" — a station's age. Units are technical tokens. */
export function ageLabel(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m`
  return `${Math.floor(m / 60)}h`
}

/** HH:MM:SS UTC from an epoch-ms stamp — the activity row's clock (invariant, never locale). */
export function utcClock(ms: number): string {
  return new Date(ms).toISOString().slice(11, 19)
}

export function countBits(mask: number): number {
  let n = 0
  for (let m = mask & 0xff; m; m >>= 1) n += m & 1
  return n
}

/**
 * The composer's pre-send frame estimate — an APPROXIMATION of `js8::proto::compose::frames`
 * (the Rust side is the authority and refuses over the cap): one header frame when the message
 * is directed (a callsign / @ALLCALL / @group, or a command), then the text in data frames at
 * ~10 chars per frame at Normal (the deprecated 70-bit Huffman/JSC form) and ~13 at every
 * other speed (72-bit JSC dense). A plain message is prefixed `MYCALL: ` on the air (spec
 * invariant 10), so those characters count. Real JSC packing depends on the words, which is
 * why this is a hint beside Send and not a gate.
 */
export function estimateFrames(
  to: string,
  cmdId: number | null,
  text: string,
  mycall: string,
  speed: Js8Speed,
): number {
  const body = text.trim()
  const directed = to.trim().length > 0 || cmdId !== null
  if (!directed && body.length === 0) return 0
  const chars = directed ? body.length : `${mycall.trim()}: ${body}`.length
  const perFrame = speed === 'normal' ? 10 : 13
  return (directed ? 1 : 0) + Math.ceil(chars / perFrame)
}
