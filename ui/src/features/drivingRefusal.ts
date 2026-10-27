// A refused rig control on a computer driving another station (client mode). At the station these
// controls stay quiet when a call fails, as they always have: the rig's next read-back shows what it
// took. From afar the refusal is usually the link's or the station's (not live, a stale picture, too
// many requests), and a press that did nothing and said nothing reads as a dead control. So there,
// and only there, the refusal is shown in the words it came with (`api.ts` puts client mode's own
// codes in words). A burst (a wheel, a drag, typing) shows one: the same words again within
// `REPEAT_MS` are not stacked.

import { pushToast } from '../toast'
import { drivesAnotherStation } from './durableStore'

const REPEAT_MS = 3000
let last: { text: string; at: number } | null = null

/** Show `e` when this computer drives another station; at the station, nothing. */
export function showWhenDriving(e: unknown): void {
  if (!drivesAnotherStation()) return
  const text = e instanceof Error ? e.message : String(e)
  const now = Date.now()
  if (last && last.text === text && now - last.at < REPEAT_MS) return
  last = { text, at: now }
  pushToast(text, 'error')
}
