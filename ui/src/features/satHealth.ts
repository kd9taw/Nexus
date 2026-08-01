// What a bird's catalog record SAYS about it, as one chip every surface that
// names a bird can render — the Birds list, the Connect Passes pane, and the
// map label.
//
// It exists because "is this bird still worth chasing" was being re-derived
// per surface from two fields that mean different things:
//
//  - `status` is SatNOGS's orbital verdict (alive | dead | re-entered |
//    future). It is ABSENT whenever the serving snapshot has no catalog (a
//    schema-1 manifest, the Celestrak fallback leg) — absent means NOT ASKED,
//    and a chip drawn from it would be an invention, not a report.
//  - `amateur` is the mirror catalog's "carries at least one LIVE amateur
//    transmitter". A bird can be perfectly alive in orbit with nothing left
//    to work, which is the case an elevation number can never show.
//
// The rule both halves share: report what was measured, say nothing when
// nothing was. A silent bird and an unknown bird are different facts.
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
  switch (status) {
    case 'alive':
      return amateur === false
        ? {
            label: 'silent',
            title:
              'In orbit, but the catalog lists no live amateur transmitter — the pass geometry is real, there is nothing to work on it.',
            tone: 'dead',
          }
        : null
    case 'dead':
      return {
        label: 'dead',
        title:
          'SatNOGS reports this bird silent. Its passes are still computed from real elements; working it is not expected.',
        tone: 'dead',
      }
    case 're-entered':
      return {
        label: 're-entered',
        title:
          'SatNOGS reports this bird has re-entered — it is gone. Any pass shown is modelled from the last elements on file.',
        tone: 'dead',
      }
    case 'future':
      return {
        label: 'pre-launch',
        title:
          'SatNOGS has this bird on record but not yet deployed — nothing to work until it launches.',
        tone: 'stale',
      }
    default:
      // An unseen upstream value degrades to a label rather than being
      // dropped — the same reason SatStatus keeps the source string.
      return {
        label: status,
        title: `SatNOGS reports this bird's status as "${status}".`,
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
        label: 'no elements',
        title:
          'No source carries orbital elements for this bird right now, so its passes cannot be computed. It stays starred; rows return when elements do.',
        tone: 'stale',
      }
    case 'staleElements':
      return {
        label: 'stale elements',
        title:
          'The newest elements on file are past the 30-day ceiling where SGP4 accuracy is gone. Refresh elements, or wait for the next automatic refresh.',
        tone: 'stale',
      }
    case 'noPosition':
      return {
        label: 'no position',
        title:
          'Current elements, but the propagator refused a position for them — a decaying orbit does this. Nothing is being hidden; there is genuinely no place to draw.',
        tone: 'stale',
      }
  }
}
