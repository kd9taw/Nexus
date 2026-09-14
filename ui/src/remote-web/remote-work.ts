// Which needs and spots a browser may Work. CW and Phone ride the station's workSpot intent;
// FT8/FT4 ride workDigitalSpot, which names its tier. Work tunes the station and sets its mode and
// tier; it never starts a QSO or enables transmit.
import { workTarget } from '../features/needs'
import type { BandChannel, NeedAlert, SpotRow } from '../types'

/** The station hints a browser Work needs, plus the cockpit switches the desktop Work obeys. */
export type RemoteWorkGrants = { workSpot: boolean; workDigitalSpot: boolean; cwEnabled: boolean; phoneEnabled: boolean }

/** The FT8/FT4 tier a need names, or null when it names none. */
export function remoteWorkTier(alert: Pick<NeedAlert, 'mode'>): 'FT8' | 'FT4' | null {
  const mode = alert.mode?.toUpperCase()
  return mode === 'FT8' || mode === 'FT4' ? mode : null
}

export function remoteWorkable(alert: NeedAlert, bandPlan: BandChannel[], grants: RemoteWorkGrants): boolean {
  const target = workTarget(alert, bandPlan)
  if (!target) return false
  // Any other digital need (JS8, FT2, a band-level "Digital" row) has no remote transaction.
  if (target.view === 'operate') return grants.workDigitalSpot && remoteWorkTier(alert) !== null
  return grants.workSpot && ((target.view === 'cw' && grants.cwEnabled) || (target.view === 'phone' && grants.phoneEnabled))
}

/** A Spots row in the Needed-board shape the Work path takes. FT8/FT4 keep their submode so the
 * tier travels with the spot. */
export function spotNeed(s: SpotRow): NeedAlert {
  return {
    call: s.call,
    entity: s.entity,
    band: s.band,
    zone: s.zone,
    tags: [],
    priority: 0,
    headline: '',
    mode: s.submode === 'FT4' || s.submode === 'FT8' ? s.submode : s.mode,
    freqMhz: s.freqMhz,
  }
}
