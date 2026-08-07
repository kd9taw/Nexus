import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { isStdCall, packsBesideHash } from './txMessages'

// The TS half of the WSJT-X predicate differential. `txMessages.ts` duplicates `stdCall` for the
// Tx-message panel, and its doc comment says "keep this in lockstep with the Rust predicate" —
// which was true, and was kept true by hand-checking it twice in one day. This makes it a gate.
//
// WHY IT MATTERS THAT THE TWO AGREE, in the panel's own terms: `genStdMessages` picks the plain
// Type 1/2 forms or the hashed i3=4 forms off this predicate, and the engine picks the same
// thing off `message.rs::is_std_call`. When they disagree the panel shows a row the engine will
// never send, `snap.qso.txNow` stops matching any row, and the next-dot confirmation lands
// nowhere — the operator is reading a different QSO than the one on the air.
//
// SAME ORACLE, SAME CORPUS AS RUST. The fixture is measurements of the real Qt regexes with
// their real flags, written by `scripts/gen-wsjtx-callsign-oracle.mjs`. It is deliberately NOT
// copied into `ui/` — one artifact, one truth, and reading the Rust crate's fixture from here is
// what makes the lockstep structural instead of a ritual. No network: the fetch happened when a
// maintainer regenerated. Full rationale in `crates/tempo-core/tests/wsjtx_predicate_differential.rs`.

interface Oracle {
  inputs: string[]
  verdicts: string
  patterns: { standard_call_re: { pattern: string; flags: string } }
  provenance: { revision: string; sha256: Record<string, string> }
}

const fixturePath = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../../crates/tempo-core/tests/fixtures/wsjtx-callsign-oracle.json',
)
const oracle = JSON.parse(readFileSync(fixturePath, 'utf8')) as Oracle
const upstreamStdCall = (i: number) => oracle.verdicts[2 * i] === '1'

/** Exactly what `isStdCall` does to its input before testing it (`.trim()`, then `/i`). */
const normalise = (s: string) => s.trim().toUpperCase()

/**
 * Inputs where the TS predicate does not answer what upstream answers, for a reason that is NOT
 * normalisation. Identical to the Rust list, and that is the finding: Qt folds case through
 * Unicode (PCRE2 in UTF mode), while JS `/i` WITHOUT `/u` deliberately excludes non-ASCII →
 * ASCII folds and `to_ascii_uppercase` cannot do them either. Both halves of Nexus land on the
 * same answer for different reasons, and both route such a call down the hashed forms, which is
 * the safe direction. Unreachable from a decoded frame — the 77-bit alphabet is ASCII.
 */
const DECLARED_DIVERGENCES: Array<[string, boolean, string]> = [
  ['ſ9XYZ', true, 'U+017F LATIN SMALL LETTER LONG S folds to S under PCRE2 caseless'],
  ['K1ABC', true, 'U+212A KELVIN SIGN folds to K — leading position'],
  ['W9XYK', true, 'U+212A KELVIN SIGN folds to K — trailing position'],
]
const declared = (s: string) => DECLARED_DIVERGENCES.find((d) => d[0] === s)

