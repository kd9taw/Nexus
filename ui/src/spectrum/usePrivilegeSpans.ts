// THE LICENCE-CLASS SPANS A SCOPE TINTS FROM, kept current: asked for when the section changes, when the
// scope comes back on screen, and every 30 s while it shows, so a class changed in Settings or by a
// profile reaches the tint without a restart. Display only (`spectrum/overlays.ts`): the gate decides.
import { useEffect, useState } from 'react'
import { getPrivilegeSpans } from '../api'
import type { PrivilegeSpans } from '../types'

/** How often a showing scope asks again (ms). The class changes only when the operator changes it. */
const REFRESH_MS = 30_000

const same = (a: PrivilegeSpans | null, b: PrivilegeSpans | null) => JSON.stringify(a) === JSON.stringify(b)

/** The spans for the section `mode` while `active` (no mode = none asked, no edges). Null on a failed
 *  read and on the Remote page, which has no such command: the scope then tints nothing. */
export function usePrivilegeSpans(mode: string | null | undefined, active: boolean): PrivilegeSpans | null {
  const [got, setGot] = useState<PrivilegeSpans | null>(null)
  useEffect(() => {
    if (!mode || !active) return
    let live = true
    const load = () => {
      // Through a promise, so any failure to ask, not only a refusal, reads as "no edges".
      Promise.resolve()
        .then(() => getPrivilegeSpans(mode))
        .then(
          (s) => s ?? null,
          () => null,
        )
        .then((s) => {
          if (live) setGot((prev) => (same(prev, s) ? prev : s))
        })
    }
    load()
    const id = setInterval(load, REFRESH_MS)
    return () => {
      live = false
      clearInterval(id)
    }
  }, [mode, active])
  // Another section's answer is never shown while this one's is on its way.
  return got && mode && got.mode === mode ? got : null
}
