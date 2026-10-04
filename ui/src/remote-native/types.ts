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
