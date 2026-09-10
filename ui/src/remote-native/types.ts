export type RemoteStationStatus = {
  phase: 'unpaired' | 'pairing' | 'approval' | 'disabled' | 'connecting' | 'connected' | 'reconnecting'
  origin: string; stationId: string | null; accountId: string | null
  pairingId: string | null; pairingCode: string | null; expiresAt: number | null
  devices: { id: string; name: string; approved: number; expiresAt: number }[]
  loggingPermissions?: string[]
  loggingController?: string | null
  observationGeneration?: string | null
  error: string | null
}
export type RemoteStationAction =
  | { type: 'begin'; name: string }
  | { type: 'refresh' | 'cancel' | 'enable' | 'disable' | 'forget' }
  | { type: 'approve'; enrollmentId: string; accountId: string }
  | { type: 'loggingPermission'; deviceId: string; allow: boolean }
  | { type: 'takeOverLogging' }
  | { type: 'device'; deviceId: string; approve: boolean }
