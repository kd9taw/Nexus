import { useEffect, useRef } from 'react'

// A short two-note audio cue when the dial crosses the operator's TX privileges:
//   - back IN band  → a rising  "ding"  (reassuring)
//   - OUT of band   → a falling "dong"  (warning)
// Driven off `txAllowed` (the privilege gate already in the live snapshot), so it fires
// exactly when you dial across your license edge. Web Audio only — no sound files. Fires
// only on a genuine transition (never on load, never while the state is unchanged).
export function useBandEdgeTones(txAllowed: boolean | undefined, enabled: boolean): void {
  const prev = useRef<boolean | undefined>(undefined)
  const ctxRef = useRef<AudioContext | null>(null)

  useEffect(() => {
    if (txAllowed === undefined) return
    const was = prev.current
    prev.current = txAllowed
    // No tone on first read (was === undefined) or when nothing changed.
    if (!enabled || was === undefined || was === txAllowed) return

    try {
      const Ctx: typeof AudioContext | undefined =
        window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
      if (!Ctx) return
      const ctx = ctxRef.current ?? (ctxRef.current = new Ctx())
      if (ctx.state === 'suspended') void ctx.resume()
      const t0 = ctx.currentTime

      // One short sine "note" with a soft attack/decay so it reads as a chime, not a click.
      const note = (freq: number, at: number, dur: number) => {
        const osc = ctx.createOscillator()
        const gain = ctx.createGain()
        osc.type = 'sine'
        osc.frequency.value = freq
        gain.gain.setValueAtTime(0.0001, t0 + at)
        gain.gain.exponentialRampToValueAtTime(0.16, t0 + at + 0.012)
        gain.gain.exponentialRampToValueAtTime(0.0001, t0 + at + dur)
        osc.connect(gain).connect(ctx.destination)
        osc.start(t0 + at)
        osc.stop(t0 + at + dur)
      }

      if (txAllowed) {
        // Ding — rising (back in band).
        note(660, 0, 0.13)
        note(988, 0.11, 0.18)
      } else {
        // Dong — falling (strayed past an edge).
        note(440, 0, 0.15)
        note(294, 0.14, 0.28)
      }
    } catch {
      /* audio unavailable in this environment — stay silent */
    }
  }, [txAllowed, enabled])
}
