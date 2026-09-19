// The Logbook's LoTW backlog — what its "Upload to LoTW" button counts. A pure pass over the log,
// kept out of the component so the Logbook can memoize it on the log (it re-renders on every
// 300 ms snapshot, and a full pass over a 150k-contact log per render is a measured cost) and a
// test can count the passes.

import type { LoggedQso } from '../types'

/** Award-unconfirmed, and never sent to LoTW or bounced by it. */
const lotwEligible = (q: LoggedQso) =>
  !q.awardConfirmed &&
  (!q.upload?.lotw || ['rejected', 'authfail'].includes(q.upload.lotw.outcome))

/** QSOs not yet sent to LoTW, in one pass. Mirrors the backend batch builder
 *  (lotw_unsent_indices): award-unconfirmed, never-sent-or-bounced, AND the time of day is
 *  known — LoTW matches on time, so a date-only import can never confirm and is excluded
 *  (`unsent`), with its count kept separately (`timeless`) so the operator learns why instead
 *  of wondering. */
export function lotwBacklog(log: LoggedQso[]): { unsent: number; timeless: number } {
  let unsent = 0
  let timeless = 0
  for (const q of log) {
    if (!lotwEligible(q)) continue
    if (q.timeKnown === false) timeless++
    else unsent++
  }
  return { unsent, timeless }
}
