// Config profiles — named full-Settings snapshots so an operator can switch a whole
// rig/antenna/CAT/band setup in one move (home HF ↔ portable VHF ↔ Field Day). Stored
// in localStorage (machine-local, survives restarts); "loading" a profile applies it
// through the normal settings-save path, so there's no separate apply mechanism to drift.

import type { Settings } from './types'
import { durableGet, durableSet } from './features/durableStore'

const KEY = 'nexus.profiles'

/** Bumped when the load/merge contract changes. v1 profiles (no stamp) predate
 * merge-loading; the merge itself makes them safe to load — an absent key keeps
 * the CURRENT value instead of silently taking the struct default, which is how
 * a three-week-old profile used to remove the per-mode power ceilings. */
export const PROFILE_SCHEMA = 2

export interface Profile {
  name: string
  settings: Settings
  /** Schema stamp; absent = saved by a pre-merge build. */
  schema?: number
}

/** Fields a profile NEVER imports: who you are (mycall), what you're licensed
 * for (licenseClass — a TX-safety gate), THIS machine's radio roster and active
 * slot (hardware, not configuration), the connector sync cursors (loading a
 * profile must not re-fetch history), and the satellite uplink pair
 * (satVfoMap + satUplinkRadios). The pair is machine wiring exactly like the
 * roster it describes: the mapping says which VFO of THIS station's radios
 * carries the transmit leg, and the consent list is raw RadioProfile.ids —
 * meaningless against a roster the profile does not bring. Importing either
 * would also bypass the backend `confirm_sat_uplink` verb, the ONE writer
 * whose change-retires-other-consents rule keeps a radio from driving its
 * uplink under a layout nobody confirmed for it (the pair is engine-owned
 * live state — even a whole-settings save cannot carry it). Everything else
 * merges over
 * the current settings, and a key the profile predates (absent) keeps its
 * current value. */
const NEVER_IMPORT: readonly string[] = [
  'mycall',
  'licenseClass',
  'radios',
  'activeRadio',
  'qrzLastSyncUnix',
  'eqslLastSync',
  // The LoTW DOWNLOAD cursor — the third of the same family, and the one that was missing.
  // A cursor means "I already have everything up to here", which is true of the machine that
  // saved the profile and not of this one. Import a NEWER stamp and Nexus skips every
  // confirmation between the two dates, permanently: the next sync asks only for records after
  // the borrowed date, so they never come back on their own. Silent, and it costs award credit
  // the operator already earned.
  'lotwLastQsl',
  // Not a sync cursor but the same hazard: importing a stale "last automatic upload"
  // stamp from another machine makes the next tick immediately due, so loading a profile
  // would fire an unattended TQSL run the operator did not ask for.
  'lotwLastAutoUploadUnix',
  'satVfoMap',
  'satUplinkRadios',
  // The BETA-CHANNEL opt-in. A profile is a whole-STATION snapshot — rig, antenna, CAT, bands
  // — and which builds this box installs is not part of a station: it is a per-machine, per-
  // operator choice, the same family as `mycall` and `licenseClass`. Left importable, loading a
  // profile saved before the operator opted in returned them to the stable channel, and that
  // loss is invisible in a way no other setting's is — no error, no toast, no log line; the
  // betas simply stop arriving and nobody finds out. (The backend keeps the same value across a
  // payload that omits the key; this is the other half — a payload that carries a STALE one.)
  'betaUpdates',
  // The LOGBOOK UPLOAD SWITCHES, every one under Settings ▸ Logging & Connectors ▸ Confirmations
  // (#396; the operator's ruling: profiles never touch them). Where your contacts are sent is not
  // part of a station. Left importable, loading a profile saved while QRZ upload was on turned it
  // back on after the operator had switched it off and forgotten the key, and every contact went
  // to QRZ before it could be corrected; a profile saved before an upload was set up switched it
  // off just as quietly. A new connector's upload switch joins this list.
  'qrzLogbookUpload',
  'clublogUpload',
  'eqslUpload',
  'hrdlogUpload',
  'wrlUpload',
  'cloudlogUpload',
  'lotwAutoUpload',
  // The PUSHES TO THE OTHER LOGGING PROGRAMS on your network, the same family (the operator's
  // ruling: profiles leave them alone too): HRD Logbook forwarding, N3FJP's and N1MM+'s every-QSO
  // pushes, and DXKeeper's own upload switch. Left importable, a profile saved before HRD
  // forwarding was set up stopped it with no error; one saved with it on restarted it beside a
  // JTAlert relay into HRD and every contact was logged twice; and `dxkeeperUploads` turned back
  // on sent every contact to LoTW, eQSL, ClubLog and QRZ a second time. A new local logger's push
  // switch joins this list.
  'hrdLogging',
  'dxkeeperUploads',
  'n3fjpUpload',
  'n1mmUpload',
  // DXKeeper's push has no switch of its own: an empty host is off, so its host is its on/off and
  // stays too. A profile saved before DXKeeper was set up blanked it and the push stopped with no
  // error; one saved with it set started the push again after the operator had cleared it. The
  // other hosts and addresses still travel with a profile, as the upload accounts do (a Field Day
  // profile carries the club's master-log address), and so does the WSJT-X UDP API (`wsjtxUdp`),
  // a feed other programs listen to rather than a push to one logger.
  'dxkeeperHost',
]

/** Merge a stored profile onto the CURRENT settings — the load contract.
 * Replaying the raw blob let serde's absent-key-takes-default behavior remove
 * safety settings the profile predated (measured: any profile saved
 * 2026-06-30…07-22 loaded with no RF power ceiling, at 100% digital duty). */
export function mergeProfile(current: Settings, profile: Settings): Settings {
  const picked: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(profile)) {
    if (NEVER_IMPORT.includes(k)) continue
    if (v !== undefined) picked[k] = v
  }
  return { ...current, ...picked } as Settings
}

/** All saved profiles (name-sorted). Tolerates absent/blocked/corrupt storage → []. */
export function loadProfiles(): Profile[] {
  try {
    const raw = durableGet(KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((p): p is Profile => !!p && typeof p.name === 'string' && !!p.settings)
      .sort((a, b) => a.name.localeCompare(b.name))
  } catch {
    return []
  }
}

function persist(profiles: Profile[]): Profile[] {
  try {
    durableSet(KEY, JSON.stringify(profiles))
  } catch {
    /* storage blocked — the returned list still applies for this session */
  }
  return profiles
}

/** Save (upsert by name) a Settings snapshot under `name`. Empty name is a no-op. */
export function saveProfile(name: string, settings: Settings): Profile[] {
  const trimmed = name.trim()
  if (!trimmed) return loadProfiles()
  const others = loadProfiles().filter((p) => p.name !== trimmed)
  return persist(
    [...others, { name: trimmed, settings, schema: PROFILE_SCHEMA }].sort((a, b) =>
      a.name.localeCompare(b.name),
    ),
  )
}

/** Remove the profile named `name` (no-op if absent). */
export function deleteProfile(name: string): Profile[] {
  return persist(loadProfiles().filter((p) => p.name !== name))
}
