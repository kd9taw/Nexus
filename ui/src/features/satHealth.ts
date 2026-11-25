// What a bird's catalog record SAYS about it, as one chip every surface that
// names a bird can render — the Birds list, the Connect Passes pane, and the
// map label.
//
// It exists because "is this bird still worth chasing" was being re-derived
// per surface from two fields that mean different things:
//
//  - `status` is SatNOGS's orbital verdict (in orbit | re-entered |
//    launch failed | future; older catalogs say alive | dead). It is ABSENT whenever the serving snapshot has no catalog (a
//    schema-1 manifest, the Celestrak fallback leg) — absent means NOT ASKED,
//    and a chip drawn from it would be an invention, not a report.
//  - `amateur` is the mirror catalog's "carries at least one LIVE amateur
//    transmitter". A bird can be perfectly alive in orbit with nothing left
//    to work, which is the case an elevation number can never show.
//
// The rule both halves share: report what was measured, say nothing when
// nothing was. A silent bird and an unknown bird are different facts.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The
// chip words come from the catalog and are resolved when the chip is BUILT (a
// fresh object per call, so nothing freezes a locale). The one thing that never
// moves is the upstream `status` value in the default branch — that is the
// source's own word, printed as it arrived.
import { t } from '../i18n'
import type { SatExcluded } from '../types'

/** Which chip species to draw. `dead` = there is nothing to work here
 * (`.sat-chip.dead`); `stale` = we cannot currently say where it is or what
 * it is doing (`.sat-chip.stale`). Both already exist in both themes. */
export type SatHealthTone = 'dead' | 'stale'

export interface SatHealth {
  /** Chip text. The `.sat-chip` family is uppercased by CSS — keep it short. */
  label: string
  /** The full sentence, on the chip's title: what was reported and what it
   * means for working the bird. */
  title: string
  tone: SatHealthTone
}

/**
 * Whether SatNOGS calls the bird up there and working. In 2026-08 SatNOGS
 * renamed that value from `alive` to `in orbit`, and the mirror carries the
 * word through as it arrived — so the live catalog says `in orbit` while
 * older catalogs and caches still say `alive`. Both mean the same thing;
 * every surface that asks "is this a live bird" asks here, never with its
 * own string compare.
 */
export function isInOrbitStatus(status?: string | null): boolean {
  return status === 'in orbit' || status === 'alive'
}

/**
 * Whether SatNOGS says the bird is gone and not coming back: `dead` (older
 * catalogs), `re-entered`, or `launch failed` (since 2026-11, a bird lost on
 * the way up). The same set satBirdHealth marks with the dead tone; every
 * surface that sinks or flags a gone bird asks here.
 */
export function isGoneStatus(status?: string | null): status is string {
  return status === 'dead' || status === 're-entered' || status === 'launch failed'
}

/**
 * The mark for a bird that HAS a row in the view (elements exist, the
 * position is real). `null` = nothing to report — either it is alive and
 * transmitting, or nobody has told us anything about it.
 *
 * `amateur` is only believed when it is explicitly `false`: schedule rows
 * (`SatPass`) carry a status with no amateur flag at all, and treating that
 * `undefined` as "no transmitters" would mark every scheduled bird silent.
 */
export function satBirdHealth(
  status?: string | null,
  amateur?: boolean | null,
): SatHealth | null {
  if (status == null || status === '') return null // never asked — say nothing
  if (isInOrbitStatus(status)) {
    return amateur === false
      ? {
          label: t('sat.health.silent.label'),
          title: t('sat.health.silent.title'),
          tone: 'dead',
        }
      : null
  }
  switch (status) {
    case 'dead':
      return {
        label: t('sat.health.dead.label'),
        title: t('sat.health.dead.title'),
        tone: 'dead',
      }
    case 're-entered':
      return {
        label: t('sat.health.reentered.label'),
        title: t('sat.health.reentered.title'),
        tone: 'dead',
      }
    // SatNOGS' word (since 2026-11) for a bird lost on the way up. It never
    // reached orbit, so it reads as gone exactly like a re-entry — never as
    // the `stale` "cannot say" an unseen word would get.
    case 'launch failed':
      return {
        label: t('sat.health.launchFailed.label'),
        title: t('sat.health.launchFailed.title'),
        tone: 'dead',
      }
    case 'future':
      return {
        label: t('sat.health.preLaunch.label'),
        title: t('sat.health.preLaunch.title'),
        tone: 'stale',
      }
    default:
      // An unseen upstream value degrades to a label rather than being
      // dropped — the same reason SatStatus keeps the source string. The label
      // IS that value, so it is printed verbatim; only the sentence moves.
      return {
        label: status,
        title: t('sat.health.unknown.title', { status }),
        tone: 'stale',
      }
  }
}

/**
 * The mark for a bird the view could NOT place (`SatView.excluded`). Never
 * null: being excluded is itself the fact the operator is owed — a ★ bird
 * used to just vanish from every list with no row and no reason.
 */
export function satExcludedHealth(reason: SatExcluded['reason']): SatHealth {
  switch (reason) {
    case 'noElements':
      return {
        label: t('sat.health.noElements.label'),
        title: t('sat.health.noElements.title'),
        tone: 'stale',
      }
    case 'staleElements':
      return {
        label: t('sat.health.staleElements.label'),
        title: t('sat.health.staleElements.title'),
        tone: 'stale',
      }
    case 'noPosition':
      return {
        label: t('sat.health.noPosition.label'),
        title: t('sat.health.noPosition.title'),
        tone: 'stale',
      }
  }
}
