// The JS8 cockpit's INVARIANT vocabulary and its pure helpers — the `pskModes.ts` pattern.
//
// Everything here is a technical token or a number: the mode's own name, the JS8Call submode
// names and their ALL.TXT letters, the 32 directed-command texts and the 8 CQ variants (both
// transcribed from JS8Call's varicode.cpp as PROTOCOL FACTS — credited in NOTICE; the Rust
// side, js8::proto::command, is the wire authority and this table must agree with it), and
// invariant formatters. None of it is prose and none of it goes through the catalog
// (i18n/index.ts, the invariant-token rule): a translated command name would not be answered.
import type { Js8ActivityRow, Js8Heard, Js8InboxEntry, Js8Speed } from './types'

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
  /** `JS8::Submode::rxThreshold`, in Hz: how far a decode may drift and still be the same
   *  signal. 10 is the default (JS8Submode.cpp:62); Fast and Turbo set 16 and 32 (:122-123). */
  rxThresholdHz: number
}

export const JS8_SPEEDS: Record<Js8Speed, Js8SpeedInfo> = {
  slow: { key: 'slow', idx: 0, label: 'Slow', letter: 'E', periodS: 30, maxFrames: 19, rxThresholdHz: 10 },
  normal: { key: 'normal', idx: 1, label: 'Normal', letter: 'A', periodS: 15, maxFrames: 39, rxThresholdHz: 10 },
  fast: { key: 'fast', idx: 2, label: 'Fast', letter: 'B', periodS: 10, maxFrames: 59, rxThresholdHz: 16 },
  turbo: { key: 'turbo', idx: 3, label: 'Turbo', letter: 'C', periodS: 6, maxFrames: 99, rxThresholdHz: 32 },
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
/** GRID, which a station row's one-click "send my grid" sends with the whole locator, as
 *  JS8Call's query menu does (mainwindow.cpp:6656-6668). */
export const JS8_GRID: Js8Command = JS8_COMMANDS[15]
/** The groups JS8Call will not let be joined (`isGroupAllowed`, varicode.cpp:1314-1320). A
 *  message may still be sent to either. */
export const JS8_UNJOINABLE_GROUPS: readonly string[] = ['@APRSIS', '@JS8NET']

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


// ── Band activity BY OFFSET (JS8Call's tableWidgetRXAll, mainwindow.ui:989) ──────────────
//
// JS8Call keeps two decode surfaces: a chronological RX text window and an OFFSET-BUCKETED
// table that answers a different question — "who is where in the passband, and how far off
// the slot clock are they". Nexus's `activity` pane is the first; this is the second, built
// from the same rows so no engine state is added for it.

/** One bucket: the newest decode heard at that offset. */
export interface Js8OffsetRow {
  /** The bucket key — the newest decode's offset, truncated to whole Hz as JS8Call's is. */
  offsetHz: number
  atMs: number
  snrDb: number
  /** Time delta against the slot clock, seconds (JS8Call's "Time Delta", shown in ms). */
  dtS: number
  speed: Js8Speed
  text: string
  directedToMe: boolean
  mine: boolean
  lowConf: boolean
}

/**
 * Collapse a chronological activity feed to ONE row per frequency offset, ordered by offset.
 *
 * JS8Call keeps the last ten *frames* per bucket and joins their text; Nexus's rows arrive
 * already reassembled (`Js8ActivityRow.text` is the display line or the joined message), so
 * joining them again would print the same words twice. The newest row in a bucket therefore
 * wins outright — which is what the table is read for ("latest text per offset").
 *
 * Pure: no clock, no state. `rows` may be in any order; the newest `atMs` wins per bucket and
 * carries the bucket's key, exactly as JS8Call re-keys a bucket on a nearby decode.
 */
export function bandActivityByOffset(rows: readonly Js8ActivityRow[]): Js8OffsetRow[] {
  const buckets = new Map<number, Js8OffsetRow>()
  for (const r of [...rows].sort((a, b) => a.atMs - b.atMs)) {
    // JS8Call's `frequencyOffset()`: the decoder's frequency held in an int, so truncated
    // (decodedtext.cpp:248, decodedtext.h:80).
    const hz = Math.trunc(r.freqHz)
    const row: Js8OffsetRow = {
      offsetHz: hz,
      atMs: r.atMs,
      snrDb: r.snrDb,
      dtS: r.dtS,
      speed: r.speed,
      text: r.text,
      directedToMe: r.directedToMe,
      mine: r.mine,
      lowConf: r.lowConf,
    }
    // A decode at an offset already filed goes there (mainwindow.cpp:3969). One at a new offset
    // takes over the filed offset within its own speed's rxThreshold that `generateOffsets` meets
    // first counting up from offset − range, the lowest in range, and that bucket takes the NEW
    // offset as its key (:3970-3981, :3730-3739).
    if (!buckets.has(hz)) {
      const range = JS8_SPEEDS[r.speed].rxThresholdHz
      for (let prev = hz - range; prev <= hz + range; prev++) {
        if (buckets.delete(prev)) break
      }
    }
    buckets.set(hz, row)
  }
  return [...buckets.values()].sort((a, b) => a.offsetHz - b.offsetHz)
}

/** JS8Call's "Time Delta" face: whole milliseconds of DT (mainwindow.cpp:9936). Units are a
 *  technical token, and the sign matters — an early station reads negative. */
export function dtLabel(dtS: number): string {
  return `${Math.round(dtS * 1000)} ms`
}

/** JS8Call's `Radio::base_callsign` (Radio.cpp:117-133): the longer side of the first '/', the
 *  right-hand side on a tie, upper-cased. */
function js8BaseCall(call: string): string {
  const c = call.trim().toUpperCase(), slash = c.indexOf('/')
  if (slash < 0) return c
  return c.length - slash - 1 >= slash ? c.slice(slash + 1) : c.slice(0, slash)
}

/**
 * The calls with an unread message to me in the inbox — to my call or its base, from that call —
 * which is JS8Call's `m_rxInboxCountCache` as `refreshInboxCounts` counts it (mainwindow.cpp:
 * 9404-9416). JS8Call flags such a station ⚑ and lifts it to the top of its call list
 * (:10199-10207, :10246-10251), and callsign aging keeps it.
 */
export function js8UnreadFrom(inbox: readonly Js8InboxEntry[], myCall: string): Set<string> {
  const me = myCall.trim().toUpperCase()
  const toMe = (to: string) => to !== '' && (to === me || to === js8BaseCall(me))
  return new Set(
    inbox.filter((e) => e.state === 'unread' && toMe(e.to.trim().toUpperCase())).map((e) => e.from),
  )
}

/** JS8Call's "pin messages to the top" (mainwindow.cpp:10199-10207): the stations with an unread
 *  message to me first, the rest after, each keeping the order it had — a stable partition. */
export function js8UnreadFirst<T extends { call: string }>(rows: readonly T[], unreadFrom: ReadonlySet<string>): T[] {
  if (unreadFrom.size === 0) return [...rows]
  return [...rows.filter((r) => unreadFrom.has(r.call)), ...rows.filter((r) => !unreadFrom.has(r.call))]
}

/**
 * The Stations list under JS8Call's callsign aging (mainwindow.cpp:10209-10233). With an aging of
 * `agingMin` minutes (0, JS8Call's default, is off: `CallsignAging`, Configuration.cpp:1853), a
 * call last heard that many whole minutes ago or more (`utcTimestamp.secsTo(now) / 60 >=
 * callsignAging`) is left off, unless it is the selected call or has an unread message to me in
 * the inbox: to my call or its base, from that call (`m_rxInboxCountCache`, :9404-9416). The order
 * is kept, and nothing is dropped from `stations` itself: JS8Call ages its list, not its memory.
 */
export function js8ListedStations(
  stations: readonly Js8Heard[],
  inbox: readonly Js8InboxEntry[],
  o: { agingMin: number; nowMs: number; selectedCall: string; myCall: string },
): Js8Heard[] {
  if (!(o.agingMin > 0)) return [...stations]
  const unreadFrom = js8UnreadFrom(inbox, o.myCall)
  return stations.filter(
    (h) => h.call === o.selectedCall || unreadFrom.has(h.call) || Math.trunc((o.nowMs - h.lastMs) / 60_000) < o.agingMin,
  )
}
