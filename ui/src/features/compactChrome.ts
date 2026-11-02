// THE COMPACT CHROME (the 1080p pass, 2026-11-01) — which window sizes fold the app's chrome, read
// from the size class `useViewport` publishes (`useViewportClass`), never measured again here.
//
// Two answers, because the two folds have two reasons:
//   · `wide` (md and up): there is width to spare beside the controls, so the read-only TX meters
//     stand in the TX strip's empty right side instead of a dock row of their own, and Phone's dock
//     is one row (PTT, Lock, then the transmit contract).
//   · `fold` (md and lg, not xl): the rows do NOT fit on one line, so the eleven FT tier pills fold
//     into one menu button, the now-bar rides in the top bar instead of a line of its own, and the
//     CW/Phone header's occasional controls (memories, rotator, split, REC) go behind a ⋯ button. At
//     xl the pills already stand on one line, and the bar there stays as it was (merging the now-bar
//     into the 2560-wide bar measured TALLER than the two lines it replaced).
// Below md (sm, xs, and before the first stamp) nothing folds: those sizes have their own rules
// (styles.css "The top bar at the small size", the xs transmit contract), and the hosted Remote
// pages run there.
//
// ⛔ NEITHER FOLD MAY REACH A TRANSMIT OR STOP CONTROL. PTT, Lock, Send, Stop TX, Tune, the TX latch,
// the Esc/Stop macro and every macro key stay where they are, in the dock or the TX strip, at every
// size; only read-only meters, a status line and occasional controls move. Pinned in
// components/compact-chrome.test.tsx.
import { useViewportClass, type ViewportClass } from '../useViewport'

/** md and up: meters in the TX strip, the one-row Phone dock. */
export function wideChrome(vc: ViewportClass | null): boolean {
  return vc === 'md' || vc === 'lg' || vc === 'xl'
}

/** md and lg: the FT tiers behind one menu button, the now-bar in the top bar, the header's
 *  occasional controls behind ⋯. */
export function foldedChrome(vc: ViewportClass | null): boolean {
  return vc === 'md' || vc === 'lg'
}

export function useCompactChrome(): { wide: boolean; fold: boolean } {
  const vc = useViewportClass()
  return { wide: wideChrome(vc), fold: foldedChrome(vc) }
}
