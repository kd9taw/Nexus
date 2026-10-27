export type RemoteStationStatus = {
  phase: 'unpaired' | 'pairing' | 'approval' | 'disabled' | 'connecting' | 'connected' | 'reconnecting'
  origin: string; stationId: string | null; accountId: string | null
  pairingId: string | null; pairingCode: string | null; expiresAt: number | null
  /** `renewsUntil` is the end that use cannot move; null (or absent) for an approval that never renews.
   *  `key` is the SHA-256 of the browser's listed device key (A5), lowercase hex, when it has one.
   *  `kind`: a browser, or Nexus itself on another computer or a phone (the native client), whose
   *  approval pins its key and grants nothing on the relay's lanes; absent from an older station, where
   *  every device is a browser. `platform`: a native device's, none for a browser. `untilRemoved`: 1 for an
   *  approval that lasts until it is removed here (every approval since 2026-10-10); absent from an older
   *  station or service. */
  devices: { id: string; name: string; approved: number; expiresAt: number; generation?: number | null; renewsUntil?: number | null
    key?: string | null; kind?: DeviceKind; platform?: DevicePlatform | null; untilRemoved?: number
    /** Listed to a station that checks proofs itself: what the device may do, and how it was added. */
    role?: AccountRole | null; addedBy?: 'shack' | 'account' | null }[]
  stationPermissions?: string[]
  /** The device holding the station's lease, if any. */
  controller?: string | null
  /** A5: the browsers whose listed key is the one pinned here, the ones that can stream. */
  pinnedDevices?: string[]
  /** The devices added by their own sign-in that the station holds from transmit now: each shown with Allow
   *  to transmit. Absent from an older station. */
  heldDevices?: string[]
  /** S3-M1: the service holds another signing key for this station, so browsers refuse its stream
   *  until it is paired again. */
  keyRefused?: boolean
  /** S3-L1: SHA-256 of this station's own key, lowercase hex, while it has one: shown as "This
   *  station's key", the way the Remote page shows the key it kept for the station. */
  stationKey?: string | null
  observationGeneration?: string | null
  error: string | null
  /** Remote over this network: absent only from a build without it. */
  lan?: LanStatus
  /** The station's account: signing in at the shack, and the devices it granted on their own proof. */
  account?: AccountStatus
  /** DISPLAY ONLY, for the shack's one Devices list: each device once, by its ids in `devices` and in
   *  `lan.devices` (what a Remove on it takes together), whose it is (an invited person's address, or not
   *  known for one added by a sign-in the station has not told), whether it holds the station now, and the
   *  seconds left of its transmit hold where the remote PC is shown them. Absent from an older station. */
  deviceRows?: { listed: string[]; lan: string[]; guest?: string | null; unknown: boolean; live: boolean; holdLeft?: number | null }[]
}
/** Remote over this network (`Status.lan`): the shack's own listener, its paired computers and its
 *  pairing window. `reason`: why it went off by itself, or why it is on and not listening
 *  (`addressGone`: the picked address is not this computer's right now, and is waited for). */
