// Pure log → worked-grid reductions for the map's "My coverage (worked)" layer and the
// 3-D globe's coverage points. Kept out of the map components so the grid math is
// unit-testable on its own, with no canvas, WebGL or React in the way.
//
// Both callers previously carried their own copy of this loop. They agreed, which is
// the problem: two identical reductions drift the moment one is touched, and neither
// had a test because reaching them meant standing up a map.
import type { LoggedQso } from './types'

/** The unique 4-character Maidenhead squares the operator has worked.
 *
 * A QSO with no grid, or a grid too short to name a square, is SKIPPED rather than
 * guessed at — there is nothing to place on a map, and inventing a square would put a
 * mark on the operator's coverage they never earned. Grids are upper-cased and cut to
 * 4 characters, which is the granularity coverage and VUCC actually work at; a 6-char
 * locator contributes its square, not a second point. */
export function workedGridSet(log: readonly LoggedQso[]): Set<string> {
  const set = new Set<string>()
  for (const q of log) {
    const g = (q.grid ?? '').trim().toUpperCase()
    if (g.length >= 4) set.add(g.slice(0, 4))
  }
  return set
}
