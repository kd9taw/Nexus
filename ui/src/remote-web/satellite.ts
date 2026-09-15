// The satellite section's gestures from a browser. Reading a pass, a schedule, a bird's detail and
// the live track already work over Remote (the `satellites`/`satellite` documents and the
// `get_remote_satellite_state` sample); this is the half that ACTS.
//
// ⛔ **A TRACK MOVES THE RADIO AND THE MAST.** Arming one is the only gesture in the app that
// leaves the station steering by itself for minutes, so it stays what it is on the desktop: an
// operator's click, never a timer, an alarm, a spot or a reconnect. Three things end it and each
// is pinned by a test — the rail's own Stop, the session's Stop, and this browser going away (the
// station drops a remotely armed track when the authority that armed it stops being held).
//
// Nothing here decides what the station will do. Every verb is judged again at the station, which
// owns the consent gates: a transmit VFO is written only under a mapping the operator confirmed
// FOR THE RADIO IN PLAY, a dead transponder row is refused, and elements past the acting ceiling
// will not arm. Disabling a control here is a courtesy; it is never the permission.
import { createContext, useContext } from 'react'
import { RemoteOperationsContext, useStationCapability, useStationControl } from '../stationAccess'
import type { StationAction } from './station-operation'
import { RemoteCollectionsContext } from './collections'
import { loadNavigation } from './navigation'
import type { SatelliteDetailData } from './navigation'
import type { SatDetail } from '../types'

export type SatelliteControl = {
  /** The desktop, where every satellite control is the operator's by definition. */
  local: boolean
  /** Draw the control as live. Local always; over Remote, this browser holds station control and
   * the station advertises the satellite section. */
  allowed: boolean
  /**
   * Send one satellite gesture. Resolves when the station has answered and the answer was
   * `applied`; rejects otherwise — a refusal the station named, or a command that never left.
   *
   * ⚠️ It reports the STATION's answer and nothing more. What the radio then did is read back the
   * way the desktop reads it back: the live sample's track and binding, which the rail already
   * polls. A pick the engine refused returns `applied` here and says so in the binding note,
   * exactly as it does at the shack — the command succeeded, the tune did not.
   */
  send: (action: StationAction) => Promise<void>
}

/**
 * One bird's detail page, fetched ONCE for a gesture rather than watched.
 *
 * "Work this pass" needs the bird's transmitter list and its element age before it picks and
 * arms, for a bird the operator has only just clicked — the watched document is still on its way,
 * and arming without the list is the defect the chain exists to avoid (a pass whose Doppler has
 * nothing to tune, with every gate in the rail showing green). This is the browser's
 * `getSatDetail`: the same document the section already reads, read on demand.
 */
export function useSatelliteDetail(): (name: string) => Promise<SatDetail> {
  const source = useContext(RemoteCollectionsContext)
  return async (name: string) => {
    if (!source) throw new Error('stationUnavailable')
    const doc = await loadNavigation<SatelliteDetailData>(source, 'satellite', name, () => true)
    // The station answers for the bird it was asked about or the answer is not usable: the
    // transmitter list is indexed by position, so a list belonging to another bird would pick a
    // different uplink, silently.
    if (doc.value.name !== name) throw new Error('invalidNavigation')
    return doc.value.detail
  }
}

/** Whether the satellite controls in this subtree may act, published once by the section rather
 * than threaded through it — the shape `StationControlContext` already has, and for the same
 * reason: the readiness rail, the radio binding and the lock-on pill all ask the one question, and
 * none of them is given what it takes to answer it. Defaults to the desktop's answer, so a rail
 * rendered outside the section behaves exactly as it always has. */
export const SatelliteControlContext = createContext(true)
export function useSatelliteAllowed(): boolean {
  return useContext(SatelliteControlContext)
}

/**
 * The satellite gesture surface.
 *
 * ⚠️ It deliberately asks for NO hardware observation. Every other control that does — the
 * amplifier strip, the RX-gain slider — is a control that names a radio and must not act on a
 * stale reading of it. These do not: the station resolves the radio itself (a pick routes on
 * band and mode class, a mapping is consented for the rig the rail NAMED), and it re-checks its
 * own context before acting. Requiring a current observation here would buy nothing and would
 * cost the one thing that must never go dead — the rail's Stop, disabled by a late monitor poll
 * at exactly the moment the operator reaches for it.
 */
export function useSatelliteControl(): SatelliteControl {
  const local = useStationControl()
  const allowed = useStationCapability('satellite')
  const client = useContext(RemoteOperationsContext)
  return {
    local,
    // Over Remote a control is live only while there is something to send it through; a button
    // that reports a failure it could have predicted is worse than one plainly unavailable.
    allowed: local || (allowed && !!client),
    send: async (action: StationAction) => {
      if (!client) throw new Error('stationUnavailable')
      const outcome = await client.control(action)
      if (outcome.outcome !== 'applied') throw new Error(outcome.outcome === 'pending' ? 'operationUnknown' : outcome.reason)
    },
  }
}
