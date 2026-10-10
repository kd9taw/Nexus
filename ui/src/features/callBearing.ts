// THE BEARING NEXUS SHOWS FOR A CALL IS THE BEARING ITS POINT TURNS TO. A point-at-call aims where the
// station's one resolver says (src-tauri `aim_at_call`): the station's own grid or callbook position when
// Nexus knows one, and only when it knows none the centre of its country. A view that draws a bearing for a
// call draws that resolver's answer, read through its read-only twin (`rotatorBearingToCall`), never one
// worked out here. Two were: the Chase box's heading was the centre of the entity while the ↗ beside it
// turned to the station's grid, and the log strip's card took the callbook's position over the square the
// operator had typed (the point trusts the typed square first) and drew nothing where the point still
// turned to a grid heard off the air or to the country's centre.
//
// A seat without station control (a browser on Nexus Remote) asks nothing and is answered nothing: the read
// is the desktop's, and Nexus Remote offers it nothing new. It draws no bearing rather than one the station
// would not turn to.
import { useEffect, useState } from 'react'
import { rotatorBearingToCall } from '../api'
import { ROTATOR_POLL_MS } from '../remote-web/rotator'
import { pollSingleFlight } from '../singleFlight'
import { useStationControl } from '../stationAccess'
import type { CallBearing } from '../types'

/** Why the station has no bearing for a call: the code its bearing read refused with (`noGrid`: the
 *  operator's own grid is not set; `unknownStation`: nothing places the station), or null for a failure it
 *  did not name, which a view draws as no bearing at all. */
export type NoBearing = 'noGrid' | 'unknownStation' | null

/** The station's last answer for one call: the bearing a short-path point at it turns to, and how far
 *  away that is, or why it has none. */
export type CallAim = { bearing: CallBearing } | { why: NoBearing }

function noBearing(e: unknown): NoBearing {
  const code = e instanceof Error ? e.message : typeof e === 'string' ? e : ''
  return code === 'noGrid' || code === 'unknownStation' ? code : null
}

/** Is this answer a bearing? Anything else (a bridge with nothing behind it answers `{}`) is no answer,
 *  never a number drawn from a field that is not there. */
function isBearing(v: unknown): v is CallBearing {
  const b = v as CallBearing | null | undefined
  return typeof b?.pointed?.bearing === 'number' && typeof b.km === 'number'
}

const NONE: ReadonlyMap<string, CallAim> = new Map()

/**
 * The station's answer for each of `calls`, keyed by the call as given. Asked the moment the list
 * changes and again every `ROTATOR_POLL_MS`, in one single-flight poll for the whole list, because what
 * the station knows of a call moves under it (the log form's grid lands from the callbook a moment after
 * the call is typed; a grid is heard off the air) and the bearing moves with it. A call keeps its last
 * answer while it is still listed, so a row that arrives or moves does not blank the others, and an
 * answer is only ever looked up by its own call. No entry until the station has answered.
 */
export function useCallBearings(calls: readonly string[]): ReadonlyMap<string, CallAim> {
  const local = useStationControl()
  // One key per set of calls: the order a list is drawn in is no reason to ask again.
  const key = local ? [...new Set(calls.filter(Boolean))].sort().join(' ') : ''
  const [aims, setAims] = useState<ReadonlyMap<string, CallAim>>(NONE)
  useEffect(() => {
    if (!key) return
    const list = key.split(' ')
    return pollSingleFlight('call bearings', ROTATOR_POLL_MS, (owns) =>
      Promise.allSettled(list.map((call) => rotatorBearingToCall(call))).then((answers) => {
        if (!owns()) return
        setAims(
          new Map(
            answers.map((a, i): [string, CallAim] => [
              list[i],
              a.status === 'rejected' ? { why: noBearing(a.reason) } : isBearing(a.value) ? { bearing: a.value } : { why: null },
            ]),
          ),
        )
      }),
    )
  }, [key])
  return key ? aims : NONE
}