/** Escape so a failure is debuggable — the divergences live in whitespace and lookalikes. */
const show = (s: string) =>
  JSON.stringify(s).replace(/[-￿]/g, (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, '0')}`)

describe('txMessages predicates vs the real WSJT-X regexes', () => {
  it('the oracle fixture is intact and carries its provenance', () => {
    expect(oracle.inputs.length).toBeGreaterThanOrEqual(20_000)
    expect(oracle.verdicts.length).toBe(oracle.inputs.length * 2)
    expect(oracle.provenance.revision).toMatch(/^[0-9a-f]{40}$/)
    expect(oracle.provenance.sha256['widgets/mainwindow.cpp']).toMatch(/^[0-9a-f]{64}$/)
    // The flags are the part a future reader drops, and without them the measurement means
    // something else: ExtendedPatternSyntaxOption is PCRE /x — remove it and upstream's
    // `( /R | /P )?` demands literal spaces and the pattern matches nothing at all.
    expect(oracle.patterns.standard_call_re.flags).toContain('CaseInsensitiveOption')
    expect(oracle.patterns.standard_call_re.flags).toContain('ExtendedPatternSyntaxOption')
  })

  // Claim 1 — transcription. On the normalised domain (every string a decoded 77-bit frame or a
  // validated callsign field can produce), isStdCall IS MainWindow::stdCall.
  it('isStdCall transcribes upstream stdCall on the normalised domain', () => {
    const wrong: string[] = []
    let checked = 0
    oracle.inputs.forEach((input, i) => {
      if (normalise(input) !== input) return
      checked++
      const ours = isStdCall(input)
      const theirs = upstreamStdCall(i)
      if (ours === theirs) return
      const d = declared(input)
      if (d) {
        expect(theirs, `declared divergence ${show(input)} (${d[2]})`).toBe(d[1])
        return
      }
      wrong.push(`  ${show(input)}  upstream stdCall=${theirs}  ts isStdCall=${ours}`)
    })
    expect(checked).toBeGreaterThan(5_000)
    expect(
      wrong,
      `isStdCall diverged from upstream on ${wrong.length} of ${checked} normalised inputs:\n${wrong.join('\n')}\n` +
        `Do not widen the predicate to match — add a declared divergence with its reason, and ` +
        `keep the Rust list in step.`,
    ).toEqual([])
  })

  // Claim 2 — normalisation. Off that domain, isStdCall answers for the trimmed/uppercased form.
  // Upstream has no such step; its pattern's own `\s*` is ASCII-only (Qt builds PCRE2 without
  // PCRE2_UCP) while JS `.trim()` is Unicode, so NBSP and U+2028 are where the two part company.
  // Pinning it here means dropping the `.trim()` fails with the input that proves it.
  it('off the normalised domain isStdCall answers for the normalised input', () => {
    const byInput = new Map(oracle.inputs.map((s, i) => [s, upstreamStdCall(i)]))
    const wrong: string[] = []
    let checked = 0
    for (const input of oracle.inputs) {
      const n = normalise(input)
      if (n === input || declared(n)) continue
      const theirs = byInput.get(n)
      if (theirs === undefined) continue // corpus did not measure the normalised form
      checked++
      if (isStdCall(input) !== theirs) {
        wrong.push(`  ${show(input)} → ${show(n)}  ts=${isStdCall(input)}  upstream(normalised)=${theirs}`)
      }
    }
    expect(checked).toBeGreaterThan(5_000)
    expect(wrong, `${wrong.length} inputs:\n${wrong.join('\n')}`).toEqual([])
  })

  it('every declared divergence still diverges', () => {
    const byInput = new Map(oracle.inputs.map((s, i) => [s, upstreamStdCall(i)]))
    for (const [input, upstream, why] of DECLARED_DIVERGENCES) {
      expect(byInput.has(input), `${show(input)} is not in the corpus (${why})`).toBe(true)
      expect(byInput.get(input), `upstream stdCall for ${show(input)} (${why})`).toBe(upstream)
      expect(
        isStdCall(input),
        `${show(input)} is declared a divergence but now agrees with upstream — delete its row (${why})`,
      ).not.toBe(upstream)
    }
  })

  // packsBesideHash is narrower than isStdCall ON PURPOSE and the whole corpus says so: `/P` and
  // `/R` are protocol-standard opposite another c28 call and refused only opposite a hash
  // (packjt77.f90:1183-1184 — a hashed token beside ANY slashed call drops to i3=4, which has a
  // slot for neither a grid nor a report). A "simplification" that made them the same predicate
  // silently costs a portable station its grid and its number on every hashed over.
  it('packsBesideHash is isStdCall minus every slash, across the corpus', () => {
    const wrong: string[] = []
    let refused = 0
    for (const input of oracle.inputs) {
      const expected = !input.trim().includes('/') && isStdCall(input)
      if (packsBesideHash(input) !== expected) wrong.push(show(input))
      if (isStdCall(input) && input.trim().includes('/')) refused++
    }
    expect(wrong.slice(0, 20)).toEqual([])
    // The refused set is exactly the /P and /R rovers — if it empties, the two predicates have
    // collapsed into one and the narrowing has stopped meaning anything.
    expect(refused).toBeGreaterThan(100)
    expect(packsBesideHash('F4CYH/P')).toBe(false)
    expect(isStdCall('F4CYH/P')).toBe(true)
  })
})
