import { useCallback, useMemo, useRef, useState } from 'react'
import type { FeatureId, View } from './features/registry'
import {
  applyProfile as applyProfileState,
  applyProfiles as applyProfilesState,
  defaultState,
  dismissReveal as dismissRevealState,
  landingFor,
  normalizeState,
  toggleFeature,
  type FeatureState,
} from './features/state'
import type { ProfileId } from './features/profiles'
import { durableGet, durableSet } from './features/durableStore'

const STORAGE_KEY = 'nexus.features.v1'

/** Load persisted state, or first-run default (Everything). Guarded — storage
 * access can throw in some private modes (matches the useTheme pattern). */
function readInitial(): FeatureState {
  try {
    const raw = durableGet(STORAGE_KEY)
    if (raw != null) return normalizeState(JSON.parse(raw))
  } catch {
    /* unreadable / malformed — fall through to default */
  }
  return defaultState()
}

/** Read the persisted enabled operating-mode flags WITHOUT the hook — for standalone
 * surfaces (e.g. the popped-out Needed panel) that fetch needs on their own
 * cadence and must gate CW/Phone rows the same way the docked Needed board does. Falls
 * back to the default state (everything-except-Field-Day → CW/Phone on) if unreadable. */
export function readEnabledModes(): { cw: boolean; phone: boolean } {
  try {
    const raw = durableGet(STORAGE_KEY)
    const state = raw != null ? normalizeState(JSON.parse(raw)) : defaultState()
    return { cw: state.enabled.cw !== false, phone: state.enabled.phone !== false }
  } catch {
    const d = defaultState()
    return { cw: d.enabled.cw !== false, phone: d.enabled.phone !== false }
  }
}

function persist(state: FeatureState): void {
  try {
    durableSet(STORAGE_KEY, JSON.stringify(state))
  } catch {
    /* full / unavailable — in-memory state still applies this session */
  }
}

export interface FeaturesApi {
  /** The active profile (or 'custom' after a manual tweak). */
  profile: FeatureState['profile']
  /** Resolved enabled-set across all feature ids. */
  enabled: Record<FeatureId, boolean>
  /** Is a feature on? (core always true.) */
  isOn: (id: FeatureId) => boolean
  /** Flip one feature (no-op for core); cascades deps/dependents. */
  toggle: (id: FeatureId) => void
  /** Switch to a goal profile (re-applies its bundle). */
  applyProfile: (id: ProfileId) => void
  /** Apply a UNION of profiles (wizard multi-select), plus `extraOn` features to
   * force-enable regardless of the profiles (the wizard's CW/Phone mode choice). */
  applyProfiles: (ids: ProfileId[], extraOn?: FeatureId[]) => void
  /** The active profile's landing view (custom → operate). */
  landing: View
  /** True when there was no persisted feature state at startup (a fresh install
   * — gates the first-run wizard so existing users aren't shown it). */
  firstRun: boolean
  /** Reveal nudges the operator has dismissed (by triggering achievement id). */
  dismissedReveals: string[]
  /** Permanently dismiss the reveal nudge for an achievement id. */
  dismissReveal: (achievementId: string) => void
}

/**
 * The modular-features hook: persisted enabled-set + profile, with dependency-safe
 * toggles and profile switching. Pure transitions live in `features/state.ts`;
 * this just adds React state + localStorage. Call once at the app root.
 */
export function useFeatures(): FeaturesApi {
  const [state, setState] = useState<FeatureState>(readInitial)
  // Captured once on first render, before any persist() — true only for a
  // genuinely fresh install. Lazy so the storage read happens exactly once.
  const firstRunRef = useRef<boolean | null>(null)
  if (firstRunRef.current === null) {
    // ⚠️ THROUGH THE DURABLE LAYER, not localStorage directly. Once this key became durable
    // its whole purpose is to survive a localStorage wipe — and an operator who has just
    // survived one would otherwise be told they are a fresh install and shown the first-run
    // wizard, which is a worse version of the bug this promotion fixes.
    try {
      firstRunRef.current = durableGet(STORAGE_KEY) == null
    } catch {
      firstRunRef.current = false
    }
  }

  const toggle = useCallback(
    (id: FeatureId) => setState((s) => {
      const next = toggleFeature(s, id)
      persist(next)
      return next
    }),
    [],
  )

  const applyProfile = useCallback(
    // Preserve prior reveal dismissals across a profile switch.
    (id: ProfileId) => setState((s) => {
      const next = applyProfileState(id, s.dismissedReveals)
      persist(next)
      return next
    }),
    [],
  )

  const applyProfiles = useCallback(
    (ids: ProfileId[], extraOn: FeatureId[] = []) => setState((s) => {
      const next = applyProfilesState(ids, s.dismissedReveals, extraOn)
      persist(next)
      return next
    }),
    [],
  )

  const dismissReveal = useCallback(
    (achievementId: string) => setState((s) => {
      const next = dismissRevealState(s, achievementId)
      persist(next)
      return next
    }),
    [],
  )

  const isOn = useCallback((id: FeatureId) => state.enabled[id] !== false, [state.enabled])

  const landing = useMemo(() => landingFor(state), [state])

  return {
    profile: state.profile,
    enabled: state.enabled,
    isOn,
    toggle,
    applyProfile,
    applyProfiles,
    landing,
    firstRun: firstRunRef.current ?? false,
    dismissedReveals: state.dismissedReveals,
    dismissReveal,
  }
}
