import { useCallback, useEffect, useRef } from 'react'
import type { RefObject } from 'react'
import type { AppSnapshot } from './types'
import { setFrequency } from './api'
import { bandLabelForMhz, bandRangeForLabel } from './band'

/** Trailing-flush window: at most one CAT write per this many ms while the wheel spins. */
const FLUSH_MS = 120
/** Re-seed the optimistic target from the live dial once the wheel has been idle this long (ms). */
const IDLE_RESEED_MS = 400
/** Accumulated scroll (pixel-equivalents) per one tuning step — normalizes wheels vs trackpads. */
const PX_PER_STEP = 100
/** Cap on whole steps a SINGLE wheel event may apply, so one violent flick of an energetic /
 * high-res mouse can't lurch the dial many steps at once. Excess is discarded, not carried. */
const MAX_STEPS_PER_EVENT = 8
/** …and the same cap in Hz, because the step count alone stopped protecting anything once a
 * step could be a DECADE. It was written when a step was ≤5 kHz (8 × 5 kHz = 40 kHz); the
 * identical flick on a 10 MHz digit would be 80 MHz of dial. 500 kHz is above every
 * uniform-tuning worst case (5 kHz × Shift ×10 × 8 = 400 kHz), so selector-step tuning is
 * bit-for-bit unchanged. ONE whole step always applies — a 10 MHz notch IS 10 MHz, which is the
 * operator's decision; only the MULTIPLE is bounded. */
const MAX_HZ_PER_EVENT = 500_000

interface WheelTuneOpts {
  /** Current rig dial (MHz) from the snapshot — the seed for a fresh wheel burst. */
  dialMhz: number
  /** Sideband to preserve so an in-band wheel never flips the mode. */
  sideband: string
  /** Only tune when CAT is up AND we're not transmitting (never move the VFO under a live over). */
  enabled: boolean
  /** Hz per tuning step (Shift = ×10). Shared with the tuning strip's step selector. */
  stepHz: number
  /** Sensitivity multiplier (1.0 = stock). <1 needs more scroll per step (damps an energetic /
   * high-res mouse), >1 tunes faster. Clamped to a sane range. Default 1.0. */
  sensitivity?: number
  /** Receive a fresh snapshot from the flushed set_frequency so the UI updates promptly. */
  onSnap?: (s: AppSnapshot) => void
  /** PER-EVENT step override, resolved from the wheel event's target: the Hz ONE notch here
   * moves, or `null` to DECLINE the event entirely (no tune, no `preventDefault` — the page keeps
   * its scroll). Omit ⇒ every event uses `stepHz`, exactly as before.
   *
   * This is what keeps per-digit tuning on ONE listener. The digits live inside the element this
   * hook already listens on, so a second listener means two optimistic targets and two coalescers
   * racing on one dial (measured: the digit's 1 kHz and the strip's 100 Hz both flush, latest
   * wins, the digit move is silently erased). Resolving the step per event makes that
   * unrepresentable: the digit under the pointer only changes the INCREMENT on the same target,
   * so carry still falls out of `target += 10**n`. */
  resolveStepHz?: (target: EventTarget | null) => number | null
  /** A burst reached the edge of the band it started in and was PARKED there. Fires ONCE per
   * burst (not once per ~120 ms flush against the edge), so the caller can SAY so: silence was
   * right for a 100 Hz creep and is wrong for a 1 MHz digit notch, which lands here on the first
   * click.
   *
   * ⚠️ It no longer means "refused" — the dial is not walled in at the band edge any more (see
   * `clampToBand`). It means the burst STOPPED here rather than carrying past; another notch from
   * this dial goes off the plan. */
  onEdge?: (mhz: number) => void
}

