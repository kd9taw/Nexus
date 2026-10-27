// The station's decode alerts, received as a PUSH and shown.
//
// The station decides which heard stations are worth interrupting the operator for — someone
// calling you, a new DXCC entity or grid, a watch-list station, an opted-in CQ — and which parks
// have just come on the air (crates/tempo-app/src/needed_alert.rs), and raises each as the `alert`
// event (`src-tauri/src/alert.rs`): kind `needed` for a station, `parks` for the parks. This
// window and a computer driving the station hear the same event under the same id, so every
// screen shows the same alerts. Nothing here decides; `alerts.ts` renders.

import { useEffect, useRef } from 'react'
import { showFreshParks, showNeededAlert, type NeededAlert } from './alerts'
import type { RaisedAlert } from './usePounce'

/** The one version of the alert's shape this build reads. */
const ALERT_VERSION = 1
/** How many alert ids are remembered against a second delivery of the same one. */
const IDS_KEPT = 256
const shown = new Set<number>()

/** Test seam: forget the ids shown. */
export function __resetStationAlertsForTest(): void {
  shown.clear()
}

/**
 * Show one raised alert, if it is the station's and this window has not shown it: a `needed`
 * alert's toast and sound, or a `parks` alert's beep. Another kind (Pounce's, which has its own
 * banner) or a later shape is not ours. An alert arriving twice under one id — a window reload
 * racing the emit — shows once.
 */
export function showRaisedAlert(raised: RaisedAlert | null | undefined, onWork?: (a: NeededAlert) => void): void {
  if (!raised || raised.v !== ALERT_VERSION) return
  if (raised.kind !== 'needed' && raised.kind !== 'parks') return
  if (!raised.alert || shown.has(raised.id)) return
  if (shown.size >= IDS_KEPT) shown.delete(shown.values().next().value as number)
  shown.add(raised.id)
  if (raised.kind === 'parks') showFreshParks()
  else showNeededAlert(raised.alert as NeededAlert, onWork)
}

/** Listen for the station's alerts for the life of the window; `onWork` is the toast's button. */
export function useStationAlerts(onWork: (a: NeededAlert) => void): void {
  const work = useRef(onWork)
  useEffect(() => {
    work.current = onWork
  }, [onWork])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    let alive = true
    // The global bridge api.ts uses for invoke (`withGlobalTauri`), as `usePounce` does.
    const listen = window.__TAURI__?.event?.listen
    if (!listen) return
    void (async () => {
      try {
        const un = await listen<RaisedAlert>('alert', (e) => showRaisedAlert(e.payload, (a) => work.current(a)))
        if (alive) unlisten = un
        else un()
      } catch {
        // A listener that fails to register must not take the app down.
      }
    })()
    return () => {
      alive = false
      unlisten?.()
    }
  }, [])
}