export type LanStatus = {
  on: boolean
  /** Where it listens, `address:port`. */
  listening?: string
  reason?: 'noKey' | 'endedAtShack' | 'addressGone' | 'chooseAddress' | 'portInUse' | 'noNetwork' | 'unavailable'
  /** While on, the networks to pick from: this computer's private addresses on adapters that are
   *  not tunnels or virtual ones, each with its adapter's name. */
  networks?: { address: string; name: string }[]
  /** The address the operator picked, if any. */
  picked?: string
  /** While listening: Windows advertises the station by name (`true`) or will not (`false`). */
  named?: boolean
  /** While listening, what stands in the way in Windows' firewall on that network. */
  firewall?: 'blocksAll' | 'blocked' | 'managed' | 'public' | 'silent' | 'ask'
  /** The LAN key's fingerprint: SHA-256 of its SPKI, lowercase hex. */
  key?: string
  /** The pairing window while it is open: its code (sixteen lowercase hex characters) and when it
   *  closes, in milliseconds since 1970. */
  pairing?: { code: string; closesAt: number }
  /** Each paired computer: its device id, the name it gave, and its key's fingerprint. */
  devices: { id: string; name: string; key: string }[]
}
/** A device's kind and a native device's platform, as the station lists them (the service's own values). */
export type AccountRole = 'operate' | 'listen'
export type AccountStatus = {
  signingIn: boolean
  /** The key that signed the station's own proof was retired: sign in again to keep adding devices. */
  reproof?: boolean
  /** Devices granted on their own proof since Nexus started, until dismissed: the notice at the shack. */
  added: { deviceId: string; name: string; kind: DeviceKind; platform?: DevicePlatform | null; key?: string | null
    role: AccountRole; guest: boolean }[]
  /** Devices the service approved on their own proof that the station granted nothing, each with why: a proof it
   *  could not confirm, or `approveAtStation` ([`waitsForApprove`]). */
  unconfirmed: Record<string, string>
  /** Devices removed here that the service has not heard of yet. */
  removals: string[]
  /** The people invited: each address as the service matches it, and a role. */
  people: { mailbox: string; role: AccountRole }[]
  /** The computers the last invitation taken back took off this network beside that guest's own: guests'
   *  computers the station could not tell whose they were, by name. */
  alsoUninvited?: string[]
  /** What a Remove leaves behind does not read: nothing is granted on a sign-in, and the LAN takes no computer, until
   *  Reset network identity. */
  removalsUnreadable?: boolean
}
export type DeviceKind = 'browser' | 'computer' | 'phone'
export type DevicePlatform = 'windows' | 'macos' | 'linux' | 'ios' | 'android'
/** The platforms by name: proper nouns, the same in every language. */
export const PLATFORM_NAMES: Record<DevicePlatform, string> = { windows: 'Windows', macos: 'macOS', linux: 'Linux', ios: 'iOS', android: 'Android' }
/** Nexus on another computer or a phone (the native client), not a browser. */
export const isNative = (device: { kind?: DeviceKind }) => device.kind === 'computer' || device.kind === 'phone'
/** A device added by its own sign-in, whose proof the station checked, waiting for Approve here: the station grants
 *  nothing on a sign-in alone (`remote_service::account::GRANTS_ON_SIGN_IN`). The service lists it as approved, and it
 *  asks as a device added at the station asks; the station never approved it. */
export const waitsForApprove = (status: RemoteStationStatus | null | undefined, deviceId: string) =>
  status?.account?.unconfirmed?.[deviceId] === 'approveAtStation'
/** A device added by its own sign-in that the station holds from transmit (`heldDevices`), offered Allow to transmit
 *  at the shack: only where it is asked nothing else. Never one waiting for Approve or with its key not pinned here (the
 *  Approve waives the hold itself), one on a proof the station could not confirm, or one removed here. The card's
 *  button and the shack's pop-up both read it, so they offer the same act for the same devices. */
export const offersAllowTransmit = (status: RemoteStationStatus | null | undefined, device: RemoteStationStatus['devices'][number]) =>
  !!status?.heldDevices?.includes(device.id) && device.approved === 1 && !(device.key && !status.pinnedDevices?.includes(device.id))
    && !status.account?.unconfirmed?.[device.id] && !status.account?.removals?.includes(device.id)
export type RemoteStationAction =
  | { type: 'begin'; name: string }
  | { type: 'refresh' | 'cancel' | 'enable' | 'disable' | 'forget' }
  | { type: 'approve'; enrollmentId: string; accountId: string }
  | { type: 'stationPermission'; deviceId: string; allow: boolean }
  | { type: 'takeOverLogging' }
  /** `key`: the device key shown beside the browser, pinned if the service still lists it (A5). `kind`: the
   *  kind it was shown as; the station grants and pins nothing for a device the service now names another. */
  | { type: 'device'; deviceId: string; approve: boolean; key?: string; kind?: DeviceKind }
  /** Remote over this network: only ever pressed at the shack (`LanStation`). */
  | { type: 'lanOn'; address?: string; port?: number }
  /** Where it listens: one of `networks`' addresses, or none to let the station choose. */
  | { type: 'lanAddress'; address?: string }
  | { type: 'lanOff' | 'lanPair' | 'lanCancel' | 'lanReset' }
  | { type: 'lanRevoke'; deviceId: string }
  /** Sign in to Nexus Remote at the shack: binds the station named `name` to the account, with no code. */
  | { type: 'signIn'; name: string }
  /** Sign in again, for a station bound already: its proof is replaced, so devices can check it again. */
  | { type: 'signInAgain' }
  /** End signing in again; unlike `cancel`, which is a stop, it leaves Remote as it was. */
  | { type: 'cancelSignInAgain' }
  | { type: 'dismissAdded'; deviceId: string }
  /** Allow to transmit: lifts the hold on a device added by its own sign-in. Only ever pressed at the shack. */
  | { type: 'allowTransmit'; deviceId: string }
  | { type: 'invite'; email: string; role: AccountRole }
  | { type: 'uninvite'; email: string }
