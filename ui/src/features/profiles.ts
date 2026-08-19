// Goal profiles — a profile is a named bundle of enabled features + a default
// landing view + a Now-Bar emphasis. Profiles are SWITCHABLE presets over the
// toggle system (operators change hats), not a one-time fork, and they're driven
// by GOAL/intent, never by self-rated experience. See feature-modularity.md §4.2.
//
// Pure data + a pure resolver (no React / storage) — node-testable.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Each profile's
// label and blurb reach the operator in Settings ▸ Features and in the setup wizard, and
// they resolve THROUGH GETTERS: this is a module constant a dozen renders index directly, so
// looking the words up at import time would freeze whichever locale loaded first and no
// re-render could move it (the `features/needVisuals.ts` lesson). The record's SHAPE is
// unchanged — every consumer still reads `.label` / `.blurb`.
//
// `POTA / SOTA` is the two programmes' own names and stays here; a translated one names
// nothing. The rest is prose.

import { t } from '../i18n'
import { FEATURES, addWithDependencies, type FeatureId, type Intent, type View } from './registry'

export type ProfileId = 'starter' | 'dx' | 'contest' | 'pota' | 'vhf' | 'everything'

/** Now-Bar emphasis a profile prefers (stored now; consumed by NowBar later). */
export type NowBarEmphasis = 'qso' | 'needs' | 'rate' | 'openings' | 'activation'

export interface Profile {
  id: ProfileId
  label: string
  blurb: string
  /** Features whose `intents` intersect these are enabled. */
  intents: Intent[]
  /** Force-on beyond the intent match (rare). */
  extra?: FeatureId[]
  /** Enable literally everything (the expert preset). */
  everything?: boolean
  /** Where the app opens under this profile (must be an enabled section). */
  landing: View
  nowBarEmphasis: NowBarEmphasis
}

export const PROFILES: Record<ProfileId, Profile> = {
  starter: {
    id: 'starter',
    get label() {
      return t('profiles.starter.label')
    },
    get blurb() {
      return t('profiles.starter.blurb')
    },
    intents: ['casual'],
    landing: 'operate',
    nowBarEmphasis: 'qso',
  },
  dx: {
    id: 'dx',
    get label() {
      return t('profiles.dx.label')
    },
    get blurb() {
      return t('profiles.dx.blurb')
    },
    intents: ['dx'],
    landing: 'operate',
    nowBarEmphasis: 'needs',
  },
  contest: {
    id: 'contest',
    get label() {
      return t('profiles.contest.label')
    },
    get blurb() {
      return t('profiles.contest.blurb')
    },
    intents: ['contest'],
    // NOT 'fieldDay': that section's visibility is gated by the persisted master
    // switch (settings.fdActive), which no path may auto-enable — landing there
    // would redirect to a hidden view. 'operate' is core, so it's always reachable.
    landing: 'operate',
    nowBarEmphasis: 'rate',
  },
  pota: {
    id: 'pota',
    // The two programmes' own names — the same four letters in every language.
    label: 'POTA / SOTA',
    get blurb() {
      return t('profiles.pota.blurb')
    },
    intents: ['pota'],
    landing: 'operate',
    nowBarEmphasis: 'activation',
  },
  vhf: {
    id: 'vhf',
    get label() {
      return t('profiles.vhf.label')
    },
    get blurb() {
      return t('profiles.vhf.blurb')
    },
    intents: ['vhf'],
    landing: 'connect',
    nowBarEmphasis: 'openings',
  },
  everything: {
    id: 'everything',
    get label() {
      return t('profiles.everything.label')
    },
    get blurb() {
      return t('profiles.everything.blurb')
    },
    intents: [],
    everything: true,
    landing: 'operate',
    nowBarEmphasis: 'needs',
  },
}

export const PROFILE_LIST: Profile[] = [
  PROFILES.starter,
  PROFILES.dx,
  PROFILES.contest,
  PROFILES.pota,
  PROFILES.vhf,
  PROFILES.everything,
]

/**
 * Resolve a profile to a full enabled-set: core features always on, plus
 * intent-matched (or all, for `everything`) plus `extra`, then transitively
 * closed over `dependsOn`.
 */
export function resolveEnabled(profileId: ProfileId): Record<FeatureId, boolean> {
  const profile = PROFILES[profileId]
  const on = new Set<FeatureId>()
  for (const f of FEATURES) {
    if (f.core) on.add(f.id)
  }
  if (profile.everything) {
    // Staged defaultOff features stay out of even the 'everything' preset —
    // only the explicit Settings ▸ Features toggle turns them on.
    for (const f of FEATURES) if (!f.defaultOff) on.add(f.id)
  } else {
    for (const f of FEATURES) {
      if (!f.defaultOff && f.intents.some((i) => profile.intents.includes(i))) on.add(f.id)
    }
    for (const id of profile.extra ?? []) on.add(id)
  }
  // Transitively pull in dependencies of everything enabled so far.
  for (const id of [...on]) addWithDependencies(on, id)

  const out = {} as Record<FeatureId, boolean>
  for (const f of FEATURES) out[f.id] = on.has(f.id)
  return out
}
