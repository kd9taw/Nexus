// Config profiles — named full-Settings snapshots so an operator can switch a whole
// rig/antenna/CAT/band setup in one move (home HF ↔ portable VHF ↔ Field Day). Stored
// in localStorage (machine-local, survives restarts); "loading" a profile applies it
// through the normal settings-save path, so there's no separate apply mechanism to drift.

import type { Settings } from './types'

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
 * slot (hardware, not configuration), and the connector sync cursors (loading a
 * profile must not re-fetch history). Everything else merges over the current
 * settings, and a key the profile predates (absent) keeps its current value. */
const NEVER_IMPORT: readonly string[] = [
  'mycall',
  'licenseClass',
  'radios',
  'activeRadio',
  'qrzLastSyncUnix',
  'eqslLastSync',
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
    const raw = localStorage.getItem(KEY)
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
    localStorage.setItem(KEY, JSON.stringify(profiles))
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
