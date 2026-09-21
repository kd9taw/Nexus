// Authorship in the RTTY transcript — our own keyed over beside the received copy.
//
// The fade grouping may average; authorship may not. A run that spans the boundary would
// show the far end's text as ours, which is a different order of wrong from being slightly
// wrong about how faint something is.
import { describe, it, expect } from 'vitest'
import { authoredRuns, confidenceRuns, TRANSCRIPT_MAX_RUNS } from './transcript'

const flags = (s: string) => [...s].map((c) => c === '1')

describe('authoredRuns', () => {
  it('never lets a run span the sent/received boundary', () => {
    // "CQ" received, "DE" sent, "K" received — all at full confidence, so the FADE would
    // happily merge the lot into one run. Only authorship keeps them apart.
    const text = 'CQDEK'
    const conf = [100, 100, 100, 100, 100]
    const runs = authoredRuns(text, conf, flags('00110'))
    expect(runs.map((r) => [r.text, r.tx])).toEqual([
      ['CQ', false],
      ['DE', true],
      ['K', false],
    ])
    // Control: without authorship the same input IS one run — so the split above is
    // authorship doing it, not the fade.
    expect(confidenceRuns(text, conf)).toHaveLength(1)
  })

  it('degrades to an all-received transcript when the station sends no flags', () => {
    // A station older than the field. It must read as received, never as ours.
    const runs = authoredRuns('HELLO', [100, 100, 100, 100, 100], undefined)
    expect(runs).toEqual([{ text: 'HELLO', opacity: 1, tx: false }])
    expect(authoredRuns('HELLO', [100, 100, 100, 100, 100], [])).toEqual(runs)
  })

  it('reads a short flag array as received rather than as ours', () => {
    // A truncated ring or a station mid-upgrade. Claiming we sent text we did not is the
    // failure this guards; the opposite is merely a missing highlight.
    const runs = authoredRuns('ABCD', [100, 100, 100, 100], flags('11'))
    expect(runs.map((r) => [r.text, r.tx])).toEqual([
      ['AB', true],
      ['CD', false],
    ])
  })

  it('keeps the fade INSIDE a segment, so both facts survive together', () => {
    // Received "AB" clean then "cd" marginal, then our own "EF". The fade splits the
    // received part; authorship splits ours off. Neither erases the other.
    const runs = authoredRuns('ABcdEF', [100, 100, 10, 10, 100, 100], flags('000011'))
    expect(runs.map((r) => [r.text, r.tx, r.opacity < 1])).toEqual([
      ['AB', false, false],
      ['cd', false, true],
      ['EF', true, false],
    ])
  })

  it('stays bounded on a transcript that blows the run cap', () => {
    // Alternating confidence forces the per-character grouping past its cap, which makes
    // the delegate fall back to block averaging. Authorship must still be exact.
    const n = TRANSCRIPT_MAX_RUNS * 6
    const text = 'x'.repeat(n)
    const conf = Array.from({ length: n }, (_, i) => (i % 2 ? 100 : 10))
    const tx = Array.from({ length: n }, (_, i) => i >= n / 2)
    const runs = authoredRuns(text, conf, tx)
    expect(runs.length).toBeLessThanOrEqual(TRANSCRIPT_MAX_RUNS * 2)
    // Every character still prints, in order, and the halves keep their authorship.
    expect(runs.map((r) => r.text).join('')).toBe(text)
    expect(runs.filter((r) => r.tx).map((r) => r.text).join('').length).toBe(n / 2)
  })
})
