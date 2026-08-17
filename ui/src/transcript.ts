// Shared keyboard-mode transcript rendering: group decoded text into runs of
// quantized confidence so a live transcript renders a handful of spans, not one
// per character. Lifted verbatim from RttyCockpit (2026-08-17, Keyboard Modes
// Phase 1) so the PSK cockpit — and every keyboard mode after it — feeds the
// SAME renderer instead of re-deriving the field-hang fix; RttyCockpit
// re-exports these under its original names, so its tests and call sites are
// unchanged.

/** Hard ceiling on the spans a transcript renders, whatever the copy quality
 * (the decodeHistory MAX_HISTORY precedent: a live feed gets a named cap, not a
 * hope). See `confidenceRuns`. */
export const TRANSCRIPT_MAX_RUNS = 200

/** Group the decoded text into runs of quantized confidence so the transcript
 * renders a handful of spans, not one per character (the ring holds up to
 * ~4000 chars at a 500 ms poll). Low-confidence copy renders FAINT — the
 * demodulator's soft metric carried per character (RTTY's ATC margin, PSK's
 * phase-error margin). Missing confidence renders solid: never hide text we
 * decoded.
 *
 * PER CHARACTER FIRST, always: that is the honest fade and it is what every
 * transcript an operator actually reads produces. Only when the exact grouping
 * would exceed TRANSCRIPT_MAX_RUNS spans does it re-group over equal BLOCKS
 * scored by their MEAN confidence. Confidence is a continuous slicer margin, so
 * on marginal copy it crosses the 75/50/25 thresholds constantly and the exact
 * grouping ran to ~1000–1900 spans (one per character in the worst case) that
 * the front-draining ring rewrote WHOLE twice a second — the field hang. Clean
 * copy always collapsed to a single span, which is why it never showed on the
 * bench.
 *
 * The fallback is LOSSY about the fade and only about the fade — every decoded
 * character still prints, in order. Averaging can hide a short bad burst inside
 * an otherwise-good block (at a full 4000-char ring the block is 20 characters,
 * so a run of up to ~10 marginal characters can average back above a
 * threshold). That is the price of the cap, and it is charged ONLY on a
 * transcript already carrying more than TRANSCRIPT_MAX_RUNS quality changes —
 * copy so broken that per-character fading is noise anyway. Anything cleaner
 * keeps character-exact fidelity. */
export function confidenceRuns(
  text: string,
  conf: number[],
): { text: string; opacity: number }[] {
  const level = (c: number) => {
    if (c >= 75) return 1
    if (c >= 50) return 0.75
    if (c >= 25) return 0.5
    return 0.3
  }
  const n = text.length
  const group = (block: number) => {
    const runs: { text: string; opacity: number }[] = []
    for (let i = 0; i < n; i += block) {
      const end = Math.min(i + block, n)
      let sum = 0
      // A missing entry is full confidence — the decoder reported no doubt.
      for (let k = i; k < end; k++) sum += conf[k] ?? 100
      const op = level(sum / (end - i))
      const last = runs[runs.length - 1]
      if (last && last.opacity === op) last.text += text.slice(i, end)
      else runs.push({ text: text.slice(i, end), opacity: op })
    }
    return runs
  }
  // block = 1 is the exact per-character grouping (a one-character mean is the
  // character). Both passes are O(n) over a 4000-char ring and the caller
  // memoizes, so the second pass only runs on copy that already blew the cap.
  const exact = group(1)
  if (exact.length <= TRANSCRIPT_MAX_RUNS) return exact
  // ceil: the block count can never exceed TRANSCRIPT_MAX_RUNS.
  return group(Math.max(1, Math.ceil(n / TRANSCRIPT_MAX_RUNS)))
}