/**
 * Mouse-wheel tuning on a scope/waterfall element: wheel up tunes up by `stepHz`, wheel down tunes
 * down; hold Shift for ×10. Robust to input variety: direction comes from the dominant scroll axis
 * (WebView2/WebKit deliver Shift+wheel as HORIZONTAL scroll — `deltaX`, `deltaY === 0`), and delta
 * magnitude is accumulated into pixel-equivalents so a trackpad's many tiny events don't lurch the
 * dial. Rapid input is COALESCED — steps accumulate against an optimistic target and a single CAT
 * `set_frequency` is flushed at most every ~120 ms — so fast scrolling never queues a backlog of
 * slow CAT writes. The optimistic target self-corrects from the snapshot dial whenever wheeling
 * pauses, and PARKS at a band edge — reported once per burst through `onEdge`, never once per
 * flush — from where a further notch carries on off the band plan (see `clampToBand`).
 * NON-passive listener so it can `preventDefault()` the page scroll (React's `onWheel` is
 * passive).
 *
 * `resolveStepHz` lets ONE listener serve two mechanisms (the selected step and per-digit
 * tuning); `stepHz` alone is the original behavior, unchanged. Returns an Hz applier so a
 * keyboard equivalent can share the same optimistic target instead of racing it.
 */
export function useWheelTune(
  ref: RefObject<HTMLElement | null>,
  opts: WheelTuneOpts,
): (deltaHz: number) => void {
  // The listener attaches once; a ref keeps it reading the latest props each event.
  const stateRef = useRef(opts)
  stateRef.current = opts
  const targetHzRef = useRef<number | null>(null) // optimistic dial while a burst is in flight
  const accumRef = useRef(0) // sub-step scroll accumulator (pixel-equivalents)
  /** The step `accumRef` was filled AT. One accumulator serves every decade, so without this a
   *  partial gesture on the 100 Hz digit completes a step on whatever digit the pointer moved to:
   *  measured 0.6 of a notch at 100 Hz + 0.6 at 100 kHz = a full 100 kHz QSY, and 24.074 from
   *  14.074 on the 10 MHz digit. Pixels are an expression of intent AT A SCALE; they do not
   *  transfer. Adversarial pass, 2026-08-05. */
  const accumStepRef = useRef<number | null>(null)
  const lastWheelRef = useRef(0)
  const timerRef = useRef<number | null>(null)
  const edgeSaidRef = useRef(false) // band edge already reported for THIS burst
  /** Where the RIG actually is, in Hz: the live snapshot we seeded a burst from, or the target the
   *  coalescer last flushed. A step OFF the band plan is allowed only from here — see
   *  `clampToBand`, which is the whole of what keeps the runaway guard alive. */
  const committedHzRef = useRef<number | null>(null)

  // Hoisted out of the effect so the keyboard applier below shares the ONE flush — a second
  // copy is a second coalescer, which is the whole failure mode this hook exists to avoid.
  const flush = useCallback(() => {
    timerRef.current = null
    const t = targetHzRef.current
    if (t == null) return
    const { sideband, onSnap, enabled } = stateRef.current
    // Re-check the gate HERE, not only at event time. The flush lands up to FLUSH_MS after the
    // last notch, so a burst that ends just as the operator keys up would otherwise put a CAT
    // set_frequency out DURING a live over — and with a digit step that can be a band change,
    // which halts the over on the engine side.
    if (!enabled) {
      targetHzRef.current = null
      return
    }
    committedHzRef.current = Math.round(t)
    const mhz = Math.round(t) / 1e6
    // An EMPTY band label is an ANSWER, not a refusal: listening off the ham bands is first-class
    // (operator, 2026-08-13) — WWV, a shortwave broadcaster, the gap between two allocations — and
    // the engine routes a bandless dial without wiping the decode context. This used to drop the
    // target and report the edge instead, which is why the only way out of a band was the rig's
    // own knob. What still bounds a burst is `clampToBand`, not this.
    void setFrequency(mhz, bandLabelForMhz(mhz), sideband || 'USB')
      .then((s) => s && onSnap?.(s))
      .catch(() => {})
  }, [])

  /** Seed the optimistic target for a fresh burst (or one resumed after a pause) from the live
   *  dial, so any drift between where we aimed and where the rig actually landed self-corrects.
   *  ONE clock for both callers: `WheelEvent.timeStamp` and `performance.now()` are the same
   *  DOMHighResTimeStamp timeline in a browser but NOT under jsdom's fake timers, and a hook
   *  whose two entry points measure idleness against different clocks re-seeds mid-burst. */
  const seed = useCallback(() => {
    const now = performance.now()
    const idle = now - lastWheelRef.current > IDLE_RESEED_MS
    lastWheelRef.current = now
    if (targetHzRef.current == null || idle) {
      targetHzRef.current = Math.round(stateRef.current.dialMhz * 1e6)
      committedHzRef.current = targetHzRef.current // the live dial IS where the rig is
      accumRef.current = 0
      accumStepRef.current = null
      if (idle) edgeSaidRef.current = false
    }
  }, [])

  /** Clamp `hz` into the band the burst started in, so a fast spin cannot walk THROUGH the band
   *  plan. `flush()` alone was checking only the FINAL accumulated target, so 7 notches of a
   *  1 MHz digit inside one 120 ms window went out as ONE write from 7.074 to 14.074 — a 7 MHz
   *  cross-band QSY still carrying 40 m's LSB, where the same notches delivered slowly are each
   *  refused. Speed alone decided whether it happened.
   *
   *  The consequences are larger than the frequency: `Engine::tune_dial` sees a band change and
   *  runs `halt_tx_for_context_change`, `clear_decode_context`, `clear_stations`, `reset_ft8_a7`,
   *  and auto radio routing can hand the QSY to a DIFFERENT RIG — all from one wheel nudge.
   *
   *  Clamping rather than discarding also fixes the sibling defect: a flick that overshoots the
   *  edge used to throw the WHOLE burst away, so a quick three notches toward the top of the band
   *  moved the dial nowhere while the same three delivered slowly moved it to the edge.
   *  Returns the clamped Hz and whether the edge was hit.
   *
   *  ⚠️ THE EDGE IS A STOP, NOT A WALL (operator, 2026-08-13). Listening off the ham bands is
   *  first-class, so a notch taken from a dial that is REALLY on the edge — the snapshot we seeded
   *  from, or the Hz we just flushed — carries straight on off the plan. That is what makes WWV
   *  and everything between two allocations reachable with the wheel, and it costs the runaway
   *  guard nothing: inside one 120 ms window nothing is committed, so the seven-notch burst above
   *  still parks on 7.3 instead of landing on 14.074. Leaving the band takes a second gesture, at
   *  a dial the operator can see. */
  const clampToBand = useCallback((hz: number, fromHz: number): { hz: number; hitEdge: boolean } => {
    const range = bandRangeForLabel(bandLabelForMhz(fromHz / 1e6))
    if (!range) return { hz, hitEdge: false } // already off-plan (60 m channels, out-of-band RX)
    const lo = Math.round(range.lo * 1e6)
    const hi = Math.round(range.hi * 1e6)
    const edge = hz < lo ? lo : hz > hi ? hi : null
    if (edge == null) return { hz, hitEdge: false }
    if (fromHz === edge && fromHz === committedHzRef.current) return { hz, hitEdge: false }
    return { hz: edge, hitEdge: true }
  }, [])

  /** Say the edge ONCE per arrival at it, and only when the dial actually MOVED there.
   *
   * ⚠️ VALUE-BASED, NOT FLAG-BASED, and that is deliberate. `edgeSaidRef` alone is reset by
   * `seed()` after `IDLE_RESEED_MS` of wall-clock idleness — correctly, because a genuinely new
   * burst should be allowed to report again. But wall-clock idleness is not something a caller
   * leaning on the edge controls, so a flag-only guard reported twice for one continuous gesture
   * whenever real time slipped past 400 ms between notches (seen in test, and a slow machine mid
   * band-change would do it on the air). Once the dial IS at the edge, `from === hz` for every
   * further notch, so there is nothing to say and nothing to double-say. */
  const reportEdge = useCallback((hz: number, from: number, hitEdge: boolean) => {
    if (!hitEdge || hz === from) return // already parked on the edge — silence, not a repeat
    if (edgeSaidRef.current) return
    edgeSaidRef.current = true
    stateRef.current.onEdge?.(hz / 1e6)
  }, [])

  /** Apply a signed Hz delta through the SAME target and coalescer as the wheel — the keyboard's
   *  route in (the readout's ←/→ digit selection + ↑/↓ spin). Never give the keyboard a writer
   *  of its own: two optimistic targets on one dial is exactly the bug above. */
  const tuneBy = useCallback(
    (deltaHz: number) => {
      if (!stateRef.current.enabled || !deltaHz) return
      seed()
      const from = targetHzRef.current ?? 0
      const { hz, hitEdge } = clampToBand(from + deltaHz, from)
      targetHzRef.current = hz
      reportEdge(hz, from, hitEdge)
      if (timerRef.current == null) timerRef.current = window.setTimeout(flush, FLUSH_MS)
    },
    [flush, seed, clampToBand, reportEdge],
  )

  useEffect(() => {
    const el = ref.current
    if (!el) return

    const onWheel = (e: WheelEvent) => {
      const { enabled, stepHz, resolveStepHz } = stateRef.current
      if (!enabled) return
      // Direction from the dominant axis: Shift+wheel arrives as horizontal scroll on Chromium.
      const raw = Math.abs(e.deltaY) >= Math.abs(e.deltaX) ? e.deltaY : e.deltaX
      if (raw === 0) return
      // What is under the pointer decides the step. `null` = not a tuning target at all (the '.',
      // the MHz unit, the band chip, the wrapper's padding) — leave the event to the page.
      const step = resolveStepHz ? resolveStepHz(e.target) : stepHz
      if (step == null) return
      e.preventDefault()
      seed()
      // Normalize to pixel-equivalents so a trackpad's dozens of tiny events don't lurch the dial;
      // a line/page-mode mouse wheel maps ~one notch → one step. The notch/page multipliers stay on
      // the BASE PX_PER_STEP; sensitivity scales the per-step THRESHOLD, so every mouse type damps
      // consistently (a higher threshold = more scroll per step = less sensitive).
      const px = e.deltaMode === 1 ? raw * PX_PER_STEP : e.deltaMode === 2 ? raw * PX_PER_STEP * 8 : raw
      // Residue belongs to the decade that produced it — see `accumStepRef`.
      if (accumStepRef.current !== step) {
        accumRef.current = 0
        accumStepRef.current = step
      }
      accumRef.current += px
      const sens = Math.max(0.2, Math.min(4, stateRef.current.sensitivity ?? 1))
      const effPx = PX_PER_STEP / sens
      const rawSteps = Math.trunc(accumRef.current / effPx)
      if (rawSteps === 0) return // still accumulating toward one whole step
      // Consume all whole steps from the accumulator (no residue lurch), but APPLY at most the cap
      // so a single violent flick can't jump many steps; the capped-away steps are discarded.
      accumRef.current -= rawSteps * effPx
      const eff = step * (e.shiftKey ? 10 : 1)
      // Cap in Hz as well as in steps — see MAX_HZ_PER_EVENT. At most one whole step survives a
      // decade-sized step; uniform tuning never reaches the Hz cap, so it keeps all 8.
      const cap = Math.max(1, Math.min(MAX_STEPS_PER_EVENT, Math.floor(MAX_HZ_PER_EVENT / eff)))
      const steps = Math.max(-cap, Math.min(cap, rawSteps))
      // Scroll up (negative delta) tunes UP, so negate: -steps × step.
      const from = targetHzRef.current ?? 0
      const { hz, hitEdge } = clampToBand(from + -steps * eff, from)
      targetHzRef.current = hz
      reportEdge(hz, from, hitEdge)
      if (timerRef.current == null) timerRef.current = window.setTimeout(flush, FLUSH_MS)
    }

    el.addEventListener('wheel', onWheel, { passive: false })
    return () => {
      el.removeEventListener('wheel', onWheel)
      if (timerRef.current != null) window.clearTimeout(timerRef.current)
    }
  }, [ref, flush, seed, clampToBand, reportEdge])

  return tuneBy
}
