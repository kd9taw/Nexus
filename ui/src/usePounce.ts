// Pounce — the "a new one just appeared" alert, received as a PUSH.
//
// Every other surface in Nexus polls. This one does not, deliberately: the whole value of the
// feature is that the alert lands the MOMENT the spot does, and a poll is late by construction.
// The backend scores each inbound cluster/RBN spot and emits `pounce` only when something clears
// the operator's rarity threshold, already de-duplicated per call/band/mode — so everything
// arriving here is worth interrupting for.
//
// SOUND IS THE PRIMARY CHANNEL, NOT THE NOTIFICATION. The OS notification is a bonus: Do Not
// Disturb, focus modes and notification permissions can all silently suppress it, and a rare-DX
// alert that can be silently suppressed is worse than useless because you will trust it. The
// earcon plays regardless of window focus and regardless of notification permission.

import { useEffect, useRef, useState } from 'react'
import { doubleBeep } from './alerts'
import type { NeedTag } from './types'

export interface PounceAlert {
  call: string
  band: string
  mode: string
  freqMhz: number | null
  tags: NeedTag[]
  entity: string
  atUnix: number
}

/** Distinct from every other earcon in the app: three rising tones, so it is unmistakable
 * without looking. The existing new-DXCC decode alert is a two-tone at 520 Hz; this sits above
 * it and adds a third note. */
function pounceEarcon(): void {
  doubleBeep(880)
  window.setTimeout(() => doubleBeep(1320), 260)
}

/** Fire a desktop notification if — and only if — the OS has already granted permission.
 * Never prompts: a permission dialog thrown at an operator mid-QSO because a spot arrived is
 * exactly the wrong moment to ask. */
function osNotify(a: PounceAlert): void {
  try {
    if (typeof Notification === 'undefined' || Notification.permission !== 'granted') return
    const where = a.freqMhz ? `${a.freqMhz.toFixed(3)} MHz` : a.band
    new Notification(`New one: ${a.entity || a.call}`, {
      body: `${a.call} — ${where} ${a.mode}`,
      tag: `pounce-${a.call}-${a.band}-${a.mode}`, // collapse duplicates in the OS tray
    })
  } catch {
    // A notification failing must never take the alert down with it — the sound already played.
  }
}

/**
 * Subscribe to Pounce alerts.
 *
 * Returns the most recent alert (for the banner) and a dismiss function. The listener is
 * registered once and torn down on unmount; the pop-out panel windows each get their own because
 * the backend emits to every window.
 */
export function usePounce(): { alert: PounceAlert | null; dismiss: () => void } {
  const [alert, setAlert] = useState<PounceAlert | null>(null)
  // Guard against a double-fire if the same alert somehow arrives twice (a window reload racing
  // the emit). The backend already de-dupes per call/band/mode; this is belt and braces.
  const lastKey = useRef<string>('')

  useEffect(() => {
    let unlisten: (() => void) | undefined
    let alive = true
    // Use the SAME global bridge api.ts uses for invoke (`withGlobalTauri` is on) rather than
    // adding @tauri-apps/api as a dependency for one listener. A permanent dependency is a big
    // price for a function that is already on `window`.
    const listen = window.__TAURI__?.event?.listen
    if (!listen) {
      // No event bridge (a browser-hosted dev view) — Pounce is simply inert there.
      return
    }
    void (async () => {
      try {
        const un = await listen<PounceAlert>('pounce', (e) => {
          const a = e.payload
          if (!a || !a.call) return
          const key = `${a.call}|${a.band}|${a.mode}|${a.atUnix}`
          if (key === lastKey.current) return
          lastKey.current = key
          pounceEarcon()
          osNotify(a)
          setAlert(a)
        })
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

  return { alert, dismiss: () => setAlert(null) }
}
