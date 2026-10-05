// The Stations on this network window's page and this computer's Nexus, on the loopback origin's
// one socket (src-tauri/src/lan_client/origin.rs: its header is the contract). The page's words go
// as JSON; what it is told is read here, strictly, and anything else is refused. The station's own
// messages on an open road (operation answers, stream signals and state, the status line) pass
// through as the station sent them, to the code that reads them on the hosted road too.

/** A paired station as the page is shown it: its LAN station id, where it answered (the last that
 *  worked first), and its key's fingerprint (SHA-256 of the key, lowercase hex). Never this
 *  computer's key. */
export type LanStation = { id: string; addresses: string[]; key: string }

/** A station found by name on this computer's networks (`tempo_stream::lan::dnssd`): the name it
 *  advertises, the address it listens at, the LAN protocol it speaks and its key tag (the first 64
 *  bits of its key's fingerprint). A hint for the address field, never an identity: the road takes
 *  only the key pinned at pairing. */
export type LanFound = { name: string; address: string; protocol: number; key: string }

/** The road, open: the ids the station stamped and the key this computer pinned for it. */
export type LanRoad = { stationId: string; deviceId: string; sessionId: string; stationKey: string; address: string }

/** Why a pairing did not happen, as this computer's Nexus names it. Nothing answering at the address
 *  is told as `tempo_stream::lan::unreached` names it (`otherNetwork`, `refused`, `noAnswer`). */
export const PAIR_REASONS = ['badAddress', 'badCode', 'badName', 'unreachable', 'pairingClosed', 'wrongCode',
  'stationProofFailed', 'pairingFull', 'stationUnavailable', 'updateStation', 'updateComputer', 'stationsFull',
  'storeUnavailable', 'notStation', 'unavailable', 'otherNetwork', 'refused', 'noAnswer'] as const
export type PairReason = (typeof PAIR_REASONS)[number]

/** Why the road did not open. Nothing answering at the addresses tried is told as
 *  `tempo_stream::lan::unreached` names it, the most telling of them: `refused`, then `otherNetwork`,
 *  then `noAnswer`. `notThisStation`: only a station found by name answered, with a key this
 *  computer did not pair with. */
export const CONNECT_REASONS = ['badAddress', 'unreachable', 'keyChanged', 'notThisStation', 'notPaired',
  'updateStation', 'updateComputer', 'notStation', 'storeUnavailable', 'unknownStation', 'otherNetwork', 'refused',
  'noAnswer'] as const
export type ConnectReason = (typeof CONNECT_REASONS)[number]

/** Why an open road closed. */
export const CLOSED_REASONS = ['stationLeft', 'connectionLost', 'disconnected'] as const
export type ClosedReason = (typeof CLOSED_REASONS)[number]

/** The station's answer did not hold here, and the page was never handed it (S3-M1). */
export const ANSWER_REFUSALS = ['stationKeyMismatch', 'stationNotSigned'] as const

/** The station's own messages, passed through on an open road. */
const STATION_MESSAGES = ['operationResponse', 'streamSignal', 'streamState', 'status'] as const

export type Told =
  | { type: 'stations'; stations: LanStation[]; computer: string; error: string | null }
  | { type: 'found'; shacks: LanFound[]; available: boolean }
  | { type: 'paired'; station: LanStation }
  | { type: 'pairRefused'; reason: PairReason }
  | { type: 'connected'; road: LanRoad }
  | { type: 'connectRefused'; reason: ConnectReason }
  | { type: 'closed'; reason: ClosedReason }
  | { type: 'answerRefused'; reason: (typeof ANSWER_REFUSALS)[number] }
  | { type: 'station'; message: Record<string, unknown> }

type Fields = Record<string, unknown>
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const HEX64 = /^[0-9a-f]{64}$/
const SPKI = /^3059301306072a8648ce3d020106082a8648ce3d03010703420004[0-9a-f]{128}$/
/** A look by name keeps at most this many records, so it never finds more stations. */
const MOST_FOUND = 256

