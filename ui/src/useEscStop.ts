// ESC IS A STOP. The one listener every screen's Esc stop rides: App's, for Tempo, Phone, SSTV, APRS
// and Satellites (2026-10-01) and the Contest screen while its log line shows (2026-10-11), and FT's,
// CW's, RTTY's, PSK's and JS8's own (operator, 2026-10-01), and the contest's pop-out windows'
// (DetachedPanel, 2026-10-18).
// What a screen's Esc does is the screen's own `stop`; only where the key is heard is decided here.
//
//   · CAPTURE phase on `window`, so nothing on the screen can swallow the key. The cockpits used to
//     listen in the bubble phase, where a control that stops keydown would keep the stop from
//     hearing it (measured in real Chrome with a planted one; no shipped control does it to Esc).
//   · Never preventDefault, so every other meaning of Esc still happens on the same press: a Radix
//     menu or dialog dismisses only an Esc nobody prevented, Satellites' bird detail checks the same.
//     A cockpit that cancels the key's default does it in its own bubble-phase listener, as before.
import { useEffect, useRef } from 'react'

/** Esc calls `stop` while `on`. `stop` is read at press time. */
export function useEscStop(on: boolean, stop: () => void): void {
  const stopRef = useRef(stop)
  stopRef.current = stop
  useEffect(() => {
    if (!on) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') stopRef.current()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [on])
}
