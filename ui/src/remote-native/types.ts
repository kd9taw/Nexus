export type RemoteStationStatus = {
  phase: 'unpaired' | 'pairing' | 'approval' | 'disabled' | 'connecting' | 'connected' | 'reconnecting'
  origin: string; stationId: string | null; accountId: string | null
  pairingId: string | null; pairingCode: string | null; expiresAt: number | null
  /** `renewsUntil` is the end that use cannot move; null (or absent) for an approval that never renews.
   *  `key` is the SHA-256 of the browser's listed device key (A5), lowercase hex, when it has one. */
  devices: { id: string; name: string; approved: number; expiresAt: number; generation?: number | null; renewsUntil?: number | null
    key?: string | null }[]
  loggingPermissions?: string[]
  stationPermissions?: string[]
  transmitPermissions?: string[]
  loggingController?: string | null
  /** A5: the browsers whose listed key is the one pinned here, the ones that can stream. */
  pinnedDevices?: string[]
  /** S3-M1: the service holds another signing key for this station, so browsers refuse its stream
   *  until it is paired again. */
  keyRefused?: boolean
  observationGeneration?: string | null
  error: string | null
  /** Remote over this network: absent only from a build without it. */
  lan?: LanStatus
}
/** Remote over this network (`Status.lan`): the shack's own listener, its paired computers and its
 *  pairing window. `reason`: why it went off by itself, or why it is on and not listening. */
export type LanStatus = {
  on: boolean
  /** Where it listens, `address:port`. */
  listening?: string
  reason?: 'noKey' | 'endedAtShack' | 'addressGone' | 'chooseAddress' | 'portInUse' | 'noNetwork' | 'unavailable'
  /** The LAN key's fingerprint: SHA-256 of its SPKI, lowercase hex. */
  key?: string
  /** The pairing window while it is open: its code (sixteen lowercase hex characters) and when it
   *  closes, in milliseconds since 1970. */
  pairing?: { code: string; closesAt: number }
  /** Each paired computer: its device id, the name it gave, and its key's fingerprint. */
  devices: { id: string; name: string; key: string }[]
}
export type RemoteStationAction =
  | { type: 'begin'; name: string }
  | { type: 'refresh' | 'cancel' | 'enable' | 'disable' | 'forget' }
  | { type: 'approve'; enrollmentId: string; accountId: string; transmit?: boolean }
  | { type: 'loggingPermission'; deviceId: string; allow: boolean }
  | { type: 'stationPermission'; deviceId: string; allow: boolean }
  | { type: 'transmitPermission'; deviceId: string; allow: boolean }
  | { type: 'takeOverLogging' }
  /** `key`: the device key shown beside the browser, pinned if the service still lists it (A5). */
  | { type: 'device'; deviceId: string; approve: boolean; transmit?: boolean; key?: string }
  /** Remote over this network: only ever pressed at the shack (`LanStation`). */
  | { type: 'lanOn'; address?: string; port?: number }
  | { type: 'lanOff' | 'lanPair' | 'lanCancel' | 'lanReset' }
  | { type: 'lanRevoke'; deviceId: string }