function fields(raw: unknown, keys: string[]): Fields {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error('invalidLanMessage')
  const value = raw as Fields
  const present = Object.keys(value)
  if (present.length !== keys.length || !present.every(key => keys.includes(key))) throw Error('invalidLanMessage')
  return value
}
function oneOf<T extends string>(list: readonly T[], value: unknown): T {
  if (typeof value !== 'string' || !(list as readonly string[]).includes(value)) throw Error('invalidLanMessage')
  return value as T
}
function text(value: unknown, pattern?: RegExp, max = 64): string {
  if (typeof value !== 'string' || value.length > max || (pattern && !pattern.test(value))) throw Error('invalidLanMessage')
  return value
}
function station(raw: unknown): LanStation {
  const value = fields(raw, ['id', 'addresses', 'key'])
  if (!Array.isArray(value.addresses) || value.addresses.length < 1 || value.addresses.length > 4) throw Error('invalidLanMessage')
  return { id: text(value.id, UUID), addresses: value.addresses.map(a => text(a, /^[0-9.]+:[0-9]+$/, 21)), key: text(value.key, HEX64) }
}
function found(raw: unknown): LanFound {
  const value = fields(raw, ['name', 'address', 'protocol', 'key'])
  if (typeof value.protocol !== 'number' || !Number.isInteger(value.protocol) || value.protocol < 0) throw Error('invalidLanMessage')
  return { name: text(value.name, /./), address: text(value.address, /^[0-9.]+:[0-9]+$/, 21), protocol: value.protocol, key: text(value.key, /^[0-9a-f]{16}$/) }
}

/** What the page was told, read strictly; throws on anything this computer's Nexus never says. */
export function readTold(raw: unknown): Told {
  const type = raw && typeof raw === 'object' ? (raw as Fields).type : undefined
  if ((STATION_MESSAGES as readonly unknown[]).includes(type)) return { type: 'station', message: raw as Fields }
  switch (type) {
    case 'stations': {
      const value = (raw as Fields).error === undefined ? fields(raw, ['type', 'stations', 'computer']) : fields(raw, ['type', 'stations', 'computer', 'error'])
      if (!Array.isArray(value.stations) || value.stations.length > 8) throw Error('invalidLanMessage')
      return { type, stations: value.stations.map(station), computer: text(value.computer, undefined, 64), error: value.error === undefined ? null : text(value.error) }
    }
    case 'found': {
      const value = fields(raw, ['type', 'shacks', 'available'])
      if (!Array.isArray(value.shacks) || value.shacks.length > MOST_FOUND || typeof value.available !== 'boolean') throw Error('invalidLanMessage')
      return { type, shacks: value.shacks.map(found), available: value.available }
    }
    case 'paired': return { type, station: station(fields(raw, ['type', 'station']).station) }
    case 'pairRefused': return { type, reason: oneOf(PAIR_REASONS, fields(raw, ['type', 'reason']).reason) }
    case 'connected': {
      const value = fields(raw, ['type', 'stationId', 'deviceId', 'sessionId', 'stationKey', 'address'])
      return { type, road: { stationId: text(value.stationId, UUID), deviceId: text(value.deviceId, UUID), sessionId: text(value.sessionId, UUID),
        stationKey: text(value.stationKey, SPKI, 182), address: text(value.address, /^[0-9.]+:[0-9]+$/, 21) } }
    }
    case 'connectRefused': return { type, reason: oneOf(CONNECT_REASONS, fields(raw, ['type', 'reason']).reason) }
    case 'closed': return { type, reason: oneOf(CLOSED_REASONS, fields(raw, ['type', 'reason']).reason) }
    case 'answerRefused': return { type, reason: oneOf(ANSWER_REFUSALS, fields(raw, ['type', 'reason']).reason) }
    default: throw Error('invalidLanMessage')
  }
}

/** Does `typed` have the shape of a pairing code: sixteen hexadecimal characters, either case, with
 *  spaces anywhere? This computer's Nexus reads the code itself (`lan_client::code`); this only says
 *  whether Pair is worth pressing. */
export function codeShaped(typed: string): boolean {
  return /^[0-9a-f]{16}$/i.test(typed.replace(/\s+/g, ''))
}

/** The first 128 bits of a key's fingerprint, in eight groups of four, as the station shows its own. */
export function groupedKey(key: string): string {
  return (key.slice(0, 32).match(/.{4}/g) ?? []).join(' ')
}

/** The socket on this page's own origin: the same path as the page, the launch secret in it. */
export function socketUrl(page: string): string {
  const url = new URL('socket', page)
  url.protocol = 'ws:'
  url.search = ''
  url.hash = ''
  return url.toString()
}
