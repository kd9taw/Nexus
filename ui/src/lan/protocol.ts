// The Stations on this network window's page and this computer's Nexus, on the loopback origin's
// one socket (src-tauri/src/lan_client/origin.rs: its header is the contract). The page's words go
// as JSON; what it is told is read here, strictly, and anything else is refused. The station's own
// messages on an open road (operation answers, stream signals and state, the status line) pass
// through as the station sent them, to the code that reads them on the hosted road too.

/** A paired station as the page is shown it: its LAN station id, where it answered (the last that
 *  worked first), its key's fingerprint (SHA-256 of the key, lowercase hex), and the address or host
 *  name Operate tries first, when one is kept. Never this computer's key. */
export type LanStation = { id: string; addresses: string[]; key: string; typedAddress?: string }

/** A station found by name on this computer's networks (`tempo_stream::lan::dnssd`): the name it
 *  advertises, the address it listens at, the LAN protocol it speaks and its key tag (the first 64
 *  bits of its key's fingerprint). A hint for the address field, never an identity: the road takes
 *  only the key pinned at pairing. */
export type LanFound = { name: string; address: string; protocol: number; key: string }
/** One of the Remote account's stations, offered while signing in. */
export type LanChoice = { id: string; name: string }
/** A station the internet road has been added to, by its LAN station id, and this computer's key for
 *  that road: its fingerprint (SHA-256 of the key, lowercase hex), the one the station shows when it
 *  asks to approve this computer. Never the private key. */
export type LanInternet = { id: string; key: string }
/** The station's key an internet road is pinned to, by the id the station is kept under here, as
 *  the station shows its own ("This station's key"): its fingerprint (SHA-256 of the key, lowercase
 *  hex). `held`: the road carries nothing until the operator checks that key against the station's
 *  screen (`stationKeyUnconfirmed`), or for good, the station having named another key on its own
 *  network (`stationKeyDisagrees`). `codePaired`: the station is paired here with a code, as the
 *  credential store says (`Stations::code_paired`), so It doesn't match removes the internet road
 *  alone and its own network can check the key. */
export type LanStationKey = { id: string; key: string; held: 'stationKeyUnconfirmed' | 'stationKeyDisagrees' | null; codePaired: boolean }

/** A station added with the account: until when it takes this computer on its network (seconds
 *  since 1970), and whether to sign in again for it now (`lan_client::account`). */
export type LanSigned = { stationId: string; until: number; renew: boolean }

/** A station signed in to from elsewhere, kept by the service's id for it until this computer first
 *  reaches it on its network (`lan_client::Stations::keep_away`): that id, its name as the account
 *  listed it, and its LAN key's fingerprint (SHA-256 of the key, lowercase hex). */
export type LanAway = { id: string; name: string; key: string }

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
  'noAnswer', 'nameUnresolved'] as const
export type ConnectReason = (typeof CONNECT_REASONS)[number]

/** Why an open road closed. */
export const CLOSED_REASONS = ['stationLeft', 'connectionLost', 'disconnected'] as const
export type ClosedReason = (typeof CLOSED_REASONS)[number]

/** The station's answer did not hold here, and the page was never handed it (S3-M1). */
export const ANSWER_REFUSALS = ['stationKeyMismatch', 'stationNotSigned'] as const

/** Why a sign-in added nothing (`lan_client::account`). `signInRefused` is the operator's own
 *  cancel; `emailUnverified`, the issuer refusing an address not confirmed yet
 *  (`tempo_client::native::callback`). A reason this page does not know reads as `signInFailed`: a
 *  sign-in that ended is always said to have ended. */
export const SIGN_IN_REASONS = ['signInPortBusy', 'signInTimedOut', 'signInRefused', 'emailUnverified', 'signInFailed', 'signInUnderWay',
  'accountNotReady', 'internetNotReady', 'internetNotIncluded', 'serviceUnavailable', 'issuerUnavailable', 'tryLater',
  'rateLimited', 'accessDenied', 'notApprovedThere', 'waitingApproval', 'trialDisabled', 'deviceLimit',
  'stationUnavailable', 'stationKeyMissing', 'stationNotChosen', 'stationsFull', 'storeUnavailable', 'unknownStation',
  'alreadyHere', 'listenOnly', 'stationNotProven', 'stationProofFailed', 'notOnThisNetwork', 'notThisStation',
  'stationNotSignedIn', 'proofNotYours', 'proofRefused', 'pairingFull', 'stationCouldNotKeep', 'updateStation',
  'updateComputer', 'keyChanged', 'notStation', 'stationKeyDisagrees'] as const
export type SignInReason = (typeof SIGN_IN_REASONS)[number]

