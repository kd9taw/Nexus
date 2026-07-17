// The app's hidden screen-reader outlet: two visually-hidden live regions fed
// by the announce.ts bus (polite = queued, assertive = interrupts). Rendered
// once in App. role="status"/"alert" carry implicit aria-live semantics; the
// regions exist from mount so readers register them before the first message.
import { useEffect, useState } from 'react'
import { subscribeAnnouncements } from '../announce'

export function Announcer() {
  const [polite, setPolite] = useState('')
  const [assertive, setAssertive] = useState('')
  useEffect(
    () =>
      subscribeAnnouncements((p, a) => {
        if (p) setPolite(p)
        if (a) setAssertive(a)
      }),
    [],
  )
  return (
    <>
      <div className="sr-only" role="status" aria-atomic="true">
        {polite}
      </div>
      <div className="sr-only" role="alert" aria-atomic="true">
        {assertive}
      </div>
    </>
  )
}