/** Why the station did not take this computer at the address last tried, while a sign-in asks where
 *  it is: nothing answered, another key did, or what was typed is no address or no name found. */
export const WHERE_REASONS = ['unreachable', 'otherNetwork', 'refused', 'noAnswer', 'keyChanged', 'badAddress',
  'nameUnresolved'] as const
export type WhereReason = (typeof WHERE_REASONS)[number]

/** The station's own messages, passed through on an open road. */
const STATION_MESSAGES = ['operationResponse', 'streamSignal', 'streamState', 'status'] as const

export type Told =
  /** `internet`: the stations the internet road has been added to, each with this computer's key
   *  for it. `account`: the stations added with the account. `away`: the stations kept by their
   *  service id until this computer first reaches them on their network. `forgetUnreached`: after a
   *  forget, why Nexus Remote could not be told to end this computer's approval there.
   *  `dropUnreached`: the same, after the internet road alone was removed and the pairing kept. */
  | { type: 'stations'; stations: LanStation[]; internet: LanInternet[]; account: LanSigned[]; away: LanAway[]; stationKeys?: LanStationKey[]; computer: string; error: string | null; forgetUnreached: string | null; dropUnreached?: string }
  | { type: 'found'; shacks: LanFound[]; available: boolean }
  | { type: 'paired'; station: LanStation }
  | { type: 'pairRefused'; reason: PairReason }
  | { type: 'connected'; road: LanRoad }
  | { type: 'connectRefused'; reason: ConnectReason }
  | { type: 'closed'; reason: ClosedReason }
  | { type: 'answerRefused'; reason: (typeof ANSWER_REFUSALS)[number] }
  /** Operate from this computer was taken: Nexus is closing, to start again driving the station. */
  | { type: 'operating' }
  /** Signing in: the sign-in waits in the system browser. */
  | { type: 'signingIn' }
  /** Signed in for a station; `waiting` while its internet road waits for Approve at the station;
   *  `away` when it was found nowhere on this network and is kept by its service id; `key`, this
   *  computer's key for the station's internet road, which the station shows when it asks. */
  | { type: 'signedIn'; stationId: string; waiting: boolean; away: boolean; key: string }
  | { type: 'signInRefused'; reason: SignInReason }
  /** Signing in: the account has more than one station, and the operator picks one. */
  | { type: 'chooseStation'; stations: LanChoice[] }
  /** Signing in: the station was found nowhere on this network, and the operator says where it is;
   *  `name` as the account lists it (empty for one on this computer), and why the last address
   *  tried did not take this computer, if one was. */
  | { type: 'stationAddress'; name: string; reason: WhereReason | null }
  | { type: 'station'; message: Record<string, unknown> }

type Fields = Record<string, unknown>
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const HEX64 = /^[0-9a-f]{64}$/
const SPKI = /^3059301306072a8648ce3d020106082a8648ce3d03010703420004[0-9a-f]{128}$/
/** A look by name keeps at most this many records, so it never finds more stations. */
const MOST_FOUND = 256
/** The most stations offered at once (`lan_client::internet`). */
const MOST_CHOICES = 16

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
/** An address or a host name with its port, as `tempo_stream::lan::Typed` writes one. */
const TYPED = /^[0-9a-z.-]+:[0-9]+$/
function station(raw: unknown): LanStation {
  const typed = (raw as Fields | null)?.typedAddress !== undefined ? ['typedAddress'] : []
  const value = fields(raw, ['id', 'addresses', 'key', ...typed])
  if (!Array.isArray(value.addresses) || value.addresses.length < 1 || value.addresses.length > 4) throw Error('invalidLanMessage')
  const shown: LanStation = { id: text(value.id, UUID), addresses: value.addresses.map(a => text(a, /^[0-9.]+:[0-9]+$/, 21)), key: text(value.key, HEX64) }
  return typed.length ? { ...shown, typedAddress: text(value.typedAddress, TYPED, 259) } : shown
}
function internetRoad(raw: unknown): LanInternet {
  const value = fields(raw, ['id', 'key'])
  return { id: text(value.id, UUID), key: text(value.key, HEX64) }
}
const HELD = ['stationKeyUnconfirmed', 'stationKeyDisagrees'] as const
function stationKey(raw: unknown): LanStationKey {
  const held = (raw as Fields | null)?.held !== undefined ? ['held'] : []
  // Said only of a station paired here with a code (`Stations::code_paired`), and only as true.
  const coded = (raw as Fields | null)?.codePaired !== undefined ? ['codePaired'] : []
  const value = fields(raw, ['id', 'key', ...held, ...coded])
  if (coded.length && value.codePaired !== true) throw Error('invalidLanMessage')
  return { id: text(value.id, UUID), key: text(value.key, HEX64), held: held.length ? oneOf(HELD, value.held) : null, codePaired: coded.length > 0 }
}
function signed(raw: unknown): LanSigned {
  const value = fields(raw, ['stationId', 'until', 'renew'])
  if (typeof value.until !== 'number' || !Number.isInteger(value.until) || value.until < 0 || typeof value.renew !== 'boolean') throw Error('invalidLanMessage')
  return { stationId: text(value.stationId, UUID), until: value.until, renew: value.renew }
}
function choice(raw: unknown): LanChoice {
  const value = fields(raw, ['id', 'name'])
  return { id: text(value.id, UUID), name: text(value.name, undefined, 96) }
}
function away(raw: unknown): LanAway {
  const value = fields(raw, ['id', 'name', 'key'])
  return { id: text(value.id, UUID), name: text(value.name, undefined, 96), key: text(value.key, HEX64) }
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
      const optional = ['internet', 'account', 'away', 'stationKeys', 'error', 'forgetUnreached', 'dropUnreached'].filter(key => (raw as Fields)[key] !== undefined)
      const value = fields(raw, ['type', 'stations', 'computer', ...optional])
      if (!Array.isArray(value.stations) || value.stations.length > 8) throw Error('invalidLanMessage')
      const internet = value.internet === undefined ? [] : value.internet
      if (!Array.isArray(internet) || internet.length > 8) throw Error('invalidLanMessage')
      const account = value.account === undefined ? [] : value.account
      if (!Array.isArray(account) || account.length > 8) throw Error('invalidLanMessage')
      const kept = value.away === undefined ? [] : value.away
      if (!Array.isArray(kept) || kept.length > 8) throw Error('invalidLanMessage')
      // Told by a Nexus that holds roads until their station's key is checked; left out otherwise.
      const keys = value.stationKeys
      if (keys !== undefined && (!Array.isArray(keys) || keys.length > 8)) throw Error('invalidLanMessage')
      return { type, stations: value.stations.map(station), internet: internet.map(internetRoad),
        account: account.map(signed), away: kept.map(away), ...(Array.isArray(keys) ? { stationKeys: keys.map(stationKey) } : {}), computer: text(value.computer, undefined, 64),
        error: value.error === undefined ? null : text(value.error),
        forgetUnreached: value.forgetUnreached === undefined ? null : text(value.forgetUnreached),
        ...(value.dropUnreached === undefined ? {} : { dropUnreached: text(value.dropUnreached) }) }
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
    case 'operating': fields(raw, ['type']); return { type }
    case 'signingIn': fields(raw, ['type']); return { type }
    case 'signedIn': {
      const value = fields(raw, ['type', 'stationId', 'waiting', 'away', 'key'])
      if (typeof value.waiting !== 'boolean' || typeof value.away !== 'boolean') throw Error('invalidLanMessage')
      return { type, stationId: text(value.stationId, UUID), waiting: value.waiting, away: value.away, key: text(value.key, HEX64) }
    }
    case 'signInRefused': {
      const reason = text(fields(raw, ['type', 'reason']).reason)
      return { type, reason: (SIGN_IN_REASONS as readonly string[]).includes(reason) ? reason as SignInReason : 'signInFailed' }
    }
    case 'stationAddress': {
      const optional = (raw as Fields).reason !== undefined ? ['reason'] : []
      const value = fields(raw, ['type', 'name', ...optional])
      return { type, name: text(value.name, undefined, 96), reason: optional.length ? oneOf(WHERE_REASONS, value.reason) : null }
    }
    case 'chooseStation': {
      const value = fields(raw, ['type', 'stations'])
      if (!Array.isArray(value.stations) || value.stations.length < 2 || value.stations.length > MOST_CHOICES) throw Error('invalidLanMessage')
      return { type, stations: value.stations.map(choice) }
    }
    default: throw Error('invalidLanMessage')
  }
}

/** Does `typed` have the shape of a pairing code: sixteen hexadecimal characters, either case, with
 *  spaces anywhere? This computer's Nexus reads the code itself (`lan_client::code`); this only says
 *  whether Pair is worth pressing. */
export function codeShaped(typed: string): boolean {
  return /^[0-9a-f]{16}$/i.test(typed.replace(/\s+/g, ''))
}

/** The socket on this page's own origin: the same path as the page, the launch secret in it. */
export function socketUrl(page: string): string {
  const url = new URL('socket', page)
  url.protocol = 'ws:'
  url.search = ''
  url.hash = ''
  return url.toString()
}
